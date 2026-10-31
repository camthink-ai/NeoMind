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
use super::resolve::resolve_cached_arguments;
use super::result_format::format_tool_results;
use super::sanitize::sanitize_tool_result_for_prompt;
use super::stream_core::StreamSafeguards;
use super::tool_exec::execute_tool_before_deadline;
use crate::agent::tool_parser::native_tool_calls;
use crate::agent::types::{
    AgentEvent, AgentInternalState, AgentMessage, AgentMessageImage, ToolCall,
};
use crate::error::{NeoMindError, Result};
use crate::llm::LlmInterface;
use crate::llm::StreamEvent;

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

    let turn_started_at = Instant::now();
    let turn_wall_clock_budget = std::time::Duration::from_secs(
        neomind_storage::AgentDefaults::get()
            .chat_turn_timeout_secs
            .max(1),
    );
    let turn_deadline = turn_started_at + turn_wall_clock_budget;
    // Clear before the first model request, not when its stream is consumed.
    llm_interface.clear_skill_context().await;
    let turn_context =
        super::context::TurnContext::new(conversation_summary.clone(), summary_up_to_index);
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
    let mut history_messages = state_guard.memory.clone();
    drop(state_guard);

    // === CHAT HISTORY DEPTH — same cap as the text-streaming path ===
    let drained = crate::agent::apply_chat_history_depth(&mut history_messages);

    // [summary alignment] shift the full-history summary index into the
    // drained window's index space — see stream_core's identical block.
    let summary_up_to_index =
        crate::agent::adjust_summary_index_after_drain(summary_up_to_index, drained);

    // [image budget] keep images only on the most recent image-bearing user
    // turns; the current turn's images are pushed to memory after this.
    crate::agent::strip_stale_images(&mut history_messages);

    // Build context window — measure actual prompt overhead instead of guessing.
    // Same budget helper as the text path: the old inline version re-inflated
    // the budget with a 20%-of-window floor AFTER subtracting the overhead,
    // constructing overflowing prompts on 8K-class models (the exact bug
    // fixed in stream_core — this is its multimodal twin, reached by every
    // image chat turn).
    let max_context = llm_interface.max_context_length().await;
    let prompt_overhead = llm_interface.estimate_prompt_overhead_tokens().await;
    let effective_max = super::stream_core::effective_history_budget(max_context, prompt_overhead);

    let history_for_llm: Vec<neomind_core::Message> = {
        let window = build_context_window_with_summary(
            &history_messages,
            effective_max,
            conversation_summary.as_deref(),
            summary_up_to_index,
        );
        window
    }
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

    // Image delegation (text-only active model + vision companion): the
    // LLM receives the companion's combined prompt as PLAIN TEXT — no image
    // parts to send to a model that cannot see them — while the history
    // push below keeps the ORIGINAL message + attachments for display.
    let llm_user_msg = match llm_interface.take_image_delegation_prompt().await {
        Some(prompt) => neomind_core::Message::new(
            neomind_core::MessageRole::User,
            neomind_core::Content::Text(prompt),
        ),
        None => multimodal_user_msg,
    };

    // Use regular multimodal chat (with thinking enabled)
    // Thinking helps the model analyze images more thoroughly
    let stream_result = super::request_before_deadline(
        llm_interface.chat_stream_multimodal_with_history(llm_user_msg, &history_for_llm),
        turn_deadline,
        safeguards.interrupt_signal.as_ref(),
    )
    .await;

    let stream = stream_result.map_err(|e| NeoMindError::Llm(e.to_string()))?;

    // Check if images are present (before moving images)
    let has_images = !images.is_empty();

    // [copy-avoidance] Cache the images from a BORROW first, then move
    // them into the history message. The old order (clone the whole Vec to
    // keep a copy alive across the move, push, then cache from the clone)
    // duplicated every image's full base64 payload once per turn.
    //
    // Cache user-uploaded images so tools can reference them via
    // $cached:user_image (or the auto-inject on image-shaped args). MUST use
    // store_user_image — the regular store() passes anything under 32KB
    // through uncached, which silently killed tool access to compressed
    // user images (2026-08-18).
    if !images.is_empty() {
        let mut state = internal_state.write().await;
        for (i, url) in images.iter().enumerate() {
            let cache_key = if i == 0 {
                "user_image".to_string()
            } else {
                format!("user_image_{}", i)
            };
            state.large_data_cache.store_user_image(&cache_key, url);
        }
    }

    // Convert the image strings to AgentMessageImage (mime parsed with the
    // shared parse_image_data helper — non-standard data URLs handled
    // consistently) and store the user message in history with them.
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

    Ok(Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut buffer = String::new();
        let mut thinking_content = String::new();
        let mut tool_calls_detected = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut content_before_tools = String::new();

        let stream_start = Instant::now();
        let mut last_event_time = Instant::now();

        // Simple progress event (only for images)
        if has_images {
            yield AgentEvent::progress("正在分析图像...", "analyzing", 0);
            last_event_time = Instant::now();
        }

        // Stream the response — bounded next(): a zero-chunk stall force-breaks
        // the round instead of hanging (same bug fixed in stream_core).
        while let Some(result) = super::next_before_deadline(&mut stream, safeguards.max_stream_duration, turn_deadline, safeguards.interrupt_signal.as_ref()).await {
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
                Ok(event) => {
                    let (text, is_thinking) = match event {
                        StreamEvent::ToolCalls(values) => {
                            let calls = native_tool_calls(values);
                            tool_calls_detected |= !calls.is_empty();
                            content_before_tools = buffer.clone();
                            for call in calls {
                                if !tool_calls.iter().any(|prev| prev.id == call.id) { tool_calls.push(call); }
                            }
                            continue;
                        }
                        StreamEvent::Text(text) => (text, false),
                        StreamEvent::Thinking(text) => (text, true),
                    };
                    if text.is_empty() {
                        continue;
                    }

                    if is_thinking {
                        thinking_content.push_str(&text);
                        yield AgentEvent::thinking(text.clone());
                        last_event_time = Instant::now();
                        continue;
                    }

                    buffer.push_str(&text);
                    last_event_time = Instant::now();

                    yield AgentEvent::content(text);
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    yield AgentEvent::warning(format!("Stream interrupted: {}. Recovering available results.", e));
                    // Finalization below persists the partial response once.
                    break;
                }
            }
        }

        if safeguards.interrupt_signal.as_ref().is_some_and(|rx| *rx.borrow()) {
            if !buffer.is_empty() || !thinking_content.is_empty() {
                internal_state.write().await.push_message(
                    AgentMessage::assistant_with_thinking(&buffer, &thinking_content),
                );
            }
            yield AgentEvent::content("\n\n[Interrupted]");
            yield AgentEvent::end();
            return;
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
                        let interrupt = safeguards.interrupt_signal.clone();

                async move {
                    (name.clone(), ToolExecutionResult {
                        _name: name.clone(),
                        arguments: arguments.clone(),
                        result: execute_tool_before_deadline(&tools_clone, &cache_clone, &name, arguments.clone(), turn_deadline, interrupt.as_ref()).await,
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
                // Round-stamped like stream_core: the pending-stream snapshot
                // stores whatever round the event carries — unstamped events
                // all collapsed to round 1, mismatching the persisted turn.
                yield AgentEvent::tool_call_start_round(&name, exec_arguments.clone(), 1);

                match execution.result {
                    Ok(output) => {
                        let succeeded = crate::toolkit::tool::effective_success(&crate::tools::resolve_tool_name(&name), &output);
                        let result_value = output.prompt_value_for_tool(&name);
                        let result_str = output.prompt_value_for_tool(&name).to_string();

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

                        // [stored-value sanitize] mirror of stream_core: the
                        // stored value is sanitized too, not just display —
                        // slim only engages >=64KB so 4-64KB data URLs/base64
                        // used to reach context + sessions.redb verbatim.
                        let sanitized_str = sanitize_tool_result_for_prompt(&slimmed_str);
                        let display_str = sanitized_str.clone();

                        tool_calls_with_results.push(ToolCall {
                            name: name.clone(),
                            id: String::new(),
                            arguments: exec_arguments,
                            result: Some(result_value.clone()),
                            round: Some(1),
                        });

                        yield AgentEvent::tool_call_end_round(&name, &display_str, succeeded, 1);

                        tool_call_results.push((name.clone(), sanitized_str));
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

                        yield AgentEvent::tool_call_end_round(&name, &error_msg, false, 1);
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
                if tool_name == "skill" && serde_json::from_str::<serde_json::Value>(result_str).ok().is_some_and(|v| v.get("guide").is_some_and(serde_json::Value::is_string)) {
                    let skill_id = crate::llm::extract_skill_id_from_result(result_str);
                    llm_interface.set_skill_context(skill_id, result_str.clone()).await;
                } else {
                    let mut state = internal_state.write().await;
                    let tool_result_msg = AgentMessage::tool_result(tool_name, result_str);
                    state.push_message(tool_result_msg);
                }
            }

            // Summary fallback: ask LLM to summarize tool results (no tools, no thinking)
            // This avoids dumping raw tool JSON to the user
            let summary_history: Vec<neomind_core::Message> = {
                let state_guard = internal_state.read().await;
                let compacted = turn_context.build(&state_guard.memory, effective_max);
                compacted.iter().map(|msg| msg.to_core()).collect()
            };

            let summary_prompt = "Based on the tool execution results in the conversation above, \
                provide a concise analysis and summary. Do NOT output any tool calls — \
                give a direct text response to the user's question.";

            let mut final_content = String::new();
            let summary_result = super::request_before_deadline(llm_interface.chat_stream_summary(
                summary_prompt,
                &summary_history,
            ), turn_deadline, safeguards.interrupt_signal.as_ref()).await;

            match summary_result {
                Ok(stream) => {
                    let mut pin = Box::pin(stream);
                    while let Some(chunk) =
                        super::next_before_deadline(&mut pin, safeguards.max_stream_duration, turn_deadline, safeguards.interrupt_signal.as_ref()).await
                    {
                        match chunk {
                            Ok(StreamEvent::Thinking(text)) => {
                                thinking_content.push_str(&text);
                                yield AgentEvent::thinking(text);
                            }
                            Ok(StreamEvent::ToolCalls(_)) => {},
                            Ok(StreamEvent::Text(text)) => {
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

            // Only an absent answer needs a factual fallback.
            if final_content.trim().is_empty() {
                let deduped_results = deduplicate_tool_results(&tool_call_results);
                let formatted = format_tool_results(&deduped_results);
                final_content = formatted.clone();
                yield AgentEvent::content(formatted);
            }

            // Persist the same answer and reasoning the UI received.
            let mut state = internal_state.write().await;
            if let Some(last) = state.memory.last_mut().filter(|m| m.role == "assistant" && m.tool_calls.is_some()) {
                last.content = final_content.into();
                last.thinking = (!thinking_content.is_empty()).then_some(thinking_content.clone());
            } else {
                state.push_message(AgentMessage::assistant_with_thinking(&final_content, &thinking_content));
            }
            drop(state);

            tracing::debug!("Multimodal tool execution complete with summary");
        } else {
            // No tool calls - save response directly
            let raw_response = if buffer.is_empty() {
                String::new()
            } else {
                buffer.clone()
            };

            // Preserve valid empty JSON containers; only absent content needs recovery.
            let degenerate = raw_response.trim().is_empty();
            let response_to_save = if degenerate {
                tracing::warn!(
                    orig_len = raw_response.len(),
                    "Multimodal stream produced no content, substituting fallback"
                );
                let fallback = "Sorry, the model produced no usable response. Please retry."
                    .to_string();
                yield AgentEvent::content(fallback.clone());
                fallback
            } else {
                str::to_owned(&raw_response)
            };

            let initial_msg = AgentMessage::assistant_with_thinking(&response_to_save, &thinking_content);
            internal_state.write().await.push_message(initial_msg);


        }

        if safeguards.interrupt_signal.as_ref().is_some_and(|rx| *rx.borrow()) {
            yield AgentEvent::content("\n\n[Interrupted]");
        }
        let pt = llm_interface.take_last_prompt_tokens().await;
        match pt {
            Some(t) => {
                let (system_tokens, tool_tokens) = llm_interface.estimate_prompt_breakdown().await;
                yield AgentEvent::end_with_usage(t, system_tokens, tool_tokens);
            }
            None => yield AgentEvent::end(),
        }
    }))
}
