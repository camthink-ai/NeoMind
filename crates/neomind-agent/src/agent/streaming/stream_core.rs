//! Core text-only streaming response processing.
//!
//! Contains the main `process_stream_events` function for text-only LLM
//! interactions with tool calling support and multi-round ReAct loop.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};

use super::{next_chunk_or_timeout, next_chunk_or_timeout_or_interrupt, StreamWait};

use super::context::{
    build_context_window_with_config, build_context_window_with_summary, ToolExecutionResult,
};
use super::dedup::deduplicate_tool_results;
use super::intent::{all_tools_were_read_only, build_list_only_dead_end_prompt};
use super::resolve::resolve_cached_arguments;
use super::result_format::format_tool_results;
use super::sanitize::sanitize_tool_result_for_prompt;
use super::thinking::cleanup_thinking_content;
use super::tool_detect::detect_json_tool_calls;
use super::tool_exec::execute_tool_with_retry;
use crate::agent::staged::{IntentCategory, IntentClassifier};
use crate::agent::tool_parser::{
    is_degenerate_final_output, parse_tool_calls, remove_tool_calls_from_response,
};
use crate::agent::types::{AgentEvent, AgentInternalState, AgentMessage, ToolCall};
// No stuck-loop breaker in the chat path — the user is watching and can
// abort manually. (The scheduled-agent path brakes via cross-round dedup →
// AllDuplicate; StuckDetector was removed in 0.9.20 — it was unreachable
// behind that same dedup.)
use crate::error::{NeoMindError, Result};
use crate::llm::LlmInterface;
use neomind_core::llm::compaction::CompactionConfig;

/// Configuration for stream processing safeguards
///
/// These safeguards prevent infinite loops and excessive resource usage
/// during LLM streaming operations.
///
/// The default values are synchronized with `neomind_core::llm::backend::StreamConfig`
/// to ensure consistent behavior across the system.
pub struct StreamSafeguards {
    /// Maximum time allowed for entire stream processing (default: 300s)
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

    /// Maximum tool call iterations per request (default: 3)
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
            // Synchronized with StreamConfig::max_stream_duration_secs (2400s)
            // This provides adequate time for thinking models like qwen3-vl:2b
            // to complete extended reasoning before generating content.
            max_stream_duration: Duration::from_secs(2400),

            // No limit on thinking content - let the LLM backend enforce limits
            max_thinking_length: usize::MAX,

            max_content_length: usize::MAX,

            // Tool iterations limit - high limit to support complex multi-step queries
            // Actual loop uses MAX_TOOL_ITERATIONS constant; this value is for truncating
            // tool calls in a single LLM response.
            max_tool_iterations: 100,

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
    /// Set the interrupt signal for this stream
    /// Returns a sender that can be used to trigger the interrupt
    pub fn with_interrupt_signal(mut self, rx: tokio::sync::watch::Receiver<bool>) -> Self {
        self.interrupt_signal = Some(rx);
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
    let user_message = user_message.to_string();

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
        IntentCategory::Help => vec![("识别帮助请求意图", "Intent"), ("提供使用说明", "Response")],
        IntentCategory::General => vec![("理解用户问题", "Intent"), ("生成回复", "Response")],
    };

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
    let stream_result = llm_interface
        .chat_stream_with_history(&user_message, &history_for_llm)
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

            // === SKILL CONTEXT: Clear transient skill context from previous turn ===
            llm_interface.clear_skill_context().await;
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

            // === SAFEGUARD: Track recent chunks for repetition detection ===
            let mut recent_chunks: Vec<String> = Vec::new();
            const RECENT_CHUNK_WINDOW: usize = 10;

            // === SAFEGUARD: Track thinking time and content ===
            let mut thinking_start_time: Option<Instant> = None;
            let mut thinking_timeout_warned = false;
            const THINKING_TIMEOUT_SECS: u64 = 300;

            // === SAFEGUARD: Track recently executed tools for multi-round context ===
            let mut recently_executed_tools: VecDeque<String> = VecDeque::new();
            // Track actual shell command strings (for list-only dead end detection)
            let mut recently_executed_commands: VecDeque<String> = VecDeque::new();
            // Mutation-flavored commands (see intent MUTATION_COMMANDS
            // phrasing) that actually SUCCEEDED this turn, kept as raw
            // command strings — the completeness note judges whether any of
            // them DELIVERED the requested (domain, action) rather than
            // whether any mutation merely happened (live round 7: widget
            // create succeeded, the requested add-to-dashboard never ran).
            let mut successful_mutations: Vec<String> = Vec::new();

            // === SAFEGUARD: Track multi-round tool calling iterations ===
            let mut tool_iteration_count = 0usize;
            const MAX_TOOL_ITERATIONS: usize = 30;
            // Soft wall-clock budget for the tool loop. The chat WS path has no
            // total-turn timeout (only a 1200s per-event idle cap), so a
            // pathological loop grinds forever with no text answer (observed:
            // 16+ min of verify rounds). Exiting here falls through to the
            // forced-summary path below, so the user ALWAYS gets a text reply.
            // Configurable via /api/settings/agent `chat_turn_timeout_secs`
            // (Settings → Preferences); default 1800s. The original hardcoded
            // 240s cut legitimate long multi-step tasks (thinking-model rounds +
            // slow tool executions) off mid-task — the agent "stopped halfway".
            let turn_wall_clock_budget = std::time::Duration::from_secs(
                neomind_storage::AgentDefaults::get().chat_turn_timeout_secs,
            );
            let turn_started_at = Instant::now();
            // Accumulate ALL tool results across rounds for final summary
            let mut all_round_tool_results: Vec<(String, String)> = Vec::new();
            // [identical-call reuse] Small models re-issue byte-identical calls
            // after a SUCCESS (observed: two identical image_edit calls back to
            // back — each a real execution on a 2.6B model, ~30s of pure waste).
            // For deterministic, side-effect-free tools the turn reuses the
            // prior result and tells the model so. Deliberately NOT shell /
            // device control / extension commands: re-running those may be the
            // point (retry after an external state change).
            const REUSABLE_TOOLS: &[&str] = &["image_edit", "vision"];
            // #3 duplicate-call advisory: signatures of NON-reusable tools that
            // SUCCEEDED in the IMMEDIATELY PREVIOUS round. Constraints that keep
            // this safe for legitimate repeats (verified with the user):
            //   - consecutive rounds only — the check→write→verify pattern
            //     (identical reads with a write between) never matches;
            //   - previous call must have SUCCEEDED — retrying a failed call is
            //     correct and never nagged;
            //   - ADVISORY ONLY — the call still executes and its real result is
            //     returned; a note is prepended for the model to read.
            let mut last_round_success_sigs: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let executed_tool_signatures: std::sync::Arc<
                std::sync::Mutex<std::collections::HashMap<String, String>>,
            > = std::sync::Arc::new(std::collections::HashMap::new().into());
            // LOOP BREAK: per-turn failure strikes — signature → (failures,
            // last error). A small model fixated on an invented endpoint
            // (live incident: 15× `web_fetch https://api.neomind.com/...`,
            // every call failing identically) needs a deterministic stop:
            // after 2 failures of the EXACT same tool+arguments, the next
            // identical call is refused with the error text + a redirect.
            let failed_tool_signatures: std::sync::Arc<
                std::sync::Mutex<std::collections::HashMap<String, (u32, String)>>,
            > = std::sync::Arc::new(std::collections::HashMap::new().into());
            // Track per-round thinking and content for persistence (round number → text)
            let mut round_thinking_map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
            let mut round_contents_map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
            // Accumulate ALL rounds' thinking for the message's thinking field
            let mut all_rounds_thinking = String::new();

            // Track whether an incomplete tool call JSON was suppressed
            // (LLM stopped mid-JSON, e.g. hit backend token limit)
            let mut incomplete_tool_json = false;

            // The list-only dead-end forced continuation fires AT MOST ONCE per
            // turn. Re-injecting every round while the condition holds traps the
            // model in an investigate→contradict loop until MAX_TOOL_ITERATIONS
            // (observed: 11 rounds, no final text).
            let mut list_only_dead_end_injected = false;

            // === POST-DEAD-END READ-ONLY STREAK (at most once per turn) ===
            // If the forced continuation was already used and two more rounds
            // pass with only read-only calls, the executor is stuck in an
            // explore loop (v22 field incident 2026-10-08: round after round of
            // --help/list, prompt ballooning to ~23K until the case timed out).
            // One final "act now" directive; if that too is ignored, the
            // existing safety nets (MAX_TOOL_ITERATIONS / stream timeout) end
            // the turn as before.
            let mut post_dead_end_read_only_rounds: u32 = 0;
            let mut explore_recovery_nudged = false;

            // === PRE-INJECTION READ-ONLY STREAK (repetition gate) ===
            // Consecutive all-read-only rounds BEFORE any dead-end injection.
            // The first injection requires the streak to already be ≥1 — a
            // single exploratory list round is ID discovery, not a dead end
            // (live 2026-10-19: `device list` + `dashboard list` was the
            // correct first step of a successful flow, and the round-1
            // injection demanding a phantom action hijacked the turn for
            // 20 minutes). See the coercion evidence gates in intent.rs.
            let mut consecutive_read_only_rounds: u32 = 0;

            // === NARRATION-COLLAPSE GUARD (bounded: a few retries per turn) ===
            // Small models sometimes narrate a promised action ("我现在使用
            // image_edit 工具…") WITHOUT emitting any tool call. Ending the turn
            // there hands the user a promise instead of a result (observed:
            // 0.3s turn end, then "你没有使用工具"). The 2026-10-08 widget-task
            // session showed once-per-turn is too few: the model collapsed FIVE
            // consecutive nudged turns the same way and the user read it as the
            // task being "interrupted". Budgeted retries now; beyond the budget
            // the narration is accepted as the final answer.
            const NARRATION_NUDGE_MAX: usize = 3;
            let mut narration_nudges = 0usize;
            let mut narration_nudge_pending: Option<String> = None;
            // Cleaned narration from the round that triggered the nudge —
            // the restore source when the nudged round degenerates (see the
            // post-nudge degenerate guard near the final save). Always the
            // LATEST narration: a second nudge overwrites it.
            let mut nudge_saved_narration: Option<String> = None;
            // True while processing the round launched by the narration
            // nudge — its output is held to stricter JSON-array suspicion
            // (see might_be_json_start) and degenerate results are restored.
            let mut round_is_nudge = false;

            // === STATED-INTENT MISMATCH GUARD (once per turn) ===
            // The model's own narrated command promises vs what actually
            // ran (see intent::promise_tracker_step). Fires only on
            // REPETITION of a concrete promise without execution — a
            // single stated intent before prep work is legitimate and is
            // never nagged.
            let mut promised_cmd: Option<String> = None;
            let mut promise_rounds: usize = 0;
            let mut promise_directive_sent = false;
            let mut promise_directive_pending: Option<String> = None;

            // User-facing guard notes for THIS turn (incomplete action,
            // guard budget spent): emitted as SystemNote events and
            // attached to the final assistant message — never merged into
            // the reply text (user feedback 2026-10-10).
            let mut turn_system_notes: Vec<String> = Vec::new();

            // === INTENT & PLAN VISUALIZATION ===
            // Send intent and plan events first to show user what's happening
            yield intent_event;
            last_event_time = Instant::now();

            for (step, stage) in &plan_steps {
                yield AgentEvent::plan(*step, *stage);
            }

            // === MULTI-ROUND TOOL CALLING LOOP ===
            // Chat path: no loop breaker — the user is watching and can abort.
            // Safety nets: MAX_TOOL_ITERATIONS (30) + stream timeout (max_stream_duration).
            // (The scheduled-agent path brakes via cross-round dedup → AllDuplicate.)
            'multi_round_loop: loop {
                if tool_iteration_count > 0 {
                    tracing::debug!("Starting tool iteration round {}", tool_iteration_count + 1);

                    // For subsequent rounds, we need a new LLM call with tools enabled.
                    // Use the same budget-managed context builder as the initial call.
                    let state_guard = internal_state.read().await;

                    let history_for_llm: Vec<neomind_core::Message> = {
                        // Build context with the same budget-managed context
                        // builder as the initial call.
                        let config = CompactionConfig::for_context_size(max_context);
                        let compacted = build_context_window_with_config(
                            &state_guard.memory, effective_max, &config
                        );
                        compacted
                        .iter().map(|msg| msg.to_core()).collect::<Vec<_>>()
                    };

                    // Build context for subsequent rounds - tell LLM what happened before
                    let recently_executed: Vec<&str> = recently_executed_tools.iter().map(|s| s.as_str()).collect();
                    drop(state_guard);

                    // A nudge round's model output is held to stricter
                    // JSON-array suspicion (see might_be_json_start below):
                    // the model was ORDERED to emit a tool-call array, so a
                    // bare `[` opening is suspect from the first byte.
                    round_is_nudge = narration_nudge_pending.is_some();
                    let context_msg = if let Some(nudge) = narration_nudge_pending.take() {
                        // Narration-collapse retry: a pointed nudge replaces the
                        // generic round prompt so the model can't pattern-match
                        // its way around it.
                        nudge
                    } else if let Some(directive) = promise_directive_pending.take() {
                        // Stated-intent mismatch: same principle — a pointed
                        // directive replaces the generic round prompt.
                        directive
                    } else if recently_executed.is_empty() {
                        format!(
                            "Round {} of processing. Call ALL needed tools in ONE batch using JSON array format. Give the final response if no more tools needed.",
                            tool_iteration_count + 1
                        )
                    } else {
                        let executed_summary = if recently_executed_commands.is_empty() {
                            recently_executed.iter()
                                .map(|s| format!("- {}", s))
                                .collect::<Vec<_>>()
                                .join("\n")
                        } else {
                            recently_executed_commands.iter()
                                .map(|s| format!("- {}", s))
                                .collect::<Vec<_>>()
                                .join("\n")
                        };

                        // === "LIST-ONLY DEAD END" DETECTION ===
                        // If the user asked for an action (create/delete/control/enable/etc)
                        // but all executed tools were read-only (list/get/latest/history),
                        // inject a FORCED continuation prompt to push the LLM to complete the task.
                        // At most once per turn (see list_only_dead_end_injected).
                        let commands_ref: Vec<&str> = recently_executed_commands.iter().map(|s| s.as_str()).collect();

                        let dead_end_msg = if list_only_dead_end_injected {
                            None
                        } else {
                            build_list_only_dead_end_prompt(
                                &user_message,
                                &commands_ref,
                                &all_round_tool_results,
                                consecutive_read_only_rounds,
                            )
                        };

                        if let Some(dead_end_msg) = dead_end_msg {
                            list_only_dead_end_injected = true;
                            dead_end_msg
                        } else if post_dead_end_read_only_rounds >= 2 && !explore_recovery_nudged {
                            explore_recovery_nudged = true;
                            tracing::warn!(
                                "Read-only explore loop after dead-end continuation \
                                 ({} consecutive rounds) — injecting act-now directive",
                                post_dead_end_read_only_rounds
                            );
                            "You have now listed the relevant CLI surface more than once but the user's action is still not performed. Do NOT call any more list/help/query tools. Either execute the actual action command now with the concrete arguments, or — only if a required value is genuinely missing — ask the user for exactly that value and end your turn.".to_string()
                        } else {
                            // Streak bookkeeping: consecutive all-read-only
                            // rounds feed BOTH the act-now directive above
                            // (post-injection) and the first-injection
                            // repetition gate (pre-injection — see the
                            // coercion evidence gates in intent.rs).
                            let cmds: Vec<&str> =
                                recently_executed_commands.iter().map(|s| s.as_str()).collect();
                            let round_was_read_only =
                                all_tools_were_read_only(&cmds, &all_round_tool_results);
                            if list_only_dead_end_injected {
                                if round_was_read_only {
                                    post_dead_end_read_only_rounds += 1;
                                } else {
                                    post_dead_end_read_only_rounds = 0;
                                }
                            } else if round_was_read_only {
                                consecutive_read_only_rounds += 1;
                            } else {
                                consecutive_read_only_rounds = 0;
                            }
                            // Anti-hallucination guard: small models sometimes
                            // report FAILED commands as successful in the final
                            // answer (observed: 404 on add-components → "已成功
                            // 添加"). When any prior shell command exited non-zero,
                            // remind the model that lying about it is forbidden and
                            // point at the recovery path.
                            let has_failed_cmds = all_round_tool_results.iter().any(|(_, r)| {
                                r.contains("\"exit_code\"") && !r.contains("\"exit_code\":0")
                            });
                            let failure_guard = if has_failed_cmds {
                                "\n\n⚠️ Some earlier command FAILED (exit≠0). You MUST NOT claim failed operations succeeded in your answer. Either fix and retry using the error/suggestion (e.g. resolve the REAL id via `neomind dashboard list` / `device list`), or report the failure honestly with its cause.".to_string()
                            } else {
                                String::new()
                            };
                            format!(
                                "Round {} of processing.\n\n\
                                Previously executed tools (results are in context above):\n{}\n\n\
                                STOP AND THINK: Do you need MORE tools, or can you answer from the results above?\n\
                                - If tools above already returned the data you need → give the final response NOW. Do NOT call them again.\n\
                                - If the original goal is NOT yet verified end-to-end (expected data not returned, created resource not yet readable) → continue with the next tool call now — a plan alone is not a completed task.\n\
                                - If you need different tools → call them in ONE batch using JSON array: [{{\"name\":\"tool\",\"arguments\":{{...}}}}]\n\
                                - NEVER call the same tool with the same arguments — results are already in context.{}",
                                tool_iteration_count + 1,
                                executed_summary,
                                failure_guard
                            )
                        }
                    };

                    tracing::debug!("Multi-round context: {}", context_msg);

                    // Disable thinking for post-tool-execution rounds to preserve generation
                    // budget for content output. Small thinking models (qwen3.5:2b) often
                    // consume all num_predict tokens on thinking, leaving content=0.
                    let thinking_override = {
                        let current_thinking = llm_interface.get_thinking_enabled().await;
                        if current_thinking == Some(true) {
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                "Disabling thinking for post-tool round to preserve content budget"
                            );
                            Some(false)
                        } else {
                            None
                        }
                    };

                    let round_stream_result = llm_interface.chat_stream_with_history_thinking(
                        &context_msg,
                        &history_for_llm,
                        thinking_override
                    ).await;

                    let round_stream = match round_stream_result {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!("Round {} LLM call failed: {}", tool_iteration_count + 1, e);

                            // Instead of just erroring out, try to summarize what we have so far.
                            // This gives the user a meaningful response instead of a blank cutoff.
                            if !all_round_tool_results.is_empty() {
    let deduped_results = deduplicate_tool_results(&all_round_tool_results);
                                let has_errors = deduped_results.iter().any(|(_, result)| {
                                    let lower = result.to_lowercase();
                                    lower.contains("error") || lower.contains("failed") || lower.contains("invalid")
                                });

                                let fallback_prompt = if has_errors {
                                    "The tool calls above encountered errors and the LLM failed to generate a follow-up response. \
                                    Summarize what was attempted and explain the errors to the user in plain language. \
                                    Suggest what the user can do next. Do NOT output any tool calls."
                                } else {
                                    "The tool calls above completed but the LLM failed to generate a follow-up response. \
                                    Summarize the results for the user. Do NOT output any tool calls."
                                };

                                let summary_history: Vec<neomind_core::Message> = {
                                    let state_guard = internal_state.read().await;
                                    let compacted = super::super::compact_tool_results(&state_guard.memory, 2);
                                    compacted.iter().map(|msg| msg.to_core()).collect()
                                };

                                let summary_result = llm_interface.chat_stream_summary(
                                    fallback_prompt,
                                    &summary_history,
                                ).await;

                                match summary_result {
                                    Ok(s) => {
                                        let mut pin = Box::pin(s);
                                        while let Some(chunk) =
                                            next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                                        {
                                            match chunk {
                                                Ok((text, _)) => { yield AgentEvent::content(text); }
                                                Err(_) => break,
                                            }
                                        }
                                    }
                                    Err(se) => {
                                        tracing::error!("Fallback summary also failed: {}", se);
                                        yield AgentEvent::error(format!("Processing failed: {}", e));
                                    }
                                }
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
                    // Reset repetition tracking for the new round to prevent
                    // carry-over from previous rounds causing false positives
                    recent_chunks.clear();
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
                        safeguards.max_stream_duration,
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
                        Ok((text, is_thinking)) => {
                            if text.is_empty() {
                                continue;
                            }

                            // === SAFEGUARD: Repetition detection ===
                            recent_chunks.push(text.clone());
                            if recent_chunks.len() > RECENT_CHUNK_WINDOW {
                                recent_chunks.remove(0);
                            }

                            // NOTE: Per-chunk repetition detection removed — it caused false positives
                            // when the LLM legitimately discusses multiple devices/sensors and words
                            // like "温度", "传感器" appear many times in a normal analysis report.

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
                                            "Thinking timeout detected ({:.1}s elapsed). Model may be stuck in thinking loop.",
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
                                            if !calls.is_empty() {
                                                tool_calls_detected = true;
                                                tool_calls.extend(calls);
                                                had_tool_calls = true;
                                                // Remove tool calls from thinking content
                                                thinking_content = format!("{}{}", &thinking_with_new[..tool_start], &thinking_with_new[tool_end + 13..]);
                                                // Don't yield tool call XML as thinking content
                                                text_to_yield = String::new();
                                                tracing::debug!("Extracted {} tool calls from thinking content", tool_calls.len());
                                            }
                                        }
                                    }
                                }
                                // Also check for JSON tool calls in thinking
                                else if let Some((json_start, tool_json, remaining)) = detect_json_tool_calls(thinking_with_new) {
                                    if let Ok((_, calls)) = parse_tool_calls(&tool_json) {
                                        if !calls.is_empty() {
                                            tool_calls_detected = true;
                                            tool_calls.extend(calls);
                                            had_tool_calls = true;
                                            // Remove tool calls from thinking content
                                            thinking_content = format!("{}{}", &thinking_with_new[..json_start], remaining);
                                            // Don't yield tool call JSON as thinking content
                                            text_to_yield = String::new();
                                            tracing::debug!("Extracted {} JSON tool calls from thinking content", tool_calls.len());
                                        }
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

                            // Add text to buffer
                            buffer.push_str(&text);

                            // Check for tool calls in buffer (support both XML and JSON formats)
                            // Try JSON format first: [{"name": "tool", "arguments": {...}}]
                            let json_tool_check = detect_json_tool_calls(&buffer);
                            if let Some((json_start, tool_json, _remaining)) = json_tool_check {
                                // Found JSON tool calls - only yield content NOT already yielded
                                if json_start > yielded_up_to {
                                    let new_content = &buffer[yielded_up_to..json_start];
                                    if !new_content.is_empty() {
                                        content_before_tools.push_str(new_content);
                                        yield AgentEvent::content(new_content);
                                    }
                                }
                                // Still track ALL content before tools for memory saving
                                let before_tool = &buffer[..json_start];
                                if before_tool.len() > content_before_tools.len() {
                                    content_before_tools = before_tool.to_string();
                                }

                                // Parse the JSON tool calls
                                if let Ok((_, calls)) = parse_tool_calls(&tool_json) {
                                    if !calls.is_empty() {
                                        tool_calls_detected = true;
                                        tool_calls.extend(calls);
                                    }
                                }

                                // Discard remaining content after embedded tool calls.
                                // Models often fabricate tool results after outputting JSON tool calls
                                // in text — these hallucinated results should not be shown to the user.
                                // The real results will come from actual tool execution.
                                buffer.clear();
                                yielded_up_to = 0;
                            } else {
                                // No JSON tool calls detected - check for XML format
                                if let Some(tool_start) = buffer.find("<tool_calls>") {
                                    // Only yield content NOT already yielded
                                    if tool_start > yielded_up_to {
                                        let new_content = &buffer[yielded_up_to..tool_start];
                                        if !new_content.is_empty() {
                                            content_before_tools.push_str(new_content);
                                            yield AgentEvent::content(new_content);
                                        }
                                    }
                                    let before_tool = &buffer[..tool_start];
                                    if before_tool.len() > content_before_tools.len() {
                                        content_before_tools = before_tool.to_string();
                                    }

                                    if let Some(tool_end) = buffer.find("</tool_calls>") {
                                        let tool_content = buffer[tool_start..tool_end + 13].to_string();
                                        // Discard remaining content after XML tool calls (same reason as JSON)
                                        buffer.clear();
                                        yielded_up_to = 0;

                                        if let Ok((_, calls)) = parse_tool_calls(&tool_content) {
                                            if !calls.is_empty() {
                                                tool_calls_detected = true;
                                                tool_calls.extend(calls);
                                            }
                                        }
                                    }
                                } else {
                                    // Check if buffer might contain the START of a JSON tool call.
                                    // Hold back suspicious content to prevent partial JSON
                                    // from being yielded before the full JSON is detected.
                                    let might_be_json_start = buffer.ends_with("[{")
                                        || buffer.ends_with("{\"")
                                        || buffer.ends_with("\"name\"")
                                        || buffer.ends_with("```")
                                        || buffer.ends_with("```json")
                                        || (buffer.contains("[{\"name") && !buffer.contains("]}"))
                                        || (buffer.contains("{\"name\"") && !buffer.contains("}]}"))
                                        // Nudge round: the model was ordered to answer with
                                        // a JSON array — a leading `[` is suspicious even
                                        // before anything else arrives.
                                        || (round_is_nudge && buffer.trim_start().starts_with('['));

                                    if might_be_json_start {
                                        // Don't yield yet — wait for more chunks to determine
                                        // if this is a tool call JSON or normal text.
                                        // Find the earliest suspicious position
                                        let suspicious_pos = {
                                            let mut pos = buffer.len();
                                            if let Some(p) = buffer.rfind("[{") { pos = pos.min(p); }
                                            if let Some(p) = buffer.rfind("{\"") { pos = pos.min(p); }
                                            if let Some(p) = buffer.rfind("```") { pos = pos.min(p); }
                                            if round_is_nudge && buffer.trim_start().starts_with('[') {
                                                if let Some(p) = buffer.find('[') { pos = pos.min(p); }
                                            }
                                            pos
                                        };
                                        if suspicious_pos > yielded_up_to {
                                            let safe_content = &buffer[yielded_up_to..suspicious_pos];
                                            if !safe_content.is_empty() {
                                                content_before_tools.push_str(safe_content);
                                                yield AgentEvent::content(safe_content);
                                            }
                                            yielded_up_to = suspicious_pos;
                                        }
                                    } else if !text.is_empty() {
                                        // Safe to yield — no JSON pattern detected
                                        yield AgentEvent::content(text.clone());
                                        yielded_up_to = buffer.len();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream error: {}", e);
                            yield AgentEvent::error(format!("Stream error: {}", e));
                            // Save partial response on error to maintain conversation context
                            // This prevents the next message from having incomplete context
                            if !buffer.is_empty() || !content_before_tools.is_empty() || !thinking_content.is_empty() {
                                let partial_content = if content_before_tools.is_empty() {
                                    buffer.clone()
                                } else {
                                    content_before_tools.clone()
                                };
                                // Don't persist incomplete tool-call JSON — it would poison the
                                // next turn's context (the LLM sees its own malformed JSON as a
                                // prior assistant message). Only save when it looks like real
                                // content, i.e. not a truncated {/[ fragment with tool markers.
                                let looks_incomplete_tool_json = {
                                    let t = partial_content.trim();
                                    (t.starts_with('{') || t.starts_with('['))
                                        && (t.contains("\"name\"")
                                            || t.contains("\"arguments\"")
                                            || t.contains("\"tool\""))
                                        && !(t.ends_with('}') || t.ends_with(']'))
                                };
                                if looks_incomplete_tool_json {
                                    tracing::warn!(
                                        "Stream error with incomplete tool-call JSON in buffer ({} chars) \
                                         — not persisting to avoid context poisoning",
                                        partial_content.len()
                                    );
                                } else {
                                    let partial_msg = if !thinking_content.is_empty() {
                                        let cleaned_thinking = cleanup_thinking_content(&thinking_content);
                                        AgentMessage::assistant_with_thinking(&partial_content, &cleaned_thinking)
                                    } else {
                                        AgentMessage::assistant(&partial_content)
                                    };
                                    internal_state.write().await.push_message(partial_msg);
                                    tracing::debug!("Saved partial response on error: {} chars content, {} chars thinking",
                                        partial_content.len(), thinking_content.len());
                                }
                            }
                            break;
                        }
                    }
                }

                // Release any held-back content if it turned out NOT to be a tool call.
                // If tool_calls_detected is true, the held content IS part of the tool call JSON
                // and should be discarded (it will not be displayed).
                if !tool_calls_detected && yielded_up_to < buffer.len() {
                    let remaining = &buffer[yielded_up_to..];
                    // Filter out incomplete tool call JSON patterns that leaked through
                    // (happens when LLM hits max_tokens mid-tool-call or stream ends abruptly)
                    let should_suppress = remaining.trim_start().starts_with('[')
                        && (remaining.contains("\"name\"") || remaining.contains("\"arguments\""))
                        && !remaining.trim_end().ends_with(']');
                    // Nudge round whose ENTIRE output is a bare empty JSON
                    // container: the model complied with "output the tool
                    // calls" literally (live 2026-10-20: `[]` on screen after
                    // a complete answer). Suppress it here — the post-nudge
                    // degenerate guard near the final save restores the
                    // pre-nudge narration as the turn's reply.
                    let degenerate_nudge_output =
                        round_is_nudge && is_degenerate_final_output(remaining);
                    if degenerate_nudge_output {
                        tracing::warn!(
                            len = remaining.len(),
                            "Nudge round produced only a bare empty JSON container — suppressing display; the pre-nudge narration will be restored as the final reply"
                        );
                    } else if !remaining.is_empty() && !should_suppress {
                        content_before_tools.push_str(remaining);
                        yield AgentEvent::content(remaining);
                    } else if should_suppress {
                        tracing::warn!(
                            "Detected incomplete tool call JSON ({} chars) — LLM stopped mid-output. \
                             Will trigger summary to guide next step.",
                            remaining.len()
                        );
                        incomplete_tool_json = true;
                    }
                    yielded_up_to = buffer.len();
                }

                // === Handle tool calls if detected ===
                if tool_calls_detected {
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

                    let reuse_map = executed_tool_signatures.clone();
                    let failed_map = failed_tool_signatures.clone();

                    let tool_futures = futures::stream::iter(tool_inputs.into_iter().map(|(i, name, arguments)| {
                        let tools_clone = tools.clone();
                        let cache_clone = cache.clone();
                        let reuse_map = reuse_map.clone();
                        let failed_map = failed_map.clone();

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
                            // Identical-call reuse for deterministic tools.
                            let reusable = REUSABLE_TOOLS.contains(&name.as_str());
                            let sig = format!(
                                "{}|{}",
                                name,
                                serde_json::to_string(&arguments).unwrap_or_default()
                            );

                            // === LOOP BREAK (see failed_tool_signatures) ===
                            if let Some((failures, last_err)) =
                                failed_map.lock().unwrap().get(&sig).cloned()
                            {
                                if let Some(msg) = loop_break_message(failures, &last_err) {
                                    tracing::warn!(
                                        tool = %name,
                                        failures,
                                        "Loop break: identical call already failed {}× this turn — refusing to re-execute",
                                        failures
                                    );
                                    return (i, name.clone(), ToolExecutionResult {
                                        _name: name.clone(),
                                        arguments: arguments.clone(),
                                        result: Err(crate::toolkit::error::ToolError::Execution(msg)),
                                    });
                                }
                            }

                            if reusable {
                                if let Some(prev) = reuse_map.lock().unwrap().get(&sig).cloned() {
                                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&prev) {
                                        if let Some(obj) = v.as_object_mut() {
                                            obj.insert(
                                                "_reused".into(),
                                                serde_json::json!(
                                                    "identical call already executed this turn — result reused, do NOT call again with the same arguments"
                                                ),
                                            );
                                        }
                                        tracing::info!(tool = %name, "Reusing identical tool call from earlier this turn");
                                        return (i, name.clone(), ToolExecutionResult {
                                            _name: name.clone(),
                                            arguments: arguments.clone(),
                                            result: Ok(crate::toolkit::tool::ToolOutput::success(v)),
                                        });
                                    }
                                }
                            }
                            let result =
                                execute_tool_with_retry(&tools_clone, &cache_clone, &name, arguments.clone()).await;

                            // Strike bookkeeping: failures accumulate per
                            // signature; a success clears the strikes.
                            {
                                let failed_now: Option<String> = match &result {
                                    Ok(out) if crate::toolkit::tool::effective_success(&name, out) => {
                                        failed_map.lock().unwrap().remove(&sig);
                                        None
                                    }
                                    Ok(out) => Some(
                                        out.error
                                            .clone()
                                            .or_else(|| {
                                                out.data
                                                    .get("stderr")
                                                    .and_then(|v| v.as_str())
                                                    .map(str::to_string)
                                            })
                                            .unwrap_or_else(|| "tool reported failure".to_string()),
                                    ),
                                    Err(e) => Some(e.to_string()),
                                };
                                if let Some(err) = failed_now {
                                    let mut m = failed_map.lock().unwrap();
                                    let entry = m.entry(sig.clone()).or_insert((0, String::new()));
                                    entry.0 += 1;
                                    entry.1 = err;
                                }
                            }
                            if reusable {
                                if let Ok(out) = &result {
                                    if out.success {
                                        if let Ok(data_str) = serde_json::to_string(&out.data) {
                                            reuse_map.lock().unwrap().insert(sig, data_str);
                                        }
                                    }
                                }
                            }
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
                    let mut this_round_success_sigs: std::collections::HashSet<String> =
                        std::collections::HashSet::new();

                    for (_, name, execution) in tool_results_executed {
                        // Use arguments from the execution result (preserves per-call arguments for same-name tools)
                        let exec_arguments = execution.arguments.clone();
                        yield AgentEvent::tool_call_start_round(&name, exec_arguments.clone(), tool_iteration_count + 1);

                        // Mutation-flavored SHELL commands that effectively
                        // SUCCEEDED this turn, as raw strings — feeds the
                        // turn-end completeness note, which checks whether
                        // any of them DELIVERS the requested action ("a
                        // command ran" is not enough: live, add-components
                        // ran and FAILED, and widget create on another
                        // domain ran and SUCCEEDED).
                        if name == "shell" {
                            if let Some(cmd) =
                                exec_arguments.get("command").and_then(|v| v.as_str())
                            {
                                if super::intent::is_mutation_command(cmd)
                                    && matches!(
                                        &execution.result,
                                        Ok(out) if crate::toolkit::tool::effective_success("shell", out)
                                    )
                                {
                                    successful_mutations.push(cmd.to_string());
                                }
                            }
                        }

                        // Duplicate-call advisory (see last_round_success_sigs):
                        // non-reusable tool, identical signature, succeeded LAST
                        // round. Tag the result value; execution already happened.
                        let dup_sig = format!(
                            "{}|{}",
                            name,
                            serde_json::to_string(&exec_arguments).unwrap_or_default()
                        );
                        let was_dup = !REUSABLE_TOOLS.contains(&name.as_str())
                            && last_round_success_sigs.contains(&dup_sig);

                        match execution.result {
                            Ok(output) => {
                                if crate::toolkit::tool::effective_success(&name, &output) {
                                    this_round_success_sigs.insert(dup_sig);
                                }
                                let mut result_value = if output.success {
                                    output.data.clone()
                                } else {
                                    output.error.clone().map(|e| serde_json::json!({"error": e}))
                                        .unwrap_or_else(|| serde_json::json!("Error"))
                                };
                                if was_dup {
                                    if let Some(obj) = result_value.as_object_mut() {
                                        obj.insert(
                                            "_duplicate".into(),
                                            serde_json::json!(
                                                "you made this EXACT call (same tool + arguments) last round and it already succeeded — if you are verifying a change, state what changed; otherwise do NOT repeat it again / 你上一轮已做过完全相同的调用且已成功；若在验证改动请说明差异，否则不要重复"
                                            ),
                                        );
                                    }
                                    tracing::info!(tool = %name, "Advisory: identical non-reusable call succeeded last round");
                                }
                                let result_str = if output.success {
                                    serde_json::to_string(&output.data).unwrap_or_else(|_| "Success".to_string())
                                } else {
                                    output.error.clone().unwrap_or_else(|| "Error".to_string())
                                };

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

                                yield AgentEvent::tool_call_end_round(&name, &display_str, output.success, tool_iteration_count + 1);

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
                    // Update recently executed tools list (for multi-round context)
                    all_round_tool_results.extend(tool_call_results.iter().cloned());
                    for (name, _result) in &tool_call_results {
                        if !recently_executed_tools.iter().any(|n| n == name) {
                            recently_executed_tools.push_back(name.clone());
                            if recently_executed_tools.len() > 10 {
                                recently_executed_tools.pop_front();
                            }
                            tracing::debug!("Added '{}' to recently executed tools (now: {:?})", name, recently_executed_tools);
                        }
                    }
                    // Track actual shell commands for list-only dead end detection
                    let shell_ran_this_round = tool_calls_to_execute
                        .iter()
                        .any(|tc| tc.name == "shell");
                    let mut round_shell_cmds: Vec<String> = Vec::new();
                    for tc in &tool_calls_to_execute {
                        if tc.name == "shell" {
                            if let Some(cmd) = tc.arguments.get("command").and_then(|v| v.as_str()) {
                                round_shell_cmds.push(cmd.to_string());
                                recently_executed_commands.push_back(cmd.to_string());
                                if recently_executed_commands.len() > 20 {
                                    recently_executed_commands.pop_front();
                                }
                            }
                        }
                    }

                    // [loop-steering hint] Non-aborting nudge when the model keeps
                    // executing similar shell commands without converging (the
                    // 2026-08-17 full eval: 19/154 cases burned their whole budget
                    // circling at 9–15 consecutive similar commands). The hint is
                    // appended to this round's shell tool result, so the LLM sees it
                    // with the next prompt. Deliberately a HINT, not a stop — chat
                    // never force-aborts (by-design; hard brakes are for scheduled
                    // agents only). Fires at streak 4 and every +4 after that.
                    if shell_ran_this_round {
                        if let Some((key, streak)) =
                            super::dedup::similar_command_streak(&recently_executed_commands)
                        {
                            if streak % super::dedup::LOOP_STREAK_THRESHOLD == 0 {
                                let hint = format!(
                                    "\n\nLOOP HINT: you have now run {streak} similar \"{key}\" commands in a row without completing the goal. Change your approach: pick a different subcommand from the shell tool's Domain index, run `neomind <domain> --help` to see what exists, or re-read the original request and check each step's actual result before continuing."
                                );
                                if let Some(last_shell) = tool_call_results
                                    .iter_mut()
                                    .rev()
                                    .find(|(n, _)| n == "shell")
                                {
                                    last_shell.1.push_str(&hint);
                                    tracing::info!(
                                        "[streaming] loop-steering hint injected (streak {} of '{}')",
                                        streak, key
                                    );
                                }
                            }
                        }
                    }

                    // === STATED-INTENT MISMATCH (fires once per turn) ===
                    // Rounds 25-37 of the 2026-10-10 tomato-clock session
                    // each SAID "Let me use the neomind dashboard
                    // add-components command" while running file_write /
                    // skill load instead — every other guard passed (tools
                    // were called, they were not all read-only, narration
                    // was not tool-less). Compare this round's narration
                    // against this round's shell commands and queue the
                    // directive when repetition crosses the threshold.
                    {
                        let narration = remove_tool_calls_from_response(&content_before_tools);
                        super::intent::promise_tracker_step(
                            &narration,
                            &round_shell_cmds,
                            &mut promised_cmd,
                            &mut promise_rounds,
                        );
                        if promise_rounds >= 2 && !promise_directive_sent {
                            if let Some(cmd) = promised_cmd.clone() {
                                promise_directive_sent = true;
                                tracing::warn!(
                                    promised = %cmd,
                                    rounds = promise_rounds,
                                    "Stated-intent mismatch — same command promised without execution, injecting directive"
                                );
                                promise_directive_pending = Some(
                                    super::intent::build_unkept_promise_directive(
                                        &cmd,
                                        promise_rounds,
                                        &narration,
                                    ),
                                );
                            }
                        }
                    }

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
                    let should_continue = !wall_clock_exhausted
                        && tool_iteration_count < MAX_TOOL_ITERATIONS - 1;

                    // === Save assistant message with tool_calls BEFORE tool results ===
                    let response_to_save = if content_before_tools.is_empty() {
                        String::new()
                    } else {
                        remove_tool_calls_from_response(&content_before_tools)
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
                        if tool_name == "skill" {
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
                            let cleaned = remove_tool_calls_from_response(&content_before_tools);
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

                    // === LOOP END: iteration limit or duplicate detected ===
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
                    let last_round_has_errors = deduped_results.iter().any(|(_, result)| {
                        let lower = result.to_lowercase();
                        lower.contains("error") || lower.contains("failed") || lower.contains("invalid")
                            || lower.contains("unauthorized") || lower.contains("401")
                    });
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
                            let compacted = super::super::compact_tool_results(&state_guard.memory, 2);
                            compacted.iter().map(|msg| msg.to_core()).collect()
                        };

                        // Detect whether tool results contain errors to tailor the prompt
                        let has_errors = deduped_results.iter().any(|(_, result)| {
                            let lower = result.to_lowercase();
                            lower.contains("error") || lower.contains("failed") || lower.contains("invalid")
                        });

                        let summary_prompt = if has_errors {
                            "The tool calls above returned errors. \
                            Analyze the errors and explain to the user what went wrong in plain language. \
                            Suggest what the user can do (e.g., provide different parameters, check the device, etc.). \
                            Do NOT output any tool calls — give a direct text response."
                        } else {
                            "Based on the tool execution results in the conversation above, \
                            provide a concise analysis and summary. Do NOT output any tool calls — \
                            give a direct text response to the user's question."
                        };

                        let summary_result = llm_interface.chat_stream_summary(
                            summary_prompt,
                            &summary_history,
                        ).await;

                        let mut final_content = String::new();
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
                        let cleaned_content = remove_tool_calls_from_response(&content_before_tools);
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
                    // No tool calls - save response directly.
                    let mut raw_response = buffer.clone();

                    // === NARRATION-COLLAPSE GUARD ===
                    // The model promised an action in words but emitted no tool
                    // call. Intercept BEFORE the final-save path: record what it
                    // said, nudge once, and run another round. Fires only with
                    // round + wall-clock budget left; a second narration falls
                    // through and is accepted as the final answer.
                    if narration_nudges < NARRATION_NUDGE_MAX
                        && tool_iteration_count < MAX_TOOL_ITERATIONS - 1
                        && turn_started_at.elapsed() < turn_wall_clock_budget
                        && crate::agent::is_narration_without_action(&raw_response)
                    {
                        narration_nudges += 1;
                        tracing::info!(
                            round = tool_iteration_count + 1,
                            chars = raw_response.chars().count(),
                            "Narration without tool call — nudging once"
                        );
                        yield AgentEvent::progress(
                            "Retrying — promised action was not executed...".to_string(),
                            "executing",
                            0,
                        );
                        // The narration was already streamed to the user; record
                        // it so the retry round sees what was said.
                        let said = remove_tool_calls_from_response(&raw_response);
                        // Keep it as the restore source in case the nudged
                        // round degenerates (bare `[]`).
                        nudge_saved_narration = Some(said.clone());
                        internal_state
                            .write()
                            .await
                            .push_message(AgentMessage::assistant(&said));
                        tool_iteration_count += 1;
                        // One language per turn: the nudge is written in the
                        // narration's language (a bilingual instruction made
                        // small models drift into the language read last).
                        narration_nudge_pending = Some(
                            crate::agent::narration_nudge_prompt(&raw_response).to_string(),
                        );
                        yield AgentEvent::IntermediateEnd;
                        continue 'multi_round_loop;
                    }

                    // === RECOVERY: post-nudge degenerate round — restore the narration ===
                    // The nudged round answered the "output the tool calls"
                    // directive with a bare empty JSON container instead of
                    // either a tool call or a real final answer (live
                    // 2026-10-20: the nudged round's entire output was `[]`).
                    // The narration recorded before the nudge was already a
                    // complete streamed reply — restore it deterministically,
                    // with no extra LLM call, so the turn keeps the real
                    // answer instead of a 2-char `[]`. Runs BEFORE the
                    // promise backstop so a restored promise-shaped narration
                    // still gets its progress label.
                    //
                    // Scoped to the NUDGE round only: a LATER round (after the
                    // nudge recovered real tool calls and they executed) can
                    // also end degenerate, and restoring the round-1 narration
                    // there would discard the tool results and resurrect a
                    // promise as the final answer — the exact failure the
                    // guard exists to prevent. Later rounds go through the
                    // no-thinking retry / summary paths instead.
                    if round_is_nudge && is_degenerate_final_output(&raw_response) {
                        if let Some(restored) = nudge_saved_narration.take() {
                            if restored.trim().is_empty() {
                                tracing::warn!(
                                    "Post-nudge round degenerated, but the saved narration cleaned to empty — falling through to the retry path"
                                );
                            } else {
                                tracing::warn!(
                                    restored_chars = restored.chars().count(),
                                    degenerate_chars = raw_response.chars().count(),
                                    "Post-nudge round degenerated to an empty JSON container — restoring the pre-nudge narration as the final reply"
                                );
                                raw_response = restored;
                            }
                        }
                    }

                    // === TURN-END PROMISE BACKSTOP ===
                    // The narration guard above could not fire (nudge budget
                    // spent, iteration cap, or wall clock) yet the final reply
                    // STILL ends on an action promise. Accept it — but LABEL it:
                    // a bare "Let me proceed with the installation:" as the last
                    // message reads to the user like the task finished silently.
                    // Emitted as a SystemNote event (rendered as a callout,
                    // persisted on the message) — NOT merged into the reply
                    // text (user feedback 2026-10-10).
                    if crate::agent::is_narration_without_action(&raw_response) {
                        tracing::warn!(
                            round = tool_iteration_count + 1,
                            nudges = narration_nudges,
                            "Turn ending on an action promise with guard budget spent — appending progress note"
                        );
                        // English only (decision 2026-10-10) — one fixed
                        // language, locale-independent.
                        let note = "The turn ended here (guard budget spent): the reply above is a progress report, not a completed result — the promised next step did NOT run. Send \"continue\" to resume from this point.".to_string();
                        yield AgentEvent::system_note(note.clone());
                        turn_system_notes.push(note);
                    } else if let Some(note) = {
                        // === TURN-END ACTION-COMPLETENESS NOTE ===
                        // Live incident: a 400s turn ended with the requested
                        // "add to dashboard" NEVER executed (files written,
                        // install/add-components skipped) while the summary
                        // (>500 chars, so the promise guard's cap exempted it)
                        // framed the task as done. A follow-up run then ended
                        // with the action ATTEMPTED but every mutation FAILING
                        // — "a command ran" was not enough. Round 7 added the
                        // third shape: a DIFFERENT domain's mutation succeeded
                        // (widget create) while the requested one never ran.
                        // The predicate is now "no successful mutation
                        // DELIVERS the requested (domain, action)".
                        let cmds: Vec<&str> =
                            recently_executed_commands.iter().map(|s| s.as_str()).collect();
                        super::intent::build_incomplete_action_note(
                            &user_message,
                            &cmds,
                            &successful_mutations,
                        )
                    } {
                        tracing::warn!(
                            "Turn ending with the requested ACTION never executed — appending completeness note"
                        );
                        yield AgentEvent::system_note(note.clone());
                        turn_system_notes.push(note);
                    }

                    // `buffer` holds *every* streamed chunk for this round and is the
                    // complete response. `content_before_tools` is only ever assigned
                    // the slice of content sitting in front of a suspected tool-call
                    // by the `might_be_json_start` hold-back heuristic — so when the
                    // model's answer quotes a JSON payload mid-text (e.g. a push log
                    // echoed as `{"source_id": ...}` inside a markdown table cell),
                    // that heuristic leaves a stale fragment behind (e.g. `":`) while
                    // the real summary lives on in `buffer`. Trusting the fragment
                    // over `buffer` here corrupted those summaries. Since no tool
                    // call was detected this round, nothing needs excluding — use the
                    // full buffer. Any residual tool-call JSON is stripped below by
                    // remove_tool_calls_from_response.

                    // === RECOVERY: Incomplete tool call JSON ===
                    // LLM stopped mid-tool-call (e.g. backend token limit).
                    // Use summary call to explain the situation and guide the user.
                    if incomplete_tool_json {
                        tracing::info!(
                            round = tool_iteration_count + 1,
                            "Triggering summary for incomplete tool call JSON"
                        );
                        yield AgentEvent::progress(
                            "Generating response from partial results...".to_string(),
                            "summarizing",
                            0,
                        );

                        let summary_history: Vec<neomind_core::Message> = {
                            let state_guard = internal_state.read().await;
                            let compacted = super::super::compact_tool_results(&state_guard.memory, 2);
                            compacted.iter().map(|msg| msg.to_core()).collect()
                        };

                        let deduped_results = deduplicate_tool_results(&all_round_tool_results);
                        let summary_prompt = if deduped_results.is_empty() {
                            "The previous tool call was interrupted mid-execution. \
                             Summarize what you were trying to do and ask the user \
                             if they want you to continue. \
                             Do NOT output any tool calls — give a direct text response."
                        } else {
                            "Based on the tool execution results gathered so far, \
                             provide a concise summary of what was accomplished. \
                             If the task is incomplete, explain what still needs to be done \
                             and ask the user if they want to continue. \
                             Do NOT output any tool calls — give a direct text response."
                        };

                        let summary_result = llm_interface.chat_stream_summary(
                            summary_prompt,
                            &summary_history,
                        ).await;

                        match summary_result {
                            Ok(stream) => {
                                let mut summary_content = String::new();
                                let mut pin = Box::pin(stream);
                                while let Some(chunk) =
                                    next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                                {
                                    match chunk {
                                        Ok((text, _)) => {
                                            summary_content.push_str(&text);
                                            yield AgentEvent::content(text);
                                        }
                                        Err(e) => {
                                            tracing::error!("Incomplete JSON summary stream error: {}", e);
                                            break;
                                        }
                                    }
                                }
                                if !summary_content.trim().is_empty() {
                                    raw_response = summary_content;
                                }
                            }
                            Err(e) => {
                                tracing::error!("Incomplete JSON summary call failed: {}", e);
                            }
                        }
                    }

                    // === RECOVERY: Retry without thinking when content is empty ===
                    // This handles the case where thinking models consume all generation
                    // budget on thinking tokens, producing no content. Retry once with
                    // thinking forcefully disabled so the model outputs content directly.
                    // Also covers degenerate output — fence-only (e.g. DeepSeek emitting
                    // just "```" as the final answer) and bare empty JSON containers
                    // (`[]`/`{}`) — treated as empty so the retry fires.
                    if is_degenerate_final_output(&raw_response) {
                        let had_thinking = !thinking_content.is_empty();
                        let orig_len = raw_response.len();
                        // Blank out the degenerate buffer so the retry fully replaces it
                        // (the "```" was already streamed to the client, but the persisted
                        // assistant_message will use the retry content below).
                        raw_response.clear();
                        tracing::warn!(
                            had_thinking = had_thinking,
                            orig_len = orig_len,
                            "Stream completed with empty/fence-only content, attempting retry without thinking"
                        );

                        // Build a compact history for the retry (keep last few messages)
                        let state_guard = internal_state.read().await;
                        let retry_history: Vec<neomind_core::Message> = {
                            let non_system: Vec<&AgentMessage> = state_guard.memory.iter()
                                .filter(|m| m.role != "system")
                                .collect();
                            // Keep at most last 6 messages to reduce prompt size
                            let keep = non_system.len().saturating_sub(6);
                            non_system[keep..].iter().map(|m| m.to_core()).collect()
                        };
                        drop(state_guard);

                        // Get original user message from the first message in history
                        let retry_user_msg = retry_history.iter()
                            .find(|m| m.role == neomind_core::MessageRole::User)
                            .map(|m| m.content.as_text())
                            .unwrap_or_default();

                        let retry_prompt = if retry_user_msg.is_empty() {
                            "Please provide a response.".to_string()
                        } else {
                            format!(
                                "Please respond to the user's message directly and concisely.\n\nUser: {}",
                                retry_user_msg
                            )
                        };

                        let retry_result = llm_interface.chat_stream_with_history_thinking(
                            &retry_prompt,
                            &retry_history,
                            Some(false), // Force disable thinking
                        ).await;

                        match retry_result {
                            Ok(retry_stream) => {
                                let mut retry_content = String::new();
                                let mut pin = Box::pin(retry_stream);
                                while let Some(chunk) =
                                    next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                                {
                                    match chunk {
                                        Ok((text, _)) => {
                                            retry_content.push_str(&text);
                                            // Don't yield yet — check for tool calls first
                                        }
                                        Err(e) => {
                                            tracing::error!("Retry stream error: {}", e);
                                            break;
                                        }
                                    }
                                }

                                // Check for tool calls in retry content and strip them.
                                // When the first stream is interrupted, the retry may produce
                                // tool call JSON instead of plain content. We must not yield
                                // raw JSON to the user.
                                let cleaned = match parse_tool_calls(&retry_content) {
                                    Ok((content, calls)) if !calls.is_empty() => {
                                        tracing::warn!(
                                            calls_count = calls.len(),
                                            "Retry produced tool calls instead of content, stripping them"
                                        );
                                        content
                                    }
                                    _ => retry_content.clone(),
                                };

                                if !cleaned.trim().is_empty() {
                                    raw_response = cleaned.clone();
                                    yield AgentEvent::content(cleaned);
                                    tracing::info!(
                                        content_len = raw_response.len(),
                                        "Retry without thinking succeeded"
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
                                        "Stream and no-thinking retry both produced no usable \
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

                    // Clean any embedded tool call JSON from response
                    let response_to_save = remove_tool_calls_from_response(&raw_response);

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

/// Loop-break decision (pure, tested): after `LOOP_BREAK_AFTER_FAILURES`
/// failures of the EXACT same tool+arguments within one turn, refuse the
/// next identical call with the last error and a redirect note. Returns the
/// message to refuse with, or None to proceed.
pub(crate) const LOOP_BREAK_AFTER_FAILURES: u32 = 2;

/// Loop guards use [`crate::toolkit::tool::effective_success`] — the shell
/// tool reports success=true with the exit code inside `data`, so guards
/// reading `ToolOutput.success` alone counted failing shell commands as
/// successes (see the helper's docs for the live incidents).
pub(crate) fn loop_break_message(failures: u32, last_err: &str) -> Option<String> {
    if failures < LOOP_BREAK_AFTER_FAILURES {
        return None;
    }
    Some(format!(
        "LOOP BREAK: this exact call has already FAILED {failures}× this turn — running it unchanged again cannot succeed. Last error: {last_err}. Change the approach NOW: fix the arguments, or switch to the right tool. Tool map: CLI-backed platform domains (dashboards, widgets, devices, rules, agents) → `shell` tool with `neomind ...` (e.g. `neomind dashboard add-components <ID> --components '<json>'`); file work → file_read/file_write/file_edit; image work → vision/image_edit; agent memory → memory. web_fetch is for EXTERNAL URLs only — the platform has no public HTTP API at external domains."
    ))
}

#[cfg(test)]
mod history_budget_tests {
    use super::*;

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

    /// Loop-break state machine: one failure never breaks; the refusal
    /// carries the previous error and the shell/CLI redirect.
    #[test]
    fn loop_break_fires_only_after_repeated_failure() {
        assert!(loop_break_message(0, "err").is_none());
        assert!(loop_break_message(1, "err").is_none());
        let msg = loop_break_message(
            2,
            "error sending request for url (https://api.neomind.com/dashboards)",
        )
        .expect("2 failures must break the loop");
        assert!(msg.contains("FAILED 2"), "{msg}");
        assert!(
            msg.contains("api.neomind.com"),
            "must carry the last error: {msg}"
        );
        assert!(
            msg.contains("Tool map"),
            "must state the tool division: {msg}"
        );
        assert!(
            msg.contains("`shell` tool"),
            "must name shell for CLI domains: {msg}"
        );
        assert!(
            msg.contains("file_read/file_write/file_edit"),
            "must route file work: {msg}"
        );
        assert!(
            msg.contains("vision/image_edit"),
            "must route image work: {msg}"
        );
        assert!(msg.contains("neomind dashboard add-components"), "{msg}");
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
