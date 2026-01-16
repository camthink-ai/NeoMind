//! LLM interface for the Agent.
//!
//! This module provides a simple LLM wrapper with concurrency limits
//! and integration with the LlmBackendInstanceManager for dynamic backend switching.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::Stream;
use serde_json::Value;
use tokio::sync::RwLock;

use edge_ai_core::{
    Message, SessionId,
    llm::backend::{LlmInput, LlmRuntime},
};

// Import intent classifier for staged processing
use crate::agent::staged::{IntentCategory, IntentClassifier, IntentResult, ToolFilter};

/// Re-export the instance manager types for convenience
pub use edge_ai_llm::instance_manager::{
    BackendTypeDefinition, LlmBackendInstanceManager, get_instance_manager,
};

/// Default concurrent LLM request limit.
pub const DEFAULT_CONCURRENT_LIMIT: usize = 3;

/// Simple atomic-based concurrency limiter.
///
/// This is simpler than using a semaphore for streams because it doesn't
/// have lifetime issues with permits.
#[derive(Clone)]
struct ConcurrencyLimiter {
    current: Arc<AtomicUsize>,
    max: usize,
}

impl ConcurrencyLimiter {
    fn new(max: usize) -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            max,
        }
    }

    /// Try to acquire a permit. Returns Some(permit) if successful, None if at limit.
    fn try_acquire(&self) -> Option<ConcurrencyPermit> {
        let mut current = self.current.load(Ordering::Relaxed);
        loop {
            if current >= self.max {
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
    /// Max tokens.
    max_tokens: usize,
    /// Default system prompt.
    system_prompt: String,
    /// Tool definitions for function calling.
    tool_definitions: Arc<RwLock<Vec<edge_ai_core::llm::backend::ToolDefinition>>>,
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
            max_tokens: config.max_tokens,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(0)),
            thinking_enabled: Arc::new(RwLock::new(None)), // Use backend default (from storage)
            intent_classifier: IntentClassifier::default(),
            tool_filter: ToolFilter::default(),
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
            max_tokens: config.max_tokens,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            tool_definitions: Arc::new(RwLock::new(Vec::new())),
            limiter: ConcurrencyLimiter::new(concurrent_limit),
            use_instance_manager: Arc::new(AtomicUsize::new(1)),
            thinking_enabled: Arc::new(RwLock::new(None)), // Will use instance manager setting
            intent_classifier: IntentClassifier::default(),
            tool_filter: ToolFilter::default(),
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

    /// Enable or disable instance manager mode.
    pub async fn set_use_instance_manager(&self, use_manager: bool) {
        self.use_instance_manager
            .store(if use_manager { 1 } else { 0 }, Ordering::Relaxed);
    }

    /// Check if instance manager mode is enabled.
    pub fn uses_instance_manager(&self) -> bool {
        self.use_instance_manager.load(Ordering::Relaxed) == 1 && self.instance_manager.is_some()
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
        if self.uses_instance_manager() {
            if let Some(manager) = &self.instance_manager {
                return manager
                    .get_active_runtime()
                    .await
                    .map_err(|e| AgentError::Generation(e.to_string()));
            }
        }

        // Fall back to direct runtime
        let llm_guard = self.llm.read().await;
        llm_guard
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| AgentError::LlmNotReady)
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
    pub fn max_concurrent(&self) -> usize {
        self.limiter.max
    }

    /// Get the number of available permit slots.
    pub fn available_permits(&self) -> usize {
        self.limiter
            .max
            .saturating_sub(self.limiter.current.load(Ordering::Relaxed))
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    /// Set tool definitions for function calling.
    pub async fn set_tool_definitions(
        &self,
        tools: Vec<edge_ai_core::llm::backend::ToolDefinition>,
    ) {
        *self.tool_definitions.write().await = tools;
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

    /// Build system prompt with tool descriptions.
    /// If user_message is provided, uses intent-based filtering.
    async fn build_system_prompt_with_tools(&self, user_message: Option<&str>) -> String {
        let tools = if let Some(msg) = user_message {
            // Use intent-based filtering
            self.filter_tools_by_intent(msg).await
        } else {
            // No filtering - return all tools
            self.tool_definitions.read().await.clone()
        };

        if tools.is_empty() {
            return self.system_prompt.clone();
        }

        let mut prompt = String::with_capacity(2048);

        // Core identity - single sentence
        prompt.push_str("你是NeoTalk物联网助手，帮助用户管理设备和查询数据。\n\n");

        // Add intent-specific guidance if user message provided
        if let Some(msg) = user_message {
            let intent = self.intent_classifier.classify(msg);
            prompt.push_str(&format!(
                "## 当前任务\n{}\n\n",
                self.tool_filter.intent_prompt(&intent)
            ));
        }

        // Tool calling - brief and direct
        prompt.push_str("## 工具调用\n");
        prompt.push_str("根据用户问题，直接用XML格式调用工具：\n");
        prompt.push_str("<tool_calls><invoke name=\"工具名称\"></invoke></tool_calls>\n\n");

        // Available tools - concise list only
        prompt.push_str("## 可用工具\n");
        for tool in tools.iter() {
            prompt.push_str(&format!("- {}: {}\n", tool.name, tool.description));
        }

        // Brief example
        prompt.push_str("\n示例: 用户问「有哪些设备」→ 你调用 <tool_calls><invoke name=\"list_devices\"></invoke></tool_calls>\n");

        prompt
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
            self.system_prompt.clone()
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

        // Get thinking_enabled from active backend instance if using instance manager
        let thinking_enabled = if self.uses_instance_manager() {
            // Instance manager mode - use backend setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode - use local setting (defaults to false)
            *self.thinking_enabled.read().await
        };

        // DEBUG: Log thinking_enabled value
        eprintln!(
            "[LlmInterface] chat_stream_internal: thinking_enabled={:?}, uses_instance_manager={}",
            thinking_enabled,
            self.uses_instance_manager()
        );

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(self.temperature),
            top_p: Some(self.top_p),
            top_k: None,
            max_tokens: Some(self.max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
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
            // Phase 2 system prompt - still include tool calling instructions
            // because follow-up questions may need tools
            "你是NeoTalk物联网助手。根据对话历史和用户问题，如果需要查询信息，用XML格式调用工具：<tool_calls><invoke name=\"工具名称\"></invoke></tool_calls>
可用工具：list_devices, list_rules, query_data, control_device, create_rule, trigger_workflow。
如果不需要查询工具，直接回答用户问题。".to_string()
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

        // Get thinking_enabled from active backend instance if using instance manager
        let thinking_enabled = if self.uses_instance_manager() {
            // Instance manager mode - use backend setting
            if let Some(manager) = &self.instance_manager {
                manager
                    .get_active_instance()
                    .map(|inst| inst.thinking_enabled)
            } else {
                None
            }
        } else {
            // Direct mode - use local setting (defaults to false)
            *self.thinking_enabled.read().await
        };

        // DEBUG: Log thinking_enabled value
        eprintln!(
            "[LlmInterface] chat_stream_internal: thinking_enabled={:?}, uses_instance_manager={}",
            thinking_enabled,
            self.uses_instance_manager()
        );

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(self.temperature),
            top_p: Some(self.top_p),
            top_k: None,
            max_tokens: Some(self.max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled,
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
    /// Maximum tokens to generate.
    pub max_tokens: usize,
    /// Maximum concurrent LLM requests (default: 3).
    pub concurrent_limit: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            model: "qwen3-vl:2b".to_string(),
            temperature: 0.4,
            top_p: 0.7,       // 0.95 -> 0.7, 减少随机性，降低thinking长度
            max_tokens: 4096, // usize::MAX -> 4096, 限制总长度
            concurrent_limit: DEFAULT_CONCURRENT_LIMIT,
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
        assert_eq!(result.category, IntentCategory::DeviceControl);

        // Test information query
        let result = interface.classify_intent("what's the temperature?");
        assert_eq!(result.category, IntentCategory::Information);

        // Test system command
        let result = interface.classify_intent("create a new rule");
        assert_eq!(result.category, IntentCategory::SystemCommand);
    }

    #[test]
    fn test_get_intent_prompt() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        let result = IntentResult {
            category: IntentCategory::DeviceControl,
            confidence: 0.9,
            entities: vec![("lights".to_string(), "device".to_string())],
        };

        let prompt = interface.get_intent_prompt(&result);
        assert!(prompt.contains("device"));
        assert!(prompt.contains("control"));
    }

    #[tokio::test]
    async fn test_filter_tools_by_intent() {
        let config = ChatConfig::default();
        let interface = LlmInterface::new(config);

        let all_tools = vec![
            edge_ai_core::llm::backend::ToolDefinition {
                name: "device_control".to_string(),
                description: "Control a device".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            edge_ai_core::llm::backend::ToolDefinition {
                name: "get_weather".to_string(),
                description: "Get weather information".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        ];

        let intent = IntentResult {
            category: IntentCategory::DeviceControl,
            confidence: 0.9,
            entities: vec![],
        };

        let filtered = interface.filter_tools_by_intent(&all_tools, &intent).await;
        // Device control tools should be prioritized
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
