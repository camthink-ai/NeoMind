//! Core text-only streaming response processing.
//!
//! Contains the main `process_stream_events` function for text-only LLM
//! interactions with tool calling support and multi-round ReAct loop.

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};

use super::{next_chunk_or_timeout_or_interrupt, StreamWait};

use super::context::{build_context_window_with_summary, ToolExecutionResult};
use super::dedup::deduplicate_tool_results;
use super::resolve::resolve_cached_arguments;
use super::result_format::format_tool_results;
use super::sanitize::sanitize_tool_result_for_prompt;
use super::thinking::cleanup_thinking_content;
use super::tool_exec::execute_tool_before_deadline;
use crate::agent::tool_parser::native_tool_calls;
use crate::agent::types::{AgentEvent, AgentInternalState, AgentMessage, ToolCall};
use crate::error::{NeoMindError, Result};
use crate::llm::LlmInterface;
use crate::llm::StreamEvent;

/// Configuration for stream processing safeguards
///
/// These safeguards prevent infinite loops and excessive resource usage
/// during LLM streaming operations.
///
/// The default values are synchronized with `neomind_core::llm::backend::StreamConfig`
/// to ensure consistent behavior across the system.
pub struct StreamSafeguards {
    /// Per-stream idle/time budget (default: 2400s); the configured turn budget also applies.
    ///
    /// This matches `StreamConfig::max_stream_duration_secs` and provides
    /// adequate time for complex reasoning tasks, especially with thinking models.
    pub max_stream_duration: Duration,

    /// Maximum thinking content length in characters (default: unlimited)
    ///
    /// Note: The actual thinking limit is enforced by the LLM backend's
    /// `StreamConfig::max_thinking_chars`. This field is retained for
    /// additional safety if needed.
    pub max_thinking_length: usize,

    /// Maximum content length in characters (default: unlimited)
    pub max_content_length: usize,

    /// Maximum calls in one model response (default: 100).
    pub max_tool_iterations: usize,

    /// Explicit per-turn tool-round budget (default: 30).
    pub max_tool_rounds: usize,

    /// Legacy compatibility field; chunk similarity no longer changes execution.
    pub max_repetition_count: usize,

    /// Heartbeat interval to keep connection alive (default: 10s)
    pub heartbeat_interval: Duration,

    /// Progress update interval during long operations (default: 5s)
    pub progress_interval: Duration,

    /// Optional interrupt signal - when set, stream should stop gracefully
    /// This allows users to interrupt long thinking processes
    pub interrupt_signal: Option<tokio::sync::watch::Receiver<bool>>,

    /// Legacy compatibility option. Semantic action guards have been retired;
    /// this field no longer changes execution or labels model prose.
    pub behavioral_guards: bool,
}

impl Default for StreamSafeguards {
    fn default() -> Self {
        Self {
            // Synchronized with StreamConfig::max_stream_duration_secs (2400s)
            // This provides adequate time for thinking models like qwen3-vl:2b
            // to complete extended reasoning before generating content.
            max_stream_duration: Duration::from_secs(2400),

            // No limit on thinking content - let the LLM backend enforce limits
            max_thinking_length: usize::MAX,

            max_content_length: usize::MAX,

            // Tool iterations limit - high limit to support complex multi-step queries
            // This value is for truncating
            // tool calls in a single LLM response.
            max_tool_iterations: 100,
            max_tool_rounds: 30,

            // Repetition detection threshold
            max_repetition_count: 3,

            // Heartbeat every 10 seconds to prevent WebSocket timeout
            heartbeat_interval: Duration::from_secs(10),

            // Progress update every 5 seconds during long operations
            progress_interval: Duration::from_secs(5),

            // No interrupt signal by default
            interrupt_signal: None,

            // Retained for source compatibility.
            behavioral_guards: false,
        }
    }
}

impl StreamSafeguards {
    /// Set the interrupt signal for this stream
    /// Returns a sender that can be used to trigger the interrupt
    pub fn with_interrupt_signal(mut self, rx: tokio::sync::watch::Receiver<bool>) -> Self {
        self.interrupt_signal = Some(rx);
        self
    }

    /// Compatibility setter; behavioral inference is no longer performed.
    pub fn with_behavioral_guards(mut self, enabled: bool) -> Self {
        self.behavioral_guards = enabled;
        self
    }
}

// 7 params: the streaming entry's dependency bundle (iface/state/tools)
// plus the turn's inputs; grouping them into a struct has been rejected to
// keep the nudge_tests harness calls flat.
#[allow(clippy::too_many_arguments)]
pub async fn process_stream_events_with_safeguards(
    llm_interface: Arc<LlmInterface>,
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<crate::toolkit::ToolRegistry>,
    user_message: &str,
    safeguards: StreamSafeguards,
    conversation_summary: Option<String>,
    summary_up_to_index: Option<u64>,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    let turn_started_at = Instant::now();
    let turn_wall_clock_budget = std::time::Duration::from_secs(
        neomind_storage::AgentDefaults::get()
            .chat_turn_timeout_secs
            .max(1),
    );
    let turn_deadline = turn_started_at + turn_wall_clock_budget;
    // Clear before building even the first request/overhead estimate. Later
    // ReAct rounds must retain skills loaded during THIS user turn.
    llm_interface.clear_skill_context().await;
    let turn_context =
        super::context::TurnContext::new(conversation_summary.clone(), summary_up_to_index);
    let user_message = user_message.to_string();

    // === Get conversation history and pass to LLM ===
    // This prevents the LLM from repeating actions or calling tools again
    // Pure async - no block_in_place
    let state_guard = internal_state.read().await;
    let mut history_messages = state_guard.memory.clone();
    drop(state_guard); // Release lock before calling LLM

    // [dup-fix] The caller (process_stream_events_with_safeguards) pushes
    // the current user message into memory BEFORE creating this stream, and
    // the LLM layer appends the user message itself when building the
    // request (`msgs.push(user_msg)` in llm.rs) — without dropping the
    // trailing copy here, every text prompt carried [.., user(current),
    // user(current)]. The multimodal path pushes after stream creation and
    // never had this duplication.
    if history_messages
        .last()
        .map(|m| m.role == "user" && m.content.as_ref() == user_message)
        .unwrap_or(false)
    {
        history_messages.pop();
    }

    // === CHAT HISTORY DEPTH (configurable, /api/settings/agent) ===
    // Same cap the non-streaming path applies — the server chat entry
    // points (SSE/WS) all stream, so without this the advertised setting
    // would never run for real conversations.
    let drained = crate::agent::apply_chat_history_depth(&mut history_messages);

    // [summary alignment] summary_up_to_index is stored against the FULL
    // history; after the front drain the indices shift. Without recentring,
    // the summary filter dropped `drained` extra messages the summary does
    // NOT cover — a silent content gap between summary and window.
    let summary_up_to_index =
        crate::agent::adjust_summary_index_after_drain(summary_up_to_index, drained);

    // [image budget] history images are re-sent (and really charged) every
    // turn; keep only the most recent ones.
    crate::agent::strip_stale_images(&mut history_messages);

    // === DYNAMIC CONTEXT WINDOW: Get model's actual capacity ===
    let max_context = llm_interface.max_context_length().await;

    // Measure actual overhead from system prompt + tool definitions
    let prompt_overhead = llm_interface.estimate_prompt_overhead_tokens().await;

    let effective_max = effective_history_budget(max_context, prompt_overhead);

    tracing::debug!(
        "Context window: model_capacity={}, prompt_overhead={}, reserve={}, effective_max={} for history",
        max_context, prompt_overhead, RESERVE_FOR_RESPONSE, effective_max
    );

    // A window that cannot even hold the system prompt + tool definitions is
    // unusable: the backend rejects every request after burning the whole
    // prompt. Fail fast with an actionable message instead of letting the
    // turn die mid-stream (2026-10-30: ctx 8192, prompt overhead 11684).
    if prompt_overhead >= max_context {
        let message = format!(
            "上下文窗口不足：系统提示与工具定义约 {prompt_overhead} tokens，而模型窗口只有 {max_context} tokens。请提高上下文设置（例如 NEOMIND_BUILTIN_LLM_CTX=32768）或减少工具/技能。"
        );
        tracing::error!(max_context, prompt_overhead, "{}", message);
        return Ok(Box::pin(async_stream::stream! {
            yield AgentEvent::Error { message };
            yield AgentEvent::end();
        }));
    }

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
        "Passing {} messages from history to LLM",
        history_for_llm.len()
    );

    // === THINKING CONTROL ===
    // Thinking is controlled by the user/instance thinking_enabled setting.
    // The LlmInterface resolves the effective thinking state from:
    //   1. Local override (per-request)
    //   2. Instance manager setting (from storage/frontend)
    //   3. Backend default
    // No keyword-based filtering — model providers have inconsistent standards.

    // Thinking control: Respect the user/instance thinking_enabled setting directly.
    // The llm_interface already resolves thinking priority: local override > instance setting > None.
    // No keyword-based filtering — model providers have different standards, keyword heuristics
    // are unreliable and override user preference without good reason.
    tracing::info!("Thinking control: respecting user/instance thinking_enabled setting directly");

    // Get the stream from llm_interface - thinking is controlled by instance/user settings
    let stream_result = super::request_before_deadline(
        llm_interface.chat_stream_with_history(&user_message, &history_for_llm),
        turn_deadline,
        safeguards.interrupt_signal.as_ref(),
    )
    .await;

    let stream = stream_result.map_err(|e| NeoMindError::Llm(e.to_string()))?;

    Ok(Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut buffer = String::new();
        let mut yielded_up_to: usize = 0; // Track how much of buffer has been yielded to prevent duplication
        let mut tool_calls_detected = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut content_before_tools = String::new();
        let mut thinking_content = String::new();

        let mut has_content = false;
        let mut has_thinking = false;

        // === SAFEGUARD: Track stream start time for timeout ===
        let stream_start = Instant::now();
        // First-chunk arrival. `stream_start` covers backend queueing +
        // prompt prefill BEFORE the first token (tens of seconds on
        // local models — longer when the previous turn's request still
        // occupies the only llama.cpp slot). Perceived-latency warnings
        // must measure actual GENERATION time, not queue wait.
        let mut first_chunk_at: Option<Instant> = None;

        // === KEEPALIVE: Track last event time for heartbeat ===
        #[allow(unused_assignments)]
        let mut last_event_time = Instant::now();
        let mut last_progress_time = Instant::now();
        #[allow(unused_assignments)]
        #[allow(unused_variables)]
        // === TIMEOUT WARNING FLAGS ===
        let mut timeout_warned = false;
        let mut long_thinking_warned = false;

        // === SAFEGUARD: Track thinking time and content ===
        let mut thinking_start_time: Option<Instant> = None;
        let mut thinking_timeout_warned = false;
        const THINKING_TIMEOUT_SECS: u64 = 300;
        // Thinking follows the same configured turn budget as content.

        // === SAFEGUARD: Track multi-round tool calling iterations ===
        let mut tool_iteration_count = 0usize;
        let max_tool_rounds = safeguards.max_tool_rounds.max(1);
        // Resource limits bound work without inferring intent or progress
        // from repeated argument strings. Repeated reads may be verification.
        // Accumulate ALL tool results across rounds for final summary
        let mut all_round_tool_results: Vec<(String, String)> = Vec::new();
        // Every (tool + arguments) call executed this turn; a round with
        // zero NEW signatures is repetition, not exploration.
        let mut seen_tool_sigs: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut loop_progress = ChatLoopProgress::default();
        let mut last_round_success_sigs = std::collections::HashSet::new();
        // Track per-round thinking and content for persistence (round number → text)
        let mut round_thinking_map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        let mut round_contents_map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        // Accumulate ALL rounds' thinking for the message's thinking field
        let mut all_rounds_thinking = String::new();

        let turn_system_notes: Vec<String> = Vec::new();
        yield AgentEvent::progress("Processing request", "generating", 0);

        // === MULTI-ROUND TOOL CALLING LOOP ===
            // Chat path: no guard-led hard loop breaker — the user is
            // watching and can abort. Safety nets: a no-new-information
            // breaker / round budget (below), the 30-round hard cap, and
            // the stream timeout.
        // (The scheduled-agent path brakes via cross-round dedup → AllDuplicate.)
        'multi_round_loop: loop {
            // Calls in THIS round whose (tool + arguments) signature has
            // not been seen yet this turn — the progress signal.
            let mut round_new_calls: usize = 0;
            if tool_iteration_count > 0 {
                tracing::debug!("Starting tool iteration round {}", tool_iteration_count + 1);

                // For subsequent rounds, we need a new LLM call with tools enabled.
                // Use the same budget-managed context builder as the initial call.
                let state_guard = internal_state.read().await;

                let history_for_llm: Vec<neomind_core::Message> = {
                    // Build context with the same budget-managed context
                    // builder as the initial call.
                    let compacted = turn_context.build(&state_guard.memory, effective_max);
                    compacted
                    .iter().map(|msg| msg.to_core()).collect::<Vec<_>>()
                };

                // Build context for subsequent rounds - tell LLM what happened before
                drop(state_guard);

                let context_msg = "Continue the original user request using the tool results above. Report actual outcomes and any remaining work.";
                tracing::debug!("Multi-round context: {}", context_msg);

                let thinking_override = None;
                let round_stream_result = super::request_before_deadline(llm_interface.chat_stream_tool_followup(
                    &user_message,
                    context_msg,
                    &history_for_llm,
                    thinking_override
                ), turn_deadline, safeguards.interrupt_signal.as_ref()).await;

                let round_stream = match round_stream_result {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Round {} LLM call failed: {}", tool_iteration_count + 1, e);

                        // Instead of just erroring out, try to summarize what we have so far.
                        // This gives the user a meaningful response instead of a blank cutoff.
                        if !all_round_tool_results.is_empty() {
                            let deduped_results = deduplicate_tool_results(&all_round_tool_results);
                            let fallback_prompt = "The model could not continue the tool loop. Summarize actual outcomes from the tool results, including errors, partial results and remaining work. Do not claim unfinished operations succeeded.";

                            let summary_history: Vec<neomind_core::Message> = {
                                let state_guard = internal_state.read().await;
                                let compacted = turn_context.build(&state_guard.memory, effective_max);
                                compacted.iter().map(|msg| msg.to_core()).collect()
                            };

                            let summary_result = super::request_before_deadline(llm_interface.chat_stream_summary(
                                fallback_prompt,
                                &summary_history,
                            ), turn_deadline, safeguards.interrupt_signal.as_ref()).await;

                            let mut fallback_content = String::new();
                            let mut fallback_thinking = String::new();
                            if let Ok(s) = summary_result {
                                let mut pin = Box::pin(s);
                                while let Some(chunk) = super::next_before_deadline(
                                    &mut pin, safeguards.max_stream_duration, turn_deadline,
                                    safeguards.interrupt_signal.as_ref(),
                                ).await {
                                    match chunk {
                                        Ok(StreamEvent::Text(text)) => {
                                            fallback_content.push_str(&text);
                                            yield AgentEvent::content(text);
                                        }
                                        Ok(StreamEvent::Thinking(text)) => {
                                            fallback_thinking.push_str(&text);
                                            yield AgentEvent::thinking(text);
                                        }
                                        Ok(StreamEvent::ToolCalls(_)) => {},
                                        Err(_) => break,
                                    }
                                }
                            }
                            if fallback_content.trim().is_empty() {
                                fallback_content = format_tool_results(&deduped_results);
                                yield AgentEvent::content(fallback_content.clone());
                            }
                            let mut message = AgentMessage::assistant(&fallback_content);
                            message.thinking = (!fallback_thinking.is_empty()).then_some(fallback_thinking);
                            internal_state.write().await.push_message(message);
                        } else {
                            yield AgentEvent::error(format!("Processing failed: {}", e));
                        }
                        break 'multi_round_loop;
                    }
                };

                stream = Box::pin(round_stream);
                buffer = String::new();
                yielded_up_to = 0;
                tool_calls.clear();
                content_before_tools = String::new();
                // Per-round stream state: the thinking-stall clock (and
                // the progress stage hints) must measure THIS round's
                // stream. The old cross-round accumulation made the 300s
                // warning fire 300s after the TURN's first thinking
                // chunk (live 2026-10-27: warned at R5+133s while that
                // stream itself was only 2m13s old), and the hard abort
                // below inherits the same per-round semantics.
                thinking_start_time = None;
                thinking_timeout_warned = false;
                has_thinking = false;
                has_content = false;
            }

            // === PHASE 1: Stream initial response (thinking + content + tool calls) ===
            // Bounded `next()`: a zero-chunk stall (upstream LLM never yields)
            // force-breaks the round instead of hanging forever. The wait
            // is ALSO cancel-aware: on local backends the first-chunk wait
            // (slot queueing + prefill) can run a minute, and a user
            // cancel during it must not sit unprocessed until the first
            // token arrives (live 2026-10-19: 55s cancel latency, the
            // zombie request blocking the backend's only slot while every
            // retry queued behind it).
            loop {
                let result = match next_chunk_or_timeout_or_interrupt(
                    &mut stream,
                    safeguards.max_stream_duration.min(turn_wall_clock_budget.saturating_sub(turn_started_at.elapsed())),
                    safeguards.interrupt_signal.as_ref(),
                )
                .await
                {
                    StreamWait::Chunk(Some(item)) => item,
                    // Stream ended (or stalled past max duration) —
                    // fall through to tool-call processing below.
                    StreamWait::Chunk(None) => break,
                    StreamWait::Interrupted => {
                        tracing::info!("Stream interrupted by user");
                        yield AgentEvent::content("\n\n[Interrupted]");
                        yield AgentEvent::end();
                        return;
                    }
                };
                if first_chunk_at.is_none() {
                    first_chunk_at = Some(Instant::now());
                }
                let elapsed = stream_start.elapsed();

                // Check timeout with early warning at 80% of max duration
                let timeout_threshold = safeguards.max_stream_duration;
                let warning_threshold = timeout_threshold.mul_f32(0.8);

                if elapsed > timeout_threshold {
                    tracing::warn!("Stream timeout ({:?} elapsed, max: {:?}), forcing completion", elapsed, timeout_threshold);
                    // Don't break here - let tool calls be processed
                    // Just log the timeout and continue to check for tool calls
                    if tool_calls_detected {
                        tracing::debug!("Timeout with tool calls detected, proceeding to execution");
                        break;
                    } else {
                        yield AgentEvent::error(format!("Request timeout ({:.1}s elapsed), completing processing...", elapsed.as_secs_f64()));
                        break;
                    }
                } else if elapsed > warning_threshold && !timeout_warned {
                    tracing::warn!("Stream approaching timeout ({:.1}s elapsed, max: {:.1}s)", elapsed.as_secs_f64(), timeout_threshold.as_secs_f64());
                    yield AgentEvent::warning(format!("Response is taking longer ({:.1}s elapsed), please wait...", elapsed.as_secs_f64()));
                    timeout_warned = true;
                }

                // Special warning for extended thinking with no content.
                // Timed from the FIRST CHUNK, not stream start: the
                // queue+prefill wait before it is backend latency, not
                // the model thinking (live 2026-10-19: this fired during
                // a 42s slot-queue wait with zero tokens generated).
                let generating_elapsed =
                    first_chunk_at.map(|t| t.elapsed()).unwrap_or_default();
                if has_thinking && !has_content
                    && generating_elapsed > Duration::from_secs(60)
                    && !long_thinking_warned
                {
                    tracing::warn!("Extended thinking detected ({:.1}s) with no content yet", generating_elapsed.as_secs_f64());
                    yield AgentEvent::warning("The model is performing deep thinking, this may take longer...".to_string());
                    long_thinking_warned = true;
                }

                // Check for interrupt signal
                // We clone the value to avoid holding the guard across await
                let is_interrupted = safeguards.interrupt_signal.as_ref().map(|rx| *rx.borrow()).unwrap_or(false);
                if is_interrupted {
                    tracing::info!("Stream interrupted by user");
                    yield AgentEvent::content("\n\n[Interrupted]");
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
                        format!("{}...", match stage_name {
                            "thinking" => "Thinking",
                            "executing" => "Executing tools",
                            _ => "Generating response",
                        }),
                        stage_name,
                        elapsed_ms
                    );
                    last_progress_time = Instant::now();
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
                            // Track thinking start time
                            if thinking_start_time.is_none() {
                                thinking_start_time = Some(Instant::now());
                            }

                            // Check for thinking timeout
                            if let Some(start) = thinking_start_time {
                                let thinking_elapsed = start.elapsed();
                                if thinking_elapsed > Duration::from_secs(THINKING_TIMEOUT_SECS) && !thinking_timeout_warned {
                                    tracing::warn!(
                                        "Thinking has continued for {:.1}s; the configured turn budget still applies.",
                                        thinking_elapsed.as_secs_f64()
                                    );
                                    yield AgentEvent::warning(
                                        "The model is taking longer than expected to think. This may indicate a complex query or the model getting stuck. Please wait...".to_string()
                                    );
                                    thinking_timeout_warned = true;
                                }

                            }

                            // No thinking limit - let the model think as much as needed
                            // First, add the new text to thinking content
                            thinking_content.push_str(&text);
                            has_thinking = true;

                            yield AgentEvent::thinking(text);
                            last_event_time = Instant::now();
                            continue;
                        }

                        // content: need to check for tool calls
                        has_content = true;
                        last_event_time = Instant::now();

                        if safeguards.max_content_length != usize::MAX
                            && buffer.len() + text.len() > safeguards.max_content_length
                        {
                            tracing::warn!("Content exceeded max length ({}), stopping stream", safeguards.max_content_length);
                            yield AgentEvent::error("Response too long - content limit reached".to_string());
                            break;
                        }

                        // Add text to buffer
                        buffer.push_str(&text);

                        yield AgentEvent::content(text);
                        yielded_up_to = buffer.len();
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        yield AgentEvent::warning(format!("Stream interrupted: {}. Recovering available results.", e));
                        // Finalization below persists partial output once.

                        break;
                    }
                }
            }

            // === Handle tool calls if detected ===
            if tool_calls_detected {
                // Nudge effectiveness: the retried round actually acted.
                tracing::debug!("Starting tool execution round {}", tool_iteration_count + 1);

                // Send progress event to inform user about tool iteration
                let current_elapsed = stream_start.elapsed();
                yield AgentEvent::progress(
                    format!("Executing tools (round {}/{})", tool_iteration_count + 1, safeguards.max_tool_iterations),
                    "executing",
                    current_elapsed.as_millis() as u64,
                );

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

                // Resolve cached data references in tool arguments
                let (large_cache, cache) = {
                    let state = internal_state.read().await;
                    (state.large_data_cache.clone(), state.tool_result_cache.clone())
                };

                // Execute tool calls with bounded concurrency (max 6 parallel)
                const MAX_TOOL_CONCURRENCY: usize = 6;

                // Collect into owned tuples to avoid lifetime issues with async_stream.
                // Carry the emission index so we can restore LLM-emission order after
                // `buffer_unordered` (which completes out of order) — the stuck
                // detector's ping-pong pattern needs chronological action order.
                let tool_inputs: Vec<(usize, String, serde_json::Value)> = tool_calls_to_execute
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| {
                        (
                            i,
                            tc.name.clone(),
                            resolve_cached_arguments(&tc.arguments, &large_cache, &tc.name),
                        )
                    })
                    .collect();

                let tool_futures = futures::stream::iter(tool_inputs.into_iter().map(|(i, name, arguments)| {
                    let tools_clone = tools.clone();
                    let cache_clone = cache.clone();
                    let interrupt = safeguards.interrupt_signal.clone();

                    async move {
                        // Shell policy check — same deny-list as Loop A (tool_loop.rs).
                        // Blocks catastrophic commands (rm -rf /, dd, mkfs, etc.) in chat.
                        if name == "shell" {
                            if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                                if let Some(reason) = crate::toolkit::policy::deny_reason(cmd) {
                                    tracing::warn!("Chat tool call blocked by safety policy: {}", reason);
                                    return (i, name.clone(), ToolExecutionResult {
                                        _name: name.clone(),
                                        arguments: arguments.clone(),
                                        result: Err(crate::toolkit::error::ToolError::Execution(
                                            format!("Blocked by safety policy: {}", reason),
                                        )),
                                    });
                                }
                            }
                        }
                        let result =
                            execute_tool_before_deadline(&tools_clone, &cache_clone, &name, arguments.clone(), turn_deadline, interrupt.as_ref()).await;

                        (i, name.clone(), ToolExecutionResult {
                            _name: name.clone(),
                            arguments: arguments.clone(),
                            result,
                        })
                    }
                })).buffer_unordered(MAX_TOOL_CONCURRENCY);

                // [keep-alive] Tool execution can legitimately run for minutes
                // (extension build/install, async agent exec waits). The stream
                // is otherwise silent during this phase — the chunk-loop
                // heartbeat (line ~521) only fires between stream chunks, so a
                // long tool yields NO events and WS listeners (chat UI / eval)
                // see an event gap and kill the turn. Yield heartbeats on an
                // independent timer while the tool batch runs.
                let collect_fut = tool_futures.collect::<Vec<_>>();
                tokio::pin!(collect_fut);
                let mut tool_heartbeat = tokio::time::interval(safeguards.heartbeat_interval);
                // tokio interval's first tick completes immediately — consume it
                // so the first heartbeat lands one interval in, not instantly.
                tool_heartbeat.tick().await;
                let mut tool_results_executed: Vec<_> = loop {
                    tokio::select! {
                        _ = tool_heartbeat.tick() => {
                            yield AgentEvent::heartbeat();
                        }
                        done = &mut collect_fut => break done,
                    }
                };
                // Restore LLM-emission order (buffer_unordered completes in arbitrary order).
                tool_results_executed.sort_by_key(|(i, _, _)| *i);

                // Process results
                let mut tool_calls_with_results: Vec<ToolCall> = Vec::new();
                let mut tool_call_results: Vec<(String, String)> = Vec::new();
                // Compare actual outcomes before advisory/cache-reference
                // decoration; those annotations are not new information.
                let mut round_progress_results: Vec<(String, String)> = Vec::new();
                let mut round_has_errors = false;
                let mut this_round_success_sigs: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for (_, name, execution) in tool_results_executed {
                    // Use arguments from the execution result (preserves per-call arguments for same-name tools)
                    let exec_arguments = execution.arguments.clone();
                    // Novel (tool + arguments) → this round is gaining
                    // information even if the model stays quiet.
                    let call_sig = format!(
                        "{}|{}",
                        name,
                        serde_json::to_string(&exec_arguments).unwrap_or_default()
                    );
                    if seen_tool_sigs.insert(call_sig) {
                        round_new_calls += 1;
                    }
                    yield AgentEvent::tool_call_start_round(&name, exec_arguments.clone(), tool_iteration_count + 1);

                    // Duplicate-call advisory (see last_round_success_sigs):
                    // non-reusable tool, identical signature, succeeded LAST
                    // round. Tag the result value; execution already happened.
                    let dup_sig = format!(
                        "{}|{}",
                        name,
                        serde_json::to_string(&exec_arguments).unwrap_or_default()
                    );
                    let was_dup = last_round_success_sigs.contains(&dup_sig);

                    let outcome = match &execution.result {
                        Ok(output) => serde_json::json!({
                            "success": output.success,
                            "data": output.data,
                            "error": output.error,
                        }).to_string(),
                        Err(error) => error.to_string(),
                    };
                    let succeeded = execution.result.as_ref().is_ok_and(|output|
                        crate::toolkit::tool::effective_success(
                            &crate::tools::resolve_tool_name(&name), output,
                        )
                    );
                    round_has_errors |= !succeeded;
                    round_progress_results.push((dup_sig.clone(), outcome));

                    match execution.result {
                        Ok(output) => {
                            if crate::toolkit::tool::effective_success(&name, &output) {
                                this_round_success_sigs.insert(dup_sig.clone());
                            }
                            let mut result_value = output.prompt_value();
                            if was_dup {
                                let advisory = serde_json::json!(
                                    "This call also succeeded in the preceding round. The current result below is fresh."
                                );
                                if let Some(obj) = result_value.as_object_mut() {
                                    obj.insert("_duplicate".into(), advisory);
                                } else {
                                    result_value = serde_json::json!({
                                        "data": result_value, "_duplicate": advisory,
                                    });
                                }
                                tracing::info!(tool = %name, "Advisory: identical non-reusable call succeeded last round");
                            }
                            let result_str = result_value.to_string();

                            // Slim large strings (image data URLs, big base64) inline:
                            // walk the JSON tree, store each large value into cache
                            // under a deterministic key, and replace the value with a
                            // one-line summary. This keeps the JSON payload small,
                            // preserves all sibling fields, lets multiple images
                            // coexist, and gives the LLM a concrete `$cached:` ref +
                            // action hint pointing at the `vision` tool.
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

                            // [stored-value sanitize] Sanitize the value that is
                            // STORED too, not just the display copy. Slimming only
                            // engages >=64KB, so data URLs / base64 between ~4KB and
                            // 64KB used to flow verbatim into every subsequent LLM
                            // round and into sessions.redb. The sanitizer strips
                            // data:image URLs and large base64 blobs to placeholders.
                            let sanitized_str = sanitize_tool_result_for_prompt(&slimmed_str);
                            let display_str = sanitized_str.clone();

                            tool_calls_with_results.push(ToolCall {
                                name: name.clone(),
                                id: String::new(),
                                arguments: exec_arguments,
                                result: Some(result_value.clone()),
                                round: Some(tool_iteration_count + 1),
                            });

                            yield AgentEvent::tool_call_end_round(&name, &display_str, succeeded, tool_iteration_count + 1);

                            tool_call_results.push((name.clone(), sanitized_str));
                        }
                        Err(e) => {
                            let mut error_msg = format!("Tool execution failed: {}", e);
                            // Weak models sometimes emit a whole `neomind ...` command or a
                            // CLI domain as the tool name. Redirect to `shell` so they recover
                            // next round instead of looping. No-op for real tool names.
                            if let Some(hint) =
                                crate::ai_agent::executor::tool_result::hallucinated_tool_hint(&name)
                            {
                                error_msg.push_str(&hint);
                            }
                            let error_value = serde_json::json!({"error": error_msg});

                            tool_calls_with_results.push(ToolCall {
                                name: name.clone(),
                                id: String::new(),
                                arguments: exec_arguments,
                                result: Some(error_value.clone()),
                                round: Some(tool_iteration_count + 1),
                            });

                            yield AgentEvent::tool_call_end_round(&name, &error_msg, false, tool_iteration_count + 1);
                            tool_call_results.push((name.clone(), error_msg));
                        }
                    }
                }

                last_round_success_sigs = this_round_success_sigs;
                all_round_tool_results.extend(tool_call_results.iter().cloned());
                // === UNIFIED ReAct LOOP: Save results and continue ===
                // Always save assistant+tool_calls and tool results to history,
                // then let the LLM decide in the next round whether to call more tools
                // or give the final answer.

                // Check iteration limit, wall-clock budget and duplicate detection
                let wall_clock_exhausted = turn_started_at.elapsed() >= turn_wall_clock_budget;
                if wall_clock_exhausted {
                    tracing::warn!(
                        elapsed = ?turn_started_at.elapsed(),
                        budget_secs = turn_wall_clock_budget.as_secs(),
                        rounds = tool_iteration_count + 1,
                        "Turn wall-clock budget exhausted — exiting tool loop for the final summary"
                    );
                    yield AgentEvent::progress(
                        "Time budget reached — generating final response...".to_string(),
                        "summarizing",
                        0,
                    );
                }
                // Loop signal: the SAME calls returning BYTE-IDENTICAL
                // results across rounds. A verification poll whose result
                // changed (online→offline, a new metric) or any new call
                // resets the counter — exploration and verify-after-write
                // are not loops. Only repeated no-op rounds are.
                let identical_rounds = loop_progress.observe(
                    round_new_calls, &round_progress_results,
                );
                tracing::debug!(identical_rounds, "Repeated tool results observed");
                let should_continue = !wall_clock_exhausted
                    && tool_iteration_count < max_tool_rounds - 1;
                // === Save assistant message with tool_calls BEFORE tool results ===
                let response_to_save = if content_before_tools.is_empty() {
                    String::new()
                } else {
                    str::to_owned(&content_before_tools)
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
                tracing::debug!("[streaming] Saving assistant message with {} tool_calls (round {})",
                    initial_msg.tool_calls.as_ref().map_or(0, |c| c.len()), tool_iteration_count + 1);
                internal_state.write().await.push_message(initial_msg);

                // Save tool results to memory.
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

                // NOTE: Mid-task compaction removed — build_context_window_with_config
                // (called above at each round) already handles LLM context trimming
                // without modifying the stored history. In-place compaction caused
                // persisted history to shrink after session switch (messages permanently
                // lost when compact_memory_mid_task modified state.memory).

                // If we should continue the ReAct loop, save round state and loop back
                if should_continue {
                    tool_iteration_count += 1;

                    // Save per-round thinking and content for persistence
                    let round_num = tool_iteration_count as u32;
                    if !thinking_content.is_empty() {
                        round_thinking_map.insert(round_num, thinking_content.clone());
                        all_rounds_thinking.push_str(&thinking_content);
                    }
                    if !content_before_tools.is_empty() {
                        let cleaned = str::to_owned(&content_before_tools);
                        let cleaned = cleaned.trim()
                            .trim_start_matches("```json").trim_start_matches("```")
                            .trim();
                        if !cleaned.is_empty() {
                            round_contents_map.insert(round_num, cleaned.to_string());
                        }
                    }

                    tool_calls_detected = false;
                    tool_calls.clear();
                    content_before_tools.clear();

                    yield AgentEvent::IntermediateEnd;
                    continue 'multi_round_loop;
                }

                // === LOOP END: explicit round or time budget ===
                // The LLM will see tool results in history on the next turn.
                // Save final round thinking for persistence.
                let last_round = (tool_iteration_count + 1) as u32;
                if !thinking_content.is_empty() {
                    let cleaned = cleanup_thinking_content(&thinking_content);
                    round_thinking_map.insert(last_round, cleaned.clone());
                    all_rounds_thinking.push_str(&cleaned);
                }

                // Convert round maps to serde_json::Value for AgentMessage
                let round_thinking_val = if !round_thinking_map.is_empty() {
                    Some(serde_json::to_value(&round_thinking_map).unwrap_or(serde_json::Value::Null))
                } else {
                    None
                };
                let round_contents_val = if !round_contents_map.is_empty() {
                    Some(serde_json::to_value(&round_contents_map).unwrap_or(serde_json::Value::Null))
                } else {
                    None
                };

                // Fallback: try a summary call when the last round had errors,
                // OR when LLM didn't produce content before tools.
                // Without tool definitions, the model must output text instead of more tool calls.
                let deduped_results = deduplicate_tool_results(&all_round_tool_results);
                // Only the LAST round's structured outcome decides the
                // summary prompt. Payload words ("error"/"invalid" as
                // device field names) are not failures; scanning all-round
                // text misclassified 30 successful reads as tool errors.
                let last_round_has_errors = round_has_errors;
                // The preamble "Let me check..." is NOT a final answer.
                // Force summary when tool errors exist, regardless of content_before_tools.
                // Chars, not bytes: `.len()` counted ~3 bytes per CJK
                // char, so a 100-char Chinese preamble (300 bytes) read
                // as a substantial answer and skipped the forced summary.
                let content_is_preamble =
                    content_before_tools.trim().chars().count() < 200;

                if content_before_tools.is_empty() || (last_round_has_errors && content_is_preamble) {

                    // Notify user that we're generating a final response
                    if last_round_has_errors {
                        yield AgentEvent::progress(
                            "Generating final response...".to_string(),
                            "summarizing",
                            0,
                        );
                    }

                    // Build compact history for the summary call
                    let summary_history: Vec<neomind_core::Message> = {
                        let state_guard = internal_state.read().await;
                        let compacted = turn_context.build(&state_guard.memory, effective_max);
                        compacted.iter().map(|msg| msg.to_core()).collect()
                    };

                    // Detect whether tool results contain errors to tailor the prompt
                    let has_errors = last_round_has_errors;

                    let summary_prompt = if has_errors {
                        "Some tool calls above returned errors. If the user's question can \
                        already be answered from the successful results, answer it now. \
                        Otherwise explain the failure in plain language and suggest what the \
                        user can do. Do NOT output any tool calls — give a direct text response."
                    } else {
                        "Based on the tool execution results in the conversation above, \
                        answer the user's question concisely. Do NOT output any tool calls. \
                        If information that ONLY the user can provide is missing, ask ONE \
                        concise clarifying question instead of guessing."
                    };

                    let summary_result = super::request_before_deadline(llm_interface.chat_stream_summary(
                        summary_prompt,
                        &summary_history,
                    ), turn_deadline, safeguards.interrupt_signal.as_ref()).await;

                    let mut final_content = String::new();
                    match summary_result {
                        Ok(stream) => {
                            let mut pin = Box::pin(stream);
                            while let Some(chunk) =
                                super::next_before_deadline(&mut pin, safeguards.max_stream_duration, turn_deadline, safeguards.interrupt_signal.as_ref()).await
                            {
                                match chunk {
                                    Ok(StreamEvent::Thinking(text)) => {
                                        all_rounds_thinking.push_str(&text);
                                        yield AgentEvent::thinking(text);
                                    }
                                    Ok(StreamEvent::ToolCalls(_)) => {},
                                    Ok(StreamEvent::Text(text)) => {
                                        final_content.push_str(&text);
                                        yield AgentEvent::content(text);
                                    }
                                    Err(e) => {
                                        tracing::error!("Summary stream error: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Summary call failed: {}", e);
                        }
                    }

                    // If summary also failed, fall back to formatted tool results
                    if final_content.trim().is_empty() {
                        final_content = format_tool_results(&deduped_results);
                        tracing::info!(
                            "Summary call produced empty content, using formatted tool results ({} chars)",
                            final_content.len()
                        );
                        yield AgentEvent::content(final_content.clone());
                    } else {
                        tracing::info!(
                            "Summary call succeeded ({} chars)",
                            final_content.len()
                        );
                    }

                    // Save as assistant message with round metadata
                    let mut final_msg = AgentMessage::assistant(&final_content);
                    final_msg.thinking = if all_rounds_thinking.is_empty() { None } else { Some(all_rounds_thinking.clone()) };
                    final_msg.round_thinking = round_thinking_val;
                    final_msg.round_contents = round_contents_val;
                    let mut state = internal_state.write().await;
                    state.register_response(&final_content);
                    state.push_message(final_msg);
                } else {
                    // LLM produced some content before tools in this last round
                    // Clean and save it as the final response
                    let cleaned_content = str::to_owned(&content_before_tools);
                    let mut final_msg = AgentMessage::assistant(&cleaned_content);
                    final_msg.thinking = if all_rounds_thinking.is_empty() { None } else { Some(all_rounds_thinking.clone()) };
                    final_msg.round_thinking = round_thinking_val;
                    final_msg.round_contents = round_contents_val;
                    let mut state = internal_state.write().await;
                    state.register_response(&cleaned_content);
                    state.push_message(final_msg);
                }

                tracing::debug!("ReAct loop completed after {} tool iterations", tool_iteration_count + 1);
            } else {
                let mut raw_response = buffer.clone();

                // Ordinary text is preserved, including JSON and examples.
                // Only an absent answer gets one bounded recovery request.
                if raw_response.trim().is_empty() && turn_started_at.elapsed() < turn_wall_clock_budget {
                    let had_thinking = !thinking_content.is_empty();
                    let orig_len = raw_response.len();
                    raw_response.clear();
                    tracing::warn!(
                        had_thinking = had_thinking,
                        orig_len = orig_len,
                        "Stream completed without content, attempting bounded recovery"
                    );

                    // Build a compact history for the retry (keep last few messages)
                    let state_guard = internal_state.read().await;
                    let retry_history: Vec<neomind_core::Message> = {
                        let context = turn_context.build(&state_guard.memory, effective_max);
                        let non_system: Vec<&AgentMessage> = context.iter().collect();
                        // Keep at most last 6 messages to reduce prompt size
                        let keep = non_system.len().saturating_sub(6);
                        context.iter().filter(|m| m.role == "system").chain(non_system[keep..].iter().copied().filter(|m| m.role != "system")).map(|m| m.to_core()).collect()
                    };
                    drop(state_guard);

                    let retry_prompt = format!(
                        "Original user request: {user_message}\n\nThe previous request produced no answer. Respond using the available evidence and report any limitations."
                    );
                    let retry_result = super::request_before_deadline(
                        llm_interface.chat_stream_summary(&retry_prompt, &retry_history),
                        turn_deadline, safeguards.interrupt_signal.as_ref(),
                    ).await;

                    match retry_result {
                        Ok(retry_stream) => {
                            let mut retry_content = String::new();
                            let mut pin = Box::pin(retry_stream);
                            // Recovery shares the turn deadline and preserves configured thinking.
                            const RETRY_IDLE_TIMEOUT: std::time::Duration =
                                std::time::Duration::from_secs(120);
                            const RETRY_TOTAL_BUDGET: std::time::Duration =
                                std::time::Duration::from_secs(180);
                            let retry_started = std::time::Instant::now();
                            while let Some(chunk) =
                                super::next_before_deadline(&mut pin, RETRY_IDLE_TIMEOUT, turn_deadline.min(retry_started + RETRY_TOTAL_BUDGET), safeguards.interrupt_signal.as_ref()).await
                            {
                                if retry_started.elapsed() >= RETRY_TOTAL_BUDGET {
                                    tracing::warn!(
                                        chars_so_far =
                                            retry_content.chars().count(),
                                        "Recovery exceeded its total budget — using whatever arrived"
                                    );
                                    break;
                                }
                                match chunk {
                                    Ok(StreamEvent::Thinking(text)) => {
                                        thinking_content.push_str(&text);
                                        yield AgentEvent::thinking(text);
                                    }
                                    Ok(StreamEvent::ToolCalls(_)) => {},
                                    Ok(StreamEvent::Text(text)) => retry_content.push_str(&text),
                                    Err(e) => {
                                        tracing::error!("Retry stream error: {}", e);
                                        break;
                                    }
                                }
                            }

                            let cleaned = retry_content.clone();

                            if !cleaned.trim().is_empty() {
                                raw_response = cleaned.clone();
                                yield AgentEvent::content(cleaned);
                                tracing::info!(
                                    content_len = raw_response.len(),
                                    "Recovery produced an answer"
                                );
                            } else {
                                // Diagnose WHICH empty-content failure this is:
                                // large thinking_chars → the model wedged in a
                                // thinking loop until the budget ran out (the
                                // known llama.cpp mid-stream wedge family);
                                // all-zero → the backend stalled/rejected the
                                // request outright. The old single warn said
                                // "only tool calls" even for plain-empty retries.
                                let retry_was_tool_calls = !retry_content.trim().is_empty();
                                tracing::warn!(
                                    thinking_chars = thinking_content.chars().count(),
                                    first_attempt_chars = orig_len,
                                    retry_chars = retry_content.chars().count(),
                                    retry_was_tool_calls,
                                    "Stream and recovery both produced no usable \
                                     content — root-cause hint: thinking_chars >> 0 means a \
                                     thinking-loop wedge against the token budget; all zeros \
                                     means the backend stalled or rejected the request shape"
                                );
                                let fallback =
                                    "Sorry, the model could not produce a response. Please retry.\n[reason: the model returned empty content after retry — the backend may be slow, over its real context limit, or rejecting the request shape]"
                                        .to_string();
                                raw_response = fallback.clone();
                                yield AgentEvent::content(fallback);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Retry LLM call failed: {}", e);
                            let fallback = format!(
                                "Sorry, the model could not produce a response. Please retry.\n[error: {e}]"
                            );
                            raw_response = fallback.clone();
                            yield AgentEvent::content(fallback);
                        }
                    }
                }

                if raw_response.trim().is_empty() {
                    raw_response = "The turn ended without a final answer within its time budget. No further operations were started.".into();
                    yield AgentEvent::content(raw_response.clone());
                }
                let response_to_save = str::to_owned(&raw_response);

                let mut initial_msg = if !thinking_content.is_empty() {
                    let cleaned_thinking = cleanup_thinking_content(&thinking_content);
                    AgentMessage::assistant_with_thinking(&response_to_save, &cleaned_thinking)
                } else {
                    AgentMessage::assistant(&response_to_save)
                };
                if !turn_system_notes.is_empty() {
                    initial_msg.system_notes = Some(turn_system_notes.clone());
                }
                {
                    let mut state = internal_state.write().await;
                    // Register response for cross-turn repetition detection
                    state.register_response(&response_to_save);
                    state.push_message(initial_msg);
                }

                // Yield any remaining un-yielded content from buffer
                if buffer.len() > yielded_up_to {
                    let remaining = buffer[yielded_up_to..].to_string();
                    if !remaining.is_empty() {
                        yield AgentEvent::content(remaining);
                    }
                }
            }

            // Break the loop after processing
            break 'multi_round_loop;
        }

        if safeguards.interrupt_signal.as_ref().is_some_and(|rx| *rx.borrow()) {
            yield AgentEvent::content("\n\n[Interrupted]");
        }
        // Read token usage from LLM interface (captured from Ollama backend stream)
        let prompt_tokens = llm_interface.take_last_prompt_tokens().await;
        match prompt_tokens {
            Some(pt) => {
                let (system_tokens, tool_tokens) = llm_interface.estimate_prompt_breakdown().await;
                yield AgentEvent::end_with_usage(pt, system_tokens, tool_tokens);
            }
            None => yield AgentEvent::end(),
        }
    }))
}

/// Reserve tokens for model response generation (minimum 1024).
const RESERVE_FOR_RESPONSE: usize = 1024;

/// History budget given the model's context capacity and the measured
/// prompt overhead (system prompt + tool definitions).
///
/// Invariant: the returned budget NEVER exceeds what actually fits —
/// `overhead + RESERVE_FOR_RESPONSE + budget <= max_context`. The old code
/// raised the budget to a hard 20%-of-context floor AFTER subtracting the
/// overhead, so on 8K-class models (where the platform prompt + tools alone
/// run 4-6K tokens) the floor re-inflated the budget past the window and
/// CONSTRUCTED an overflowing prompt every turn: llama-server 400 →
/// compact-retry ladder → tools stripped / hard "context exceeds" error.
/// The floor is now capped at the real remaining budget; a starved budget
/// (<20% of the window) is logged instead of silently exceeded.
pub(crate) fn effective_history_budget(max_context: usize, prompt_overhead: usize) -> usize {
    let raw_budget = max_context
        .saturating_sub(prompt_overhead)
        .saturating_sub(RESERVE_FOR_RESPONSE);

    let floor = ((max_context * 20) / 100).min(raw_budget);
    let effective = raw_budget.max(floor);

    if effective < (max_context * 20) / 100 {
        tracing::warn!(
            max_context,
            prompt_overhead,
            effective,
            "Context starvation: prompt overhead leaves under 20% of the window for \
             history — consider a larger context preset or a lighter prompt"
        );
    }
    effective
}

/// Fingerprint of one round's tool results (name + payload). The chat loop
/// breaker compares consecutive rounds: identical calls with a CHANGED result
/// are progress (verification/polling), not a loop.
fn tool_round_fingerprint(results: &[(String, String)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, result) in results {
        name.hash(&mut hasher);
        result.hash(&mut hasher);
    }
    hasher.finish()
}

#[derive(Default)]
struct ChatLoopProgress {
    previous: Option<u64>,
    identical_rounds: usize,
}

impl ChatLoopProgress {
    fn observe(&mut self, new_calls: usize, results: &[(String, String)]) -> usize {
        let fingerprint = tool_round_fingerprint(results);
        self.identical_rounds = if new_calls == 0 && self.previous == Some(fingerprint) {
            self.identical_rounds + 1
        } else {
            0
        };
        self.previous = Some(fingerprint);
        self.identical_rounds
    }
}

#[cfg(test)]
mod history_budget_tests {
    use super::*;

    #[test]
    fn repeated_poll_can_change_state_on_third_and_later_calls() {
        let mut progress = ChatLoopProgress::default();
        let pending = vec![("shell|status".into(), "pending".into())];
        let ready = vec![("shell|status".into(), "ready".into())];
        assert_eq!(progress.observe(1, &pending), 0);
        assert_eq!(progress.observe(0, &pending), 1);
        assert_eq!(progress.observe(0, &ready), 0);
        assert_eq!(progress.observe(0, &ready), 1);
        assert_eq!(progress.observe(0, &pending), 0);
    }

    #[test]
    fn repeated_unchanged_rounds_stop_but_new_calls_reset_progress() {
        let mut progress = ChatLoopProgress::default();
        let results = vec![("shell|status".into(), "pending".into())];
        assert_eq!(progress.observe(1, &results), 0);
        assert_eq!(progress.observe(0, &results), 1);
        assert_eq!(progress.observe(0, &results), 2);
        assert_eq!(progress.observe(0, &results), 3);
        assert_eq!(progress.observe(1, &results), 0);
    }

    /// The shell tool reports success=true with the exit code in DATA —
    /// the guards must read the exit code, or failing shell commands count
    /// as successes (live: a failed add-components got the "already
    /// succeeded last round" advisory, and the failure-strike break never
    /// armed for shell).
    #[test]
    fn effective_success_reads_shell_exit_code() {
        use crate::toolkit::tool::ToolOutput;
        // shell exit 0 → success
        let ok = ToolOutput::success(serde_json::json!({"exit_code": 0, "stdout": "fine"}));
        assert!(crate::toolkit::tool::effective_success("shell", &ok));
        // shell exit 1 → FAILURE despite ToolOutput.success==true
        let bad = ToolOutput::success(
            serde_json::json!({"exit_code": 1, "stderr": "missing field `position`"}),
        );
        assert!(!crate::toolkit::tool::effective_success("shell", &bad));
        // timed out → failure
        let t = ToolOutput::success(serde_json::json!({"exit_code": null, "timed_out": true}));
        assert!(!crate::toolkit::tool::effective_success("shell", &t));
        // non-shell tools: ToolOutput.success is authoritative
        let w = ToolOutput::success(serde_json::json!({"path": "x"}));
        assert!(crate::toolkit::tool::effective_success("file_write", &w));
        let e = ToolOutput::error("nope");
        assert!(!crate::toolkit::tool::effective_success("web_fetch", &e));
    }

    /// Successful device/API payloads contain "error"/"invalid" as FIELD
    /// names; only the structured outcome may count as a failed call.
    /// 2026-10-30: 30 rounds of successful device reads were summarized as
    /// "the tool calls above returned errors".
    #[test]
    fn tool_result_error_detection_ignores_payload_words() {
        use crate::toolkit::tool::{effective_success, ToolOutput};
        let payload = serde_json::json!({"error": "none", "status": "invalid"});
        assert!(effective_success(
            "extension_status",
            &ToolOutput::success(payload)
        ));
        assert!(effective_success(
            "shell",
            &ToolOutput::success(
                serde_json::json!({"exit_code": 0, "stdout": "error: none, invalid: 0"})
            )
        ));
        assert!(!effective_success(
            "shell",
            &ToolOutput::success(serde_json::json!({"exit_code": 1, "stderr": "boom"}))
        ));
        assert!(!effective_success(
            "extension_status",
            &ToolOutput::error("bad request")
        ));
    }

    /// The loop breaker must treat a changed tool result as progress
    /// (verification/polling), not as a repeated round.
    #[test]
    fn round_fingerprint_distinguishes_changed_results() {
        let a = vec![(
            "shell".to_string(),
            r#"{"exit_code":0,"stdout":"online"}"#.to_string(),
        )];
        let b = a.clone();
        let changed = vec![(
            "shell".to_string(),
            r#"{"exit_code":0,"stdout":"offline"}"#.to_string(),
        )];
        assert_eq!(tool_round_fingerprint(&a), tool_round_fingerprint(&b));
        assert_ne!(tool_round_fingerprint(&a), tool_round_fingerprint(&changed));
    }

    /// The regression that motivated the extraction: a 8K-class model whose
    /// platform prompt + tools overhead crosses ~5.5K tokens. The old floor
    /// forced ~1.6K of history back in, constructing a prompt larger than
    /// the window itself. The budget must respect the subtraction.
    #[test]
    fn budget_never_exceeds_remaining_capacity() {
        for (max_ctx, overhead) in [
            (8192usize, 6000usize), // the reported-bug zone
            (8192, 7100),           // overhead nearly fills the window
            (8192, 8192),           // overhead alone fills it
            (4096, 2500),           // small custom backend
            (16384, 5000),          // healthy 16K
            (131072, 6000),         // LFM-class
        ] {
            let budget = effective_history_budget(max_ctx, overhead);
            // Saturating form: when the overhead alone fills the window no
            // budget can satisfy the plain inequality — the real claim is
            // that the BUDGET never adds overflow beyond the subtraction.
            let ceiling = max_ctx
                .saturating_sub(overhead)
                .saturating_sub(RESERVE_FOR_RESPONSE);
            assert!(
                budget <= ceiling,
                "budget exceeds remaining capacity: budget={budget} ceiling={ceiling} \
                 overhead={overhead} max={max_ctx}"
            );
        }
    }

    #[test]
    fn healthy_budget_keeps_normal_arithmetic() {
        // 16K window, 5K overhead: 16384 - 5000 - 1024 = 10360.
        assert_eq!(effective_history_budget(16384, 5000), 10360);
        // The old code returned max(10360, 3276) = 10360 too — unchanged here.
    }

    #[test]
    fn starved_budget_degrades_to_zero_not_negative() {
        assert_eq!(effective_history_budget(8192, 8192), 0);
        assert_eq!(effective_history_budget(8192, 9000), 0);
    }

    #[test]
    fn overflow_free_floor_still_helps_small_overheads() {
        // Overhead small → floor (20%) is below the raw budget → no effect.
        // Overhead moderate → raw budget governs. Both directions must hold.
        let b = effective_history_budget(8192, 3000);
        assert_eq!(b, 8192 - 3000 - RESERVE_FOR_RESPONSE);
        assert!(b >= (8192 * 20) / 100);
    }
}
