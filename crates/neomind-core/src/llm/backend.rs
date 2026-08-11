//! Abstract LLM runtime backend.
//!
//! This module defines the core abstraction for LLM inference,
//! supporting multiple backends (Hailo, Candle, Cloud, etc.).

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;

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
    pub const LLAMACPP: &'static str = "llamacpp";
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

    /// Enable thinking/reasoning mode (for models that support it like qwen3-vl).
    ///
    /// Legacy boolean control. Prefer [`Self::thinking_effort`]; this field is
    /// kept for compatibility and maps `true`→`Some(High)`, `false`→`Some(None)`.
    pub thinking_enabled: Option<bool>,

    /// Unified thinking/reasoning effort. Takes precedence over
    /// [`Self::thinking_enabled`] when both are set. Backends translate this to
    /// their native mechanism (Ollama `think`, Anthropic `thinking`, …).
    pub thinking_effort: Option<ThinkingEffort>,

    /// Maximum context window size in tokens
    /// CRITICAL for Qwen3: must be >= 16384 to avoid infinite repetition loops
    pub max_context: Option<usize>,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            max_tokens: Some(usize::MAX),
            stop: None,
            frequency_penalty: Some(0.0),
            presence_penalty: Some(0.0),
            thinking_enabled: None, // Let backend decide based on model capabilities
            thinking_effort: None,  // Let backend decide based on model capabilities
            max_context: None,      // Let backend decide based on model capabilities
        }
    }
}

/// Tool definition for LLM function calling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Parameters as JSON Schema
    pub parameters: serde_json::Value,
}

/// Sanitize a tool name for OpenAI-compatible API compatibility.
///
/// OpenAI-compatible APIs require `function.name` to match `^[a-zA-Z0-9_-]+$`.
/// Extension tools use `{extension_id}:{command_name}` format (e.g. `test.extension:test_command`)
/// which contains `.` and `:`. This function replaces any character that is not
/// alphanumeric, underscore, or hyphen with an underscore.
pub fn sanitize_tool_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
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

    /// Tool definitions for function calling (optional)
    pub tools: Option<Vec<ToolDefinition>>,
}

impl LlmInput {
    /// Create a new input with a single user message.
    pub fn new(content: impl Into<ModalityContent>) -> Self {
        Self {
            messages: vec![Message::user(content.into().as_text())],
            params: GenerationParams::default(),
            model: None,
            stream: false,
            tools: None,
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

    /// Set tool definitions for function calling.
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
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
                    if text.is_empty() {
                        "Describe this image."
                    } else {
                        &text
                    }
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

    /// Thinking content (for models that support reasoning/thinking)
    pub thinking: Option<String>,

    /// Native structured tool calls from the API response.
    /// Populated by backends that support native tool calling (OpenAI, Ollama, llama.cpp).
    /// Each entry is a JSON object with `id`, `name`, and `arguments` fields.
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// Finish reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FinishReason {
    /// Model stopped naturally
    Stop,

    /// Max tokens reached
    Length,

    /// Model hit an error
    Error,

    /// Content filter triggered
    ContentFilter,

    /// Model wants to call tools
    ToolCalls,
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

/// Strip a `__NEOMIND_TOKEN_PROMPT:N__` marker from a stream chunk and
/// return `(clean_text, Some(N))`. Handles both standalone-chunk emission
/// (markers are typically yielded alone as `Ok((marker, false))`) and the
/// defensive mid-text case. Returns `(original, None)` if no marker present.
///
/// The marker is in-band metadata streaming backends inject to surface
/// `prompt_tokens` (completion_tokens is not available via streaming).
fn strip_token_marker(content: &str) -> (String, Option<u32>) {
    const MARKER: &str = "__NEOMIND_TOKEN_PROMPT:";
    if let Some(start) = content.find(MARKER) {
        let after = &content[start + MARKER.len()..];
        if let Some(end) = after.find("__") {
            if let Ok(n) = after[..end].parse::<u32>() {
                let tail_start = start + MARKER.len() + end + 2;
                let clean = format!("{}{}", &content[..start], &content[tail_start..])
                    .trim()
                    .to_string();
                return (clean, Some(n));
            }
        }
    }
    (content.to_string(), None)
}

/// Stream configuration for LLM backends.
///
/// This configuration controls timeouts, thinking limits, and progress reporting
/// for streaming responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Maximum thinking characters before cutoff.
    ///
    /// When the model generates more than this many characters in the "thinking" field,
    /// the remaining thinking content will be skipped and the system will wait for
    /// the actual content to begin. This prevents models from getting stuck in
    /// infinite thinking loops.
    ///
    /// Default: 50,000 characters
    #[serde(default = "StreamConfig::default_max_thinking_chars")]
    pub max_thinking_chars: usize,

    /// Maximum thinking time in seconds.
    ///
    /// If the model spends more than this time generating thinking content,
    /// the system will skip remaining thinking and wait for content.
    ///
    /// Default: 120 seconds
    #[serde(default = "StreamConfig::default_max_thinking_time_secs")]
    pub max_thinking_time_secs: u64,

    /// Total stream timeout in seconds.
    ///
    /// The entire streaming operation (thinking + content generation) must
    /// complete within this time limit.
    ///
    /// Default: 300 seconds (5 minutes)
    #[serde(default = "StreamConfig::default_max_stream_duration_secs")]
    pub max_stream_duration_secs: u64,

    /// Progressive warning thresholds in seconds.
    ///
    /// The system will send progress warnings at these elapsed times.
    /// This helps users understand long-running operations.
    ///
    /// Default: [60, 120, 180, 240] seconds
    #[serde(default = "StreamConfig::default_warning_thresholds")]
    pub warning_thresholds: Vec<u64>,

    /// Maximum consecutive identical thinking chunks before assuming loop.
    ///
    /// This detects when a model is stuck repeating the same thinking content.
    ///
    /// Default: 10
    #[serde(default = "StreamConfig::default_max_thinking_loop")]
    pub max_thinking_loop: usize,

    /// Enable progressive progress reporting.
    ///
    /// When enabled, the backend will send progress events at regular intervals
    /// and at warning thresholds.
    ///
    /// Default: true
    #[serde(default = "StreamConfig::default_progress_enabled")]
    pub progress_enabled: bool,

    /// Maximum total characters (thinking + content) before hard cutoff.
    ///
    /// This is a safety limit to prevent infinite loops where the model
    /// continuously generates content without completing. If the total
    /// output exceeds this limit, the stream will be terminated.
    ///
    /// Default: 200,000 characters
    #[serde(default = "StreamConfig::default_max_total_chars")]
    pub max_total_chars: usize,
}

impl StreamConfig {
    fn default_max_thinking_chars() -> usize {
        50_000
    }

    fn default_max_thinking_time_secs() -> u64 {
        300
    }

    fn default_max_stream_duration_secs() -> u64 {
        1200
    }

    fn default_warning_thresholds() -> Vec<u64> {
        vec![60, 300, 600, 900]
    }

    fn default_max_thinking_loop() -> usize {
        10
    }

    fn default_progress_enabled() -> bool {
        true
    }

    fn default_max_total_chars() -> usize {
        200_000
    }

    /// Get the max stream duration as a Duration.
    pub fn max_stream_duration(&self) -> Duration {
        Duration::from_secs(self.max_stream_duration_secs)
    }

    /// Get the max thinking time as a Duration.
    pub fn max_thinking_time(&self) -> Duration {
        Duration::from_secs(self.max_thinking_time_secs)
    }
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_thinking_chars: Self::default_max_thinking_chars(),
            max_thinking_time_secs: Self::default_max_thinking_time_secs(),
            max_stream_duration_secs: Self::default_max_stream_duration_secs(),
            warning_thresholds: Self::default_warning_thresholds(),
            max_thinking_loop: Self::default_max_thinking_loop(),
            progress_enabled: Self::default_progress_enabled(),
            max_total_chars: Self::default_max_total_chars(),
        }
    }
}

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

    /// HTTP API error with status code and response body.
    /// Used by cloud backends to preserve status code for classification.
    #[error("API error {status}: {body}")]
    Api {
        /// HTTP status code (e.g., 401, 403, 429, 500).
        status: u16,
        /// Raw response body (may be JSON, may be empty).
        body: String,
    },

    /// Context window exceeded - request too large for model's context
    #[error("Context overflow: {prompt_tokens} prompt tokens exceed {max_context} context limit")]
    ContextOverflow {
        /// Number of prompt tokens in the failed request
        prompt_tokens: usize,
        /// Maximum context size of the model
        max_context: usize,
    },

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
            + latency_ms as f64)
            / self.total_requests as f64;
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

    /// Warm up the model by sending a minimal request.
    ///
    /// This eliminates first-request latency by triggering model loading
    /// during initialization. Implementations should use minimal tokens
    /// to reduce overhead. Default implementation does nothing.
    async fn warmup(&self) -> Result<(), LlmError> {
        // Default: no warmup
        Ok(())
    }

    /// Generate a response (non-streaming).
    async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError>;

    /// Generate a response (streaming).
    async fn generate_stream(
        &self,
        input: LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError>;

    /// Generate a response by consuming the stream to completion and
    /// aggregating into a single `LlmOutput`.
    ///
    /// Default implementation consumes `generate_stream` and accumulates:
    /// - `text`: concatenation of all content chunks (`is_thinking = false`)
    /// - `thinking`: concatenation of all thinking chunks (`is_thinking = true`)
    /// - token-usage markers (`\n__NEOMIND_TOKEN_PROMPT:N__`) are filtered out
    ///   as metadata, not content
    ///
    /// `tool_calls` is left as `None` — backends that emit tool calls via
    /// the stream embed them as JSON in content chunks (same shape as the
    /// non-streaming path's text+JSON concatenation), so `tool_loop`'s
    /// existing fallback parser extracts them from `text`.
    ///
    /// `usage` is populated when any signal is available:
    /// - `prompt_tokens`: real, extracted from `__NEOMIND_TOKEN_PROMPT:N__`
    ///   markers emitted by streaming backends (Ollama/OpenAI/llama.cpp).
    ///   `0` if no marker was emitted.
    /// - `completion_tokens`: estimated via the trait's `estimate_tokens()`
    ///   heuristic on `text + thinking` chars. Streaming backends do not
    ///   surface completion_tokens, so this is necessarily approximate.
    /// - `usage` is `None` only when the stream produced no content AND no
    ///   marker (empty response — typically a stream-error case).
    ///
    /// Why this exists: thinking-capable cloud backends (DashScope qwen3.x-plus)
    /// can sit silent for 30+ seconds during the reasoning phase under
    /// non-streaming mode, hitting gateway idle timeouts. Routing through
    /// streaming keeps bytes flowing so the connection survives.
    async fn generate_to_completion(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
        use futures::StreamExt;

        let mut stream = self.generate_stream(input).await?;
        let mut text_parts: Vec<String> = Vec::new();
        let mut thinking_parts: Vec<String> = Vec::new();
        // prompt_tokens is extracted in-band from `__NEOMIND_TOKEN_PROMPT:N__`
        // markers emitted by streaming backends (Ollama/OpenAI/llama.cpp).
        // completion_tokens is not surfaced by streaming backends, so we
        // estimate from the concatenated output below. Together this lets
        // tool_loop callers reading `output.usage` get partial-real +
        // partial-estimated data instead of `None` — important for token
        // accounting, telemetry, and any per-iteration budgets.
        let mut prompt_tokens: Option<u32> = None;
        let mut stream_error: Option<LlmError> = None;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok((text, is_thinking)) => {
                    // Streaming backends inject `\n__NEOMIND_TOKEN_PROMPT:N__`
                    // markers to surface prompt-token counts. Strip the marker
                    // (wherever it appears in the chunk — standalone or
                    // embedded mid-text) and capture the value.
                    let (clean, extracted) = strip_token_marker(&text);
                    if let Some(n) = extracted {
                        prompt_tokens = Some(n);
                    }
                    if clean.is_empty() {
                        continue;
                    }
                    if is_thinking {
                        thinking_parts.push(clean);
                    } else {
                        text_parts.push(clean);
                    }
                }
                Err(e) => {
                    stream_error = Some(e);
                    break;
                }
            }
        }

        if let Some(e) = stream_error {
            return Err(e);
        }

        let text = text_parts.concat();
        let thinking = if thinking_parts.is_empty() {
            None
        } else {
            Some(thinking_parts.concat())
        };

        // Estimate completion_tokens via the trait's heuristic (~4 chars/token).
        // Includes thinking chars so reasoning-heavy models are accounted for.
        let usage = if prompt_tokens.is_some() || !text.is_empty() || thinking.is_some() {
            let completion_chars = text.len() + thinking.as_ref().map(|s| s.len()).unwrap_or(0);
            let completion_tokens = (completion_chars / 4) as u32;
            Some(TokenUsage::new(
                prompt_tokens.unwrap_or(0),
                completion_tokens,
            ))
        } else {
            None
        };

        Ok(LlmOutput {
            text,
            finish_reason: FinishReason::Stop,
            usage,
            thinking,
            tool_calls: None,
        })
    }

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
/// Unified thinking/reasoning effort control.
///
/// This is the canonical "how much should the model think" enum threaded
/// through requests. Each backend maps it to its own native mechanism in the
/// translate layer (Ollama `think`, Anthropic `thinking`/`output_config.effort`,
/// OpenAI-style `reasoning_effort`/`enable_thinking`, llama.cpp read-only).
///
/// `None` (the enum variant, not the Option) means "explicitly disable
/// thinking". The previous boolean control is a subset: `true` ≈ `Some(High)`,
/// `false` ≈ `Some(None)` — kept working via a compatibility mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingEffort {
    /// Explicitly disable thinking/reasoning.
    None,
    /// Minimize thinking — best for simple/latency-sensitive queries.
    Low,
    /// Moderate thinking — may skip thinking for simple queries.
    Medium,
    /// Deep reasoning on complex tasks (the common default).
    High,
    /// Extended exploration beyond the default.
    XHigh,
    /// Always think with no constraint on depth.
    Max,
}

impl ThinkingEffort {
    /// Map from the legacy boolean control. `true` ≈ `High`, `false` ≈ `None`.
    pub fn from_bool(enabled: bool) -> Self {
        if enabled {
            Self::High
        } else {
            Self::None
        }
    }

    /// Whether this effort is an explicit "disable".
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Stable string id for persistence / API / UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// Declared reasoning/thinking capabilities of a backend/model.
///
/// Mirrors the OpenRouter `model_listing.reasoning` shape
/// (`supported_efforts`/`default_effort`/`mandatory`) — capability
/// *declaration* that the UI and translate layer consult, rather than
/// hardcoding effort support per backend.
#[derive(Debug, Clone, Default)]
pub struct ReasoningCapabilities {
    /// Which effort levels this backend can honor. Empty = unknown; callers
    /// should treat that as "only None/High (boolean) are safe".
    pub supported_efforts: Vec<ThinkingEffort>,
    /// The model's default effort when the caller sends nothing.
    pub default_effort: Option<ThinkingEffort>,
    /// Whether thinking is mandatory for this model — a caller sending
    /// `effort: None` would be rejected (OpenRouter warns on `mandatory`).
    pub mandatory: bool,
    /// How the backend controls thinking (not all can be turned off).
    pub control: ReasoningControl,
}

/// How a backend controls thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningControl {
    /// No control at all — thinking follows model defaults; cannot be toggled.
    #[default]
    ReadOnly,
    /// Boolean on/off (e.g. legacy Ollama `think: true/false`).
    Boolean,
    /// Discrete effort levels (e.g. Ollama `think: "low"/"medium"/"high"`).
    Level,
    /// An OpenAI-style `reasoning_effort` string / per-token budget.
    Effort,
}

/// Model capability declaration.
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

    /// Declared reasoning/thinking capabilities (effort levels, mandatory, control).
    pub reasoning: ReasoningCapabilities,
}

impl BackendCapabilities {
    /// Create a new builder for capabilities.
    pub fn builder() -> BackendCapabilitiesBuilder {
        BackendCapabilitiesBuilder::new()
    }
}

/// Builder for BackendCapabilities.
pub struct BackendCapabilitiesBuilder {
    capabilities: BackendCapabilities,
}

impl Default for BackendCapabilitiesBuilder {
    fn default() -> Self {
        Self::new()
    }
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
        self.capabilities.supports_images = true; // Multimodal implies image support
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

    /// Enable thinking display.
    pub fn thinking_display(mut self) -> Self {
        self.capabilities.thinking_display = true;
        self
    }

    /// Declare reasoning capabilities (effort levels, control mechanism).
    pub fn reasoning(mut self, caps: ReasoningCapabilities) -> Self {
        self.capabilities.reasoning = caps;
        if !self.capabilities.reasoning.supported_efforts.is_empty() {
            self.capabilities.thinking_display = true;
        }
        self
    }

    /// Build the capabilities.
    pub fn build(self) -> BackendCapabilities {
        self.capabilities
    }
}

impl LlmError {
    /// Classify whether this error requires user action (permanent) or may
    /// succeed on retry (transient).
    ///
    /// Used for log clarity only — both classes propagate as Failed execution
    /// per the agent error surfacing design (Rev 4, Option A).
    pub fn is_permanent(&self) -> bool {
        match self {
            Self::BackendUnavailable(_)
            | Self::ModelNotFound(_)
            | Self::InvalidInput(_)
            | Self::ContextOverflow { .. }
            | Self::Serialization(_) => true,
            Self::Api { status, .. } => *status >= 400 && *status < 500 && *status != 429,
            Self::Timeout(_)
            | Self::Network(_)
            | Self::Io(_)
            | Self::Generation(_)
            | Self::Unknown(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_input_builder() {
        let input = LlmInput::new("Hello")
            .with_model("qwen2")
            .with_streaming(true);

        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.model.as_deref(), Some("qwen2"));
        assert!(input.stream);
    }

    #[test]
    fn test_thinking_effort_from_bool() {
        // Legacy boolean control maps to the effort enum.
        assert_eq!(ThinkingEffort::from_bool(true), ThinkingEffort::High);
        assert_eq!(ThinkingEffort::from_bool(false), ThinkingEffort::None);
        assert!(ThinkingEffort::None.is_disabled());
        assert!(!ThinkingEffort::High.is_disabled());
    }

    #[test]
    fn test_generation_params_defaults() {
        let params = GenerationParams::default();
        assert_eq!(params.thinking_enabled, None);
        assert_eq!(params.thinking_effort, None);
        // `thinking_effort` takes precedence when set, but both default None.
    }

    #[test]
    fn test_reasoning_capabilities_default() {
        let caps = ReasoningCapabilities::default();
        assert!(caps.supported_efforts.is_empty());
        assert_eq!(caps.default_effort, None);
        assert!(!caps.mandatory);
        assert_eq!(caps.control, ReasoningControl::ReadOnly);
    }

    // ── generate_to_completion default impl ──────────────────────────────
    //
    // The default trait method consumes `generate_stream` and aggregates it
    // into an `LlmOutput`. This is what `tool_loop` uses for thinking-capable
    // cloud backends to avoid gateway idle timeouts (see commit c6385169 +
    // follow-up). The tests below verify the default impl handles:
    //   1. Plain content chunks → text field
    //   2. Thinking chunks → thinking field
    //   3. Token-usage markers → filtered out (not content)
    //   4. Mid-stream errors → propagated as Err
    //   5. Tool-call JSON chunks → preserved in text (tool_loop's fallback
    //      parser extracts them; we don't need structured tool_calls here)

    /// Clone-friendly chunk spec — `LlmError` itself isn't `Clone`, so the
    /// mock stores a serializable spec and rebuilds fresh `StreamChunk`s on
    /// each `generate_stream` call.
    #[derive(Clone)]
    enum MockChunk {
        Content(String),
        Thinking(String),
        TokenMarker(u32),
        Error(String),
    }

    struct MockStreamRuntime {
        chunks: Vec<MockChunk>,
    }

    #[async_trait::async_trait]
    impl LlmRuntime for MockStreamRuntime {
        fn backend_id(&self) -> BackendId {
            BackendId::new("mock")
        }
        fn model_name(&self) -> &str {
            "mock-model"
        }
        async fn generate(&self, _input: LlmInput) -> Result<LlmOutput, LlmError> {
            unreachable!("generate_to_completion must use generate_stream, not generate")
        }
        async fn generate_stream(
            &self,
            _input: LlmInput,
        ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
            let chunks: Vec<StreamChunk> = self
                .chunks
                .iter()
                .map(|c| match c {
                    MockChunk::Content(s) => Ok((s.clone(), false)),
                    MockChunk::Thinking(s) => Ok((s.clone(), true)),
                    MockChunk::TokenMarker(n) => {
                        Ok((format!("\n__NEOMIND_TOKEN_PROMPT:{}__", n), false))
                    }
                    MockChunk::Error(msg) => Err(LlmError::Network(msg.clone())),
                })
                .collect();
            Ok(Box::pin(futures::stream::iter(chunks)))
        }
        fn max_context_length(&self) -> usize {
            4096
        }
    }

    #[tokio::test]
    async fn generate_to_completion_aggregates_content_chunks() {
        let runtime = MockStreamRuntime {
            chunks: vec![
                MockChunk::Content("Hello, ".into()),
                MockChunk::Content("world!".into()),
            ],
        };
        let out = runtime
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        assert_eq!(out.text, "Hello, world!");
        assert!(out.thinking.is_none());
        assert!(out.tool_calls.is_none());
        // No token marker emitted → prompt_tokens unknown (0), completion
        // estimated from "Hello, world!" (13 chars / 4 = 3 tokens).
        let usage = out
            .usage
            .expect("usage should be populated when text exists");
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 3);
    }

    #[tokio::test]
    async fn generate_to_completion_separates_thinking_from_content() {
        let runtime = MockStreamRuntime {
            chunks: vec![
                MockChunk::Thinking("Let me think...".into()),
                MockChunk::Thinking("First, ".into()),
                MockChunk::Thinking("I'll do X".into()),
                MockChunk::Content("Doing X now".into()),
            ],
        };
        let out = runtime
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        assert_eq!(out.text, "Doing X now");
        assert_eq!(
            out.thinking.as_deref(),
            Some("Let me think...First, I'll do X")
        );
        // completion_tokens should include BOTH text + thinking chars.
        let usage = out
            .usage
            .expect("usage populated when text/thinking exists");
        // text(12) + thinking(31) = 43 chars / 4 = 10 tokens
        assert_eq!(usage.completion_tokens, 10);
    }

    #[tokio::test]
    async fn generate_to_completion_captures_prompt_token_marker() {
        // Streaming backends inject `\n__NEOMIND_TOKEN_PROMPT:N__` markers to
        // surface prompt_tokens. The default impl must:
        //   (a) strip the marker from the visible text
        //   (b) capture the value into `usage.prompt_tokens`
        //   (c) still estimate completion_tokens from concatenated output.
        let runtime = MockStreamRuntime {
            chunks: vec![
                MockChunk::Content("Hello".into()),
                MockChunk::TokenMarker(42),
                MockChunk::Content(" world".into()),
            ],
        };
        let out = runtime
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        assert_eq!(out.text, "Hello world");
        let usage = out.usage.expect("usage must be populated");
        assert_eq!(usage.prompt_tokens, 42);
        // "Hello world" = 11 chars / 4 = 2 tokens
        assert_eq!(usage.completion_tokens, 2);
    }

    #[tokio::test]
    async fn generate_to_completion_handles_embedded_token_marker() {
        // Defensive: if a marker ever arrives mid-text (not as a standalone
        // chunk), it should still be stripped + captured without corrupting
        // the surrounding content.
        let runtime = MockStreamRuntime {
            chunks: vec![MockChunk::Content(
                "before __NEOMIND_TOKEN_PROMPT:99__ after".into(),
            )],
        };
        let out = runtime
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        assert_eq!(out.text, "before  after");
        assert!(!out.text.contains("NEOMIND_TOKEN"));
        let usage = out.usage.expect("usage must be populated");
        assert_eq!(usage.prompt_tokens, 99);
    }

    #[tokio::test]
    async fn generate_to_completion_propagates_stream_errors() {
        let runtime = MockStreamRuntime {
            chunks: vec![
                MockChunk::Content("partial...".into()),
                MockChunk::Error("connection reset".into()),
            ],
        };
        let result = runtime.generate_to_completion(LlmInput::new("hi")).await;
        match result {
            Err(LlmError::Network(msg)) => assert!(msg.contains("connection reset")),
            other => panic!("expected Network error, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod error_classification_tests {
    use super::LlmError;

    #[test]
    fn permanent_variants() {
        assert!(LlmError::BackendUnavailable("ollama".into()).is_permanent());
        assert!(LlmError::ModelNotFound("qwen3.5:4b".into()).is_permanent());
        assert!(LlmError::InvalidInput("bad request".into()).is_permanent());
        assert!(LlmError::ContextOverflow {
            prompt_tokens: 10000,
            max_context: 8000
        }
        .is_permanent());
        assert!(
            LlmError::Serialization(serde_json::from_str::<i32>("not a number").unwrap_err())
                .is_permanent()
        );
    }

    #[test]
    fn permanent_http_statuses() {
        assert!(LlmError::Api {
            status: 400,
            body: "".into()
        }
        .is_permanent());
        assert!(LlmError::Api {
            status: 401,
            body: "".into()
        }
        .is_permanent());
        assert!(LlmError::Api {
            status: 403,
            body: "quota exhausted".into()
        }
        .is_permanent());
        assert!(LlmError::Api {
            status: 404,
            body: "".into()
        }
        .is_permanent());
    }

    #[test]
    fn transient_http_statuses() {
        assert!(!LlmError::Api {
            status: 429,
            body: "rate limited".into()
        }
        .is_permanent());
        assert!(!LlmError::Api {
            status: 500,
            body: "".into()
        }
        .is_permanent());
        assert!(!LlmError::Api {
            status: 502,
            body: "".into()
        }
        .is_permanent());
        assert!(!LlmError::Api {
            status: 503,
            body: "".into()
        }
        .is_permanent());
    }

    #[test]
    fn transient_variants() {
        assert!(!LlmError::Timeout(60).is_permanent());
        assert!(!LlmError::Network("connection refused".into()).is_permanent());
        assert!(!LlmError::Generation("legacy fallback".into()).is_permanent());
        assert!(!LlmError::Unknown("something".into()).is_permanent());
    }

    #[test]
    fn api_variant_display_format() {
        let e = LlmError::Api {
            status: 403,
            body: "quota exhausted".into(),
        };
        let s = format!("{}", e);
        assert!(
            s.contains("403"),
            "Display should include status: got {}",
            s
        );
        assert!(
            s.contains("quota exhausted"),
            "Display should include body: got {}",
            s
        );
    }
}
