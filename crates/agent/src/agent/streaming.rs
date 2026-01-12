//! Streaming response processing with thinking tag support.

use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::llm::LlmInterface;
use super::types::{AgentEvent, AgentMessage, SessionState};
use super::tool_parser::{parse_tool_call_json, remove_tool_calls_from_response};

/// Process a user message with streaming response.
///
/// Returns a stream of AgentEvent including thinking, content, tool calls, etc.
pub fn process_stream_events(
    llm_interface: Arc<LlmInterface>,
    short_term_memory: Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    state: Arc<RwLock<SessionState>>,
    tools: Arc<edge_ai_tools::ToolRegistry>,
    user_message: &str,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    // Get the stream from llm_interface
    let stream_result = tokio::task::block_in_place(move || {
        tokio::runtime::Handle::try_current()
            .unwrap()
            .block_on(llm_interface.chat_stream(user_message))
    });

    match stream_result {
        Ok(stream) => {
            // Convert the stream to yield AgentEvent with tool call detection
            let converted_stream = async_stream::stream! {
                let mut full_response = String::new();
                let mut accumulated_thinking = String::new();
                let mut stream = stream;
                let mut buffer = String::new();
                let mut in_thinking_block = false;

                while let Some(result) = StreamExt::next(&mut stream).await {
                    match result {
                        Ok((text, is_thinking_field)) => {
                            // Empty string marks end of stream
                            if text.is_empty() {
                                break;
                            }

                            full_response.push_str(&text);
                            buffer.push_str(&text);

                            // Process buffer for various tags in order of priority:
                            // 1. Tool calls <tool_calls>...</tool_calls>
                            // 2. Thinking tags <think>...</think> (DeepSeek R1, etc.)
                            // 3. Backend thinking field (qwen3-vl)

                            // First, check for tool calls
                            while let Some(tool_start) = buffer.find("<tool_calls>") {
                                let before_tool = &buffer[..tool_start];

                                // Process any content before the tool call
                                if !before_tool.is_empty() {
                                    let processed = process_thinking_tags(before_tool, &mut in_thinking_block);
                                    for event in processed {
                                        if let AgentEvent::Thinking { content } = &event {
                                            accumulated_thinking.push_str(content);
                                        }
                                        yield event;
                                    }
                                }

                                // Look for tool call end
                                if let Some(tool_end) = buffer.find("</tool_calls>") {
                                    let tool_content = &buffer[tool_start + 12..tool_end];

                                    // Parse the tool call
                                    if let Ok((tool_name, args)) = parse_tool_call_json(tool_content) {
                                        yield AgentEvent::tool_call_start(&tool_name, args.clone());

                                        // Execute the tool
                                        let tool_result = tokio::task::block_in_place(|| {
                                            tokio::runtime::Handle::try_current()
                                                .unwrap()
                                                .block_on(tools.execute(&tool_name, args))
                                        });

                                        match tool_result {
                                            Ok(output) => {
                                                let result_str = if output.success {
                                                    serde_json::to_string(&output.data).unwrap_or_default()
                                                } else {
                                                    output.error.clone().unwrap_or_default()
                                                };
                                                yield AgentEvent::tool_call_end(&tool_name, result_str, output.success);
                                            }
                                            Err(e) => {
                                                yield AgentEvent::tool_call_end(&tool_name, e.to_string(), false);
                                            }
                                        }
                                    }

                                    // Remove processed content from buffer
                                    buffer = buffer[tool_end + 13..].to_string();
                                } else {
                                    // Incomplete tool call, wait for more data
                                    break;
                                }
                            }

                            // If no tool calls found, process thinking tags and regular content
                            if !buffer.contains("<tool_calls>") {
                                // Check for think tags (DeepSeek R1 style)
                                if buffer.contains("<think>") || buffer.contains("</think>") || in_thinking_block {
                                    let processed = process_thinking_tags(&buffer, &mut in_thinking_block);
                                    for event in processed {
                                        if let AgentEvent::Thinking { content } = &event {
                                            accumulated_thinking.push_str(content);
                                        }
                                        yield event;
                                    }
                                    buffer.clear();
                                } else {
                                    // No special tags, use backend is_thinking_field flag
                                    if !buffer.is_empty() {
                                        if is_thinking_field {
                                            accumulated_thinking.push_str(&buffer);
                                            yield AgentEvent::thinking(buffer.clone());
                                        } else {
                                            yield AgentEvent::content(buffer.clone());
                                        }
                                        buffer.clear();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Chat error: {}", e);
                            yield AgentEvent::error(e.to_string());
                            break;
                        }
                    }
                }

                // Yield any remaining content
                if !buffer.is_empty() {
                    let processed = process_thinking_tags(&buffer, &mut in_thinking_block);
                    for event in processed {
                        yield event;
                    }
                }

                // Add complete response to memory (without tool calls)
                let clean_response = remove_tool_calls_from_response(&full_response);
                if !clean_response.is_empty() {
                    let message = if !accumulated_thinking.is_empty() {
                        AgentMessage::assistant_with_thinking(&clean_response, &accumulated_thinking)
                    } else {
                        AgentMessage::assistant(clean_response)
                    };
                    short_term_memory.write().await.push(message);
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

/// Process thinking tags and return AgentEvents.
///
/// Handles models that output thinking content in special tags like <think>...</think>.
/// This is common in reasoning models like DeepSeek R1.
pub fn process_thinking_tags(text: &str, in_thinking_block: &mut bool) -> Vec<AgentEvent> {
    let mut events = Vec::new();

    let think_open = "<think>";
    let think_close = "</think>";

    let mut remaining = text;
    let mut current_in_thinking = *in_thinking_block;

    loop {
        // Find next occurrence of open or close tag
        let open_pos = remaining.find(think_open);
        let close_pos = remaining.find(think_close);

        match (open_pos, close_pos) {
            (Some(o), Some(c)) if o < c => {
                // Opening tag comes first - emit content before it as content
                if o > 0 {
                    let before = &remaining[..o];
                    if !before.trim().is_empty() {
                        if current_in_thinking {
                            events.push(AgentEvent::thinking(before.trim().to_string()));
                        } else {
                            events.push(AgentEvent::content(before.trim().to_string()));
                        }
                    }
                }
                // Extract thinking content
                let think_content = &remaining[o + think_open.len()..c];
                if !think_content.trim().is_empty() {
                    events.push(AgentEvent::thinking(think_content.trim().to_string()));
                }
                remaining = &remaining[c + think_close.len()..];
                current_in_thinking = false;
                *in_thinking_block = false;
            }
            (Some(o), None) => {
                // Only opening tag - emit content before it
                if o > 0 {
                    let before = &remaining[..o];
                    if !before.trim().is_empty() {
                        if current_in_thinking {
                            events.push(AgentEvent::thinking(before.trim().to_string()));
                        } else {
                            events.push(AgentEvent::content(before.trim().to_string()));
                        }
                    }
                }
                // Rest is thinking content (incomplete)
                let think_content = &remaining[o + think_open.len()..];
                if !think_content.trim().is_empty() {
                    events.push(AgentEvent::thinking(think_content.trim().to_string()));
                }
                *in_thinking_block = true;
                break;
            }
            (None, Some(c)) => {
                // Only closing tag - end of thinking block
                let before = &remaining[..c];
                if !before.trim().is_empty() {
                    events.push(AgentEvent::thinking(before.trim().to_string()));
                }
                remaining = &remaining[c + think_close.len()..];
                current_in_thinking = false;
                *in_thinking_block = false;
            }
            (None, None) => {
                // No more tags - emit remaining based on current state
                if !remaining.trim().is_empty() {
                    if current_in_thinking {
                        events.push(AgentEvent::thinking(remaining.trim().to_string()));
                    } else {
                        events.push(AgentEvent::content(remaining.trim().to_string()));
                    }
                }
                *in_thinking_block = current_in_thinking;
                break;
            }
            _ => break,
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_thinking_tags_complete() {
        let text = "Regular text <think>thinking content</think> more text";
        let events = process_thinking_tags(text, &mut false);

        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], AgentEvent::Content { .. }));
        assert!(matches!(events[1], AgentEvent::Thinking { .. }));
        assert!(matches!(events[2], AgentEvent::Content { .. }));
    }

    #[test]
    fn test_process_thinking_tags_incomplete() {
        let text = "Text <think>incomplete thinking";
        let mut in_block = false;
        let events = process_thinking_tags(text, &mut in_block);

        assert_eq!(events.len(), 2);
        assert!(in_block); // Should be true after processing incomplete tag
    }

    #[test]
    fn test_process_thinking_tags_only_content() {
        let text = "Just regular content with no tags";
        let events = process_thinking_tags(text, &mut false);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AgentEvent::Content { .. }));
    }

    #[test]
    fn test_process_thinking_tags_in_block() {
        let text = "continue thinking";
        let mut in_block = true;
        let events = process_thinking_tags(text, &mut in_block);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AgentEvent::Thinking { .. }));
    }
}
