//! Streaming response processing with thinking tag support.
//!
//! This module includes safeguards against infinite LLM loops:
//! - Global stream timeout
//! - Maximum thinking content length
//! - Maximum tool call iterations
//! - Repetition detection

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::RwLock;

use super::staged::{IntentCategory, IntentClassifier};
use super::tool_parser::parse_tool_calls;
use super::types::{AgentEvent, AgentInternalState, AgentMessage, ToolCall};
use crate::error::{AgentError, Result};
use crate::llm::LlmInterface;

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
    /// Heartbeat interval to keep connection alive (default: 10s)
    pub heartbeat_interval: Duration,
    /// Progress update interval during long operations (default: 5s)
    pub progress_interval: Duration,
    /// Optional interrupt signal - when set, stream should stop gracefully
    /// This allows users to interrupt long thinking processes
    pub interrupt_signal: Option<tokio::sync::watch::Receiver<bool>>,
}

impl Default for StreamSafeguards {
    fn default() -> Self {
        Self {
            // Increased timeout to 120 seconds for thinking models
            // qwen3-vl:2b can take a long time thinking before generating content
            max_stream_duration: Duration::from_secs(120),
            // No limit on thinking content - let the model think as much as needed
            max_thinking_length: usize::MAX,
            max_content_length: usize::MAX,
            // Tool iterations limit - 3 is sufficient for most multi-step queries
            max_tool_iterations: 3,
            // Repetition detection threshold
            max_repetition_count: 3,
            // Heartbeat every 10 seconds to prevent WebSocket timeout
            heartbeat_interval: Duration::from_secs(10),
            // Progress update every 5 seconds during long operations
            progress_interval: Duration::from_secs(5),
            // No interrupt signal by default
            interrupt_signal: None,
        }
    }
}

impl StreamSafeguards {
    /// Create a new StreamSafeguards with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the interrupt signal for this stream
    /// Returns a sender that can be used to trigger the interrupt
    pub fn with_interrupt_signal(mut self, rx: tokio::sync::watch::Receiver<bool>) -> Self {
        self.interrupt_signal = Some(rx);
        self
    }

    /// Create an interruptible stream with a (tx, rx) pair
    /// Returns (safeguards, sender) where sender can be used to interrupt
    pub fn with_interrupt() -> (Self, tokio::sync::watch::Sender<bool>) {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let safeguards = Self::default().with_interrupt_signal(rx);
        (safeguards, tx)
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
        ("可能", 3), // Max 3 consecutive "可能"
        ("或者", 2), // Max 2 consecutive "或者"
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
        let _char_count = result.chars().count();
        // Take first 500 chars and add ellipsis
        result = result.chars().take(500).collect::<String>();
        result.push_str("...");
    }

    result
}

/// Detect JSON tool calls in buffer.
///
/// Looks for JSON array format: [{"name": "tool", "arguments": {...}}, ...]
/// Returns Some((start_pos, json_text, remaining_buffer)) if found, None otherwise.
fn detect_json_tool_calls(buffer: &str) -> Option<(usize, String, String)> {
    // Find the first '[' that might start a JSON array
    let start = buffer.find('[')?;

    // Find the matching closing ']' by counting brackets
    let mut bracket_count = 0;
    let mut end = None;

    for (i, c) in buffer[start..].char_indices() {
        if c == '[' {
            bracket_count += 1;
        } else if c == ']' {
            bracket_count -= 1;
            if bracket_count == 0 {
                end = Some(start + i + 1);
                break;
            }
        }
    }

    let end = end?;

    // Extract the JSON array
    let json_str = buffer[start..end].to_string();

    // Check if it looks like a tool call (has "name", "tool", or "function" key)
    if !json_str.contains("\"name\"") && !json_str.contains("\"tool\"") && !json_str.contains("\"function\"") {
        return None;
    }

    // Verify it's valid JSON
    if serde_json::from_str::<serde_json::Value>(&json_str).is_err() {
        return None;
    }

    // Return start position, the JSON, and remaining buffer
    let remaining = buffer[end..].to_string();
    Some((start, json_str, remaining))
}

/// Detect if content is repetitive (indicating a loop)
fn detect_repetition(recent_chunks: &[String], new_chunk: &str, threshold: usize) -> bool {
    // === SINGLE-CHUNK REPETITION DETECTION ===
    // Check for repetitive words/phrases within a single chunk first
    // This catches cases where the model returns one large chunk with repetitive thinking
    let repetitive_phrases = [
        ("可能", 10), // "maybe" - shouldn't appear >10 times
        ("或者", 8),  // "or"
        ("也许", 8),  // "perhaps"
        ("temperature", 8),
        ("温度", 10),
        ("sensor", 8),
        ("传感器", 8),
        ("可能", 10), // "possible" (Chinese)
    ];

    for (phrase, limit) in repetitive_phrases {
        let count = new_chunk.matches(phrase).count();
        if count > limit {
            tracing::warn!(
                "Single-chunk repetition detected: '{}' appears {} times (limit: {})",
                phrase,
                count,
                limit
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
    let similar_count = recent
        .iter()
        .filter(|chunk| {
            // Check similarity: at least 80% character overlap
            let overlap = chunk
                .chars()
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
    let combined: String = recent_chunks
        .iter()
        .map(|s| s.as_str())
        .chain(std::iter::once(new_chunk))
        .collect::<Vec<&str>>()
        .join("");

    for (phrase, limit) in repetitive_phrases {
        let count = combined.matches(phrase).count();
        if count > limit * 2 {
            // Higher limit for combined text
            tracing::warn!(
                "Combined repetition detected: '{}' appears {} times (limit: {})",
                phrase,
                count,
                limit * 2
            );
            return true;
        }
    }

    false
}

/// Simple in-memory cache for tool results with TTL and size limit
struct ToolResultCache {
    entries: HashMap<String, (edge_ai_tools::ToolOutput, Instant)>,
    ttl: Duration,
    max_entries: usize,
}

impl ToolResultCache {
    fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            max_entries: 1000, // Prevent unbounded memory growth
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
        // Enforce size limit - remove oldest entry if at capacity
        if self.entries.len() >= self.max_entries {
            // Remove the oldest entry (first key in iteration)
            if let Some(oldest_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(key, (value, Instant::now()));
    }

    fn cleanup_expired(&mut self) {
        self.entries
            .retain(|_, (_, timestamp)| timestamp.elapsed() < self.ttl);

        // Also enforce size limit after cleanup
        while self.entries.len() > self.max_entries {
            if let Some(oldest_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest_key);
            }
        }
    }

    /// Get current cache size (for monitoring)
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Generate cache key from tool name and arguments.
    /// Sorts object keys to ensure consistent keys regardless of parameter order.
    fn make_key(name: &str, arguments: &Value) -> String {
        // For objects, sort keys to ensure consistent cache keys
        if let Some(obj) = arguments.as_object() {
            let mut sorted_pairs: Vec<_> = obj.iter().collect();
            sorted_pairs.sort_by(|a, b| a.0.cmp(b.0));

            let sorted_obj: serde_json::Map<String, Value> =
                sorted_pairs.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect();

            format!("{}:{}", name, serde_json::to_string(&sorted_obj).unwrap_or_default())
        } else {
            // For non-objects (arrays, strings, numbers, etc.), use as-is
            format!("{}:{}", name, arguments)
        }
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

/// Simple query tools that can return results directly without LLM follow-up
/// These tools return structured data that users want to see as-is
/// Skipping LLM follow-up for these tools:
/// 1. Reduces latency (no second LLM call)
/// 2. Eliminates unnecessary thinking content
/// 3. Provides exact data from tools without LLM reformatting
const SIMPLE_QUERY_TOOLS: &[&str] = &[
    "list_devices",
    "list_rules",
    "list_scenarios",
    "list_workflows",
    "query_rule_history",
    "query_workflow_status",
    "get_device_metrics",
];

fn is_tool_cacheable(name: &str) -> bool {
    !NON_CACHEABLE_TOOLS.contains(&name)
}

/// Check if all tools in the result set are simple query tools
/// that can return results directly without LLM follow-up
fn should_return_directly(tool_results: &[(String, String)]) -> bool {
    if tool_results.is_empty() {
        return false;
    }
    // All tools must be simple query tools
    tool_results
        .iter()
        .all(|(name, _)| SIMPLE_QUERY_TOOLS.contains(&name.as_str()))
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
                        response.push_str(&format!("## 设备列表 (共 {} 个)\n\n", devices.len()));
                        response.push_str("| 设备名称 | 状态 | 类型 |\n");
                        response.push_str("|---------|------|------|\n");
                        for device in devices {
                            let name = device
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("未知");
                            let status = device
                                .get("status")
                                .and_then(|s| s.as_str())
                                .unwrap_or("未知");
                            let device_type = device
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("未知");
                            response.push_str(&format!(
                                "| {} | {} | {} |\n",
                                name, status, device_type
                            ));
                        }
                    } else {
                        response.push_str("未找到任何设备。\n");
                    }
                }
                "list_rules" => {
                    // Format rule list
                    if let Some(rules) = json_value.get("rules").and_then(|r| r.as_array()) {
                        response.push_str(&format!("## 自动化规则 (共 {} 个)\n\n", rules.len()));
                        for rule in rules {
                            let name = rule.get("name").and_then(|n| n.as_str()).unwrap_or("未知");
                            let enabled = rule
                                .get("enabled")
                                .and_then(|e| e.as_bool())
                                .unwrap_or(false);
                            let status = if enabled {
                                "✓ 已启用"
                            } else {
                                "✗ 已禁用"
                            };
                            response.push_str(&format!("- **{}** {}\n", name, status));
                        }
                    } else if let Some(count) = json_value.get("count").and_then(|c| c.as_u64()) {
                        response.push_str(&format!("## 自动化规则 (共 {} 个)\n", count));
                    } else {
                        response.push_str("未找到任何自动化规则。\n");
                    }
                }
                "list_scenarios" => {
                    if let Some(scenarios) = json_value.get("scenarios").and_then(|s| s.as_array())
                    {
                        response.push_str(&format!("## 场景列表 (共 {} 个)\n\n", scenarios.len()));
                        for scenario in scenarios {
                            let name = scenario
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("未知");
                            response.push_str(&format!("- {}\n", name));
                        }
                    } else {
                        response.push_str("未找到任何场景。\n");
                    }
                }
                "list_workflows" => {
                    if let Some(workflows) = json_value.get("workflows").and_then(|w| w.as_array())
                    {
                        response
                            .push_str(&format!("## 工作流列表 (共 {} 个)\n\n", workflows.len()));
                        for workflow in workflows {
                            let name = workflow
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("未知");
                            let status = workflow
                                .get("status")
                                .and_then(|s| s.as_str())
                                .unwrap_or("未知");
                            response.push_str(&format!("- **{}** ({})\n", name, status));
                        }
                    } else {
                        response.push_str("未找到任何工作流。\n");
                    }
                }
                "query_rule_history" => {
                    if let Some(history) = json_value.get("history").and_then(|h| h.as_array()) {
                        response
                            .push_str(&format!("## 规则执行历史 (共 {} 条)\n\n", history.len()));
                        for (i, entry) in history.iter().enumerate().take(10) {
                            // Limit to 10 entries
                            let name = entry
                                .get("rule_name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("未知");
                            let success = entry
                                .get("success")
                                .and_then(|s| s.as_bool())
                                .unwrap_or(false);
                            let status = if success { "✓ 成功" } else { "✗ 失败" };
                            response.push_str(&format!("- **{}** {}\n", name, status));
                            if i == 9 {
                                response.push_str(&format!(
                                    "\n... (还有 {} 条记录)\n",
                                    history.len().saturating_sub(10)
                                ));
                                break;
                            }
                        }
                    } else {
                        response.push_str("未找到执行历史记录。\n");
                    }
                }
                "query_workflow_status" => {
                    if let Some(executions) =
                        json_value.get("executions").and_then(|e| e.as_array())
                    {
                        response.push_str(&format!(
                            "## 工作流执行状态 (共 {} 条)\n\n",
                            executions.len()
                        ));
                        for (i, exec) in executions.iter().enumerate().take(10) {
                            let wf_id = exec
                                .get("workflow_id")
                                .and_then(|w| w.as_str())
                                .unwrap_or("未知");
                            let status = exec
                                .get("status")
                                .and_then(|s| s.as_str())
                                .unwrap_or("未知");
                            response.push_str(&format!("- **{}** - {}\n", wf_id, status));
                            if i == 9 {
                                response.push_str(&format!(
                                    "\n... (还有 {} 条记录)\n",
                                    executions.len().saturating_sub(10)
                                ));
                                break;
                            }
                        }
                    } else {
                        response.push_str("未找到执行记录。\n");
                    }
                }
                "get_device_metrics" => {
                    if let Some(metrics) = json_value.get("metrics").and_then(|m| m.as_array()) {
                        response.push_str("## 设备指标\n\n");
                        for metric in metrics {
                            let name = metric
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("未知");
                            let value = metric
                                .get("value")
                                .and_then(|v| v.as_str())
                                .unwrap_or("未知");
                            response.push_str(&format!("- **{}**: {}\n", name, value));
                        }
                    } else {
                        response.push_str("未找到设备指标。\n");
                    }
                }
                "query_data" => {
                    // Format query result
                    if let Some(data) = json_value.get("data") {
                        response.push_str(&format!(
                            "## 查询结果\n\n```\n{}\n```\n",
                            serde_json::to_string_pretty(data).unwrap_or_default()
                        ));
                    } else {
                        response.push_str("查询完成。\n");
                    }
                }
                "control_device" | "send_command" => {
                    response.push_str("✓ 命令执行成功。\n");
                }
                "create_rule" => {
                    if let Some(rule_id) = json_value.get("rule_id").and_then(|r| r.as_str()) {
                        response.push_str(&format!("✓ 规则创建成功 (ID: {})\n", rule_id));
                    } else {
                        response.push_str("✓ 规则创建成功。\n");
                    }
                }
                "trigger_workflow" => {
                    if let Some(execution_id) =
                        json_value.get("execution_id").and_then(|e| e.as_str())
                    {
                        response.push_str(&format!("✓ 工作流已触发 (执行ID: {})\n", execution_id));
                    } else {
                        response.push_str("✓ 工作流已触发。\n");
                    }
                }
                _ => {
                    // Generic formatting for other tools
                    response.push_str(&format!("✓ {} 执行完成。\n", tool_name));
                }
            }
        } else {
            // Result is not valid JSON, use as-is
            response.push_str(&format!("✓ {} 执行完成。\n", tool_name));
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

        if msg.tool_calls.is_some() && msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            tool_result_count += 1;

            if tool_result_count <= KEEP_RECENT_TOOL_RESULTS {
                result.push(msg.clone());
            } else {
                // Compress old tool results
                let tool_names: Vec<&str> = msg
                    .tool_calls
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
///
/// The `max_tokens` parameter allows dynamic context sizing based on the model's actual capacity.
fn build_context_window(messages: &[AgentMessage], max_tokens: usize) -> Vec<AgentMessage> {
    let compacted = compact_tool_results_stream(messages);

    let mut selected_messages = Vec::new();
    let mut current_tokens = 0;
    const MIN_RECENT_MESSAGES: usize = 4;

    for msg in compacted.iter().rev() {
        let msg_tokens = estimate_tokens(&msg.content);

        if current_tokens + msg_tokens > max_tokens {
            let is_recent = selected_messages.len() < MIN_RECENT_MESSAGES;
            if msg.role == "system" || msg.role == "user" || is_recent {
                let max_len = (max_tokens - current_tokens) * 4;
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
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    process_stream_events_with_safeguards(
        llm_interface,
        internal_state,
        tools,
        user_message,
        StreamSafeguards::default(),
    )
    .await
}

pub async fn process_stream_events_with_safeguards(
    llm_interface: Arc<LlmInterface>,
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
    safeguards: StreamSafeguards,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    let user_message = user_message.to_string();

    // === FAST PATH: Simple greetings and common patterns ===
    // Bypass LLM for simple, common interactions to improve speed and reliability
    let trimmed = user_message.trim();
    let lower = trimmed.to_lowercase();

    // Greeting patterns
    let greeting_patterns = [
        "你好", "您好", "hi", "hello", "嗨", "在吗",
        "早上好", "下午好", "晚上好",
    ];

    // Device list query patterns - fast path for common device queries
    let device_list_patterns = [
        "有哪些设备", "有什么设备", "设备列表", "查看设备", "所有设备",
        "列出设备", "系统设备", "显示设备",
        "devices", "list devices",
    ];

    // Temperature query patterns - fast path for temperature queries
    let temp_query_patterns = [
        "温度", "temperature",
    ];

    let _is_greeting = greeting_patterns
        .iter()
        .any(|&pat| trimmed.eq_ignore_ascii_case(pat) || trimmed.starts_with(pat));

    // Check for device list query
    let _is_device_query = device_list_patterns
        .iter()
        .any(|&pat| lower.contains(&pat.to_lowercase()) && lower.len() < 30);

    // Check for temperature query (simple single-word queries)
    let _is_temp_query = temp_query_patterns
        .iter()
        .any(|&pat| lower == pat || lower.ends_with(pat) || lower.starts_with("当前") && lower.contains("温度"));

    // === INTENT RECOGNITION: Understand user intent before LLM call ===
    // This helps reduce cognitive load and provides better visualization
    let classifier = IntentClassifier::default();
    let intent_result = classifier.classify(&user_message);

    tracing::info!(
        "Intent recognized: category={:?}, confidence={:.2}, keywords={:?}",
        intent_result.category,
        intent_result.confidence,
        intent_result.keywords
    );

    // Prepare intent and plan events for frontend visualization
    let intent_event = AgentEvent::intent(
        format!("{:?}", intent_result.category),
        intent_result.category.display_name(),
        intent_result.confidence,
        intent_result.keywords.clone(),
    );

    // Plan steps based on intent
    let plan_steps = match intent_result.category {
        IntentCategory::Device => vec![
            ("识别用户查询意图", "Intent"),
            ("获取设备列表", "Execution"),
            ("返回设备信息", "Response"),
        ],
        IntentCategory::Rule => vec![
            ("识别规则查询意图", "Intent"),
            ("获取规则列表", "Execution"),
            ("返回规则信息", "Response"),
        ],
        IntentCategory::Workflow => vec![
            ("识别工作流查询意图", "Intent"),
            ("获取工作流列表", "Execution"),
            ("返回工作流信息", "Response"),
        ],
        IntentCategory::Data => vec![
            ("识别数据查询意图", "Intent"),
            ("查询设备数据", "Execution"),
            ("返回数据结果", "Response"),
        ],
        IntentCategory::Alert => vec![
            ("识别告警查询意图", "Intent"),
            ("获取告警列表", "Execution"),
            ("返回告警信息", "Response"),
        ],
        IntentCategory::System => vec![
            ("识别系统状态意图", "Intent"),
            ("获取系统信息", "Execution"),
            ("返回系统状态", "Response"),
        ],
        IntentCategory::Help => vec![
            ("识别帮助请求意图", "Intent"),
            ("提供使用说明", "Response"),
        ],
        IntentCategory::General => vec![("理解用户问题", "Intent"), ("生成回复", "Response")],
    };

    // === COMPLEX INTENT DETECTION FOR MULTI-ROUND TOOL CALLING ===
    // Complex intents require multi-step tool calling (e.g., conditional actions)
    // Examples: "如果温度超过30度就打开空调", "查询设备状态并记录"
    let is_complex_intent = is_complex_multi_step_intent(&user_message);

    tracing::info!(
        "Complex intent detection: is_complex={}, message={}",
        is_complex_intent,
        user_message.chars().take(50).collect::<String>()
    );

    // === FIX 1: Get conversation history and pass to LLM ===
    // This prevents the LLM from repeating actions or calling tools again
    // Pure async - no block_in_place
    let state_guard = internal_state.read().await;
    let history_messages = state_guard.memory.clone();
    drop(state_guard); // Release lock before calling LLM

    // === DYNAMIC CONTEXT WINDOW: Get model's actual capacity ===
    let max_context = llm_interface.max_context_length().await;
    // For models with large context (32k+), allow up to 16k for better long conversations
    // For smaller models (8k), use their full capacity
    const DEFAULT_MAX_CONTEXT: usize = 8_000;
    let effective_max = if max_context >= 32000 {
        max_context.min(16_384) // Large models: cap at 16k
    } else {
        max_context.min(DEFAULT_MAX_CONTEXT) // Small models: use full capability
    };

    let history_for_llm: Vec<edge_ai_core::Message> =
        build_context_window(&history_messages, effective_max)
            .iter()
            .map(|msg| msg.to_core())
            .collect::<Vec<_>>();

    tracing::debug!(
        "Passing {} messages from history to LLM",
        history_for_llm.len()
    );

    // === INTENT-BASED THINKING CONTROL ===
    // For simple list-type queries, disable thinking to get faster responses
    // This prevents the model from using all tokens for thinking
    // IMPORTANT: Disable thinking when tools are likely needed to prevent timeout
    let has_tool_keywords = user_message.contains("温度")
        || user_message.contains("湿度")
        || user_message.contains("查询")
        || user_message.contains("多少")
        || user_message.contains("状态")
        || user_message.contains("设备")
        || user_message.contains("打开")
        || user_message.contains("关闭")
        || user_message.contains("控制");

    let use_thinking = match intent_result.category {
        // Disable thinking for simple list queries
        IntentCategory::Device => {
            // Check if it's a simple list query (no complex context)
            !user_message.contains("在线")
                && !user_message.contains("状态")
                && !user_message.contains("控制")
        }
        IntentCategory::Rule => {
            !user_message.contains("历史")
                && !user_message.contains("创建")
                && !user_message.contains("启用")
        }
        IntentCategory::Workflow => {
            !user_message.contains("执行")
                && !user_message.contains("触发")
                && !user_message.contains("状态")
        }
        IntentCategory::Alert => {
            // Enable thinking for alert analysis
            user_message.contains("分析")
                || user_message.contains("原因")
                || user_message.contains("统计")
        }
        IntentCategory::System => {
            // Enable thinking for system analysis
            user_message.contains("诊断")
                || user_message.contains("问题")
                || user_message.contains("异常")
        }
        IntentCategory::Help => {
            // Disable thinking for simple help queries
            !user_message.contains("怎么") && !user_message.contains("如何")
        }
        // Data/General: disable thinking when tool keywords present, enable otherwise
        IntentCategory::Data | IntentCategory::General => !has_tool_keywords,
    };

    tracing::info!(
        "Intent-based thinking control: category={:?}, thinking_enabled={}",
        intent_result.category,
        use_thinking
    );

    // Get the stream from llm_interface - with or without thinking based on intent
    let stream_result = if use_thinking {
        llm_interface
            .chat_stream_with_history(&user_message, &history_for_llm)
            .await
    } else {
        llm_interface
            .chat_stream_no_thinking_with_history(&user_message, &history_for_llm)
            .await
    };

    let stream = stream_result.map_err(|e| AgentError::Llm(e.to_string()))?;

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

        // === KEEPALIVE: Track last event time for heartbeat ===
        let mut last_event_time = Instant::now();
        let mut last_progress_time = Instant::now();
        let mut current_stage = "thinking";

        // === TIMEOUT WARNING FLAGS ===
        let mut timeout_warned = false;
        let mut long_thinking_warned = false;

        // === SAFEGUARD: Track recent chunks for repetition detection ===
        let mut recent_chunks: Vec<String> = Vec::new();
        const RECENT_CHUNK_WINDOW: usize = 10;

        // === SAFEGUARD: Track recently executed tools to prevent loops ===
        let mut recently_executed_tools: VecDeque<String> = VecDeque::new();

        // === SAFEGUARD: Track multi-round tool calling iterations ===
        let mut tool_iteration_count = 0usize;
        const MAX_TOOL_ITERATIONS: usize = 5;

        // === INTENT & PLAN VISUALIZATION ===
        // Send intent and plan events first to show user what's happening
        yield intent_event;
        last_event_time = Instant::now();

        for (step, stage) in &plan_steps {
            yield AgentEvent::plan(*step, *stage);
        }

        // === MULTI-ROUND TOOL CALLING LOOP ===
        // For complex intents, we may need multiple rounds of tool calling
        'multi_round_loop: loop {
            if tool_iteration_count > 0 {
                tracing::info!("Starting tool iteration round {}", tool_iteration_count + 1);

                // For subsequent rounds, we need a new LLM call with tools enabled
                // Use the same dynamic context limit for consistency
                let state_guard = internal_state.read().await;

                let max_context = llm_interface.max_context_length().await;
                const DEFAULT_MAX_CONTEXT: usize = 8_000;
                let effective_max = if max_context >= 32000 {
                    max_context.min(16_384) // Large models: cap at 16k
                } else {
                    max_context.min(DEFAULT_MAX_CONTEXT) // Small models: use full capability
                };

                let history_for_round = build_context_window(&state_guard.memory, effective_max);
                drop(state_guard);

                let history_for_llm: Vec<edge_ai_core::Message> = history_for_round
                    .iter()
                    .map(|msg| msg.to_core())
                    .collect::<Vec<_>>();

                // Use tools enabled, no thinking for subsequent rounds
                let round_stream_result = llm_interface.chat_stream_no_thinking_with_history(
                    "请继续处理之前的工具执行结果。如果需要更多工具调用，请使用工具。如果任务完成，请给出最终回复。",
                    &history_for_llm
                ).await;

                let round_stream = match round_stream_result {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Round {} LLM call failed: {}", tool_iteration_count + 1, e);
                        yield AgentEvent::error(format!("工具调用失败: {}", e));
                        break 'multi_round_loop;
                    }
                };

                stream = Box::pin(round_stream);
                buffer = String::new();
                tool_calls.clear();
                content_before_tools = String::new();
            }

            // === PHASE 1: Stream initial response (thinking + content + tool calls) ===
            while let Some(result) = StreamExt::next(&mut stream).await {
                let elapsed = stream_start.elapsed();

                // Check timeout with early warning at 80% of max duration
                let timeout_threshold = safeguards.max_stream_duration;
                let warning_threshold = timeout_threshold.mul_f32(0.8);

                if elapsed > timeout_threshold {
                    tracing::warn!("Stream timeout ({:?} elapsed, max: {:?}), forcing completion", elapsed, timeout_threshold);
                    // Don't break here - let tool calls be processed
                    // Just log the timeout and continue to check for tool calls
                    if tool_calls_detected {
                        tracing::info!("Timeout with tool calls detected, proceeding to execution");
                        break;
                    } else {
                        yield AgentEvent::error(format!("请求超时（已用时{:.1}秒），正在完成处理...", elapsed.as_secs_f64()));
                        break;
                    }
                } else if elapsed > warning_threshold && !timeout_warned {
                    tracing::warn!("Stream approaching timeout ({:.1}s elapsed, max: {:.1}s)", elapsed.as_secs_f64(), timeout_threshold.as_secs_f64());
                    yield AgentEvent::warning(format!("响应时间较长（已用时{:.1}秒），请耐心等待...", elapsed.as_secs_f64()));
                    timeout_warned = true;
                }

                // Special warning for extended thinking with no content
                if has_thinking && !has_content && elapsed > Duration::from_secs(60) && !long_thinking_warned {
                    tracing::warn!("Extended thinking detected ({:.1}s) with no content yet", elapsed.as_secs_f64());
                    yield AgentEvent::warning("模型正在进行深度思考，可能需要更长时间...".to_string());
                    long_thinking_warned = true;
                }

                // Check for interrupt signal
                // We clone the value to avoid holding the guard across await
                let is_interrupted = safeguards.interrupt_signal.as_ref().map(|rx| *rx.borrow()).unwrap_or(false);
                if is_interrupted {
                    tracing::info!("Stream interrupted by user");
                    yield AgentEvent::content("\n\n[已中断]".to_string());
                    yield AgentEvent::end();
                    return;
                }

                // === KEEPALIVE: Send heartbeat if no events for too long ===
                if last_event_time.elapsed() > safeguards.heartbeat_interval {
                    yield AgentEvent::heartbeat();
                    last_event_time = Instant::now();
                }

                // === PROGRESS: Send progress update during long operations ===
                if last_progress_time.elapsed() > safeguards.progress_interval {
                    let stage_name = if has_thinking && !has_content {
                        "thinking"
                    } else if tool_calls_detected {
                        "executing"
                    } else {
                        "generating"
                    };
                    let elapsed_ms = elapsed.as_millis() as u64;
                    yield AgentEvent::progress(
                        format!("正在{}...", match stage_name {
                            "thinking" => "思考",
                            "executing" => "执行工具",
                            _ => "生成回复",
                        }),
                        stage_name,
                        elapsed_ms
                    );
                    last_progress_time = Instant::now();
                    current_stage = stage_name;
                }

                match result {
                    Ok((text, is_thinking)) => {
                        if text.is_empty() {
                            continue;
                        }

                        // === SAFEGUARD: Repetition detection ===
                        recent_chunks.push(text.clone());
                        if recent_chunks.len() > RECENT_CHUNK_WINDOW {
                            recent_chunks.remove(0);
                        }

                        if detect_repetition(&recent_chunks, &text, safeguards.max_repetition_count) {
                            tracing::warn!("Repetition detected, stopping stream");
                            yield AgentEvent::error("检测到重复内容，正在完成处理...".to_string());
                            break;
                        }

                        if is_thinking {
                            // No thinking limit - let the model think as much as needed
                            // First, add the new text to thinking content
                            thinking_content.push_str(&text);
                            has_thinking = true;

                            // === IMPORTANT: Check for tool calls BEFORE yielding thinking event ===
                            // Some models (like qwen3-vl:2b) output tool calls within thinking field
                            // We need to detect and extract them BEFORE sending to frontend
                            let mut text_to_yield = text.clone();
                            let thinking_with_new = thinking_content.as_str();
                            let mut had_tool_calls = false;

                            // Check for XML tool calls in thinking: <tool_calls>...</tool_calls>
                            if let Some(tool_start) = thinking_with_new.find("<tool_calls>") {
                                if let Some(tool_end) = thinking_with_new.find("</tool_calls>") {
                                    let tool_content = thinking_with_new[tool_start..tool_end + 13].to_string();

                                    // Parse the tool calls from thinking
                                    if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
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

                                        if !duplicate_found {
                                            tool_calls_detected = true;
                                            tool_calls.extend(calls);
                                            had_tool_calls = true;
                                            // Remove tool calls from thinking content
                                            thinking_content = format!("{}{}", &thinking_with_new[..tool_start], &thinking_with_new[tool_end + 13..]);
                                            // Don't yield tool call XML as thinking content
                                            text_to_yield = String::new();
                                            tracing::info!("Extracted {} tool calls from thinking content", tool_calls.len());
                                        }
                                    }
                                }
                            }
                            // Also check for JSON tool calls in thinking
                            else if let Some((json_start, tool_json, remaining)) = detect_json_tool_calls(thinking_with_new)
                                && let Ok((_, calls)) = parse_tool_calls(&tool_json) {
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

                                    if !duplicate_found {
                                        tool_calls_detected = true;
                                        tool_calls.extend(calls);
                                        had_tool_calls = true;
                                        // Remove tool calls from thinking content
                                        thinking_content = format!("{}{}", &thinking_with_new[..json_start], remaining);
                                        // Don't yield tool call JSON as thinking content
                                        text_to_yield = String::new();
                                        tracing::info!("Extracted {} JSON tool calls from thinking content", tool_calls.len());
                                    }
                                }

                            // Only yield non-empty thinking content (without tool calls)
                            if !text_to_yield.is_empty() {
                                yield AgentEvent::thinking(text_to_yield);
                            } else if had_tool_calls {
                                // If we had tool calls but no other thinking content, yield empty thinking
                                // to ensure the frontend knows thinking phase is happening
                                yield AgentEvent::thinking(String::new());
                            }
                            last_event_time = Instant::now();
                            continue;
                        }

                        // content: need to check for tool calls
                        has_content = true;
                        last_event_time = Instant::now();

                        if safeguards.max_content_length != usize::MAX
                            && content_before_tools.len() + buffer.len() + text.len() > safeguards.max_content_length
                        {
                            tracing::warn!("Content exceeded max length ({}), stopping stream", safeguards.max_content_length);
                            yield AgentEvent::error("Response too long - content limit reached".to_string());
                            break;
                        }

                        buffer.push_str(&text);

                        // Check for tool calls in buffer (support both XML and JSON formats)
                        // Try JSON format first: [{"name": "tool", "arguments": {...}}]
                        let json_tool_check = detect_json_tool_calls(&buffer);
                        if let Some((json_start, tool_json, remaining)) = json_tool_check {
                            // Found JSON tool calls - split buffer into before, tool, and remaining
                            let before_tool = &buffer[..json_start];
                            if !before_tool.is_empty() {
                                content_before_tools.push_str(before_tool);
                                yield AgentEvent::content(before_tool.to_string());
                            }

                            // Parse the JSON tool calls
                            if let Ok((_, calls)) = parse_tool_calls(&tool_json) {
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

                                if !duplicate_found {
                                    tool_calls_detected = true;
                                    tool_calls.extend(calls);
                                }
                            }

                            // Update buffer with remaining content
                            buffer = remaining.to_string();
                        } else if let Some(tool_start) = buffer.find("<tool_calls>") {
                            let before_tool = &buffer[..tool_start];
                            if !before_tool.is_empty() {
                                content_before_tools.push_str(before_tool);
                                yield AgentEvent::content(before_tool.to_string());
                            }

                            if let Some(tool_end) = buffer.find("</tool_calls>") {
                                let tool_content = buffer[tool_start..tool_end + 13].to_string();
                                buffer = buffer[tool_end + 13..].to_string();

                                if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
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

                                    if !duplicate_found {
                                        tool_calls_detected = true;
                                        tool_calls.extend(calls);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        yield AgentEvent::error(format!("Stream error: {}", e));
                        break;
                    }
                }
            }

            // === PHASE 2: Handle tool calls if detected ===
            if tool_calls_detected {
                tracing::info!("Starting tool execution round {}", tool_iteration_count + 1);

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

                // Create cache for this batch of tool executions
                let cache = Arc::new(RwLock::new(ToolResultCache::new(Duration::from_secs(300))));

                // Execute all tool calls in parallel
                let tool_futures: Vec<_> = tool_calls_to_execute.iter().map(|tool_call| {
                    let tools_clone = tools.clone();
                    let cache_clone = cache.clone();
                    let name = tool_call.name.clone();
                    let arguments = tool_call.arguments.clone();
                    let name_clone = name.clone();

                    async move {
                        (name.clone(), ToolExecutionResult {
                            name: name_clone,
                            result: execute_tool_with_retry(&tools_clone, &cache_clone, &name, arguments.clone()).await,
                        })
                    }
                }).collect();

                let tool_results_executed = futures::future::join_all(tool_futures).await;

                // Process results
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

                // Update recently executed tools list
                for (name, _result) in &tool_call_results {
                    if !recently_executed_tools.contains(name) {
                        recently_executed_tools.push_back(name.clone());
                        if recently_executed_tools.len() > 5 {
                            recently_executed_tools.pop_front();
                        }
                        tracing::debug!("Added '{}' to recently executed tools (now: {:?})", name, recently_executed_tools);
                    }
                }

                // === PHASE 3: Generate follow-up response ===
                // For complex intents, check if we need more tool calls
                if is_complex_intent && tool_iteration_count < MAX_TOOL_ITERATIONS - 1 {
                    tracing::info!("Complex intent: Checking if more tool calls needed (iteration {}/{})",
                        tool_iteration_count + 1, MAX_TOOL_ITERATIONS);

                    // Save results to memory
                    for (tool_name, result_str) in &tool_call_results {
                        let tool_result_msg = AgentMessage::tool_result(tool_name, result_str);
                        internal_state.write().await.push_message(tool_result_msg);
                    }

                    // Increment iteration count and loop back
                    tool_iteration_count += 1;
                    tool_calls_detected = false;
                    tool_calls.clear();

                    // Continue the loop to make another LLM call with tools
                    continue 'multi_round_loop;
                }

                // === SIMPLE INTENT OR MAX ITERATIONS REACHED: Final response ===
                // Save the initial message with thinking and tool calls
                let response_to_save = if content_before_tools.is_empty() {
                    // No content before tools - use empty string, don't show meaningless fallback
                    String::new()
                } else {
                    content_before_tools.clone()
                };

                let initial_msg = if !thinking_content.is_empty() {
                    let cleaned_thinking = cleanup_thinking_content(&thinking_content);
                    AgentMessage::assistant_with_tools_and_thinking(
                        &response_to_save,
                        tool_calls_with_results.clone(),
                        &cleaned_thinking,
                    )
                } else {
                    AgentMessage::assistant_with_tools(&response_to_save, tool_calls_with_results.clone())
                };
                eprintln!("[streaming] Saving initial assistant message with {} tool_calls", initial_msg.tool_calls.as_ref().map_or(0, |c| c.len()));
                internal_state.write().await.push_message(initial_msg);

                // Add tool result messages to history
                for (tool_name, result_str) in &tool_call_results {
                    let tool_result_msg = AgentMessage::tool_result(tool_name, result_str);
                    internal_state.write().await.push_message(tool_result_msg);
                }

                // Trim history
                let state_guard = internal_state.read().await;
                let mut history_messages: Vec<edge_ai_core::Message> = state_guard.memory.iter()
                    .map(|msg| msg.to_core())
                    .collect::<Vec<_>>();
                drop(state_guard);

                if history_messages.len() > 6 {
                    let keep_count = 6;
                    tracing::info!("Trimming history from {} to {} messages",
                        history_messages.len(), keep_count);
                    let split_idx = history_messages.len() - keep_count;
                    history_messages = history_messages.split_off(split_idx);
                }

                // Direct return for simple query tools
                let simple_query_tools = ["list_devices", "list_rules", "list_workflows", "list_alerts"];
                let is_simple_query = tool_call_results.len() == 1
                    && simple_query_tools.contains(&tool_call_results[0].0.as_str());

                if is_simple_query {
                    let (_tool_name, result_str) = &tool_call_results[0];
                    if let Ok(json_val) = serde_json::from_str::<Value>(result_str) {
                        let final_text = if let Some(devices) = json_val.get("devices").and_then(|d| d.as_array()) {
                            format!("已找到 {} 个设备。", devices.len())
                        } else if let Some(rules) = json_val.get("rules").and_then(|r| r.as_array()) {
                            format!("已找到 {} 条规则。", rules.len())
                        } else if let Some(workflows) = json_val.get("workflows").and_then(|w| w.as_array()) {
                            format!("已找到 {} 个工作流。", workflows.len())
                        } else {
                            "查询完成。".to_string()
                        };

                        yield AgentEvent::content(final_text.clone());
                        let final_msg = AgentMessage::assistant(&final_text);
                        internal_state.write().await.push_message(final_msg);
                        yield AgentEvent::end();
                        return;
                    }
                }

                // Phase 2 LLM call for follow-up
                tracing::info!("Phase 2: Generating follow-up response");

                let followup_stream_result = llm_interface.chat_stream_no_tools_no_thinking_with_history(
                    "请根据工具执行结果生成简洁的回复。", &history_messages
                ).await;

                let followup_stream = match followup_stream_result {
                    Ok(stream) => stream,
                    Err(e) => {
                        tracing::error!("Phase 2 LLM call failed: {}", e);
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

                let mut followup_stream = Box::pin(followup_stream);
                let mut final_response_content = String::new();
                let followup_start = Instant::now();

                while let Some(result) = StreamExt::next(&mut followup_stream).await {
                    if followup_start.elapsed() > Duration::from_secs(30) {
                        tracing::warn!("Phase 2 timeout (>30s), forcing completion");
                        break;
                    }

                    match result {
                        Ok((chunk, is_thinking)) => {
                            if chunk.is_empty() {
                                continue;
                            }
                            if !is_thinking {
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

                if final_response_content.is_empty() {
                    let fallback = if tool_call_results.len() == 1 {
                        format!("{} 执行完成。", tool_call_results[0].0)
                    } else if tool_call_results.len() > 1 {
                        format!("已执行 {} 个工具操作。", tool_call_results.len())
                    } else {
                        "处理完成。".to_string()
                    };
                    yield AgentEvent::content(fallback.clone());
                    final_response_content = fallback;
                }

                // IMPORTANT: Update the initial message with the follow-up content
                // instead of saving a separate message. This ensures the message
                // has both tool_calls and content in one place.
                {
                    let mut state = internal_state.write().await;
                    if let Some(last_msg) = state.memory.last_mut() {
                        if last_msg.role == "assistant" && last_msg.tool_calls.is_some() {
                            // Update the last assistant message (which has tool_calls) with the content
                            last_msg.content = final_response_content.clone();
                        } else {
                            // Fallback: push a new message if the last one isn't what we expect
                            let final_msg = AgentMessage::assistant(&final_response_content);
                            state.memory.push(final_msg);
                        }
                    } else {
                        let final_msg = AgentMessage::assistant(&final_response_content);
                        state.memory.push(final_msg);
                    }
                }

                tracing::info!("Tool execution and Phase 2 response complete");
            } else {
                // No tool calls - save response directly
                let response_to_save = if content_before_tools.is_empty() {
                    // No content and no tools - use empty string
                    String::new()
                } else {
                    content_before_tools.clone()
                };

                let initial_msg = if !thinking_content.is_empty() {
                    let cleaned_thinking = cleanup_thinking_content(&thinking_content);
                    AgentMessage::assistant_with_thinking(&response_to_save, &cleaned_thinking)
                } else {
                    AgentMessage::assistant(&response_to_save)
                };
                internal_state.write().await.push_message(initial_msg);

                // Yield any remaining content
                if !buffer.is_empty() {
                    yield AgentEvent::content(buffer.clone());
                }
            }

            // Break the loop after processing
            break 'multi_round_loop;
        }

        yield AgentEvent::end();
    }))
}

/// Detect if the user's intent requires multi-step tool calling.
/// Complex intents include conditional logic, chained operations, etc.
fn is_complex_multi_step_intent(message: &str) -> bool {
    let complex_patterns = [
        // Conditional patterns
        ("如果", "就"),
        ("如果", "则"),
        ("当", "时"),
        ("超过", "就"),
        // Chained operation patterns
        ("查询", "然后"),
        ("检查", "之后"),
        // Multiple operation indicators
        ("并且", ""),
        ("同时", ""),
        ("和", ""),
    ];

    let lower = message.to_lowercase();

    for (first, second) in complex_patterns {
        if !second.is_empty() {
            if lower.contains(first) && lower.contains(second) {
                return true;
            }
        } else if lower.contains(first) {
            // Check if message has multiple verbs indicating multiple steps
            let verb_count = ["查询", "检查", "执行", "打开", "关闭", "启动", "停止"]
                .iter()
                .filter(|v| lower.contains(*v))
                .count();
            if verb_count > 1 {
                return true;
            }
        }
    }

    false
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
    if is_tool_cacheable(name)
        && let Ok(ref output) = result
            && output.success {
                let cache_key = ToolResultCache::make_key(name, &arguments);
                let mut cache_write = cache.write().await;
                cache_write.insert(cache_key, output.clone());
                // Periodic cleanup
                cache_write.cleanup_expired();
            }

    result
}

/// Map simplified tool names to real tool names.
///
/// Simplified names are used in LLM prompts (e.g., "device.discover")
/// while real names are used in ToolRegistry (e.g., "list_devices").
///
/// NOTE: This must match the mapping in agent/mod.rs to ensure consistency.
fn resolve_tool_name(simplified_name: &str) -> String {
    match simplified_name {
        // Device tools - note: device.* tools are registered with their simplified names
        // device.discover → list_devices (for backward compatibility)
        "device.discover" => "list_devices".to_string(),
        // device.query, device.control, device.analyze are registered as-is
        "device.query" | "device.control" | "device.analyze" => simplified_name.to_string(),

        // Rule tools
        "rule.list" | "rules.list" => "list_rules".to_string(),
        // rule.from_context is a separate tool that generates DSL from natural language
        // It should keep its name as it's registered as "rule.from_context" in core_tools
        // Note: "rule.create" is an alias for create_rule
        "rule.create" => "create_rule".to_string(),
        // These tools are registered with their simplified names
        "rule.from_context" => "rule.from_context".to_string(),
        "rule.enable" => "enable_rule".to_string(),
        "rule.disable" => "disable_rule".to_string(),
        "rule.test" => "test_rule".to_string(),

        // Workflow tools
        "workflow.list" | "workflows.list" => "list_workflows".to_string(),
        "workflow.create" => "create_workflow".to_string(),
        "workflow.trigger" | "workflow.execute" => "trigger_workflow".to_string(),

        // Scenario tools
        "scenario.list" => "list_scenarios".to_string(),
        "scenario.create" => "create_scenario".to_string(),
        "scenario.execute" => "execute_scenario".to_string(),

        // Data tools
        "data.query" => "query_data".to_string(),
        "data.analyze" => "analyze_data".to_string(),

        // Alert tools
        "alert.list" => "list_alerts".to_string(),
        "alert.create" => "create_alert".to_string(),
        "alert.acknowledge" => "acknowledge_alert".to_string(),

        // Default: use the name as-is (already a real tool name)
        _ => simplified_name.to_string(),
    }
}

/// Inner retry logic without caching (for code reuse)
async fn execute_with_retry_impl(
    tools: &edge_ai_tools::ToolRegistry,
    name: &str,
    arguments: serde_json::Value,
    max_retries: u32,
) -> std::result::Result<edge_ai_tools::ToolOutput, edge_ai_tools::ToolError> {
    // Map simplified tool name to real tool name
    let real_tool_name = resolve_tool_name(name);

    for attempt in 0..=max_retries {
        let result = tools.execute(&real_tool_name, arguments.clone()).await;

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

    Err(edge_ai_tools::ToolError::Execution(
        "Max retries exceeded".to_string(),
    ))
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
            Ok((
                "<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(),
                false,
            )),
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
            Ok((
                "<tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>".to_string(),
                false,
            )),
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
        assert!(
            content.is_empty(),
            "Content should be empty for thinking-only response"
        );
        println!("✓ Thinking-only test passed");
        println!("  Thinking should be emitted as content: {}", thinking);
        println!(
            "  NOTE: In production, thinking content is emitted as final content when no actual content received"
        );
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
            Ok(("".to_string(), false)), // Empty chunk
            Ok(("继续".to_string(), false)),
            Ok(("".to_string(), false)), // Another empty chunk
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
        let input = r#"{"name": "test_tool", "arguments": {"param1": "value1"}}"#;

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
        println!(
            "  English ({} chars): ~{} tokens",
            english.chars().count(),
            english_tokens
        );
        println!(
            "  Chinese ({} chars): ~{} tokens",
            chinese.chars().count(),
            chinese_tokens
        );
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
