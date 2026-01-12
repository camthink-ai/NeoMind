//! Abstract LLM runtime backend.
//!
//! This module defines the core abstraction for LLM inference,
//! supporting multiple backends (Hailo, Candle, Cloud, etc.).

use async_trait::async_trait;
use std::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use futures::Stream;

use super::modality::{ImageContent, ModalityContent};
use crate::message::{Message, MessageRole};

/// LLM backend identifier.
///
/// This is a dynamic string identifier instead of an enum,
/// allowing backends to be registered at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendId(String);

impl BackendId {
    /// Create a new backend ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Common backend IDs.
    pub const OLLAMA: &'static str = "ollama";
    pub const OPENAI: &'static str = "openai";
    pub const QWEN: &'static str = "qwen";
    pub const CANDLE: &'static str = "candle";
    pub const HAILO: &'static str = "hailo";
    pub const MOCK: &'static str = "mock";
}

impl AsRef<str> for BackendId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for BackendId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BackendId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<BackendType> for BackendId {
    fn from(backend_type: BackendType) -> Self {
        match backend_type {
            BackendType::Hailo => Self(BackendId::HAILO.to_string()),
            BackendType::Candle => Self(BackendId::CANDLE.to_string()),
            BackendType::Cloud => Self(BackendId::OPENAI.to_string()),
            BackendType::Remote => Self("remote".to_string()),
            BackendType::Mock => Self(BackendId::MOCK.to_string()),
        }
    }
}

impl From<&BackendType> for BackendId {
    fn from(backend_type: &BackendType) -> Self {
        match backend_type {
            BackendType::Hailo => Self(BackendId::HAILO.to_string()),
            BackendType::Candle => Self(BackendId::CANDLE.to_string()),
            BackendType::Cloud => Self(BackendId::OPENAI.to_string()),
            BackendType::Remote => Self("remote".to_string()),
            BackendType::Mock => Self(BackendId::MOCK.to_string()),
        }
    }
}

/// Legacy backend type enum for compatibility.
#[deprecated(note = "Use BackendId instead for dynamic backends")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendType {
    /// Hailo-10H NPU accelerator (production)
    Hailo,

    /// Candle CPU/GPU inference (development)
    Candle,

    /// Cloud LLM (OpenAI, Qwen, etc.)
    Cloud,

    /// Remote inference server (mistral.rs, vLLM, etc.)
    Remote,

    /// Mock backend for testing
    Mock,
}

impl BackendType {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "hailo" => Some(Self::Hailo),
            "candle" => Some(Self::Candle),
            "cloud" | "openai" | "qwen" => Some(Self::Cloud),
            "remote" | "mistral" => Some(Self::Remote),
            "mock" => Some(Self::Mock),
            _ => None,
        }
    }
}

/// Generation parameters.
#[derive(Debug, Clone)]
pub struct GenerationParams {
    /// Temperature (0.0 - 2.0)
    pub temperature: Option<f32>,

    /// Top-p sampling (0.0 - 1.0)
    pub top_p: Option<f32>,

    /// Top-k sampling
    pub top_k: Option<u32>,

    /// Maximum tokens to generate
    pub max_tokens: Option<usize>,

    /// Stop sequences
    pub stop: Option<Vec<String>>,

    /// Frequency penalty (-2.0 - 2.0)
    pub frequency_penalty: Option<f32>,

    /// Presence penalty (-2.0 - 2.0)
    pub presence_penalty: Option<f32>,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            max_tokens: Some(512),
            stop: None,
            frequency_penalty: Some(0.0),
            presence_penalty: Some(0.0),
        }
    }
}

/// LLM input.
#[derive(Debug, Clone)]
pub struct LlmInput {
    /// Messages for the conversation
    pub messages: Vec<Message>,

    /// Generation parameters
    pub params: GenerationParams,

    /// Model identifier (backend-specific)
    pub model: Option<String>,

    /// Stream response
    pub stream: bool,
}

impl LlmInput {
    /// Create a new input with a single user message.
    pub fn new(content: impl Into<ModalityContent>) -> Self {
        Self {
            messages: vec![Message::user(content.into().as_text())],
            params: GenerationParams::default(),
            model: None,
            stream: false,
        }
    }

    /// Add a message to the conversation.
    pub fn with_message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Add messages to the conversation.
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages.extend(messages);
        self
    }

    /// Set generation parameters.
    pub fn with_params(mut self, params: GenerationParams) -> Self {
        self.params = params;
        self
    }

    /// Set model identifier.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Enable streaming.
    pub fn with_streaming(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    /// Add multimodal content to user message.
    pub fn with_image(mut self, _image: ImageContent) -> Self {
        if let Some(msg) = self.messages.last_mut() {
            if msg.role == MessageRole::User {
                // Convert to multimodal content
                let text = msg.text();
                msg.content = crate::message::Content::text(format!(
                    "{} <image>",
                    if text.is_empty() { "Describe this image." } else { &text }
                ));
            }
        }
        self
    }
}

/// LLM output.
#[derive(Debug, Clone)]
pub struct LlmOutput {
    /// Generated text content
    pub text: String,

    /// Finish reason (stop, length, error)
    pub finish_reason: FinishReason,

    /// Tokens used (prompt + completion)
    pub usage: Option<TokenUsage>,
}

/// Finish reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    /// Model stopped naturally
    Stop,

    /// Max tokens reached
    Length,

    /// Model hit an error
    Error,

    /// Content filter triggered
    ContentFilter,
}

/// Token usage statistics.
#[derive(Debug, Clone, Copy)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

/// Stream chunk.
/// Contains the text content and a boolean indicating if it's from a "thinking" field
/// (e.g., qwen3-vl's thinking field vs actual content).
pub type StreamChunk = Result<(String, bool), LlmError>;

/// LLM runtime error.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Backend not available
    #[error("Backend {0} not available")]
    BackendUnavailable(String),

    /// Model not found
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Generation error
    #[error("Generation error: {0}")]
    Generation(String),

    /// Network error (for cloud/remote backends)
    #[error("Network error: {0}")]
    Network(String),

    /// Timeout
    #[error("Operation timed out after {0}s")]
    Timeout(u64),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Unknown error
    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Factory for creating LLM runtime backends.
///
/// This trait allows for dynamic backend registration and instantiation.
#[async_trait]
pub trait BackendFactory: Send + Sync {
    /// Get the unique identifier for this backend type.
    fn backend_id(&self) -> &str;

    /// Get a human-readable name for this backend.
    fn display_name(&self) -> &str;

    /// Create a new backend instance from configuration.
    ///
    /// The config is a JSON value that allows flexible configuration
    /// without requiring changes to core types.
    fn create(&self, config: &serde_json::Value) -> Result<Box<dyn LlmRuntime>, LlmError>;

    /// Validate backend configuration before creation.
    fn validate_config(&self, config: &serde_json::Value) -> Result<(), LlmError>;

    /// Get the default configuration for this backend.
    fn default_config(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    /// Check if this backend is available on the current system.
    async fn is_available(&self) -> bool {
        true
    }
}

/// Registry for LLM backend factories.
///
/// This registry allows dynamic registration and instantiation of backends.
pub struct BackendRegistry {
    factories: HashMap<String, Box<dyn BackendFactory>>,
}

impl BackendRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a backend factory.
    pub fn register(&mut self, factory: Box<dyn BackendFactory>) {
        let id = factory.backend_id().to_string();
        self.factories.insert(id, factory);
    }

    /// Get a backend factory by ID.
    pub fn get_factory(&self, id: &str) -> Option<&dyn BackendFactory> {
        self.factories.get(id).map(|f| f.as_ref())
    }

    /// List all registered backend IDs.
    pub fn list_backends(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }

    /// Create a backend instance from configuration.
    ///
    /// The configuration should have a "backend" field specifying the backend type.
    pub fn create_backend(
        &self,
        config: &serde_json::Value,
    ) -> Result<Box<dyn LlmRuntime>, LlmError> {
        let backend_id = config.get("backend")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::InvalidInput("Missing 'backend' field in config".into()))?;

        let factory = self.get_factory(backend_id)
            .ok_or_else(|| LlmError::BackendUnavailable(backend_id.to_string()))?;

        factory.validate_config(config)?;
        factory.create(config)
    }

    /// Find the best available backend for given requirements.
    pub fn find_best_backend(&self, requirements: &BackendRequirements) -> Option<String> {
        for id in self.factories.keys() {
            if let Ok(true) = self.meets_requirements(id, requirements) {
                return Some(id.clone());
            }
        }
        None
    }

    /// Check if a backend meets the given requirements.
    fn meets_requirements(&self, backend_id: &str, req: &BackendRequirements) -> Result<bool, LlmError> {
        let factory = self.get_factory(backend_id)
            .ok_or_else(|| LlmError::BackendUnavailable(backend_id.to_string()))?;

        // Create a temp instance to check capabilities
        let config = factory.default_config();
        if let Ok(runtime) = factory.create(&config) {
            let caps = runtime.capabilities();

            if req.streaming && !caps.streaming {
                return Ok(false);
            }
            if req.multimodal && !caps.multimodal {
                return Ok(false);
            }
            if req.function_calling && !caps.function_calling {
                return Ok(false);
            }
            if let Some(min_context) = req.min_context {
                if let Some(max_context) = caps.max_context {
                    if max_context < min_context {
                        return Ok(false);
                    }
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global backend registry singleton.
static GLOBAL_REGISTRY: once_cell::sync::Lazy<Arc<RwLock<BackendRegistry>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(BackendRegistry::new())));

/// Get the global backend registry.
pub fn global_registry() -> Arc<RwLock<BackendRegistry>> {
    Arc::clone(&GLOBAL_REGISTRY)
}

/// Register a backend factory with the global registry.
pub fn register_backend(factory: Box<dyn BackendFactory>) {
    let mut registry = GLOBAL_REGISTRY.write().unwrap();
    registry.register(factory);
}

/// Requirements for backend selection.
#[derive(Debug, Clone, Default)]
pub struct BackendRequirements {
    /// Requires streaming support
    pub streaming: bool,

    /// Requires multimodal support
    pub multimodal: bool,

    /// Requires function calling support
    pub function_calling: bool,

    /// Minimum context length
    pub min_context: Option<usize>,

    /// Required capabilities
    pub required_capabilities: Vec<String>,
}

/// Metrics for LLM backend operations.
#[derive(Debug, Clone, Default)]
pub struct BackendMetrics {
    /// Total number of requests
    pub total_requests: u64,

    /// Successful requests
    pub successful_requests: u64,

    /// Failed requests
    pub failed_requests: u64,

    /// Total tokens generated
    pub total_tokens: u64,

    /// Average latency in milliseconds
    pub avg_latency_ms: f64,

    /// Last request timestamp
    pub last_request: Option<std::time::SystemTime>,
}

impl BackendMetrics {
    /// Record a successful request.
    pub fn record_success(&mut self, tokens: u64, latency_ms: u64) {
        self.total_requests += 1;
        self.successful_requests += 1;
        self.total_tokens += tokens;
        self.avg_latency_ms = (self.avg_latency_ms * (self.total_requests - 1) as f64
            + latency_ms as f64) / self.total_requests as f64;
        self.last_request = Some(std::time::SystemTime::now());
    }

    /// Record a failed request.
    pub fn record_failure(&mut self) {
        self.total_requests += 1;
        self.failed_requests += 1;
        self.last_request = Some(std::time::SystemTime::now());
    }

    /// Get success rate (0.0 to 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 1.0;
        }
        self.successful_requests as f64 / self.total_requests as f64
    }
}

/// Abstract LLM runtime backend.
#[async_trait::async_trait]
pub trait LlmRuntime: Send + Sync {
    /// Get the backend type identifier.
    fn backend_id(&self) -> BackendId;

    /// Get the current model name.
    fn model_name(&self) -> &str;

    /// Check if the backend is available.
    async fn is_available(&self) -> bool {
        true
    }

    /// Generate a response (non-streaming).
    async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError>;

    /// Generate a response (streaming).
    async fn generate_stream(
        &self,
        input: LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError>;

    /// Get max context length.
    fn max_context_length(&self) -> usize;

    /// Estimate token count (approximate).
    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough estimate: ~4 chars per token
        text.len() / 4
    }

    /// Check if multimodal (vision) is supported.
    fn supports_multimodal(&self) -> bool {
        false
    }

    /// Get backend capabilities.
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }

    /// Get backend metrics (if supported).
    ///
    /// This allows backends to optionally provide runtime metrics
    /// such as request counts, latencies, etc.
    fn metrics(&self) -> BackendMetrics {
        BackendMetrics::default()
    }
}

/// Backend capabilities.
#[derive(Debug, Clone, Default)]
pub struct BackendCapabilities {
    /// Supports streaming generation
    pub streaming: bool,

    /// Supports multimodal (vision)
    pub multimodal: bool,

    /// Supports function calling
    pub function_calling: bool,

    /// Supports multiple models
    pub multiple_models: bool,

    /// Maximum context length
    pub max_context: Option<usize>,

    /// Supported modalities
    pub modalities: Vec<String>,

    /// Supports thinking/reasoning display
    pub thinking_display: bool,

    /// Supports image input
    pub supports_images: bool,

    /// Supports audio input
    pub supports_audio: bool,
}

impl BackendCapabilities {
    /// Create a new builder for capabilities.
    pub fn builder() -> BackendCapabilitiesBuilder {
        BackendCapabilitiesBuilder::new()
    }

    /// Check if all specified capabilities are supported.
    pub fn supports_all(&self, capabilities: &[&str]) -> bool {
        capabilities.iter().all(|cap| self.modalities.contains(&cap.to_string()))
    }

    /// Check if any of the specified capabilities are supported.
    pub fn supports_any(&self, capabilities: &[&str]) -> bool {
        capabilities.iter().any(|cap| self.modalities.contains(&cap.to_string()))
    }

    /// Add a capability.
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.modalities.push(capability.into());
        self
    }

    /// Set streaming support.
    pub fn with_streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Set multimodal support.
    pub fn with_multimodal(mut self, multimodal: bool) -> Self {
        self.multimodal = multimodal;
        self
    }

    /// Set function calling support.
    pub fn with_function_calling(mut self, function_calling: bool) -> Self {
        self.function_calling = function_calling;
        self
    }

    /// Set max context length.
    pub fn with_max_context(mut self, max_context: usize) -> Self {
        self.max_context = Some(max_context);
        self
    }
}

/// Builder for BackendCapabilities.
pub struct BackendCapabilitiesBuilder {
    capabilities: BackendCapabilities,
}

impl BackendCapabilitiesBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            capabilities: BackendCapabilities::default(),
        }
    }

    /// Enable streaming.
    pub fn streaming(mut self) -> Self {
        self.capabilities.streaming = true;
        self
    }

    /// Enable multimodal.
    pub fn multimodal(mut self) -> Self {
        self.capabilities.multimodal = true;
        self
    }

    /// Enable function calling.
    pub fn function_calling(mut self) -> Self {
        self.capabilities.function_calling = true;
        self
    }

    /// Set max context.
    pub fn max_context(mut self, max: usize) -> Self {
        self.capabilities.max_context = Some(max);
        self
    }

    /// Add a supported modality.
    pub fn modality(mut self, modality: impl Into<String>) -> Self {
        self.capabilities.modalities.push(modality.into());
        self
    }

    /// Enable thinking display.
    pub fn thinking_display(mut self) -> Self {
        self.capabilities.thinking_display = true;
        self
    }

    /// Build the capabilities.
    pub fn build(self) -> BackendCapabilities {
        self.capabilities
    }
}

/// Dynamic LLM runtime that can switch between backends.
pub struct DynamicLlmRuntime {
    backends: std::collections::HashMap<String, Box<dyn LlmRuntime>>,
    default_backend: String,
}

impl DynamicLlmRuntime {
    /// Create a new dynamic runtime.
    pub fn new(default_backend: impl Into<String>) -> Self {
        Self {
            backends: std::collections::HashMap::new(),
            default_backend: default_backend.into(),
        }
    }

    /// Add a backend.
    pub fn add_backend(&mut self, backend: Box<dyn LlmRuntime>) {
        let backend_id = backend.backend_id().as_str().to_string();
        self.backends.insert(backend_id, backend);
    }

    /// Get a backend by ID.
    pub fn get_backend(&self, backend_id: &str) -> Option<&dyn LlmRuntime> {
        self.backends.get(backend_id).map(|b| b.as_ref())
    }

    /// Get the default backend.
    pub fn default_backend(&self) -> Option<&dyn LlmRuntime> {
        self.get_backend(&self.default_backend)
    }

    /// Set the default backend.
    pub fn set_default_backend(&mut self, backend_id: impl Into<String>) {
        self.default_backend = backend_id.into();
    }

    /// Get the first available backend.
    pub fn first_available(&self) -> Option<(&str, &dyn LlmRuntime)> {
        for (backend_id, backend) in &self.backends {
            return Some((backend_id.as_str(), backend.as_ref()));
        }
        None
    }
}

#[async_trait::async_trait]
impl LlmRuntime for DynamicLlmRuntime {
    fn backend_id(&self) -> BackendId {
        BackendId::new(self.default_backend.clone())
    }

    fn model_name(&self) -> &str {
        self.default_backend()
            .map(|b| b.model_name())
            .unwrap_or("none")
    }

    async fn is_available(&self) -> bool {
        if let Some(backend) = self.default_backend() {
            backend.is_available().await
        } else {
            false
        }
    }

    async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
        let backend = self.default_backend()
            .ok_or_else(|| LlmError::BackendUnavailable(format!("{:?}", self.default_backend)))?;
        backend.generate(input).await
    }

    async fn generate_stream(
        &self,
        input: LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        let backend = self.default_backend()
            .ok_or_else(|| LlmError::BackendUnavailable(format!("{:?}", self.default_backend)))?;
        backend.generate_stream(input).await
    }

    fn max_context_length(&self) -> usize {
        self.default_backend()
            .map(|b| b.max_context_length())
            .unwrap_or(0)
    }

    fn supports_multimodal(&self) -> bool {
        self.default_backend()
            .map(|b| b.supports_multimodal())
            .unwrap_or(false)
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.default_backend()
            .map(|b| b.capabilities())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_from_str() {
        assert_eq!(BackendType::from_str("hailo"), Some(BackendType::Hailo));
        assert_eq!(BackendType::from_str("candle"), Some(BackendType::Candle));
        assert_eq!(BackendType::from_str("cloud"), Some(BackendType::Cloud));
        assert_eq!(BackendType::from_str("openai"), Some(BackendType::Cloud));
        assert_eq!(BackendType::from_str("unknown"), None);
    }

    #[test]
    fn test_llm_input_builder() {
        let input = LlmInput::new("Hello")
            .with_model("qwen2")
            .with_streaming(true);

        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.model.as_deref(), Some("qwen2"));
        assert!(input.stream);
    }
}
