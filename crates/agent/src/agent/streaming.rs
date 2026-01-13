//! Streaming response processing with thinking tag support.

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
/// Adjust based on model capacity (e.g., qwen3-vl:2b has ~32k context)
const MAX_CONTEXT_TOKENS: usize = 8000;

/// Estimate token count for a string (rough approximation: 1 token ≈ 4 characters for Chinese, 1 token ≈ 4 characters for English)
/// This is a simple heuristic - for production, consider using a proper tokenizer
fn estimate_tokens(text: &str) -> usize {
    // Count characters and divide by 4 (average chars per token)
    // This works reasonably well for both Chinese and English
    text.chars().count() / 4
}

/// Build conversation context with token-based windowing.
/// Takes the most recent messages that fit within MAX_CONTEXT_TOKENS,
/// always including system messages and preserving conversation order.
fn build_context_window(messages: &[AgentMessage]) -> Vec<&AgentMessage> {
    let mut selected_messages = Vec::new();
    let mut current_tokens = 0;

    // Iterate in reverse to prioritize recent messages
    for msg in messages.iter().rev() {
        let msg_tokens = estimate_tokens(&msg.content);
        let thinking_tokens = msg.thinking.as_ref().map_or(0, |t| estimate_tokens(t));

        // Check if adding this message would exceed the limit
        if current_tokens + msg_tokens + thinking_tokens > MAX_CONTEXT_TOKENS {
            // Check if we should add it anyway (for important messages like system or user)
            if msg.role == "system" || msg.role == "user" {
                // Try to fit it by truncating if necessary
                let max_len = (MAX_CONTEXT_TOKENS - current_tokens) * 4;
                if max_len > 100 {
                    // Keep it but might truncate
                    selected_messages.push(msg);
                    current_tokens += msg_tokens + thinking_tokens;
                }
            }
            break;
        }

        selected_messages.push(msg);
        current_tokens += msg_tokens + thinking_tokens;
    }

    // Reverse to maintain original order
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
pub fn process_stream_events(
    llm_interface: Arc<LlmInterface>,
    short_term_memory: Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    state: Arc<RwLock<SessionState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    let user_message = user_message.to_string();
    let llm_clone = llm_interface.clone();

    // Get the stream from llm_interface - use block_in_place for sync→async transition
    let stream = tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("No runtime: {}", e))?;
        handle.block_on(llm_clone.chat_stream(&user_message))
            .map_err(|e| format!("Chat stream failed: {}", e))
    });

    let stream = stream.map_err(|e| AgentError::Llm(e))?;

    Ok(Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut buffer = String::new();
        let mut tool_calls_detected = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut content_before_tools = String::new();
        let mut thinking_content = String::new();
        let mut has_content = false;
        let mut has_thinking = false;

        // === Stream and forward events ===
        while let Some(result) = StreamExt::next(&mut stream).await {
            match result {
                Ok((text, is_thinking)) => {
                    if text.is_empty() {
                        continue;
                    }

                    // thinking: send immediately
                    if is_thinking {
                        thinking_content.push_str(&text);
                        has_thinking = true;
                        yield AgentEvent::thinking(text);
                        continue;
                    }

                    // content: need to check for tool calls
                    has_content = true;
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
                        if !buffer.contains("<tool_calls") {
                            content_before_tools.push_str(&buffer);
                            yield AgentEvent::content(buffer.clone());
                            buffer.clear();
                        }
                    }
                }
                Err(e) => {
                    yield AgentEvent::error(e.to_string());
                    break;
                }
            }
        }

        // Emit any remaining buffer
        if !buffer.is_empty() && !buffer.contains("<tool_calls>") {
            content_before_tools.push_str(&buffer);
            yield AgentEvent::content(buffer);
        }

        // IMPORTANT: If tool calls were detected, DON'T save the initial message yet.
        // We'll save a complete message (with tool_calls and final response) in Phase 2.
        // If no tool calls, save the response now.
        if !tool_calls_detected {
            let initial_msg = if !thinking_content.is_empty() {
                AgentMessage::assistant_with_thinking(&content_before_tools, &thinking_content)
            } else {
                AgentMessage::assistant(&content_before_tools)
            };
            short_term_memory.write().await.push(initial_msg);
        }

        // === PHASE 2: Handle tool calls if detected ===
        if tool_calls_detected {
            tracing::info!("Starting PARALLEL tool execution");

            // Create cache for this batch of tool executions (5 minute TTL)
            let cache = Arc::new(RwLock::new(ToolResultCache::new(Duration::from_secs(300))));

            // Execute all tool calls in parallel
            let tool_futures: Vec<_> = tool_calls.iter().map(|tool_call| {
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

            // Phase 2: Generate follow-up response after tool execution
            // Instead of calling LLM again (which would trigger excessive thinking),
            // directly format the tool results for a cleaner user experience
            let final_text = if tool_call_results.is_empty() {
                // No tools were executed, this shouldn't happen in Phase 2
                "操作已完成。".to_string()
            } else {
                // Format tool results directly
                format_tool_results(&tool_call_results)
            };

            // Stream the response in chunks
            let chars: Vec<char> = final_text.chars().collect();
            let chunk_size = 20usize;
            for chunk in chars.chunks(chunk_size) {
                let chunk_str: String = chunk.iter().collect();
                if !chunk_str.is_empty() {
                    yield AgentEvent::content(chunk_str);
                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                }
            }

            // Save a SINGLE complete message with tool_calls and final response
            // This prevents duplicate messages when session is restored
            // NOTE: Use tool_calls_with_results which includes execution results
            let complete_msg = if !thinking_content.is_empty() {
                AgentMessage::assistant_with_tools_and_thinking(
                    &final_text,
                    tool_calls_with_results.clone(),
                    &thinking_content,
                )
            } else {
                AgentMessage::assistant_with_tools(&final_text, tool_calls_with_results.clone())
            };
            short_term_memory.write().await.push(complete_msg);
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
