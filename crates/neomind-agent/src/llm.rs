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

/// Transient skill context for the current turn.
///
/// Tracks which skill ids have already been loaded so re-loading the same skill
/// (a common LLM pattern when stuck in a retry loop) doesn't duplicate the body
/// in the system prompt. The first legitimate load always preserves the full body.
#[derive(Default)]
struct TransientSkillContext {
    loaded_ids: std::collections::HashSet<String>,
    body: String,
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
    /// Last prompt token count from the most recent stream response.
    /// Updated in-band when the stream completes.
    last_prompt_tokens: Arc<tokio::sync::Mutex<Option<u32>>>,
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
    /// Pinned skill IDs selected by the user for this session.
    /// These skills are injected as full guides (not just hints) into the system prompt.
    pinned_skills: Arc<RwLock<Vec<String>>>,
    /// Frozen memory-snapshot prompt section (user.md/knowledge.md/procedures),
    /// injected into the chat system prompt so the base prompt's "auto-loaded
    /// memory" promise is truthful. Set by `Agent::set_memory_snapshot`.
    memory_context: Arc<RwLock<Option<String>>>,
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
    /// Reads the GLOBAL active instance, not `self.instance_manager`: production
    /// `LlmInterface`s are built via `LlmInterface::new()` and never carry
    /// `self.instance_manager`, so the old per-interface read made this guard
    /// inert (the builtin LFM would get `thinking_enabled=false` forced on
    /// non-chat calls). Gated on `is_builtin` so a session directly configured
    /// to a non-builtin backend while the builtin happens to be globally active
    /// does not inherit the exception.
    async fn active_thinking_is_integral(&self) -> bool {
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
        let resolved = self.resolve_thinking_control().await;
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
            last_prompt_tokens: Arc::new(tokio::sync::Mutex::new(None)),
            intent_classifier: IntentClassifier::default(),
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
            skill_registry: Arc::new(RwLock::new(None)),
            skill_context: Arc::new(RwLock::new(TransientSkillContext::default())),
            pinned_skills: Arc::new(RwLock::new(Vec::new())),
            memory_context: Arc::new(RwLock::new(None)),
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
            last_prompt_tokens: Arc::new(tokio::sync::Mutex::new(None)),
            intent_classifier: IntentClassifier::default(),
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
            skill_registry: Arc::new(RwLock::new(None)),
            skill_context: Arc::new(RwLock::new(TransientSkillContext::default())),
            pinned_skills: Arc::new(RwLock::new(Vec::new())),
            memory_context: Arc::new(RwLock::new(None)),
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
    pub async fn take_last_prompt_tokens(&self) -> Option<u32> {
        self.last_prompt_tokens.lock().await.take()
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

        const SETTINGS_DB_PATH: &str = "data/settings.redb";

        let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
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

    /// Get effective generation parameters.
    /// When using instance manager, reads from the active backend instance.
    /// Otherwise falls back to local ChatConfig values.
    async fn get_effective_params(&self) -> (f32, f32, usize, usize) {
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                if let Some(inst) = manager.get_active_instance() {
                    return (inst.temperature, inst.top_p, inst.top_k, inst.max_tokens);
                }
            }
        }
        // Fall back to local config
        (self.temperature, self.top_p, self.top_k, self.max_tokens)
    }

    /// Set the LLM runtime (direct mode).
    pub async fn set_llm(&self, llm: Arc<dyn LlmRuntime>) {
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

        let overhead = prompt_tokens + tools_tokens;
        tracing::debug!(
            "Prompt overhead: system_prompt={} tokens, tools={} tokens ({} tools), total={} tokens",
            prompt_tokens,
            tools_tokens,
            tools.len(),
            overhead
        );
        overhead
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
    pub async fn clear_skill_context(&self) {
        *self.skill_context.write().await = TransientSkillContext::default();
    }

    /// Append transient skill context (from skill tool results).
    ///
    /// Deduplicates by `skill_id`: if the same skill is loaded again in this turn,
    /// the call is a no-op (full body from the first load is retained unchanged).
    /// Distinct skill ids accumulate, separated by a blank line.
    pub async fn set_skill_context(&self, skill_id: String, content: String) {
        let mut guard = self.skill_context.write().await;
        if !guard.loaded_ids.insert(skill_id) {
            // Already loaded this turn — skip to avoid duplicate context bloat.
            return;
        }
        if !guard.body.is_empty() {
            guard.body.push_str("\n\n");
        }
        guard.body.push_str(&content);
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

    /// Set the frozen memory-snapshot prompt section for this session (or None
    /// to clear). Injected into the chat system prompt by
    /// `build_system_prompt_with_tools` so the base prompt's "auto-loaded
    /// memory" instruction is truthful.
    pub async fn set_memory_context(&self, section: Option<String>) {
        *self.memory_context.write().await = section;
    }

    /// Get pinned skill IDs for this session.
    pub async fn get_pinned_skills(&self) -> Vec<String> {
        self.pinned_skills.read().await.clone()
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
        // Start from the RAW base prompt (time placeholders still intact).
        // Time is injected LAST, after all variable sections are appended —
        // the template deliberately places the time block near the end for
        // KV-prefix caching, and appending sections after a resolved time
        // block used to re-prefill pinned skills / memory / suffix on every
        // time change.
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

        // Inject pinned skills (user-selected) as full guides
        let pinned = self.pinned_skills.read().await.clone();
        if !pinned.is_empty() {
            let registry_opt = self.skill_registry.read().await.clone();
            if let Some(registry) = registry_opt {
                let registry_guard = registry.read().await;
                for skill_id in &pinned {
                    if let Some(skill) = registry_guard.get(skill_id) {
                        prompt.push_str(&format!(
                            "\n## Pinned Skill: {}\n{}\n",
                            skill.metadata.name, skill.body
                        ));
                    }
                }
            }
        }

        // Inject frozen memory snapshot (user.md/knowledge.md/procedures) —
        // stable per session. The base prompt promises this is "auto-loaded";
        // this injection makes that truthful on the chat path (previously the
        // chat builder discarded the snapshot that only the dead Path-A
        // assembler used).
        let memory_section = self.memory_context.read().await.clone();
        if let Some(section) = memory_section {
            if !section.is_empty() {
                prompt.push('\n');
                prompt.push_str(&section);
                prompt.push('\n');
            }
        }

        // Auto-load skill guides — DISABLED: the slim prompt (default) dropped the
        // full CLI reference table, and the shell tool's Command Choice covers only
        // a subset of domains. Skills are loaded on-demand via the skill tool
        // (search/load) when the LLM needs domain-specific guidance beyond the
        // always-present Command Choice (complex workflows, error troubleshooting).
        // Re-enabling auto-load would re-add ~3000 tokens per turn.

        // Inject transient skill context (from skill tool calls in current turn's multi-round loop)
        // This content is NOT in session history — only available during the current turn.
        if let Some(skill_content) = self.get_skill_context().await {
            prompt.push_str("\n## Skill Reference\n");
            prompt.push_str("Commands in this skill are canonical — use them exactly; don't guess subcommand names.\n\n");
            prompt.push_str(&skill_content);
            prompt.push('\n');
        }

        // Add intent-specific addon if we can classify the user's message
        if let Some(msg) = user_message {
            let intent = self.intent_classifier.classify(msg);

            // Get intent addon using legacy PromptBuilder
            use crate::prompts::PromptBuilder;

            let task_type = match intent.category {
                crate::agent::staged::IntentCategory::Device => "device",
                crate::agent::staged::IntentCategory::Data => "data",
                crate::agent::staged::IntentCategory::Rule => "rule",
                _ => "general",
            };

            // Get intent-specific addon from PromptBuilder
            let addon = PromptBuilder::new().get_intent_prompt_addon(task_type);

            if !addon.is_empty() {
                prompt.push_str(&addon);
            }
        }

        // Inject time LAST (see the comment at the top of this builder) —
        // everything above the time block is now prefix-cache stable.
        self.inject_time_placeholders(prompt, None).await
    }

    /// Replace the base prompt's time placeholders with current values.
    /// Extracted so the streaming builder can defer time injection until all
    /// variable sections are appended (KV-prefix-cache friendly) while the
    /// other callers keep the up-front behavior.
    async fn inject_time_placeholders(&self, prompt: String, timezone: Option<&str>) -> String {
        use crate::prompts::{
            CURRENT_TIME_PLACEHOLDER, LOCAL_TIME_PLACEHOLDER, TIMEZONE_PLACEHOLDER,
        };

        // Calculate current times
        let now = chrono::Utc::now();
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

        // Replace placeholders
        prompt
            .replace(CURRENT_TIME_PLACEHOLDER, &current_time_utc)
            .replace(LOCAL_TIME_PLACEHOLDER, &local_time_with_context)
            .replace(TIMEZONE_PLACEHOLDER, &effective_timezone)
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

        // Clear transient skill context from previous turn
        self.clear_skill_context().await;

        let max_ctx = self.max_context_length().await;

        // === FAST PATH: Simple greetings ===
        // Bypass LLM for simple greetings to improve response time
        let trimmed = user_message.trim();
        let greeting_patterns = [
            "你好",
            "您好",
            "hi",
            "hello",
            "嗨",
            "在吗",
            "早上好",
            "下午好",
            "晚上好",
        ];
        let is_greeting = greeting_patterns
            .iter()
            .any(|&pat| trimmed.eq_ignore_ascii_case(pat) || trimmed.starts_with(pat));

        if is_greeting && trimmed.len() < 20 {
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

        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Check if we have tools registered
        let has_tools = !self.tool_definitions.read().await.is_empty();

        let system_prompt = if has_tools {
            self.build_system_prompt_with_tools(Some(&user_message))
                .await
        } else {
            self.system_prompt.read().await.clone()
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
        };

        let system_msg = Message::system(&system_prompt);
        let user_msg = Message::user(&user_message);

        // Build messages with history if provided, truncated to fit context window
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();

            let (available_tokens, _prompt_budget) = self
                .compute_history_budget(
                    max_ctx,
                    &system_prompt,
                    &user_message,
                    history_msgs.len(),
                    has_tools,
                )
                .await;

            let total_history_tokens: usize = history_msgs
                .iter()
                .map(|m| {
                    let text = m.content.as_text();
                    estimate_tokens(&text)
                })
                .sum();

            if total_history_tokens <= available_tokens {
                for msg in &history_msgs {
                    msgs.push((*msg).clone());
                }
            } else {
                let mut used = 0usize;
                let mut kept = Vec::new();
                for msg in history_msgs.iter().rev() {
                    let text = msg.content.as_text();
                    let tokens = estimate_tokens(&text);
                    if used + tokens > available_tokens {
                        break;
                    }
                    used += tokens;
                    kept.push((*msg).clone());
                }
                kept.reverse();
                tracing::info!(
                    total_history_tokens,
                    kept_messages = kept.len(),
                    total_messages = history_msgs.len(),
                    available_tokens,
                    "Truncated conversation history to fit context window"
                );
                msgs.extend(kept);
            }

            msgs.push(user_msg);
            msgs
        } else {
            vec![system_msg, user_msg]
        };

        // Get tool definitions — no vision filtering here (chat_internal is
        // text-only; chat_internal_message handles image filtering separately).
        let tools_input = if has_tools {
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
        // Clear transient skill context from previous turn
        self.clear_skill_context().await;

        let max_ctx = self.max_context_length().await;

        // Acquire permit for concurrency limiting
        let _permit = self.limiter.acquire().await;

        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Check if we have tools registered
        let has_tools = !self.tool_definitions.read().await.is_empty();

        let system_prompt = if has_tools {
            // Extract text from user message for system prompt
            let user_text = user_message.content.as_text();
            self.build_system_prompt_with_tools(Some(&user_text)).await
        } else {
            self.system_prompt.read().await.clone()
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
        let (thinking_enabled, thinking_effort) = self.resolve_thinking_control().await;

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
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();

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

            let total_history_tokens: usize = history_msgs
                .iter()
                .map(|m| {
                    let text = m.content.as_text();
                    estimate_tokens(&text)
                })
                .sum();

            if total_history_tokens <= available_tokens {
                for msg in &history_msgs {
                    msgs.push((*msg).clone());
                }
            } else {
                let mut used = 0usize;
                let mut kept = Vec::new();
                for msg in history_msgs.iter().rev() {
                    let text = msg.content.as_text();
                    let tokens = estimate_tokens(&text);
                    if used + tokens > available_tokens {
                        break;
                    }
                    used += tokens;
                    kept.push((*msg).clone());
                }
                kept.reverse();
                tracing::info!(
                    total_history_tokens,
                    kept_messages = kept.len(),
                    total_messages = history_msgs.len(),
                    available_tokens,
                    "Truncated conversation history to fit context window"
                );
                msgs.extend(kept);
            }

            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
        };

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
        let tools_input = if has_tools {
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
        self.chat_stream_internal(user_message, Some(history), true, None)
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
        self.chat_stream_internal(user_message, Some(history), true, thinking_override)
            .await
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
        self.chat_stream_internal(user_message, Some(history), false, Some(false))
            .await
    }

    /// Send a multimodal chat message (with images) with streaming response.
    /// This method accepts a Message directly, which can contain text and images.
    pub async fn chat_stream_multimodal_with_history(
        &self,
        user_message: Message, // Can contain text + images via Content::Parts
        history: &[Message],
    ) -> AgentResult<Pin<Box<dyn Stream<Item = AgentResult<(String, bool)>> + Send>>> {
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

        // Build system prompt (with or without tools based on phase)
        let system_prompt = if include_tools {
            self.build_system_prompt_with_tools(None).await
        } else {
            // Simple system prompt for non-tool calls
            "You are NeoMind, a helpful IoT assistant. Answer questions concisely and accurately."
                .to_string()
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
        let (thinking_enabled, thinking_effort) = self.resolve_thinking_control().await;

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
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
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();

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

            let total_history_tokens: usize = history_msgs
                .iter()
                .map(|m| {
                    let text = m.content.as_text();
                    estimate_tokens(&text)
                })
                .sum();

            if total_history_tokens <= available_tokens {
                for msg in &history_msgs {
                    msgs.push((*msg).clone());
                }
            } else {
                let mut used = 0usize;
                let mut kept = Vec::new();
                for msg in history_msgs.iter().rev() {
                    let text = msg.content.as_text();
                    let tokens = estimate_tokens(&text);
                    if used + tokens > available_tokens {
                        break;
                    }
                    used += tokens;
                    kept.push((*msg).clone());
                }
                kept.reverse();
                tracing::info!(
                    total_history_tokens,
                    kept_messages = kept.len(),
                    total_messages = history_msgs.len(),
                    available_tokens,
                    "Truncated conversation history to fit context window (multimodal stream)"
                );
                msgs.extend(kept);
            }

            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
        };

        // Get tool definitions — filter out "vision" when the user message already
        // contains images.  The model is multimodal and can analyse them directly;
        // the vision tool is only needed for fetching images from URLs/files.
        let tools = self.tool_definitions.read().await;
        let tools_input = if tools.is_empty() {
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
        let wrapped_stream = PermitStream::new(stream, permit);
        let token_tracker = self.last_prompt_tokens.clone();

        // Convert stream
        Ok(Box::pin(async_stream::stream! {
            let mut stream = wrapped_stream;
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
                "You are NeoMind, a helpful IoT assistant. Answer questions concisely. \
                 You can help with device management, data queries, and automation rules. \
                 Keep responses brief."
                    .to_string()
            } else {
                self.build_system_prompt_with_tools(Some(&user_message))
                    .await
            }
        } else {
            // Simple system prompt for non-tool calls
            "You are NeoMind, a helpful IoT assistant. Answer questions concisely and accurately."
                .to_string()
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
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal"
        );

        // Safety: auto-disable thinking for very large prompts.
        // Small thinking models (2B) with large prompts (20k+ tokens) tend to
        // consume all num_predict tokens on thinking alone, producing no content.
        // Observed: qwen3.5:2b with prompt_eval=32259, eval=32768, content=0.
        let thinking_enabled = if thinking_enabled == Some(true) {
            // Size the prompt with the same estimator the budget math uses, so the
            // guard reflects real token density — not a bytes*0.8 approximation
            // that over-counted English (~3×) and tripped the guard far too
            // eagerly on text-heavy prompts.
            let estimated_tokens = crate::agent::tokenizer::estimate_prompt_tokens(
                history.unwrap_or(&[]),
                &system_prompt,
                &user_message,
            );

            if estimated_tokens > 18000 {
                tracing::info!(
                    estimated_tokens,
                    hist_messages = history.map(|h| h.len()).unwrap_or(0),
                    "Auto-disabling thinking for large prompt to prevent thinking loop hang"
                );
                Some(false)
            } else {
                thinking_enabled
            }
        } else {
            thinking_enabled
        };

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
        let compact_fallback_system = "You are NeoMind IoT assistant. Summarize results concisely based on the tool results shown.".to_string();
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
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];

            // Collect non-system history messages
            let history_msgs: Vec<&Message> = hist
                .iter()
                .filter(|msg| msg.role != neomind_core::MessageRole::System)
                .collect();

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

            let total_history_tokens: usize = history_msgs
                .iter()
                .map(|m| {
                    let text = m.content.as_text();
                    estimate_tokens(&text)
                })
                .sum();

            if total_history_tokens <= available_tokens {
                for msg in &history_msgs {
                    msgs.push((*msg).clone());
                }
            } else {
                // Find the first user message index (original question) — always preserve it
                let original_user_idx = history_msgs
                    .iter()
                    .position(|m| m.role == neomind_core::MessageRole::User);
                let user_msg_tokens = original_user_idx
                    .map(|idx| estimate_tokens(&history_msgs[idx].content.as_text()))
                    .unwrap_or(0);
                let budget_for_others = available_tokens.saturating_sub(user_msg_tokens);

                // Keep most recent messages that fit within remaining budget
                let mut used = 0usize;
                let mut kept = Vec::new();
                for (i, msg) in history_msgs.iter().enumerate().rev() {
                    // Skip the original user message — we'll add it separately
                    if original_user_idx == Some(i) {
                        continue;
                    }
                    let text = msg.content.as_text();
                    let tokens = estimate_tokens(&text);
                    if used + tokens > budget_for_others {
                        break;
                    }
                    used += tokens;
                    kept.push((*msg).clone());
                }
                kept.reverse();

                // Always prepend the original user message
                if let Some(idx) = original_user_idx {
                    kept.insert(0, history_msgs[idx].clone());
                }

                tracing::info!(
                    total_history_tokens,
                    kept_messages = kept.len(),
                    total_messages = history_msgs.len(),
                    available_tokens,
                    "Truncated conversation history to fit context window (original user message preserved)"
                );
                msgs.extend(kept);
            }

            msgs.push(user_msg);
            msgs
        } else {
            vec![system_msg, user_msg]
        };

        // Get tool definitions
        let tools = self.tool_definitions.read().await;
        let tools_input = if tools.is_empty() {
            None
        } else {
            Some(tools.clone())
        };
        drop(tools);

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
        let wrapped_stream = PermitStream::new(stream, permit);

        // Prepare retry data for tiered context overflow retry
        let llm_retry = llm.clone();
        let limiter_retry = self.limiter.clone();
        let token_tracker = self.last_prompt_tokens.clone();

        // Convert stream with tiered context overflow retry
        Ok(Box::pin(async_stream::stream! {
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
                                    thinking_enabled: None, // Disable thinking on retry
                                    thinking_effort: None,
                                    max_context: None,
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

/// Extract a stable skill id from a `skill` tool result body for dedup purposes.
///
/// Skill tool `load` results are JSON like `{"id":"rule-management","name":"...","guide":"..."}`.
/// We pull `id` (or `skill_id`) out of that. If the payload isn't JSON or lacks an id
/// (e.g. `search` results, error payloads), fall back to a content-derived key so the call
/// still dedups repeated identical payloads without accidentally collapsing distinct skills.
pub(crate) fn extract_skill_id_from_result(result_str: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(result_str) {
        if let Some(id) = v
            .get("id")
            .or_else(|| v.get("skill_id"))
            .and_then(|i| i.as_str())
        {
            return id.to_string();
        }
        // `search` returns an array of entries — distinct skill loads shouldn't collapse,
        // so derive a stable key from the payload itself.
        if let Some(arr) = v.as_array() {
            if let Some(first) = arr.first() {
                if let Some(id) = first.get("id").and_then(|i| i.as_str()) {
                    return format!("search:{}", id);
                }
            }
            return format!("search:len:{}", result_str.len());
        }
    }
    // Last-resort stable fallback: hash the content so repeated identical payloads dedup.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    result_str.hash(&mut h);
    format!("hash:{:x}", h.finish())
}

/// Extract in-band token usage marker from a stream chunk.
/// Returns (clean_content, is_thinking, extracted_prompt_tokens).
/// The marker format is `\n__NEOMIND_TOKEN_PROMPT:NN__`.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
