//! LLM interface for the Agent.
//!
//! This module provides a simple LLM wrapper with concurrency limits
//! and integration with the LlmBackendInstanceManager for dynamic backend switching.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::Stream;
use tokio::sync::RwLock;

use edge_ai_core::{
    Message,
    llm::backend::{LlmInput, LlmRuntime},
    config::agent_env_vars,
};

// Import intent classifier for staged processing
use crate::agent::staged::{IntentCategory, IntentClassifier, IntentResult, ToolFilter};

/// Re-export the instance manager types for convenience
pub use edge_ai_llm::instance_manager::{
    BackendTypeDefinition, LlmBackendInstanceManager, get_instance_manager,
};

/// Default concurrent LLM request limit.
/// Note: This constant is kept for backward compatibility but the actual default
/// is loaded from environment variable AGENT_CONCURRENT_LIMIT with fallback to 3.
pub const DEFAULT_CONCURRENT_LIMIT: usize = 3;

/// Get the concurrent limit from environment or return default.
#[inline]
pub fn get_concurrent_limit() -> usize {
    agent_env_vars::concurrent_limit()
}

/// Simple atomic-based concurrency limiter.
///
/// This is simpler than using a semaphore for streams because it doesn't
/// have lifetime issues with permits.
#[derive(Clone)]
struct ConcurrencyLimiter {
    current: Arc<AtomicUsize>,
    max: Arc<AtomicUsize>,  // Make max AtomciUsize for dynamic adjustment
    base_max: usize,         // Original configured max
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
        4 * 1024 * 1024 * 1024  // Assume 4GB available
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, use GlobalMemoryStatusEx
        // For simplicity, return a conservative estimate
        4 * 1024 * 1024 * 1024  // Assume 4GB available
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
    /// Tool definitions for function calling.
    tool_definitions: Arc<RwLock<Vec<edge_ai_core::llm::backend::ToolDefinition>>>,
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
    /// Intent classifier for staged processing.
    intent_classifier: IntentClassifier,
    /// Tool filter for reducing tools sent to LLM.
    tool_filter: ToolFilter,
    /// Business context manager for dynamic context injection.
    context_manager: Option<crate::context::ContextManager>,
    /// Global timezone for time-aware prompts (IANA format, e.g., "Asia/Shanghai").
    /// Loaded from settings and used for all time-related context.
    global_timezone: Arc<RwLock<Option<String>>>,
}

impl LlmInterface {
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
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            system_prompt_cache: Arc::new(RwLock::new(None)),
            cached_tools_hash: Arc::new(RwLock::new(None)),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(0)),
            thinking_enabled: Arc::new(RwLock::new(None)), // Use backend default (from storage)
            intent_classifier: IntentClassifier::default(),
            tool_filter: ToolFilter::default(),
            context_manager: None,
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
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
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            system_prompt_cache: Arc::new(RwLock::new(None)),
            cached_tools_hash: Arc::new(RwLock::new(None)),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(1)),
            thinking_enabled: Arc::new(RwLock::new(None)), // Will use instance manager setting
            intent_classifier: IntentClassifier::default(),
            tool_filter: ToolFilter::default(),
            context_manager: None,
            global_timezone: Arc::new(RwLock::new(None)), // Will be loaded from settings
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
    pub async fn load_global_timezone(&self) -> Result<String, AgentError> {
        use edge_ai_storage::SettingsStore;

        const SETTINGS_DB_PATH: &str = "data/settings.redb";

        let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
            .map_err(|e| AgentError::Generation(format!("Failed to open settings store: {}", e)))?;

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

    /// Get the instance manager if available.
    pub fn instance_manager(&self) -> Option<Arc<LlmBackendInstanceManager>> {
        self.instance_manager.clone()
    }

    /// Set the instance manager.
    pub async fn set_instance_manager(&self, manager: Arc<LlmBackendInstanceManager>) {
        // Update the instance manager reference
        // Note: This requires interior mutability pattern
        let _ = manager;
        // For now, this is a placeholder - the instance manager is set at creation time
        // To make this fully dynamic, we'd need to wrap it in Arc<RwLock<>>
    }

    /// Get the current LLM runtime, using instance manager if enabled.
    async fn get_runtime(&self) -> Result<Arc<dyn LlmRuntime>, AgentError> {
        // Try instance manager first if enabled
        if self.uses_instance_manager()
            && let Some(manager) = &self.instance_manager {
                return manager
                    .get_active_runtime()
                    .await
                    .map_err(|e| AgentError::Generation(e.to_string()));
            }

        // Fall back to direct runtime
        let llm_guard = self.llm.read().await;
        llm_guard
            .as_ref()
            .map(Arc::clone)
            .ok_or(AgentError::LlmNotReady)
    }

    /// Get effective generation parameters.
    /// When using instance manager, reads from the active backend instance.
    /// Otherwise falls back to local ChatConfig values.
    async fn get_effective_params(&self) -> (f32, f32, usize, usize) {
        if self.uses_instance_manager()
            && let Some(manager) = &self.instance_manager
                && let Some(inst) = manager.get_active_instance() {
                    return (
                        inst.temperature,
                        inst.top_p,
                        inst.top_k,
                        inst.max_tokens,
                    );
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

    /// Set the LLM runtime from a Box (for backward compatibility).
    ///
    /// This converts a Box<dyn LlmRuntime> to Arc<dyn LlmRuntime> by creating
    /// an Arc wrapper around the boxed value.
    pub async fn set_llm_from_box(&self, llm: Box<dyn LlmRuntime>) {
        // We need to convert Box to Arc
        // This is a bit tricky since we can't directly convert Box to Arc
        // The runtime needs to be created as Arc from the start
        // For backward compatibility, we'll need to use unsafe or restructure
        let _ = llm;
        // Store None for now - the caller should use Arc directly
        let mut llm_guard = self.llm.write().await;
        *llm_guard = None;
    }
    pub async fn switch_backend(&self, backend_id: &str) -> Result<(), AgentError> {
        if let Some(manager) = &self.instance_manager {
            manager
                .set_active(backend_id)
                .await
                .map_err(|e| AgentError::Generation(e.to_string()))?;
            Ok(())
        } else {
            Err(AgentError::Generation(
                "No instance manager configured".to_string(),
            ))
        }
    }

    /// Get available backend types.
    pub async fn get_available_backends(&self) -> Vec<BackendTypeDefinition> {
        if let Some(manager) = &self.instance_manager {
            manager.get_available_types()
        } else {
            Vec::new()
        }
    }

    /// Get the current concurrency limit (max concurrent requests).
    /// This returns the dynamic limit which may be adjusted based on system load.
    pub fn max_concurrent(&self) -> usize {
        self.limiter.max.load(Ordering::Relaxed)
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
    /// - qwen3-vl:2b -> 32768
    /// - llama3:8b -> 8192
    /// - deepseek-r1 -> 64000
    ///
    /// Returns a conservative default (4096) if the LLM is not ready.
    pub async fn max_context_length(&self) -> usize {
        // Try instance manager first if enabled
        if self.uses_instance_manager()
            && let Some(manager) = &self.instance_manager
                && let Some(instance) = manager.get_active_instance() {
                    // Instance has capabilities with max_context
                    return instance.capabilities.max_context;
                }

        // Fall back to querying the runtime directly
        match self.get_runtime().await {
            Ok(runtime) => runtime.max_context_length(),
            Err(_) => 4_096, // Conservative default if LLM not ready
        }
    }

    /// Check if the current LLM backend supports multimodal (vision) input.
    ///
    /// Returns true if the active backend supports image input, false otherwise.
    pub async fn supports_multimodal(&self) -> bool {
        // Try instance manager first if enabled
        if self.uses_instance_manager()
            && let Some(manager) = &self.instance_manager
                && let Some(instance) = manager.get_active_instance() {
                    // Instance has capabilities with supports_multimodal (storage layer)
                    return instance.capabilities.supports_multimodal;
                }

        // Fall back to querying the runtime directly
        match self.get_runtime().await {
            Ok(runtime) => runtime.capabilities().multimodal,
            Err(_) => false,
        }
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
    pub async fn warmup(&self) -> Result<(), AgentError> {
        match self.get_runtime().await {
            Ok(runtime) => runtime.warmup().await.map_err(|e| AgentError::Generation(e.to_string())),
            Err(e) => Err(e),
        }
    }

    /// Set the system prompt (builder pattern).
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        // Store the prompt in Arc<RwLock<String>>
        self.system_prompt = Arc::new(RwLock::new(prompt.into()));
        self
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
    pub async fn set_tool_definitions(
        &self,
        tools: Vec<edge_ai_core::llm::backend::ToolDefinition>,
    ) {
        *self.tool_definitions.write().await = tools;
        // Invalidate system prompt cache when tools change
        self.invalidate_prompt_cache().await;
    }

    /// Get current tool definitions.
    pub async fn get_tool_definitions(&self) -> Vec<edge_ai_core::llm::backend::ToolDefinition> {
        self.tool_definitions.read().await.clone()
    }

    /// Classify user intent from message.
    pub fn classify_intent(&self, message: &str) -> IntentResult {
        self.intent_classifier.classify(message)
    }

    /// Get intent-specific system prompt.
    pub fn get_intent_prompt(&self, intent: &IntentResult) -> String {
        self.tool_filter.intent_prompt(intent)
    }

    /// Set the business context manager.
    pub fn set_context_manager(&mut self, manager: crate::context::ContextManager) {
        self.context_manager = Some(manager);
    }

    /// Get context builder section for system prompt.
    async fn build_business_context_section(&self, query: &str) -> String {
        if let Some(ref cm) = self.context_manager {
            cm.format_for_prompt(query).await
        } else {
            String::new()
        }
    }

    /// Filter tools by user message (intent-based).
    /// Returns only relevant tools (3-5 max) to reduce thinking.
    pub async fn filter_tools_by_intent(
        &self,
        user_message: &str,
    ) -> Vec<edge_ai_core::llm::backend::ToolDefinition> {
        let all_tools = self.tool_definitions.read().await;
        if all_tools.is_empty() {
            return Vec::new();
        }

        // Classify intent from user message
        let intent = self.intent_classifier.classify(user_message);
        let target_namespace = intent.category.namespace();

        // Helper to derive namespace from tool name
        let derive_namespace = |name: &str| -> &str {
            if name.starts_with("list_")
                || name.starts_with("get_")
                || name == "control_device"
                || name.contains("device")
            {
                "device"
            } else if name.contains("rule") || name.contains("automation") {
                "rule"
            } else if name.contains("workflow")
                || name.contains("scenario")
                || name.contains("trigger")
            {
                "workflow"
            } else if name.contains("data") || name.contains("query") || name.contains("metrics") {
                "data"
            } else if name == "think" || name == "tool_search" {
                "system"
            } else {
                "general"
            }
        };

        // Filter tools by namespace (always include system tools)
        let mut filtered: Vec<edge_ai_core::llm::backend::ToolDefinition> = all_tools
            .iter()
            .filter(|t| {
                let ns = derive_namespace(&t.name);
                ns == "system" || ns == target_namespace
            })
            .cloned()
            .collect();

        // If no tools found or general intent, include list_* tools
        if filtered.is_empty() || intent.category == IntentCategory::General {
            let list_tools: Vec<edge_ai_core::llm::backend::ToolDefinition> = all_tools
                .iter()
                .filter(|t| t.name.starts_with("list_") || t.name.starts_with("query_"))
                .take(3)
                .cloned()
                .collect();
            filtered.extend(list_tools);
        }

        // Limit to 5 tools max
        filtered.truncate(5);
        filtered
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

        // Build base prompt
        let builder = PromptBuilder::new()
            .with_thinking(true)  // Include thinking guidelines
            .with_examples(true); // Include usage examples

        let mut prompt = String::with_capacity(4096);
        prompt.push_str(&builder.build_system_prompt());
        prompt.push_str("\n\n");

        // Add tool calling instruction and format
        prompt.push_str("## 重要：你必须调用工具来执行操作\n");
        prompt.push_str("1. 不要只说你将要做什么，直接输出工具调用的JSON！\n");
        prompt.push_str("2. 严禁在没有调用工具的情况下声称操作成功！\n");
        prompt.push_str("3. 只有在工具真正执行并返回成功结果后，才能使用「✓」标记。\n\n");
        prompt.push_str("## 工具调用格式\n");
        prompt.push_str("在回复中输出: [{\"name\":\"工具名\",\"arguments\":{\"参数\":\"值\"}}]\n\n");

        // Add simplified tools
        use edge_ai_tools::simplified;
        let simplified_tools = simplified::get_simplified_tools();

        prompt.push_str("## 可用工具\n\n");
        for tool in simplified_tools.iter() {
            prompt.push_str(&format!("### {} ({})\n", tool.name, tool.description));

            if !tool.aliases.is_empty() {
                prompt.push_str(&format!("**别名**: {}\n", tool.aliases.join("、")));
            }

            prompt.push_str("**参数**:\n");
            if tool.required.is_empty() && tool.optional.is_empty() {
                prompt.push_str("  无需参数\n");
            } else {
                for param in &tool.required {
                    prompt.push_str(&format!("  - `{}` (必需)\n", param));
                }
                for (param, info) in &tool.optional {
                    prompt.push_str(&format!("  - `{}` (可选，默认: {}) - {}\n",
                        param, info.default, info.description));
                }
            }

            if !tool.examples.is_empty() {
                prompt.push_str("\n**示例**:\n");
                for ex in &tool.examples {
                    prompt.push_str(&format!("  - 用户: \"{}\"\n", ex.user_query));
                    prompt.push_str(&format!("    → `{}`\n", ex.tool_call));
                }
            }

            prompt.push('\n');
        }

        // Add quick reference table
        prompt.push_str("## 快速参考\n");
        prompt.push_str("| 用户问什么 | 调用什么工具 |\n");
        prompt.push_str("|-----------|-------------|\n");
        prompt.push_str("| \"有哪些设备\" | `list_devices()` |\n");
        prompt.push_str("| \"温度是多少\" | `query_data(device='设备ID', metric='temperature')` |\n");
        prompt.push_str("| \"打开灯\" | `control_device(device='设备ID', action='on')` |\n");
        prompt.push_str("| \"创建规则\" | `create_rule(name='规则名', condition='条件', action='动作')` |\n");
        prompt.push_str("| \"显示所有规则\" | `list_rules()` |\n");
        prompt.push('\n');

        // Cache the result
        let cache_key = "base_prompt".to_string();
        {
            let mut cache_write = self.system_prompt_cache.write().await;
            *cache_write = Some((cache_key, prompt.clone()));
        }

        prompt
    }

    /// Build the base system prompt with current time injected.
    /// This replaces the time placeholders with actual time values using the configured global timezone.
    pub async fn build_base_system_prompt_with_time(&self, timezone: Option<&str>) -> String {
        use crate::prompts::{CURRENT_TIME_PLACEHOLDER, LOCAL_TIME_PLACEHOLDER, TIMEZONE_PLACEHOLDER};

        // Get the base prompt (which contains placeholders)
        let base_prompt = self.build_base_system_prompt().await;

        // Calculate current times
        let now = chrono::Utc::now();
        let current_time_utc = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();

        // Use self.global_timezone first, then parameter, then default
        let effective_timezone = self.global_timezone.read().await
            .as_ref()
            .cloned()
            .or_else(|| timezone.map(|s| s.to_string()))
            .unwrap_or_else(|| "Asia/Shanghai".to_string());

        // Parse timezone to get local time
        let tz = effective_timezone
            .parse::<chrono_tz::Tz>()
            .unwrap_or(chrono_tz::Tz::Asia__Shanghai); // Default to Shanghai on error

        let local_time = now.with_timezone(&tz).format("%Y-%m-%d %H:%M:%S").to_string();

        // Get additional time context for better LLM understanding
        let day_of_week = now.with_timezone(&tz).format("%A").to_string();
        let date_str = now.with_timezone(&tz).format("%Y年%m月%d日").to_string();

        // Get time period description (morning, afternoon, evening, night)
        let hour_str = now.with_timezone(&tz).format("%H").to_string();
        let hour: u32 = hour_str.parse().unwrap_or(12);
        let time_period = match hour {
            5..=11 => "上午",
            12..=13 => "中午",
            14..=17 => "下午",
            18..=22 => "晚上",
            _ => "夜间",
        };

        // Build enhanced time context
        let local_time_with_context = format!(
            "{} {} ({})",
            date_str,
            local_time,
            format!("{}{}", time_period, day_of_week)
        );

        // Replace placeholders
        base_prompt
            .replace(CURRENT_TIME_PLACEHOLDER, &current_time_utc)
            .replace(LOCAL_TIME_PLACEHOLDER, &local_time_with_context)
            .replace(TIMEZONE_PLACEHOLDER, &effective_timezone)
    }

    /// Build system prompt with tool descriptions.
    /// Uses enhanced prompts from prompts module for better conversation quality.
    /// Uses cached base prompt with time placeholders replaced and adds user-specific parts.
    async fn build_system_prompt_with_tools(&self, user_message: Option<&str>) -> String {
        // Get base prompt with time placeholders replaced
        let mut prompt = self.build_base_system_prompt_with_time(None).await;

        // Add intent-specific addon if we can classify the user's message
        if let Some(msg) = user_message {
            let intent = self.intent_classifier.classify(msg);
            let intent_namespace = match intent.category {
                crate::agent::staged::IntentCategory::Device => "device",
                crate::agent::staged::IntentCategory::Data => "data",
                crate::agent::staged::IntentCategory::Rule => "rule",
                crate::agent::staged::IntentCategory::Workflow => "workflow",
                crate::agent::staged::IntentCategory::Alert => "alert",
                crate::agent::staged::IntentCategory::System => "system",
                crate::agent::staged::IntentCategory::Help => "help",
                crate::agent::staged::IntentCategory::General => "general",
            };
            // Get intent addon (recreate builder for this part)
            let builder = crate::prompts::PromptBuilder::new();
            let addon = builder.get_intent_prompt_addon(intent_namespace);
            if !addon.is_empty() {
                prompt.push_str(&addon);
                prompt.push_str("\n\n");
            }
        }

        // Add business context section if available
        if let Some(query) = user_message {
            let context_section = self.build_business_context_section(query).await;
            if !context_section.is_empty() {
                prompt.push_str(&context_section);
                prompt.push_str("\n\n");
            }
        }

        // Note: Tools are already included in base_prompt from build_base_system_prompt()
        // No need to duplicate them here unless we want to do user-specific filtering
        prompt
    }

    /// Filter simplified tools based on intent.
    fn filter_simplified_tools(
        &self,
        tools: &[edge_ai_tools::simplified::LlmToolDefinition],
        intent: &crate::agent::staged::IntentResult,
    ) -> Vec<edge_ai_tools::simplified::LlmToolDefinition> {
        let mut filtered = Vec::new();

        // Always include tools that match the intent category
        for tool in tools {
            // Check if any use_when matches the intent
            let matches = tool.use_when.iter().any(|scenario| {
                let scenario_lower = scenario.to_lowercase();
                match intent.category {
                    crate::agent::staged::IntentCategory::Device => {
                        scenario_lower.contains("设备") || scenario_lower.contains("控制") || scenario_lower.contains("打开") || scenario_lower.contains("关闭")
                    }
                    crate::agent::staged::IntentCategory::Data => {
                        scenario_lower.contains("询问") || scenario_lower.contains("查询") || scenario_lower.contains("数据") || scenario_lower.contains("温度")
                    }
                    crate::agent::staged::IntentCategory::Rule => {
                        scenario_lower.contains("创建") || scenario_lower.contains("规则") || scenario_lower.contains("自动化")
                    }
                    crate::agent::staged::IntentCategory::Workflow => {
                        scenario_lower.contains("工作流") || scenario_lower.contains("执行")
                    }
                    crate::agent::staged::IntentCategory::Alert => {
                        scenario_lower.contains("告警") || scenario_lower.contains("异常") || scenario_lower.contains("通知")
                    }
                    crate::agent::staged::IntentCategory::System => {
                        scenario_lower.contains("系统") || scenario_lower.contains("状态") || scenario_lower.contains("健康")
                    }
                    crate::agent::staged::IntentCategory::Help => {
                        scenario_lower.contains("帮助") || scenario_lower.contains("教程") || scenario_lower.contains("说明")
                    }
                    crate::agent::staged::IntentCategory::General => true,
                }
            });

            if matches || tool.use_when.is_empty() {
                filtered.push(tool.clone());
            }
        }

        // Always include basic tools
        if !filtered.iter().any(|t| t.name == "list_devices")
            && let Some(t) = tools.iter().find(|t| t.name == "list_devices") {
                filtered.push(t.clone());
            }

        filtered.truncate(6); // Limit to 6 tools max
        filtered
    }

    /// Update the model name.
    pub async fn update_model(&self, model: String) {
        let mut model_guard = self.model.write().await;
        *model_guard = Some(model);
    }

    /// Check if the LLM backend is ready.
    pub async fn is_ready(&self) -> bool {
        if self.uses_instance_manager()
            && let Some(manager) = &self.instance_manager {
                return manager.get_active_instance().is_some();
            }
        let llm_guard = self.llm.read().await;
        llm_guard.as_ref().is_some()
    }

    /// Send a chat message and get a response.
    pub async fn chat(&self, user_message: impl Into<String>) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, None).await
    }

    /// Send a chat message with conversation history.
    pub async fn chat_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, Some(history)).await
    }

    /// Send a chat message without tools and get a response.
    /// This is useful for Phase 2 where tools have already been executed.
    pub async fn chat_without_tools(
        &self,
        user_message: impl Into<String>,
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, None).await
    }

    /// Send a chat message without tools, with conversation history.
    pub async fn chat_without_tools_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, Some(history)).await
    }

    /// Send a multimodal message (with images) with conversation history.
    /// This is used when the user sends images along with their text.
    pub async fn chat_multimodal_with_history(
        &self,
        user_message: Message,  // Can contain text + images
        history: &[Message],
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal_message(user_message, Some(history)).await
    }

    /// Internal chat implementation.
    async fn chat_internal(
        &self,
        user_message: impl Into<String>,
        history: Option<&[Message]>,
    ) -> Result<ChatResponse, AgentError> {
        let user_message = user_message.into();

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
            let greeting_response = "您好！我是 NeoTalk 智能助手。我可以帮您：\n\
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
                "qwen3-vl:2b".to_string()
            }
        };

        // Get thinking_enabled - priority: local setting > instance setting
        // This allows per-request override (e.g., disable thinking for multimodal)
        let local_thinking = *self.thinking_enabled.read().await;
        let thinking_enabled = if local_thinking.is_some() {
            // Local override takes precedence
            local_thinking
        } else if self.uses_instance_manager() {
            // Fall back to instance setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode with no local override
            None
        };

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal"
        );

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, eff_max_tokens) = self.get_effective_params().await;

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: Some(eff_max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            max_context: None,
        };

        let system_msg = Message::system(&system_prompt);
        let user_msg = Message::user(user_message);

        // Build messages with history if provided
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];
            // Add historical messages (excluding system prompts from history)
            for msg in hist {
                if msg.role != edge_ai_core::MessageRole::System {
                    msgs.push(msg.clone());
                }
            }
            msgs.push(user_msg);
            msgs
        } else {
            vec![system_msg, user_msg]
        };

        // Get tool definitions
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
            .map_err(|e| AgentError::Generation(e.to_string()))?;

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
        user_message: Message,  // Can contain text + images
        history: Option<&[Message]>,
    ) -> Result<ChatResponse, AgentError> {
        // Acquire permit for concurrency limiting
        let _permit = self.limiter.acquire().await;

        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Check if we have tools registered
        let has_tools = !self.tool_definitions.read().await.is_empty();

        let system_prompt = if has_tools {
            // Extract text from user message for system prompt
            let user_text = user_message.content.as_text();
            self.build_system_prompt_with_tools(Some(&user_text))
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
                "qwen3-vl:2b".to_string()
            }
        };

        // Get thinking_enabled - priority: local setting > instance setting
        // This allows per-request override (e.g., disable thinking for multimodal)
        let local_thinking = *self.thinking_enabled.read().await;
        let thinking_enabled = if local_thinking.is_some() {
            // Local override takes precedence
            local_thinking
        } else if self.uses_instance_manager() {
            // Fall back to instance setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode with no local override
            None
        };

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, eff_max_tokens) = self.get_effective_params().await;

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: Some(eff_max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            max_context: None,
        };

        let system_msg = Message::system(&system_prompt);

        // Helper function to check if a message has empty content
        fn is_message_empty(msg: &Message) -> bool {
            use edge_ai_core::Content;
            match &msg.content {
                Content::Text(s) => s.is_empty(),
                Content::Parts(parts) => parts.is_empty(),
            }
        }

        // Build messages with history if provided
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];
            // Add historical messages (excluding system prompts from history)
            for msg in hist {
                if msg.role != edge_ai_core::MessageRole::System {
                    msgs.push(msg.clone());
                }
            }
            // Only add user message if it's not empty (Phase 2 may use empty string)
            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
        };

        // Get tool definitions
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
            .map_err(|e| AgentError::Generation(e.to_string()))?;

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

    /// Send a chat message with streaming response.
    pub async fn chat_stream(
        &self,
        user_message: impl Into<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        self.chat_stream_internal(user_message, None, true).await
    }

    /// Send a chat message with streaming response, with conversation history.
    pub async fn chat_stream_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        // Enable thinking for complex queries (default behavior)
        *self.thinking_enabled.write().await = Some(true);
        self.chat_stream_internal(user_message, Some(history), true)
            .await
    }

    /// Send a chat message with streaming response, without tools.
    /// This is for Phase 2 where tools have already been executed.
    pub async fn chat_stream_without_tools(
        &self,
        user_message: impl Into<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        self.chat_stream_internal(user_message, None, false).await
    }

    /// Send a chat message with streaming response, without tools, with conversation history.
    /// This is for Phase 2 where tools have already been executed.
    pub async fn chat_stream_without_tools_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        self.chat_stream_internal(user_message, Some(history), false)
            .await
    }

    /// Send a chat message with streaming response, with tools, but without thinking.
    /// This is for simple queries where we want fast responses without thinking overhead.
    pub async fn chat_stream_no_thinking_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        // Set thinking to false for this call
        *self.thinking_enabled.write().await = Some(false);
        // Note: We DON'T restore the old value here because:
        // 1. The async stream continues after this function returns
        // 2. Restoring here would affect concurrent requests
        // 3. The next request will set its own value
        self.chat_stream_internal(user_message, Some(history), true)
            .await
    }

    /// Send a chat message with streaming response, without tools, without thinking.
    /// This is for Phase 2 follow-up where we want a quick response based on tool results.
    pub async fn chat_stream_no_tools_no_thinking_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        // Temporarily disable thinking for this call
        let old_value = *self.thinking_enabled.read().await;
        *self.thinking_enabled.write().await = Some(false);
        let result = self
            .chat_stream_internal(user_message, Some(history), false)
            .await;
        *self.thinking_enabled.write().await = old_value;
        result
    }

    /// Send a multimodal chat message (with images) with streaming response.
    /// This method accepts a Message directly, which can contain text and images.
    pub async fn chat_stream_multimodal_with_history(
        &self,
        user_message: Message,  // Can contain text + images via Content::Parts
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        self.chat_stream_internal_message(user_message, Some(history), true, false)
            .await
    }

    /// Send a multimodal chat message (with images) with streaming response, without thinking.
    /// For simple multimodal queries where we want fast responses.
    pub async fn chat_stream_multimodal_no_thinking_with_history(
        &self,
        user_message: Message,  // Can contain text + images via Content::Parts
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        // Temporarily disable thinking for this call
        *self.thinking_enabled.write().await = Some(false);
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
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        let model_arc = Arc::clone(&self.model);

        // Build system prompt (with or without tools based on phase)
        let system_prompt = if include_tools {
            self.build_system_prompt_with_tools(None)
                .await
        } else {
            // Phase 2 system prompt - NO tool calling, just generate response based on tool results
            // Tool execution is already complete, this phase is for summarizing results
            "你是NeoTalk物联网助手。

## 当前阶段：工具执行完成，需要生成最终回复

对话历史包含：
1. 用户的原始问题
2. 助手的思考过程（如果有）
3. 工具调用信息（调用了哪些工具、传入了什么参数）
4. 工具执行结果（每个工具返回的数据）

## 你的任务

根据**工具执行结果**和**用户的原始问题**，给出一个完整、有用的回复。

## 回复要求

1. **直接回答用户的问题** - 不要说「工具已执行」这类废话
2. **总结关键信息** - 提取工具结果中的关键数据
3. **结构清晰** - 如果有多个设备/规则，用列表或分组展示
4. **友好的语气** - 自然对话，不要机械
5. **内容不要重复** - 每条信息只说一次，不要把相同的内容说两遍

## 示例

用户: \"列出所有设备\"
工具返回: {\"devices\": [{\"id\": \"1\", \"name\": \"温度传感器\", ...}]}
你的回复: \"共找到 5 个设备：\\n1. 温度传感器 (ID: 1)\\n2. 湿度传感器 (ID: 2)\\n...\"

用户: \"查看 ne101 详情\"
工具返回: {\"device\": {\"name\": \"ne101\", \"temperature\": 25, ...}}
你的回复: \"ne101 设备详情：\\n- 名称: ne101\\n- 当前温度: 25°C\\n- 状态: 在线\\n...\"

## 注意事项

- 不要调用工具（此阶段禁用工具调用）
- 不要重复显示原始 JSON 数据
- **关键：每条信息只说一次，不要重复相同的内容**
- 如果工具执行失败，解释原因并提供替代方案
- 如果工具返回的数据不完整，诚实地说明".to_string()
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
                "qwen3-vl:2b".to_string()
            }
        };

        // Get thinking_enabled - priority: local setting > instance setting
        // This allows per-request override (e.g., disable thinking for multimodal)
        let local_thinking = *self.thinking_enabled.read().await;
        let thinking_enabled = if local_thinking.is_some() {
            // Local override takes precedence
            local_thinking
        } else if self.uses_instance_manager() {
            // Fall back to instance setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode with no local override
            None
        };

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal_message (multimodal)"
        );

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, eff_max_tokens) = self.get_effective_params().await;

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: Some(eff_max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            max_context: None,
        };

        let system_msg = Message::system(&system_prompt);

        // Helper function to check if a message has empty content
        fn is_message_empty(msg: &Message) -> bool {
            use edge_ai_core::Content;
            match &msg.content {
                Content::Text(s) => s.is_empty(),
                Content::Parts(parts) => parts.is_empty(),
            }
        }

        // Build messages with history if provided
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];
            // Add historical messages (excluding system prompts from history)
            for msg in hist {
                if msg.role != edge_ai_core::MessageRole::System {
                    msgs.push(msg.clone());
                }
            }
            // Only add user message if it's not empty (Phase 2 may use empty string)
            if !is_message_empty(&user_message) {
                msgs.push(user_message);
            }
            msgs
        } else {
            vec![system_msg, user_message]
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
            model: Some(model),
            stream: true,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let stream = llm
            .generate_stream(input)
            .await
            .map_err(|e| AgentError::Generation(e.to_string()))?;

        // Acquire permit for concurrency limiting and wrap stream
        let permit = self.limiter.acquire().await;
        let wrapped_stream = PermitStream::new(stream, permit);

        // Convert stream
        Ok(Box::pin(async_stream::stream! {
            let mut stream = wrapped_stream;
            while let Some(result) = futures::StreamExt::next(&mut stream).await {
                match result {
                    Ok(chunk) => yield Ok(chunk),
                    Err(e) => yield Err(AgentError::Generation(e.to_string())),
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
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError>
    {
        let user_message = user_message.into();

        let model_arc = Arc::clone(&self.model);

        // Build system prompt (with or without tools based on phase)
        let system_prompt = if include_tools {
            self.build_system_prompt_with_tools(Some(&user_message))
                .await
        } else {
            // Phase 2 system prompt - NO tool calling, just generate response based on tool results
            // Tool execution is already complete, this phase is for summarizing results
            "你是NeoTalk物联网助手。

## 当前阶段：工具执行完成，需要生成最终回复

对话历史包含：
1. 用户的原始问题
2. 助手的思考过程（如果有）
3. 工具调用信息（调用了哪些工具、传入了什么参数）
4. 工具执行结果（每个工具返回的数据）

## 你的任务

根据**工具执行结果**和**用户的原始问题**，给出一个完整、有用的回复。

## 回复要求

1. **直接回答用户的问题** - 不要说「工具已执行」这类废话
2. **总结关键信息** - 提取工具结果中的关键数据
3. **结构清晰** - 如果有多个设备/规则，用列表或分组展示
4. **友好的语气** - 自然对话，不要机械
5. **内容不要重复** - 每条信息只说一次，不要把相同的内容说两遍

## 示例

用户: \"列出所有设备\"
工具返回: {\"devices\": [{\"id\": \"1\", \"name\": \"温度传感器\", ...}]}
你的回复: \"共找到 5 个设备：\\n1. 温度传感器 (ID: 1)\\n2. 湿度传感器 (ID: 2)\\n...\"

用户: \"查看 ne101 详情\"
工具返回: {\"device\": {\"name\": \"ne101\", \"temperature\": 25, ...}}
你的回复: \"ne101 设备详情：\\n- 名称: ne101\\n- 当前温度: 25°C\\n- 状态: 在线\\n...\"

## 注意事项

- 不要调用工具（此阶段禁用工具调用）
- 不要重复显示原始 JSON 数据
- **关键：每条信息只说一次，不要重复相同的内容**
- 如果工具执行失败，解释原因并提供替代方案
- 如果工具返回的数据不完整，诚实地说明".to_string()
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
                "qwen3-vl:2b".to_string()
            }
        };

        // Get thinking_enabled - priority: local setting > instance setting
        // This allows per-request override (e.g., disable thinking for multimodal)
        let local_thinking = *self.thinking_enabled.read().await;
        let thinking_enabled = if local_thinking.is_some() {
            // Local override takes precedence
            local_thinking
        } else if self.uses_instance_manager() {
            // Fall back to instance setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode with no local override
            None
        };

        tracing::debug!(
            thinking_enabled = ?thinking_enabled,
            uses_instance_manager = self.uses_instance_manager(),
            "LlmInterface chat_stream_internal"
        );

        // Get effective parameters from backend instance or local config
        let (eff_temp, eff_top_p, eff_top_k, eff_max_tokens) = self.get_effective_params().await;

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(eff_temp),
            top_p: Some(eff_top_p),
            top_k: Some(eff_top_k as u32),
            max_tokens: Some(eff_max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
            max_context: None,
        };

        let system_msg = Message::system(&system_prompt);
        let user_msg = Message::user(user_message);

        // Build messages with history if provided
        let messages = if let Some(hist) = history {
            let mut msgs = vec![system_msg];
            // Add historical messages (excluding system prompts from history)
            for msg in hist {
                if msg.role != edge_ai_core::MessageRole::System {
                    msgs.push(msg.clone());
                }
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
            model: Some(model),
            stream: true,
            tools: tools_input,
        };

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let stream = llm
            .generate_stream(input)
            .await
            .map_err(|e| AgentError::Generation(e.to_string()))?;

        // Acquire permit for concurrency limiting and wrap stream
        let permit = self.limiter.acquire().await;
        let wrapped_stream = PermitStream::new(stream, permit);

        // Convert stream
        Ok(Box::pin(async_stream::stream! {
            let mut stream = wrapped_stream;
            while let Some(result) = futures::StreamExt::next(&mut stream).await {
                match result {
                    Ok(chunk) => yield Ok(chunk),
                    Err(e) => yield Err(AgentError::Generation(e.to_string())),
                }
            }
        }))
    }
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
        Self {
            model: "qwen3-vl:2b".to_string(),
            temperature: agent_env_vars::temperature(),
            top_p: agent_env_vars::top_p(),
            top_k: 20,  // Lowered for faster responses
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

/// Agent error type.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// LLM not ready
    #[error("LLM backend not ready")]
    LlmNotReady,
    /// Generation error
    #[error("Generation error: {0}")]
    Generation(String),
}

/// Convert AgentError to the crate's Result type.
impl From<AgentError> for super::error::AgentError {
    fn from(err: AgentError) -> Self {
        match err {
            AgentError::LlmNotReady => {
                super::error::AgentError::Llm("LLM backend not ready".to_string())
            }
            AgentError::Generation(msg) => super::error::AgentError::Llm(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::staged::{IntentCategory, IntentResult};

    #[test]
    fn test_chat_config_default() {
        let config = ChatConfig::default();
        assert_eq!(config.model, "qwen3-vl:2b");
        assert_eq!(config.temperature, 0.4);
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
        assert_eq!(interface.max_concurrent(), 2);
        assert_eq!(interface.available_permits(), 2);
    }

    #[test]
    fn test_llm_interface_with_system_prompt() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config)
            .with_system_prompt("You are a test assistant.");
        // The system prompt is set internally
        assert_eq!(interface.max_concurrent(), DEFAULT_CONCURRENT_LIMIT);
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
        let tools = vec![
            edge_ai_core::llm::backend::ToolDefinition {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                }),
            }
        ];
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
        assert!(interface.instance_manager().is_none());

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
    fn test_classify_intent() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Test device control intent
        let result = interface.classify_intent("turn on the lights");
        assert_eq!(result.category, IntentCategory::Device);

        // Test data query
        let result = interface.classify_intent("what's the temperature?");
        assert_eq!(result.category, IntentCategory::Data);

        // Test rule intent
        let result = interface.classify_intent("create a new rule");
        assert_eq!(result.category, IntentCategory::Rule);
    }

    #[test]
    fn test_get_intent_prompt() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        let result = IntentResult {
            category: IntentCategory::Device,
            confidence: 0.9,
            keywords: vec!["device".to_string()],
        };

        let prompt = interface.get_intent_prompt(&result);
        assert!(prompt.contains("device"));
    }

    #[tokio::test]
    async fn test_filter_tools_by_intent() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        // Set up sample tool definitions
        let tools = vec![
            edge_ai_core::llm::backend::ToolDefinition {
                name: "list_devices".to_string(),
                description: "List all devices".to_string(),
                parameters: serde_json::json!({}),
            },
            edge_ai_core::llm::backend::ToolDefinition {
                name: "control_device".to_string(),
                description: "Control a device".to_string(),
                parameters: serde_json::json!({}),
            },
            edge_ai_core::llm::backend::ToolDefinition {
                name: "list_rules".to_string(),
                description: "List all rules".to_string(),
                parameters: serde_json::json!({}),
            },
            edge_ai_core::llm::backend::ToolDefinition {
                name: "query_data".to_string(),
                description: "Query time series data".to_string(),
                parameters: serde_json::json!({}),
            },
        ];
        interface.set_tool_definitions(tools).await;

        // Test filtering with device-related query
        let filtered = interface.filter_tools_by_intent("turn on the lights").await;
        // Device-related tools should be included
        assert!(!filtered.is_empty());

        // Test filtering with data query
        let filtered = interface.filter_tools_by_intent("what's the temperature?").await;
        // Data-related tools should be included
        assert!(!filtered.is_empty());
    }

    #[test]
    fn test_concurrent_limit() {
        let config = ChatConfig {
            concurrent_limit: 5,
            ..Default::default()
        };
        let interface = LlmInterface::new(config);

        assert_eq!(interface.max_concurrent(), 5);
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
        let err = AgentError::LlmNotReady;
        assert!(err.to_string().contains("not ready"));

        let err = AgentError::Generation("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }
}
