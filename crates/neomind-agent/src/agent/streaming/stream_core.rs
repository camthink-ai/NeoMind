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

use super::context::ToolExecutionResult;
use super::dedup::deduplicate_tool_results;
use super::intent::{
    all_tools_were_read_only, build_agent_conclusion_redirect, build_device_onboarding_redirect,
    build_list_only_dead_end_prompt,
};
use super::resolve::resolve_cached_arguments;
use super::result_format::format_tool_results;
use super::sanitize::sanitize_tool_result_for_prompt;
use super::thinking::cleanup_thinking_content;
use super::tool_detect::detect_json_tool_calls;
use super::tool_exec::{execute_tool_with_retry, shell_policy_block};
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

    /// Whether the BEHAVIOURAL guards (narration nudge, list-only coercion,
    /// explore recovery) may run on this path. Default **false**: interactive
    /// chat keeps only zero-cost labels (promise backstop, completeness note)
    /// **plus** NM-250's one foreshadow bounded continue (this round ended
    /// foreshadow-only) — that recovery is independent of this flag and does
    /// **not** require that earlier rounds issued no tools.
    ///
    /// Evidence (2026-10-21 A/B, 10 cases x 2 rounds, guards on vs off): no
    /// measurable lift, a weak harm signal (the one coercion-fired case timed
    /// out in both rounds while the off arm produced a verdict), and false
    /// labels were the observable cost. Do **not** flip this whole suite on
    /// for foreshadow stalls; use the narrow foreshadow continue instead.
    /// The scheduled-agent executor (tool_loop) keeps its own nudge because
    /// nobody is there to. Re-enable the full suite only with new evidence.
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

            // Chat policy: labels only. See the field docs.
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

    /// Opt INTO the behavioural guards (nudge / coercion / explore recovery).
    /// Chat leaves this off; it exists so the latent path stays testable and
    /// can be re-enabled with evidence.
    pub fn with_behavioral_guards(mut self, enabled: bool) -> Self {
        self.behavioral_guards = enabled;
        self
    }
}

/// Boxed stream of `(text, is_thinking)` chunks returned by the LLM backend.
pub(super) type LlmChunkStream = Pin<Box<dyn Stream<Item = Result<(String, bool)>> + Send>>;

/// Boxed stream of user-visible [`AgentEvent`] items.
pub(super) type AgentEventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;

/// Mode-specific entry metadata for [`drive_react_stream`].
///
/// Text and multimodal streams differ only in how their first user message is
/// constructed and what initial progress/intent events are emitted; the entire
/// multi-round ReAct loop, two-layer gating, PreparedInvoke host context,
/// single-copy skill tracking, exploration budget, bounded foreshadow continue,
/// and tool-free empty-reply recovery are shared.
pub(super) enum InitialStreamMode {
    Text {
        intent_event: AgentEvent,
        plan_steps: Vec<(&'static str, &'static str)>,
    },
    Multimodal {
        has_images: bool,
    },
}

/// Shared turn-history preparation and dynamic context-window budget calculation
/// used by both text and multimodal streaming entry points.
pub(super) async fn prepare_turn_history_and_budget(
    llm_interface: &Arc<LlmInterface>,
    internal_state: &Arc<tokio::sync::RwLock<AgentInternalState>>,
    user_message: &str,
    conversation_summary: Option<&str>,
    summary_up_to_index: Option<u64>,
) -> std::result::Result<(Vec<neomind_core::Message>, usize), AgentEventStream> {
    let state_guard = internal_state.read().await;
    let mut history_messages = state_guard.memory.clone();
    // Album from full memory (incl. current turn) before depth drain / strip.
    let album = crate::agent::session_album::build_session_album(&history_messages);
    drop(state_guard);

    // [dup-fix] The caller pushes the current user message into memory BEFORE
    // creating this stream, and the LLM layer appends the user message itself
    // when building the request (`msgs.push(user_msg)` in llm.rs) — drop the
    // trailing copy so prompts never carry duplicate current user messages.
    if history_messages
        .last()
        .map(|m| m.role == "user" && m.content.as_ref() == user_message)
        .unwrap_or(false)
    {
        history_messages.pop();
    }

    // === CHAT HISTORY DEPTH (configurable, /api/settings/agent) ===
    let drained = crate::agent::apply_chat_history_depth(&mut history_messages);

    // [summary alignment] shift the full-history summary index into the
    // drained window's index space.
    let summary_up_to_index =
        crate::agent::adjust_summary_index_after_drain(summary_up_to_index, drained);

    // === DYNAMIC CONTEXT WINDOW: Get model's actual capacity ===
    let max_context = llm_interface.max_context_length().await;

    llm_interface
        .set_session_album_prompt(crate::agent::session_album::format_album_prompt_block(
            &album,
            Some(max_context),
        ))
        .await;

    // [image budget] history images are re-sent every turn; keep only recent ones
    // (≤16K windows keep 1; larger keep 2). Annotates durable handles before hydrate.
    crate::agent::strip_stale_images(&mut history_messages, Some(max_context));
    let prompt_overhead = llm_interface.estimate_prompt_overhead_tokens().await;
    let effective_max = effective_history_budget(max_context, prompt_overhead);

    tracing::debug!(
        "Context window: model_capacity={}, prompt_overhead={}, reserve={}, effective_max={} for history",
        max_context,
        prompt_overhead,
        RESERVE_FOR_RESPONSE,
        effective_max
    );

    if prompt_overhead >= max_context {
        let message = format!(
            "上下文窗口不足：系统提示与工具定义约 {prompt_overhead} tokens，而模型窗口只有 {max_context} tokens。请提高上下文设置（例如 NEOMIND_BUILTIN_LLM_CTX=32768）或减少工具/技能。"
        );
        tracing::error!(max_context, prompt_overhead, "{}", message);
        {
            let mut state = internal_state.write().await;
            state.stamp_final_goal_on_last_assistant(
                crate::agent::task_continuity::SystemGoalStatus::NotApplicable,
                crate::agent::task_continuity::StreamEndReason::Error,
            );
        }
        return Err(Box::pin(async_stream::stream! {
            yield AgentEvent::Error { message };
            yield AgentEvent::end()
                .with_goal_status(
                    crate::agent::task_continuity::SystemGoalStatus::NotApplicable,
                )
                .with_end_reason(crate::agent::task_continuity::StreamEndReason::Error);
        }));
    }

    let history_for_llm: Vec<neomind_core::Message> = {
        let built = super::context::build_context_window_with_summary_marked(
            &history_messages,
            effective_max,
            conversation_summary,
            summary_up_to_index,
        );
        if built.pressure || built.latest_tool_feedback_dropped {
            llm_interface.mark_history_truncated();
        }
        if built.latest_tool_feedback_dropped {
            llm_interface.mark_history_window_dropped_latest_feedback();
        }
        built.messages
    }
    .iter()
    .map(|msg| msg.to_core())
    .collect::<Vec<_>>();

    Ok((history_for_llm, effective_max))
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
    // Immutable Turn from AcceptedTurnContext (same lock as message save).
    // Passed locally — never re-read from shared agent state after awaits.
    call_site_turn: u64,
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
    // New user turn: drop transient skill bodies; TaskState (goal / writes)
    // survives on AgentInternalState (accept already applied directives).
    super::super::task_continuity::begin_user_turn_clear_transient_skills(&llm_interface).await;
    {
        let (section, new_op_tools) = {
            let state = internal_state.read().await;
            (
                state.task_state.to_prompt_section(),
                state.task_state.tools_needing_new_operation_protocol(),
            )
        };
        llm_interface.set_task_state_prompt(section).await;
        llm_interface
            .set_new_operation_protocol_tools(new_op_tools)
            .await;
    }
    let turn_context =
        super::context::TurnContext::new(conversation_summary.clone(), summary_up_to_index);
    let user_message = user_message.to_string();

    // === INTENT RECOGNITION: Understand user intent before LLM call ===
    let classifier = IntentClassifier::default();
    let intent_result = classifier.classify(&user_message);

    tracing::info!(
        "Intent recognized: category={:?}, confidence={:.2}, keywords={:?}",
        intent_result.category,
        intent_result.confidence,
        intent_result.keywords
    );

    let intent_event = AgentEvent::intent(
        format!("{:?}", intent_result.category),
        intent_result.category.display_name(),
        intent_result.confidence,
        intent_result.keywords.clone(),
    );

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

    let (history_for_llm, effective_max) = match prepare_turn_history_and_budget(
        &llm_interface,
        &internal_state,
        &user_message,
        conversation_summary.as_deref(),
        summary_up_to_index,
    )
    .await
    {
        Ok(ready) => ready,
        Err(overflow_stream) => return Ok(overflow_stream),
    };

    tracing::debug!(
        "Passing {} messages from history to LLM",
        history_for_llm.len()
    );

    tracing::info!("Thinking control: respecting user/instance thinking_enabled setting directly");

    let stream_result = llm_interface
        .chat_stream_with_history(&user_message, &history_for_llm)
        .await;

    let stream = stream_result.map_err(|e| NeoMindError::Llm(e.to_string()))?;

    Ok(drive_react_stream(
        llm_interface,
        internal_state,
        tools,
        user_message,
        stream,
        safeguards,
        turn_context,
        effective_max,
        call_site_turn,
        InitialStreamMode::Text {
            intent_event,
            plan_steps,
        },
    ))
}

/// Unified multi-round ReAct streaming engine shared by text and multimodal chats.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive_react_stream(
    llm_interface: Arc<LlmInterface>,
    internal_state: Arc<tokio::sync::RwLock<AgentInternalState>>,
    tools: Arc<crate::toolkit::ToolRegistry>,
    user_message: String,
    initial_stream: LlmChunkStream,
    safeguards: StreamSafeguards,
    turn_context: super::context::TurnContext,
    effective_max: usize,
    call_site_turn: u64,
    mode: InitialStreamMode,
) -> AgentEventStream {
    Box::pin(async_stream::stream! {
            // Owner-scoped lease teardown: only THIS stream's Prepared/Running
            // ops are abandoned on interrupt/drop/timeout — never sibling turns.
            let stream_owner = super::super::task_continuity::new_stream_owner();
            let call_site_turn = call_site_turn;
            let has_user_images =
                matches!(&mode, InitialStreamMode::Multimodal { has_images: true });
            let lease_drop_reason = match &mode {
                InitialStreamMode::Text { .. } => "stream dropped or timed out",
                InitialStreamMode::Multimodal { .. } => "stream dropped or timed out (multimodal)",
            };
            let _stream_lease_guard = super::super::task_continuity::StreamLeaseGuard::new(
                internal_state.clone(),
                stream_owner.clone(),
                lease_drop_reason,
            );
            let stream = initial_stream;
            // Turn-scoped `(skill_id, section_opt)` tracker for single-copy skill
            // loading in conversation `tool` history.
            let turn_loaded_skills =
                std::sync::Arc::new(crate::toolkit::skill_tool::TurnLoadedSkills::new());
            let current_turn_id = internal_state.read().await.task_state.turn_count;
            // Emit confirm removals / SystemNotes from accept_user_message.
            {
                let (notices, sys_notes) = {
                    let mut state = internal_state.write().await;
                    (
                        state.task_state.drain_confirm_lifecycle_notices(),
                        state.task_state.drain_pending_system_notes(),
                    )
                };
                for notice in notices {
                    match notice {
                        super::super::task_continuity::ConfirmLifecycleNotice::Requested(
                            confirm,
                        ) => {
                            yield AgentEvent::authorize_confirm_request(&confirm);
                        }
                        super::super::task_continuity::ConfirmLifecycleNotice::Removed {
                            request_id,
                            reason,
                        } => {
                            yield AgentEvent::authorize_confirm_removed(request_id, reason);
                        }
                    }
                }
                for note in sys_notes {
                    yield AgentEvent::system_note(note);
                }
            }
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

            // === SAFEGUARD: Track recent chunks for repetition detection ===
            let mut recent_chunks: Vec<String> = Vec::new();
            const RECENT_CHUNK_WINDOW: usize = 10;

            // === SAFEGUARD: Track thinking time and content ===
            let mut thinking_start_time: Option<Instant> = None;
            let mut thinking_timeout_warned = false;
            const THINKING_TIMEOUT_SECS: u64 = 300;
            // Hard cut for a pure-thinking stall in ONE stream: zero content
            // and zero tool calls for this many seconds means the model is
            // wedged in a think-tag repetition loop. The per-chunk idle
            // timeout (max_stream_duration) never fires in that state
            // because chunks keep arriving every few seconds — there is no
            // TOTAL cap without this. (live 2026-10-27: minicpm5-2b final
            // round, 478s of thinking-only chunks, orig_len=0; llama.cpp
            // ignores request-side thinking toggles, so the model burned
            // its whole generation budget in the think channel.) Breaking
            // the consume loop funnels into the existing empty-content
            // no-thinking retry, so the user still gets an answer.
            const THINKING_STALL_ABORT_SECS: u64 = 300;

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
            // Chat-path loop breaker: a round is a repetition only when its
            // tool calls are ALL already-seen AND its results are byte-identical
            // to the previous round. A verification poll whose result changed,
            // or any new call, resets it. Three consecutive identical-result
            // rounds = a loop; the 30-round hard cap and wall clock remain.
            const CHAT_IDENTICAL_ROUNDS_LIMIT: usize = 3;
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
            let mut any_failed_tool_call = false;
            // Every (tool + arguments) call executed this turn; a round with
            // zero NEW signatures is repetition, not exploration.
            let mut seen_tool_sigs: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut loop_progress = ChatLoopProgress::default();
            let mut skill_exploration = super::guard::SkillExplorationBudget::default();
            let mut guide_recovery_pending = false;
            let mut guide_recovery_used = false;
            let mut seen_non_skill_outcomes = std::collections::HashSet::new();
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
            // Round 3: count status-poll rounds that produced new evidence
            // (timed allow under progress_loop_decision).
            let mut status_poll_progress_rounds: usize = 0;

            // === PRE-INJECTION READ-ONLY STREAK (repetition gate) ===
            // Consecutive all-read-only rounds BEFORE any dead-end injection.
            // The first injection requires the streak to already be ≥1 — a
            // single exploratory list round is ID discovery, not a dead end
            // (live 2026-10-19: `device list` + `dashboard list` was the
            // correct first step of a successful flow, and the round-1
            // injection demanding a phantom action hijacked the turn for
            // 20 minutes). See the coercion evidence gates in intent.rs.
            let mut consecutive_read_only_rounds: u32 = 0;

            // === NARRATION-COLLAPSE GUARD (one retry per turn) ===
            // Small models sometimes narrate a promised action ("我现在使用
            // image_edit 工具…") WITHOUT emitting any tool call. Ending the turn
            // there hands the user a promise instead of a result (observed:
            // 0.3s turn end, then "你没有使用工具").
            //
            // Budget is ONE. The old budget of three came from the 2026-10-08
            // session (five consecutive collapses), but that showed extra
            // rounds do NOT fix a model that will not act — my 2026-10-21 run
            // reproduced it: three nudges, the model repeated the same promise
            // three times, tens of seconds per round wasted. This check is a
            // SEMANTIC guess over prose, unlike Aider/Cline's structural
            // "no edit block / no tool call" retries, so one attempt is all it
            // gets; beyond that the turn-end backstop LABELS the reply.
            const NARRATION_NUDGE_MAX: usize = 1;
            let mut narration_nudges = 0usize;
            let mut narration_nudge_pending: Option<String> = None;
            // NM-250: one foreshadow continue under default chat policy
            // (`behavioral_guards` off). Independent of narration_nudges so
            // opt-in behavioural path accounting stays unchanged.
            let mut foreshadow_continues = 0usize;
            const FORESHADOW_CONTINUE_MAX: usize = 1;
            // Agent status question answered without `latest-execution` —
            // one bounded redirect (same policy class as foreshadow).
            let mut agent_conclusion_redirect_sent = false;
            let mut agent_conclusion_redirect_pending: Option<String> = None;
            // Guided MQTT onboarding without `device create`.
            let mut device_onboarding_redirect_sent = false;
            let mut device_onboarding_redirect_pending: Option<String> = None;
            // Cleaned narration from the round that triggered the nudge —
            // the restore source when the nudged round degenerates (see the
            // post-nudge degenerate guard near the final save). Always the
            // LATEST narration: a second nudge overwrites it.
            let mut nudge_saved_narration: Option<String> = None;
            // The user explicitly asked for a plan/explanation only ("先不要
            // 执行", "just tell me"): a promise-shaped reply IS the requested
            // output, so neither the nudge nor the progress-report label may
            // fire — forcing execution would contradict the instruction.
            let no_execution_requested = crate::agent::user_requested_no_execution(&user_message);
            // Greeting inputs ("你好") request no action — suppress the
            // narration label for their promise-shaped replies.
            let greeting_input = crate::agent::is_simple_greeting(&user_message);
            // Eval-only ablation switch (`NEOMIND_GUARD_MODE=off`): A/B runs use
            // it to measure the guards' net lift. Output sanitizers (degenerate
            // `[]` recovery) and safety caps stay active.
            let guards_disabled = super::guard::guards_disabled(
                std::env::var("NEOMIND_GUARD_MODE").ok().as_deref(),
            );
            // Chat policy: labels only (see StreamSafeguards::behavioral_guards).
            let behavioral_guards = safeguards.behavioral_guards;
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
            let mut skill_action_exit_hint_pending: Option<String> = None;

            // User-facing guard notes for THIS turn (incomplete action,
            // guard budget spent): emitted as SystemNote events and
            // attached to the final assistant message — never merged into
            // the reply text (user feedback 2026-10-10).
            let mut turn_system_notes: Vec<String> = Vec::new();

            // Every guard decision lands here as a structured event, so a
            // turn can be summarized and guard hit rates aggregated instead
            // of reconstructed from prose logs after an incident.
            let mut guard_ledger = super::guard::GuardLedger::new();
            if no_execution_requested {
                guard_ledger.record(
                    super::guard::GuardKind::PlanOnlySuppression,
                    super::guard::GuardOutcome::Suppressed,
                    0,
                    "user asked for a plan",
                );
            }
            if guards_disabled {
                guard_ledger.record(
                    super::guard::GuardKind::GuardsDisabled,
                    super::guard::GuardOutcome::Suppressed,
                    0,
                    "guard_mode_off",
                );
            } else if !behavioral_guards {
                guard_ledger.record(
                    super::guard::GuardKind::BehavioralGuardsOff,
                    super::guard::GuardOutcome::Suppressed,
                    0,
                    "chat_policy",
                );
            }

            // === INTENT & PLAN / IMAGE ANALYSIS VISUALIZATION ===
            match mode {
                InitialStreamMode::Text {
                    intent_event,
                    plan_steps,
                } => {
                    yield intent_event;
                    last_event_time = Instant::now();
                    for (step, stage) in &plan_steps {
                        yield AgentEvent::plan(*step, *stage);
                    }
                }
                InitialStreamMode::Multimodal { has_images } => {
                    if has_images {
                        yield AgentEvent::progress("正在分析图像...", "analyzing", 0);
                        last_event_time = Instant::now();
                    }
                }
            }

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
                let guide_recovery_this_round = guide_recovery_pending;
                guide_recovery_pending = false;
                if tool_iteration_count > 0 {
                    tracing::debug!("Starting tool iteration round {}", tool_iteration_count + 1);

                    // For subsequent rounds, we need a new LLM call with tools enabled.
                    // Use the same budget-managed context builder as the initial call.
                    let state_guard = internal_state.read().await;

                    let history_for_llm: Vec<neomind_core::Message> = {
                        // Build context with the same budget-managed context
                        // builder as the initial call.
                        let (compacted, outer_feedback_dropped) = turn_context.build_for_llm(&state_guard.memory, effective_max, &llm_interface);
                        if outer_feedback_dropped {
                            // Tool-capable request follows: feed the assemble gate so tools
                            // are refused when the newest tool feedback was evicted (the inner
                            // selector cannot see the loss).
                            llm_interface.mark_history_window_dropped_latest_feedback();
                        }
                        compacted.iter().map(|msg| {
                            if !guide_recovery_this_round { return msg.to_core(); }
                            let mut clean = msg.clone();
                            if let Some(calls) = clean.tool_calls.as_mut() {
                                for call in calls {
                                    if crate::tools::resolve_tool_name(&call.name) == "skill" { call.result = None; }
                                }
                            }
                            if clean.role == "tool" && clean.tool_call_name.as_deref() == Some("skill") {
                                clean.content = "Workflow guide already consulted; no new task evidence.".into();
                            }
                            clean.to_core()
                        }).collect::<Vec<_>>()
                    };

                    // Build context for subsequent rounds - tell LLM what happened before
                    let recently_executed: Vec<&str> = recently_executed_tools.iter().map(|s| s.as_str()).collect();
                    drop(state_guard);

                    // A nudge round's model output is held to stricter
                    // JSON-array suspicion (see might_be_json_start below):
                    // the model was ORDERED to emit a tool-call array, so a
                    // bare `[` opening is suspect from the first byte.
                    round_is_nudge = narration_nudge_pending.is_some();
                    let context_msg = if guide_recovery_this_round {
                        super::guard::guide_recovery_directive().to_string()
                    } else if let Some(nudge) = narration_nudge_pending.take() {
                        // Narration-collapse retry: a pointed nudge replaces the
                        // generic round prompt so the model can't pattern-match
                        // its way around it.
                        nudge
                    } else if let Some(directive) = promise_directive_pending.take() {
                        // Stated-intent mismatch: same principle — a pointed
                        // directive replaces the generic round prompt.
                        directive
                    } else if let Some(redirect) = agent_conclusion_redirect_pending.take() {
                        // Status-question → latest-execution redirect.
                        redirect
                    } else if let Some(redirect) = device_onboarding_redirect_pending.take() {
                        redirect
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

                        // Agent status → latest-execution redirect (default
                        // policy; independent of behavioral_guards). Prefer
                        // this over generic continuation when applicable.
                        let conclusion_redirect_msg = if guards_disabled
                            || agent_conclusion_redirect_sent
                        {
                            None
                        } else {
                            build_agent_conclusion_redirect(&user_message, &commands_ref)
                        };

                        let onboarding_redirect_msg = if guards_disabled
                            || device_onboarding_redirect_sent
                        {
                            None
                        } else {
                            build_device_onboarding_redirect(&user_message, &commands_ref)
                        };

                        let dead_end_msg = if guards_disabled
                            || !behavioral_guards
                            || list_only_dead_end_injected
                        {
                            None
                        } else {
                            build_list_only_dead_end_prompt(
                                &user_message,
                                &commands_ref,
                                &all_round_tool_results,
                                consecutive_read_only_rounds,
                            )
                        };

                        if let Some(redirect) = conclusion_redirect_msg {
                            agent_conclusion_redirect_sent = true;
                            guard_ledger.record(
                                super::guard::GuardKind::AgentConclusionRedirect,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "status_without_conclusion",
                            );
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                "Agent status question without conclusion read — mid-turn redirect"
                            );
                            redirect
                        } else if let Some(redirect) = onboarding_redirect_msg {
                            device_onboarding_redirect_sent = true;
                            guard_ledger.record(
                                super::guard::GuardKind::DeviceOnboardingRedirect,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "onboarding_without_create",
                            );
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                "Guided device onboarding without create — mid-turn redirect"
                            );
                            redirect
                        } else if let Some(dead_end_msg) = dead_end_msg {
                            list_only_dead_end_injected = true;
                            guard_ledger.record(
                                super::guard::GuardKind::ListOnlyCoercion,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "only_read_only_commands",
                            );
                            dead_end_msg
                        } else if behavioral_guards
                            && !guards_disabled
                            && post_dead_end_read_only_rounds >= 2
                            && !explore_recovery_nudged
                        {
                            explore_recovery_nudged = true;
                            guard_ledger.record(
                                super::guard::GuardKind::ExploreRecovery,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "read_only_streak",
                            );
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
                            // Anti-hallucination guard + E4 structured SystemGoalStatus
                            // hard-fact block (with per-target receipts) when any failure,
                            // partial failure, unknown operation, or Incomplete goal exists.
                            let goal_fact_block = {
                                let state_guard = internal_state.read().await;
                                state_guard
                                    .task_state
                                    .build_summary_goal_status_fact_block(
                                        current_turn_id,
                                        any_failed_tool_call,
                                    )
                            };
                            let failure_guard = match goal_fact_block {
                                Some(block) => format!("\n\n{block}"),
                                None => String::new(),
                            };
                            let action_exit_guard = match skill_action_exit_hint_pending.take() {
                                Some(hint) => format!("\n\n{}", hint),
                                None => String::new(),
                            };
                            format!(
                                "Round {} of processing.\n\n\
                                Previously executed tools (results are in context above):\n{}\n\n\
                                STOP AND THINK: Do you need MORE tools, or can you answer from the results above?\n\
                                - If tools above already returned the data you need → give the final response NOW. Do NOT call them again.\n\
                                - If information that ONLY the user can provide is missing → ask ONE concise clarifying question in your final answer; do not guess and do not keep calling tools to search for it.\n\
                                - If the original goal is NOT yet verified end-to-end (expected data not returned, created resource not yet readable) → continue with the next tool call now — a plan alone is not a completed task.\n\
                                - If you need different tools → call them in ONE batch using JSON array: [{{\"name\":\"tool\",\"arguments\":{{...}}}}]\n\
                                - Repeat a read-only call only when fresh state is needed to verify progress; otherwise use the results already in context.{}{}",
                                tool_iteration_count + 1,
                                executed_summary,
                                failure_guard,
                                action_exit_guard
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

                    let round_stream_result = if guide_recovery_this_round {
                        // Result persistence may have reattached the last guide.
                        // Clear it immediately before constructing this request.
                        llm_interface.clear_skill_context().await;
                        llm_interface.chat_stream_tool_followup_without_guides(
                            &user_message, &context_msg, &history_for_llm, thinking_override,
                        ).await
                    } else {
                        llm_interface.chat_stream_tool_followup(
                            &user_message, &context_msg, &history_for_llm, thinking_override,
                        ).await
                    };

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

                                let fallback_base = if has_errors {
                                    "The tool calls above encountered errors and the LLM failed to generate a follow-up response. \
                                    Summarize what was attempted and explain the errors to the user in plain language. \
                                    Suggest what the user can do next. Do NOT output any tool calls."
                                } else {
                                    "The tool calls above completed but the LLM failed to generate a follow-up response. \
                                    Summarize the results for the user. Do NOT output any tool calls."
                                };
                                let fallback_fact_block = {
                                    let state_guard = internal_state.read().await;
                                    state_guard
                                        .task_state
                                        .build_summary_goal_status_fact_block(
                                            current_turn_id,
                                            any_failed_tool_call || has_errors,
                                        )
                                };
                                let fallback_prompt = match fallback_fact_block {
                                    Some(block) => format!("{fallback_base}\n\n{block}"),
                                    None => fallback_base.to_string(),
                                };

                                let summary_history: Vec<neomind_core::Message> = {
                                    let state_guard = internal_state.read().await;
                                    let (compacted, _) = turn_context.build_for_llm(&state_guard.memory, effective_max, &llm_interface);
                                    compacted.iter().map(|msg| msg.to_core()).collect()
                                };

                                let summary_result = llm_interface.chat_stream_summary_for_task(
                                    &user_message,
                                    &fallback_prompt,
                                    &summary_history,
                                ).await;

                                match summary_result {
                                    Ok(s) => {
                                        let mut pin = Box::pin(s);
                                        while let Some(chunk) =
                                            next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                                        {
                                            match chunk {
                                                Ok((text, is_thinking)) => {
                                                    if !is_thinking {
                                                        yield AgentEvent::content(text);
                                                    }
                                                }
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
                            {
                                let mut state = internal_state.write().await;
                                state.task_state.abandon_owned_operations_on_interrupt(
                                    &stream_owner,
                                    "user interrupt during stream wait",
                                );
                            }
                            yield AgentEvent::content("\n\n[Interrupted]");
                            {
                                let mut state = internal_state.write().await;
                                let reason =
                                    crate::agent::task_continuity::StreamEndReason::Interrupted;
                                let goal = state.task_state.system_goal_status();
                state.stamp_final_goal_on_last_assistant(goal, reason);
                                yield AgentEvent::final_end(&state.task_state, reason, None);
                            }
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
                        {
                            let mut state = internal_state.write().await;
                            state.task_state.abandon_owned_operations_on_interrupt(
                                &stream_owner,
                                "user interrupt",
                            );
                        }
                        yield AgentEvent::content("\n\n[Interrupted]");
                        {
                            let mut state = internal_state.write().await;
                            let reason =
                                crate::agent::task_continuity::StreamEndReason::Interrupted;
                            let goal = state.task_state.system_goal_status();
                state.stamp_final_goal_on_last_assistant(goal, reason);
                            yield AgentEvent::final_end(&state.task_state, reason, None);
                        }
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

                                    // Hard abort: a pure-thinking stall in THIS
                                    // stream (see THINKING_STALL_ABORT_SECS).
                                    // The gate excludes any stream that has
                                    // produced content or a tool call — only a
                                    // wedged think-loop reaches this. Breaks the
                                    // chunk-consume loop; the empty buffer then
                                    // flows into the empty-content no-thinking
                                    // retry below, so the turn still answers.
                                    if !has_content
                                        && !tool_calls_detected
                                        && thinking_elapsed
                                            > Duration::from_secs(THINKING_STALL_ABORT_SECS)
                                    {
                                        tracing::warn!(
                                            elapsed_secs = thinking_elapsed.as_secs(),
                                            thinking_chars =
                                                thinking_content.chars().count(),
                                            "Thinking-only stall: aborting the stream, falling through to the no-thinking retry"
                                        );
                                        yield AgentEvent::warning(
                                            "The model is stuck in a thinking loop — cutting it off and retrying without thinking. / 模型陷入思考循环，正在切断并以禁用思考的方式重试。".to_string()
                                        );
                                        break;
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

                if let Some(prompt_tokens) = llm_interface.peek_last_prompt_tokens().await {
                    // Invalid (0/MAX) meters stay on last_prompt_tokens for End,
                    // but must not paint the context ring as a real bill.
                    if neomind_core::plausible_prompt_token_meter(prompt_tokens) {
                        // S3-3: fold measured prompt_eval into EWMA, then emit a
                        // calibrated breakdown that never exceeds measured total.
                        llm_interface
                            .observe_prompt_calibration(prompt_tokens)
                            .await;
                        let (system_prompt_tokens, tool_tokens) = llm_interface
                            .calibrated_context_breakdown(prompt_tokens, has_user_images)
                            .await;
                        yield AgentEvent::ContextUsage {
                            prompt_tokens,
                            system_prompt_tokens,
                            tool_tokens,
                            max_context_tokens: llm_interface.max_context_length().await,
                        };
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
                        guard_ledger.record(
                            super::guard::GuardKind::DegenerateSuppressed,
                            super::guard::GuardOutcome::Fired,
                            tool_iteration_count + 1,
                            "bare_empty_container",
                        );
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
                    // Nudge effectiveness: the retried round actually acted.
                    if round_is_nudge {
                        guard_ledger.record(
                            super::guard::GuardKind::NarrationNudgeOutcome,
                            super::guard::GuardOutcome::Fired,
                            tool_iteration_count + 1,
                            "tool_call_in_nudge_round",
                        );
                    }
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
                    // The 4th element is the `$cached:` read-back short-circuit:
                    // file_read with a $cached path is served from the cache itself.
                    let tool_inputs: Vec<(usize, String, serde_json::Value, Option<crate::toolkit::ToolOutput>)> =
                        tool_calls_to_execute
                            .iter()
                            .enumerate()
                            .map(|(i, tc)| {
                                let cached_read = super::resolve::try_cached_file_read(
                                    &tools,
                                    &tc.name,
                                    &tc.arguments,
                                    &large_cache,
                                );
                                (
                                    i,
                                    tc.name.clone(),
                                    resolve_cached_arguments(&tc.arguments, &large_cache, &tc.name),
                                    cached_read,
                                )
                            })
                            .collect();

                    let reuse_map = executed_tool_signatures.clone();
                    let failed_map = failed_tool_signatures.clone();
                    let state_for_write_gate = internal_state.clone();
                    let stream_owner_for_tools = stream_owner.clone();
                    let call_site_turn_for_tools = call_site_turn;
                    let turn_loaded_skills_for_tools = turn_loaded_skills.clone();
                    let tool_futures = futures::stream::iter(tool_inputs.into_iter().map(|(i, name, arguments, cached_read)| {
                        let tools_clone = tools.clone();
                        let cache_clone = cache.clone();
                        let reuse_map = reuse_map.clone();
                        let failed_map = failed_map.clone();
                        let state_for_write_gate = state_for_write_gate.clone();
                        let stream_owner = stream_owner_for_tools.clone();
                        let call_site_turn = call_site_turn_for_tools;
                        let turn_loaded_skills = turn_loaded_skills_for_tools.clone();

                        async move {
                            // `$cached:` read-back: served from the cache in
                            // full — no registry execution, no reuse bookkeeping.
                            if let Some(output) = cached_read {
                                return (
                                    i,
                                    name.clone(),
                                    ToolExecutionResult {
                                        _name: name.clone(),
                                        arguments,
                                        result: Ok(output),
                                        operation_id: None,
                                    },
                                );
                            }
                            // Enforce the request-scoped exclusion even for a model
                            // that emits an unadvertised tool in plain JSON.
                            if guide_recovery_this_round
                                && crate::tools::resolve_tool_name(&name) == "skill"
                            {
                                return (
                                    i,
                                    name.clone(),
                                    ToolExecutionResult {
                                        _name: name.clone(),
                                        arguments: arguments.clone(),
                                        result: Err(crate::toolkit::ToolError::Execution(
                                            "The skill tool is unavailable for this recovery request. Use another available capability or answer honestly.".to_string()
                                        )),
                                        operation_id: None,
                                    },
                                );
                            }
                            // Shared shell deny-list gate (blocks catastrophic commands).
                            if let Some(blocked) = shell_policy_block(&name, &arguments) {
                                return (i, name.clone(), blocked);
                            }

                            // =========================================================
                            // TWO-LAYER TOOL GATE (single canonical fingerprint):
                            //   Layer 1 (Read-Only): turn-local deterministic reuse
                            //     (`REUSABLE_TOOLS`) + `ToolResultCache` + duplicate
                            //     advisory + no-progress loop tracking.
                            //   Layer 2 (Write): `OperationLedger` (`TaskState`) owns
                            //     idempotency, InProgress lease, Unknown confirm,
                            //     Succeeded replay block, and Rejected pre-validation
                            //     exemption. `failed_tool_signatures` uses the exact
                            //     same canonical fingerprint as a consecutive real-
                            //     failure circuit breaker without preempting PreferKnown.
                            // =========================================================
                            let real_name = crate::tools::resolve_tool_name(&name);
                            let canonical_sig =
                                super::intent::canonical_tool_call_fingerprint(&name, &arguments);
                            // Failure breaker may collapse transform --code churn
                            // (see failure_circuit_fingerprint); PreferKnown /
                            // reuse / progress tracking keep full canonical_sig.
                            let failure_sig =
                                super::intent::failure_circuit_fingerprint(&name, &arguments);

                            // Layer 1a: identical-call reuse for deterministic tools.
                            // Checked before the write gate so a same-turn
                            // image_edit/vision re-call keeps the `_reused` marker
                            // instead of short-circuiting on PreferKnown.
                            let reusable = REUSABLE_TOOLS.contains(&real_name.as_str());
                            if reusable {
                                if let Some(prev) =
                                    reuse_map.lock().unwrap().get(&canonical_sig).cloned()
                                {
                                    if let Ok(mut v) =
                                        serde_json::from_str::<serde_json::Value>(&prev)
                                    {
                                        if let Some(obj) = v.as_object_mut() {
                                            obj.insert(
                                                "_reused".into(),
                                                serde_json::json!(
                                                    "identical call already executed this turn — result reused, do NOT call again with the same arguments"
                                                ),
                                            );
                                        }
                                        tracing::info!(
                                            tool = %name,
                                            "Reusing identical tool call from earlier this turn"
                                        );
                                        return (
                                            i,
                                            name.clone(),
                                            ToolExecutionResult {
                                                _name: name.clone(),
                                                arguments: arguments.clone(),
                                                result: Ok(
                                                    crate::toolkit::tool::ToolOutput::success(v),
                                                ),
                                                operation_id: None,
                                            },
                                        );
                                    }
                                }
                            }

                            // Layer 2: Write gate / OperationLedger + failure circuit breaker.
                            // NM-240: bind PreparedInvoke id to this dispatch
                            // before write-gate / execute (no global FIFO claim).
                            let mut arguments = arguments;
                            let _cap = super::super::task_continuity::attach_chat_prepared_invoke(
                                &name,
                                &mut arguments,
                            );
                            let mut tracked_operation_id: Option<String> = None;
                            if let Some(fp) =
                                super::intent::mutating_write_fingerprint(&name, &arguments)
                            {
                                use super::super::task_continuity::{
                                    MutatingGateOutcome, WriteDispatch,
                                };
                                let mut state = state_for_write_gate.write().await;
                                // Never let the consecutive-failure circuit breaker
                                // preempt `OperationLedger` (InProgress / PreferKnown /
                                // Unknown confirm / Succeeded replay block). Only check
                                // the circuit breaker when the ledger would start a fresh
                                // execution (first attempt, post-Rejected retry, or
                                // explicit `_new_operation: true`) — before creating a
                                // Running lease so a refused loop leaves no orphan lease.
                                let ledger_authoritative_non_exec =
                                    !super::super::task_continuity::wants_new_operation(&arguments)
                                        && !matches!(
                                            state.task_state.dispatch_mutating_write(&fp),
                                            WriteDispatch::Execute { .. }
                                        );
                                if !ledger_authoritative_non_exec {
                                    if let Some((failures, last_err)) =
                                        failed_map.lock().unwrap().get(&failure_sig).cloned()
                                    {
                                        if let Some(msg) = loop_break_message(failures, &last_err) {
                                            drop(state);
                                            tracing::warn!(
                                                tool = %name,
                                                failures,
                                                "Loop break: identical canonical call already failed {}× this turn — refusing to re-execute",
                                                failures
                                            );
                                            return (
                                                i,
                                                name.clone(),
                                                ToolExecutionResult {
                                                    _name: name.clone(),
                                                    arguments: arguments.clone(),
                                                    result: Err(
                                                        crate::toolkit::error::ToolError::Execution(
                                                            msg,
                                                        ),
                                                    ),
                                                    operation_id: None,
                                                },
                                            );
                                        }
                                    }
                                }
                                match state.evaluate_mutating_gate_durable(
                                    &name,
                                    &arguments,
                                    &fp,
                                    Some(stream_owner.as_str()),
                                    call_site_turn,
                                ) {
                                    MutatingGateOutcome::Proceed { operation_id } => {
                                        tracked_operation_id = Some(operation_id);
                                    }
                                    MutatingGateOutcome::InProgress { operation_id } => {
                                        tracing::info!(
                                            tool = %name,
                                            operation_id = %operation_id,
                                            fingerprint = %fp,
                                            "Concurrent lease: operation already Running — wait"
                                        );
                                        return (
                                            i,
                                            name.clone(),
                                            ToolExecutionResult {
                                                _name: name.clone(),
                                                arguments: arguments.clone(),
                                                result: Ok(
                                                    super::super::task_continuity::in_progress_tool_output(
                                                        &operation_id,
                                                    ),
                                                ),
                                                operation_id: Some(operation_id),
                                            },
                                        );
                                    }
                                    MutatingGateOutcome::ResumeKnown {
                                        operation_id,
                                        output,
                                    } => {
                                        tracing::info!(
                                            tool = %name,
                                            operation_id = %operation_id,
                                            fingerprint = %fp,
                                            "Same-op resume: preferring known Operation result"
                                        );
                                        return (
                                            i,
                                            name.clone(),
                                            ToolExecutionResult {
                                                _name: name.clone(),
                                                arguments: arguments.clone(),
                                                result: Ok(output),
                                                operation_id: Some(operation_id),
                                            },
                                        );
                                    }
                                    MutatingGateOutcome::Block { message, confirm: _ } => {
                                        tracing::info!(
                                            tool = %name,
                                            fingerprint = %fp,
                                            "Write gate blocked mutating tool"
                                        );
                                        return (
                                            i,
                                            name.clone(),
                                            ToolExecutionResult {
                                                _name: name.clone(),
                                                arguments: arguments.clone(),
                                                result: Err(
                                                    crate::toolkit::error::ToolError::Execution(
                                                        message,
                                                    ),
                                                ),
                                                operation_id: None,
                                            },
                                        );
                                    }
                                }
                            } else {
                                // Read-only call: apply consecutive real-failure
                                // circuit breaker on the failure-circuit fingerprint.
                                if let Some((failures, last_err)) =
                                    failed_map.lock().unwrap().get(&failure_sig).cloned()
                                {
                                    if let Some(msg) = loop_break_message(failures, &last_err) {
                                        tracing::warn!(
                                            tool = %name,
                                            failures,
                                            "Loop break: identical canonical call already failed {}× this turn — refusing to re-execute",
                                            failures
                                        );
                                        return (
                                            i,
                                            name.clone(),
                                            ToolExecutionResult {
                                                _name: name.clone(),
                                                arguments: arguments.clone(),
                                                result: Err(
                                                    crate::toolkit::error::ToolError::Execution(
                                                        msg,
                                                    ),
                                                ),
                                                operation_id: None,
                                            },
                                        );
                                    }
                                }
                            }
                            let mut exec_args = arguments.clone();
                            super::super::task_continuity::strip_new_operation_flag(
                                &mut exec_args,
                            );
                            let result = crate::toolkit::skill_tool::with_turn_loaded_skills(
                                turn_loaded_skills,
                                execute_tool_with_retry(
                                    &tools_clone,
                                    &cache_clone,
                                    &name,
                                    exec_args,
                                ),
                            )
                            .await;

                            // Strike bookkeeping on failure-circuit fingerprint:
                            // real failures accumulate; a success clears strikes.
                            {
                                let failed_now: Option<String> = match &result {
                                    Ok(out)
                                        if crate::toolkit::tool::effective_success(
                                            &real_name, out,
                                        ) =>
                                    {
                                        failed_map.lock().unwrap().remove(&failure_sig);
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
                                            .unwrap_or_else(|| {
                                                "tool reported failure".to_string()
                                            }),
                                    ),
                                    Err(e) => Some(e.to_string()),
                                };
                                if let Some(err) = failed_now {
                                    let mut m = failed_map.lock().unwrap();
                                    let entry = m
                                        .entry(failure_sig.clone())
                                        .or_insert((0, String::new()));
                                    entry.0 += 1;
                                    entry.1 = err;
                                }
                            }
                            if reusable {
                                if let Ok(out) = &result {
                                    if crate::toolkit::tool::effective_success(&real_name, out) {
                                        if let Ok(data_str) = serde_json::to_string(&out.data) {
                                            reuse_map
                                                .lock()
                                                .unwrap()
                                                .insert(canonical_sig, data_str);
                                        }
                                    }
                                }
                            }
                            (
                                i,
                                name.clone(),
                                ToolExecutionResult {
                                    _name: name.clone(),
                                    arguments: arguments.clone(),
                                    result,
                                    operation_id: tracked_operation_id,
                                },
                            )
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
                                let interrupted = safeguards
                                    .interrupt_signal
                                    .as_ref()
                                    .map(|rx| *rx.borrow())
                                    .unwrap_or(false);
                                if interrupted {
                                    tracing::info!("Stream interrupted during tool batch");
                                    {
                                        let mut state = internal_state.write().await;
                                        state.task_state.abandon_owned_operations_on_interrupt(
                                            &stream_owner,
                                            "user interrupt during tool batch",
                                        );
                                    }
                                    yield AgentEvent::content("\n\n[Interrupted]");
                                    {
                                        let mut state = internal_state.write().await;
                                        let reason = crate::agent::task_continuity::StreamEndReason::Interrupted;
                                        let goal = state.task_state.system_goal_status();
                state.stamp_final_goal_on_last_assistant(goal, reason);
                                        yield AgentEvent::final_end(&state.task_state, reason, None);
                                    }
                                    return;
                                }
                                yield AgentEvent::heartbeat();
                            }
                            done = &mut collect_fut => break done,
                        }
                    };
                    // Restore LLM-emission order (buffer_unordered completes in arbitrary order).
                    tool_results_executed.sort_by_key(|(i, _, _)| *i);

                    // Emit structured confirm lifecycle events (JSON fields —
                    // fingerprints may contain spaces; never text-parse).
                    {
                        let notices = {
                            let mut state = internal_state.write().await;
                            state.task_state.drain_confirm_lifecycle_notices()
                        };
                        for notice in notices {
                            match notice {
                                super::super::task_continuity::ConfirmLifecycleNotice::Requested(
                                    confirm,
                                ) => {
                                    yield AgentEvent::authorize_confirm_request(&confirm);
                                }
                                super::super::task_continuity::ConfirmLifecycleNotice::Removed {
                                    request_id,
                                    reason,
                                } => {
                                    yield AgentEvent::authorize_confirm_removed(request_id, reason);
                                }
                            }
                        }
                    }

                    // Process results
                    let mut tool_calls_with_results: Vec<ToolCall> = Vec::new();
                    let mut tool_call_results: Vec<(String, String)> = Vec::new();
                    // Compare actual outcomes before advisory/cache-reference
                    // decoration; those annotations are not new information.
                    let mut round_progress_results: Vec<(String, String)> = Vec::new();
                    let mut round_has_errors = false;
                    let mut round_other_progress = false;
                    let mut round_had_skill = false;
                    let mut round_had_help = false;
                    let mut round_had_successful_mutation = false;
                    let mut this_round_success_sigs: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    for (_, name, execution) in tool_results_executed {
                        // Use arguments from the execution result (preserves per-call arguments for same-name tools)
                        // NM-247: strip host PreparedInvoke carriage before
                        // model-visible tool-call persistence / history.
                        let mut exec_arguments = execution.arguments.clone();
                        super::super::task_continuity::strip_host_prepared_from_arguments(
                            &mut exec_arguments,
                        );
                        // Unified canonical fingerprint shared across read-only
                        // loop detection, duplicate advisory, and write ledger.
                        let real_name = crate::tools::resolve_tool_name(&name);
                        let canonical_sig = super::intent::canonical_tool_call_fingerprint(
                            &name,
                            &exec_arguments,
                        );
                        let is_new_call = seen_tool_sigs.insert(canonical_sig.clone());
                        yield AgentEvent::tool_call_start_round(&name, exec_arguments.clone(), tool_iteration_count + 1);

                        // Mutation-flavored SHELL commands that effectively
                        // SUCCEEDED this turn, as raw strings — feeds the
                        // turn-end completeness note.
                        if real_name == "shell" {
                            if let Some(cmd) =
                                exec_arguments.get("command").and_then(|v| v.as_str())
                            {
                                if super::guard::is_shell_help_command(cmd) {
                                    round_had_help = true;
                                    skill_exploration.observe_shell_command(cmd);
                                }
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
                        // Round 2: finalize Operation status + confirm steps.
                        // PreferKnown resume skips finalize (already terminal).
                        let resumed = matches!(
                            &execution.result,
                            Ok(out) if out.data.get("_operation_resume").and_then(|v| v.as_bool()) == Some(true)
                        );
                        {
                            let mut state = internal_state.write().await;
                            if let Some(op_id) = execution.operation_id.as_deref() {
                                let in_progress = matches!(
                                    &execution.result,
                                    Ok(out) if out.data.get("_operation_in_progress").and_then(|v| v.as_bool()) == Some(true)
                                );
                                if !resumed && !in_progress {
                                    let mut cache =
                                        std::mem::take(&mut state.large_data_cache);
                                    super::super::task_continuity::finalize_operation_from_tool_result(
                                        &mut state.task_state,
                                        &mut cache,
                                        op_id,
                                        &name,
                                        &execution.result,
                                    );
                                    state.large_data_cache = cache;
                                    // Persist terminal Succeeded/Failed/Unknown
                                    // after the side-effect tool settles.
                                    state.persist_op_ledger_terminal_best_effort();
                                }
                            }
                            if matches!(
                                &execution.result,
                                Ok(out) if crate::toolkit::tool::effective_success(
                                    &real_name, out
                                )
                            ) {
                                let result_body = match &execution.result {
                                    Ok(out) => out.data.to_string(),
                                    Err(e) => e.to_string(),
                                };
                                let result_fp =
                                    super::super::task_continuity::result_fingerprint(&result_body);
                                let target = super::super::task_continuity::extract_operation_target(
                                    &exec_arguments,
                                );
                                let summary =
                                    super::super::task_continuity::build_structured_summary(
                                        &real_name,
                                        "succeeded",
                                        &result_body,
                                        target.as_deref(),
                                        None,
                                    );
                                state.task_state.confirm_step_from_tool(
                                    &real_name,
                                    &canonical_sig,
                                    &result_fp,
                                    summary,
                                );
                                if let Ok(out) = &execution.result {
                                    let op_id = execution.operation_id.clone();
                                    state.task_state.observe_tool_result_for_goal(
                                        &real_name,
                                        &exec_arguments,
                                        &out.data,
                                        op_id.as_deref(),
                                    );
                                    // P2-2: if `neomind operation get <op_id>` confirmed a previously
                                    // `Unknown` operation reached a terminal state (`succeeded`/`failed`),
                                    // reconcile TaskState, persist the ledger, and sync `_new_operation`
                                    // protocol exposure below.
                                    if state.task_state.reconcile_operation_lookup_tool_result(
                                        &real_name,
                                        &exec_arguments,
                                        &out.data,
                                    ) {
                                        state.persist_op_ledger_terminal_best_effort();
                                    }
                                }
                            }
                        }
                        // Drain any confirm-removed notices emitted by operation reconciliation.
                        {
                            let recon_notices = internal_state
                                .write()
                                .await
                                .task_state
                                .drain_confirm_lifecycle_notices();
                            for notice in recon_notices {
                                match notice {
                                    super::super::task_continuity::ConfirmLifecycleNotice::Requested(
                                        confirm,
                                    ) => {
                                        yield AgentEvent::authorize_confirm_request(&confirm);
                                    }
                                    super::super::task_continuity::ConfirmLifecycleNotice::Removed {
                                        request_id,
                                        reason,
                                    } => {
                                        yield AgentEvent::authorize_confirm_removed(request_id, reason);
                                    }
                                }
                            }
                        }

                        // Duplicate-call advisory (read-only layer, canonical fingerprint):
                        // non-reusable tool, identical canonical signature, succeeded LAST
                        // round. Tag the result value; execution already happened.
                        let was_dup = !REUSABLE_TOOLS.contains(&real_name.as_str())
                            && last_round_success_sigs.contains(&canonical_sig);

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
                                &real_name, output,
                            )
                        );
                        round_had_skill |= real_name == "skill";
                        if succeeded {
                            if real_name == "skill" {
                                round_had_skill = true;
                                if let Ok(output) = &execution.result {
                                    skill_exploration.observe(&exec_arguments, &output.data);
                                }
                            } else {
                                let command = exec_arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                                // Re-reading volatile host status does not supply
                                // new vendor evidence; new telemetry/query results do.
                                let progress_key = if real_name == "shell" && super::dedup::command_similarity_key(command) == "system info" {
                                    "host_system_info".to_string()
                                } else {
                                    format!("{}|{}", canonical_sig, outcome)
                                };
                                round_other_progress |= seen_non_skill_outcomes.insert(progress_key);
                                round_had_successful_mutation |= (real_name == "shell" && super::intent::is_mutation_command(&command.to_lowercase()))
                                    || matches!(real_name.as_str(), "run_now" | "file_write" | "file_edit" | "image_edit");
                            }
                        }
                        round_has_errors |= !succeeded;
                        any_failed_tool_call |= !succeeded;
                        let image_failure = image_failure_progress(&name, &exec_arguments, &execution.result);
                        if is_new_call && image_failure.is_none() {
                            round_new_calls += 1;
                        }
                        round_progress_results.push(image_failure.unwrap_or((canonical_sig.clone(), outcome)));

                        match execution.result {
                            Ok(output) => {
                                if crate::toolkit::tool::effective_success(&real_name, &output) {
                                    this_round_success_sigs.insert(canonical_sig.clone());
                                }
                                let mut result_value = output.prompt_value();
                                if was_dup {
                                    let advisory = serde_json::json!(
                                        "you made this EXACT call (same tool + arguments) last round and it already succeeded — if you are verifying a change, state what changed; otherwise do NOT repeat it again / 你上一轮已做过完全相同的调用且已成功；若在验证改动请说明差异，否则不要重复"
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
                        if !guards_disabled
                            && super::guard::should_inject_promise_directive(
                                promise_rounds,
                                promise_directive_sent,
                            )
                        {
                            if let Some(cmd) = promised_cmd.clone() {
                                promise_directive_sent = true;
                                guard_ledger.record(
                                    super::guard::GuardKind::PromiseDirective,
                                    super::guard::GuardOutcome::Fired,
                                    tool_iteration_count + 1,
                                    "repeated_promise",
                                );
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
                    // Loop signal: the SAME calls returning BYTE-IDENTICAL
                    // results across rounds. A verification poll whose result
                    // changed (online→offline, a new metric) or any new call
                    // resets the counter — exploration and verify-after-write
                    // are not loops. Only repeated no-op rounds are.
                    let identical_rounds = loop_progress.observe(
                        round_new_calls, &round_progress_results,
                    );
                    // Round 3: status-poll with changing evidence gets a timed
                    // allow; identical no-progress still halts (≠ success).
                    let call_fps: Vec<String> = round_progress_results
                        .iter()
                        .map(|(sig, _)| sig.clone())
                        .collect();
                    let results_changed = identical_rounds == 0;
                    if results_changed
                        && call_fps
                            .iter()
                            .any(|fp| crate::agent::task_continuity::looks_like_status_poll(fp))
                    {
                        status_poll_progress_rounds =
                            status_poll_progress_rounds.saturating_add(1);
                    }
                    // Only the *current* Operation if it is Unknown — never
                    // treat an older historical Unknown as a permanent halt.
                    let write_unknown_op = {
                        let state = internal_state.read().await;
                        let cur = state.task_state.current_operation_id.clone();
                        state
                            .task_state
                            .operation(&cur)
                            .filter(|op| {
                                op.status
                                    == crate::agent::task_continuity::OperationStatus::Unknown
                            })
                            .map(|op| op.operation_id.clone())
                    };
                    let progress_decision =
                        crate::agent::task_continuity::progress_loop_decision(
                            identical_rounds,
                            CHAT_IDENTICAL_ROUNDS_LIMIT,
                            &call_fps,
                            results_changed,
                            status_poll_progress_rounds,
                            write_unknown_op.as_deref(),
                        );
                    let chat_repeating = matches!(
                        progress_decision,
                        crate::agent::task_continuity::ProgressDecision::HaltNoProgress { .. }
                            | crate::agent::task_continuity::ProgressDecision::InquireUnknown { .. }
                    );
                    if chat_repeating {
                        // G3 / Round-3 stop: write halt reason — never leave Active.
                        {
                            let mut state = internal_state.write().await;
                            let stopped = crate::agent::task_continuity::apply_progress_stop(
                                &mut state.task_state,
                                &progress_decision,
                            );
                            debug_assert!(stopped);
                            debug_assert!(matches!(
                                state.task_state.status,
                                crate::agent::task_continuity::TaskStatus::Halted { .. }
                            ));
                        }
                        tracing::warn!(
                            rounds = tool_iteration_count + 1,
                            identical_rounds,
                            new_calls = round_new_calls,
                            ?progress_decision,
                            counts_as_task_success = false,
                            "Chat tool loop / unknown-write stop — forcing final summary (halt ≠ success)"
                        );
                        yield AgentEvent::progress(
                            "Repeated the same calls with no new information — generating final response..."
                                .to_string(),
                            "summarizing",
                            0,
                        );
                    }
                    // Read-only streak force-end: the act-now directive was
                    // injected and the model STILL produced another all-
                    // read-only round (production 2026-11-02: 30-round
                    // list/skill cycles — advisory text does not stop a
                    // looping small model). End the turn instead of granting
                    // another round; the forced-summary path below produces
                    // the text reply, with the honesty directive attached.
                    let read_only_loop_terminated = behavioral_guards
                        && !guards_disabled
                        && explore_recovery_nudged
                        && post_dead_end_read_only_rounds >= 3;
                    if read_only_loop_terminated {
                        guard_ledger.record(
                            super::guard::GuardKind::ReadOnlyStreak,
                            super::guard::GuardOutcome::Fired,
                            tool_iteration_count + 1,
                            "directive_ignored_force_end",
                        );
                        tracing::warn!(
                            consecutive_read_only = post_dead_end_read_only_rounds,
                            "Read-only loop ignored the act-now directive — forcing turn end"
                        );
                        yield AgentEvent::progress(
                            "No new information after repeated read-only calls — generating final response..."
                                .to_string(),
                            "summarizing",
                            0,
                        );
                    }
                    // Sync dynamic `_new_operation` schema exposure after this
                    // round's operations settled in TaskState (only tools with a
                    // terminal Succeeded/Failed/Unknown write expose it).
                    {
                        let new_op_tools = internal_state
                            .read()
                            .await
                            .task_state
                            .tools_needing_new_operation_protocol();
                        llm_interface
                            .set_new_operation_protocol_tools(new_op_tools)
                            .await;
                    }

                    // Tools in a batch execute concurrently. Decide progress only
                    // after ALL results, independent of their emission order.
                    skill_exploration.observe_round_end(
                        round_had_skill,
                        round_had_help,
                        round_had_successful_mutation,
                    );
                    if round_had_successful_mutation {
                        skill_exploration = super::guard::SkillExplorationBudget::default();
                    }
                    let guide_stalled = round_had_skill && !round_other_progress && skill_exploration.exhausted();
                    let try_guide_recovery = guide_stalled && !guide_recovery_used
                        && !wall_clock_exhausted && !chat_repeating
                        && tool_iteration_count < MAX_TOOL_ITERATIONS - 1;
                    if try_guide_recovery {
                        guide_recovery_used = true;
                        guide_recovery_pending = true;
                        llm_interface.clear_skill_context().await;
                        guard_ledger.record(super::guard::GuardKind::SkillRecovery,
                            super::guard::GuardOutcome::Fired, tool_iteration_count + 1,
                            "retry_without_repeated_guide_tool");
                    }
                    let skill_exploration_exhausted = guide_stalled && !try_guide_recovery;
                    if skill_exploration_exhausted {
                        guard_ledger.record(
                            super::guard::GuardKind::SkillExploration,
                            super::guard::GuardOutcome::Fired,
                            tool_iteration_count + 1,
                            "workflow_guide_budget_exhausted",
                        );
                        yield AgentEvent::progress(
                            "Workflow-guide exploration made no further progress — generating final response...".to_string(),
                            "summarizing", 0,
                        );
                    }
                    let force_summary = wall_clock_exhausted || chat_repeating
                        || read_only_loop_terminated || skill_exploration_exhausted
                        || tool_iteration_count >= MAX_TOOL_ITERATIONS - 1;
                    let should_continue = !force_summary;
                    // Pre-exhaustion action-exit hint: if the user asked for a
                    // concrete mutation and the matching domain guide is already
                    // in context, emit one lightweight reminder before the
                    // exploration budget exhausts (helps break `skill ↔ --help`
                    // stalls without modifying the system prompt).
                    if should_continue && !try_guide_recovery {
                        if let Some(hint) = skill_exploration.action_exit_hint(
                            &user_message,
                            &successful_mutations,
                            no_execution_requested,
                        ) {
                            guard_ledger.record(
                                super::guard::GuardKind::SkillActionExitHint,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "guide_loaded_exit_to_action",
                            );
                            skill_action_exit_hint_pending = Some(hint);
                        }
                    }
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

                    // Save ALL tool results (including `skill`) as `tool` messages in
                    // conversation history. Mid-turn `skill(load)` no longer mutates
                    // the system prompt, keeping the KV-prefix cache byte-identical
                    // across ReAct rounds within a turn.
                    {
                        let mut state = internal_state.write().await;
                        for (tool_name, result_str) in &tool_call_results {
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

                    if force_summary || content_before_tools.is_empty() || (last_round_has_errors && content_is_preamble) {

                        // Notify user that we're generating a final response
                        if last_round_has_errors {
                            yield AgentEvent::progress(
                                "Generating final response...".to_string(),
                                "summarizing",
                                0,
                            );
                        }

                        let mut final_content = String::new();
                        // Build compact history for the summary call
                        let summary_history: Vec<neomind_core::Message> = if skill_exploration_exhausted {
                            llm_interface.clear_skill_context().await;
                            Vec::new()
                        } else {
                            let state_guard = internal_state.read().await;
                            let (compacted, _) = turn_context.build_for_llm(&state_guard.memory, effective_max, &llm_interface);
                            compacted.iter().map(|msg| msg.to_core()).collect()
                        };

                        // Detect whether tool results contain errors to tailor the prompt
                        let has_errors = last_round_has_errors;

                        let summary_base = if skill_exploration_exhausted {
                            super::guard::guide_exploration_brief()
                        } else if force_summary {
                            super::guard::forced_summary_prompt()
                        } else if has_errors {
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
                        let summary_fact_block = if skill_exploration_exhausted {
                            None
                        } else {
                            internal_state
                                .read()
                                .await
                                .task_state
                                .build_summary_goal_status_fact_block(
                                    current_turn_id,
                                    any_failed_tool_call || has_errors,
                                )
                        };
                        let summary_prompt = match summary_fact_block {
                            Some(block) => format!("{summary_base}\n\n{block}"),
                            None => summary_base.to_string(),
                        };

                        let summary_result = llm_interface.chat_stream_summary_for_task(
                            &user_message,
                            &summary_prompt,
                            &summary_history,
                        ).await;


                        match summary_result {
                            Ok(stream) => {
                                let mut pin = Box::pin(stream);
                                while let Some(chunk) =
                                    next_chunk_or_timeout(&mut pin, safeguards.max_stream_duration).await
                                {
                                    match chunk {
                                        Ok((text, is_thinking)) => {
                                            if !is_thinking {
                                                final_content.push_str(&text);
                                                if !skill_exploration_exhausted {
                                                    yield AgentEvent::content(text);
                                                }
                                            }
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

                        if skill_exploration_exhausted {
                            // Validate before display; the fallback is a last resort,
                            // not the normal wording of every guarded turn.
                            if !super::guard::usable_exploration_reply(&final_content) {
                                final_content = super::guard::guide_exploration_reply(&user_message).to_string();
                            }
                            yield AgentEvent::content(final_content.clone());
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
                    // Nudge effectiveness: the retried round STILL did not act.
                    if round_is_nudge {
                        guard_ledger.record(
                            super::guard::GuardKind::NarrationNudgeOutcome,
                            super::guard::GuardOutcome::Skipped,
                            tool_iteration_count + 1,
                            "no_tool_call_in_nudge_round",
                        );
                    }
                    let mut raw_response = buffer.clone();

                    // === NARRATION-COLLAPSE / FORESHADOW CONTINUE ===
                    // The model promised an action in words but emitted no tool
                    // call. Intercept BEFORE the final-save path: record what it
                    // said, continue once, and run another round.
                    //
                    // Two gates share the same recovery round:
                    //   1. opt-in `behavioral_guards` narration nudge (full suite)
                    //   2. NM-250 foreshadow continue — default chat policy
                    //      (`behavioral_guards` off): this round ended
                    //      foreshadow-only → exactly one bounded continue,
                    //      even if earlier rounds already searched/listed.
                    //      Does NOT enable list-only coercion / explore recovery.
                    let is_narration_reply =
                        crate::agent::is_narration_without_action(&raw_response);
                    let iteration_budget_left =
                        tool_iteration_count < MAX_TOOL_ITERATIONS - 1;
                    let wall_clock_left =
                        turn_started_at.elapsed() < turn_wall_clock_budget;
                    let behavioral_nudge_allowed = behavioral_guards
                        && !guards_disabled
                        && super::guard::should_nudge_narration(
                            is_narration_reply,
                            narration_nudges,
                            NARRATION_NUDGE_MAX,
                            no_execution_requested,
                            iteration_budget_left,
                            wall_clock_left,
                        );
                    let foreshadow_continue_allowed = !behavioral_guards
                        && !guards_disabled
                        && super::guard::should_foreshadow_continue(
                            is_narration_reply,
                            foreshadow_continues < FORESHADOW_CONTINUE_MAX,
                            no_execution_requested,
                            greeting_input,
                            iteration_budget_left,
                            wall_clock_left,
                        );
                    let nudge_allowed =
                        behavioral_nudge_allowed || foreshadow_continue_allowed;

                    // Agent status → latest-execution: model answered without
                    // reading a conclusion (zero tools, or ignored mid-turn
                    // redirect). One bounded continue under default policy.
                    let cmds_for_redirect: Vec<&str> =
                        recently_executed_commands.iter().map(|s| s.as_str()).collect();
                    let redirect_prompt = if guards_disabled {
                        None
                    } else {
                        build_agent_conclusion_redirect(&user_message, &cmds_for_redirect)
                    };
                    let conclusion_redirect_allowed =
                        super::guard::should_redirect_agent_conclusion(
                            agent_conclusion_redirect_sent,
                            no_execution_requested,
                            iteration_budget_left,
                            wall_clock_left,
                            redirect_prompt.is_some(),
                        );
                    if conclusion_redirect_allowed {
                        if let Some(redirect) = redirect_prompt {
                            agent_conclusion_redirect_sent = true;
                            guard_ledger.record(
                                super::guard::GuardKind::AgentConclusionRedirect,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "status_answer_without_conclusion",
                            );
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                chars = raw_response.chars().count(),
                                "Agent status answer without conclusion — bounded redirect once"
                            );
                            yield AgentEvent::progress(
                                "Retrying — agent conclusion not read yet...".to_string(),
                                "executing",
                                0,
                            );
                            let said = remove_tool_calls_from_response(&raw_response);
                            nudge_saved_narration = Some(said.clone());
                            internal_state
                                .write()
                                .await
                                .push_message(AgentMessage::assistant(&said));
                            tool_iteration_count += 1;
                            agent_conclusion_redirect_pending = Some(redirect);
                            yield AgentEvent::IntermediateEnd;
                            continue 'multi_round_loop;
                        }
                    }

                    let onboarding_prompt = if guards_disabled {
                        None
                    } else {
                        build_device_onboarding_redirect(&user_message, &cmds_for_redirect)
                    };
                    let onboarding_redirect_allowed =
                        super::guard::should_redirect_device_onboarding(
                            device_onboarding_redirect_sent,
                            no_execution_requested,
                            iteration_budget_left,
                            wall_clock_left,
                            onboarding_prompt.is_some(),
                        );
                    if onboarding_redirect_allowed {
                        if let Some(redirect) = onboarding_prompt {
                            device_onboarding_redirect_sent = true;
                            guard_ledger.record(
                                super::guard::GuardKind::DeviceOnboardingRedirect,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "onboarding_answer_without_create",
                            );
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                chars = raw_response.chars().count(),
                                "Guided onboarding answer without create — bounded redirect once"
                            );
                            yield AgentEvent::progress(
                                "Retrying — device not created yet...".to_string(),
                                "executing",
                                0,
                            );
                            let said = remove_tool_calls_from_response(&raw_response);
                            nudge_saved_narration = Some(said.clone());
                            internal_state
                                .write()
                                .await
                                .push_message(AgentMessage::assistant(&said));
                            tool_iteration_count += 1;
                            device_onboarding_redirect_pending = Some(redirect);
                            yield AgentEvent::IntermediateEnd;
                            continue 'multi_round_loop;
                        }
                    }

                    if !nudge_allowed && is_narration_reply {
                        let (kind, outcome, reason) = if guards_disabled {
                            (
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Suppressed,
                                "guard_mode_off",
                            )
                        } else if no_execution_requested {
                            (
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Suppressed,
                                "plan_only",
                            )
                        } else if !behavioral_guards {
                            let reason = if foreshadow_continues >= FORESHADOW_CONTINUE_MAX {
                                "budget_exhausted"
                            } else if greeting_input {
                                "greeting_input"
                            } else if !iteration_budget_left {
                                "iteration_cap"
                            } else {
                                "wall_clock"
                            };
                            (
                                super::guard::GuardKind::ForeshadowContinue,
                                super::guard::GuardOutcome::Skipped,
                                reason,
                            )
                        } else if narration_nudges >= NARRATION_NUDGE_MAX {
                            (
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Skipped,
                                "budget_exhausted",
                            )
                        } else if !iteration_budget_left {
                            (
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Skipped,
                                "iteration_cap",
                            )
                        } else {
                            (
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Skipped,
                                "wall_clock",
                            )
                        };
                        guard_ledger.record(kind, outcome, tool_iteration_count + 1, reason);
                    }
                    if nudge_allowed {
                        if foreshadow_continue_allowed {
                            guard_ledger.record(
                                super::guard::GuardKind::ForeshadowContinue,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "foreshadow_without_tool",
                            );
                            foreshadow_continues += 1;
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                chars = raw_response.chars().count(),
                                "Foreshadow without tool — bounded continue once"
                            );
                        } else {
                            guard_ledger.record(
                                super::guard::GuardKind::NarrationNudge,
                                super::guard::GuardOutcome::Fired,
                                tool_iteration_count + 1,
                                "promise_without_action",
                            );
                            narration_nudges += 1;
                            tracing::info!(
                                round = tool_iteration_count + 1,
                                chars = raw_response.chars().count(),
                                "Narration without tool call — nudging once"
                            );
                        }
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
                                guard_ledger.record(
                                    super::guard::GuardKind::DegenerateRestore,
                                    super::guard::GuardOutcome::Fired,
                                    tool_iteration_count + 1,
                                    "nudge_round_degenerate",
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
                    //
                    // Suppressed when the user asked for a plan only: that
                    // promise-shaped reply is the requested output, not a
                    // failed turn.
                    let label_narration = crate::agent::is_narration_without_action(&raw_response);
                    let label_allowed = !guards_disabled
                        && super::guard::should_label_narration(
                            label_narration,
                            no_execution_requested,
                            greeting_input,
                        );
                    if label_narration {
                        guard_ledger.record(
                            super::guard::GuardKind::NarrationLabel,
                            if label_allowed {
                                super::guard::GuardOutcome::Fired
                            } else {
                                super::guard::GuardOutcome::Suppressed
                            },
                            tool_iteration_count + 1,
                            if label_allowed {
                                "turn_ends_on_promise"
                            } else if guards_disabled {
                                "guard_mode_off"
                            } else if no_execution_requested {
                                "plan_only"
                            } else {
                                "greeting_input"
                            },
                        );
                    }
                    if label_allowed {
                        // NM-250: after the foreshadow continue already spent
                        // its one shot (or default chat ends foreshadow-only
                        // without asking the user to type 继续), prefer an
                        // honest "no result" note. Opt-in behavioural path
                        // keeps the legacy "Send continue" progress report.
                        let used_foreshadow_continue = foreshadow_continues > 0;
                        let default_chat_foreshadow_end =
                            !behavioral_guards && is_narration_reply;
                        let note = if used_foreshadow_continue || default_chat_foreshadow_end
                        {
                            tracing::warn!(
                                round = tool_iteration_count + 1,
                                foreshadow_continues,
                                "Turn ending on foreshadow without a result — appending no-result note"
                            );
                            super::guard::foreshadow_no_result_note(&user_message)
                        } else {
                            tracing::warn!(
                                round = tool_iteration_count + 1,
                                nudges = narration_nudges,
                                "Turn ending on an action promise with guard budget spent — appending progress note"
                            );
                            // English only (decision 2026-10-10) — one fixed
                            // language, locale-independent.
                            "The turn ended here (guard budget spent): the reply above is a progress report, not a completed result — the promised next step did NOT run. Send \"continue\" to resume from this point.".to_string()
                        };
                        yield AgentEvent::system_note(note.clone());
                        turn_system_notes.push(note);
                    } else if let Some(note) = {
                        if guards_disabled {
                            None
                        } else {
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
                        }
                    } {
                        guard_ledger.record(
                            super::guard::GuardKind::IncompleteActionNote,
                            super::guard::GuardOutcome::Fired,
                            tool_iteration_count + 1,
                            "requested_action_not_delivered",
                        );
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
                            let (compacted, _) = turn_context.build_for_llm(&state_guard.memory, effective_max, &llm_interface);
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
                                        Ok((text, is_thinking)) => {
                                            if !is_thinking {
                                                summary_content.push_str(&text);
                                                yield AgentEvent::content(text);
                                            }
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

                        // Build a compact history for the tool-free summary retry
                        // (keep last few messages so the prompt stays small).
                        let state_guard = internal_state.read().await;
                        let retry_history: Vec<neomind_core::Message> = {
                            let (context, _) = turn_context.build_for_llm(&state_guard.memory, effective_max, &llm_interface);
                            let non_system: Vec<&AgentMessage> = context.iter().collect();
                            // Keep at most last 6 messages to reduce prompt size
                            let keep = non_system.len().saturating_sub(6);
                            context.iter().filter(|m| m.role == "system").chain(non_system[keep..].iter().copied().filter(|m| m.role != "system")).map(|m| m.to_core()).collect()
                        };
                        drop(state_guard);

                        // Call the tool-free summary path (`include_tools = false`,
                        // `thinking_enabled = Some(false)`) so the model must emit
                        // a direct text answer instead of emitting tool-call JSON
                        // that would be stripped into an empty error.
                        let retry_base = if !all_round_tool_results.is_empty() {
                            "Based on the tool execution results already in the conversation above, \
                             provide a direct and concise final answer to the user's request. \
                             Do NOT output any tool calls or JSON arrays — respond in plain text only."
                        } else {
                            "Please respond to the user's message directly and concisely in plain text. \
                             Do NOT output any tool calls or JSON arrays."
                        };
                        let retry_fact_block = internal_state
                            .read()
                            .await
                            .task_state
                            .build_summary_goal_status_fact_block(
                                current_turn_id,
                                any_failed_tool_call,
                            );
                        let retry_prompt = match retry_fact_block {
                            Some(block) => format!("{retry_base}\n\n{block}"),
                            None => retry_base.to_string(),
                        };

                        let retry_result = llm_interface.chat_stream_summary_for_task(
                            &user_message,
                            &retry_prompt,
                            &retry_history,
                        ).await;

                        match retry_result {
                            Ok(retry_stream) => {
                                let mut retry_content = String::new();
                                let mut pin = Box::pin(retry_stream);
                                // Dedicated bounds for the recovery retry. It
                                // must NOT inherit the main path's 2400s
                                // per-chunk idle: that timeout only fires on a
                                // SILENT stream, and there is no total cap at
                                // all — a wedged backend held the turn
                                // indefinitely while the user watched nothing
                                // (live 2026-10-27: retry sent 15:38:44, no
                                // reply ever rendered). This call has thinking
                                // forced off and a compact history, so 120s
                                // idle (still covers the single-slot llama.cpp
                                // queue+prefill first-chunk wait, ~62s
                                // observed) and a 180s total budget are ample.
                                const RETRY_IDLE_TIMEOUT: std::time::Duration =
                                    std::time::Duration::from_secs(120);
                                const RETRY_TOTAL_BUDGET: std::time::Duration =
                                    std::time::Duration::from_secs(180);
                                let retry_started = std::time::Instant::now();
                                while let Some(chunk) =
                                    next_chunk_or_timeout(&mut pin, RETRY_IDLE_TIMEOUT).await
                                {
                                    if retry_started.elapsed() >= RETRY_TOTAL_BUDGET {
                                        tracing::warn!(
                                            chars_so_far =
                                                retry_content.chars().count(),
                                            "No-thinking retry exceeded its total budget — using whatever arrived"
                                        );
                                        break;
                                    }
                                    match chunk {
                                        Ok((text, is_thinking)) => {
                                            if !is_thinking {
                                                retry_content.push_str(&text);
                                            }
                                            // Don't yield yet — check for stray tool JSON first.
                                        }
                                        Err(e) => {
                                            tracing::error!("Retry stream error: {}", e);
                                            break;
                                        }
                                    }
                                }

                                // Defensive strip in case a model emits literal tool JSON in text
                                // despite tool definitions being omitted.
                                let cleaned = match parse_tool_calls(&retry_content) {
                                    Ok((content, calls)) if !calls.is_empty() => {
                                        tracing::warn!(
                                            calls_count = calls.len(),
                                            "Tool-free summary retry still emitted tool JSON, stripping it"
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
                                        "Tool-free no-thinking recovery succeeded"
                                    );
                                } else if !all_round_tool_results.is_empty() {
                                    let deduped = deduplicate_tool_results(&all_round_tool_results);
                                    let formatted = format_tool_results(&deduped);
                                    tracing::warn!(
                                        formatted_len = formatted.len(),
                                        "Tool-free recovery produced empty content; falling back to formatted tool results"
                                    );
                                    raw_response = formatted.clone();
                                    yield AgentEvent::content(formatted);
                                } else {
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
                                let fallback = if !all_round_tool_results.is_empty() {
                                    let deduped = deduplicate_tool_results(&all_round_tool_results);
                                    format_tool_results(&deduped)
                                } else {
                                    format!(
                                        "Sorry, the model could not produce a response. Please retry.\n[error: {e}]"
                                    )
                                };
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

            // One structured line summarizing every guard decision this turn
            // (silent when no guard had anything to say).
            guard_ledger.emit_summary();

            // Read token usage from LLM interface (captured from Ollama backend stream)
            let prompt_tokens = llm_interface.take_last_prompt_tokens().await;
            let usage = match prompt_tokens {
                Some(pt) => {
                    // Calibration may already have consumed the paired raw
                    // estimate at ContextUsage time — observe is a no-op then.
                    llm_interface.observe_prompt_calibration(pt).await;
                    let (system_tokens, tool_tokens) = llm_interface
                        .calibrated_context_breakdown(pt, has_user_images)
                        .await;
                    Some((pt, system_tokens, tool_tokens))
                }
                None => None,
            };
            // Unified End: system goal status + reason (never bare end()).
            // Stamp the same snapshot onto the turn's last assistant message
            // so history reload returns stored values as-is (no TaskState recompute).
            let end = {
                let mut state = internal_state.write().await;
                let reason = match &state.task_state.status {
                    crate::agent::task_continuity::TaskStatus::Halted { .. } => {
                        crate::agent::task_continuity::StreamEndReason::CannotContinue
                    }
                    crate::agent::task_continuity::TaskStatus::Cancelled => {
                        crate::agent::task_continuity::StreamEndReason::Cancelled
                    }
                    _ => crate::agent::task_continuity::StreamEndReason::Normal,
                };
                let goal = state.task_state.system_goal_status();
                state.stamp_final_goal_on_last_assistant(goal, reason);
                AgentEvent::final_end(&state.task_state, reason, usage)
            };
            yield end;
        })
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

/// A new filename is not progress when the image operation still fails.
/// Keep the detailed error in history; only the loop fingerprint is normalized.
fn image_failure_progress(
    name: &str,
    arguments: &serde_json::Value,
    result: &crate::toolkit::error::Result<crate::toolkit::ToolOutput>,
) -> Option<(String, String)> {
    let real_name = crate::tools::resolve_tool_name(name);
    let image_call = matches!(real_name.as_str(), "vision" | "image_edit")
        || ["image", "image_base64", "base64_data", "image_data", "img"]
            .iter()
            .any(|key| arguments.get(key).is_some());
    if !image_call {
        return None;
    }
    let failure_kind = match result {
        Ok(output) if crate::toolkit::tool::effective_success(&real_name, output) => return None,
        Ok(_) => "tool_result_error".to_string(),
        Err(error) => format!("{:?}", std::mem::discriminant(error)),
    };
    Some((format!("image_failure:{real_name}"), failure_kind))
}

/// Fingerprint of one round's tool results as an ORDER-INDEPENDENT SET:
/// pairs are sorted before hashing, and the loop breaker compares each
/// round's set against a small ring of recent rounds. Rationale
/// (production traces 2026-11-02, T4/1.2.5): a looping small model does
/// not replay one byte-identical sequence — it re-cycles COMBINATIONS of
/// the same read-only calls (R5 {list,info,types} → R6 {info,list} →
/// R7 {list,skill,info}…), so an ordered-sequence hash never repeats and
/// the old breaker never fired. A set hash matched against a ring catches
/// the cycle; identical calls with a CHANGED result still hash differently
/// and count as progress (verification/polling).
fn tool_round_fingerprint(results: &[(String, String)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut sorted: Vec<&(String, String)> = results.iter().collect();
    sorted.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, result) in sorted {
        name.hash(&mut hasher);
        result.hash(&mut hasher);
    }
    hasher.finish()
}

/// How many recent round set-fingerprints the breaker remembers. Looping
/// models cycle through a handful of combinations (observed ~4), so a ring
/// of 6 covers the cycle without matching one-off re-verifications minutes
/// apart (the ring only holds the LAST 6 rounds).
const LOOP_FINGERPRINT_RING: usize = 6;

#[derive(Default)]
struct ChatLoopProgress {
    ring: std::collections::VecDeque<u64>,
    repeated_set_rounds: usize,
}

impl ChatLoopProgress {
    /// Returns the consecutive count of "zero new calls AND this round's
    /// result-set already appeared within the recent ring" — i.e. pure
    /// re-exploration of already-seen state.
    fn observe(&mut self, new_calls: usize, results: &[(String, String)]) -> usize {
        let fingerprint = tool_round_fingerprint(results);
        let seen_before = new_calls == 0 && self.ring.contains(&fingerprint);
        self.repeated_set_rounds = if seen_before {
            self.repeated_set_rounds + 1
        } else {
            0
        };
        self.ring.push_back(fingerprint);
        if self.ring.len() > LOOP_FINGERPRINT_RING {
            self.ring.pop_front();
        }
        self.repeated_set_rounds
    }
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

    #[test]
    fn repeated_poll_state_flip_counts_toward_loop_within_ring() {
        // Ring semantics (2026-11-02): a poll whose state flips BACK to a
        // recently-seen value still counts toward the loop budget — an
        // endless ping-pong poll is also a loop the model must exit by
        // acting or answering. A FIRST change still resets (see below).
        let mut progress = ChatLoopProgress::default();
        let pending = vec![("shell|status".into(), "pending".into())];
        let ready = vec![("shell|status".into(), "ready".into())];
        assert_eq!(progress.observe(1, &pending), 0);
        assert_eq!(progress.observe(0, &pending), 1);
        assert_eq!(progress.observe(0, &ready), 0); // first change = progress
        assert_eq!(progress.observe(0, &ready), 1);
        assert_eq!(progress.observe(0, &pending), 2); // flip-back: cycling
    }

    #[test]
    fn combination_cycles_of_same_results_are_loops() {
        // The production pattern (T4 2026-11-02, 30 rounds): the model
        // re-cycled COMBINATIONS of the same read-only calls — never one
        // byte-identical sequence, so the old consecutive-hash breaker
        // never fired. Set-hashing + the ring must catch the cycle.
        let mut progress = ChatLoopProgress::default();
        let abc = vec![
            ("shell|sysinfo".into(), "info".into()),
            ("shell|types".into(), "types".into()),
            ("shell|list".into(), "devices".into()),
        ];
        let ab = vec![
            ("shell|list".into(), "devices".into()),
            ("shell|sysinfo".into(), "info".into()),
        ];
        let abl = vec![
            ("shell|list".into(), "devices".into()),
            ("skill|load".into(), "guide".into()),
            ("shell|sysinfo".into(), "info".into()),
        ];
        assert_eq!(progress.observe(1, &abc), 0);
        assert_eq!(progress.observe(0, &ab), 0); // new combo, unseen set
        assert_eq!(progress.observe(0, &abl), 0);
        assert_eq!(progress.observe(0, &ab), 1); // ab seen within the ring
        assert_eq!(progress.observe(0, &abc), 2); // abc cycles back
        assert_eq!(progress.observe(0, &abl), 3); // abl cycles back → break
    }

    #[test]
    fn ring_forgets_old_rounds() {
        // Outside the ring window an old fingerprint no longer counts —
        // long-running verification work isn't punished for history.
        let mut progress = ChatLoopProgress::default();
        let a = vec![("shell|x".into(), "1".into())];
        let mut expected = 0;
        for i in 0..(LOOP_FINGERPRINT_RING + 2) {
            let v = vec![("shell|filler".into(), format!("f{i}"))];
            expected = progress.observe(1, &v);
        }
        assert_eq!(expected, 0);
        assert_eq!(progress.observe(0, &a), 0); // a fell out of the ring
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

    /// Loop-break state machine: one failure never breaks; the refusal
    /// carries the previous error and the shell/CLI redirect.
    #[test]
    fn rotating_image_tools_with_repeated_errors_eventually_stop() {
        let mut guard = ChatLoopProgress::default();
        for i in 0..6 {
            let name = ["vision", "ocr", "detect"][i % 3];
            let args = serde_json::json!({"image_base64":format!("/tmp/retry-{i}.jpg")});
            let result = Err(crate::toolkit::ToolError::InvalidArguments(
                "missing image".into(),
            ));
            let entry = image_failure_progress(name, &args, &result).unwrap();
            let repeats = guard.observe(0, &[entry]);
            assert_eq!(repeats, if i < 3 { 0 } else { i - 2 });
        }
    }

    #[test]
    fn repeated_image_failures_ignore_path_churn_but_success_recovers() {
        use crate::toolkit::{ToolError, ToolOutput};
        let failure = Err(ToolError::PermissionDenied("system path".into()));
        let first = image_failure_progress(
            "vision",
            &serde_json::json!({"image":"/tmp/one.jpg"}),
            &failure,
        )
        .expect("image failure must have a stable progress key");
        let second = image_failure_progress(
            "vision",
            &serde_json::json!({"image":"/tmp/two.jpg"}),
            &failure,
        )
        .unwrap();
        assert_eq!(first, second);
        let mut guard = ChatLoopProgress::default();
        assert_eq!(guard.observe(0, std::slice::from_ref(&first)), 0);
        assert_eq!(guard.observe(0, std::slice::from_ref(&second)), 1);
        assert_eq!(guard.observe(0, std::slice::from_ref(&first)), 2);
        assert_eq!(guard.observe(0, std::slice::from_ref(&second)), 3);
        let ok = Ok(ToolOutput::success(
            serde_json::json!({"analysis":"visible scene"}),
        ));
        assert!(image_failure_progress(
            "vision",
            &serde_json::json!({"image":"/api/images/device/image/good.jpg"}),
            &ok
        )
        .is_none());
        assert_eq!(
            guard.observe(1, &[("vision".into(), "visible scene".into())]),
            0
        );
        assert!(
            image_failure_progress("shell", &serde_json::json!({"command":"probe"}), &failure)
                .is_none()
        );
        let corrected_error = Err(ToolError::InvalidArguments("invalid format".into()));
        assert_ne!(
            image_failure_progress("vision", &serde_json::json!({}), &corrected_error).unwrap(),
            first
        );
    }

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
