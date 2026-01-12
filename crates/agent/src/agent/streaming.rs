//! Streaming response processing with thinking tag support.

use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::llm::LlmInterface;
use super::types::{AgentEvent, AgentMessage, SessionState, ToolCall};
use super::tool_parser::{parse_tool_calls, remove_tool_calls_from_response};

/// Process a user message with streaming response.
///
/// Logic:
/// 1. Stream LLM response in real-time
/// 2. Detect tool calls during streaming
/// 3. If tool call detected:
///    - Execute tool
///    - Get final LLM response based on tool result
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

    // Get the stream from llm_interface
    let stream_result = tokio::task::block_in_place(move || {
        tokio::runtime::Handle::try_current()
            .unwrap()
            .block_on(llm_clone.chat_stream(&user_message))
    });

    match stream_result {
        Ok(stream) => {
            let converted_stream = async_stream::stream! {
                let mut stream = stream;
                let mut full_response = String::new();
                let mut buffer = String::new();
                let mut tool_calls_detected = false;
                let mut tool_calls: Vec<ToolCall> = Vec::new();
                let mut content_before_tools = String::new();

                // === PHASE 1: Stream and detect tool calls ===
                while let Some(result) = StreamExt::next(&mut stream).await {
                    match result {
                        Ok((text, _is_thinking)) => {
                            if text.is_empty() {
                                break;
                            }

                            full_response.push_str(&text);
                            buffer.push_str(&text);

                            // Check for tool calls in buffer
                            if let Some(tool_start) = buffer.find("<tool_calls>") {
                                // Found tool call start - emit content before it
                                let before_tool = &buffer[..tool_start];
                                if !before_tool.is_empty() {
                                    content_before_tools.push_str(before_tool);
                                    yield AgentEvent::content(before_tool.to_string());
                                }

                                // Look for tool call end
                                if let Some(tool_end) = buffer.find("</tool_calls>") {
                                    // Complete tool call found
                                    let tool_content = buffer[tool_start..tool_end + 13].to_string();
                                    buffer = buffer[tool_end + 13..].to_string();

                                    // Parse tool calls
                                    if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
                                        tool_calls = calls;
                                        tool_calls_detected = true;
                                    }
                                    break; // Exit streaming loop, we have tool calls to process
                                } else {
                                    // Incomplete tool call, wait for more data
                                    continue;
                                }
                            }

                            // No tool calls yet - stream the content immediately
                            if !tool_calls_detected && !buffer.contains("<tool_calls>") && !buffer.is_empty() {
                                content_before_tools.push_str(&buffer);
                                yield AgentEvent::content(buffer.clone());
                                buffer.clear();
                            }
                        }
                        Err(e) => {
                            yield AgentEvent::error(e.to_string());
                            break;
                        }
                    }
                }

                // Emit any remaining content that wasn't followed by tool calls
                if !tool_calls_detected && !buffer.is_empty() {
                    yield AgentEvent::content(buffer.clone());
                    content_before_tools.push_str(&buffer);
                }

                // Save initial response to memory
                let initial_msg = if tool_calls_detected {
                    AgentMessage::assistant_with_tools(&content_before_tools, tool_calls.clone())
                } else {
                    AgentMessage::assistant(&content_before_tools)
                };
                short_term_memory.write().await.push(initial_msg);

                // === PHASE 2: Handle tool calls if detected ===
                if tool_calls_detected {
                    // Execute all tool calls
                    for tool_call in &tool_calls {
                        yield AgentEvent::tool_call_start(&tool_call.name, tool_call.arguments.clone());

                        // Execute with retry logic for transient errors
                        let max_retries = 2u32;
                        let mut tool_result: std::result::Result<edge_ai_tools::ToolOutput, edge_ai_tools::ToolError> = Err(
                            edge_ai_tools::ToolError::Execution("Not executed".to_string())
                        );
                        let mut last_error = String::new();

                        for attempt in 0..=max_retries {
                            tool_result = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::try_current()
                                    .unwrap()
                                    .block_on(tools.execute(&tool_call.name, tool_call.arguments.clone()))
                            });

                            match &tool_result {
                                Ok(output) if output.success => break,
                                Err(e) => {
                                    last_error = e.to_string();
                                    let is_transient = last_error.contains("timeout")
                                        || last_error.contains("network")
                                        || last_error.contains("connection")
                                        || last_error.contains("unavailable");

                                    if is_transient && attempt < max_retries {
                                        let delay_ms = 100u64 * (2_u64.pow(attempt));
                                        tokio::task::block_in_place(|| {
                                            tokio::runtime::Handle::try_current()
                                                .unwrap()
                                                .block_on(tokio::time::sleep(std::time::Duration::from_millis(delay_ms)))
                                        });
                                        continue;
                                    }
                                }
                                _ => break,
                            }
                        }

                        match tool_result {
                            Ok(output) => {
                                let result_str = if output.success {
                                    serde_json::to_string(&output.data).unwrap_or_else(|_| "Success".to_string())
                                } else {
                                    output.error.clone().unwrap_or_else(|| "Error".to_string())
                                };
                                yield AgentEvent::tool_call_end(&tool_call.name, &result_str, output.success);

                                // Save tool result to memory
                                short_term_memory.write().await.push(
                                    AgentMessage::tool_result(&tool_call.name, &result_str)
                                );
                            }
                            Err(e) => {
                                let error_msg = format!("工具执行失败: {}", e);
                                yield AgentEvent::tool_call_end(&tool_call.name, &error_msg, false);

                                short_term_memory.write().await.push(
                                    AgentMessage::tool_result(&tool_call.name, &error_msg)
                                );
                            }
                        }
                    }

                    // Build conversation context for follow-up
                    let history = short_term_memory.read().await;
                    let conversation: Vec<String> = history.iter()
                        .take(50) // Increased context window for better conversation retention
                        .map(|msg| {
                            match msg.role.as_str() {
                                "user" => format!("User: {}", msg.content),
                                "assistant" => format!("Assistant: {}", msg.content),
                                "tool" => format!(
                                    "Tool[{}]: {}",
                                    msg.tool_call_name.as_ref().unwrap_or(&String::new()),
                                    msg.content
                                ),
                                _ => String::new(),
                            }
                        })
                        .collect();
                    drop(history);

                    // Build follow-up prompt
                    let conversation_text = conversation.join("\n");
                    let follow_up_prompt = format!(
                        "{}\n\nBased on the conversation above and the tool results, provide a helpful response to the user.",
                        conversation_text
                    );

                    // Get LLM's final interpretation
                    let final_response = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::try_current()
                            .unwrap()
                            .block_on(llm_interface.chat(&follow_up_prompt))
                    });

                    match final_response {
                        Ok(resp) => {
                            // Stream the final response chunk by chunk
                            let text = resp.text;
                            let chunk_size = 4usize;
                            for i in (0..text.len()).step_by(chunk_size) {
                                let end = (i + chunk_size).min(text.len());
                                let chunk = &text[i..end];
                                if !chunk.is_empty() {
                                    yield AgentEvent::content(chunk.to_string());
                                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                                }
                            }

                            // Save final response to memory
                            short_term_memory.write().await.push(AgentMessage::assistant(&text));
                        }
                        Err(e) => {
                            yield AgentEvent::error(format!("Failed to get final response: {}", e));
                        }
                    }
                }

                state.write().await.increment_messages();
                yield AgentEvent::end();
            };

            Ok(Box::pin(converted_stream))
        }
        Err(e) => Err(crate::error::AgentError::Llm(e.to_string())),
    }
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

    #[test]
    fn test_remove_tool_calls_from_response() {
        let response = "Here's the result <tool_calls><invoke name=\"test\"></invoke></tool_calls> done.";
        let cleaned = remove_tool_calls_from_response(response);

        assert!(!cleaned.contains("<tool_calls>"));
        assert!(!cleaned.contains("</tool_calls>"));
        assert!(cleaned.contains("done"));
    }
}
