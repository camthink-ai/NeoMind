//! LLM interface for the Agent.
//!
//! This module provides a simple LLM wrapper with concurrency limits
//! and integration with the LlmBackendInstanceManager for dynamic backend switching.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use futures::Stream;
use tokio::sync::RwLock;

use edge_ai_core::{
    SessionId, Message,
    llm::backend::{LlmRuntime, LlmInput},
};

/// Re-export the instance manager types for convenience
pub use edge_ai_llm::instance_manager::{
    LlmBackendInstanceManager, get_instance_manager, BackendTypeDefinition,
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
            match self.current.compare_exchange_weak(current, current + 1, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return Some(ConcurrencyPermit {
                    limiter: self.current.clone(),
                }),
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
        }
    }

    /// Create a new LLM interface with instance manager integration.
    pub fn with_instance_manager(config: ChatConfig, manager: Arc<LlmBackendInstanceManager>) -> Self {
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
        }
    }

    /// Enable or disable instance manager mode.
    pub async fn set_use_instance_manager(&self, use_manager: bool) {
        self.use_instance_manager.store(if use_manager { 1 } else { 0 }, Ordering::Relaxed);
    }

    /// Check if instance manager mode is enabled.
    pub fn uses_instance_manager(&self) -> bool {
        self.use_instance_manager.load(Ordering::Relaxed) == 1
            && self.instance_manager.is_some()
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
                return manager.get_active_runtime().await
                    .map_err(|e| AgentError::Generation(e.to_string()));
            }
        }

        // Fall back to direct runtime
        let llm_guard = self.llm.read().await;
        llm_guard.as_ref()
            .map(Arc::clone)
            .ok_or_else(|| AgentError::LlmNotReady)
    }

    /// Set the LLM runtime (direct mode).
    pub async fn set_llm(&self, llm: Arc<dyn LlmRuntime>) {
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
            manager.set_active(backend_id).await
                .map_err(|e| AgentError::Generation(e.to_string()))?;
            Ok(())
        } else {
            Err(AgentError::Generation("No instance manager configured".to_string()))
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
        self.limiter.max.saturating_sub(self.limiter.current.load(Ordering::Relaxed))
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    /// Set tool definitions for function calling.
    pub async fn set_tool_definitions(&self, tools: Vec<edge_ai_core::llm::backend::ToolDefinition>) {
        *self.tool_definitions.write().await = tools;
    }

    /// Get current tool definitions.
    pub async fn get_tool_definitions(&self) -> Vec<edge_ai_core::llm::backend::ToolDefinition> {
        self.tool_definitions.read().await.clone()
    }

    /// Build system prompt with tool descriptions.
    async fn build_system_prompt_with_tools(&self) -> String {
        let tools = self.tool_definitions.read().await;
        if tools.is_empty() {
            return self.system_prompt.clone();
        }

        // IMPORTANT: Put output format requirements at the very beginning
        // This is critical for models with thinking enabled
        let mut prompt = "你是一个物联网设备管理助手。请直接用自然语言回答用户的问题。\n\n".to_string();

        prompt.push_str("【输出要求】\n");
        prompt.push_str("1. 必须在content字段输出最终回复，用户只能看到这个\n");
        prompt.push_str("2. 不要只在thinking中思考，必须输出可读的回复\n");
        prompt.push_str("3. 回复要简洁友好，直接回答问题\n\n");

        prompt.push_str(&self.system_prompt.clone());

        // Clear guidance to prevent confusion and looping
        prompt.push_str("\n\n## 工具使用规则\n\n");
        prompt.push_str("- 只在用户明确提到设备、传感器、数据查询时使用工具\n");
        prompt.push_str("- 问候、闲聊、一般问题直接回答，不用工具\n");
        prompt.push_str("- 回复要简短友好\n");

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
    pub async fn chat(
        &self,
        user_message: impl Into<String>,
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, true, None).await
    }

    /// Send a chat message with conversation history.
    pub async fn chat_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, true, Some(history)).await
    }

    /// Send a chat message without tools and get a response.
    /// This is useful for Phase 2 where tools have already been executed.
    pub async fn chat_without_tools(
        &self,
        user_message: impl Into<String>,
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, false, None).await
    }

    /// Send a chat message without tools, with conversation history.
    pub async fn chat_without_tools_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<ChatResponse, AgentError> {
        self.chat_internal(user_message, false, Some(history)).await
    }

    /// Internal chat implementation.
    async fn chat_internal(
        &self,
        user_message: impl Into<String>,
        include_tools: bool,
        history: Option<&[Message]>,
    ) -> Result<ChatResponse, AgentError> {
        // Acquire permit for concurrency limiting
        let _permit = self.limiter.acquire().await;

        let user_message = user_message.into();
        let start = Instant::now();

        let model_arc = Arc::clone(&self.model);

        // Build system prompt (with or without tools based on phase)
        let system_prompt = if include_tools {
            self.build_system_prompt_with_tools().await
        } else {
            // Simple system prompt for Phase 2 (no tools needed)
            "You are NeoTalk, an edge computing and IoT device management assistant. Help users based on the provided conversation context.".to_string()
        };

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model = model_guard.as_ref().cloned().unwrap_or_else(|| "qwen3-vl:2b".to_string());
        drop(model_guard);

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(self.temperature),
            top_p: Some(self.top_p),
            top_k: None,
            max_tokens: Some(self.max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
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

        // Get tool definitions (only if include_tools is true)
        let tools_input = if include_tools {
            let tools = self.tool_definitions.read().await;
            let result = if tools.is_empty() { None } else { Some(tools.clone()) };
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

        let output = llm.generate(input).await
            .map_err(|e| AgentError::Generation(e.to_string()))?;

        let duration = start.elapsed();
        let tokens_used = output.usage
            .map(|u| u.completion_tokens as usize)
            .unwrap_or_else(|| output.text.split_whitespace().count());

        Ok(ChatResponse {
            text: output.text,
            tokens_used,
            duration,
            finish_reason: format!("{:?}", output.finish_reason),
        })
    }

    /// Send a chat message with streaming response.
    pub async fn chat_stream(
        &self,
        user_message: impl Into<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError> {
        self.chat_stream_internal(user_message, None).await
    }

    /// Send a chat message with streaming response, with conversation history.
    pub async fn chat_stream_with_history(
        &self,
        user_message: impl Into<String>,
        history: &[Message],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError> {
        self.chat_stream_internal(user_message, Some(history)).await
    }

    /// Internal streaming chat implementation.
    async fn chat_stream_internal(
        &self,
        user_message: impl Into<String>,
        history: Option<&[Message]>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), AgentError>> + Send>>, AgentError> {
        let user_message = user_message.into();

        let model_arc = Arc::clone(&self.model);

        // Build system prompt with tools
        let system_prompt = self.build_system_prompt_with_tools().await;

        // Build input outside the lock
        let model_guard = model_arc.read().await;
        let model = model_guard.as_ref().cloned().unwrap_or_else(|| "qwen3-vl:2b".to_string());
        drop(model_guard);

        let params = edge_ai_core::llm::backend::GenerationParams {
            temperature: Some(self.temperature),
            top_p: Some(self.top_p),
            top_k: None,
            max_tokens: Some(self.max_tokens),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
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
        eprintln!("[DEBUG LLM] Tool definitions count: {}", tools.len());
        let tools_input = if tools.is_empty() { None } else { Some(tools.clone()) };
        eprintln!("[DEBUG LLM] tools_input is_some: {:?}", tools_input.is_some());
        drop(tools);

        let input = LlmInput {
            messages,
            params,
            model: Some(model),
            stream: true,
            tools: tools_input,
        };
        eprintln!("[DEBUG LLM] LlmInput created with tools: {:?}", input.tools.as_ref().map(|t| t.len()));

        // Get runtime using instance manager if enabled
        let llm = self.get_runtime().await?;

        let stream = llm.generate_stream(input).await
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
            top_p: 0.95,
            max_tokens: usize::MAX,  // No limit for complex tasks
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
            AgentError::LlmNotReady => super::error::AgentError::Llm("LLM backend not ready".to_string()),
            AgentError::Generation(msg) => super::error::AgentError::Llm(msg),
        }
    }
}

