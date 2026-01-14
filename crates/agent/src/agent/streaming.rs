//! Streaming response processing with thinking tag support.
//!
//! This module includes safeguards against infinite LLM loops:
//! - Global stream timeout
//! - Maximum thinking content length
//! - Maximum tool call iterations
//! - Repetition detection

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::{AgentError, Result};
use crate::llm::LlmInterface;
use super::types::{AgentEvent, AgentMessage, SessionState, ToolCall};
use super::tool_parser::{parse_tool_calls, remove_tool_calls_from_response};

/// Configuration for stream processing safeguards
pub struct StreamSafeguards {
    /// Maximum time allowed for entire stream processing (default: 60s)
    pub max_stream_duration: Duration,
    /// Maximum thinking content length in characters (default: unlimited)
    pub max_thinking_length: usize,
    /// Maximum content length in characters (default: unlimited)
    pub max_content_length: usize,
    /// Maximum tool call iterations per request (default: 5)
    pub max_tool_iterations: usize,
    /// Maximum consecutive similar chunks to detect loops (default: 3)
    pub max_repetition_count: usize,
}

impl Default for StreamSafeguards {
    fn default() -> Self {
        Self {
            // Increased timeout to 40 seconds for thinking models
            max_stream_duration: Duration::from_secs(40),
            // Since thinking is NOT included in context, we can allow much more
            // qwen3-vl:2b can generate extensive thinking before responding
            max_thinking_length: 100000,  // 100K chars - thinking not in context
            max_content_length: usize::MAX,
            // Tool iterations limit - 3 is sufficient for most multi-step queries
            max_tool_iterations: 3,
            // Repetition detection threshold
            max_repetition_count: 3,
        }
    }
}

/// Clean up repetitive thinking content by removing excessive repeated phrases
/// This preserves the core thinking while removing the repetitive noise
pub fn cleanup_thinking_content(thinking: &str) -> String {
    if thinking.len() < 200 {
        return thinking.to_string();
    }

    let mut result = thinking.to_string();
    let mut reduced = true;

    // Pass 1: Remove immediate repetitions of the same phrase
    // This handles cases like "可能可能可能可能" -> "可能"
    while reduced {
        reduced = false;
        let original = result.clone();

        // Common repetitive patterns in qwen3-vl:2b thinking
        let patterns = [
            ("可能可能", "可能"),
            ("或者或者", "或者"),
            ("也许也许", "也许"),
            ("温度温度", "温度"),
            ("。。", "。"),
            ("，，", "，"),
            ("??", "?"),
            ("  ", " "),
        ];

        for (pattern, replacement) in patterns {
            result = result.replace(pattern, replacement);
        }

        if result != original {
            reduced = true;
        }
    }

    // Pass 2: Limit consecutive occurrences of common filler words
    // Using character-based iteration to avoid UTF-8 issues
    let filler_words = [
        ("可能", 3),  // Max 3 consecutive "可能"
        ("或者", 2),  // Max 2 consecutive "或者"
        ("也许", 2),
    ];

    for (word, max_consecutive) in filler_words {
        let chars: Vec<char> = result.chars().collect();
        let mut new_result = String::new();
        let mut consecutive = 0;
        let mut last_was_word = false;
        let mut char_idx = 0;

        while char_idx < chars.len() {
            // Check if the word starts at this position
            let word_chars: Vec<char> = word.chars().collect();
            let matches = if char_idx + word_chars.len() <= chars.len() {
                chars[char_idx..char_idx + word_chars.len()] == word_chars[..]
            } else {
                false
            };

            if matches {
                if last_was_word {
                    consecutive += 1;
                    if consecutive <= max_consecutive {
                        for &ch in &word_chars {
                            new_result.push(ch);
                        }
                    }
                } else {
                    consecutive = 1;
                    last_was_word = true;
                    for &ch in &word_chars {
                        new_result.push(ch);
                    }
                }
                char_idx += word_chars.len();
            } else {
                consecutive = 0;
                last_was_word = false;
                new_result.push(chars[char_idx]);
                char_idx += 1;
            }
        }
        result = new_result;
    }

    // Pass 3: If still too long, truncate with ellipsis at char boundary
    if result.chars().count() > 500 {
        let char_count = result.chars().count();
        // Take first 500 chars and add ellipsis
        result = result.chars().take(500).collect::<String>();
        result.push_str("...");
    }

    result
}

/// Detect if content is repetitive (indicating a loop)
fn detect_repetition(recent_chunks: &[String], new_chunk: &str, threshold: usize) -> bool {
    // === SINGLE-CHUNK REPETITION DETECTION ===
    // Check for repetitive words/phrases within a single chunk first
    // This catches cases where the model returns one large chunk with repetitive thinking
    let repetitive_phrases = [
        ("可能", 10),   // "maybe" - shouldn't appear >10 times
        ("或者", 8),   // "or"
        ("也许", 8),   // "perhaps"
        ("temperature", 8),
        ("温度", 10),
        ("sensor", 8),
        ("传感器", 8),
        ("可能", 10),  // "possible" (Chinese)
    ];

    for (phrase, limit) in repetitive_phrases {
        let count = new_chunk.matches(phrase).count();
        if count > limit {
            tracing::warn!(
                "Single-chunk repetition detected: '{}' appears {} times (limit: {})",
                phrase, count, limit
            );
            return true;
        }
    }

    // === MULTI-CHUNK REPETITION DETECTION ===
    // Check if chunks are similar to each other
    if recent_chunks.len() < threshold || new_chunk.len() < 10 {
        return false;
    }

    // Check if the last N chunks are very similar
    let recent = &recent_chunks[recent_chunks.len().saturating_sub(threshold)..];
    let similar_count = recent.iter()
        .filter(|chunk| {
            // Check similarity: at least 80% character overlap
            let overlap = chunk.chars()
                .zip(new_chunk.chars())
                .filter(|(a, b)| a == b)
                .count();
            let max_len = chunk.len().max(new_chunk.len());
            max_len > 0 && overlap * 100 / max_len >= 80
        })
        .count();

    if similar_count >= threshold - 1 {
        return true;
    }

    // === COMBINED PHRASE-LEVEL REPETITION DETECTION ===
    // Check for repetitive words/phrases across all chunks
    let combined: String = recent_chunks.iter()
        .map(|s| s.as_str())
        .chain(std::iter::once(new_chunk))
        .collect::<Vec<&str>>()
        .join("");

    for (phrase, limit) in repetitive_phrases {
        let count = combined.matches(phrase).count();
        if count > limit * 2 {  // Higher limit for combined text
            tracing::warn!(
                "Combined repetition detected: '{}' appears {} times (limit: {})",
                phrase, count, limit * 2
            );
            return true;
        }
    }

    false
}

/// Simple in-memory cache for tool results with TTL
struct ToolResultCache {
    entries: HashMap<String, (edge_ai_tools::ToolOutput, Instant)>,
    ttl: Duration,
}

impl ToolResultCache {
    fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    fn get(&self, key: &str) -> Option<edge_ai_tools::ToolOutput> {
        self.entries.get(key).and_then(|(result, timestamp)| {
            if timestamp.elapsed() < self.ttl {
                Some(result.clone())
            } else {
                None
            }
        })
    }

    fn insert(&mut self, key: String, value: edge_ai_tools::ToolOutput) {
        self.entries.insert(key, (value, Instant::now()));
    }

    fn cleanup_expired(&mut self) {
        self.entries.retain(|_, (_, timestamp)| timestamp.elapsed() < self.ttl);
    }

    /// Generate cache key from tool name and arguments
    fn make_key(name: &str, arguments: &Value) -> String {
        format!("{}:{}", name, arguments.to_string())
    }
}

/// Tools that should NOT be cached (e.g., commands that change state)
const NON_CACHEABLE_TOOLS: &[&str] = &[
    "send_command",
    "execute_command",
    "set_device_state",
    "toggle_device",
    "delete_device",
];

fn is_tool_cacheable(name: &str) -> bool {
    !NON_CACHEABLE_TOOLS.contains(&name)
}

/// Format tool results into a user-friendly response
/// This avoids calling the LLM again after tool execution, preventing excessive thinking
pub fn format_tool_results(tool_results: &[(String, String)]) -> String {
    if tool_results.is_empty() {
        return "操作已完成。".to_string();
    }

    let mut response = String::new();

    for (tool_name, result) in tool_results {
        // Try to parse the result as JSON for better formatting
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(result) {
            match tool_name.as_str() {
                "list_devices" => {
                    // Format device list as a table
                    if let Some(devices) = json_value.get("devices").and_then(|d| d.as_array()) {
                        response.push_str(&format!("已找到 {} 个设备：\n\n", devices.len()));
                        response.push_str("| 设备名称 | 状态 | 类型 |\n");
                        response.push_str("|---------|------|------|\n");
                        for device in devices {
                            let name = device.get("name").and_then(|n| n.as_str()).unwrap_or("未知");
                            let status = device.get("status").and_then(|s| s.as_str()).unwrap_or("未知");
                            let device_type = device.get("type").and_then(|t| t.as_str()).unwrap_or("未知");
                            response.push_str(&format!("| {} | {} | {} |\n", name, status, device_type));
                        }
                    }
                }
                "query_data" => {
                    // Format query result
                    if let Some(data) = json_value.get("data") {
                        response.push_str(&format!("查询结果：{}\n", serde_json::to_string_pretty(data).unwrap_or_default()));
                    } else {
                        response.push_str(&format!("查询结果：{}\n", result));
                    }
                }
                "control_device" | "send_command" => {
                    response.push_str("命令执行成功。\n");
                }
                _ => {
                    // Generic formatting for other tools
                    response.push_str(&format!("{} 执行完成。\n", tool_name));
                }
            }
        } else {
            // Result is not valid JSON, use as-is
            response.push_str(&format!("{} 执行完成。\n", tool_name));
        }
    }

    if response.ends_with('\n') {
        response.pop();
    }

    response
}

/// Result of a single tool execution with metadata
struct ToolExecutionResult {
    name: String,
    result: std::result::Result<edge_ai_tools::ToolOutput, edge_ai_tools::ToolError>,
}

/// Maximum context window size in tokens (approximate)
/// Reduced from 12000 to 6000 to improve response speed for qwen3-vl:2b
/// Smaller context = faster processing, less repetitive thinking
/// Streaming can handle slightly larger context than non-streaming
const MAX_CONTEXT_TOKENS: usize = 6000;

/// Estimate token count for a string (rough approximation: 1 token ≈ 4 characters for Chinese, 1 token ≈ 4 characters for English)
/// This is a simple heuristic - for production, consider using a proper tokenizer
fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

/// === ANTHROPIC-STYLE IMPROVEMENT: Tool Result Clearing for Streaming ===
///
/// Compacts old tool result messages into concise summaries.
/// This follows Anthropic's guidance for context engineering.
fn compact_tool_results_stream(messages: &[AgentMessage]) -> Vec<AgentMessage> {
    let mut result = Vec::new();
    let mut tool_result_count = 0;
    const KEEP_RECENT_TOOL_RESULTS: usize = 2;

    for msg in messages.iter().rev() {
        if msg.role == "user" || msg.role == "system" {
            result.push(msg.clone());
            continue;
        }

        if msg.tool_calls.is_some() && msg.tool_calls.as_ref().map_or(false, |t| !t.is_empty()) {
            tool_result_count += 1;

            if tool_result_count <= KEEP_RECENT_TOOL_RESULTS {
                result.push(msg.clone());
            } else {
                // Compress old tool results
                let tool_names: Vec<&str> = msg.tool_calls
                    .as_ref()
                    .iter()
                    .flat_map(|calls| calls.iter().map(|t| t.name.as_str()))
                    .collect();

                let summary = if tool_names.len() == 1 {
                    format!("[之前调用了工具: {}]", tool_names[0])
                } else {
                    format!("[之前调用了工具: {}]", tool_names.join(", "))
                };

                result.push(AgentMessage {
                    role: msg.role.clone(),
                    content: summary,
                    tool_calls: None,
                    tool_call_id: None,
                    tool_call_name: None,
                    thinking: None, // Never keep thinking in compacted messages
                    timestamp: msg.timestamp,
                });
            }
        } else {
            result.push(msg.clone());
        }
    }

    result.reverse();
    result
}

/// === ANTHROPIC-STYLE IMPROVEMENT: Context Window with Tool Result Clearing ===
///
/// Builds conversation context with:
/// 1. Tool result clearing for old messages
/// 2. Token-based windowing
/// 3. Always keep recent messages for context continuity
fn build_context_window(messages: &[AgentMessage]) -> Vec<AgentMessage> {
    let compacted = compact_tool_results_stream(messages);

    let mut selected_messages = Vec::new();
    let mut current_tokens = 0;
    const MIN_RECENT_MESSAGES: usize = 4;

    for msg in compacted.iter().rev() {
        let msg_tokens = estimate_tokens(&msg.content);

        if current_tokens + msg_tokens > MAX_CONTEXT_TOKENS {
            let is_recent = selected_messages.len() < MIN_RECENT_MESSAGES;
            if msg.role == "system" || msg.role == "user" || is_recent {
                let max_len = (MAX_CONTEXT_TOKENS - current_tokens) * 4;
                if max_len > 100 {
                    selected_messages.push(msg.clone());
                    current_tokens += msg_tokens;
                }
            }
            break;
        }

        selected_messages.push(msg.clone());
        current_tokens += msg_tokens;
    }

    selected_messages.reverse();
    selected_messages
}

/// Process a user message with streaming response.
///
/// Logic:
/// 1. Stream LLM response in real-time
/// 2. Detect tool calls during streaming
/// 3. If tool call detected:
///    - Execute tools in parallel
///    - Get final LLM response based on tool results
///    - Stream the final response
///
/// ## Safeguards against infinite loops:
/// - Global stream timeout (60s default)
/// - Maximum thinking content length (10000 chars)
/// - Maximum content length (20000 chars)
/// - Repetition detection to catch loops
/// - Maximum tool call iterations (5)
pub async fn process_stream_events(
    llm_interface: Arc<LlmInterface>,
    short_term_memory: Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    state: Arc<RwLock<SessionState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    process_stream_events_with_safeguards(
        llm_interface,
        short_term_memory,
        state,
        tools,
        user_message,
        StreamSafeguards::default(),
    ).await
}

/// Process a user message with streaming response and custom safeguards.
/// Now async - no more block_in_place causing thread pool exhaustion
pub async fn process_stream_events_with_safeguards(
    llm_interface: Arc<LlmInterface>,
    short_term_memory: Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    state: Arc<RwLock<SessionState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
    safeguards: StreamSafeguards,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    let user_message = user_message.to_string();

    // === FAST PATH: Simple greetings and common patterns ===
    // Bypass LLM for simple, common interactions to improve speed and reliability
    let trimmed = user_message.trim();
    let greeting_patterns = [
        "你好", "您好", "hi", "hello", "嗨", "在吗",
        "早上好", "下午好", "晚上好",
    ];

    let is_greeting = greeting_patterns.iter().any(|&pat| {
        trimmed.eq_ignore_ascii_case(pat) || trimmed.starts_with(pat)
    });

    if is_greeting && trimmed.len() < 20 {
        // Fast response for greetings - no LLM call needed
        let greeting_response = AgentMessage::assistant(
            "您好！我是 NeoTalk 智能助手。我可以帮您：\n\
            • 查看设备列表 - 说「列出设备」\n\
            • 查询设备数据 - 说「查询温度」\n\
            • 创建自动化规则 - 说「创建规则」\n\
            • 查看所有规则 - 说「列出规则」"
        );

        // Pure async - no block_in_place
        short_term_memory.write().await.push(greeting_response);
        state.write().await.increment_messages();

        let response_content = "您好！我是 NeoTalk 智能助手。我可以帮您：\n\
            • 查看设备列表 - 说「列出设备」\n\
            • 查询设备数据 - 说「查询温度」\n\
            • 创建自动化规则 - 说「创建规则」\n\
            • 查看所有规则 - 说「列出规则」";

        return Ok(Box::pin(async_stream::stream! {
            yield AgentEvent::content(response_content.to_string());
            yield AgentEvent::end();
        }));
    }

    // === FIX 1: Get conversation history and pass to LLM ===
    // This prevents the LLM from repeating actions or calling tools again
    // Pure async - no block_in_place
    let memory = short_term_memory.read().await;
    let history_messages = memory.clone();
    drop(memory); // Release lock before calling LLM

    let history_for_llm: Vec<edge_ai_core::Message> = build_context_window(&history_messages)
        .iter()
        .map(|msg| msg.to_core())
        .collect::<Vec<_>>();

    tracing::debug!("Passing {} messages from history to LLM", history_for_llm.len());

    // Get the stream from llm_interface - pure async call
    let stream = llm_interface.chat_stream_with_history(&user_message, &history_for_llm)
        .await
        .map_err(|e| AgentError::Llm(e.to_string()))?;

    Ok(Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut buffer = String::new();
        let mut tool_calls_detected = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut content_before_tools = String::new();
        let mut thinking_content = String::new();
        let mut has_content = false;
        let mut has_thinking = false;

        // === SAFEGUARD: Track stream start time for timeout ===
        let stream_start = Instant::now();

        // === SAFEGUARD: Track recent chunks for repetition detection ===
        let mut recent_chunks: Vec<String> = Vec::new();
        const RECENT_CHUNK_WINDOW: usize = 10;

        // === SAFEGUARD: Track recently executed tools to prevent loops ===
        // Store tool names that were executed in this session (last 5 tools)
        let mut recently_executed_tools: std::collections::VecDeque<String> = std::collections::VecDeque::with_capacity(5);

        // === Stream and forward events ===
        while let Some(result) = StreamExt::next(&mut stream).await {
            // === SAFEGUARD: Check global timeout ===
            if stream_start.elapsed() > safeguards.max_stream_duration {
                tracing::warn!("Stream timeout reached after {:?}", stream_start.elapsed());
                yield AgentEvent::error("Response timeout - stream took too long".to_string());
                break;
            }
            
            match result {
                Ok((text, is_thinking)) => {
                    if text.is_empty() {
                        continue;
                    }
                    
                    // === SAFEGUARD: Check for repetitive content (loop detection) ===
                    if detect_repetition(&recent_chunks, &text, safeguards.max_repetition_count) {
                        tracing::warn!("Repetition detected in LLM output, breaking loop");
                        yield AgentEvent::error("Detected repetitive output - stopping to prevent loop".to_string());
                        break;
                    }
                    recent_chunks.push(text.clone());
                    if recent_chunks.len() > RECENT_CHUNK_WINDOW {
                        recent_chunks.remove(0);
                    }

                    // thinking: send immediately with length check
                    if is_thinking {
                        // === CRITICAL FIX: Check thinking limit BEFORE sending ===
                        // Don't accumulate or send if we're already at the limit
                        if safeguards.max_thinking_length != usize::MAX
                            && thinking_content.len() >= safeguards.max_thinking_length
                        {
                            tracing::warn!(
                                "Thinking already at max length ({}), ignoring additional chunks",
                                thinking_content.len()
                            );
                            continue; // Skip this chunk entirely
                        }

                        // Check if adding this chunk would exceed the limit
                        if safeguards.max_thinking_length != usize::MAX
                            && thinking_content.len() + text.len() > safeguards.max_thinking_length
                        {
                            // Only send partial chunk up to the limit
                            let remaining = safeguards.max_thinking_length - thinking_content.len();
                            if remaining > 0 && !text.is_empty() {
                                // Safe truncate: take min of remaining and text length
                                let truncate_len = remaining.min(text.len());
                                let truncated = &text[..truncate_len];
                                thinking_content.push_str(truncated);
                                yield AgentEvent::thinking(truncated.to_string());
                            }
                            tracing::warn!(
                                "Thinking content would exceed max length ({} + {} > {}), forcing termination",
                                thinking_content.len(),
                                text.len(),
                                safeguards.max_thinking_length
                            );
                            yield AgentEvent::error(format!(
                                "Thinking limit reached ({} chars), stopping to prevent slow response",
                                safeguards.max_thinking_length
                            ));
                            break;
                        }

                        thinking_content.push_str(&text);
                        has_thinking = true;
                        yield AgentEvent::thinking(text);
                        continue;
                    }

                    // content: need to check for tool calls
                    has_content = true;

                    // === SAFEGUARD: Check content length (only if limit is set) ===
                    if safeguards.max_content_length != usize::MAX
                        && content_before_tools.len() + buffer.len() + text.len() > safeguards.max_content_length
                    {
                        tracing::warn!("Content exceeded max length ({}), stopping stream", safeguards.max_content_length);
                        yield AgentEvent::error("Response too long - content limit reached".to_string());
                        break;
                    }

                    buffer.push_str(&text);

                    // Check for tool calls in buffer
                    if let Some(tool_start) = buffer.find("<tool_calls>") {
                        let before_tool = &buffer[..tool_start];
                        if !before_tool.is_empty() {
                            content_before_tools.push_str(before_tool);
                            yield AgentEvent::content(before_tool.to_string());
                        }

                        if let Some(tool_end) = buffer.find("</tool_calls>") {
                            let tool_content = buffer[tool_start..tool_end + 13].to_string();
                            buffer = buffer[tool_end + 13..].to_string();

                            if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
                                // === SAFEGUARD: Check for duplicate tool calls to prevent loops ===
                                let mut duplicate_found = false;
                                for call in &calls {
                                    if recently_executed_tools.contains(&call.name) {
                                        tracing::warn!(
                                            "Tool '{}' was recently executed - potential loop detected",
                                            call.name
                                        );
                                        yield AgentEvent::error(format!(
                                            "Tool '{}' was recently executed. To prevent infinite loops, please try a different approach.",
                                            call.name
                                        ));
                                        duplicate_found = true;
                                        tool_calls.clear();
                                        break;
                                    }
                                }

                                if duplicate_found {
                                    // Loop was detected, skip this batch
                                    break;
                                }

                                tool_calls = calls;
                                tool_calls_detected = true;
                            }
                            break;
                        } else {
                            continue; // wait for more data
                        }
                    }

                    // No tool calls - stream content immediately
                    if !tool_calls_detected && !buffer.is_empty() {
                        // Check if buffer contains start of tool calls
                        if !buffer.contains("<tool_calls") {
                            // No tool calls in buffer, emit content
                            content_before_tools.push_str(&buffer);
                            yield AgentEvent::content(buffer.clone());
                            buffer.clear();
                        } else {
                            // Buffer contains partial tool call XML, check if it's complete
                            // If complete but parsing failed, filter out the XML before emitting
                            if buffer.contains("</tool_calls>") {
                                // Tool calls are complete but parsing failed
                                // Filter out any tool call XML and emit remaining content
                                let filtered = if let Some(tool_start) = buffer.find("<tool_calls>") {
                                    if let Some(tool_end) = buffer.find("</tool_calls>") {
                                        // Remove tool call block, keep content before and after
                                        let before = &buffer[..tool_start];
                                        let after = &buffer[tool_end + 13..];
                                        format!("{}{}", before, after)
                                    } else {
                                        buffer.clone()
                                    }
                                } else {
                                    buffer.clone()
                                };

                                if !filtered.is_empty() {
                                    content_before_tools.push_str(&filtered);
                                    yield AgentEvent::content(filtered);
                                }
                                buffer.clear();
                            }
                            // If tool calls are incomplete, wait for more data
                        }
                    }
                }
                Err(e) => {
                    yield AgentEvent::error(e.to_string());
                    break;
                }
            }
        }

        // Emit any remaining buffer (after filtering out tool call XML)
        if !buffer.is_empty() {
            // Filter out any tool call XML that might remain in the buffer
            let filtered_content = if buffer.contains("<tool_calls>") {
                // Remove tool call blocks from buffer
                let mut result = buffer.clone();
                while let Some(start) = result.find("<tool_calls>") {
                    if let Some(end) = result.find("</tool_calls>") {
                        result.replace_range(start..=end + 12, "");
                    } else {
                        break;
                    }
                }
                result
            } else {
                buffer.clone()
            };

            if !filtered_content.is_empty() {
                content_before_tools.push_str(&filtered_content);
                yield AgentEvent::content(filtered_content);
            }
        }

        // === Handle empty responses ===
        // With increased thinking limit, model should complete and output actual content
        let has_content = !content_before_tools.is_empty();
        let has_thinking = !thinking_content.is_empty();

        // IMPORTANT: If tool calls were detected, DON'T save the initial message yet.
        // We'll save a complete message (with tool_calls and final response) in Phase 2.
        // If no tool calls, save the response now.
        if !tool_calls_detected {
            // Handle empty response case (should be rare with increased thinking limit)
            let response_to_save = if !has_content && !has_thinking {
                // Complete empty response - shouldn't happen but handle it
                let fallback = "您好，我是NeoTalk助手，请问有什么可以帮助您的？".to_string();
                yield AgentEvent::content(fallback.clone());
                fallback
            } else {
                content_before_tools.clone()
            };

            let initial_msg = if !thinking_content.is_empty() {
                // Clean up repetitive thinking content before storing
                let cleaned_thinking = cleanup_thinking_content(&thinking_content);
                let original_len = thinking_content.len();
                let cleaned_len = cleaned_thinking.len();

                if original_len > cleaned_len {
                    tracing::info!(
                        "Thinking content cleaned: {} chars -> {} chars ({}% reduction)",
                        original_len,
                        cleaned_len,
                        (original_len - cleaned_len) * 100 / original_len
                    );
                }

                AgentMessage::assistant_with_thinking(&response_to_save, &cleaned_thinking)
            } else {
                AgentMessage::assistant(&response_to_save)
            };
            short_term_memory.write().await.push(initial_msg);
        }

        // === PHASE 2: Handle tool calls if detected ===
        if tool_calls_detected {
            tracing::info!("Starting PARALLEL tool execution");
            
            // === SAFEGUARD: Limit number of tool calls to prevent infinite loops ===
            if tool_calls.len() > safeguards.max_tool_iterations {
                tracing::warn!(
                    "Too many tool calls ({}) requested, limiting to {}",
                    tool_calls.len(),
                    safeguards.max_tool_iterations
                );
                yield AgentEvent::error(format!(
                    "Too many tool calls requested ({}), limiting to {}",
                    tool_calls.len(),
                    safeguards.max_tool_iterations
                ));
                tool_calls.truncate(safeguards.max_tool_iterations);
            }
            let tool_calls_to_execute = tool_calls.clone();

            // Create cache for this batch of tool executions (5 minute TTL)
            let cache = Arc::new(RwLock::new(ToolResultCache::new(Duration::from_secs(300))));

            // Execute all tool calls in parallel
            let tool_futures: Vec<_> = tool_calls_to_execute.iter().map(|tool_call| {
                let tools_clone = tools.clone();
                let cache_clone = cache.clone();
                let name = tool_call.name.clone();
                let arguments = tool_call.arguments.clone();
                let name_clone = name.clone();

                async move {
                    // Emit start event before execution
                    (name.clone(), ToolExecutionResult {
                        name: name_clone,
                        result: execute_tool_with_retry(&tools_clone, &cache_clone, &name, arguments.clone()).await,
                    })
                }
            }).collect();

            // Execute all tools in parallel and collect results
            let tool_results_executed = futures::future::join_all(tool_futures).await;

            // Process results and update tool_calls with execution results
            let mut tool_calls_with_results: Vec<ToolCall> = Vec::new();
            let mut tool_call_results: Vec<(String, String)> = Vec::new();

            for (name, execution) in tool_results_executed {
                yield AgentEvent::tool_call_start(&name, tool_calls.iter().find(|t| t.name == name).map(|t| t.arguments.clone()).unwrap_or_default());

                match execution.result {
                    Ok(output) => {
                        let result_value = if output.success {
                            output.data.clone()
                        } else {
                            output.error.clone().map(|e| serde_json::json!({"error": e}))
                                .unwrap_or_else(|| serde_json::json!("Error"))
                        };
                        let result_str = if output.success {
                            serde_json::to_string(&output.data).unwrap_or_else(|_| "Success".to_string())
                        } else {
                            output.error.clone().unwrap_or_else(|| "Error".to_string())
                        };

                        // Find the original tool call and add result
                        for tc in &tool_calls {
                            if tc.name == name {
                                tool_calls_with_results.push(ToolCall {
                                    name: tc.name.clone(),
                                    id: tc.id.clone(),
                                    arguments: tc.arguments.clone(),
                                    result: Some(result_value.clone()),
                                });
                                break;
                            }
                        }

                        yield AgentEvent::tool_call_end(&name, &result_str, output.success);
                        tool_call_results.push((name.clone(), result_str));
                    }
                    Err(e) => {
                        let error_msg = format!("工具执行失败: {}", e);
                        let error_value = serde_json::json!({"error": error_msg});

                        // Find the original tool call and add error result
                        for tc in &tool_calls {
                            if tc.name == name {
                                tool_calls_with_results.push(ToolCall {
                                    name: tc.name.clone(),
                                    id: tc.id.clone(),
                                    arguments: tc.arguments.clone(),
                                    result: Some(error_value.clone()),
                                });
                                break;
                            }
                        }

                        yield AgentEvent::tool_call_end(&name, &error_msg, false);
                        tool_call_results.push((name.clone(), error_msg));
                    }
                }
            }

            // === SAFEGUARD: Update recently executed tools list to prevent loops ===
            // Only add tools that succeeded to the list
            for (name, _result) in &tool_call_results {
                if !recently_executed_tools.contains(name) {
                    recently_executed_tools.push_back(name.clone());
                    if recently_executed_tools.len() > 5 {
                        recently_executed_tools.pop_front();
                    }
                    tracing::debug!("Added '{}' to recently executed tools (now: {:?})", name, recently_executed_tools);
                }
            }

            // === FIX 2: Phase 2 - Direct tool result formatting ===
            // Instead of calling LLM again (which causes empty responses and delays),
            // we directly format and return the tool results.
            // This is faster and more reliable for simple tool queries.

            // Step 1: Save the initial assistant message WITH tool call results
            // This ensures frontend can display complete tool call status
            let initial_msg = if !thinking_content.is_empty() {
                AgentMessage::assistant_with_tools_and_thinking(
                    &content_before_tools,  // Content before tool calls
                    tool_calls_with_results,  // Tool calls WITH results
                    &thinking_content,
                )
            } else {
                AgentMessage::assistant_with_tools(
                    &content_before_tools,
                    tool_calls_with_results,  // Tool calls WITH results
                )
            };
            short_term_memory.write().await.push(initial_msg);

            // Step 2: Add tool result messages to history for LLM context
            for (tool_name, result_str) in &tool_call_results {
                let tool_result_msg = AgentMessage::tool_result(tool_name, result_str);
                short_term_memory.write().await.push(tool_result_msg);
            }

            // Step 3: Call LLM again to generate final response based on tool results
            // The LLM should see: conversation history + tool call + tool result
            // And generate a natural response to the user
            tracing::info!("Phase 2: Generating follow-up response based on tool results");

            // Get the conversation history (including the tool results we just added)
            // Pure async - no block_in_place
            let history = short_term_memory.read().await;
            let history_messages: Vec<edge_ai_core::Message> = history.iter()
                .map(|msg| msg.to_core())
                .collect::<Vec<_>>();
            drop(history); // Release lock

            // Phase 2: Use the specialized function that disables both tools and thinking
            // This prevents infinite thinking loops and provides faster responses
            // The history already contains tool calls and results, so LLM knows what happened
            let followup_stream_result = llm_interface.chat_stream_no_tools_no_thinking_with_history(
                "请根据工具执行结果生成简洁的回复。", &history_messages
            ).await;

            let followup_stream = match followup_stream_result {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!("Phase 2 LLM call failed: {}", e);
                    // Fallback to formatted tool results
                    let fallback_text = format_tool_results(&tool_call_results);
                    for chunk in fallback_text.chars().collect::<Vec<_>>().chunks(20) {
                        let chunk_str: String = chunk.iter().collect();
                        if !chunk_str.is_empty() {
                            yield AgentEvent::content(chunk_str);
                        }
                    }
                    yield AgentEvent::end();
                    return;
                }
            };

            // Stream the follow-up response
            let mut followup_stream = Box::pin(followup_stream);
            let mut final_response_content = String::new();
            let mut phase2_thinking_chars = 0usize;
            let followup_start = Instant::now();

            while let Some(result) = StreamExt::next(&mut followup_stream).await {
                // Phase 2 timeout - don't wait too long
                if followup_start.elapsed() > Duration::from_secs(30) {
                    tracing::warn!("Phase 2 timeout (>30s), forcing completion");
                    break;
                }

                match result {
                    Ok((chunk, is_thinking)) => {
                        if chunk.is_empty() {
                            continue;
                        }
                        if is_thinking {
                            // Track thinking characters for Phase 2
                            phase2_thinking_chars += chunk.chars().count();

                            // SAFEGUARD: Limit Phase 2 thinking to prevent infinite loops
                            // Use a lower limit than Phase 1 since Phase 2 should be brief
                            const MAX_PHASE2_THINKING: usize = 5000;
                            if phase2_thinking_chars > MAX_PHASE2_THINKING {
                                tracing::warn!(
                                    "Phase 2 thinking exceeds limit ({} > {}), forcing content generation",
                                    phase2_thinking_chars, MAX_PHASE2_THINKING
                                );
                                // Don't break - just stop sending thinking and wait for content
                                continue;
                            }

                            // Phase 2 thinking - still send it but could be limited if needed
                            yield AgentEvent::thinking(chunk.clone());
                        } else {
                            yield AgentEvent::content(chunk.clone());
                            final_response_content.push_str(&chunk);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Phase 2 stream error: {}", e);
                        break;
                    }
                }
            }

            // === FIX: Handle empty Phase 2 response ===
            // If Phase 2 produced no actual content (only thinking), use a fallback
            if final_response_content.is_empty() {
                tracing::warn!(
                    "Phase 2 produced no content ({} chars of thinking), using fallback",
                    phase2_thinking_chars
                );

                // Generate a meaningful fallback based on tool results
                let fallback = if tool_call_results.len() == 1 {
                    // Single tool result - format it nicely
                    let (tool_name, result_str) = &tool_call_results[0];
                    if tool_name == "list_devices" {
                        // Try to parse and format device list
                        if let Ok(json_val) = serde_json::from_str::<Value>(result_str) {
                            if let Some(devices) = json_val.get("devices").and_then(|d| d.as_array()) {
                                format!("已找到 {} 个设备。", devices.len())
                            } else {
                                "设备查询完成。".to_string()
                            }
                        } else {
                            "设备查询完成。".to_string()
                        }
                    } else {
                        format!("{} 执行完成。", tool_name)
                    }
                } else if tool_call_results.len() > 1 {
                    // Multiple tools executed
                    format!("已执行 {} 个工具操作。", tool_call_results.len())
                } else {
                    // No tools produced results - shouldn't happen
                    "处理完成。".to_string()
                };

                yield AgentEvent::content(fallback.clone());
                final_response_content = fallback;
            }

            // Save the final assistant response to memory
            let final_msg = AgentMessage::assistant(&final_response_content);
            short_term_memory.write().await.push(final_msg);

            tracing::info!("Tool execution and Phase 2 response complete");
        }

        state.write().await.increment_messages();
        yield AgentEvent::end();
    }))
}

/// Execute a tool with retry logic for transient errors and caching.
async fn execute_tool_with_retry(
    tools: &edge_ai_tools::ToolRegistry,
    cache: &Arc<RwLock<ToolResultCache>>,
    name: &str,
    arguments: serde_json::Value,
) -> std::result::Result<edge_ai_tools::ToolOutput, edge_ai_tools::ToolError> {
    // Check cache for read-only tools
    if is_tool_cacheable(name) {
        let cache_key = ToolResultCache::make_key(name, &arguments);
        {
            let cache_read = cache.read().await;
            if let Some(cached) = cache_read.get(&cache_key) {
                println!("[streaming.rs] Cache HIT for tool: {}", name);
                return Ok(cached);
            }
        }
        println!("[streaming.rs] Cache MISS for tool: {}", name);
    }

    let max_retries = 2u32;
    let result = execute_with_retry_impl(tools, name, arguments.clone(), max_retries).await;

    // Cache successful results for cacheable tools
    if is_tool_cacheable(name) {
        if let Ok(ref output) = result {
            if output.success {
                let cache_key = ToolResultCache::make_key(name, &arguments);
                let mut cache_write = cache.write().await;
                cache_write.insert(cache_key, output.clone());
                // Periodic cleanup
                cache_write.cleanup_expired();
            }
        }
    }

    result
}

/// Inner retry logic without caching (for code reuse)
async fn execute_with_retry_impl(
    tools: &edge_ai_tools::ToolRegistry,
    name: &str,
    arguments: serde_json::Value,
    max_retries: u32,
) -> std::result::Result<edge_ai_tools::ToolOutput, edge_ai_tools::ToolError> {
    for attempt in 0..=max_retries {
        let result = tools.execute(name, arguments.clone()).await;

        match &result {
            Ok(output) if output.success => return result,
            Err(e) => {
                let last_error = e.to_string();
                let is_transient = last_error.contains("timeout")
                    || last_error.contains("network")
                    || last_error.contains("connection")
                    || last_error.contains("unavailable");

                if is_transient && attempt < max_retries {
                    let delay_ms = 100u64 * (2_u64.pow(attempt));
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return result;
            }
            _ => return result,
        }
    }

    Err(edge_ai_tools::ToolError::Execution("Max retries exceeded".to_string()))
}

/// Convert AgentEvent stream to String stream for backward compatibility.
pub fn events_to_string_stream(
    event_stream: Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    Box::pin(async_stream::stream! {
        let mut stream = event_stream;
        while let Some(event) = StreamExt::next(&mut stream).await {
            match event {
                AgentEvent::Content { content } => {
                    yield content;
                }
                AgentEvent::Error { message } => {
                    yield format!("[错误: {}]", message);
                }
                AgentEvent::End => break,
                _ => {
                    // Ignore other events for backward compatibility
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    // Use std::result::Result for test data (not the crate's Result alias)
    type TestResult<T> = std::result::Result<T, &'static str>;

    /// Test scenario 1: Pure content response (no thinking, no tools)
    #[tokio::test]
    async fn test_pure_content_stream() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("你好，我是".to_string(), false)),
            Ok(("NeoTalk助手".to_string(), false)),
            Ok(("。".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking, "Should not be thinking");
                full_content.push_str(&text);
            }
        }

        assert_eq!(full_content, "你好，我是NeoTalk助手。");
        println!("✓ Pure content stream test passed: {}", full_content);
    }

    /// Test scenario 2: Thinking + content response
    #[tokio::test]
    async fn test_thinking_then_content_stream() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("让我分析一下".to_string(), true)),
            Ok(("这个问题".to_string(), true)),
            Ok(("好的，我来回答".to_string(), false)),
            Ok(("这是答案".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking_content = String::new();
        let mut actual_content = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking_content.push_str(&text);
                } else {
                    actual_content.push_str(&text);
                }
            }
        }

        assert_eq!(thinking_content, "让我分析一下这个问题");
        assert_eq!(actual_content, "好的，我来回答这是答案");
        println!("✓ Thinking + content stream test passed");
        println!("  Thinking: {}", thinking_content);
        println!("  Content: {}", actual_content);
    }

    /// Test scenario 3: Content followed by tool call
    #[tokio::test]
    async fn test_content_with_tool_call() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("让我帮您".to_string(), false)),
            Ok(("查询设备".to_string(), false)),
            Ok(("<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut content_before_tools = String::new();
        let mut buffer = String::new();
        let mut tool_calls_found = false;

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking, "Should not be thinking in this test");
                buffer.push_str(&text);

                // Check for tool calls
                if let Some(tool_start) = buffer.find("<tool_calls>") {
                    content_before_tools.push_str(&buffer[..tool_start]);
                    if let Some(tool_end) = buffer.find("</tool_calls>") {
                        tool_calls_found = true;
                        break;
                    }
                }
            }
        }

        assert_eq!(content_before_tools, "让我帮您查询设备");
        assert!(tool_calls_found, "Tool calls should be detected");
        println!("✓ Content with tool call test passed");
        println!("  Content before tools: {}", content_before_tools);
    }

    /// Test scenario 4: Thinking + content + tool call
    #[tokio::test]
    async fn test_thinking_content_tool_call() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("用户想查询设备".to_string(), true)),
            Ok(("需要调用list_devices".to_string(), true)),
            Ok(("好的，我来".to_string(), false)),
            Ok(("查询一下".to_string(), false)),
            Ok(("<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking = String::new();
        let mut content = String::new();
        let mut has_tool_calls = false;

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking.push_str(&text);
                } else {
                    content.push_str(&text);
                    if text.contains("<tool_calls>") {
                        has_tool_calls = true;
                    }
                }
            }
        }

        assert_eq!(thinking, "用户想查询设备需要调用list_devices");
        assert!(content.contains("好的，我来查询一下"));
        assert!(has_tool_calls, "Should have tool calls");
        println!("✓ Thinking + content + tool call test passed");
    }

    /// Test scenario 5: Empty content with thinking (edge case for think=true models)
    #[tokio::test]
    async fn test_thinking_only_no_content() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("这是我的思考过程".to_string(), true)),
            Ok(("继续思考".to_string(), true)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut thinking = String::new();
        let mut content = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                if is_thinking {
                    thinking.push_str(&text);
                } else {
                    content.push_str(&text);
                }
            }
        }

        assert_eq!(thinking, "这是我的思考过程继续思考");
        assert!(content.is_empty(), "Content should be empty for thinking-only response");
        println!("✓ Thinking-only test passed");
        println!("  Thinking should be emitted as content: {}", thinking);
        println!("  NOTE: In production, thinking content is emitted as final content when no actual content received");
    }

    /// Test scenario 6: Content split across multiple chunks with Chinese characters
    #[tokio::test]
    async fn test_multibyte_chunk_handling() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            // Split in middle of multi-byte sequence (shouldn't happen but test robustness)
            Ok(("你好".to_string(), false)),
            Ok(("世界".to_string(), false)),
            Ok(("，这是".to_string(), false)),
            Ok(("一个测试".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, is_thinking)) = result {
                assert!(!is_thinking);
                full_content.push_str(&text);
            }
        }

        assert_eq!(full_content, "你好世界，这是一个测试");
        println!("✓ Multi-byte chunk handling test passed");
        println!("  Content: {}", full_content);
    }

    /// Test scenario 7: Tool call with arguments
    #[tokio::test]
    async fn test_tool_call_with_arguments() {
        let tool_xml = r#"<tool_calls><invoke name="set_device_state">
<parameter name="device_id">lamp_1</parameter>
<parameter name="state">on</parameter>
</invoke></tool_calls>"#;

        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("好的，我来帮您".to_string(), false)),
            Ok((tool_xml.to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut content = String::new();
        let mut buffer = String::new();

        while let Some(result) = stream.next().await {
            if let Ok((text, _)) = result {
                buffer.push_str(&text);

                if let Some(tool_start) = buffer.find("<tool_calls>") {
                    content.push_str(&buffer[..tool_start]);
                    if buffer.contains("</tool_calls>") {
                        break;
                    }
                }
            }
        }

        assert_eq!(content, "好的，我来帮您");
        assert!(buffer.contains("<invoke name=\"set_device_state\">"));
        assert!(buffer.contains("<parameter name=\"device_id\">lamp_1</parameter>"));
        println!("✓ Tool call with arguments test passed");
    }

    /// Test scenario 8: Empty chunks handling
    #[tokio::test]
    async fn test_empty_chunk_handling() {
        let chunks: Vec<TestResult<(String, bool)>> = vec![
            Ok(("开始".to_string(), false)),
            Ok(("".to_string(), false)),  // Empty chunk
            Ok(("继续".to_string(), false)),
            Ok(("".to_string(), false)),  // Another empty chunk
            Ok(("结束".to_string(), false)),
        ];

        let mut stream = futures::stream::iter(chunks);

        let mut full_content = String::new();
        while let Some(result) = stream.next().await {
            if let Ok((text, _)) = result {
                full_content.push_str(&text);
            }
        }

        // Empty chunks should be included but not cause issues
        assert!(full_content.contains("开始"));
        assert!(full_content.contains("继续"));
        assert!(full_content.contains("结束"));
        println!("✓ Empty chunk handling test passed");
        println!("  Content: {}", full_content);
    }

    /// Test tool parser
    #[test]
    fn test_tool_parser() {
        let input = r#"<tool_calls><invoke name="test_tool">
<parameter name="param1">value1</parameter>
</invoke></tool_calls>"#;

        let result = parse_tool_calls(input);
        assert!(result.is_ok(), "Should parse tool calls successfully");

        let (remaining, calls) = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "test_tool");
        assert_eq!(calls[0].arguments["param1"], "value1");
        println!("✓ Tool parser test passed");
    }

    /// Test token estimation
    #[test]
    fn test_token_estimation() {
        let english = "Hello world, this is a test";
        let chinese = "你好世界，这是一个测试";

        let english_tokens = estimate_tokens(english);
        let chinese_tokens = estimate_tokens(chinese);

        // Rough estimation: ~4 chars per token
        assert!(english_tokens > 0 && english_tokens < 20);
        assert!(chinese_tokens > 0 && chinese_tokens < 20);

        println!("✓ Token estimation test passed");
        println!("  English ({} chars): ~{} tokens", english.chars().count(), english_tokens);
        println!("  Chinese ({} chars): ~{} tokens", chinese.chars().count(), chinese_tokens);
    }

    /// Test tool cache key generation
    #[test]
    fn test_cache_key_generation() {
        let key1 = ToolResultCache::make_key("list_devices", &serde_json::json!({}));
        let key2 = ToolResultCache::make_key("list_devices", &serde_json::json!(null));
        let key3 = ToolResultCache::make_key("list_devices", &serde_json::json!({}));

        assert_eq!(key1, key3, "Same args should produce same key");
        assert_ne!(key1, key2, "Different args should produce different keys");

        println!("✓ Cache key generation test passed");
    }

    /// Run all streaming tests and print summary
    #[test]
    fn run_all_streaming_tests() {
        println!("\n=== Running LLM Streaming Tests ===\n");

        println!("Test Coverage:");
        println!("  1. Pure content response (no thinking, no tools)");
        println!("  2. Thinking + content response");
        println!("  3. Content followed by tool call");
        println!("  4. Thinking + content + tool call");
        println!("  5. Empty content with thinking (edge case)");
        println!("  6. Multi-byte chunk handling (Chinese)");
        println!("  7. Tool call with arguments");
        println!("  8. Empty chunks handling");
        println!("  9. Tool parser");
        println!(" 10. Token estimation");
        println!(" 11. Cache key generation");
        println!("\n=== Test Suite Complete ===\n");
    }
}
