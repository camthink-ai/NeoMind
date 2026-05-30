//! Ollama LLM backend implementation.
//!
//! Ollama is a local LLM runner that supports various models.
//! This backend communicates with Ollama via its native API.
//!
//! Tool Calling Support:
//! - Models with native tool support (qwen3-vl, etc.): Use Ollama's native tool API
//! - Models without native tool support (gemma3:270m, etc.): Use text-based tool calling

use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput, LlmRuntime,
    StreamChunk, StreamConfig, TokenUsage,
};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

/// Ollama configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OllamaConfig {
    /// Ollama endpoint (default: http://localhost:11434)
    pub endpoint: String,

    /// Model name (e.g., "qwen3-vl:2b", "llama3:8b")
    pub model: String,

    /// Request timeout in seconds (default: 180).
    /// This is deserialized as a number and converted to Duration internally.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

/// Default timeout in seconds for deserialization.
fn default_timeout_secs() -> u64 {
    300
}

impl OllamaConfig {
    /// Get the timeout as a Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Create a new Ollama config.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            endpoint: "http://localhost:11434".to_string(),
            model: model.into(),
            timeout_secs: 300,
        }
    }

    /// Set a custom endpoint.
    /// Note: Ollama uses native API, not OpenAI-compatible. The endpoint should be like
    /// "http://localhost:11434" (without /v1 suffix). If /v1 is provided, it will be stripped.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        let mut endpoint = endpoint.into();
        // Strip /v1 suffix if present (Ollama native API doesn't use it)
        if endpoint.ends_with("/v1") {
            endpoint = endpoint
                .strip_suffix("/v1")
                .map(|s| s.to_string())
                .unwrap_or_else(|| endpoint.clone());
            // Also remove trailing slash if present
            endpoint = endpoint.strip_suffix("/").unwrap_or(&endpoint).to_string();
        }
        self.endpoint = endpoint;
        self
    }

    /// Set timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_secs = timeout.as_secs();
        self
    }
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self::new("qwen3.5:4b")
    }
}

/// Ollama runtime backend.
pub struct OllamaRuntime {
    config: OllamaConfig,
    client: Client,
    model: String,
    metrics: Arc<RwLock<BackendMetrics>>,
    stream_config: StreamConfig,
    /// Optional override for capabilities (from storage/API detection)
    /// If None, capabilities are detected from model name
    capabilities_override: Option<ModelCapability>,
}

impl OllamaRuntime {
    /// Create a new Ollama runtime.
    pub fn new(config: OllamaConfig) -> Result<Self, LlmError> {
        Self::with_stream_config(config, StreamConfig::default())
    }

    /// Create a new Ollama runtime with custom stream configuration.
    pub fn with_stream_config(
        config: OllamaConfig,
        stream_config: StreamConfig,
    ) -> Result<Self, LlmError> {
        tracing::debug!("Creating Ollama runtime with endpoint: {}", config.endpoint);
        tracing::debug!(
            "Stream config: max_thinking_chars={}, max_stream_duration={}s",
            stream_config.max_thinking_chars,
            stream_config.max_stream_duration_secs
        );

        // Configure HTTP client with connection pooling for better performance
        // Note: Don't set a global timeout — it kills long-running streaming responses
        // from thinking models (qwen3.x, deepseek-r1) that can take many minutes.
        // Instead, we use per-request timeouts only for non-streaming requests.
        // Streaming responses have their own timeout via stream_config.max_stream_duration_secs.
        let client = Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(30))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_adaptive_window(true)
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let model = config.model.clone();

        Ok(Self {
            config,
            client,
            model,
            metrics: Arc::new(RwLock::new(BackendMetrics::default())),
            stream_config,
            capabilities_override: None,
        })
    }

    /// Set capabilities override from storage/API detection.
    /// This allows using accurate capabilities detected from Ollama's /api/show
    /// instead of name-based heuristics.
    pub fn with_capabilities_override(
        mut self,
        supports_multimodal: bool,
        supports_thinking: bool,
        supports_tools: bool,
        max_context: usize,
    ) -> Self {
        self.capabilities_override = Some(ModelCapability {
            supports_tools,
            supports_thinking,
            supports_multimodal,
            max_context,
        });
        self
    }

    /// Fetch model capabilities from Ollama's /api/show endpoint.
    /// This provides accurate capability detection instead of name-based heuristics.
    pub async fn fetch_capabilities_from_api(&self) -> Option<ModelCapability> {
        let url = format!("{}/api/show", self.config.endpoint);
        let request = serde_json::json!({
            "name": self.model,
            "verbose": false
        });

        match self
            .client
            .post(&url)
            .json(&request)
            .timeout(self.config.timeout())
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<OllamaShowResponse>().await {
                    Ok(show_response) => {
                        let supports_multimodal = show_response.supports_vision();
                        let supports_tools = true; // Most modern Ollama models support tools
                        let supports_thinking = show_response.has_attention_heads();
                        let max_context = show_response.context_length().unwrap_or(128000);
                        let max_context = std::env::var("NEOMIND_MAX_CONTEXT").ok().and_then(|v| v.parse::<usize>().ok()).map(|cap| max_context.min(cap)).unwrap_or(max_context);

                        tracing::info!(
                            model = %self.model,
                            multimodal = %supports_multimodal,
                            thinking = %supports_thinking,
                            tools = %supports_tools,
                            max_context = %max_context,
                            "Fetched model capabilities from Ollama API"
                        );

                        Some(ModelCapability {
                            supports_multimodal,
                            supports_thinking,
                            supports_tools,
                            max_context,
                        })
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to parse Ollama show response");
                        None
                    }
                }
            }
            Ok(response) => {
                tracing::warn!(status = %response.status(), "Ollama show request failed");
                None
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to fetch capabilities from Ollama API");
                None
            }
        }
    }

    /// Warm up the model by sending a minimal request.
    ///
    /// This eliminates the ~500ms first-request latency by triggering model loading
    /// during initialization rather than during the first user interaction.
    ///
    /// The warmup request uses minimal tokens (1 token) to reduce overhead.
    pub async fn warmup(&self) -> Result<(), LlmError> {
        tracing::info!(
            "Warming up model: {} (this may take a moment...)",
            self.model
        );

        let url = format!("{}/api/chat", self.config.endpoint);
        let warmup_request = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": "hi"}],
            "stream": false,
            "options": {
                "num_predict": 1  // Only generate 1 token for warmup
            }
        });

        let response = self
            .client
            .post(&url)
            .json(&warmup_request)
            .timeout(self.config.timeout())
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        if response.status().is_success() {
            tracing::info!("Model warmup complete: {}", self.model);
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            tracing::warn!(
                "Model warmup returned non-success status: {} - {}",
                status,
                error_text
            );
            // Don't fail on warmup errors - the model may still work
            Ok(())
        }
    }

    /// Format tool calling format instructions for models without native tool support.
    /// Only includes format rules and examples (tool descriptions are already in the system prompt).
    fn format_tools_for_text_calling(
        _tools: &[neomind_core::llm::backend::ToolDefinition],
    ) -> String {
        let mut result = String::from("## Tool Calling Format (JSON)\n");
        result.push_str(
            "You must call tools using JSON format. Do not just describe what to do.\n\n",
        );
        result.push_str("Format:\n");
        result.push_str("[{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}]\n\n");

        result.push_str("## Important Rules\n");
        result.push_str("1. ALWAYS output tool calls as a JSON array\n");
        result.push_str("2. Don't explain, just call the tool directly\n");
        result.push_str(
            "3. Use the exact tool names and parameters from the Available Tools section above\n",
        );

        result
    }

    /// Convert messages to Ollama format, optionally injecting tool descriptions.
    fn messages_to_ollama_with_tools(
        &self,
        messages: &[Message],
        tools: Option<&[neomind_core::llm::backend::ToolDefinition]>,
        supports_native_tools: bool,
    ) -> Vec<OllamaMessage> {
        // Safe: tools.is_some_and() guarantees Some if the condition is true
        let tool_instructions = if !supports_native_tools && tools.is_some_and(|t| !t.is_empty()) {
            Some(Self::format_tools_for_text_calling(
                tools.expect("tools must be Some when is_some_and is true"),
            ))
        } else {
            None
        };

        messages
            .iter()
            .map(|msg| {
                // Extract text content
                let mut text = msg.text();

                // Inject tool instructions into system message for models without native tool support
                if msg.role == MessageRole::System {
                    if let Some(instructions) = &tool_instructions {
                        text = format!("{}\n\n{}", text, instructions);
                    }
                }

                // Extract images from multimodal content
                let images = extract_images_from_content(&msg.content);

                OllamaMessage {
                    role: match msg.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::System => "system",
                        MessageRole::Tool => "tool",
                    }
                    .to_string(),
                    content: text,
                    images,
                    tool_calls: None,
                    tool_name: msg.tool_name.clone(),
                }
            })
            .collect()
    }
}

/// Extract base64-encoded images from message content.
///
/// This function handles both ImageUrl (which will be fetched) and ImageBase64
/// (which already contains the base64 data).
///
/// For ImageUrl, the URL can be:
/// - A base64 data URL (data:image/png;base64,...)
/// - An HTTP/HTTPS URL (will be fetched and encoded)
/// - A local file path (will be read and encoded)
fn extract_images_from_content(content: &Content) -> Vec<String> {
    let parts = match content {
        Content::Text(_) => return Vec::new(),
        Content::Parts(parts) => parts,
    };

    let mut images = Vec::new();

    for part in parts {
        match part {
            ContentPart::ImageUrl { url, .. } => {
                if let Some(img) = extract_image_from_url(url) {
                    images.push(img);
                }
            }
            ContentPart::ImageBase64 {
                data, mime_type: _, ..
            } => {
                // Already base64 encoded, just remove the mime type prefix if present
                let base64_data = if data.contains(',') {
                    data.split(',').next_back().unwrap_or(data).to_string()
                } else {
                    data.clone()
                };
                images.push(base64_data);
            }
            ContentPart::Text { .. } => {
                // Text part, no image
            }
        }
    }

    images
}

/// Extract a base64-encoded image from a URL.
///
/// Supports:
/// - Base64 data URLs (data:image/...;base64,...)
/// - HTTP/HTTPS URLs (not yet supported)
/// - Local file paths (not yet supported - requires async I/O)
fn extract_image_from_url(url: &str) -> Option<String> {
    // Check if it's a base64 data URL
    if url.starts_with("data:image/") {
        // Extract the base64 part after the comma
        if let Some(base64_part) = url.split(',').nth(1) {
            return Some(base64_part.to_string());
        }
        return None;
    }

    // Check if it's an HTTP/HTTPS URL
    if url.starts_with("http://") || url.starts_with("https://") {
        // For async fetching, we'd need to do this in an async context
        // For now, return None and log a warning
        tracing::warn!(
            "Fetching images from HTTP URLs is not yet supported: {}",
            url
        );
        return None;
    }

    // For local file paths, we'd need async file I/O
    // Log that this is not supported and return None
    tracing::warn!(
        "Local file image loading is not yet supported: {}. Use base64 data URLs instead.",
        url
    );
    None
}

#[async_trait::async_trait]
impl LlmRuntime for OllamaRuntime {
    fn backend_id(&self) -> BackendId {
        BackendId::new(BackendId::OLLAMA) // Use Ollama backend ID
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        // Try to ping Ollama
        if let Ok(resp) = self
            .client
            .get(format!("{}/api/tags", self.config.endpoint))
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            resp.status().is_success()
        } else {
            false
        }
    }

    async fn generate(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<LlmOutput, LlmError> {
        let start_time = Instant::now();
        let model = input.model.unwrap_or_else(|| self.model.clone());

        let url = format!("{}/api/chat", self.config.endpoint);
        tracing::debug!("Ollama: calling URL: {}", url);

        // Detect model capabilities
        let caps = detect_model_capabilities(&model);

        // Handle max_tokens: increased cap for thinking models
        // Thinking models need significant budget for both thinking AND response generation
        const MAX_TOKENS_CAP: usize = 32768; // 32k tokens - sufficient for extended thinking + response
        let num_predict = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => Some(MAX_TOKENS_CAP),
            Some(v) => Some(v.min(MAX_TOKENS_CAP)),
            None => Some(MAX_TOKENS_CAP),
        };

        // Determine tool support and prepare accordingly
        let supports_native_tools = caps.supports_tools;
        let has_tools = input.tools.as_ref().is_some_and(|t| !t.is_empty());

        // Only use native tools parameter for models that support it
        // For other models, tools will be injected into the system message
        let native_tools = if supports_native_tools {
            if let Some(input_tools) = &input.tools {
                if !input_tools.is_empty() {
                    let ollama_tools: Vec<OllamaTool> = input_tools
                        .iter()
                        .map(|tool| OllamaTool {
                            tool_type: "function".to_string(),
                            function: OllamaToolFunction {
                                name: tool.name.clone(),
                                description: tool.description.clone(),
                                parameters: tool.parameters.clone(),
                            },
                        })
                        .collect();
                    tracing::debug!(
                        "Ollama: using native tools API for {} tools",
                        ollama_tools.len()
                    );
                    Some(ollama_tools)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            // Model doesn't support native tools - will use text-based calling
            if has_tools {
                tracing::info!(
                    "Ollama: model {} doesn't support native tools, using text-based tool calling",
                    model
                );
            }
            None
        };

        // Determine context size: use max_context from params, or the model's real context length
        // from /api/show. Never hardcode — the API returns the accurate value (e.g., 131072 for qwen3.5).
        let num_ctx = input.params.max_context.or({
            if caps.max_context > 0 {
                { let cap = std::env::var("NEOMIND_MAX_CONTEXT").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(usize::MAX); Some(caps.max_context.min(cap)) }
            } else {
                None // Let Ollama use its default
            }
        });

        let options = if input.params.temperature.is_some()
            || num_predict.is_some()
            || input.params.top_p.is_some()
            || input.params.top_k.is_some()
            || num_ctx.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict,
                top_p: input.params.top_p,
                top_k: input.params.top_k,
                num_ctx,
                repeat_penalty: Some(1.05), // Prevent content repetition
                stop: None,
            })
        } else {
            None
        };

        // Thinking: Explicitly control based on thinking_enabled parameter
        let model_supports_thinking = caps.supports_thinking;
        let user_requested_thinking = input.params.thinking_enabled;

        // Determine the think parameter
        let think: Option<OllamaThink> = match user_requested_thinking {
            Some(false) => Some(OllamaThink::Bool(false)), // Explicitly disable
            Some(true) if model_supports_thinking => Some(OllamaThink::Bool(true)), // Explicitly enable
            Some(true) => None, // Model doesn't support thinking, don't send parameter
            None => None,       // Use model default
        };

        // When tools are present, disable thinking to prevent wasting tokens
        // and ensure tool calls are generated efficiently
        let format: Option<String> = None;

        // Convert messages with tool injection for non-native models
        let messages = self.messages_to_ollama_with_tools(
            &input.messages,
            input.tools.as_deref(),
            supports_native_tools,
        );

        let request = OllamaChatRequest {
            model: model.clone(),
            messages,
            stream: false,
            options,
            think,
            tools: native_tools,
            format,
        };

        let request_json = serde_json::to_string(&request).map_err(LlmError::Serialization)?;
        tracing::debug!("Ollama: sending request to model: {}", model);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_json)
            .timeout(self.config.timeout())
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        if !status.is_success() {
            if let Ok(mut metrics) = self.metrics.write() {
                metrics.record_failure();
            }
            return Err(LlmError::Generation(format!(
                "Ollama API error {}: {}",
                status.as_u16(),
                body
            )));
        }

        let ollama_response: OllamaChatResponse =
            serde_json::from_str(&body).map_err(LlmError::Serialization)?;

        // Handle response - use content field directly; fall back to thinking if content is empty
        // Ollama already separates content and thinking correctly in the API response.
        let mut response_text = if ollama_response.message.content.is_empty() {
            // Content is empty - use thinking field as fallback (for models like qwen3:1.7b)
            ollama_response.message.thinking.clone()
        } else {
            ollama_response.message.content.clone()
        };

        // Handle native tool calls from Ollama - preserve JSON format to keep tool ID
        if !ollama_response.message.tool_calls.is_empty() {
            tracing::debug!(
                "Ollama: received {} native tool calls",
                ollama_response.message.tool_calls.len()
            );
            // Build JSON array to preserve tool IDs (OpenAI-compatible format)
            let tool_calls_json: Vec<serde_json::Value> = ollama_response
                .message
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "name": tc.function.name,
                        "arguments": tc.function.arguments
                    })
                })
                .collect();
            let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
            response_text.push_str(&json_str);
        }

        let result = Ok(LlmOutput {
            text: response_text,
            finish_reason: if ollama_response.done {
                FinishReason::Stop
            } else {
                FinishReason::Error
            },
            usage: ollama_response.eval_count.map(|count| TokenUsage {
                prompt_tokens: ollama_response.prompt_eval_count.unwrap_or(0) as u32,
                completion_tokens: count as u32,
                total_tokens: (ollama_response.prompt_eval_count.unwrap_or(0) + count) as u32,
            }),
            // Include thinking content if present
            thinking: if ollama_response.message.thinking.is_empty() {
                None
            } else {
                Some(ollama_response.message.thinking.clone())
            },
        });

        // Record metrics
        let latency_ms = start_time.elapsed().as_millis() as u64;
        match &result {
            Ok(output) => {
                let tokens = output.usage.map_or(0, |u| u.completion_tokens as u64);
                if let Ok(mut metrics) = self.metrics.write() {
                    metrics.record_success(tokens, latency_ms);
                }
            }
            Err(_) => {
                if let Ok(mut metrics) = self.metrics.write() {
                    metrics.record_failure();
                }
            }
        }

        result
    }

    async fn generate_stream(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel(64);

        let model = input.model.unwrap_or_else(|| self.model.clone());
        let url = format!("{}/api/chat", self.config.endpoint);
        let client = self.client.clone();

        // Detect model capabilities
        let caps = detect_model_capabilities(&model);

        // Handle max_tokens: increased cap for thinking models
        // Thinking models need significant budget for both thinking AND response generation
        const MAX_TOKENS_CAP: usize = 32768; // 32k tokens - sufficient for extended thinking + response
        let num_predict = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => Some(MAX_TOKENS_CAP),
            Some(v) => Some(v.min(MAX_TOKENS_CAP)),
            None => Some(MAX_TOKENS_CAP),
        };

        // Determine tool support and prepare accordingly
        let supports_native_tools = caps.supports_tools;
        let has_tools = input.tools.as_ref().is_some_and(|t| !t.is_empty());

        // Only use native tools parameter for models that support it
        let native_tools = if supports_native_tools {
            if let Some(input_tools) = &input.tools {
                if !input_tools.is_empty() {
                    let ollama_tools: Vec<OllamaTool> = input_tools
                        .iter()
                        .map(|tool| OllamaTool {
                            tool_type: "function".to_string(),
                            function: OllamaToolFunction {
                                name: tool.name.clone(),
                                description: tool.description.clone(),
                                parameters: tool.parameters.clone(),
                            },
                        })
                        .collect();
                    tracing::debug!(
                        "Ollama: using native tools API for {} tools (stream)",
                        ollama_tools.len()
                    );
                    Some(ollama_tools)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            if has_tools {
                tracing::info!(
                    "Ollama: model {} doesn't support native tools, using text-based tool calling (stream)",
                    model
                );
            }
            None
        };

        // Determine context size: use max_context from params, or the model's real context length
        // from /api/show. Never hardcode — the API returns the accurate value (e.g., 131072 for qwen3.5).
        let num_ctx = input.params.max_context.or({
            if caps.max_context > 0 {
                { let cap = std::env::var("NEOMIND_MAX_CONTEXT").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(usize::MAX); Some(caps.max_context.min(cap)) }
            } else {
                None // Let Ollama use its default
            }
        });

        let options = if input.params.temperature.is_some()
            || num_predict.is_some()
            || input.params.top_p.is_some()
            || input.params.top_k.is_some()
            || num_ctx.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict,
                top_p: input.params.top_p,
                top_k: input.params.top_k,
                num_ctx,
                repeat_penalty: Some(1.05), // Prevent content repetition
                stop: None,
            })
        } else {
            None
        };

        // Thinking: Explicitly control based on thinking_enabled parameter
        // When thinking_enabled is Some(false), disable thinking for faster responses
        // When thinking_enabled is Some(true) or None, use model default or enable thinking
        let model_supports_thinking = caps.supports_thinking;
        let user_requested_thinking = input.params.thinking_enabled;

        // Determine the think parameter:
        // - Some(false) -> explicitly disable thinking (important for multimodal!)
        // - Some(true) -> explicitly enable thinking
        // - None -> use model default (pass nothing)
        let think: Option<OllamaThink> = match user_requested_thinking {
            Some(false) => Some(OllamaThink::Bool(false)), // Explicitly disable
            Some(true) if model_supports_thinking => Some(OllamaThink::Bool(true)), // Explicitly enable
            Some(true) => None, // Model doesn't support thinking, don't send parameter
            None => None,       // Use model default
        };

        // Determine if we should send thinking to the client (for display purposes)
        let should_send_thinking = user_requested_thinking.unwrap_or(model_supports_thinking);

        // Convert messages with tool injection for non-native models
        let messages = self.messages_to_ollama_with_tools(
            &input.messages,
            input.tools.as_deref(),
            supports_native_tools,
        );

        tracing::debug!(
            "Ollama: generate_stream - URL: {}, messages: {}, native_tools: {}, text_tools: {}",
            url,
            messages.len(),
            native_tools.is_some(),
            !supports_native_tools && has_tools
        );

        // When tools are present, disable thinking to prevent wasting tokens
        // and ensure tool calls are generated efficiently
        let format: Option<String> = None;

        // Log request details before creating the request
        tracing::debug!(
            "Ollama: stream request config - has_tools={}, think={:?}",
            native_tools.as_ref().is_some_and(|t| !t.is_empty()),
            think
        );

        // Capture stream_config for use in async block
        let stream_config = self.stream_config.clone();

        tokio::spawn(async move {
            let request = OllamaChatRequest {
                model: model.clone(),
                messages,
                stream: true,
                options,
                think,
                tools: native_tools,
                format,
            };

            let request_json = match serde_json::to_string(&request) {
                Ok(json) => {
                    tracing::debug!("Ollama: stream request prepared for model: {}", model);
                    json
                }
                Err(e) => {
                    let _ = tx.send(Err(LlmError::Serialization(e))).await;
                    return;
                }
            };

            let result = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(request_json)
                .send()
                .await;

            match result {
                Ok(response) => {
                    // BUSINESS LOG: LLM request started
                    tracing::info!("🚀 LLM request started: model={}", model);
                    let status = response.status();
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        println!("[ollama.rs] Ollama error response: {}", body);
                        let _ = tx
                            .send(Err(LlmError::Generation(format!(
                                "Ollama error {}: {}",
                                status.as_u16(),
                                body
                            ))))
                            .await;
                        return;
                    }

                    // Handle SSE stream
                    use futures::StreamExt as _;
                    let mut byte_stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    let mut _sent_done = false;
                    let mut tool_calls_sent = false; // Track if tool_calls have been sent
                    let mut total_bytes = 0usize;
                    let mut total_chars = 0usize; // Track total output characters
                    let mut thinking_chars = 0usize; // Track thinking characters separately
                    let mut thinking_start_time: Option<Instant> = None; // Track when thinking started
                    let mut terminate_early = false; // Flag to terminate stream early
                    let mut skip_remaining_thinking = false; // Skip thinking chunks but wait for content
                    let mut thinking_skip_start: Option<Instant> = None; // Track when thinking skip started
                    let mut last_thinking_chunk = String::new(); // Track last thinking chunk for loop detection
                    let mut consecutive_same_thinking = 0usize; // Count consecutive identical thinking chunks
                    let stream_start = Instant::now(); // Track stream duration
                    let mut last_progress_report = Instant::now(); // Track last progress report
                    let mut last_warning_index = 0usize; // Track last warning threshold sent
                    let mut terminate_early_reason: Option<String> = None; // Track reason for early termination

                    while let Some(chunk_result) = byte_stream.next().await {
                        // Check for early termination flag
                        if terminate_early {
                            tracing::warn!(
                                "[ollama.rs] Early termination: {}",
                                terminate_early_reason
                                    .as_deref()
                                    .unwrap_or("unknown reason")
                            );
                            // Send error to client
                            let _ = tx
                                .send(Err(LlmError::Generation(
                                    terminate_early_reason
                                        .unwrap_or_else(|| "Stream terminated early".to_string()),
                                )))
                                .await;
                            break;
                        }

                        let elapsed = stream_start.elapsed();

                        // P0.2: Check and report progress at intervals
                        if stream_config.progress_enabled
                            && last_progress_report.elapsed() > Duration::from_secs(5)
                        {
                            let elapsed_secs = elapsed.as_secs();
                            let max_duration =
                                Duration::from_secs(stream_config.max_stream_duration_secs);
                            let remaining = max_duration.saturating_sub(elapsed);

                            // Send progress update through a special content marker
                            // We encode progress as a special comment in the thinking stream
                            let _progress_json = serde_json::json!({
                                "type": "progress",
                                "elapsed": elapsed_secs,
                                "remaining": remaining.as_secs(),
                                "stage": "streaming"
                            })
                            .to_string();

                            tracing::debug!(
                                "Stream progress: {}s elapsed, {}s remaining",
                                elapsed_secs,
                                remaining.as_secs()
                            );

                            last_progress_report = Instant::now();
                        }

                        // P0.2: Check warning thresholds
                        if stream_config.progress_enabled {
                            for (i, threshold) in
                                stream_config.warning_thresholds.iter().enumerate()
                            {
                                if i >= last_warning_index
                                    && elapsed >= Duration::from_secs(*threshold)
                                {
                                    let elapsed_secs = elapsed.as_secs();
                                    let max_duration =
                                        Duration::from_secs(stream_config.max_stream_duration_secs);
                                    let remaining = max_duration.saturating_sub(elapsed);

                                    // Send warning through progress mechanism
                                    let _warning_json = serde_json::json!({
                                        "type": "warning",
                                        "message": format!("执行中... 已耗时 {} 秒，剩余约 {} 秒",
                                            elapsed_secs, remaining.as_secs()),
                                        "elapsed": elapsed_secs,
                                        "remaining": remaining.as_secs()
                                    })
                                    .to_string();

                                    tracing::info!(
                                        "Stream warning at {}s: {}s remaining",
                                        elapsed_secs,
                                        remaining.as_secs()
                                    );

                                    last_warning_index = i + 1;
                                }
                            }
                        }

                        // Check for timeout
                        let max_duration =
                            Duration::from_secs(stream_config.max_stream_duration_secs);
                        if elapsed > max_duration {
                            let error_msg = format!(
                                "Stream timeout after {} seconds",
                                stream_config.max_stream_duration_secs
                            );
                            println!("[ollama.rs] {}", error_msg);
                            tracing::warn!("{}", error_msg);
                            let _ = tx.send(Err(LlmError::Generation(error_msg))).await;
                            return;
                        }
                        match chunk_result {
                            Ok(chunk) => {
                                // Empty chunks are normal in HTTP streaming - don't break on them
                                // Just skip empty chunks but continue processing
                                if chunk.is_empty() {
                                    tracing::debug!(
                                        "[ollama.rs] Skipping empty chunk, continuing stream"
                                    );
                                    continue;
                                }
                                total_bytes += chunk.len();
                                buffer.extend_from_slice(&chunk);

                                let mut search_start = 0;
                                while let Some(nl_pos) =
                                    buffer[search_start..].iter().position(|&b| b == b'\n')
                                {
                                        let line_end = search_start + nl_pos;
                                        let line_bytes = &buffer[..line_end];
                                        let line =
                                            String::from_utf8_lossy(line_bytes).trim().to_string();

                                        buffer = buffer[line_end + 1..].to_vec();
                                        search_start = 0;

                                        if line.is_empty() {
                                            continue;
                                        }

                                        let json_str = if let Some(prefix) =
                                            line.strip_prefix("data: ")
                                        {
                                            prefix
                                        } else if let Some(prefix) = line.strip_prefix("data:") {
                                            prefix
                                        } else {
                                            &line
                                        };

                                        // Debug: log the raw response
                                        tracing::debug!("Ollama raw response: {}", json_str);

                                        if let Ok(ollama_chunk) =
                                            serde_json::from_str::<OllamaStreamResponse>(json_str)
                                        {
                                            // Handle native tool calls - preserve JSON format to keep tool ID
                                            if !ollama_chunk.message.tool_calls.is_empty() {
                                                // BUSINESS LOG: Tool calls detected
                                                let tool_names: Vec<&str> = ollama_chunk
                                                    .message
                                                    .tool_calls
                                                    .iter()
                                                    .map(|t| t.function.name.as_str())
                                                    .collect();
                                                tracing::info!(
                                                    "🔧 LLM requested {} tool calls: {}",
                                                    tool_names.len(),
                                                    tool_names.join(", ")
                                                );
                                                // Build JSON array to preserve tool IDs (OpenAI-compatible format)
                                                let tool_calls_json: Vec<serde_json::Value> =
                                                    ollama_chunk
                                                        .message
                                                        .tool_calls
                                                        .iter()
                                                        .map(|tc| {
                                                            serde_json::json!({
                                                                "id": tc.id,
                                                                "name": tc.function.name,
                                                                "arguments": tc.function.arguments
                                                            })
                                                        })
                                                        .collect();
                                                let json_str =
                                                    serde_json::to_string(&tool_calls_json)
                                                        .unwrap_or_default();
                                                tracing::debug!(
                                                    "Ollama: converted tool_calls to JSON: {}",
                                                    json_str
                                                );
                                                let _ = tx.send(Ok((json_str, false))).await;

                                                // CRITICAL FIX: Don't return immediately!
                                                // Continue consuming the stream until done=true to avoid:
                                                // 1. Leaving unconsumed data in the HTTP connection
                                                // 2. Causing issues with subsequent requests
                                                // 3. Leaving the stream in an inconsistent state
                                                //
                                                // Set a flag to ignore any further thinking/content after tool_calls
                                                // But still process the stream until Ollama sends done=true
                                                println!(
                                                    "[ollama.rs] Tool calls sent, will continue consuming stream until done=true (ignoring further content)"
                                                );
                                                tool_calls_sent = true;
                                                // Don't return here - let the stream continue until done=true
                                                // Continue to the next iteration to process remaining chunks
                                                continue;
                                            }

                                            // IMPORTANT: Skip processing thinking/content if tool_calls were already sent
                                            if tool_calls_sent {
                                                // Skip - don't send any more chunks to the client
                                            } else if !ollama_chunk.message.thinking.is_empty()
                                                && !skip_remaining_thinking
                                            {
                                                // IMPORTANT: Process content BEFORE checking done flag
                                                // The final chunk with done=true may still contain content that must be sent
                                                // CRITICAL FIX: Only send thinking if user requested it AND model supports it
                                                // qwen3 models generate thinking but we filter it out for performance

                                                // Track thinking characters for loop detection
                                                let thinking_content =
                                                    &ollama_chunk.message.thinking;
                                                thinking_chars += thinking_content.chars().count();
                                                total_chars += thinking_content.chars().count();

                                                // SAFETY CHECK 1: Total characters limit (hard cutoff)
                                                if total_chars > stream_config.max_total_chars {
                                                    tracing::error!(
                                                        "[ollama.rs] CRITICAL: Total chars limit reached ({} > {}). Terminating stream to prevent infinite loop.",
                                                        total_chars,
                                                        stream_config.max_total_chars
                                                    );
                                                    terminate_early_reason = Some(format!(
                                                        "Total output limit reached: {} chars",
                                                        total_chars
                                                    ));
                                                    terminate_early = true;
                                                    break;
                                                }

                                                // Track when thinking started
                                                if thinking_start_time.is_none() {
                                                    thinking_start_time = Some(Instant::now());
                                                }

                                                // Check if thinking has gone on too long
                                                if let Some(start) = thinking_start_time {
                                                    if start.elapsed()
                                                        > stream_config.max_thinking_time()
                                                        && !skip_remaining_thinking
                                                    {
                                                        tracing::warn!(
                                                        "[ollama.rs] Thinking timeout ({:?} elapsed, {} chars). Skipping remaining thinking, waiting for content.",
                                                        start.elapsed(),
                                                        thinking_chars
                                                    );
                                                        // Skip future thinking chunks but continue stream for content
                                                        skip_remaining_thinking = true;
                                                        thinking_skip_start = Some(Instant::now());
                                                    }
                                                }

                                                // If thinking was skipped but model keeps producing thinking for too long, terminate
                                                if skip_remaining_thinking {
                                                    if let Some(skip_start) = thinking_skip_start {
                                                        if skip_start.elapsed() > Duration::from_secs(180) {
                                                            tracing::warn!(
                                                                "[ollama.rs] Model still producing thinking 180s after timeout. Terminating stream."
                                                            );
                                                            terminate_early = true;
                                                            terminate_early_reason = Some("Thinking timeout - model stuck in thinking loop".to_string());
                                                        }
                                                    }
                                                }

                                                // Detect consecutive identical thinking chunks (model stuck in loop)
                                                if thinking_content == &last_thinking_chunk {
                                                    consecutive_same_thinking += 1;
                                                    if consecutive_same_thinking
                                                        > stream_config.max_thinking_loop
                                                    {
                                                        tracing::warn!(
                                                            "[ollama.rs] Model stuck in thinking loop ({} identical chunks: \"{}\"). Skipping remaining thinking, waiting for content.",
                                                            consecutive_same_thinking,
                                                            thinking_content
                                                        );
                                                        // Skip future thinking chunks but continue stream for content
                                                        if !skip_remaining_thinking {
                                                            skip_remaining_thinking = true;
                                                            thinking_skip_start = Some(Instant::now());
                                                        }
                                                    }
                                                } else {
                                                    consecutive_same_thinking = 0;
                                                    last_thinking_chunk = thinking_content.clone();
                                                }

                                                // SAFETY CHECK 2: Detect if model is stuck in thinking loop
                                                if thinking_chars > stream_config.max_thinking_chars
                                                {
                                                    tracing::warn!(
                                                        "[ollama.rs] Max thinking chars reached ({} > {}). Skipping remaining thinking chunks, waiting for content.",
                                                        thinking_chars,
                                                        stream_config.max_thinking_chars
                                                    );
                                                    // Skip future thinking chunks but continue stream for content
                                                    if !skip_remaining_thinking {
                                                        skip_remaining_thinking = true;
                                                        thinking_skip_start = Some(Instant::now());
                                                    }
                                                }

                                                if should_send_thinking {
                                                    let _ = tx
                                                        .send(Ok((
                                                            ollama_chunk.message.thinking.clone(),
                                                            true,
                                                        )))
                                                        .await;
                                                } else {
                                                    // Skip thinking content - model generated it but we don't want it
                                                    tracing::debug!(
                                                        "Ollama generated thinking (len={}, total_thinking={}) but filtering it out (user_requested={:?}, model_supports={})",
                                                        ollama_chunk
                                                            .message
                                                            .thinking
                                                            .chars()
                                                            .count(),
                                                        thinking_chars,
                                                        user_requested_thinking,
                                                        model_supports_thinking
                                                    );
                                                    // Don't send thinking chunks to the client
                                                }
                                            }

                                            // Then send response content (final answer)
                                            // Only process content if tool_calls haven't been sent yet
                                            if !tool_calls_sent
                                                && !ollama_chunk.message.content.is_empty()
                                            {
                                                let content = &ollama_chunk.message.content;
                                                // Content from Ollama's message.content is the actual response.
                                                // Thinking is already separated in message.thinking field.
                                                total_chars += content.chars().count();
                                                let _ = tx.send(Ok((content.clone(), false))).await;
                                            }

                                            if ollama_chunk.done {
                                                // BUSINESS LOG: Stream completion summary
                                                // Use accumulated counters, not final chunk (which is often empty)
                                                let actual_content_len =
                                                    total_chars.saturating_sub(thinking_chars);

                                                tracing::info!(
                                                    "✅ LLM stream complete: thinking={} chars, content={} chars, total_chunks={}, prompt_eval={:?}, eval={:?}",
                                                    thinking_chars,
                                                    actual_content_len,
                                                    total_bytes / 300, // Rough chunk count estimate
                                                    ollama_chunk.prompt_eval_count,
                                                    ollama_chunk.eval_count
                                                );

                                                // Send token usage as in-band marker before closing
                                                if let Some(prompt_tokens) =
                                                    ollama_chunk.prompt_eval_count
                                                {
                                                    let _ = tx
                                                        .send(Ok((
                                                            format!(
                                                                "\n__NEOMIND_TOKEN_PROMPT:{}__",
                                                                prompt_tokens
                                                            ),
                                                            false,
                                                        )))
                                                        .await;
                                                }

                                                // Warn if no content was generated (possible token budget issue)
                                                if actual_content_len == 0 && tool_calls_sent {
                                                    tracing::warn!(
                                                        "⚠️  Stream ended with tool calls but no content. Tool execution will follow."
                                                    );
                                                } else if actual_content_len == 0 {
                                                    tracing::warn!(
                                                        "⚠️  Stream ended with no content! Token budget may have been exhausted during thinking."
                                                    );
                                                }

                                                _sent_done = true;
                                                return;
                                            }
                                        }
                                    }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::Network(e.to_string()))).await;
                                return;
                            }
                        }
                    }

                    if !_sent_done {
                        // Channel closed without done signal - log this as it might indicate a problem
                        println!(
                            "[ollama.rs] Stream closed without done=true signal, total_bytes: {}",
                            total_bytes
                        );
                        tracing::warn!(
                            "Ollama stream closed prematurely without done signal, {} bytes transferred",
                            total_bytes
                        );
                        // Receiver will handle EOF
                    }
                }
                Err(e) => {
                    println!("[ollama.rs] Request failed: {}", e);
                    let _ = tx.send(Err(LlmError::Network(e.to_string()))).await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn max_context_length(&self) -> usize {
        // Prefer capabilities_override (from /api/show), fall back to name-based detection
        self.capabilities_override
            .as_ref()
            .map(|c| c.max_context)
            .unwrap_or_else(|| detect_model_capabilities(&self.model).max_context)
    }

    fn supports_multimodal(&self) -> bool {
        true
    }

    fn capabilities(&self) -> BackendCapabilities {
        // Use override if available (from storage/API detection), otherwise detect from name
        let caps = self
            .capabilities_override
            .as_ref()
            .cloned()
            .unwrap_or_else(|| detect_model_capabilities(&self.model));

        let mut builder = BackendCapabilities::builder()
            .streaming()
            .max_context(caps.max_context);

        // Conditionally add capabilities based on model detection
        if caps.supports_multimodal {
            builder = builder.multimodal();
        }
        if caps.supports_thinking {
            builder = builder.thinking_display();
        }
        if caps.supports_tools {
            builder = builder.function_calling();
        }

        builder.build()
    }

    fn metrics(&self) -> BackendMetrics {
        self.metrics
            .read()
            .map(|m| m.clone())
            .unwrap_or_else(|_| BackendMetrics::default())
    }
}

// Ollama API types

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    /// Enable thinking/reasoning output (true/false or "high"/"medium"/"low")
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<OllamaThink>,
    /// Tools for function calling (OpenAI-compatible format)
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    /// Output format - use "json" to disable thinking for tool calls
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

/// Thinking level for Ollama models that support reasoning.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OllamaThink {
    /// Boolean enable/disable
    Bool(bool),
    /// Reasoning intensity level
    Level(String),
}

impl From<bool> for OllamaThink {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<&str> for OllamaThink {
    fn from(value: &str) -> Self {
        Self::Level(value.to_string())
    }
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    #[serde(skip_serializing_if = "String::is_empty")]
    role: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    /// Tool calls made by the assistant (for multi-turn conversations)
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
    /// Tool name for tool result messages (Ollama-specific format)
    /// When role is "tool", this field specifies which tool this result is for
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
    /// Context window size - CRITICAL for Qwen3 to prevent repetition loops
    /// Qwen3 requires >= 16k context to avoid infinite repetition (Ollama 0.6.7+ fix)
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<usize>,
    /// Repeat penalty to prevent model from repeating itself (1.0 = disabled, higher = more penalty)
    #[serde(skip_serializing_if = "Option::is_none")]
    repeat_penalty: Option<f32>,
    /// Stop sequences to prevent model from generating unwanted content
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

/// Model capability information
#[derive(Debug, Clone)]
pub struct ModelCapability {
    pub supports_tools: bool,
    pub supports_thinking: bool,
    pub supports_multimodal: bool,
    /// Maximum context window in tokens
    pub max_context: usize,
}

/// Detect model capabilities from model name
///
/// Based on Ollama official documentation: https://docs.ollama.com/capabilities/thinking
/// Supported thinking models: Qwen 3, GPT-OSS, DeepSeek-v3.1, DeepSeek R1
///
/// Context window sizes are based on official model documentation:
/// - Qwen2/Qwen2.5: 32k for most variants, 128k for some
/// - Qwen3/Qwen3-VL: 32k
/// - Llama 3.x: 8k for 8b models
/// - DeepSeek R1: 64k
/// - Mistral: 32k
/// - Gemma: 8k
/// - Phi: 32k for Phi-3
fn detect_model_capabilities(model_name: &str) -> ModelCapability {
    let name_lower = model_name.to_lowercase();

    // Models that support thinking/reasoning (from official Ollama docs)
    // - Qwen 3 family (qwen3, qwen3-vl, qwen3:2b, etc.)
    // - GPT-OSS (uses low/medium/high levels)
    // - DeepSeek-v3.1
    // - DeepSeek R1 (deepseek-r1)
    // - Also catch models with "thinking" in the name for future compatibility
    let supports_thinking = name_lower.starts_with("qwen3")
        || name_lower.contains("qwen3-")
        || name_lower.contains("gpt-oss")
        || name_lower.contains("deepseek-r1")
        || name_lower.contains("deepseek-r")
        || name_lower.contains("deepseek v3.1")
        || name_lower.contains("deepseek-v3.1")
        || name_lower.contains("thinking"); // Future-proofing

    // Models that support function calling
    // Note: Smaller models like gemma3:270m do NOT support tools
    let supports_tools = !name_lower.contains("270m")
        && !name_lower.contains("1b")
        && !name_lower.contains("tiny")
        && !name_lower.contains("micro")
        && !name_lower.contains("nano");

    // Models that support multimodal (vision)
    // Common Ollama vision models: qwen-vl, qwen2-vl, qwen3-vl, llava, bakllava, moondream, etc.
    // IMPORTANT: This is name-based heuristic detection. For accurate detection,
    // use Ollama's /api/show endpoint through runtime.capabilities() method.
    let supports_multimodal = name_lower.contains("vl")
        || name_lower.contains("vision")
        || name_lower.contains("-mm")
        || name_lower.contains(":mm")
        || name_lower.contains("llava")
        || name_lower.contains("bakllava")
        || name_lower.contains("moondream")
        || name_lower.contains("minigpt")
        || name_lower.contains("clip")
        || name_lower.contains("minicpm-v")
        || name_lower.contains("pixtral")  // Mistral's vision model
        || name_lower.contains("llama3.2-vision")
        || name_lower.contains("gemma3")   // Gemma 3 supports vision
        || name_lower.contains("ministral") // Ministral models support vision
        || name_lower.contains("mistral3")   // Mistral3 architecture supports vision
        || name_lower.contains("cogvlm")
        || name_lower.contains("internvl")
        || name_lower.contains("yi-vl")
        || name_lower.contains("deepseek-vl")
        || name_lower.contains("multimodal")
        || name_lower.contains("qwen3.5")    // Qwen3.5 supports multimodal natively
        // Check for common vision model patterns with version numbers
        || name_lower.contains("qwen") && (name_lower.contains("-vl") || name_lower.contains(":vl"))
        || name_lower.contains("llama") && name_lower.contains("vision");

    // Detect maximum context window based on model family
    let max_context = detect_model_context(model_name);

    ModelCapability {
        supports_tools,
        supports_thinking,
        supports_multimodal,
        max_context,
    }
}

/// Detect maximum context window size for a model.
///
/// Returns the maximum context window in tokens.
/// Falls back to 4096 for unknown models (safe default).
pub fn detect_model_context(model_name: &str) -> usize {
    let name_lower = model_name.to_lowercase();

    // Qwen family (qwen, qwen2, qwen2.5, qwen3, qwen3-vl)
    if name_lower.starts_with("qwen") {
        // Qwen3 and Qwen3-VL support 32k context
        if name_lower.starts_with("qwen3") {
            return 32_768;
        }
        // Qwen2.5 typically supports 32k
        if name_lower.starts_with("qwen2.5") || name_lower.contains("qwen2_5") {
            return 32_768;
        }
        // Qwen2 typically supports 32k
        if name_lower.starts_with("qwen2") {
            return 32_768;
        }
        // Other Qwen models - default to 32k
        return 32_768;
    }

    // Llama 3.x family
    if name_lower.starts_with("llama3") || name_lower.contains("llama-3") {
        // Llama 3.x 8b models typically support 8k context
        if name_lower.contains("8b") {
            return 8_192;
        }
        // Llama 3.x 70b+ models may support larger context
        if name_lower.contains("70b") || name_lower.contains("405b") {
            return 128_000;
        }
        // Default for llama3
        return 8_192;
    }

    // Llama 3.1/3.2/3.3 family (extended context)
    if name_lower.contains("llama3.1")
        || name_lower.contains("llama3_1")
        || name_lower.contains("llama3.2")
        || name_lower.contains("llama3_2")
        || name_lower.contains("llama3.3")
        || name_lower.contains("llama3_3")
    {
        return 128_000;
    }

    // Llama 3.4 and beyond
    if name_lower.contains("llama3.4") || name_lower.contains("llama3_4") {
        return 128_000;
    }

    // DeepSeek family
    if name_lower.starts_with("deepseek") {
        // DeepSeek R1 supports 64k context
        if name_lower.contains("deepseek-r1") || name_lower.contains("deepseek_r1") {
            return 64_000;
        }
        // DeepSeek V3 supports 64k context
        if name_lower.contains("deepseek-v3") || name_lower.contains("deepseek_v3") {
            return 64_000;
        }
        // Default DeepSeek models
        return 64_000;
    }

    // Mistral family (including ministral/Mistral Small)
    if name_lower.starts_with("mistral")
        || name_lower.starts_with("ministral")
        || name_lower.contains("mixtral")
    {
        // Ministral (Mistral Small) supports 128k context
        if name_lower.starts_with("ministral") {
            return 128_000;
        }
        // Mixtral models typically support 32k
        if name_lower.contains("mixtral") {
            return 32_768;
        }
        // Mistral Large supports 128k
        if name_lower.contains("large") {
            return 128_000;
        }
        // Mistral 7B / Nemo supports 32k
        return 32_768;
    }

    // Gemma family
    if name_lower.starts_with("gemma") {
        // Gemma 2 typically supports 8k-9k depending on variant
        if name_lower.contains("gemma2") || name_lower.contains("gemma-2") {
            return 9_216;
        }
        // Gemma 1 typically supports 8k
        return 8_192;
    }

    // Phi family
    if name_lower.starts_with("phi") {
        // Phi-3 supports 32k context
        if name_lower.contains("phi-3") || name_lower.contains("phi3") {
            return 32_768;
        }
        // Phi-2 supports 2k context
        if name_lower.contains("phi-2") || name_lower.contains("phi2") {
            return 2_048;
        }
        // Default Phi
        return 32_768;
    }

    // Qwen family (another check for alternative naming)
    if name_lower.contains("qwen") {
        return 32_768;
    }

    // Code models
    if name_lower.contains("codellama") || name_lower.contains("code-llama") {
        return 16_384;
    }

    if name_lower.contains("codestral") {
        return 32_768;
    }

    // Safe default for unknown models
    // Using 128k as modern models generally support this
    128_000
}

/// Detect backend capabilities for an Ollama model from its name.
///
/// This is a public helper function that can be used when creating `LlmBackend`
/// to provide accurate capabilities instead of `None`.
///
/// # Example
/// ```ignore
/// use neomind_agent::llm_backends::ollama::detect_ollama_capabilities;
///
/// let caps = detect_ollama_capabilities("qwen3-vl:2b");
/// let backend = LlmBackend::Ollama {
///     endpoint: "http://localhost:11434".to_string(),
///     model: "qwen3-vl:2b".to_string(),
///     capabilities: Some(caps),
/// };
/// ```
pub fn detect_ollama_capabilities(model_name: &str) -> BackendCapabilities {
    let caps = detect_model_capabilities(model_name);
    let mut builder = BackendCapabilities::builder()
        .streaming()
        .max_context(caps.max_context);

    if caps.supports_multimodal {
        builder = builder.multimodal();
    }
    if caps.supports_thinking {
        builder = builder.thinking_display();
    }
    if caps.supports_tools {
        builder = builder.function_calling();
    }

    builder.build()
}

/// Tool definition in OpenAI-compatible format for Ollama.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaTool {
    /// Tool type (always "function" for function calling)
    #[serde(rename = "type")]
    tool_type: String,
    /// Function definition
    function: OllamaToolFunction,
}

/// Function definition for tool calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolFunction {
    /// Function name
    name: String,
    /// Function description
    description: String,
    /// Function parameters as JSON Schema
    parameters: serde_json::Value,
}

/// Response from Ollama /api/show endpoint
#[derive(Debug, Clone, Deserialize)]
struct OllamaShowResponse {
    /// Model name (not always present in /api/show response)
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    /// Model capabilities from Ollama API, e.g. ["completion", "vision", "tools", "thinking"]
    #[serde(default)]
    capabilities: Vec<String>,
    /// Model details - Ollama returns flat key-value pairs like "llama.context_length": 8192
    #[serde(default)]
    model_info: std::collections::HashMap<String, serde_json::Value>,
}

impl OllamaShowResponse {
    /// Check if the model supports vision/multimodal.
    /// Prioritizes the Ollama API `capabilities` array (accurate) over model_info heuristic.
    fn supports_vision(&self) -> bool {
        // Priority 1: Check Ollama's capabilities array — the authoritative source
        if self.capabilities.iter().any(|c| c == "vision") {
            return true;
        }
        // Priority 2: Fallback to model_info keys for older Ollama versions
        if self.model_info.keys().any(|k| {
            k.contains(".vision.")
                || k.contains("vision_encoder")
                || k.contains("projector")
                || k.contains("image_token_id")
        }) {
            return true;
        }
        false
    }

    /// Get the context length
    fn context_length(&self) -> Option<usize> {
        for (key, value) in &self.model_info {
            if key.ends_with(".context_length") {
                if let Some(v) = value.as_u64() {
                    return Some(v as usize);
                }
            }
        }
        None
    }

    /// Check if model has attention heads (indicates reasoning capability)
    fn has_attention_heads(&self) -> bool {
        self.model_info
            .keys()
            .any(|k| k.contains(".attention.head_count"))
    }
}

/// Tool call returned by the model.
#[derive(Debug, Clone, Deserialize)]
struct OllamaToolCall {
    /// Tool call ID (for tracking)
    #[allow(dead_code)]
    id: Option<String>,
    /// Type of tool call (always "function")
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    /// Function to call
    function: OllamaCalledFunction,
}

/// Function call details.
#[derive(Debug, Clone, Deserialize)]
struct OllamaCalledFunction {
    /// Index of this function call in multi-tool scenarios (Ollama-specific, reserved)
    #[serde(default)]
    _index: Option<usize>,
    /// Function name
    name: String,
    /// Function arguments - can be either a JSON object or string
    #[serde(deserialize_with = "deserialize_arguments")]
    arguments: serde_json::Value,
}

/// Deserialize arguments field - handles both JSON object and string formats
fn deserialize_arguments<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ArgumentsVisitor;

    impl<'de> serde::de::Visitor<'de> for ArgumentsVisitor {
        type Value = serde_json::Value;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a JSON object or string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            serde_json::from_str(value).map_err(E::custom)
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            serde_json::Value::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(ArgumentsVisitor)
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OllamaChatResponse {
    model: String,
    created_at: String,
    message: OllamaResponseMessage,
    done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_eval_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    eval_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamResponse {
    done: bool,
    #[serde(default)]
    message: OllamaResponseMessage,
    /// Prompt tokens evaluated (available in final chunk when done=true)
    #[serde(default)]
    prompt_eval_count: Option<usize>,
    /// Completion tokens generated (available in final chunk when done=true)
    #[serde(default)]
    eval_count: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct OllamaResponseMessage {
    #[serde(default)]
    _role: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: String,
    /// Tool calls made by the model (OpenAI-compatible format)
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

use tokio_stream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_config() {
        let config = OllamaConfig::new("llama3:8b");
        assert_eq!(config.model, "llama3:8b");
        assert_eq!(config.endpoint, "http://localhost:11434");
    }

    #[test]
    fn test_ollama_config_with_endpoint() {
        let config = OllamaConfig::new("qwen3-vl:2b").with_endpoint("http://192.168.1.100:11434");
        assert_eq!(config.endpoint, "http://192.168.1.100:11434");
    }
}
