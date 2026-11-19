//! LLM interface for the Agent.
//!
//! This module provides a simple LLM wrapper with concurrency limits
//! and integration with the LlmBackendInstanceManager for dynamic backend switching.

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::Stream;
use tokio::sync::RwLock;

use neomind_core::{
    config::agent_env_vars,
    llm::backend::{LlmError, LlmInput, LlmRuntime},
    Message,
};

use crate::agent::tokenizer::estimate_tokens;

// Import intent classifier for staged processing
use crate::agent::staged::IntentClassifier;
// Import the unified error type
use crate::error::NeoMindError;
// Import the Result type alias
use crate::error::Result as AgentResult;

/// Re-export the instance manager types for convenience
pub use crate::llm_backends::{
    get_instance_manager, BackendTypeDefinition, LlmBackendInstanceManager,
};

/// Default concurrent LLM request limit.
/// Note: This constant is kept for backward compatibility but the actual default
/// is loaded from environment variable AGENT_CONCURRENT_LIMIT with fallback to 3.
pub const DEFAULT_CONCURRENT_LIMIT: usize = 3;

/// Simple atomic-based concurrency limiter.
///
/// This is simpler than using a semaphore for streams because it doesn't
/// have lifetime issues with permits.
#[derive(Clone)]
struct ConcurrencyLimiter {
    current: Arc<AtomicUsize>,
    max: Arc<AtomicUsize>, // Make max AtomciUsize for dynamic adjustment
    base_max: usize,       // Original configured max
}

impl ConcurrencyLimiter {
    fn new(max: usize) -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            max: Arc::new(AtomicUsize::new(max)),
            base_max: max,
        }
    }

    /// Try to acquire a permit. Returns Some(permit) if successful, None if at limit.
    fn try_acquire(&self) -> Option<ConcurrencyPermit> {
        // Update dynamic limit before acquiring
        self.update_dynamic_limit();

        let max = self.max.load(Ordering::Relaxed);
        let mut current = self.current.load(Ordering::Relaxed);
        loop {
            if current >= max {
                return None;
            }
            match self.current.compare_exchange_weak(
                current,
                current + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(ConcurrencyPermit {
                        limiter: self.current.clone(),
                    });
                }
                Err(new_current) => current = new_current,
            }
        }
    }

    /// Acquire a permit, waiting until one is available.
    async fn acquire(&self) -> ConcurrencyPermit {
        loop {
            if let Some(permit) = self.try_acquire() {
                return permit;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Update the dynamic concurrency limit based on system load.
    ///
    /// This adjusts the maximum concurrent requests based on:
    /// - System memory usage
    /// - CPU load
    /// - Current utilization
    fn update_dynamic_limit(&self) {
        let utilization = self.current.load(Ordering::Relaxed) as f64 / self.base_max as f64;

        // Get system memory (in bytes)
        let memory_available = get_available_memory_bytes();
        let memory_gb = memory_available as f64 / (1024.0 * 1024.0 * 1024.0);

        // Calculate dynamic limit based on system conditions
        let dynamic_max = if memory_gb < 1.0 {
            // Low memory: reduce concurrency
            (self.base_max as f64 * 0.5).max(1.0) as usize
        } else if memory_gb < 2.0 {
            // Medium-low memory
            (self.base_max as f64 * 0.75).max(1.0) as usize
        } else if utilization > 0.8 {
            // High utilization: reduce slightly
            (self.base_max as f64 * 0.85).max(1.0) as usize
        } else {
            // Good conditions: use configured max
            self.base_max
        };

        self.max.store(dynamic_max, Ordering::Relaxed);
    }
}

/// Get available memory in bytes.
/// Returns a conservative estimate to avoid over-committing.
fn get_available_memory_bytes() -> usize {
    // Target 2GB minimum free memory for healthy operation
    // On systems with more memory, allow higher concurrency
    #[cfg(target_os = "linux")]
    {
        // Try to read from /proc/meminfo
        if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
            for line in info.lines() {
                if line.starts_with("MemAvailable:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<usize>() {
                            return kb * 1024;
                        }
                    }
                }
            }
        }
        // Fallback: assume 2GB available
        2 * 1024 * 1024 * 1024
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use sysctl to get memory info
        // For simplicity, return a conservative estimate
        4 * 1024 * 1024 * 1024 // Assume 4GB available
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, use GlobalMemoryStatusEx
        // For simplicity, return a conservative estimate
        4 * 1024 * 1024 * 1024 // Assume 4GB available
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // Unknown platform: conservative estimate
        2 * 1024 * 1024 * 1024
    }
}

/// A permit that releases when dropped.
struct ConcurrencyPermit {
    limiter: Arc<AtomicUsize>,
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        self.limiter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Process-wide count of in-flight FOREGROUND chat requests (any session,
/// any LlmInterface instance). Background LLM work — memory extraction,
/// summarization — consults this before firing on the SAME shared local
/// backend: a background call landing mid-turn queues behind / steals decode
/// from the user's stream on a small llama-server (observed 2026-10-14: a
/// 34s extraction overlapped the next turn's generation and the following
/// turns returned empty content after retry).
pub static FOREGROUND_CHAT_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// RAII guard for [`FOREGROUND_CHAT_INFLIGHT`]. Acquire alongside the
/// concurrency permit at every foreground entry point; the guard's lifetime
/// must mirror the permit's (function scope for blocking chat, stream scope
/// for streaming).
struct ForegroundChatGuard;

impl ForegroundChatGuard {
    fn acquire() -> Self {
        FOREGROUND_CHAT_INFLIGHT.fetch_add(1, Ordering::Relaxed);
        ForegroundChatGuard
    }
}

impl Drop for ForegroundChatGuard {
    fn drop(&mut self) {
        FOREGROUND_CHAT_INFLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Wrapper stream that holds a concurrency permit.
///
/// This ensures the permit is held for the entire lifetime of the stream,
/// releasing it only when the stream is dropped.
struct PermitStream<S> {
    inner: S,
    _permit: ConcurrencyPermit,
}

impl<S> PermitStream<S> {
    fn new(inner: S, permit: ConcurrencyPermit) -> Self {
        Self {
            inner,
            _permit: permit,
        }
    }
}

impl<S: Stream + Unpin> Stream for PermitStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

// Preserve the existing 18K guard on small windows while scaling it for larger
// contexts. This is a budget policy, not a parser for ambiguous model output.
fn long_prompt_thinking_threshold(max_context: usize) -> usize {
    18_000usize.max(max_context / 2)
}

/// Transient skill context for the current turn.
///
/// Tracks which skill ids have already been loaded so re-loading the same skill
/// (a common LLM pattern when stuck in a retry loop) doesn't duplicate the body
/// in the system prompt. Distinct skill ids accumulate under
/// [`skill_bodies_token_budget`] so 15+ full guides cannot zero history budget.
#[derive(Default)]
struct TransientSkillContext {
    loaded_ids: std::collections::HashSet<(String, Option<String>)>,
    /// Per-key ACTUAL injection state for every guide/section loaded THIS turn —
    /// complete and unbounded (the source of truth for duplicate loads).
    /// Kept separately from the display ring: a guide evicted from `recent`
    /// by the 8-entry cap still has its true state here, so a repeat request
    /// cannot misreport an in-context body as "not in context".
    turn_states: std::collections::HashMap<(String, Option<String>), RecentSkillState>,
    body: String,
    /// Display-only usage record of the most recent guides, kept ACROSS user
    /// turns (the bodies themselves are cleared at each turn boundary).
    /// Bounded ring; never consulted for load-state decisions.
    recent: std::collections::VecDeque<RecentSkillLoad>,
}

/// One entry of the cross-turn "recently used guides" record.
///
/// `state` records what ACTUALLY entered the skill block on the load that
/// put this entry here — full / partial (truncated) / skipped. A duplicate
/// load request must never upgrade the recorded state (the body was not
/// re-appended), and the note shown to the model must never claim a guide is
/// fully in context when the shared budget skipped or truncated it away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecentSkillState {
    /// The whole body entered the skill block.
    Full,
    /// Only a truncated prefix entered.
    Partial,
    /// Nothing entered (budget exhausted before this load).
    Skipped,
}

#[derive(Debug, Clone)]
struct RecentSkillLoad {
    key: String,
    state: RecentSkillState,
}

/// How many recent-guide entries the usage note keeps.
const RECENT_SKILL_MEMORY: usize = 8;

/// Refresh the bounded recent-usage record: update-or-push the key at the
/// back, trim to [`RECENT_SKILL_MEMORY`].
fn touch_recent(
    recent: &mut std::collections::VecDeque<RecentSkillLoad>,
    key: &str,
    state: RecentSkillState,
) {
    if let Some(pos) = recent.iter().position(|r| r.key == key) {
        recent.remove(pos);
    }
    recent.push_back(RecentSkillLoad {
        key: key.to_string(),
        state,
    });
    while recent.len() > RECENT_SKILL_MEMORY {
        recent.pop_front();
    }
}

/// The model-facing "recently used guides" usage note. Deliberately explicit
/// that entries are a USAGE RECORD — guide text is in context only when the
/// per-entry state says so; skipped/truncated loads are never claimed as
/// fully present. Rendered whenever any guide was consulted, INCLUDING while
/// other bodies are in the block: a later load the budget skipped must still
/// be visible as "not in context" in the very same request.
fn recent_skill_note(recent: &std::collections::VecDeque<RecentSkillLoad>) -> Option<String> {
    if recent.is_empty() {
        return None;
    }
    let mut lines = String::from(
        "## Recently Used Guides\nUsage record only — a guide's full text is in context only \
         when marked retained. Reload with the skill tool if you need its content.\n",
    );
    for entry in recent.iter().rev() {
        let state = match entry.state {
            RecentSkillState::Full => "retained above",
            RecentSkillState::Partial => "partially retained (truncated)",
            RecentSkillState::Skipped => "not in context",
        };
        lines.push_str(&format!("- {} ({})\n", entry.key, state));
    }
    Some(lines)
}

/// Total estimated-token budget for skill bodies injected into one system
/// prompt. **Pinned + transient share this single cap** on the production
/// assemble path ([`build_skill_injection_block`]) — they must not each get a
/// full budget or a large pin set plus a mid-turn skill load can still zero
/// history (plan §12 P1 / T4 production symptom).
///
/// Scales with the model window (`ctx/8`), clamped to `[2_000, 6_000]`:
/// - 8K → 2K, 32K → 4K, 64K+ → 6K.
///
/// Enough for a few full guides; not enough for the whole builtin catalog.
/// Independent of per-skill `token_budget` metadata and auto-match
/// [`crate::skills::TokenBudgetConfig`].
pub(crate) fn skill_bodies_token_budget(max_context: usize) -> usize {
    (max_context / 8).clamp(2_000, 6_000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillBodyAppendOutcome {
    Full,
    Truncated,
    Skipped,
    /// The same guide key was already injected this turn — nothing appended.
    /// Only produced by the session-level skill-context setter, never by the
    /// budget planner itself.
    AlreadyPresent,
}

/// Plan the next skill-body fragment under a total token budget.
///
/// `existing` is the already-accumulated body (no leading separator). Returns
/// `(outcome, piece)` where `piece` includes the `\n\n` separator when needed
/// and is empty on [`SkillBodyAppendOutcome::Skipped`].
pub(crate) fn plan_skill_body_append(
    existing: &str,
    content: &str,
    budget_tokens: usize,
) -> (SkillBodyAppendOutcome, String) {
    use crate::agent::tokenizer::{estimate_tokens, truncate_to_tokens};

    if content.is_empty() {
        return (SkillBodyAppendOutcome::Skipped, String::new());
    }
    let used = if existing.is_empty() {
        0
    } else {
        estimate_tokens(existing)
    };
    let sep = if existing.is_empty() { "" } else { "\n\n" };
    let sep_tokens = if sep.is_empty() {
        0
    } else {
        estimate_tokens(sep)
    };
    let remaining = budget_tokens.saturating_sub(used.saturating_add(sep_tokens));
    // Keep a tiny floor so we don't inject a useless stub.
    if remaining < 32 {
        return (SkillBodyAppendOutcome::Skipped, String::new());
    }
    let need = estimate_tokens(content);
    if need <= remaining {
        return (SkillBodyAppendOutcome::Full, format!("{sep}{content}"));
    }
    let truncated = truncate_to_tokens(
        content,
        remaining.saturating_sub(estimate_tokens("\n…[skill truncated: total skill budget]")),
    );
    if truncated.trim().is_empty() {
        return (SkillBodyAppendOutcome::Skipped, String::new());
    }
    (
        SkillBodyAppendOutcome::Truncated,
        format!("{sep}{truncated}\n…[skill truncated: total skill budget]"),
    )
}

/// Production-path skill injection: pinned guides first, then transient
/// skill-tool bodies, under **one** [`skill_bodies_token_budget`].
///
/// Earlier P1 capped each source independently, so a large pin set plus a
/// mid-turn load could still inject ~2× the budget into the system prompt.
pub(crate) fn build_skill_injection_block(
    pinned_chunks: &[(String, String)],
    transient_body: Option<&str>,
    budget_tokens: usize,
) -> String {
    let mut combined = String::new();
    for (skill_id, chunk) in pinned_chunks {
        let (outcome, piece) = plan_skill_body_append(&combined, chunk, budget_tokens);
        match outcome {
            SkillBodyAppendOutcome::Skipped => {
                tracing::warn!(
                    skill_id = %skill_id,
                    budget_tokens,
                    "pinned skill skipped: shared skill body budget exhausted"
                );
                break;
            }
            SkillBodyAppendOutcome::Truncated => {
                tracing::warn!(
                    skill_id = %skill_id,
                    budget_tokens,
                    "pinned skill truncated to fit shared skill body budget"
                );
                combined.push_str(&piece);
                break;
            }
            SkillBodyAppendOutcome::Full => combined.push_str(&piece),
            // plan_skill_body_append never returns this; exhaustive for the
            // session-level variant.
            SkillBodyAppendOutcome::AlreadyPresent => {}
        }
    }
    if let Some(body) = transient_body {
        if !body.trim().is_empty() {
            let chunk = format!(
                "## Skill Reference\nCommands in this skill are canonical — use them exactly; don't guess subcommand names.\n\n{body}"
            );
            let (outcome, piece) = plan_skill_body_append(&combined, &chunk, budget_tokens);
            match outcome {
                SkillBodyAppendOutcome::Skipped => {
                    tracing::warn!(
                        budget_tokens,
                        "transient skill reference skipped: shared skill body budget exhausted"
                    );
                }
                SkillBodyAppendOutcome::Truncated => {
                    tracing::warn!(
                        budget_tokens,
                        "transient skill reference truncated to fit shared skill body budget"
                    );
                    combined.push_str(&piece);
                }
                SkillBodyAppendOutcome::Full => combined.push_str(&piece),
                SkillBodyAppendOutcome::AlreadyPresent => {}
            }
        }
    }
    combined
}

/// Credential-free request defaults captured from the backend selected for a session.
#[derive(Clone)]
pub(crate) struct BackendRequestDefaults {
    /// Id of the instance this snapshot was captured from. A later edit of
    /// that backend uses it to refresh exactly the sessions still holding
    /// this snapshot (sessions bound elsewhere keep their own settings).
    backend_id: String,
    params: (f32, f32, usize, usize),
    thinking: (Option<bool>, Option<neomind_core::ThinkingEffort>),
    thinking_is_integral: bool,
}

impl From<&neomind_storage::LlmBackendInstance> for BackendRequestDefaults {
    fn from(instance: &neomind_storage::LlmBackendInstance) -> Self {
        Self {
            backend_id: instance.id.clone(),
            params: (
                instance.temperature,
                instance.top_p,
                instance.top_k,
                instance.max_tokens,
            ),
            thinking: (Some(instance.thinking_enabled), instance.thinking_effort),
            thinking_is_integral: instance.is_builtin && instance.thinking_is_integral,
        }
    }
}

/// Simple LLM chat interface.
///
/// This is a lightweight wrapper around LLM runtime with concurrency limiting.
/// It can operate in two modes:
/// 1. Direct runtime mode - Uses a manually set LLM runtime (backward compatible)
/// 2. Instance manager mode - Uses LlmBackendInstanceManager for dynamic backend switching
#[derive(Clone)]
pub struct LlmInterface {
    /// The LLM runtime backend (used in direct mode, stored as Arc for compatibility).
    llm: Arc<RwLock<Option<Arc<dyn LlmRuntime>>>>,
    /// Optional instance manager for dynamic backend switching.
    instance_manager: Option<Arc<LlmBackendInstanceManager>>,
    /// Model name.
    model: Arc<RwLock<Option<String>>>,
    /// Temperature.
    temperature: f32,
    /// Top-p sampling.
    top_p: f32,
    /// Top-k sampling (0 = disabled).
    top_k: usize,
    /// Max tokens.
    max_tokens: usize,
    /// Default system prompt (wrapped for dynamic updates).
    system_prompt: Arc<RwLock<String>>,
    /// Session-scoped suffix appended by the streaming chat prompt builder
    /// (page-scoped focus). Kept separate from `system_prompt` — the
    /// streaming builder assembles its own slim-template prompt and only
    /// injects this suffix, never the full legacy prompt.
    system_prompt_suffix: Arc<RwLock<Option<String>>>,
    /// Tool definitions for function calling.
    tool_definitions: Arc<RwLock<Vec<neomind_core::llm::backend::ToolDefinition>>>,
    /// System prompt cache to avoid rebuilding on every request.
    /// Stores (tools_hash, prompt) to invalidate when tools change.
    system_prompt_cache: Arc<RwLock<Option<(String, String)>>>,
    /// Hash of cached tool definitions for invalidation.
    cached_tools_hash: Arc<RwLock<Option<String>>>,
    /// Concurrency limiter.
    limiter: ConcurrencyLimiter,
    /// Whether to use instance manager for runtime retrieval.
    use_instance_manager: Arc<AtomicUsize>,
    /// Whether thinking mode is enabled (for direct mode when not using instance manager).
    /// Defaults to false for faster responses.
    thinking_enabled: Arc<RwLock<Option<bool>>>,
    backend_defaults: Arc<RwLock<Option<BackendRequestDefaults>>>,
    temperature_override: Arc<RwLock<Option<f32>>>,
    /// Last prompt token count from the most recent stream response.
    /// Updated in-band when the stream completes.
    last_prompt_tokens: Arc<tokio::sync::Mutex<Option<u32>>>,
    /// Set when the last context build had to drop history messages. Consumed
    /// at turn end so summarization triggers on ACTUAL truncation instead of
    /// a percentage of the total window — with a large system prompt/tool
    /// overhead the old 60%-of-window threshold could be unreachable while
    /// history was already being truncated every turn (2026-10-30).
    history_truncated: Arc<std::sync::atomic::AtomicBool>,
    /// Set when the streaming context window (outer builder) evicted the
    /// newest complete tool group under hard budget. The inner history
    /// selector inspects the ALREADY-TRIMMED window, so its survives-check
    /// would vacuously pass; the assemble gate consumes this flag (only on
    /// tool-capable requests) to refuse tools instead of continuing a
    /// feedback-less tool loop.
    history_window_dropped_latest_feedback: Arc<std::sync::atomic::AtomicBool>,
    /// Last desensitized prompt archive recorded at the final LLM send
    /// boundary (roles/lengths/hashes/budget + tool-feedback survival).
    /// No auth headers / raw secrets. Take/peek for failure forensics.
    last_prompt_archive:
        Arc<tokio::sync::Mutex<Option<crate::agent::prompt_capture::DesensitizedPromptArchive>>>,
    /// Intent classifier for staged processing.
    intent_classifier: IntentClassifier,
    /// Global timezone for time-aware prompts (IANA format, e.g., "Asia/Shanghai").
    /// Loaded from settings and used for all time-related context.
    global_timezone: Arc<RwLock<Option<String>>>,
    /// Skill registry for scenario-driven prompt injection.
    skill_registry: Arc<RwLock<Option<crate::skills::SharedSkillRegistry>>>,
    /// Transient skill context: skill tool results injected into system prompt during current turn.
    /// Cleared at the start of each new user message. Not stored in session history.
    /// Deduplicates by skill id so loading the same skill N times in a turn only appends once
    /// (preserves full body on first legitimate load).
    skill_context: Arc<RwLock<TransientSkillContext>>,
    /// Transient image-delegation prompt: when the active model is text-only
    /// but a vision companion described the images, this holds the FULL
    /// prompt (original message + descriptions) for the LLM while history
    /// keeps the ORIGINAL message + image attachments for display. One-shot
    /// (take semantics) — consumed by the multimodal stream/turn entry and
    /// never persisted.
    image_delegation_prompt: Arc<RwLock<Option<String>>>,
    /// Pinned skill IDs selected by the user for this session.
    /// These skills are injected as full guides (not just hints) into the system prompt.
    pinned_skills: Arc<RwLock<Vec<String>>>,
    /// Cross-turn TaskState prompt section (goal / confirmed / next). Set by
    /// stream entry from `AgentInternalState.task_state.to_prompt_section()`.
    task_state_prompt: Arc<RwLock<Option<String>>>,
    /// Timestamp pinned at the start of a user turn so multi-round ReAct calls
    /// within the same turn emit an identical `## Current time` tail and keep
    /// llama.cpp KV-prefix cache hot.
    turn_timestamp: Arc<RwLock<Option<chrono::DateTime<chrono::Utc>>>>,
}

impl LlmInterface {
    /// Resolve the thinking control for a request.
    ///
    /// Priority: local override (bool) > active instance's `thinking_effort`
    /// (unified) > instance's legacy `thinking_enabled` (bool) > model default.
    async fn resolve_thinking_control(
        &self,
    ) -> (Option<bool>, Option<neomind_core::ThinkingEffort>) {
        let local_thinking = *self.thinking_enabled.read().await;
        if let Some(local) = local_thinking {
            // A local override pins both: effort derives from the bool.
            return (
                Some(local),
                Some(neomind_core::ThinkingEffort::from_bool(local)),
            );
        }
        if let Some(defaults) = self.backend_defaults.read().await.as_ref() {
            return defaults.thinking;
        }
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                if let Some(inst) = manager.get_active_instance() {
                    return (Some(inst.thinking_enabled), inst.thinking_effort);
                }
            }
        }
        (None, None)
    }

    /// Whether the active backend instance is an integral-thinking model whose
    /// thinking cannot be turned off (e.g. LFM2.5).
    ///
    /// Prefer the backend bound to this session, then its instance manager.
    /// Legacy unbound interfaces fall back to the global builtin instance.
    /// A different globally active model must not change a bound session.
    async fn active_thinking_is_integral(&self) -> bool {
        if let Some(defaults) = self.backend_defaults.read().await.as_ref() {
            return defaults.thinking_is_integral;
        }
        if let Some(manager) = &self.instance_manager {
            return manager
                .get_active_instance()
                .is_some_and(|inst| inst.is_builtin && inst.thinking_is_integral);
        }
        match crate::get_instance_manager()
            .ok()
            .and_then(|m| m.get_active_instance())
        {
            Some(inst) => inst.is_builtin && inst.thinking_is_integral,
            None => false,
        }
    }

    /// Resolve the effective thinking control for a call, combining the
    /// local/instance resolution with the per-call `thinking_override`.
    ///
    /// For integral-thinking models (e.g. LFM2.5) the per-call override is
    /// IGNORED so non-chat calls (summarization, post-tool rounds) don't force
    /// thinking off — the model cannot disable it, so forcing `false` would
    /// waste the call. All other models keep the existing behavior: the
    /// override wins when present, pinning both `thinking_enabled` and the
    /// derived `thinking_effort`.
    async fn effective_thinking_control(
        &self,
        thinking_override: Option<bool>,
    ) -> (Option<bool>, Option<neomind_core::ThinkingEffort>) {
        let (enabled, effort) = self.resolve_thinking_control().await;
        // Backends prefer effort. Keep the legacy boolean consistent with it.
        let resolved = (
            effort
                .map(|e| e != neomind_core::ThinkingEffort::None)
                .or(enabled),
            effort,
        );
        if self.active_thinking_is_integral().await {
            resolved
        } else if let Some(ov) = thinking_override {
            (Some(ov), Some(neomind_core::ThinkingEffort::from_bool(ov)))
        } else {
            resolved
        }
    }

    /// Create a new LLM interface.
    pub fn new(config: ChatConfig) -> Self {
        let concurrent_limit = config.concurrent_limit;
        Self {
            llm: Arc::new(RwLock::new(None)),
            instance_manager: None,
            model: Arc::new(RwLock::new(Some(config.model))),
            temperature: config.temperature,
            top_p: config.top_p,
            top_k: config.top_k,
            max_tokens: config.max_tokens,
            system_prompt: Arc::new(RwLock::new("You are a helpful AI assistant.".to_string())),
            system_prompt_suffix: Arc::new(RwLock::new(None)),
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            system_prompt_cache: Arc::new(RwLock::new(None)),
            cached_tools_hash: Arc::new(RwLock::new(None)),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(0)),
            thinking_enabled: Arc::new(RwLock::new(
                neomind_storage::AgentDefaults::get().default_thinking_enabled,
            )), // Use backend default (from storage)
            backend_defaults: Arc::new(RwLock::new(None)),
            temperature_override: Arc::new(RwLock::new(None)),
            last_prompt_tokens: Arc::new(tokio::sync::Mutex::new(None)),
            history_truncated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            history_window_dropped_latest_feedback: Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            last_prompt_archive: Arc::new(tokio::sync::Mutex::new(None)),
            intent_classifier: IntentClassifier::default(),
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
            skill_registry: Arc::new(RwLock::new(None)),
            skill_context: Arc::new(RwLock::new(TransientSkillContext::default())),
            image_delegation_prompt: Arc::new(RwLock::new(None)),
            pinned_skills: Arc::new(RwLock::new(Vec::new())),
            task_state_prompt: Arc::new(RwLock::new(None)),
            turn_timestamp: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new LLM interface with instance manager integration.
    pub fn with_instance_manager(
        config: ChatConfig,
        manager: Arc<LlmBackendInstanceManager>,
    ) -> Self {
        let concurrent_limit = config.concurrent_limit;
        Self {
            llm: Arc::new(RwLock::new(None)),
            instance_manager: Some(manager),
            model: Arc::new(RwLock::new(Some(config.model))),
            temperature: config.temperature,
            top_p: config.top_p,
            top_k: config.top_k,
            max_tokens: config.max_tokens,
            system_prompt: Arc::new(RwLock::new("You are a helpful AI assistant.".to_string())),
            system_prompt_suffix: Arc::new(RwLock::new(None)),
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            system_prompt_cache: Arc::new(RwLock::new(None)),
            cached_tools_hash: Arc::new(RwLock::new(None)),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(1)),
            thinking_enabled: Arc::new(RwLock::new(
                neomind_storage::AgentDefaults::get().default_thinking_enabled,
            )), // Will use instance manager setting
            backend_defaults: Arc::new(RwLock::new(None)),
            temperature_override: Arc::new(RwLock::new(None)),
            last_prompt_tokens: Arc::new(tokio::sync::Mutex::new(None)),
            history_truncated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            history_window_dropped_latest_feedback: Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            last_prompt_archive: Arc::new(tokio::sync::Mutex::new(None)),
            intent_classifier: IntentClassifier::default(),
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
            skill_registry: Arc::new(RwLock::new(None)),
            skill_context: Arc::new(RwLock::new(TransientSkillContext::default())),
            image_delegation_prompt: Arc::new(RwLock::new(None)),
            pinned_skills: Arc::new(RwLock::new(Vec::new())),
            task_state_prompt: Arc::new(RwLock::new(None)),
            turn_timestamp: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the thinking mode for direct LLM usage (when not using instance manager).
    pub async fn set_thinking_enabled(&self, enabled: bool) {
        *self.thinking_enabled.write().await = Some(enabled);
    }

    /// Get the thinking mode setting.
    pub async fn get_thinking_enabled(&self) -> Option<bool> {
        *self.thinking_enabled.read().await
    }

    /// Restore the thinking mode to a previously-saved state (including None).
    /// Used in save/restore patterns (e.g. summarization temporarily disables thinking).
    pub async fn restore_thinking_enabled(&self, enabled: Option<bool>) {
        *self.thinking_enabled.write().await = enabled;
    }

    /// Take the last prompt token count from the most recent stream response.
    /// Returns the value and resets it to None.
    pub async fn peek_last_prompt_tokens(&self) -> Option<u32> {
        *self.last_prompt_tokens.lock().await
    }

    pub async fn take_last_prompt_tokens(&self) -> Option<u32> {
        self.last_prompt_tokens.lock().await.take()
    }

    /// Whether the last context build dropped history messages. Taking it
    /// clears the flag so one truncated turn triggers at most one summary.
    pub fn mark_history_truncated(&self) {
        self.history_truncated
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn take_history_truncated(&self) -> bool {
        self.history_truncated
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// Mark that the outer context window dropped the newest complete tool
    /// group (hard-budget eviction). The next tool-capable assemble request
    /// consumes this and refuses tools.
    pub fn mark_history_window_dropped_latest_feedback(&self) {
        self.history_window_dropped_latest_feedback
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Take (read-and-clear) the outer-window feedback-drop flag. Called only
    /// on tool-capable requests so summary/no-tool requests never consume it.
    pub fn take_history_window_dropped_latest_feedback(&self) -> bool {
        self.history_window_dropped_latest_feedback
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// Peek the last desensitized send-boundary prompt archive (no take).
    pub async fn peek_last_prompt_archive(
        &self,
    ) -> Option<crate::agent::prompt_capture::DesensitizedPromptArchive> {
        self.last_prompt_archive.lock().await.clone()
    }

    /// Take the last desensitized send-boundary prompt archive.
    pub async fn take_last_prompt_archive(
        &self,
    ) -> Option<crate::agent::prompt_capture::DesensitizedPromptArchive> {
        self.last_prompt_archive.lock().await.take()
    }

    /// Tip label for observation archives (`NEOMIND_TIP_SHA` or `"runtime"`).
    fn observation_tip_sha() -> String {
        std::env::var("NEOMIND_TIP_SHA").unwrap_or_else(|_| "runtime".into())
    }

    /// Record sanitized request shape at the final LLM send boundary.
    async fn observe_send_boundary(
        &self,
        assembled_messages: &[Message],
        history_before_trim: &[Message],
        available_tokens: usize,
        tools: Option<&[neomind_core::llm::backend::ToolDefinition]>,
        boundary: &str,
    ) {
        let tools_attached = tools.map(|t| !t.is_empty()).unwrap_or(false);
        let tool_names: Vec<String> = tools
            .map(|t| t.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        let tools_count = tool_names.len();
        let history_truncated_flag = self
            .history_truncated
            .load(std::sync::atomic::Ordering::SeqCst);
        let tip_sha = Self::observation_tip_sha();
        let mut archive = crate::agent::prompt_capture::build_prompt_archive(
            crate::agent::prompt_capture::PromptArchiveInput {
                tip_sha: &tip_sha,
                assembled_messages,
                history_before_trim,
                available_tokens,
                tools_count,
                tool_names,
                tools_attached,
                history_truncated_flag,
                notes: Vec::new(),
            },
        );
        archive.notes = vec![
            format!("send_boundary={boundary}"),
            format!("tools_attached={tools_attached}"),
            format!(
                "latest_tool_feedback_survived={}",
                archive.tool_feedback.survived
            ),
            format!(
                "latest_tool_feedback_label={:?}",
                archive.tool_feedback.latest_feedback_label
            ),
        ];
        if archive.tool_feedback.interaction_present_in_history && !archive.tool_feedback.survived {
            tracing::warn!(
                boundary,
                tools_attached,
                label = ?archive.tool_feedback.latest_feedback_label,
                "send_boundary_tool_feedback_dropped"
            );
        } else {
            tracing::debug!(
                boundary,
                message_count = archive.message_count,
                select_reason = %archive.budget.select_reason,
                feedback_survived = archive.tool_feedback.survived,
                "send_boundary_prompt_observed"
            );
        }
        *self.last_prompt_archive.lock().await = Some(archive);
    }

    /// Set the global timezone for time-aware prompts.
    /// This should be loaded from the settings store on initialization and updated when settings change.
    pub async fn set_global_timezone(&self, timezone: String) {
        *self.global_timezone.write().await = Some(timezone);
        // Clear the system prompt cache so time placeholders are re-evaluated with new timezone
        *self.system_prompt_cache.write().await = None;
    }

    /// Get the current global timezone setting.
    pub async fn get_global_timezone(&self) -> Option<String> {
        self.global_timezone.read().await.clone()
    }

    /// Load global timezone from settings store and apply it.
    /// Returns the loaded timezone or default if not found.
    pub async fn load_global_timezone(&self) -> AgentResult<String> {
        use neomind_storage::SettingsStore;

        let settings_store = SettingsStore::open_default()
            .map_err(|e| NeoMindError::Llm(format!("Failed to open settings store: {}", e)))?;

        let timezone = settings_store.get_global_timezone();
        self.set_global_timezone(timezone.clone()).await;

        tracing::debug!("Loaded global timezone: {}", timezone);
        Ok(timezone)
    }

    /// Enable or disable instance manager mode.
    pub async fn set_use_instance_manager(&self, use_manager: bool) {
        self.use_instance_manager
            .store(if use_manager { 1 } else { 0 }, Ordering::Relaxed);
    }

    /// Check if instance manager mode is enabled.
    /// Returns true if the flag is set, regardless of whether the instance manager is currently available.
    pub fn uses_instance_manager(&self) -> bool {
        self.use_instance_manager.load(Ordering::Relaxed) == 1
    }

    /// Get the current LLM runtime.
    /// Priority: Direct runtime (set via configure_llm) > Instance manager active runtime
    /// This ensures that when a specific backend is configured via backendId, it takes precedence.
    pub(crate) async fn get_runtime(&self) -> AgentResult<Arc<dyn LlmRuntime>> {
        // First, check if a direct runtime is set (via configure_llm)
        // This takes precedence over instance manager to support backendId selection
        let llm_guard = self.llm.read().await;
        if let Some(runtime) = llm_guard.as_ref() {
            tracing::debug!(
                model = %runtime.model_name(),
                "get_runtime: using direct runtime from configure_llm"
            );
            return Ok(Arc::clone(runtime));
        }
        drop(llm_guard);

        tracing::debug!("get_runtime: no direct runtime, checking instance manager");

        // Fall back to instance manager if no direct runtime is set
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                return manager
                    .get_active_runtime()
                    .await
                    .map_err(|e| NeoMindError::Llm(e.to_string()));
            }
        }

        Err(NeoMindError::Llm("LLM backend not ready".to_string()))
    }

    /// Compute the token budget for conversation history given the model's context window.
    ///
    /// Uses `estimate_tokens` for accurate token counting that handles Chinese (~1.8 tokens/char),
    /// English (~0.25 tokens/char), and mixed content correctly.
    ///
    /// Returns `(available_tokens, prompt_budget_tokens)` where `available_tokens` is the
    /// token budget for history messages after reserving space for everything else.
    async fn compute_history_budget(
        &self,
        max_ctx: usize,
        system_prompt: &str,
        user_message: &str,
        history_msg_count: usize,
        include_tools: bool,
    ) -> (usize, usize) {
        // Use more conservative budget for small contexts
        // < 8k: 50% for prompt, 50% for generation + overhead
        // < 16k: 60%, >= 16k: 70%
        // Thinking/reasoning models burn a large chunk of the GENERATION budget
        // on CoT tokens (Anthropic: thinking counts toward max_tokens), so trim
        // the prompt share further for them to avoid the model exhausting its
        // output budget mid-thinking.
        let thinking = self
            .model
            .read()
            .await
            .as_deref()
            .map(neomind_core::llm::detect_thinking)
            .unwrap_or(false);
        let prompt_ratio = if thinking {
            if max_ctx < 8192 {
                45
            } else if max_ctx < 16384 {
                55
            } else {
                65
            }
        } else if max_ctx < 8192 {
            50
        } else if max_ctx < 16384 {
            60
        } else {
            70
        };
        let prompt_budget = (max_ctx * prompt_ratio) / 100;

        // Estimate tool definition overhead in tokens using estimate_tokens.
        // Serialize each tool to its actual API JSON shape ({"type":"function",
        // "function":{"name","description","parameters"}}) so the JSON syntax
        // tokens are counted — the old per-field estimate undercounted.
        let tool_overhead_tokens = if include_tools {
            let tools = self.tool_definitions.read().await;
            if tools.is_empty() {
                0
            } else {
                tools
                    .iter()
                    .map(|t| {
                        let json = serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            }
                        });
                        estimate_tokens(&json.to_string())
                    })
                    .sum::<usize>()
            }
        } else {
            0
        };

        // Chat template overhead: ~10 tokens per message for role markers and special tokens
        let template_overhead_tokens = 10 * (history_msg_count + 2);

        // Use estimate_tokens for accurate Chinese/English token counting
        let system_tokens = estimate_tokens(system_prompt);
        let user_tokens = estimate_tokens(user_message);

        let reserved =
            system_tokens + user_tokens + tool_overhead_tokens + template_overhead_tokens + 200; // additional safety margin

        let available_tokens = prompt_budget.saturating_sub(reserved);

        tracing::debug!(
            max_ctx,
            prompt_ratio,
            prompt_budget,
            reserved,
            available_tokens,
            tool_overhead_tokens,
            template_overhead_tokens,
            system_tokens,
            user_tokens,
            "Computed history budget (token-based)"
        );

        (available_tokens, prompt_budget)
    }

    /// Apply the shared history-budget selector and mark truncation when needed.
    ///
    /// Recent tool-call/result groups are kept atomically. A zero soft budget
    /// yields an empty kept set (never a lone early user message). Callers must
    /// pass [`selected.reason`] through
    /// [`crate::agent::history_select::tools_allowed_after_history_select`]
    /// before attaching tools.
    fn select_history_for_prompt(
        &self,
        history_msgs: &[&Message],
        available_tokens: usize,
        log_label: &str,
    ) -> crate::agent::history_select::HistorySelectResult {
        let owned: Vec<Message> = history_msgs.iter().map(|m| (*m).clone()).collect();
        let selected = crate::agent::history_select::select_messages_for_history_budget(
            &owned,
            available_tokens,
        );
        if selected.truncated {
            tracing::info!(
                total_history_tokens = owned
                    .iter()
                    .map(|m| estimate_tokens(&m.content.as_text()))
                    .sum::<usize>(),
                kept_messages = selected.messages.len(),
                total_messages = owned.len(),
                available_tokens,
                used_tokens = selected.used_tokens,
                reason = ?selected.reason,
                "{log_label}"
            );
            self.history_truncated
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        selected
    }

    /// Get effective generation parameters.
    /// Bound backend defaults take precedence over the global active instance.
    /// Explicit session temperature overrides win; unbound direct runtimes use ChatConfig.
    async fn get_effective_params(&self) -> (f32, f32, usize, usize) {
        let defaults = self.backend_defaults.read().await.clone();
        let mut params = if let Some(defaults) = defaults {
            defaults.params
        } else if self.uses_instance_manager() {
            self.instance_manager
                .as_ref()
                .and_then(|manager| manager.get_active_instance())
                .map(|inst| (inst.temperature, inst.top_p, inst.top_k, inst.max_tokens))
                .unwrap_or((self.temperature, self.top_p, self.top_k, self.max_tokens))
        } else {
            (self.temperature, self.top_p, self.top_k, self.max_tokens)
        };
        if let Some(temperature) = *self.temperature_override.read().await {
            params.0 = temperature;
        }
        params
    }

    pub(crate) async fn set_backend_defaults(&self, defaults: BackendRequestDefaults) {
        *self.backend_defaults.write().await = Some(defaults);
    }

    /// Backend id this session's request-defaults snapshot was captured
    /// from, if any (None = direct runtime / unbound).
    pub(crate) async fn bound_backend_id(&self) -> Option<String> {
        self.backend_defaults
            .read()
            .await
            .as_ref()
            .map(|d| d.backend_id.clone())
    }

    pub(crate) async fn set_temperature_override(&self, temperature: f32) {
        *self.temperature_override.write().await = Some(temperature);
    }

    /// Set the LLM runtime (direct mode).
    pub async fn set_llm(&self, llm: Arc<dyn LlmRuntime>) {
        *self.backend_defaults.write().await = None;
        // Update the model name when setting a custom LLM
        let model_name = llm.model_name().to_string();
        *self.model.write().await = Some(model_name);
        let mut llm_guard = self.llm.write().await;
        *llm_guard = Some(llm);
    }

    /// Get the number of available permit slots.
    pub fn available_permits(&self) -> usize {
        let max = self.limiter.max.load(Ordering::Relaxed);
        let current = self.limiter.current.load(Ordering::Relaxed);
        max.saturating_sub(current)
    }

    /// Get the maximum context window size for the current LLM backend.
    ///
    /// This returns the actual context limit based on the model's capabilities.
    /// For example:
    /// - qwen3.5:4b -> 32768
    /// - llama3:8b -> 8192
    /// - deepseek-r1 -> 64000
    ///
    /// Returns a conservative default (4096) if the LLM is not ready.
    pub async fn max_context_length(&self) -> usize {
        // First, try to query the runtime directly (most accurate)
        if let Ok(runtime) = self.get_runtime().await {
            return runtime.max_context_length();
        }

        // Fall back to instance manager if runtime is not available
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                if let Some(instance) = manager.get_active_instance() {
                    return instance.capabilities.max_context;
                }
            }
        }

        4_096 // Conservative default if LLM not ready
    }

    /// Estimate the token overhead from system prompt + tool definitions.
    /// This is the non-history cost that must be deducted from the context window budget.
    pub async fn estimate_prompt_overhead_tokens(&self) -> usize {
        let (prompt_tokens, tools_tokens) = self.estimate_prompt_breakdown().await;
        let overhead = prompt_tokens + tools_tokens;
        tracing::debug!(
            "Prompt overhead: system_prompt={} tokens, tools={} tokens, total={} tokens",
            prompt_tokens,
            tools_tokens,
            overhead
        );
        overhead
    }

    /// Per-part prompt overhead: (system prompt tokens, tool-definition tokens).
    pub async fn estimate_prompt_breakdown(&self) -> (usize, usize) {
        // System prompt: build it and measure
        let system_prompt = self.build_system_prompt_with_tools(None).await;
        let prompt_tokens = crate::agent::tokenizer::estimate_tokens(&system_prompt);

        // Tool definitions: serialize to the actual API JSON shape and measure.
        // (The old per-field estimate undercounted JSON syntax tokens, notably
        // for large descriptions like the shell tool's ~3KB.)
        let tools = self.tool_definitions.read().await;
        let tools_tokens = if tools.is_empty() {
            0
        } else {
            tools
                .iter()
                .map(|t| {
                    let json = serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    });
                    crate::agent::tokenizer::estimate_tokens(&json.to_string())
                })
                .sum()
        };

        (prompt_tokens, tools_tokens)
    }

    /// Check if the current LLM backend supports multimodal (vision) input.
    ///
    /// Returns true if the active backend supports image input, false otherwise.
    ///
    /// Priority: Runtime capabilities > Storage layer capabilities
    /// This ensures correct detection when user selects a specific backend via backendId.
    pub async fn supports_multimodal(&self) -> bool {
        // 1. User override is sacred — check first. If the user has explicitly
        //    set multimodal_user_override, their value wins over every other
        //    signal (runtime detection, registry, heuristic).
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                if let Some(instance) = manager.get_active_instance() {
                    if let Some(override_val) = instance.capabilities.multimodal_user_override {
                        tracing::debug!(
                            override_val = %override_val,
                            model = %instance.model,
                            "supports_multimodal: respecting user override"
                        );
                        return override_val;
                    }
                }
            }
        }

        // 2. Runtime detection (most accurate — uses /api/show for Ollama, or
        //    model-name detection for cloud providers).
        if let Ok(runtime) = self.get_runtime().await {
            let caps = runtime.capabilities();
            tracing::debug!(
                multimodal = %caps.multimodal,
                model = %runtime.model_name(),
                "supports_multimodal: checking runtime capabilities"
            );
            return caps.multimodal;
        }

        tracing::debug!("supports_multimodal: no runtime available, checking instance manager");

        // 3. Fall back to stored capabilities (instance manager).
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                if let Some(instance) = manager.get_active_instance() {
                    tracing::debug!(
                        supports_multimodal = %instance.capabilities.supports_multimodal,
                        model = %instance.model,
                        "supports_multimodal: using instance manager capabilities"
                    );
                    return instance.capabilities.supports_multimodal;
                }
            }
        }

        tracing::warn!("supports_multimodal: no runtime or instance available, returning false");
        false
    }

    /// Warm up the model by sending a minimal request.
    ///
    /// This eliminates the ~500ms first-request latency by triggering model loading
    /// during initialization. Should be called during application startup.
    ///
    /// # Example
    /// ```ignore
    /// // During application startup
    /// llm_interface.warmup().await?;
    /// ```
    pub async fn warmup(&self) -> AgentResult<()> {
        match self.get_runtime().await {
            Ok(runtime) => runtime
                .warmup()
                .await
                .map_err(|e| NeoMindError::Llm(e.to_string())),
            Err(e) => Err(e),
        }
    }

    /// Set the system prompt (builder pattern).
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        // Store the prompt in Arc<RwLock<String>>
        self.system_prompt = Arc::new(RwLock::new(prompt.into()));
        self
    }

    /// Set the streaming-path system prompt suffix (builder pattern).
    pub fn with_system_prompt_suffix(mut self, suffix: Option<String>) -> Self {
        self.system_prompt_suffix = Arc::new(RwLock::new(suffix));
        self
    }

    /// Set the streaming-path system prompt suffix (for runtime changes).
    /// Invalidates nothing — the suffix is appended after the cached base.
    pub async fn set_system_prompt_suffix(&self, suffix: Option<String>) {
        *self.system_prompt_suffix.write().await = suffix;
    }

    /// The streaming-path system prompt suffix, if set.
    pub async fn get_system_prompt_suffix(&self) -> Option<String> {
        self.system_prompt_suffix.read().await.clone()
    }

    /// Set the system prompt (for dynamic updates).
    pub async fn set_system_prompt(&self, prompt: &str) {
        *self.system_prompt.write().await = prompt.to_string();
    }

    /// Get the current system prompt.
    pub async fn get_system_prompt(&self) -> String {
        self.system_prompt.read().await.clone()
    }

    /// Set tool definitions for function calling.
    ///
    /// Tool names are automatically sanitized for OpenAI-compatible API
    /// compatibility (e.g., `test.extension:cmd` → `test_extension_cmd`).
    /// The original names are preserved in a reverse mapping for tool call routing.
    pub async fn set_tool_definitions(
        &self,
        tools: Vec<neomind_core::llm::backend::ToolDefinition>,
    ) {
        use neomind_core::llm::backend::sanitize_tool_name;

        let sanitized: Vec<neomind_core::llm::backend::ToolDefinition> = tools
            .into_iter()
            .map(|mut t| {
                t.name = sanitize_tool_name(&t.name);
                t
            })
            .collect();
        *self.tool_definitions.write().await = sanitized;
        // Invalidate system prompt cache when tools change
        self.invalidate_prompt_cache().await;
    }

    /// Get current tool definitions.
    pub async fn get_tool_definitions(&self) -> Vec<neomind_core::llm::backend::ToolDefinition> {
        self.tool_definitions.read().await.clone()
    }

    /// Set the skill registry for scenario-driven prompt injection.
    pub async fn set_skill_registry(&self, registry: crate::skills::SharedSkillRegistry) {
        *self.skill_registry.write().await = Some(registry);
    }

    /// Get the skill registry (for external access).
    pub async fn get_skill_registry(&self) -> Option<crate::skills::SharedSkillRegistry> {
        self.skill_registry.read().await.clone()
    }

    /// Clear transient skill context (called at start of each new user message).
    /// Stage the image-delegation prompt for the NEXT multimodal turn (see
    /// the field doc). The turn entry TAKES it — staging twice before a
    /// turn simply replaces the pending prompt.
    pub async fn set_image_delegation_prompt(&self, prompt: String) {
        *self.image_delegation_prompt.write().await = Some(prompt);
    }

    /// One-shot consume: returns the staged delegation prompt (if any) and
    /// clears it, so a missed turn can never leak the prompt into a later,
    /// unrelated message.
    pub async fn take_image_delegation_prompt(&self) -> Option<String> {
        self.image_delegation_prompt.write().await.take()
    }

    pub async fn clear_skill_context(&self) {
        // Turn boundary: drop bodies + dedup set, KEEP the recent-usage record
        // (bounded) so the next turn knows what was already consulted. All
        // entries flip to not-retained — their bodies just left the context,
        // and the usage note must never claim otherwise.
        let mut guard = self.skill_context.write().await;
        guard.loaded_ids.clear();
        guard.turn_states.clear();
        guard.body.clear();
        for entry in guard.recent.iter_mut() {
            entry.state = RecentSkillState::Skipped;
        }
    }

    /// Pin the current UTC timestamp for the duration of a single user turn so
    /// multi-round follow-up requests emit a byte-identical `## Current time`
    /// tail and preserve llama.cpp KV-prefix cache.
    pub async fn pin_turn_timestamp(&self) {
        *self.turn_timestamp.write().await = Some(chrono::Utc::now());
    }

    #[cfg(test)]
    pub(crate) async fn set_turn_timestamp_for_test(
        &self,
        timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        *self.turn_timestamp.write().await = timestamp;
    }

    /// Append transient skill context (from skill tool results).
    ///
    /// Deduplicates by `(skill_id, section)`: loading a skill overview and then
    /// loading a specific reference section in the same turn are distinct entries
    /// and both survive; repeating the same `(skill_id, section)` is a no-op
    /// (first load retained). Distinct loads accumulate under
    /// [`skill_bodies_token_budget`] — later loads truncate or skip when the
    /// total would blow the fixed-prefix budget (plan §12 P1).
    pub async fn set_skill_context(&self, skill_id: String, content: String) {
        self.set_skill_context_outcome(skill_id, content).await;
    }

    /// [`Self::set_skill_context`] returning the injection outcome — the
    /// internal full/partial/skipped/already_present state (surfaced via the
    /// recent-usage note and diagnostics, not a new model-facing parameter).
    pub(crate) async fn set_skill_context_outcome(
        &self,
        skill_id: String,
        content: String,
    ) -> SkillBodyAppendOutcome {
        let (canonical_id, section) = normalize_skill_dedup_key(&skill_id, &content);
        self.set_skill_section_context_outcome(canonical_id, section, content)
            .await
    }

    /// Append transient skill context keyed explicitly by `(skill_id, section)`.
    pub(crate) async fn set_skill_section_context_outcome(
        &self,
        skill_id: String,
        section: Option<String>,
        content: String,
    ) -> SkillBodyAppendOutcome {
        // Reserve tokens already committed to pinned skills so mid-turn loads
        // cannot fill the whole shared budget before assemble merges both.
        let pinned_reserved = self.estimate_pinned_skill_tokens().await;
        let total = skill_bodies_token_budget(self.max_context_length().await);
        let budget = total.saturating_sub(pinned_reserved);
        let dedup_key = (skill_id.clone(), section.clone());
        let display_key = format_skill_dedup_display_key(&skill_id, section.as_deref());
        let mut guard = self.skill_context.write().await;
        if !guard.loaded_ids.insert(dedup_key.clone()) {
            // Already loaded this turn — skip to avoid duplicate context bloat.
            // The usage record keeps the state recorded by the ORIGINAL load
            // (a repeat request must never upgrade a partial/skipped entry to
            // "full": the body was not re-appended).
            tracing::info!(
                target: "neomind::skills",
                skill = %display_key,
                outcome = "already_present",
                "skill_inject"
            );
            let prior = guard
                .turn_states
                .get(&dedup_key)
                .copied()
                .unwrap_or(RecentSkillState::Skipped);
            touch_recent(&mut guard.recent, &display_key, prior);
            return SkillBodyAppendOutcome::AlreadyPresent;
        }
        let (outcome, piece) = plan_skill_body_append(&guard.body, &content, budget);
        let recorded_state = match outcome {
            SkillBodyAppendOutcome::Full => RecentSkillState::Full,
            SkillBodyAppendOutcome::Truncated => RecentSkillState::Partial,
            SkillBodyAppendOutcome::Skipped => RecentSkillState::Skipped,
            SkillBodyAppendOutcome::AlreadyPresent => RecentSkillState::Skipped,
        };
        let logged = match outcome {
            SkillBodyAppendOutcome::Full => "full",
            SkillBodyAppendOutcome::Truncated => {
                tracing::warn!(
                    skill_id = %display_key,
                    budget_tokens = budget,
                    "skill context truncated to fit total skill body budget"
                );
                "partial"
            }
            SkillBodyAppendOutcome::Skipped => {
                tracing::warn!(
                    skill_id = %display_key,
                    budget_tokens = budget,
                    "skill context skipped: total skill body budget exhausted"
                );
                "skipped"
            }
            SkillBodyAppendOutcome::AlreadyPresent => "already_present",
        };
        tracing::info!(
            target: "neomind::skills",
            skill = %display_key,
            outcome = logged,
            budget_tokens = budget,
            "skill_inject"
        );
        match outcome {
            SkillBodyAppendOutcome::Full | SkillBodyAppendOutcome::Truncated => {
                guard.body.push_str(&piece)
            }
            // Key stays in loaded_ids so the model cannot re-append the same
            // oversized body in a retry loop.
            SkillBodyAppendOutcome::Skipped | SkillBodyAppendOutcome::AlreadyPresent => {}
        }
        guard.turn_states.insert(dedup_key, recorded_state);
        touch_recent(&mut guard.recent, &display_key, recorded_state);
        outcome
    }

    /// Get transient skill context body (None if empty).
    pub async fn get_skill_context(&self) -> Option<String> {
        let guard = self.skill_context.read().await;
        if guard.body.is_empty() {
            None
        } else {
            Some(guard.body.clone())
        }
    }

    /// Set pinned skill IDs for this session (user-selected skills).
    pub async fn set_pinned_skills(&self, skills: Vec<String>) {
        *self.pinned_skills.write().await = skills;
    }

    /// Get pinned skill IDs for this session.
    pub async fn get_pinned_skills(&self) -> Vec<String> {
        self.pinned_skills.read().await.clone()
    }

    /// Inject / clear the TaskState prompt section for the next assemble.
    pub async fn set_task_state_prompt(&self, section: Option<String>) {
        *self.task_state_prompt.write().await = section;
    }

    /// Estimated tokens already committed to pinned skill chunks (same shape
    /// as the production assemble path). Used to reserve shared budget for
    /// mid-turn [`set_skill_context`] loads.
    async fn estimate_pinned_skill_tokens(&self) -> usize {
        use crate::agent::tokenizer::estimate_tokens;

        let pinned = self.pinned_skills.read().await.clone();
        if pinned.is_empty() {
            return 0;
        }
        let Some(registry) = self.skill_registry.read().await.clone() else {
            return 0;
        };
        let registry_guard = registry.read().await;
        let mut body = String::new();
        let budget = skill_bodies_token_budget(self.max_context_length().await);
        for skill_id in &pinned {
            if let Some(skill) = registry_guard.get(skill_id) {
                let chunk = format!(
                    "## Pinned Skill: {}\n{}",
                    skill.metadata.name,
                    skill.body_within_budget()
                );
                let (outcome, piece) = plan_skill_body_append(&body, &chunk, budget);
                if matches!(outcome, SkillBodyAppendOutcome::Skipped) {
                    break;
                }
                body.push_str(&piece);
                if matches!(outcome, SkillBodyAppendOutcome::Truncated) {
                    break;
                }
            }
        }
        estimate_tokens(&body)
    }

    /// Invalidate the system prompt cache.
    /// Call this when tools or configuration changes.
    pub async fn invalidate_prompt_cache(&self) {
        *self.system_prompt_cache.write().await = None;
        *self.cached_tools_hash.write().await = None;
    }

    /// Build the base system prompt (without user-specific parts).
    /// This is cached to avoid rebuilding on every request.
    async fn build_base_system_prompt(&self) -> String {
        use crate::prompts::PromptBuilder;

        // Check cache first
        {
            let cache_read = self.system_prompt_cache.read().await;
            if let Some((_, cached)) = cache_read.as_ref() {
                return cached.clone();
            }
        }

        // Build base prompt using PromptBuilder
        let supports_vision = self.supports_multimodal().await;
        let base_prompt = PromptBuilder::new()
            .with_thinking(true)
            .with_vision(supports_vision)
            .build_system_prompt();

        let prompt = base_prompt;

        // Cache the result — key includes vision flag to invalidate on model change
        let cache_key = format!("base_prompt_v:{}", supports_vision);
        {
            let mut cache_write = self.system_prompt_cache.write().await;
            *cache_write = Some((cache_key, prompt.clone()));
        }

        prompt
    }

    /// Build the base system prompt with current time injected.
    /// This replaces the time placeholders with actual time values using the configured global timezone.
    pub async fn build_base_system_prompt_with_time(&self, timezone: Option<&str>) -> String {
        // Get the base prompt (which contains placeholders), then resolve them.
        let base_prompt = self.build_base_system_prompt().await;
        self.inject_time_placeholders(base_prompt, timezone).await
    }

    /// Build system prompt with tool descriptions.
    /// Uses enhanced prompts from prompts module for better conversation quality.
    /// Uses cached base prompt with time placeholders replaced and adds user-specific parts.
    pub(crate) async fn build_system_prompt_with_tools(
        &self,
        user_message: Option<&str>,
    ) -> String {
        let prompt = self.assemble_system_prompt(user_message).await;
        self.inject_time_placeholders(prompt, None).await
    }

    /// Assemble session/skill context without resolving volatile time fields.
    /// All chat paths append `## Current time` LAST after all stable and
    /// session sections so KV-prefix caches stay warm.
    async fn assemble_system_prompt(&self, user_message: Option<&str>) -> String {
        // Start from the static base prompt (no volatile time block).
        // `inject_time_placeholders` appends `## Current time` strictly at the
        // tail after suffix, pinned/transient skills, task state, intent addon,
        // and any history summary.
        let mut prompt = self.build_base_system_prompt().await;

        // Session-scoped suffix (page-scoped focus). This is the ONLY place
        // the streaming path honors it — `system_prompt` (the legacy full
        // prompt) is intentionally not appended here to avoid duplicating
        // tools/capability sections the slim template already carries.
        if let Some(suffix) = self.system_prompt_suffix.read().await.clone() {
            if !suffix.trim().is_empty() {
                prompt.push_str("\n\n");
                prompt.push_str(suffix.trim());
                prompt.push('\n');
            }
        }

        // Pinned (user-selected) + transient skill-tool bodies share one total
        // budget on this production assemble path. Per-skill metadata still
        // caps individuals via body_within_budget. The eval-only no-skill
        // mode skips every skill surface (see skills::disabled_by_env).
        if !crate::skills::disabled_by_env() {
            let pinned = self.pinned_skills.read().await.clone();
            let mut pinned_chunks: Vec<(String, String)> = Vec::new();
            if !pinned.is_empty() {
                if let Some(registry) = self.skill_registry.read().await.clone() {
                    let registry_guard = registry.read().await;
                    for skill_id in &pinned {
                        if let Some(skill) = registry_guard.get(skill_id) {
                            pinned_chunks.push((
                                skill_id.clone(),
                                format!(
                                    "## Pinned Skill: {}\n{}",
                                    skill.metadata.name,
                                    skill.body_within_budget()
                                ),
                            ));
                        }
                    }
                }
            }
            // Auto-load skill guides — DISABLED: the slim prompt (default) dropped the
            // full CLI reference table, and the shell tool's Command Choice covers only
            // a subset of domains. Skills are loaded on-demand via the skill tool
            // (search/load) when the LLM needs domain-specific guidance beyond the
            // always-present Command Choice (complex workflows, error troubleshooting).
            // Re-enabling auto-load would re-add ~3000 tokens per turn.
            let transient = self.get_skill_context().await;
            let budget = skill_bodies_token_budget(self.max_context_length().await);
            let skill_block =
                build_skill_injection_block(&pinned_chunks, transient.as_deref(), budget);
            if !skill_block.is_empty() {
                prompt.push('\n');
                prompt.push_str(&skill_block);
                prompt.push('\n');
            }
            // Cross-turn/within-turn usage note: ALWAYS rendered when any guide
            // was consulted — including while bodies are present, so a later load
            // the shared budget skipped is visible as "not in context" in the
            // very same request (review P1-2: state must reach the model, not
            // just the logs).
            let recent = {
                let guard = self.skill_context.read().await;
                recent_skill_note(&guard.recent)
            };
            if let Some(note) = recent {
                prompt.push('\n');
                prompt.push_str(&note);
                prompt.push('\n');
            }
        }

        // G2 TaskState (goal / confirmed steps / next) — production path.
        if let Some(section) = self.task_state_prompt.read().await.clone() {
            if !section.trim().is_empty() {
                prompt.push('\n');
                prompt.push_str(section.trim());
                prompt.push('\n');
            }
        }

        // Add intent-specific addon if we can classify the user's message
        if let Some(msg) = user_message {
            let intent = self.intent_classifier.classify_for_prompt(msg);

            // Get intent addon using legacy PromptBuilder
            use crate::prompts::PromptBuilder;

            let task_type = match intent {
                Some(crate::agent::staged::IntentCategory::Device) => "device",
                Some(crate::agent::staged::IntentCategory::Data) => "data",
                Some(crate::agent::staged::IntentCategory::Rule) => "rule",
                _ => "general",
            };

            // Get intent-specific addon from PromptBuilder
            let addon = PromptBuilder::new().get_intent_prompt_addon(task_type);

            if !addon.is_empty() {
                prompt.push_str(&addon);
            }
        }

        prompt
    }

    /// History filtering intentionally discards old platform prompts. Keep the
    /// context builder's summary in the CURRENT system prompt before budgeting
    /// history, otherwise both summarized messages and their replacement vanish.
    fn with_history_summary(mut prompt: String, history: Option<&[Message]>) -> String {
        for message in history.into_iter().flatten() {
            if message.role == neomind_core::MessageRole::System {
                let text = message.content.as_text();
                if text.starts_with(crate::agent::types::CONVERSATION_SUMMARY_PREFIX) {
                    prompt.push_str("\n\n");
                    prompt.push_str(&text);
                }
            }
        }
        prompt
    }

    /// Resolve time placeholders or append `## Current time` strictly at the
    /// end of an assembled system prompt.
    ///
    /// Uses the turn-pinned timestamp when present so multi-round ReAct calls
    /// within one turn emit a byte-identical tail and avoid invalidating the
    /// local KV-prefix cache across second/minute boundaries.
    async fn inject_time_placeholders(&self, prompt: String, timezone: Option<&str>) -> String {
        use crate::prompts::{
            CURRENT_TIME_PLACEHOLDER, LOCAL_TIME_PLACEHOLDER, TIMEZONE_PLACEHOLDER,
        };

        // Reuse the turn-pinned timestamp when available; fall back to Utc::now().
        let now = self
            .turn_timestamp
            .read()
            .await
            .unwrap_or_else(chrono::Utc::now);
        let current_time_utc = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();

        // Use self.global_timezone first, then parameter, then default
        let effective_timezone = self
            .global_timezone
            .read()
            .await
            .as_ref()
            .cloned()
            .or_else(|| timezone.map(|s| s.to_string()))
            .unwrap_or_else(|| "Asia/Shanghai".to_string());

        // Parse timezone to get local time
        let tz = effective_timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Tz::Asia__Shanghai); // Default to Shanghai on error

        let local_time = now
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // Get additional time context for better LLM understanding
        let day_of_week = now.with_timezone(&tz).format("%A").to_string();
        let date_str = now.with_timezone(&tz).format("%B %d, %Y").to_string();

        // Get time period description (morning, afternoon, evening, night)
        let hour_str = now.with_timezone(&tz).format("%H").to_string();
        let hour: u32 = hour_str.parse().unwrap_or(12);
        let time_period = match hour {
            5..=11 => "Morning",
            12..=13 => "Noon",
            14..=17 => "Afternoon",
            18..=22 => "Evening",
            _ => "Night",
        };

        // Build enhanced time context
        let local_time_with_context = format!(
            "{} {} ({}{})",
            date_str, local_time, time_period, day_of_week
        );

        let has_placeholder = prompt.contains(CURRENT_TIME_PLACEHOLDER)
            || prompt.contains(LOCAL_TIME_PLACEHOLDER)
            || prompt.contains(TIMEZONE_PLACEHOLDER);

        if has_placeholder {
            prompt
                .replace(CURRENT_TIME_PLACEHOLDER, &current_time_utc)
                .replace(LOCAL_TIME_PLACEHOLDER, &local_time_with_context)
                .replace(TIMEZONE_PLACEHOLDER, &effective_timezone)
        } else {
            let trimmed = prompt.trim_end();
            format!(
                "{trimmed}\n\n## Current time\n- Current Time (UTC): {current_time_utc}\n- Local: {local_time_with_context}\n- Timezone: {effective_timezone}\n"
            )
        }
    }

    /// Update the model name.
    pub async fn update_model(&self, model: String) {
        let mut model_guard = self.model.write().await;
        *model_guard = Some(model);
    }

    /// Check if the LLM backend is ready.
    pub async fn is_ready(&self) -> bool {
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                return manager.get_active_instance().is_some();
            }
        }
        let llm_guard = self.llm.read().await;
        llm_guard.as_ref().is_some()
    }

    /// Send a chat message and get a response.
    pub async fn chat(&self, user_message: impl Into<String>) -> AgentResult<ChatResponse> {
        self.chat_internal(user_message, None, None).await
    }

    /// Send a chat message with an explicit per-call thinking override.
    ///
    /// [race-free background work] Background summarization used to mutate
    /// the interface-global thinking flag (set false -> call -> restore): a
    /// user turn started during the multi-second summary call ran with
    /// thinking silently off, a user toggle made mid-summary was clobbered
    /// by the restore, and an abort between set/restore left thinking
    /// disabled forever. A per-call override touches no shared state.
    pub async fn chat_with_thinking(
        &self,
        user_message: impl Into<String>,
        thinking_override: Option<bool>,
    ) -> AgentResult<ChatResponse> {
        self.chat_internal(user_message, None, thinking_override)
            .await
    }

    /// Send a chat message with conversation history.
    pub async fn chat_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> AgentResult<ChatResponse> {
        self.chat_internal(user_message, Some(history), None).await
    }

    /// Send a multimodal message (with images) with conversation history.
    /// This is used when the user sends images along with their text.
    pub async fn chat_multimodal_with_history(
        &self,
        user_message: Message, // Can contain text + images
        history: &[Message],
    ) -> AgentResult<ChatResponse> {
        self.chat_internal_message(user_message, Some(history))
            .await
    }

    /// Internal chat implementation.
    async fn chat_internal(
        &self,
        user_message: impl Into<String>,
        history: Option<&[Message]>,
        thinking_override: Option<bool>,
    ) -> AgentResult<ChatResponse> {
        let user_message: String = user_message.into();

        // Clear transient skill context and pin timestamp for this turn
        self.clear_skill_context().await;
        self.pin_turn_timestamp().await;

        let max_ctx = self.max_context_length().await;

        // === FAST PATH: Simple greetings ===
        // Bypass LLM for simple greetings to improve response time
        // (is_simple_greeting is shared with the streaming guards).
        if crate::agent::is_simple_greeting(&user_message) {
            let greeting_response = "您好！我是 NeoMind 智能助手。我可以帮您：\n\
                • 查看设备列表 - 说「列出设备」\n\
                • 查询设备数据 - 说「查询温度」\n\
                • 创建自动化规则 - 说「创建规则」\n\
                • 查看所有规则 - 说「列出规则」";

            return Ok(ChatResponse {
                text: greeting_response.to_string(),
                tokens_used: 0,
                duration: Duration::from_millis(0),
                finish_reason: "stop".to_string(),
                thinking: None,
            });
        }

        // Acquire permit for concurrency limiting
        let _permit = self.limiter.acquire().await;
        let _fg_guard = ForegroundChatGuard::acquire();

        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Check if we have tools registered
        let has_tools = !self.tool_definitions.read().await.is_empty();

        let system_prompt = if has_tools {
            let assembled = self.assemble_system_prompt(Some(&user_message)).await;
            let assembled = Self::with_history_summary(assembled, history);
            self.inject_time_placeholders(assembled, None).await
        } else {
            Self::with_history_summary(self.system_prompt.read().await.clone(), history)
        };

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model_from_config = model_guard.as_ref().cloned();
        drop(model_guard);

        // If no model is configured, try to get it from the LLM runtime
        let model = if let Some(m) = model_from_config {
            m
        } else {
            // Try to get model name from the runtime
            let llm_guard = self.llm.read().await;
            if let Some(ref llm) = *llm_guard {
                llm.model_name().to_string()
            } else {
                // Ultimate fallback
                "qwen3.5:4b".to_string()
            }
        };

        // Get thinking control - priority: local setting > instance setting.
        let (thinking_enabled, thinking_effort) =
            self.effective_thinking_control(thinking_override).await;

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            thinking_effort = ?thinking_effort,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal"
        );

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, _static_max_tokens) =
            self.get_effective_params().await;

        // Don't set max_tokens — let the LLM backend use its own default.
        // Backends (Ollama, OpenAI-compatible) have their own num_predict limits.

        let params = neomind_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            thinking_effort,
            max_context: None,
            // Chat path — no JSON constraint (structured extraction sets this).
            json_mode: None,
        };

        let system_msg = Message::system(&system_prompt);
        let user_msg = Message::user(&user_message);

        // Build messages with history if provided, truncated to fit context window
        let mut history_reason = crate::agent::history_select::HistorySelectReason::Fits;
        let mut available_tokens_obs = 0usize;
        let mut history_before_trim: Vec<Message> = Vec::new();
        let mut selected_history: Vec<Message> = Vec::new();
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();
            history_before_trim = history_msgs.iter().map(|m| (*m).clone()).collect();

            let (available_tokens, _prompt_budget) = self
                .compute_history_budget(
                    max_ctx,
                    &system_prompt,
                    &user_message,
                    history_msgs.len(),
                    has_tools,
                )
                .await;
            available_tokens_obs = available_tokens;

            let selected = self.select_history_for_prompt(
                &history_msgs,
                available_tokens,
                "Truncated conversation history to fit context window",
            );
            history_reason = selected.reason;
            selected_history = selected.messages.clone();
            msgs.extend(selected.messages);

            msgs.push(user_msg);
            msgs
        } else {
            vec![system_msg, user_msg]
        };

        // Outer context window (streaming layer) may have evicted the newest
        // complete tool group under hard budget — the inner survives-check
        // below inspects the already-trimmed window and would vacuously pass.
        // Consume the flag only on tool-capable requests.
        let outer_window_dropped_latest_feedback = if has_tools {
            self.take_history_window_dropped_latest_feedback()
        } else {
            false
        };
        let attach_tools = crate::agent::history_select::tools_allowed_after_history_select(
            history_reason,
            &history_before_trim,
            &selected_history,
            has_tools,
            outer_window_dropped_latest_feedback,
        );
        if has_tools && !attach_tools {
            tracing::warn!(
                reason = ?history_reason,
                outer_window_dropped_latest_feedback,
                "no_feedback_degrade_no_tools: refusing tool-enabled assemble when latest tool feedback cannot be kept"
            );
        }

        // Get tool definitions — no vision filtering here (chat_internal is
        // text-only; chat_internal_message handles image filtering separately).
        let tools_input = if attach_tools {
            let tools = self.tool_definitions.read().await;
            let result = if tools.is_empty() {
                None
            } else {
                Some(tools.clone())
            };
            drop(tools);
            result
        } else {
            None
        };

        self.observe_send_boundary(
            &messages,
            &history_before_trim,
            available_tokens_obs,
            tools_input.as_deref(),
            "chat_internal",
        )
        .await;

        let input = LlmInput {
            messages,
            params,
            model: Some(model),
            stream: false,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let output = llm
            .generate(input)
            .await
            .map_err(|e| NeoMindError::Llm(e.to_string()))?;

        let duration = start.elapsed();
        let tokens_used = output
            .usage
            .map(|u| u.completion_tokens as usize)
            .unwrap_or_else(|| output.text.split_whitespace().count());

        Ok(ChatResponse {
            text: output.text,
            tokens_used,
            duration,
            finish_reason: format!("{:?}", output.finish_reason),
            thinking: output.thinking,
        })
    }

    /// Internal chat implementation that accepts a Message directly (for multimodal).
    async fn chat_internal_message(
        &self,
        user_message: Message, // Can contain text + images
        history: Option<&[Message]>,
    ) -> AgentResult<ChatResponse> {
        // Clear transient skill context and pin timestamp for this turn
        self.clear_skill_context().await;
        self.pin_turn_timestamp().await;

        let max_ctx = self.max_context_length().await;

        // Acquire permit for concurrency limiting
        let _permit = self.limiter.acquire().await;
        let _fg_guard = ForegroundChatGuard::acquire();

        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Check if we have tools registered
        let has_tools = !self.tool_definitions.read().await.is_empty();

        let system_prompt = if has_tools {
            // Extract text from user message for system prompt
            let user_text = user_message.content.as_text();
            let assembled = self.assemble_system_prompt(Some(&user_text)).await;
            let assembled = Self::with_history_summary(assembled, history);
            self.inject_time_placeholders(assembled, None).await
        } else {
            Self::with_history_summary(self.system_prompt.read().await.clone(), history)
        };

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model_from_config = model_guard.as_ref().cloned();
        drop(model_guard);

        // If no model is configured, try to get it from the LLM runtime
        let model = if let Some(m) = model_from_config {
            m
        } else {
            // Try to get model name from the runtime
            let llm_guard = self.llm.read().await;
            if let Some(ref llm) = *llm_guard {
                llm.model_name().to_string()
            } else {
                // Ultimate fallback
                "qwen3.5:4b".to_string()
            }
        };

        // Get thinking control - priority: local setting > instance setting.
        let (thinking_enabled, thinking_effort) = self.effective_thinking_control(None).await;

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, _static_max_tokens) =
            self.get_effective_params().await;

        // Don't set max_tokens — let the LLM backend use its own default.
        // Backends (Ollama, OpenAI-compatible) have their own num_predict limits.

        let params = neomind_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            thinking_effort,
            max_context: None,
            // Chat path — no JSON constraint (structured extraction sets this).
            json_mode: None,
        };

        let system_msg = Message::system(&system_prompt);

        // Helper function to check if a message has empty content
        fn is_message_empty(msg: &Message) -> bool {
            use neomind_core::Content;
            match &msg.content {
                Content::Text(s) => s.is_empty(),
                Content::Parts(parts) => parts.is_empty(),
            }
        }

        // Build messages with history if provided, truncated to fit context window
        let mut history_reason = crate::agent::history_select::HistorySelectReason::Fits;
        let mut available_tokens_obs = 0usize;
        let mut history_before_trim: Vec<Message> = Vec::new();
        let mut selected_history: Vec<Message> = Vec::new();
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();
            history_before_trim = history_msgs.iter().map(|m| (*m).clone()).collect();

            let user_text = user_message.content.as_text();
            let (available_tokens, _prompt_budget) = self
                .compute_history_budget(
                    max_ctx,
                    &system_prompt,
                    &user_text,
                    history_msgs.len(),
                    has_tools,
                )
                .await;
            available_tokens_obs = available_tokens;

            let selected = self.select_history_for_prompt(
                &history_msgs,
                available_tokens,
                "Truncated conversation history to fit context window",
            );
            history_reason = selected.reason;
            selected_history = selected.messages.clone();
            msgs.extend(selected.messages);

            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
        };

        // Outer context window (streaming layer) may have evicted the newest
        // complete tool group under hard budget — the inner survives-check
        // below inspects the already-trimmed window and would vacuously pass.
        // Consume the flag only on tool-capable requests.
        let outer_window_dropped_latest_feedback = if has_tools {
            self.take_history_window_dropped_latest_feedback()
        } else {
            false
        };
        let attach_tools = crate::agent::history_select::tools_allowed_after_history_select(
            history_reason,
            &history_before_trim,
            &selected_history,
            has_tools,
            outer_window_dropped_latest_feedback,
        );
        if has_tools && !attach_tools {
            tracing::warn!(
                reason = ?history_reason,
                outer_window_dropped_latest_feedback,
                "no_feedback_degrade_no_tools: refusing tool-enabled assemble when latest tool feedback cannot be kept"
            );
        }

        // Get tool definitions — filter out "vision" when the user message
        // already contains images.  The model is multimodal and can analyse
        // them directly; the vision tool is only needed for fetching images
        // from URLs/files.  Mirrors chat_stream_internal_message.  The session
        // layer guarantees the model is multimodal whenever images are present.
        //
        // `messages.last()` is the current user message (system msg + history
        // are pushed before it); it is `None` only when the user message is
        // empty, in which case there are no images anyway.
        let has_user_images = messages.last().is_some_and(|m| m.has_images());
        let tools_input = if attach_tools {
            let tools = self.tool_definitions.read().await;
            let result = if tools.is_empty() {
                None
            } else if has_user_images {
                let filtered: Vec<_> = tools
                    .iter()
                    .filter(|t| t.name != "vision")
                    .cloned()
                    .collect();
                if filtered.is_empty() {
                    None
                } else {
                    Some(filtered)
                }
            } else {
                Some(tools.clone())
            };
            drop(tools);
            result
        } else {
            None
        };

        self.observe_send_boundary(
            &messages,
            &history_before_trim,
            available_tokens_obs,
            tools_input.as_deref(),
            "chat_internal_message",
        )
        .await;

        let input = LlmInput {
            messages,
            params,
            model: Some(model),
            stream: false,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let output = llm
            .generate(input)
            .await
            .map_err(|e| NeoMindError::Llm(e.to_string()))?;

        let duration = start.elapsed();
        let tokens_used = output
            .usage
            .map(|u| u.completion_tokens as usize)
            .unwrap_or_else(|| output.text.split_whitespace().count());

        Ok(ChatResponse {
            text: output.text,
            tokens_used,
            duration,
            finish_reason: format!("{:?}", output.finish_reason),
            thinking: output.thinking,
        })
    }

    /// Send a chat message with streaming response, with conversation history.
    /// Thinking is controlled by the instance/user thinking_enabled setting.
    pub async fn chat_stream_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.pin_turn_timestamp().await;
        self.chat_stream_internal(user_message, Some(history), true, None, None)
            .await
    }

    /// Same as `chat_stream_with_history` but with an explicit thinking override.
    ///
    /// Pass `Some(false)` to disable thinking (useful for post-tool-execution rounds
    /// where thinking models waste generation budget on reasoning instead of content).
    /// Pass `None` to use the default thinking setting.
    pub async fn chat_stream_with_history_thinking(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
        thinking_override: Option<bool>,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.pin_turn_timestamp().await;
        self.chat_stream_internal(user_message, Some(history), true, thinking_override, None)
            .await
    }

    /// Continue one user task without reclassifying tool output as a new intent.
    pub(crate) async fn chat_stream_tool_followup(
        &self,
        original_request: &str,
        round_context: &str,
        history: &[Message],
        thinking_override: Option<bool>,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        let message = format!(
            "Original user request:\n{original_request}\n\nTool-round context:\n{round_context}"
        );
        self.chat_stream_internal(
            message,
            Some(history),
            true,
            thinking_override,
            Some(original_request),
        )
        .await
    }

    /// One recovery request can use other tools without mutating the session's
    /// registered tool set. The next ordinary request has the full set again.
    pub(crate) async fn chat_stream_tool_followup_without_guides(
        &self,
        original_request: &str,
        round_context: &str,
        history: &[Message],
        thinking_override: Option<bool>,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.chat_stream_internal_scoped(
            format!("Original user request:\n{original_request}\n\nTool-round context:\n{round_context}"),
            Some(history), true, thinking_override, Some(original_request), true,
        ).await
    }

    /// Lightweight summary call: no tools, no thinking.
    /// Used as fallback when the ReAct loop ends without the model producing
    /// its own summary content. Without tool definitions, the model must
    /// output text instead of more tool calls.
    pub async fn chat_stream_summary(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.chat_stream_internal(user_message, Some(history), false, Some(false), None)
            .await
    }

    /// Final expression of a known task state without reclassifying an English
    /// executor directive as the user's intent or language.
    pub(crate) async fn chat_stream_summary_for_task(
        &self,
        original_request: &str,
        instruction: &str,
        history: &[Message],
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.chat_stream_internal(
            format!("Original user request:\n{original_request}\n\nFinal response instructions:\n{instruction}"),
            Some(history), false, Some(false), Some(original_request),
        ).await
    }

    /// Send a multimodal chat message (with images) with streaming response.
    /// This method accepts a Message directly, which can contain text and images.
    pub async fn chat_stream_multimodal_with_history(
        &self,
        user_message: Message, // Can contain text + images via Content::Parts
        history: &[Message],
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.pin_turn_timestamp().await;
        self.chat_stream_internal_message(user_message, Some(history), true, false)
            .await
    }

    /// Internal streaming chat implementation that accepts a Message directly (for multimodal).
    async fn chat_stream_internal_message(
        &self,
        user_message: Message,
        history: Option<&[Message]>,
        include_tools: bool,
        _restore_thinking: bool,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        // Check early before user_message is moved into the messages vec.
        let has_user_images = user_message.has_images();

        let model_arc = Arc::clone(&self.model);
        let max_ctx = self.max_context_length().await;
        let _prompt_budget = (max_ctx * 50) / 100;

        // Build system prompt (with or without tools based on phase).
        // A tool-less vision turn must send the session prompt (the VLM
        // contract) rather than the platform tool catalog. The catalog is
        // what pushed MiniCPM image embeddings out of alignment.
        let tools_disabled = self.tool_definitions.read().await.is_empty();
        let system_prompt = if has_user_images && tools_disabled {
            let custom = self.system_prompt.read().await.clone();
            let base = if custom.trim().is_empty() {
                self.build_base_system_prompt().await
            } else {
                custom
            };
            Self::with_history_summary(base, history)
        } else if include_tools {
            let assembled = self.assemble_system_prompt(None).await;
            let assembled = Self::with_history_summary(assembled, history);
            self.inject_time_placeholders(assembled, None).await
        } else {
            // Simple system prompt for non-tool calls
            Self::with_history_summary(
                "You are NeoMind, a helpful IoT assistant. Answer questions concisely and accurately."
                    .to_string(),
                history,
            )
        };

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model_from_config = model_guard.as_ref().cloned();
        drop(model_guard);

        // If no model is configured, try to get it from the LLM runtime
        let model = if let Some(m) = model_from_config {
            m
        } else {
            // Try to get model name from the runtime
            let llm_guard = self.llm.read().await;
            if let Some(ref llm) = *llm_guard {
                llm.model_name().to_string()
            } else {
                // Ultimate fallback
                "qwen3.5:4b".to_string()
            }
        };

        // Get thinking control - priority: local setting > instance setting.
        let (thinking_enabled, thinking_effort) = self.effective_thinking_control(None).await;

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            thinking_effort = ?thinking_effort,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal_message (multimodal)"
        );

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, _static_max_tokens) =
            self.get_effective_params().await;

        // Don't set max_tokens — let the LLM backend use its own default.
        // Backends (Ollama, OpenAI-compatible) have their own num_predict limits.

        let params = neomind_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            thinking_effort,
            max_context: None,
            // Chat path — no JSON constraint (structured extraction sets this).
            json_mode: None,
        };

        let system_msg = Message::system(&system_prompt);

        // Helper function to check if a message has empty content
        fn is_message_empty(msg: &Message) -> bool {
            use neomind_core::Content;
            match &msg.content {
                Content::Text(s) => s.is_empty(),
                Content::Parts(parts) => parts.is_empty(),
            }
        }

        // Build messages with history if provided, truncated to fit context window
        let mut history_reason = crate::agent::history_select::HistorySelectReason::Fits;
        let mut available_tokens_obs = 0usize;
        let mut history_before_trim: Vec<Message> = Vec::new();
        let mut selected_history: Vec<Message> = Vec::new();
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();
            history_before_trim = history_msgs.iter().map(|m| (*m).clone()).collect();

            let user_text = user_message.content.as_text();
            let (available_tokens, _prompt_budget) = self
                .compute_history_budget(
                    max_ctx,
                    &system_prompt,
                    &user_text,
                    history_msgs.len(),
                    include_tools,
                )
                .await;
            available_tokens_obs = available_tokens;

            let selected = self.select_history_for_prompt(
                &history_msgs,
                available_tokens,
                "Truncated conversation history to fit context window (multimodal stream)",
            );
            history_reason = selected.reason;
            selected_history = selected.messages.clone();
            msgs.extend(selected.messages);

            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
        };

        // Outer context window (streaming layer) may have evicted the newest
        // complete tool group under hard budget — the inner survives-check
        // below inspects the already-trimmed window and would vacuously pass.
        // Consume the flag only on tool-capable requests.
        let outer_window_dropped_latest_feedback = if include_tools {
            self.take_history_window_dropped_latest_feedback()
        } else {
            false
        };
        let attach_tools = crate::agent::history_select::tools_allowed_after_history_select(
            history_reason,
            &history_before_trim,
            &selected_history,
            include_tools,
            outer_window_dropped_latest_feedback,
        );
        if include_tools && !attach_tools {
            tracing::warn!(
                reason = ?history_reason,
                outer_window_dropped_latest_feedback,
                "no_feedback_degrade_no_tools: refusing tool-enabled assemble when latest tool feedback cannot be kept"
            );
        }

        // Get tool definitions — filter out "vision" when the user message already
        // contains images.  The model is multimodal and can analyse them directly;
        // the vision tool is only needed for fetching images from URLs/files.
        let tools = self.tool_definitions.read().await;
        let tools_input = if !attach_tools || tools.is_empty() {
            None
        } else if has_user_images {
            let filtered: Vec<_> = tools
                .iter()
                .filter(|t| t.name != "vision")
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(filtered)
            }
        } else {
            Some(tools.clone())
        };
        drop(tools);

        self.observe_send_boundary(
            &messages,
            &history_before_trim,
            available_tokens_obs,
            tools_input.as_deref(),
            "chat_stream_internal_message",
        )
        .await;

        let input = LlmInput {
            messages,
            params,
            model: Some(model),
            stream: true,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let stream = llm
            .generate_stream(input)
            .await
            .map_err(|e| NeoMindError::Llm(e.to_string()))?;

        // Acquire permit for concurrency limiting and wrap stream
        let permit = self.limiter.acquire().await;
        let fg_guard = ForegroundChatGuard::acquire();
        let wrapped_stream = PermitStream::new(stream, permit);
        let token_tracker = self.last_prompt_tokens.clone();

        // Convert stream
        Ok(Box::pin(async_stream::stream! {
            let _fg_guard = fg_guard; // counts as foreground until the stream ends
            let mut stream = wrapped_stream;
            while let Some(result) = futures::StreamExt::next(&mut stream).await {
                match result {
                    Ok((content, is_thinking)) => {
                        let (clean, is_thinking, tokens) = extract_token_marker(&content, is_thinking);
                        if let Some(t) = tokens {
                            // Record invalid (0/MAX) too — End must not look like
                            // missing usage. ContextUsage UI gates on plausible.
                            *token_tracker.lock().await = Some(t);
                        }
                        if !clean.is_empty() {
                            yield Ok((clean, is_thinking));
                        }
                    }
                    Err(e) => yield Err(NeoMindError::Llm(e.to_string())),
                }
            }
        }))
    }

    /// Internal streaming chat implementation.
    async fn chat_stream_internal(
        &self,
        user_message: impl Into<String>,
        history: Option<&[Message]>,
        include_tools: bool,
        thinking_override: Option<bool>,
        task_message: Option<&str>,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        self.chat_stream_internal_scoped(
            user_message,
            history,
            include_tools,
            thinking_override,
            task_message,
            false,
        )
        .await
    }

    /// Internal streaming chat implementation.
    async fn chat_stream_internal_scoped(
        &self,
        user_message: impl Into<String>,
        history: Option<&[Message]>,
        include_tools: bool,
        thinking_override: Option<bool>,
        task_message: Option<&str>,
        exclude_skill: bool,
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
        let user_message = user_message.into();

        let model_arc = Arc::clone(&self.model);

        // Check model context capacity for adaptive prompt sizing
        let max_ctx = self.max_context_length().await;
        // Use more conservative budget for small contexts
        let prompt_budget = if max_ctx < 8192 {
            (max_ctx * 50) / 100
        } else if max_ctx < 16384 {
            (max_ctx * 60) / 100
        } else {
            (max_ctx * 70) / 100
        };

        let use_compact_prompt = prompt_budget < 3000; // < ~3000 tokens → use compact prompt
        let skip_history = prompt_budget < 1500; // < ~1500 tokens → skip history entirely
        let skip_tools = prompt_budget < 2000; // < ~2000 tokens → no tool definitions

        tracing::info!(
            max_ctx = max_ctx,
            prompt_budget = prompt_budget,
            use_compact = use_compact_prompt,
            skip_history = skip_history,
            skip_tools = skip_tools,
            "Adaptive prompt sizing for chat_stream_internal"
        );

        // Build system prompt (with or without tools based on phase)
        let system_prompt = if include_tools {
            if use_compact_prompt {
                // Compact prompt for small context models (< 4096)
                let mut compact = Self::with_history_summary(
                    "You are NeoMind, a helpful IoT assistant. Answer questions concisely. \
                     You can help with device management, data queries, and automation rules. \
                     Keep responses brief."
                        .to_string(),
                    history,
                );
                if exclude_skill {
                    compact.push_str("\nFor this request only, the skill tool is unavailable. Use remaining tools within the original user's scope, or answer from verified results. Do not invent missing prerequisites because guide lookup failed.");
                }
                compact
            } else {
                let mut assembled = self
                    .assemble_system_prompt(Some(task_message.unwrap_or(&user_message)))
                    .await;
                assembled = Self::with_history_summary(assembled, history);
                if exclude_skill {
                    assembled.push_str("\nFor this request only, the skill tool is unavailable. Use remaining tools within the original user's scope, or answer from verified results. Do not invent missing prerequisites because guide lookup failed.");
                }
                self.inject_time_placeholders(assembled, None).await
            }
        } else {
            // Simple system prompt for non-tool calls
            let mut simple = Self::with_history_summary(
                "You are NeoMind, a helpful IoT assistant. Answer questions concisely and accurately."
                    .to_string(),
                history,
            );
            if exclude_skill {
                simple.push_str("\nFor this request only, the skill tool is unavailable. Use remaining tools within the original user's scope, or answer from verified results. Do not invent missing prerequisites because guide lookup failed.");
            }
            simple
        };

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model_from_config = model_guard.as_ref().cloned();
        drop(model_guard);

        // If no model is configured, try to get it from the LLM runtime
        let model = if let Some(m) = model_from_config {
            m
        } else {
            // Try to get model name from the runtime
            let llm_guard = self.llm.read().await;
            if let Some(ref llm) = *llm_guard {
                llm.model_name().to_string()
            } else {
                // Ultimate fallback
                "qwen3.5:4b".to_string()
            }
        };

        // Get thinking control - priority: thinking_override > local setting > instance setting.
        // (Explicit per-call override takes highest priority; otherwise resolve
        // local/instance — effort preferred over the legacy bool. Integral-thinking
        // models, e.g. LFM2.5, ignore the override — their thinking can't be off.)
        let (thinking_enabled, thinking_effort) =
            self.effective_thinking_control(thinking_override).await;

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            thinking_effort = ?thinking_effort,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal"
        );

        // Safety: auto-disable thinking for very large prompts.
        // Small thinking models (2B) with large prompts (20k+ tokens) tend to
        // consume all num_predict tokens on thinking alone, producing no content.
        // Observed: qwen3.5:2b with prompt_eval=32259, eval=32768, content=0.
        let (thinking_enabled, thinking_effort) = if thinking_enabled == Some(true) {
            // Size the prompt with the same estimator the budget math uses, so the
            // guard reflects real token density — not a bytes*0.8 approximation
            // that over-counted English (~3×) and tripped the guard far too
            // eagerly on text-heavy prompts.
            let estimated_tokens = crate::agent::tokenizer::estimate_prompt_tokens_with_image_cost(
                history.unwrap_or(&[]),
                &system_prompt,
                &user_message,
                // Price images by the backend's REAL window: on an 8–16K
                // model a 1024/image estimate under-counts 2K-token vision
                // payloads 2×, exactly where two images eat a quarter of a
                // budget the guard believes is still roomy.
                crate::agent::tokenizer::image_token_estimate(self.max_context_length().await),
            );

            if estimated_tokens > long_prompt_thinking_threshold(self.max_context_length().await) {
                tracing::info!(
                    estimated_tokens,
                    hist_messages = history.map(|h| h.len()).unwrap_or(0),
                    "Requesting dynamic thinking disable for large prompt"
                );
                self.effective_thinking_control(Some(false)).await
            } else {
                (thinking_enabled, thinking_effort)
            }
        } else {
            (thinking_enabled, thinking_effort)
        };

        tracing::debug!(thinking_enabled = ?thinking_enabled,
            thinking_effort = ?thinking_effort,
            "Final streaming thinking policy after dynamic budget control");

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, _static_max_tokens) =
            self.get_effective_params().await;

        // Don't set max_tokens — let the LLM backend use its own default.
        // Setting a generation budget caused LLM to stop mid-tool-call JSON,
        // producing incomplete output that gets silently discarded.
        // Backends (Ollama, OpenAI-compatible) have their own num_predict limits.

        let params = neomind_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            thinking_effort,
            max_context: None,
            // Chat path — no JSON constraint (structured extraction sets this).
            json_mode: None,
        };

        let system_msg = Message::system(&system_prompt);
        let user_msg = Message::user(&user_message);

        tracing::info!(
            max_ctx,
            prompt_budget,
            "Prompt budget for chat_stream_internal (max_tokens delegated to backend)"
        );

        // Prepare fallback strategies for context overflow retry
        // Strategy 1: Compact fallback - keep user message + last tool round (if any)
        // Strategy 2: Minimal fallback - system prompt + user message only
        let compact_fallback_system = Self::with_history_summary(
            "You are NeoMind IoT assistant. Summarize results concisely based on the tool results shown.".to_string(),
            history,
        );
        let compact_fallback_messages = if let Some(hist) = history {
            let mut compact = vec![Message::system(&compact_fallback_system)];
            // Keep only the last few messages (most recent tool round + user message)
            let non_system: Vec<&Message> = hist
                .iter()
                .filter(|m| m.role != neomind_core::MessageRole::System)
                .collect();
            // Take last 4 messages max (covers: user → assistant tool_call → tool result → assistant response)
            let keep = non_system.len().saturating_sub(4);
            for msg in &non_system[keep..] {
                compact.push((*msg).clone());
            }
            // Add current user message if not already present
            compact.push(Message::user(&user_message));
            compact
        } else {
            vec![
                Message::system(&compact_fallback_system),
                Message::user(&user_message),
            ]
        };
        // Strategy 2: Minimal fallback
        let minimal_fallback_messages = vec![
            Message::system(&compact_fallback_system),
            Message::user(&user_message),
        ];

        // Build messages with history if provided, truncated to fit context window
        let mut history_reason = crate::agent::history_select::HistorySelectReason::Fits;
        let mut available_tokens_obs = 0usize;
        let mut history_before_trim: Vec<Message> = Vec::new();
        let mut selected_history: Vec<Message> = Vec::new();
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            // Collect non-system history messages
            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();
            history_before_trim = history_msgs.iter().map(|m| (*m).clone()).collect();

            // Use token-based budget calculation accounting for tools and template overhead
            let (available_tokens, _prompt_budget) = self
                .compute_history_budget(
                    max_ctx,
                    &system_prompt,
                    &user_message,
                    history_msgs.len(),
                    include_tools,
                )
                .await;
            available_tokens_obs = available_tokens;

            // Group-aware selection: recent tool-call/result units stay
            // atomic; available_tokens==0 keeps nothing (never force-insert
            // the earliest user message — that produced message_count=3
            // re-query loops after large tool returns ate the soft budget).
            let selected = self.select_history_for_prompt(
                &history_msgs,
                available_tokens,
                "Truncated conversation history to fit context window (group-aware)",
            );
            history_reason = selected.reason;
            selected_history = selected.messages.clone();
            msgs.extend(selected.messages);

            msgs.push(user_msg);
            msgs
        } else {
            vec![system_msg, user_msg]
        };

        // Outer context window (streaming layer) may have evicted the newest
        // complete tool group under hard budget — the inner survives-check
        // below inspects the already-trimmed window and would vacuously pass.
        // Consume the flag only on tool-capable requests.
        let outer_window_dropped_latest_feedback = if include_tools {
            self.take_history_window_dropped_latest_feedback()
        } else {
            false
        };
        let attach_tools = crate::agent::history_select::tools_allowed_after_history_select(
            history_reason,
            &history_before_trim,
            &selected_history,
            include_tools,
            outer_window_dropped_latest_feedback,
        );
        if include_tools && !attach_tools {
            tracing::warn!(
                reason = ?history_reason,
                outer_window_dropped_latest_feedback,
                "no_feedback_degrade_no_tools: refusing tool-enabled assemble when latest tool feedback cannot be kept"
            );
        }

        // Get tool definitions
        let tools = self.tool_definitions.read().await;
        let tools_input = if !attach_tools {
            None
        } else {
            let available: Vec<_> = tools
                .iter()
                .filter(|tool| !exclude_skill || tool.name != "skill")
                .cloned()
                .collect();
            if available.is_empty() {
                None
            } else {
                Some(available)
            }
        };
        drop(tools);

        self.observe_send_boundary(
            &messages,
            &history_before_trim,
            available_tokens_obs,
            tools_input.as_deref(),
            "chat_stream_internal_scoped",
        )
        .await;

        let input = LlmInput {
            messages,
            params,
            model: Some(model.clone()),
            stream: true,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let stream = llm
            .generate_stream(input)
            .await
            .map_err(|e| NeoMindError::Llm(e.to_string()))?;

        // Acquire permit for concurrency limiting and wrap stream
        let permit = self.limiter.acquire().await;
        let fg_guard = ForegroundChatGuard::acquire();
        let wrapped_stream = PermitStream::new(stream, permit);

        let retry_thinking = self.effective_thinking_control(Some(false)).await;
        // Prepare retry data for tiered context overflow retry
        let llm_retry = llm.clone();
        let limiter_retry = self.limiter.clone();
        let token_tracker = self.last_prompt_tokens.clone();

        // Convert stream with tiered context overflow retry
        Ok(Box::pin(async_stream::stream! {
            let _fg_guard = fg_guard; // counts as foreground until the stream ends
            let mut stream = wrapped_stream;
            let mut retry_stage: u8 = 0; // 0=initial, 1=compact retry, 2=minimal retry
            while let Some(result) = futures::StreamExt::next(&mut stream).await {
                match result {
                    Ok((content, is_thinking)) => {
                        let (clean, is_thinking, tokens) = extract_token_marker(&content, is_thinking);
                        if let Some(t) = tokens {
                            *token_tracker.lock().await = Some(t);
                        }
                        if !clean.is_empty() {
                            yield Ok((clean, is_thinking));
                        }
                    }
                    Err(LlmError::ContextOverflow { prompt_tokens, max_context }) => {
                        retry_stage += 1;
                        if retry_stage <= 2 {
                            let (retry_messages, retry_label) = if retry_stage == 1 {
                                // Strategy 1: Compact - keep recent tool round
                                (compact_fallback_messages.clone(), "compact (recent tool round)")
                            } else {
                                // Strategy 2: Minimal - system + user only
                                (minimal_fallback_messages.clone(), "minimal (no history)")
                            };
                            tracing::warn!(
                                prompt_tokens,
                                max_context,
                                retry_stage,
                                %retry_label,
                                "Context overflow, retrying with {} strategy", retry_label
                            );
                            drop(stream);
                            tracing::debug!(reason = "context_overflow_recovery",
                                enabled = ?retry_thinking.0, effort = ?retry_thinking.1,
                                "Applying recovery thinking policy");
                            let retry_input = LlmInput {
                                messages: retry_messages,
                                params: neomind_core::llm::backend::GenerationParams {
                                    temperature: Some(eff_temp),
                                    top_p: Some(eff_top_p),
                                    top_k: Some(eff_top_k as u32),
                                    max_tokens: None,
                                    stop: None,
                                    frequency_penalty: None,
                                    presence_penalty: None,
                                    thinking_enabled: retry_thinking.0,
                                    thinking_effort: retry_thinking.1,
                                    max_context: None,
                                    json_mode: None,
                                },
                                model: Some(model.clone()),
                                stream: true,
                                tools: None, // Strip tools from retry to save context
                            };
                            let permit2 = limiter_retry.acquire().await;
                            match llm_retry.generate_stream(retry_input).await {
                                Ok(fallback_stream) => {
                                    stream = PermitStream::new(fallback_stream, permit2);
                                    continue;
                                }
                                Err(e) => {
                                    yield Err(NeoMindError::Llm(format!(
                                        "Context overflow retry ({}): {}", retry_label, e
                                    )));
                                    return;
                                }
                            }
                        } else {
                            yield Err(NeoMindError::Llm(format!(
                                "Context exceeds model limit ({} > {}) after all retries, please shorten the conversation",
                                prompt_tokens, max_context
                            )));
                            return;
                        }
                    }
                    Err(e) => yield Err(NeoMindError::Llm(e.to_string())),
                }
            }
        }))
    }
}

const SKILL_SECTION_SEP: &str = "#section:";

fn format_skill_dedup_display_key(skill_id: &str, section: Option<&str>) -> String {
    match section {
        Some(section) if !section.is_empty() => format!("{skill_id}{SKILL_SECTION_SEP}{section}"),
        _ => skill_id.to_string(),
    }
}

/// Normalize `(skill_id, content)` into a structured `(skill_id, section_opt)`
/// dedup key. Honors either an already-encoded `id#section:title` string or a
/// JSON payload carrying `"section": "..."`, so loading an overview followed by
/// a section with the same base `skill_id` never collides.
fn normalize_skill_dedup_key(skill_id: &str, content: &str) -> (String, Option<String>) {
    if let Some((base, sec)) = skill_id.split_once(SKILL_SECTION_SEP) {
        if !sec.is_empty() {
            return (base.to_string(), Some(sec.to_string()));
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(sec) = v
            .get("section")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
        {
            return (skill_id.to_string(), Some(sec.to_string()));
        }
    }
    (skill_id.to_string(), None)
}

/// Extract a structured `(skill_id, section_opt)` dedup key from a `skill` tool
/// result body.
pub(crate) fn extract_skill_key_from_result(result_str: &str) -> (String, Option<String>) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(result_str) {
        if let Some(id) = v
            .get("id")
            .or_else(|| v.get("skill_id"))
            .and_then(|i| i.as_str())
        {
            let section = v
                .get("section")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            return (id.to_string(), section);
        }
        // `search` returns an array of entries — distinct skill loads shouldn't collapse,
        // so derive a stable key from the payload itself.
        if let Some(arr) = v.as_array() {
            if let Some(first) = arr.first() {
                if let Some(id) = first.get("id").and_then(|i| i.as_str()) {
                    return (format!("search:{}", id), None);
                }
            }
            return (format!("search:len:{}", result_str.len()), None);
        }
    }
    // Last-resort stable fallback: hash the content so repeated identical payloads dedup.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    result_str.hash(&mut h);
    (format!("hash:{:x}", h.finish()), None)
}

/// Extract a stable skill id from a `skill` tool result body for dedup purposes.
///
/// Skill tool `load` results are JSON like `{"id":"rule-management","name":"...","guide":"..."}`.
/// We pull `id` (or `skill_id`) out of that. If the payload isn't JSON or lacks an id
/// (e.g. `search` results, error payloads), fall back to a content-derived key so the call
/// still dedups repeated identical payloads without accidentally collapsing distinct skills.
#[cfg(test)]
pub(crate) fn extract_skill_id_from_result(result_str: &str) -> String {
    let (id, section) = extract_skill_key_from_result(result_str);
    format_skill_dedup_display_key(&id, section.as_deref())
}

/// Extract in-band token usage marker from a stream chunk.
/// Returns (clean_content, is_thinking, extracted_prompt_tokens).
/// The marker format is `\n__NEOMIND_TOKEN_PROMPT:NN__`.
///
/// Preserves meter kinds: `Some(0)` / `Some(u32::MAX)` mean invalid
/// (reported but unusable); `None` means usage was never reported. Do not
/// collapse invalid → None — that incorrectly opens truncation auto-compact.
fn extract_token_marker(content: &str, is_thinking: bool) -> (String, bool, Option<u32>) {
    if is_thinking {
        return (content.to_string(), is_thinking, None);
    }
    if let Some(start) = content.find("__NEOMIND_TOKEN_PROMPT:") {
        let after = &content[start + "__NEOMIND_TOKEN_PROMPT:".len()..];
        if let Some(end) = after.find("__") {
            if let Ok(tokens) = after[..end].parse::<u32>() {
                let clean = format!(
                    "{}{}",
                    &content[..start],
                    &content[start + "__NEOMIND_TOKEN_PROMPT:".len() + end + 2..]
                );
                return (clean.trim().to_string(), is_thinking, Some(tokens));
            }
        }
    }
    (content.to_string(), is_thinking, None)
}

impl Default for LlmInterface {
    fn default() -> Self {
        Self::new(ChatConfig::default())
    }
}

/// Configuration for LLM chat operations.
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// Model identifier.
    pub model: String,
    /// Temperature (0.0 to 2.0).
    pub temperature: f32,
    /// Top-p sampling.
    pub top_p: f32,
    /// Top-k sampling (0 = disabled).
    pub top_k: usize,
    /// Maximum tokens to generate.
    pub max_tokens: usize,
    /// Maximum concurrent LLM requests (default: 3).
    pub concurrent_limit: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        let ad = neomind_storage::AgentDefaults::get();
        Self {
            model: "qwen3.5:4b".to_string(),
            temperature: std::env::var(agent_env_vars::TEMPERATURE)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(ad.default_temperature),
            top_p: std::env::var(agent_env_vars::TOP_P)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(ad.default_top_p),
            top_k: 40,
            max_tokens: agent_env_vars::max_tokens(),
            concurrent_limit: agent_env_vars::concurrent_limit(),
        }
    }
}

/// Response from a chat request.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// Generated text.
    pub text: String,
    /// Number of tokens generated.
    pub tokens_used: usize,
    /// Time taken to generate.
    pub duration: std::time::Duration,
    /// Finish reason.
    pub finish_reason: String,
    /// Thinking content (if the model generated any).
    pub thinking: Option<String>,
}

#[cfg(all(test, feature = "test-utils"))]
#[path = "llm_request_tests.rs"]
mod request_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recent_skill_note_survives_turn_boundary_and_stays_honest() {
        let interface = LlmInterface::new(ChatConfig::default());
        let outcome = interface
            .set_skill_context_outcome("rule-management".into(), "# guide body".repeat(20))
            .await;
        assert_eq!(outcome, SkillBodyAppendOutcome::Full);

        // While the body is present the note is rendered too, marking the
        // guide as retained (state must be visible in the same request).
        let prompt = interface.assemble_system_prompt(None).await;
        assert!(prompt.contains("# guide body"));
        assert!(
            prompt.contains("rule-management (retained above)"),
            "{prompt}"
        );

        // Turn boundary: body cleared, usage record kept, state degrades to
        // not-in-context.
        interface.clear_skill_context().await;
        assert!(interface.get_skill_context().await.is_none());
        let prompt = interface.assemble_system_prompt(None).await;
        assert!(
            prompt.contains("Recently Used Guides"),
            "usage note must appear after the turn boundary"
        );
        assert!(
            prompt.contains("rule-management (not in context)"),
            "cleared guides must be marked not-in-context, never 'retained'"
        );
        assert!(!prompt.contains("retained above"), "{prompt}");
        assert!(!prompt.contains("# guide body"), "body itself must be gone");

        // Reload in the new turn: body back AND the record says retained.
        interface
            .set_skill_context_outcome("rule-management".into(), "# guide body".repeat(20))
            .await;
        assert!(interface.get_skill_context().await.is_some());
        let prompt = interface.assemble_system_prompt(None).await;
        assert!(prompt.contains("# guide body"));
        assert!(prompt.contains("rule-management (retained above)"));
    }

    /// Review P1-2 (request-level): load A full, then exhaust the budget so B
    /// is skipped — the SAME assembled request must show A's body AND B
    /// marked "not in context".
    #[tokio::test]
    async fn skipped_load_is_visible_in_the_same_request_as_present_bodies() {
        let interface = LlmInterface::new(ChatConfig::default());
        let budget = skill_bodies_token_budget(interface.max_context_length().await);

        // A: well under budget.
        let body_a = "A-SENTINEL ".repeat(budget / 8);
        let out_a = interface
            .set_skill_context_outcome("guide-a".into(), body_a)
            .await;
        assert_eq!(out_a, SkillBodyAppendOutcome::Full);

        // B: fills the remainder (Truncated or Full — both fine).
        let body_b = "B-SENTINEL ".repeat(budget);
        interface
            .set_skill_context_outcome("guide-b".into(), body_b)
            .await;

        // C: tiny load with (near-)zero remaining budget -> Skipped.
        let out_c = interface
            .set_skill_context_outcome("guide-c".into(), "C-SENTINEL tiny".to_string())
            .await;
        if out_c == SkillBodyAppendOutcome::Full {
            // Budget math landed differently — force exhaustion with one more
            // oversized load and use D as the skipped one.
            let out_d = interface
                .set_skill_context_outcome("guide-d".into(), "D-SENTINEL tiny".to_string())
                .await;
            assert_ne!(
                out_d,
                SkillBodyAppendOutcome::Full,
                "test needs a skipped load; budget too large"
            );
        }

        let prompt = interface.assemble_system_prompt(None).await;
        assert!(prompt.contains("A-SENTINEL"), "A's body must be in context");
        // Whichever guide got skipped must be named as not-in-context in the
        // SAME request — the state reaches the model, not just the logs.
        assert!(
            prompt.contains("(not in context)"),
            "skipped guide must be listed as not in context: {prompt}"
        );
    }

    /// Deterministic filler with an exact estimated token count (review-2:
    /// budget fixtures must not depend on estimate rounding).
    fn filler_of_tokens(tokens: usize, mark: &str) -> String {
        use crate::agent::tokenizer::{estimate_tokens, truncate_to_tokens};
        let long = format!("{mark}{} ", "x".repeat(tokens * 8));
        let cut = truncate_to_tokens(&long, tokens);
        assert_eq!(estimate_tokens(&cut), tokens, "filler token count");
        cut
    }

    /// Review P1-1 (review-2 split): Full / Partial / Skipped states each
    /// survive a duplicate load — no upgrade, no body growth.
    #[tokio::test]
    async fn duplicate_load_keeps_full_partial_and_skipped_states_exactly() {
        let budget = skill_bodies_token_budget(
            LlmInterface::new(ChatConfig::default())
                .max_context_length()
                .await,
        );

        // ---- Full ----
        let f = LlmInterface::new(ChatConfig::default());
        let body = filler_of_tokens(budget / 4, "F-SENT");
        assert_eq!(
            f.set_skill_context_outcome("g-full".into(), body.clone())
                .await,
            SkillBodyAppendOutcome::Full
        );
        assert_eq!(
            f.set_skill_context_outcome("g-full".into(), body.clone())
                .await,
            SkillBodyAppendOutcome::AlreadyPresent
        );
        let prompt = f.assemble_system_prompt(None).await;
        assert!(prompt.contains("g-full (retained above)"), "{prompt}");
        assert!(prompt.matches("F-SENT").count() >= 1);

        // ---- Partial: filler leaves exactly 100 tokens, guide-p needs more ----
        let p = LlmInterface::new(ChatConfig::default());
        let pre = filler_of_tokens(budget - 100, "P-PRE");
        assert_eq!(
            p.set_skill_context_outcome("g-pre".into(), pre).await,
            SkillBodyAppendOutcome::Full
        );
        let big = filler_of_tokens(budget, "P-BIG");
        assert_eq!(
            p.set_skill_context_outcome("g-partial".into(), big.clone())
                .await,
            SkillBodyAppendOutcome::Truncated
        );
        let before = p.get_skill_context().await.unwrap();
        assert_eq!(
            p.set_skill_context_outcome("g-partial".into(), big.clone())
                .await,
            SkillBodyAppendOutcome::AlreadyPresent
        );
        assert_eq!(
            p.get_skill_context().await.unwrap(),
            before,
            "duplicate partial load must not grow the body"
        );
        let prompt = p.assemble_system_prompt(None).await;
        assert!(
            prompt.contains("g-partial (partially retained (truncated))"),
            "{prompt}"
        );

        // ---- Skipped: budget exactly consumed, then a tiny load ----
        let s = LlmInterface::new(ChatConfig::default());
        let fill = filler_of_tokens(budget, "S-FILL");
        assert_eq!(
            s.set_skill_context_outcome("g-fill".into(), fill).await,
            SkillBodyAppendOutcome::Full
        );
        let tiny = "S-TINY body".to_string();
        assert_eq!(
            s.set_skill_context_outcome("g-skipped".into(), tiny.clone())
                .await,
            SkillBodyAppendOutcome::Skipped
        );
        assert_eq!(
            s.set_skill_context_outcome("g-skipped".into(), tiny).await,
            SkillBodyAppendOutcome::AlreadyPresent
        );
        let prompt = s.assemble_system_prompt(None).await;
        assert!(prompt.contains("g-skipped (not in context)"), "{prompt}");
        assert!(!prompt.contains("S-TINY"), "{prompt}");
        assert!(
            !prompt.contains("g-skipped (retained above)")
                && !prompt.contains("g-skipped (partially retained"),
            "skipped must not upgrade: {prompt}"
        );
    }

    #[tokio::test]
    async fn duplicate_load_within_turn_reports_already_present_without_growth() {
        let interface = LlmInterface::new(ChatConfig::default());
        // One unique sentinel per body so counting proves (non-)duplication.
        let body = format!("GUIDE-SENTINEL {}", "正文".repeat(40));
        let first = interface
            .set_skill_context_outcome("agent-management".into(), body.clone())
            .await;
        let second = interface
            .set_skill_context_outcome("agent-management".into(), body.clone())
            .await;
        assert_eq!(first, SkillBodyAppendOutcome::Full);
        assert_eq!(second, SkillBodyAppendOutcome::AlreadyPresent);
        let injected = interface.get_skill_context().await.unwrap();
        assert_eq!(
            injected.matches("GUIDE-SENTINEL").count(),
            1,
            "duplicate load must not double the body"
        );
    }

    #[test]
    fn touch_recent_bounds_the_ring_and_refreshes_order() {
        let mut recent = std::collections::VecDeque::new();
        for i in 0..(RECENT_SKILL_MEMORY + 3) {
            touch_recent(&mut recent, &format!("skill-{i}"), RecentSkillState::Full);
        }
        assert_eq!(recent.len(), RECENT_SKILL_MEMORY);
        assert_eq!(recent.front().unwrap().key, "skill-3");
        // Re-touching an old entry moves it to the back with updated state.
        touch_recent(&mut recent, "skill-3", RecentSkillState::Skipped);
        assert_eq!(recent.back().unwrap().key, "skill-3");
        assert_eq!(recent.back().unwrap().state, RecentSkillState::Skipped);
    }

    #[test]
    fn extract_token_marker_preserves_implausible_meters() {
        let (clean, _, meter) = extract_token_marker("\n__NEOMIND_TOKEN_PROMPT:0__", false);
        assert!(clean.is_empty());
        assert_eq!(meter, Some(0));
        let (clean, _, meter) =
            extract_token_marker(&format!("\n__NEOMIND_TOKEN_PROMPT:{}__", u32::MAX), false);
        assert!(clean.is_empty());
        assert_eq!(meter, Some(u32::MAX));
        let (clean, _, meter) =
            extract_token_marker("answer\n__NEOMIND_TOKEN_PROMPT:1234__", false);
        assert_eq!(clean, "answer");
        assert_eq!(meter, Some(1234));
        // Composition with the bill-first gate: invalid ≠ missing.
        assert!(!neomind_core::plausible_prompt_token_meter(0));
        assert!(!neomind_core::plausible_prompt_token_meter(u32::MAX));
    }

    #[test]
    fn test_chat_config_default() {
        let config = ChatConfig::default();
        assert_eq!(config.model, "qwen3.5:4b");
        assert_eq!(config.temperature, 0.3);
        assert_eq!(config.top_p, 0.7);
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.concurrent_limit, DEFAULT_CONCURRENT_LIMIT);
    }

    #[test]
    fn test_llm_interface_new() {
        let config = ChatConfig {
            model: "test-model".to_string(),
            temperature: 0.5,
            top_p: 0.9,
            top_k: 0,
            max_tokens: 2048,
            concurrent_limit: 2,
        };
        let interface = LlmInterface::new(config);
        assert!(!interface.uses_instance_manager());
        assert_eq!(interface.available_permits(), 2);
    }

    #[test]
    fn test_llm_interface_with_system_prompt() {
        let config = ChatConfig::default();
        let _interface = LlmInterface::new(config).with_system_prompt("You are a test assistant.");
    }

    #[test]
    fn test_extract_skill_id_from_result_load_payload() {
        let body = r#"{"id":"rule-management","name":"Rule Management","guide":"..."}"#;
        assert_eq!(extract_skill_id_from_result(body), "rule-management");
    }

    #[test]
    fn test_extract_skill_id_from_result_skill_id_field() {
        let body = r#"{"skill_id":"device-onboarding","guide":"..."}"#;
        assert_eq!(extract_skill_id_from_result(body), "device-onboarding");
    }

    #[test]
    fn test_extract_skill_id_from_result_search_array() {
        let body = r#"[{"id":"rule-management","name":"..."},{"id":"agent-management"}]"#;
        assert_eq!(extract_skill_id_from_result(body), "search:rule-management");
    }

    #[test]
    fn test_extract_skill_id_from_result_non_json_is_stable() {
        let body = "some plain text error";
        let key1 = extract_skill_id_from_result(body);
        let key2 = extract_skill_id_from_result(body);
        assert_eq!(
            key1, key2,
            "fallback must be deterministic for identical input"
        );
        assert!(key1.starts_with("hash:"));
    }

    #[tokio::test]
    async fn overview_and_reference_survive_together_without_duplicate_sections() {
        let interface = LlmInterface::new(ChatConfig::default());
        for payload in [
            r#"{"id":"rule-management","guide":"OVERVIEW_MARKER"}"#,
            r#"{"id":"rule-management","section":"Syntax","guide":"REFERENCE_MARKER"}"#,
            r#"{"id":"rule-management","section":"Syntax","guide":"REFERENCE_MARKER"}"#,
        ] {
            interface
                .set_skill_context(extract_skill_id_from_result(payload), payload.into())
                .await;
        }
        let body = interface.get_skill_context().await.unwrap();
        assert_eq!(body.matches("OVERVIEW_MARKER").count(), 1);
        assert_eq!(body.matches("REFERENCE_MARKER").count(), 1);
        interface.clear_skill_context().await;
        assert!(interface.get_skill_context().await.is_none());
    }

    #[tokio::test]
    async fn test_set_skill_context_dedups_repeated_loads() {
        let interface = LlmInterface::new(ChatConfig::default());
        let payload = r#"{"id":"rule-management","name":"Rule","guide":"RULE GUIDE"}"#;

        // First load: body is appended.
        interface
            .set_skill_context(extract_skill_id_from_result(payload), payload.to_string())
            .await;
        let after_first = interface.get_skill_context().await.expect("body present");
        assert!(after_first.contains("RULE GUIDE"));

        // Second identical load: should be a no-op (dedup).
        interface
            .set_skill_context(extract_skill_id_from_result(payload), payload.to_string())
            .await;
        let after_second = interface.get_skill_context().await.expect("body present");
        assert_eq!(
            after_first.matches("RULE GUIDE").count(),
            after_second.matches("RULE GUIDE").count(),
            "duplicate load must not re-append the body"
        );

        // A different skill id still accumulates.
        let payload2 = r#"{"id":"device-onboarding","name":"Dev","guide":"DEVICE GUIDE"}"#;
        interface
            .set_skill_context(extract_skill_id_from_result(payload2), payload2.to_string())
            .await;
        let after_third = interface.get_skill_context().await.expect("body present");
        assert!(after_third.contains("RULE GUIDE"));
        assert!(after_third.contains("DEVICE GUIDE"));

        // clear_skill_context resets everything.
        interface.clear_skill_context().await;
        assert!(interface.get_skill_context().await.is_none());
        // After clear, the same id can load again (fresh turn).
        interface
            .set_skill_context(extract_skill_id_from_result(payload), payload.to_string())
            .await;
        assert!(interface
            .get_skill_context()
            .await
            .expect("body present after clear")
            .contains("RULE GUIDE"));
    }

    #[test]
    fn skill_bodies_token_budget_scales_and_clamps() {
        assert_eq!(skill_bodies_token_budget(4_096), 2_000);
        assert_eq!(skill_bodies_token_budget(8_000), 2_000);
        assert_eq!(skill_bodies_token_budget(32_768), 4_096);
        assert_eq!(skill_bodies_token_budget(65_536), 6_000);
        assert_eq!(skill_bodies_token_budget(128_000), 6_000);
    }

    #[test]
    fn plan_skill_body_append_keeps_first_full_then_caps_total() {
        use crate::agent::tokenizer::estimate_tokens;

        let budget = 200;
        let first = "ALPHA ".repeat(40); // comfortably under budget alone
        let (o1, p1) = plan_skill_body_append("", &first, budget);
        assert_eq!(o1, SkillBodyAppendOutcome::Full);
        let mut body = p1;
        assert!(estimate_tokens(&body) <= budget);

        // Second body is large — must truncate or skip, never exceed budget.
        let second = "BETA ".repeat(400);
        let (o2, p2) = plan_skill_body_append(&body, &second, budget);
        assert_ne!(o2, SkillBodyAppendOutcome::Full);
        body.push_str(&p2);
        assert!(
            estimate_tokens(&body) <= budget + 8,
            "accumulated skill bodies must stay near the total budget (got {})",
            estimate_tokens(&body)
        );
        assert!(body.contains("ALPHA"));
        if o2 == SkillBodyAppendOutcome::Truncated {
            assert!(body.contains("skill truncated: total skill budget"));
        }

        // With budget already full, further appends skip.
        let (o3, p3) = plan_skill_body_append(&body, "GAMMA leftover", budget);
        assert_eq!(o3, SkillBodyAppendOutcome::Skipped);
        assert!(p3.is_empty());
    }

    #[tokio::test]
    async fn test_set_skill_context_caps_many_full_guides() {
        use crate::agent::tokenizer::estimate_tokens;

        let interface = LlmInterface::new(ChatConfig::default());
        // Default interface has no runtime → max_context=4096 → budget=2000.
        let budget = skill_bodies_token_budget(4_096);
        assert_eq!(budget, 2_000);

        // 15 oversized guides — without a total cap these would blow the
        // fixed prefix (T4 production symptom with 15+ builtin skills).
        for i in 0..15 {
            let guide = format!("GUIDE{i} {}", "技能正文".repeat(200));
            let payload = format!(r#"{{"id":"skill-{i}","name":"S{i}","guide":"{guide}"}}"#);
            interface
                .set_skill_context(format!("skill-{i}"), payload)
                .await;
        }
        let body = interface
            .get_skill_context()
            .await
            .expect("at least the first skill should land");
        let tokens = estimate_tokens(&body);
        assert!(
            tokens <= budget + 16,
            "15 skill loads must respect total budget {budget}, got {tokens}"
        );
        // First skill retained; not all 15 full bodies can fit.
        assert!(
            body.contains("GUIDE0")
                || body.contains("skill-0")
                || body.contains("\"id\":\"skill-0\"")
        );
        let loaded_markers = (0..15)
            .filter(|i| body.contains(&format!("GUIDE{i}")))
            .count();
        assert!(
            loaded_markers < 15,
            "expected total cap to drop some of 15 full guides, kept {loaded_markers}"
        );
    }

    #[test]
    fn build_skill_injection_block_shares_budget_across_pin_and_transient() {
        use crate::agent::tokenizer::estimate_tokens;

        let budget = 200;
        let pin = format!("## Pinned Skill: Weather\n{}", "PIN ".repeat(80));
        let transient = format!("TRANSIENT {}", "导 ".repeat(200));
        let block = build_skill_injection_block(
            &[("weather".into(), pin.clone())],
            Some(&transient),
            budget,
        );
        assert!(
            estimate_tokens(&block) <= budget + 8,
            "pin+transient must share one budget, got {}",
            estimate_tokens(&block)
        );
        assert!(block.contains("Pinned Skill: Weather"));
        // Independent budgets would keep both full bodies (~2×); shared cap
        // must truncate or drop the later transient section.
        let both_full = block.contains("PIN ")
            && block.contains("TRANSIENT")
            && !block.contains("skill truncated");
        assert!(
            !both_full || estimate_tokens(&block) <= budget + 8,
            "must not inject full pin and full transient beyond shared budget"
        );
    }

    #[tokio::test]
    async fn assemble_system_prompt_pin_plus_transient_respects_shared_budget() {
        use crate::agent::tokenizer::estimate_tokens;
        use crate::skills::SkillRegistry;

        let interface = LlmInterface::new(ChatConfig::default());
        let budget = skill_bodies_token_budget(4_096);
        assert_eq!(budget, 2_000);

        // Register an oversized pin (high per-skill metadata budget so
        // body_within_budget does not hide the total-cap bug).
        let mut registry = SkillRegistry::new();
        let pin_body = "PINBODY ".repeat(800);
        let yaml = format!(
            "---\nid: pin-big\nname: PinBig\ncategory: general\norigin: user\ntoken_budget: 8000\n---\n{pin_body}"
        );
        registry.add_user_skill(&yaml).expect("parse pin skill");
        interface
            .set_skill_registry(std::sync::Arc::new(tokio::sync::RwLock::new(registry)))
            .await;
        interface.set_pinned_skills(vec!["pin-big".into()]).await;

        // Transient load also oversized — previously each path used a full
        // budget, so assemble could inject ~4k tokens of skill bodies.
        let transient = format!("TRANS {}", "正文".repeat(800));
        interface
            .set_skill_context("transient-1".into(), transient)
            .await;

        let prompt = interface.assemble_system_prompt(None).await;
        // Extract skill region roughly: from first "## Pinned" / "## Skill".
        let skill_start = prompt
            .find("## Pinned Skill:")
            .or_else(|| prompt.find("## Skill Reference"))
            .expect("skill injection present");
        // The budget covers the skill BLOCK only — the cross-turn usage note
        // ("## Recently Used Guides") may follow it and is not skill body.
        let skill_region = match prompt[skill_start..].find("## Recently Used Guides") {
            Some(note_at) => &prompt[skill_start..skill_start + note_at],
            None => &prompt[skill_start..],
        };
        let tokens = estimate_tokens(skill_region);
        assert!(
            tokens <= budget + 32,
            "production assemble must keep pin+transient under shared budget {budget}, got {tokens}"
        );
    }

    #[test]
    fn image_delegation_prompt_is_one_shot() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let interface = LlmInterface::new(ChatConfig::default());
            assert!(interface.take_image_delegation_prompt().await.is_none());
            interface
                .set_image_delegation_prompt("combined".into())
                .await;
            // Staging twice replaces the pending prompt.
            interface
                .set_image_delegation_prompt("combined-2".into())
                .await;
            assert_eq!(
                interface.take_image_delegation_prompt().await.as_deref(),
                Some("combined-2")
            );
            // One-shot: a missed turn can never leak into a later message.
            assert!(interface.take_image_delegation_prompt().await.is_none());
        });
    }

    #[tokio::test]
    async fn test_thinking_enabled() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Initially None (uses backend default)
        assert_eq!(interface.get_thinking_enabled().await, None);

        // Set to true
        interface.set_thinking_enabled(true).await;
        assert_eq!(interface.get_thinking_enabled().await, Some(true));

        // Set to false
        interface.set_thinking_enabled(false).await;
        assert_eq!(interface.get_thinking_enabled().await, Some(false));
    }

    #[tokio::test]
    async fn test_tool_definitions() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Initially empty
        assert!(interface.get_tool_definitions().await.is_empty());

        // Set tool definitions
        let tools = vec![neomind_core::llm::backend::ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            }),
        }];
        interface.set_tool_definitions(tools.clone()).await;

        let retrieved = interface.get_tool_definitions().await;
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_use_instance_manager() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Initially not using instance manager
        assert!(!interface.uses_instance_manager());

        // Enable instance manager mode
        interface.set_use_instance_manager(true).await;
        assert!(interface.uses_instance_manager());

        // Disable
        interface.set_use_instance_manager(false).await;
        assert!(!interface.uses_instance_manager());
    }

    #[tokio::test]
    async fn test_update_model() {
        let config = ChatConfig {
            model: "initial-model".to_string(),
            ..Default::default()
        };
        let interface = LlmInterface::new(config);

        // Update the model
        interface.update_model("new-model".to_string()).await;
        // Model is updated internally (verified through is_ready which checks model)
    }

    #[tokio::test]
    async fn test_is_ready_without_llm() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Without LLM set and not using instance manager, should not be ready
        assert!(!interface.is_ready().await);
    }

    #[test]
    fn test_concurrent_limit() {
        let config = ChatConfig {
            concurrent_limit: 5,
            ..Default::default()
        };
        let interface = LlmInterface::new(config);

        assert_eq!(interface.available_permits(), 5);
    }

    #[tokio::test]
    async fn test_chat_response_structure() {
        let response = ChatResponse {
            text: "Test response".to_string(),
            tokens_used: 10,
            duration: std::time::Duration::from_millis(100),
            finish_reason: "stop".to_string(),
            thinking: None,
        };

        assert_eq!(response.text, "Test response");
        assert_eq!(response.tokens_used, 10);
        assert_eq!(response.finish_reason, "stop");
        assert!(response.thinking.is_none());
    }

    #[tokio::test]
    async fn test_chat_response_with_thinking() {
        let response = ChatResponse {
            text: "Test response".to_string(),
            tokens_used: 15,
            duration: std::time::Duration::from_millis(200),
            finish_reason: "stop".to_string(),
            thinking: Some("Let me think...".to_string()),
        };

        assert!(response.thinking.is_some());
        assert_eq!(response.thinking.as_ref().unwrap(), "Let me think...");
    }

    #[test]
    fn test_agent_error_display() {
        let err = NeoMindError::Llm("LLM not ready".to_string());
        assert!(err.to_string().contains("not ready"));

        let err = NeoMindError::Llm("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }

    /// Open a throwaway store at a unique temp path (mirrors the `test_store`
    /// pattern in `instance_manager.rs`). The store layer keeps a process-global
    /// singleton keyed by path, so distinct paths yield isolated databases.
    fn test_store(tag: &str) -> Arc<neomind_storage::LlmBackendStore> {
        crate::prune_stale_temp_entries("neomind-test-", std::time::Duration::from_secs(3600));
        let path =
            std::env::temp_dir().join(format!("neomind-test-{}-{}.redb", tag, std::process::id()));
        let _ = std::fs::remove_file(&path);
        neomind_storage::LlmBackendStore::open(&path).expect("open test store")
    }

    /// Both integral-thinking tests below install a manager into the
    /// process-global `INSTANCE_MANAGER` singleton (`active_thinking_is_integral`
    /// reads the GLOBAL active instance). cargo test runs them on separate
    /// threads, so serialize them or they'd clobber each other's global.
    fn test_global_manager_serial() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[serial_test::serial]
    #[tokio::test]
    // The guard is held across the whole test ON PURPOSE: it serializes tests
    // that swap the process-global instance manager (test_global_manager_serial).
    #[allow(clippy::await_holding_lock)]
    async fn thinking_override_ignored_when_integral() {
        let _serial = test_global_manager_serial().lock().unwrap();
        // Active (GLOBAL) instance is an integral-thinking BUILTIN model
        // (e.g. LFM2.5) whose thinking cannot be disabled. The per-call
        // thinking_override — used by non-chat calls (summarization, post-tool
        // rounds) to force thinking off — must be IGNORED so thinking stays
        // enabled.
        let manager = Arc::new(LlmBackendInstanceManager::new(test_store("integral")));
        let mut inst = neomind_storage::LlmBackendInstance::new(
            "lfm25".to_string(),
            "LFM2.5".to_string(),
            neomind_storage::LlmBackendType::LlamaCpp,
        );
        inst.is_builtin = true;
        inst.thinking_is_integral = true;
        manager
            .upsert_instance(inst)
            .await
            .expect("upsert integral instance");
        manager.set_active("lfm25").await.expect("set active");
        crate::llm_backends::set_instance_manager(manager.clone());

        let interface = LlmInterface::with_instance_manager(ChatConfig::default(), manager);
        assert!(
            interface.active_thinking_is_integral().await,
            "integral builtin instance must be detected"
        );

        let (enabled, _effort) = interface.effective_thinking_control(Some(false)).await;
        assert_eq!(
            enabled,
            Some(true),
            "integral model must keep thinking on despite override"
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    // Deliberate serialization guard (see the test above).
    #[allow(clippy::await_holding_lock)]
    async fn thinking_override_honored_when_not_integral() {
        let _serial = test_global_manager_serial().lock().unwrap();
        // Non-integral models keep existing behavior: the per-call override wins
        // (gotcha #7 — qwen/deepseek memory-extraction calls disable thinking).
        let manager = Arc::new(LlmBackendInstanceManager::new(test_store("non-integral")));
        let inst = neomind_storage::LlmBackendInstance::new(
            "qwen35".to_string(),
            "qwen3.5".to_string(),
            neomind_storage::LlmBackendType::Ollama,
        );
        manager
            .upsert_instance(inst)
            .await
            .expect("upsert non-integral instance");
        manager.set_active("qwen35").await.expect("set active");
        crate::llm_backends::set_instance_manager(manager.clone());

        let interface = LlmInterface::with_instance_manager(ChatConfig::default(), manager);
        assert!(
            !interface.active_thinking_is_integral().await,
            "non-integral instance must not be flagged integral"
        );

        let (enabled, _effort) = interface.effective_thinking_control(Some(false)).await;
        assert_eq!(
            enabled,
            Some(false),
            "non-integral model must honor the override"
        );
    }

    #[tokio::test]
    async fn current_time_is_appended_at_tail_of_assembled_system_prompt() {
        let interface = LlmInterface::new(ChatConfig::default());
        interface
            .set_system_prompt_suffix(Some("SUFFIX_MARKER_TAIL_TEST".into()))
            .await;
        interface
            .set_skill_context("rule-management".into(), "SKILL_MARKER_TAIL_TEST".into())
            .await;
        interface
            .set_task_state_prompt(Some("## Task State\nTASK_MARKER_TAIL_TEST".into()))
            .await;

        let fixed = chrono::DateTime::parse_from_rfc3339("2026-11-19T12:34:56Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        interface.set_turn_timestamp_for_test(Some(fixed)).await;

        let prompt = interface
            .build_system_prompt_with_tools(Some("list my devices"))
            .await;
        let time_pos = prompt
            .find("## Current time")
            .expect("final system prompt must include `## Current time`");
        for marker in [
            "SUFFIX_MARKER_TAIL_TEST",
            "SKILL_MARKER_TAIL_TEST",
            "TASK_MARKER_TAIL_TEST",
            "## Current Task:",
        ] {
            let marker_pos = prompt
                .find(marker)
                .unwrap_or_else(|| panic!("expected marker `{marker}` in prompt"));
            assert!(
                time_pos > marker_pos,
                "`## Current time` (at {time_pos}) must appear AFTER `{marker}` (at {marker_pos})"
            );
        }
        // `## Current time` must be the final section (no `## ` heading after it).
        let after_time = &prompt[time_pos + "## Current time".len()..];
        assert!(
            !after_time.contains("\n## "),
            "no section may follow `## Current time`: {after_time}"
        );
        assert!(after_time.contains("2026-11-19 12:34:56 UTC"));
    }

    #[tokio::test]
    async fn turn_pinned_timestamp_remains_stable_across_rounds() {
        let interface = LlmInterface::new(ChatConfig::default());
        let t1 = chrono::DateTime::parse_from_rfc3339("2026-11-19T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        interface.set_turn_timestamp_for_test(Some(t1)).await;

        let round1 = interface
            .build_system_prompt_with_tools(Some("check devices"))
            .await;
        let round2 = interface
            .build_system_prompt_with_tools(Some("check devices"))
            .await;
        assert_eq!(
            round1, round2,
            "system prompt must stay byte-identical across rounds within one turn"
        );
        assert!(round1.contains("2026-11-19 08:00:00 UTC"));

        // A new turn refreshes the pinned timestamp.
        let t2 = chrono::DateTime::parse_from_rfc3339("2026-11-19T08:05:30Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        interface.set_turn_timestamp_for_test(Some(t2)).await;
        let next_turn = interface
            .build_system_prompt_with_tools(Some("check devices"))
            .await;
        assert!(next_turn.contains("2026-11-19 08:05:30 UTC"));
    }

    #[tokio::test]
    async fn skill_dedup_key_includes_section_so_overview_never_blocks_section() {
        let interface = LlmInterface::new(ChatConfig::default());

        // 1) Load overview `(id, None)`.
        let o1 = interface
            .set_skill_section_context_outcome(
                "dashboard-management".into(),
                None,
                r#"{"id":"dashboard-management","guide":"OVERVIEW_BODY"}"#.into(),
            )
            .await;
        assert_eq!(o1, SkillBodyAppendOutcome::Full);

        // 2) Load a specific section `(id, Some("Layout"))` via direct API with
        //    the SAME base `skill_id` — must NOT be dropped as AlreadyPresent.
        let o2 = interface
            .set_skill_context_outcome(
                "dashboard-management".into(),
                r#"{"id":"dashboard-management","section":"Layout","guide":"LAYOUT_SECTION"}"#
                    .into(),
            )
            .await;
        assert_eq!(
            o2,
            SkillBodyAppendOutcome::Full,
            "loading a section after overview must not be deduped as AlreadyPresent"
        );

        // 3) Repeating the same `(id, Some("Layout"))` IS AlreadyPresent.
        let o3 = interface
            .set_skill_section_context_outcome(
                "dashboard-management".into(),
                Some("Layout".into()),
                r#"{"id":"dashboard-management","section":"Layout","guide":"LAYOUT_SECTION"}"#
                    .into(),
            )
            .await;
        assert_eq!(o3, SkillBodyAppendOutcome::AlreadyPresent);

        // 4) Loading a DIFFERENT section of the same skill still succeeds.
        let o4 = interface
            .set_skill_section_context_outcome(
                "dashboard-management".into(),
                Some("DataSource".into()),
                r#"{"id":"dashboard-management","section":"DataSource","guide":"DATASOURCE_SECTION"}"#
                    .into(),
            )
            .await;
        assert_eq!(o4, SkillBodyAppendOutcome::Full);

        let body = interface.get_skill_context().await.unwrap();
        assert_eq!(body.matches("OVERVIEW_BODY").count(), 1);
        assert_eq!(body.matches("LAYOUT_SECTION").count(), 1);
        assert_eq!(body.matches("DATASOURCE_SECTION").count(), 1);
    }
}
