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

use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use edge_ai_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput, LlmRuntime,
    StreamChunk, TokenUsage,
};
use edge_ai_core::message::{Content, ContentPart, Message, MessageRole};

/// Stream timeout in seconds - prevents infinite loops
const STREAM_TIMEOUT_SECS: u64 = 300;

/// Maximum thinking characters - set to limit to prevent infinite thinking loops
/// Some models (like qwen3-vl:2b) can get stuck in thinking loops
const MAX_THINKING_CHARS: usize = 1000;

/// Maximum consecutive identical thinking chunks before assuming loop
const MAX_THINKING_LOOP: usize = 3;

/// Maximum thinking time before forcing completion (in seconds)
const MAX_THINKING_TIME_SECS: u64 = 10;

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
    180
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
            timeout_secs: 180,
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
        Self::new("qwen3-vl:2b")
    }
}

/// Ollama runtime backend.
pub struct OllamaRuntime {
    config: OllamaConfig,
    client: Client,
    model: String,
    metrics: Arc<RwLock<BackendMetrics>>,
}

impl OllamaRuntime {
    /// Create a new Ollama runtime.
    pub fn new(config: OllamaConfig) -> Result<Self, LlmError> {
        tracing::debug!("Creating Ollama runtime with endpoint: {}", config.endpoint);

        // Configure HTTP client with connection pooling for better performance
        // - pool_max_idle_per_host: Keep up to 5 idle connections ready for reuse
        // - pool_idle_timeout: Close idle connections after 90 seconds
        // - connect_timeout: Fail fast if server doesn't respond within 5 seconds
        // - http2_prior_knowledge: Skip ALPN negotiation for local Ollama
        let client = Client::builder()
            .timeout(config.timeout())
            .pool_max_idle_per_host(5)                      // Keep 5 idle connections
            .pool_idle_timeout(Duration::from_secs(90))      // Close after 90s idle
            .connect_timeout(Duration::from_secs(5))         // Fast connection fail
            .http2_keep_alive_interval(Duration::from_secs(30)) // Keep HTTP/2 alive
            .http2_keep_alive_timeout(Duration::from_secs(10))  // Keep-alive timeout
            .http2_adaptive_window(true)                     // Adaptive flow control
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let model = config.model.clone();

        Ok(Self {
            config,
            client,
            model,
            metrics: Arc::new(RwLock::new(BackendMetrics::default())),
        })
    }

    /// Format tools for text-based tool calling (for models without native tool support).
    /// Uses JSON format instead of XML for better model compatibility.
    fn format_tools_for_text_calling(
        tools: &[edge_ai_core::llm::backend::ToolDefinition],
    ) -> String {
        let mut result = String::from("## Tool Calling Requirements\n");
        result.push_str("You must call tools using JSON format. Do not just describe what to do.\n\n");
        result.push_str("Format:\n");
        result.push_str("[{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}]\n\n");

        result.push_str("## Examples\n");
        result.push_str("User: 有哪些设备？\n");
        result.push_str("Assistant: [{\"name\": \"list_devices\", \"arguments\": {}}]\n\n");

        result.push_str("User: 把客厅灯打开\n");
        result.push_str("Assistant: [{\"name\": \"control_device\", \"arguments\": {\"device_id\": \"light_living\", \"action\": \"on\"}}]\n\n");

        result.push_str("## Available Tools\n\n");

        for tool in tools {
            result.push_str(&format!("### {}\n", tool.name));
            result.push_str(&format!("Description: {}\n", tool.description));

            if let Some(props) = tool.parameters.get("properties")
                && let Some(obj) = props.as_object()
                    && !obj.is_empty() {
                        result.push_str("Parameters:\n");
                        for (name, prop) in obj {
                            let desc = prop
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("No description");
                            let type_name = prop
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("unknown");
                            result.push_str(&format!("- {}: {} ({})\n", name, desc, type_name));
                        }
                    }

            if let Some(required) = tool.parameters.get("required")
                && let Some(arr) = required.as_array()
                    && !arr.is_empty() {
                        let required_names: Vec<&str> =
                            arr.iter().filter_map(|v| v.as_str()).collect();
                        result.push_str(&format!("Required: {}\n", required_names.join(", ")));
                    }

            result.push('\n');
        }

        result.push_str("## Important Rules\n");
        result.push_str("1. ALWAYS output tool calls as a JSON array\n");
        result.push_str("2. When user asks about devices, call list_devices\n");
        result.push_str("3. When user asks about data, call query_data\n");
        result.push_str("4. When user asks to control device, call control_device\n");
        result.push_str("5. Don't explain, just call the tool directly\n");

        result
    }

    /// Convert messages to Ollama format, optionally injecting tool descriptions.
    fn messages_to_ollama_with_tools(
        &self,
        messages: &[Message],
        tools: Option<&[edge_ai_core::llm::backend::ToolDefinition]>,
        supports_native_tools: bool,
    ) -> Vec<OllamaMessage> {
        let tool_instructions = if !supports_native_tools && tools.is_some_and(|t| !t.is_empty())
        {
            Some(Self::format_tools_for_text_calling(tools.unwrap()))
        } else {
            None
        };

        messages
            .iter()
            .map(|msg| {
                // Extract text content
                let mut text = msg.text();

                // Inject tool instructions into system message for models without native tool support
                if msg.role == MessageRole::System
                    && let Some(instructions) = &tool_instructions {
                        text = format!("{}\n\n{}", text, instructions);
                    }

                // Extract images from multimodal content
                let images = extract_images_from_content(&msg.content);

                OllamaMessage {
                    role: match msg.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::System => "system",
                    }
                    .to_string(),
                    content: text,
                    images,
                    tool_calls: None, // Will be populated from AgentMessage.tool_calls if needed
                    tool_call_id: None,
                }
            })
            .collect()
    }

    /// Convert messages to Ollama format (backward compatibility).
    fn messages_to_ollama(&self, messages: &[Message]) -> Vec<OllamaMessage> {
        self.messages_to_ollama_with_tools(messages, None, true)
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
        input: edge_ai_core::llm::backend::LlmInput,
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

        // Build stop sequences
        let stop_sequences = if has_tools {
            Some(vec!["\n\nUser:".to_string()])
        } else {
            None
        };

        let options = if input.params.temperature.is_some()
            || num_predict.is_some()
            || input.params.top_p.is_some()
            || stop_sequences.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict,
                top_p: input.params.top_p,
                repeat_penalty: None,
                stop: stop_sequences,
            })
        } else {
            None
        };

        // Thinking: Don't pass think parameter at all to avoid qwen3-vl:2b thinking loops
        // The think parameter doesn't work reliably for qwen3-vl:2b
        let think: Option<OllamaThink> = None;

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

        let request_json =
            serde_json::to_string(&request).map_err(LlmError::Serialization)?;
        tracing::debug!("Ollama: sending request to model: {}", model);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_json)
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

        // Handle response - detect if content is actually thinking
        // Some models (qwen3-vl, qwen3:1.7b) put thinking in the content or thinking field
        // We need to detect and filter this out
        let (mut response_text, detected_thinking_in_content) =
            if ollama_response.message.content.is_empty() {
                // Content is empty - check if thinking field has the response
                if ollama_response.message.thinking.is_empty() {
                    // Both content and thinking are empty - truly empty response
                    (String::new(), false)
                } else {
                    // Thinking field has content - use it as response
                    // This is the expected behavior for models like qwen3:1.7b
                    (ollama_response.message.thinking.clone(), true)
                }
            } else {
                // Content is not empty - check if it's actually thinking
                let content = &ollama_response.message.content;
                let _thinking = &ollama_response.message.thinking;

                // Check for common thinking patterns that should be filtered
                let is_likely_thinking = content.starts_with("好的，用户")
                    || content.starts_with("首先，")
                    || content.starts_with("让我")
                    || content.starts_with("我需要")
                    || (content.len() > 200
                        && content.contains("我需要确定")
                        && content.contains("根据"));

                if is_likely_thinking {
                    // Content appears to be thinking - return empty and use thinking field instead
                    (String::new(), true)
                } else {
                    // Content is actual response
                    (content.clone(), false)
                }
            };

        // Handle native tool calls from Ollama
        if !ollama_response.message.tool_calls.is_empty() {
            tracing::debug!(
                "Ollama: received {} native tool calls",
                ollama_response.message.tool_calls.len()
            );
            let mut xml_buffer = String::from("<tool_calls>");
            for tool_call in &ollama_response.message.tool_calls {
                xml_buffer.push_str(&format!("<invoke name=\"{}\">", tool_call.function.name));
                if let Some(obj) = tool_call.function.arguments.as_object() {
                    for (key, value) in obj {
                        xml_buffer.push_str(&format!(
                            "<parameter name=\"{}\">{}</parameter>",
                            key,
                            value.as_str().unwrap_or(&value.to_string())
                        ));
                    }
                }
                xml_buffer.push_str("</invoke>");
            }
            xml_buffer.push_str("</tool_calls>");
            response_text.push_str(&xml_buffer);
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
            // If content was detected as thinking, include it in the thinking field
            thinking: if detected_thinking_in_content {
                // Content was thinking - combine content and thinking fields
                let combined = if ollama_response.message.thinking.is_empty() {
                    ollama_response.message.content.clone()
                } else {
                    format!(
                        "{}\n{}",
                        ollama_response.message.content, ollama_response.message.thinking
                    )
                };
                Some(combined)
            } else if ollama_response.message.thinking.is_empty() {
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
        input: edge_ai_core::llm::backend::LlmInput,
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

        // Build stop sequences
        let stop_sequences = if has_tools {
            Some(vec!["\n\nUser:".to_string()])
        } else {
            None
        };

        let options = if input.params.temperature.is_some()
            || num_predict.is_some()
            || input.params.top_p.is_some()
            || stop_sequences.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict,
                top_p: input.params.top_p,
                repeat_penalty: None,
                stop: stop_sequences,
            })
        } else {
            None
        };

        // Thinking: Don't pass think parameter at all to avoid qwen3-vl:2b thinking loops
        // The think parameter doesn't work reliably for qwen3-vl:2b
        let think: Option<OllamaThink> = None;

        // Determine if we should send thinking to the client (for display purposes)
        // Default to true for models that support thinking - user can disable if desired
        let model_supports_thinking = caps.supports_thinking;
        let user_requested_thinking = input.params.thinking_enabled.unwrap_or(true); // Default to true - send thinking
        let should_send_thinking = user_requested_thinking;

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

        // No format parameter needed - let Ollama handle thinking naturally
        let format: Option<String> = None;

        // Log request details before creating the request
        tracing::debug!("Ollama: stream request config - has_tools={}, think={:?}",
            native_tools.as_ref().is_some_and(|t| !t.is_empty()),
            think);

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
                    let mut sent_done = false;
                    let mut tool_calls_sent = false; // Track if tool_calls have been sent
                    let mut total_bytes = 0usize;
                    let mut total_chars = 0usize; // Track total output characters
                    let mut thinking_chars = 0usize; // Track thinking characters separately
                    let mut thinking_start_time: Option<Instant> = None; // Track when thinking started
                    let mut terminate_early = false; // Flag to terminate stream early
                    let mut skip_remaining_thinking = false; // Skip thinking chunks but wait for content
                    let mut last_thinking_chunk = String::new(); // Track last thinking chunk for loop detection
                    let mut consecutive_same_thinking = 0usize; // Count consecutive identical thinking chunks
                    let stream_start = Instant::now(); // Track stream duration
                    let mut content_buffer = String::new(); // Buffer for detecting thinking in content
                    let mut detected_thinking_in_content = false; // Flag for thinking detection

                    while let Some(chunk_result) = byte_stream.next().await {
                        // Check for early termination flag
                        if terminate_early {
                            tracing::info!("[ollama.rs] Early termination requested, ending stream.");
                            break;
                        }

                        // Check for timeout
                        if stream_start.elapsed() > Duration::from_secs(STREAM_TIMEOUT_SECS) {
                            let error_msg =
                                format!("Stream timeout after {} seconds", STREAM_TIMEOUT_SECS);
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
                                loop {
                                    if let Some(nl_pos) =
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
                                            // Handle native tool calls - convert to XML format for compatibility
                                            if !ollama_chunk.message.tool_calls.is_empty() {
                                                // BUSINESS LOG: Tool calls detected
                                                let tool_names: Vec<&str> = ollama_chunk.message.tool_calls
                                                    .iter()
                                                    .map(|t| t.function.name.as_str())
                                                    .collect();
                                                tracing::info!(
                                                    "🔧 LLM requested {} tool calls: {}",
                                                    tool_names.len(),
                                                    tool_names.join(", ")
                                                );
                                                // Convert tool_calls to XML format for streaming.rs compatibility
                                                let mut xml_buffer = String::from("<tool_calls>");
                                                for tool_call in &ollama_chunk.message.tool_calls {
                                                    xml_buffer.push_str(&format!(
                                                        "<invoke name=\"{}\">",
                                                        tool_call.function.name
                                                    ));
                                                    // Convert arguments JSON to XML parameter format
                                                    if let Some(obj) =
                                                        tool_call.function.arguments.as_object()
                                                    {
                                                        for (key, value) in obj {
                                                            xml_buffer.push_str(&format!(
                                                                "<parameter name=\"{}\">{}</parameter>",
                                                                key,
                                                                value.as_str().unwrap_or(&value.to_string())
                                                            ));
                                                        }
                                                    }
                                                    xml_buffer.push_str("</invoke>");
                                                }
                                                xml_buffer.push_str("</tool_calls>");
                                                tracing::debug!(
                                                    "Ollama: converted tool_calls to XML: {}",
                                                    xml_buffer
                                                );
                                                let _ = tx.send(Ok((xml_buffer, false))).await;

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
                                            } else if !ollama_chunk.message.thinking.is_empty() && !skip_remaining_thinking {
                                                // IMPORTANT: Process content BEFORE checking done flag
                                                // The final chunk with done=true may still contain content that must be sent
                                                // CRITICAL FIX: Only send thinking if user requested it AND model supports it
                                                // qwen3 models generate thinking but we filter it out for performance

                                                // Track thinking characters for loop detection
                                                let thinking_content = &ollama_chunk.message.thinking;
                                                thinking_chars += thinking_content.chars().count();
                                                total_chars += thinking_content.chars().count();

                                                // Track when thinking started
                                                if thinking_start_time.is_none() {
                                                    thinking_start_time = Some(Instant::now());
                                                }

                                                // Check if thinking has gone on too long
                                                if let Some(start) = thinking_start_time
                                                    && start.elapsed() > Duration::from_secs(MAX_THINKING_TIME_SECS) {
                                                        tracing::warn!(
                                                            "[ollama.rs] Thinking timeout ({:?} elapsed, {} chars). Skipping remaining thinking, waiting for content.",
                                                            start.elapsed(),
                                                            thinking_chars
                                                        );
                                                        // Skip future thinking chunks but continue stream for content
                                                        skip_remaining_thinking = true;
                                                    }

                                                // Detect consecutive identical thinking chunks (model stuck in loop)
                                                if thinking_content == &last_thinking_chunk {
                                                    consecutive_same_thinking += 1;
                                                    if consecutive_same_thinking > MAX_THINKING_LOOP {
                                                        tracing::warn!(
                                                            "[ollama.rs] Model stuck in thinking loop ({} identical chunks: \"{}\"). Skipping remaining thinking, waiting for content.",
                                                            consecutive_same_thinking,
                                                            thinking_content
                                                        );
                                                        // Skip future thinking chunks but continue stream for content
                                                        skip_remaining_thinking = true;
                                                    }
                                                } else {
                                                    consecutive_same_thinking = 0;
                                                    last_thinking_chunk = thinking_content.clone();
                                                }

                                                // SAFETY CHECK: Detect if model is stuck in thinking loop
                                                if thinking_chars > MAX_THINKING_CHARS {
                                                    tracing::warn!(
                                                        "[ollama.rs] Max thinking chars reached ({}). Skipping remaining thinking chunks, waiting for content.",
                                                        thinking_chars
                                                    );
                                                    // Skip future thinking chunks but continue stream for content
                                                    skip_remaining_thinking = true;
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
                                                        "Ollama generated thinking (len={}, total_thinking={}) but filtering it out (user_requested={}, model_supports={})",
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
                                            if !tool_calls_sent && !ollama_chunk.message.content.is_empty() {
                                                let content = &ollama_chunk.message.content;
                                                content_buffer.push_str(content);

                                                // Detect if content is actually thinking (qwen3-vl puts thinking in content field)
                                                // Skip this detection if we've exceeded thinking limit
                                                if !detected_thinking_in_content && !skip_remaining_thinking {
                                                    // Check initial buffer for thinking patterns
                                                    let is_likely_thinking = content_buffer
                                                        .starts_with("好的，用户")
                                                        || content_buffer.starts_with("首先，")
                                                        || content_buffer.starts_with("让我")
                                                        || content_buffer.starts_with("我需要")
                                                        || (content_buffer.len() > 200
                                                            && content_buffer
                                                                .contains("我需要确定")
                                                            && content_buffer.contains("根据"));

                                                    if is_likely_thinking {
                                                        detected_thinking_in_content = true;
                                                    }
                                                }

                                                // If we've exceeded thinking limit, treat everything as content from now on
                                                if skip_remaining_thinking && detected_thinking_in_content {
                                                    // Reset - treat remaining as real content
                                                    detected_thinking_in_content = false;
                                                    // Send any buffered thinking content first (if not already sent)
                                                    if !content_buffer.is_empty() {
                                                        let _ =
                                                            tx.send(Ok((content_buffer.clone(), false))).await;
                                                        content_buffer.clear();
                                                    }
                                                }

                                                if detected_thinking_in_content {
                                                    // Content is actually thinking - send as thinking
                                                    thinking_chars += content.chars().count();
                                                    total_chars += content.chars().count();
                                                    tracing::debug!(
                                                        "Ollama content detected as thinking: {}",
                                                        content
                                                    );

                                                    // Check thinking limit
                                                    if thinking_chars <= MAX_THINKING_CHARS {
                                                        let _ = tx
                                                            .send(Ok((content.clone(), true)))
                                                            .await;
                                                    } else if !skip_remaining_thinking {
                                                        // Just exceeded limit - set flag and continue
                                                        skip_remaining_thinking = true;
                                                        tracing::warn!(
                                                            "[ollama.rs] Max thinking chars reached in content ({}). Switching to content mode.",
                                                            thinking_chars
                                                        );
                                                    }
                                                } else {
                                                    // Track content characters and send content
                                                    total_chars += content.chars().count();
                                                    let _ =
                                                        tx.send(Ok((content.clone(), false))).await;
                                                }
                                            }

                                            if ollama_chunk.done {
                                                // BUSINESS LOG: Stream completion summary
                                                // Use accumulated counters, not final chunk (which is often empty)
                                                let actual_content_len = total_chars.saturating_sub(thinking_chars);

                                                tracing::info!(
                                                    "✅ LLM stream complete: thinking={} chars, content={} chars, total_chunks={}",
                                                    thinking_chars,
                                                    actual_content_len,
                                                    total_bytes / 300  // Rough chunk count estimate
                                                );

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

                                                sent_done = true;
                                                return;
                                            }
                                        }
                                    } else {
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::Network(e.to_string()))).await;
                                return;
                            }
                        }
                    }

                    if !sent_done {
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
        // Detect the actual context window for the current model
        detect_model_capabilities(&self.model).max_context
    }

    fn supports_multimodal(&self) -> bool {
        true
    }

    fn capabilities(&self) -> BackendCapabilities {
        let caps = detect_model_capabilities(&self.model);
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
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    /// Tool calls made by the assistant (for multi-turn conversations)
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
    /// Tool call ID (for tool response messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    /// Repeat penalty to prevent model from repeating itself (1.0 = disabled, higher = more penalty)
    #[serde(skip_serializing_if = "Option::is_none")]
    repeat_penalty: Option<f32>,
    /// Stop sequences to prevent model from generating unwanted content
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

/// Model capability information
struct ModelCapability {
    supports_tools: bool,
    supports_thinking: bool,
    supports_multimodal: bool,
    /// Maximum context window in tokens
    max_context: usize,
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
    let supports_multimodal =
        name_lower.contains("vl") || name_lower.contains("vision") || name_lower.contains("mm");

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
fn detect_model_context(model_name: &str) -> usize {
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

    // Mistral family
    if name_lower.starts_with("mistral") || name_lower.contains("mixtral") {
        // Mixtral models typically support 32k
        if name_lower.contains("mixtral") {
            return 32_768;
        }
        // Mistral 7B supports 32k
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
    // Using 4096 as a conservative baseline that most models should support
    4_096
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

/// Tool call returned by the model.
#[derive(Debug, Clone, Deserialize)]
struct OllamaToolCall {
    /// Tool call ID (for tracking)
    id: Option<String>,
    /// Type of tool call (always "function")
    #[serde(rename = "type")]
    call_type: Option<String>,
    /// Function to call
    function: OllamaCalledFunction,
}

/// Function call details.
#[derive(Debug, Clone, Deserialize)]
struct OllamaCalledFunction {
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
    use serde::de::Error;

    // Create a visitor that can handle both string and object
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
            // Try to parse the string as JSON
            serde_json::from_str(value).map_err(E::custom)
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            // Deserialize as a JSON object directly
            serde_json::Value::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(ArgumentsVisitor)
}

#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize, Default)]
struct OllamaResponseMessage {
    #[serde(default)]
    role: String,
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
