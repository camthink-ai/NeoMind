//! Multimodal (text + images) streaming response processing.
//!
//! Contains `process_multimodal_stream_events` for LLM interactions that
//! include images alongside text, with tool calling support.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use futures::{Stream, StreamExt};

use super::context::{build_context_window_with_summary, ToolExecutionResult};
use super::dedup::deduplicate_tool_results;
use super::intent::build_list_only_dead_end_prompt;
use super::next_chunk_or_timeout;
use super::resolve::resolve_cached_arguments;
use super::result_format::format_tool_results;
use super::sanitize::sanitize_tool_result_for_prompt;
use super::stream_core::StreamSafeguards;
use super::tool_detect::detect_json_tool_calls;
use super::tool_exec::execute_tool_with_retry;
use crate::agent::tool_parser::{
    is_degenerate_fence_only_output, parse_tool_calls, remove_tool_calls_from_response,
};
use crate::agent::types::{
    AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, ToolCall,
};
use crate::error::{NeoMindError, Result};
use crate::llm::LlmInterface;

/// Process multimodal message with configurable safeguards.
#[allow(clippy::too_many_arguments)]
pub async fn process_multimodal_stream_events_with_safeguards(
    llm_interface: Arc<LlmInterface>,
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<crate::toolkit::ToolRegistry>,
    user_message: &str,
    images: Vec<String>,
    safeguards: StreamSafeguards,
    conversation_summary: Option<String>,
    summary_up_to_index: Option<u64>,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    use neomind_core::ContentPart;

    let user_message = user_message.to_string();

    // Build multimodal message content with images
    let mut parts = vec![ContentPart::text(&user_message)];

    // Add images as ContentPart
    for image_data in &images {
        if let Some(parsed) = crate::image_utils::parse_image_data(image_data) {
            parts.push(ContentPart::image_base64(parsed.base64, parsed.mime_type));
        }
    }

    // Get conversation history
    let state_guard = internal_state.read().await;
    let history_messages = state_guard.memory.clone();
    drop(state_guard);

    // Build context window — measure actual prompt overhead instead of guessing
    let max_context = llm_interface.max_context_length().await;
    let prompt_overhead = llm_interface.estimate_prompt_overhead_tokens().await;
    let effective_max = max_context
        .saturating_sub(prompt_overhead)
        .saturating_sub(1024)
        .max((max_context * 20) / 100);

    let history_for_llm: Vec<neomind_core::Message> = build_context_window_with_summary(
        &history_messages,
        effective_max,
        conversation_summary.as_deref(),
        summary_up_to_index,
    )
    .iter()
    .map(|msg| msg.to_core())
    .collect::<Vec<_>>();

    tracing::debug!(
        "Passing {} messages from history to LLM (multimodal)",
        history_for_llm.len()
    );

    // Create multimodal user message
    let multimodal_user_msg = neomind_core::Message::new(
        neomind_core::MessageRole::User,
        neomind_core::Content::Parts(parts),
    );

    // Use regular multimodal chat (with thinking enabled)
    // Thinking helps the model analyze images more thoroughly
    let stream_result = llm_interface
        .chat_stream_multimodal_with_history(multimodal_user_msg, &history_for_llm)
        .await;

    let stream = stream_result.map_err(|e| NeoMindError::Llm(e.to_string()))?;

    // Check if images are present (before moving images)
    let has_images = !images.is_empty();

    // Extract base64 data for caching before images are consumed.
    // Use the shared parse_image_data helper instead of fragile split(',')
    // so non-standard data URL formats are handled consistently.
    let image_base64_list: Vec<String> = images
        .iter()
        .filter_map(|data_url| {
            crate::image_utils::parse_image_data(data_url).map(|p| p.base64.to_string())
        })
        .collect();

    // Store user message in history with images
    // Convert the image strings to AgentMessageImage
    let user_images: Vec<AgentMessageImage> = images
        .into_iter()
        .map(|data_url| {
            let mime_type =
                crate::image_utils::parse_image_data(&data_url).map(|p| p.mime_type.to_string());
            AgentMessageImage {
                data: data_url,
                mime_type,
            }
        })
        .collect();

    let user_msg = AgentMessage::user_with_images(&user_message, user_images);
    internal_state.write().await.push_message(user_msg);

    // Cache user-uploaded images so tools can reference them via $cached:user_image
    if !image_base64_list.is_empty() {
        let mut state = internal_state.write().await;
        for (i, base64_data) in image_base64_list.iter().enumerate() {
            let cache_key = if i == 0 {
                "user_image".to_string()
            } else {
                format!("user_image_{}", i)
            };
            state.large_data_cache.store(&cache_key, base64_data);
        }
    }

    Ok(Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut buffer = String::new();
        let mut tool_calls_detected = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut content_before_tools = String::new();

        // === SKILL CONTEXT: Clear transient skill context from previous turn ===
        llm_interface.clear_skill_context().await;

        let stream_start = Instant::now();
        let mut last_event_time = Instant::now();

        // Simple progress event (only for images)
        if has_images {
            yield AgentEvent::progress("正在分析图像...", "analyzing", 0);
            last_event_time = Instant::now();
        }

        // Stream the response — bounded next(): a zero-chunk stall force-breaks
        // the round instead of hanging (same bug fixed in stream_core).
        while let Some(result) = next_chunk_or_timeout(&mut stream, safeguards.max_stream_duration).await {
            let elapsed = stream_start.elapsed();

            // [cancellable] Same interrupt check as stream_core — the
            // multimodal path previously had ZERO references to the
            // interrupt signal, so the Stop button was dead for image
            // chats (streams ran to natural end or the 2400s bound).
            let is_interrupted = safeguards
                .interrupt_signal
                .as_ref()
                .map(|rx| *rx.borrow())
                .unwrap_or(false);
            if is_interrupted {
                tracing::info!("Multimodal stream interrupted by user");
                yield AgentEvent::content("\n\n[Interrupted]");
                yield AgentEvent::end();
                return;
            }

            if elapsed > safeguards.max_stream_duration {
                tracing::warn!("Stream timeout ({:?} elapsed)", elapsed);
                yield AgentEvent::error(format!("Request timeout ({:.1}s elapsed)", elapsed.as_secs_f64()));
                break;
            }

            // Heartbeat
            if last_event_time.elapsed() > safeguards.heartbeat_interval {
                yield AgentEvent::heartbeat();
                last_event_time = Instant::now();
            }

            match result {
                Ok((text, is_thinking)) => {
                    if text.is_empty() {
                        continue;
                    }

                    if is_thinking {
                        yield AgentEvent::thinking(text.clone());
                        last_event_time = Instant::now();
                        continue;
                    }

                    buffer.push_str(&text);
                    last_event_time = Instant::now();

                    // Check for tool calls in buffer
                    let json_tool_check = detect_json_tool_calls(&buffer);
                    if let Some((json_start, tool_json, _remaining)) = json_tool_check {
                        let before_tool = &buffer[..json_start];
                        if !before_tool.is_empty() {
                            content_before_tools.push_str(before_tool);
                            yield AgentEvent::content(before_tool);
                        }

                        if let Ok((_, calls)) = parse_tool_calls(&tool_json) {
                            tool_calls_detected = true;
                            tool_calls.extend(calls);
                        }

                        // Discard remaining hallucinated content after embedded tool calls
                        buffer.clear();
                    } else {
                        // No JSON tool calls detected - check for XML format
                        if let Some(tool_start) = buffer.find("<tool_calls>") {
                            let before_tool = &buffer[..tool_start];
                            if !before_tool.is_empty() {
                                content_before_tools.push_str(before_tool);
                                yield AgentEvent::content(before_tool);
                            }

                            if let Some(tool_end) = buffer.find("</tool_calls>") {
                                let tool_content = buffer[tool_start..tool_end + 13].to_string();
                                // Discard remaining hallucinated content after XML tool calls
                                buffer.clear();

                                if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
                                    tool_calls_detected = true;
                                    tool_calls.extend(calls);
                                }
                            }
                        } else {
                            // No tool calls detected - yield content immediately for real-time streaming
                            if !text.is_empty() {
                                yield AgentEvent::content(text.clone());
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    yield AgentEvent::error(format!("Stream error: {}", e));
                    // Save partial response on error to maintain conversation context
                    if !buffer.is_empty() || !content_before_tools.is_empty() {
                        let partial_content = if content_before_tools.is_empty() {
                            buffer.clone()
                        } else {
                            content_before_tools.clone()
                        };
                        let partial_msg = AgentMessage::assistant(&partial_content);
                        internal_state.write().await.push_message(partial_msg);
                        tracing::debug!("Saved partial multimodal response on error: {} chars", partial_content.len());
                    }
                    break;
                }
            }
        }

        // Handle tool calls if detected
        if tool_calls_detected {
            tracing::debug!("Tool calls detected in multimodal response, executing {} tools", tool_calls.len());

            let tool_calls_to_execute = tool_calls.clone();

            // Resolve cached data references in tool arguments
            let (large_cache, cache) = {
                let state = internal_state.read().await;
                (state.large_data_cache.clone(), state.tool_result_cache.clone())
            };

            // Execute tool calls with bounded concurrency (max 6 parallel)
            let tool_inputs: Vec<(String, serde_json::Value)> = tool_calls_to_execute
                .iter()
                .map(|tc| {
                    (
                        tc.name.clone(),
                        resolve_cached_arguments(&tc.arguments, &large_cache, &tc.name),
                    )
                })
                .collect();

            let tool_futures = futures::stream::iter(tool_inputs.into_iter().map(|(name, arguments)| {
                let tools_clone = tools.clone();
                let cache_clone = cache.clone();

                async move {
                    (name.clone(), ToolExecutionResult {
                        _name: name.clone(),
                        arguments: arguments.clone(),
                        result: execute_tool_with_retry(&tools_clone, &cache_clone, &name, arguments.clone()).await,
                    })
                }
            })).buffer_unordered(6);

            // [keep-alive] Same as stream_core: tool execution can run for
            // minutes and the stream is otherwise silent during this phase,
            // so WS listeners see an event gap and kill the turn. Yield
            // heartbeats on an independent timer while the tool batch runs.
            let collect_fut = tool_futures.collect::<Vec<_>>();
            tokio::pin!(collect_fut);
            let mut tool_heartbeat = tokio::time::interval(safeguards.heartbeat_interval);
            tool_heartbeat.tick().await; // consume the immediate first tick
            let tool_results_executed: Vec<_> = loop {
                tokio::select! {
                    _ = tool_heartbeat.tick() => {
                        // [cancellable] surf the interrupt between heartbeats
                        // so a long tool batch doesn't defer cancel for its
                        // full duration (up to the shell timeout).
                        let interrupted = safeguards
                            .interrupt_signal
                            .as_ref()
                            .map(|rx| *rx.borrow())
                            .unwrap_or(false);
                        if interrupted {
                            tracing::info!("Multimodal stream interrupted during tool batch");
                            yield AgentEvent::content("\n\n[Interrupted]");
                            yield AgentEvent::end();
                            return;
                        }
                        yield AgentEvent::heartbeat();
                    }
                    done = &mut collect_fut => break done,
                }
            };

            // Process results
            let mut tool_calls_with_results: Vec<ToolCall> = Vec::new();
            let mut tool_call_results: Vec<(String, String)> = Vec::new();

            for (name, execution) in tool_results_executed {
                // Use arguments from the execution result (preserves per-call arguments for same-name tools)
                let exec_arguments = execution.arguments.clone();
                yield AgentEvent::tool_call_start(&name, exec_arguments.clone());

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

                        // Slim large strings (image data URLs, big base64) inline
                        // BEFORE display/sanitize: walks JSON, stores each large
                        // value into cache under a deterministic key, replaces
                        // value with a one-line `$cached:` summary + action hint.
                        // See stream_core.rs main path for full rationale.
                        let slimmed_str = {
                            let mut state = internal_state.write().await;
                            match serde_json::from_str::<serde_json::Value>(&result_str) {
                                Ok(mut v) => {
                                    let n = state.large_data_cache.slim_large_strings_in_json(&mut v, &name);
                                    if n > 0 {
                                        tracing::debug!(
                                            tool = %name,
                                            slimmed_values = n,
                                            "Slimmed large strings from tool result into cache"
                                        );
                                    }
                                    serde_json::to_string(&v)
                                        .unwrap_or_else(|_| result_str.clone())
                                }
                                Err(_) => result_str.clone(),
                            }
                        };

                        let display_str = sanitize_tool_result_for_prompt(&slimmed_str);

                        tool_calls_with_results.push(ToolCall {
                            name: name.clone(),
                            id: String::new(),
                            arguments: exec_arguments,
                            result: Some(result_value.clone()),
                            round: Some(1),
                        });

                        yield AgentEvent::tool_call_end(&name, &display_str, output.success);

                        tool_call_results.push((name.clone(), slimmed_str));
                    }
                    Err(e) => {
                        let error_msg = format!("Tool execution failed: {}", e);
                        let error_value = serde_json::json!({"error": error_msg});

                        tool_calls_with_results.push(ToolCall {
                            name: name.clone(),
                            id: String::new(),
                            arguments: exec_arguments,
                            result: Some(error_value.clone()),
                            round: Some(1),
                        });

                        yield AgentEvent::tool_call_end(&name, &error_msg, false);
                        tool_call_results.push((name.clone(), error_msg));
                    }
                }
            }

            // Save assistant message with tool calls
            let response_to_save = if content_before_tools.is_empty() {
                String::new()
            } else {
                content_before_tools.clone()
            };

            let initial_msg = AgentMessage::assistant_with_tools(&response_to_save, tool_calls_with_results.clone());
            internal_state.write().await.push_message(initial_msg);

            // Add tool result messages.
            // `tool_call_results` already contains the cache summaries (or
            // passthrough for small results) computed during the execute loop
            // above — no second `cache.store` needed here.
            for (tool_name, result_str) in &tool_call_results {
                if tool_name == "skill" {
                    let skill_id = crate::llm::extract_skill_id_from_result(result_str);
                    llm_interface.set_skill_context(skill_id, result_str.clone()).await;
                } else {
                    let mut state = internal_state.write().await;
                    let tool_result_msg = AgentMessage::tool_result(tool_name, result_str);
                    state.push_message(tool_result_msg);
                }
            }

            // === "LIST-ONLY DEAD END" DETECTION (multimodal path) ===
            // Symmetric with stream_core.rs: if the user asked for an action but
            // only read-only tools executed, fire ONE more tool round with a
            // forced continuation prompt before falling through to the summary.
            // Without this, multimodal users silently hit the dead end that the
            // text path was fixed to handle.
            let executed_commands: Vec<String> = tool_calls_to_execute
                .iter()
                .filter(|tc| tc.name == "shell")
                .filter_map(|tc| {
                    tc.arguments.get("command").and_then(|v| v.as_str()).map(String::from)
                })
                .collect();
            let commands_ref: Vec<&str> = executed_commands.iter().map(|s| s.as_str()).collect();

            if let Some(dead_end_prompt) = build_list_only_dead_end_prompt(
                &user_message,
                &commands_ref,
                &tool_call_results,
            ) {
                tracing::info!("Multimodal dead-end: invoking one continuation round");
                yield AgentEvent::progress("Continuing action...", "continuing", 0);

                // Build history for the continuation LLM call (text-only; user image
                // is already in history as a prior turn)
                let cont_history: Vec<neomind_core::Message> = {
                    let state_guard = internal_state.read().await;
                    let compacted = super::super::compact_tool_results(&state_guard.memory, 2);
                    compacted.iter().map(|msg| msg.to_core()).collect()
                };

                let cont_stream_result = llm_interface
                    .chat_stream_with_history_thinking(&dead_end_prompt, &cont_history, None)
                    .await;

                if let Ok(cont_stream) = cont_stream_result {
                    // Collect the continuation response — buffer for tool calls
                    let mut cont_buffer = String::new();
                    let mut cont_stream = Box::pin(cont_stream);
                    while let Some(chunk) =
                        next_chunk_or_timeout(&mut cont_stream, safeguards.max_stream_duration).await
                    {
                        match chunk {
                            Ok((text, _)) => cont_buffer.push_str(&text),
                            Err(_) => break,
                        }
                    }

                    // Parse tool calls from the continuation response
                    let cont_tool_calls = parse_tool_calls(&cont_buffer)
                        .map(|(_, calls)| calls)
                        .unwrap_or_default();

                    if !cont_tool_calls.is_empty() {
                        // Execute continuation tool calls with the same pattern as above
                        let (large_cache_cont, cache_cont) = {
                            let state = internal_state.read().await;
                            (state.large_data_cache.clone(), state.tool_result_cache.clone())
                        };
                        let cont_inputs: Vec<(String, serde_json::Value)> = cont_tool_calls
                            .iter()
                            .map(|tc| {
                                (
                                    tc.name.clone(),
                                    resolve_cached_arguments(&tc.arguments, &large_cache_cont, &tc.name),
                                )
                            })
                            .collect();
                        let cont_futures = futures::stream::iter(cont_inputs.into_iter().map(|(name, arguments)| {
                            let tools_clone = tools.clone();
                            let cache_clone = cache_cont.clone();
                            async move {
                                (name.clone(), ToolExecutionResult {
                                    _name: name.clone(),
                                    arguments: arguments.clone(),
                                    result: execute_tool_with_retry(&tools_clone, &cache_clone, &name, arguments.clone()).await,
                                })
                            }
                        })).buffer_unordered(6);
                        let cont_results: Vec<_> = cont_futures.collect().await;

                        // Save continuation assistant message + tool results to history
                        let cont_msg = AgentMessage::assistant_with_tools(
                            "",
                            cont_tool_calls.iter().map(|tc| ToolCall {
                                name: tc.name.clone(),
                                id: String::new(),
                                arguments: tc.arguments.clone(),
                                result: None,
                                round: Some(2),
                            }).collect(),
                        );
                        internal_state.write().await.push_message(cont_msg);

                        for (name, execution) in cont_results {
                            yield AgentEvent::tool_call_start(&name, execution.arguments.clone());
                            match execution.result {
                                Ok(output) => {
                                    let result_str = if output.success {
                                        serde_json::to_string(&output.data).unwrap_or_else(|_| "Success".to_string())
                                    } else {
                                        output.error.clone().unwrap_or_else(|| "Error".to_string())
                                    };
                                    // Slim large strings inline (see main path above).
                                    let slimmed_str = {
                                        let mut state = internal_state.write().await;
                                        match serde_json::from_str::<serde_json::Value>(&result_str) {
                                            Ok(mut v) => {
                                                state.large_data_cache.slim_large_strings_in_json(&mut v, &name);
                                                serde_json::to_string(&v).unwrap_or_else(|_| result_str.clone())
                                            }
                                            Err(_) => result_str.clone(),
                                        }
                                    };
                                    let display_str = sanitize_tool_result_for_prompt(&slimmed_str);
                                    yield AgentEvent::tool_call_end(&name, &display_str, output.success);
                                    let mut state = internal_state.write().await;
                                    state.push_message(AgentMessage::tool_result(&name, &slimmed_str));
                                    tool_call_results.push((name.clone(), slimmed_str));
                                }
                                Err(e) => {
                                    let err_msg = format!("Tool execution failed: {}", e);
                                    yield AgentEvent::tool_call_end(&name, &err_msg, false);
                                    tool_call_results.push((name.clone(), err_msg));
                                }
                            }
                        }
                    }
                }
            }

            // Summary fallback: ask LLM to summarize tool results (no tools, no thinking)
            // This avoids dumping raw tool JSON to the user
            let summary_history: Vec<neomind_core::Message> = {
                let state_guard = internal_state.read().await;
                let compacted = super::super::compact_tool_results(&state_guard.memory, 2);
                compacted.iter().map(|msg| msg.to_core()).collect()
            };

            let summary_prompt = "Based on the tool execution results in the conversation above, \
                provide a concise analysis and summary. Do NOT output any tool calls — \
                give a direct text response to the user's question.";

            let mut final_content = String::new();
            let summary_result = llm_interface.chat_stream_summary(
                summary_prompt,
                &summary_history,
            ).await;

            match summary_result {
                Ok(stream) => {
                    let mut pin = Box::pin(stream);
                    while let Some(chunk) =
                        next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                    {
                        match chunk {
                            Ok((text, _)) => {
                                final_content.push_str(&text);
                                yield AgentEvent::content(text);
                            }
                            Err(e) => {
                                tracing::error!("Multimodal summary stream error: {}", e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Multimodal summary call failed: {}", e);
                }
            }

            // Fallback to formatted tool results if summary is empty OR degenerate
            // (e.g. DeepSeek emitting just "```" as the summary).
            if is_degenerate_fence_only_output(&final_content) {
                let deduped_results = deduplicate_tool_results(&tool_call_results);
                let formatted = format_tool_results(&deduped_results);
                final_content = formatted.clone();
                yield AgentEvent::content(formatted);
            }

            // Save the final content
            {
                let mut state = internal_state.write().await;
                if let Some(last_msg) = state.memory.last_mut() {
                    if last_msg.role == "assistant" && last_msg.tool_calls.is_some() {
                        last_msg.content = final_content.into();
                    } else {
                        let final_msg = AgentMessage::assistant(&final_content);
                        state.memory.push(final_msg);
                    }
                } else {
                    let final_msg = AgentMessage::assistant(&final_content);
                    state.memory.push(final_msg);
                }
            }

            tracing::debug!("Multimodal tool execution complete with summary");
        } else {
            // No tool calls - save response directly
            let raw_response = if buffer.is_empty() {
                String::new()
            } else {
                buffer.clone()
            };

            // Detect degenerate fence-only output (e.g. DeepSeek "```") and
            // substitute a safe non-empty fallback so the user/judge never sees
            // a content-less reply. The streamed "```" is already out, but the
            // persisted assistant_message uses this cleaned value.
            let degenerate = is_degenerate_fence_only_output(&raw_response);
            let response_to_save = if degenerate {
                tracing::warn!(
                    orig_len = raw_response.len(),
                    "Multimodal stream produced degenerate fence-only output, substituting fallback"
                );
                let fallback = "Sorry, the model produced no usable response. Please retry."
                    .to_string();
                yield AgentEvent::content(fallback.clone());
                fallback
            } else {
                // Clean any embedded tool call JSON from response
                remove_tool_calls_from_response(&raw_response)
            };

            let initial_msg = AgentMessage::assistant(&response_to_save);
            internal_state.write().await.push_message(initial_msg);

            // Yield any remaining content (skip when degenerate — already yielded fallback)
            if !degenerate && !buffer.is_empty() {
                yield AgentEvent::content(buffer.clone());
            }
        }

        let pt = llm_interface.take_last_prompt_tokens().await;
        match pt {
            Some(t) => yield AgentEvent::end_with_tokens(t),
            None => yield AgentEvent::end(),
        }
    }))
}
