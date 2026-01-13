//! Ollama LLM backend implementation.
//!
//! Ollama is a local LLM runner that supports various models.
//! This backend communicates with Ollama via its native API.

use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use edge_ai_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput,
    LlmRuntime, StreamChunk, TokenUsage,
};
use edge_ai_core::message::{Content, ContentPart, Message, MessageRole};

/// Ollama configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OllamaConfig {
    /// Ollama endpoint (default: http://localhost:11434)
    pub endpoint: String,

    /// Model name (e.g., "qwen3-vl:2b", "llama3:8b")
    pub model: String,

    /// Request timeout.
    pub timeout: Duration,
}

impl OllamaConfig {
    /// Create a new Ollama config.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            endpoint: "http://localhost:11434".to_string(),
            model: model.into(),
            timeout: Duration::from_secs(120),
        }
    }

    /// Set a custom endpoint.
    /// Note: Ollama uses native API, not OpenAI-compatible. The endpoint should be like
    /// "http://localhost:11434" (without /v1 suffix). If /v1 is provided, it will be stripped.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        let mut endpoint = endpoint.into();
        // Strip /v1 suffix if present (Ollama native API doesn't use it)
        if endpoint.ends_with("/v1") {
            endpoint = endpoint.strip_suffix("/v1")
                .map(|s| s.to_string())
                .unwrap_or_else(|| endpoint.clone());
            // Also remove trailing slash if present
            endpoint = endpoint.strip_suffix("/")
                .unwrap_or(&endpoint)
                .to_string();
        }
        self.endpoint = endpoint;
        self
    }

    /// Set timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
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
        let client = Client::builder()
            .timeout(config.timeout)
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

    /// Convert messages to Ollama format.
    fn messages_to_ollama(&self, messages: &[Message]) -> Vec<OllamaMessage> {
        messages
            .iter()
            .map(|msg| {
                // Extract text content
                let text = msg.text();

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
            ContentPart::ImageBase64 { data, mime_type, .. } => {
                // Already base64 encoded, just remove the mime type prefix if present
                let base64_data = if data.contains(',') {
                    data.split(',').last().unwrap_or(data).to_string()
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
        tracing::warn!("Fetching images from HTTP URLs is not yet supported: {}", url);
        return None;
    }

    // For local file paths, we'd need async file I/O
    // Log that this is not supported and return None
    tracing::warn!("Local file image loading is not yet supported: {}. Use base64 data URLs instead.", url);
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

    async fn generate(&self, input: edge_ai_core::llm::backend::LlmInput) -> Result<LlmOutput, LlmError> {
        let start_time = Instant::now();
        let model = input.model.unwrap_or_else(|| self.model.clone());

        let url = format!("{}/api/chat", self.config.endpoint);
        tracing::debug!("Ollama: calling URL: {}", url);

        let options = if input.params.temperature.is_some()
            || input.params.max_tokens.is_some()
            || input.params.top_p.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict: input.params.max_tokens,
                top_p: input.params.top_p,
            })
        } else {
            None
        };

        // Convert tools to Ollama/OpenAI format
        let tools = if let Some(input_tools) = &input.tools {
            if !input_tools.is_empty() {
                let ollama_tools: Vec<OllamaTool> = input_tools.iter().map(|tool| {
                    OllamaTool {
                        tool_type: "function".to_string(),
                        function: OllamaToolFunction {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            parameters: tool.parameters.clone(),
                        },
                    }
                }).collect();
                tracing::debug!("Ollama: sending {} tools in request", ollama_tools.len());
                Some(ollama_tools)
            } else {
                None
            }
        } else {
            None
        };

        // Enable thinking for all models that support it
        // Disable when tools are present - excessive thinking interferes with tool calling
        // Based on Ollama docs: https://docs.ollama.com/capabilities/thinking
        let has_tools = tools.as_ref().map_or(false, |t| !t.is_empty());

        // When tools are present, completely omit the think field (not even false)
        // This prevents Ollama from generating thinking content during tool calls
        let think = if !has_tools {
            Some(OllamaThink::Bool(true))  // Enable thinking when no tools
        } else {
            None  // Omit think field when tools are present
        };

        if has_tools {
            tracing::debug!("Ollama: thinking omitted for tool call request");
        }

        // No format parameter needed - let Ollama handle thinking naturally
        let format: Option<String> = None;

        let request = OllamaChatRequest {
            model: model.clone(),
            messages: self.messages_to_ollama(&input.messages),
            stream: false,
            options,
            think,
            tools,
            format,
        };

        let request_json = serde_json::to_string(&request).map_err(|e| LlmError::Serialization(e))?;
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
        let body = response.text().await.map_err(|e| LlmError::Network(e.to_string()))?;

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

        let ollama_response: OllamaChatResponse = serde_json::from_str(&body)
            .map_err(|e| LlmError::Serialization(e))?;

        // Handle native tool calls - convert to XML format for compatibility
        let mut response_text = if ollama_response.message.content.is_empty() {
            ollama_response.message.thinking.clone()
        } else {
            ollama_response.message.content.clone()
        };

        // If tool_calls are present, append them in XML format
        if !ollama_response.message.tool_calls.is_empty() {
            tracing::debug!("Ollama: received {} tool calls in non-streaming response", ollama_response.message.tool_calls.len());
            let mut xml_buffer = String::from("<tool_calls>");
            for tool_call in &ollama_response.message.tool_calls {
                xml_buffer.push_str(&format!(
                    "<invoke name=\"{}\">",
                    tool_call.function.name
                ));
                // Convert arguments JSON to XML parameter format
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
            // Append tool calls to response text
            response_text.push_str(&xml_buffer);
            tracing::debug!("Ollama: converted tool_calls to XML: {}", xml_buffer);
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

        // Handle max_tokens: usize::MAX means "no limit" - don't send num_predict to Ollama
        // This lets the model use its natural token limit without artificial constraints
        let num_predict = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => None,  // No limit - don't send num_predict
            Some(v) => Some(v),                        // Use specified limit
            None => None,                              // No limit specified
        };

        let options = if input.params.temperature.is_some()
            || num_predict.is_some()
            || input.params.top_p.is_some()
        {
            Some(OllamaOptions {
                temperature: input.params.temperature,
                num_predict,
                top_p: input.params.top_p,
            })
        } else {
            None
        };

        // Convert tools to Ollama/OpenAI format
        let tools = if let Some(input_tools) = &input.tools {
            if !input_tools.is_empty() {
                let ollama_tools: Vec<OllamaTool> = input_tools.iter().map(|tool| {
                    OllamaTool {
                        tool_type: "function".to_string(),
                        function: OllamaToolFunction {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            parameters: tool.parameters.clone(),
                        },
                    }
                }).collect();
                tracing::debug!("Ollama: sending {} tools in stream request", ollama_tools.len());
                Some(ollama_tools)
            } else {
                None
            }
        } else {
            None
        };

        // Enable thinking for all models that support it
        // Disable when tools are present - excessive thinking interferes with tool calling
        // Based on Ollama docs: https://docs.ollama.com/capabilities/thinking
        let has_tools = tools.as_ref().map_or(false, |t| !t.is_empty());

        // When tools are present, completely omit the think field (not even false)
        // This prevents Ollama from generating thinking content during tool calls
        // TEMPORARILY DISABLED: Some models like qwen3-vl:2b only output thinking when think=true
        // TODO: Re-enable once model behavior is fixed or we handle thinking properly
        let think = None; /*
        let think = if !has_tools {
            tracing::debug!("Enabling think=true for model {}", model);
            Some(OllamaThink::Bool(true))
        } else {
            tracing::debug!("Thinking disabled for tool call request on model {}", model);
            None
        };
        */

        // Convert messages to Ollama format before spawning the task
        let messages = self.messages_to_ollama(&input.messages);
        tracing::debug!("Ollama: generate_stream - URL: {}, messages count: {}", url, messages.len());
        eprintln!("[DEBUG OLLAMA] has_tools: {}, think set: {}", has_tools, think.is_some());

        // No format parameter needed - let Ollama handle thinking naturally
        let format: Option<String> = None;

        tokio::spawn(async move {
            let request = OllamaChatRequest {
                model: model.clone(),
                messages,
                stream: true,
                options,
                think,
                tools,
                format,
            };

            let request_json = match serde_json::to_string(&request) {
                Ok(json) => {
                    tracing::debug!("Ollama: stream request prepared for model: {}", model);
                    // Print request details for debugging
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                        // Print options (num_predict, temperature, etc.)
                        if let Some(opts) = value.get("options") {
                            println!("[ollama.rs] Request options: {}", serde_json::to_string_pretty(opts).unwrap_or_default());
                        }
                        // Print think setting
                        if let Some(think_val) = value.get("think") {
                            println!("[ollama.rs] Request think: {}", think_val);
                        }
                        // Print tools section
                        if let Some(tools) = value.get("tools") {
                            println!("[ollama.rs] Tools being sent: {}", serde_json::to_string_pretty(tools).unwrap_or_default());
                        }
                    }
                    json
                },
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
                    println!("[ollama.rs] Got response, status: {}", response.status());
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
                    let mut total_bytes = 0usize;

                    while let Some(chunk_result) = byte_stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                // Empty chunks are normal in HTTP streaming - don't break on them
                                // Just skip empty chunks but continue processing
                                if chunk.is_empty() {
                                    tracing::debug!("[ollama.rs] Skipping empty chunk, continuing stream");
                                    continue;
                                }
                                total_bytes += chunk.len();
                                println!("[ollama.rs] Received chunk: {} bytes, total: {}", chunk.len(), total_bytes);
                                buffer.extend_from_slice(&chunk);

                                let mut search_start = 0;
                                loop {
                                    if let Some(nl_pos) = buffer[search_start..].iter().position(|&b| b == b'\n') {
                                        let line_end = search_start + nl_pos;
                                        let line_bytes = &buffer[..line_end];
                                        let line = String::from_utf8_lossy(line_bytes).trim().to_string();

                                        buffer = buffer[line_end + 1..].to_vec();
                                        search_start = 0;

                                        if line.is_empty() {
                                            continue;
                                        }

                                        let json_str = if let Some(prefix) = line.strip_prefix("data: ") {
                                            prefix
                                        } else if let Some(prefix) = line.strip_prefix("data:") {
                                            prefix
                                        } else {
                                            &line
                                        };

                                        // Debug: log the raw response
                                        tracing::debug!("Ollama raw response: {}", json_str);
                                        println!("[ollama.rs] Parsing JSON: {}", if json_str.len() > 150 { &json_str[..150] } else { &json_str });

                                        // Log the full JSON for chunks with done=true to debug missing content
                                        if json_str.contains("\"done\":true") {
                                            println!("[ollama.rs] FULL JSON for done=true: {}", json_str);
                                        }

                                        if let Ok(ollama_chunk) =
                                            serde_json::from_str::<OllamaStreamResponse>(json_str)
                                        {
                                            println!("[ollama.rs] Parsed ollama_chunk: thinking={}, content={}, tool_calls={}, done={}",
                                                !ollama_chunk.message.thinking.is_empty(),
                                                !ollama_chunk.message.content.is_empty(),
                                                !ollama_chunk.message.tool_calls.is_empty(),
                                                ollama_chunk.done);
                                            // Log actual content lengths
                                            if !ollama_chunk.message.thinking.is_empty() {
                                                println!("[ollama.rs] -> thinking content: '{}' (len={})", ollama_chunk.message.thinking, ollama_chunk.message.thinking.chars().count());
                                            }
                                            if !ollama_chunk.message.content.is_empty() {
                                                println!("[ollama.rs] -> response content: '{}' (len={})", ollama_chunk.message.content, ollama_chunk.message.content.chars().count());
                                            }
                                            // Handle native tool calls - convert to XML format for compatibility
                                            if !ollama_chunk.message.tool_calls.is_empty() {
                                                tracing::debug!("Ollama: received {} tool calls", ollama_chunk.message.tool_calls.len());
                                                // Convert tool_calls to XML format for streaming.rs compatibility
                                                let mut xml_buffer = String::from("<tool_calls>");
                                                for tool_call in &ollama_chunk.message.tool_calls {
                                                    xml_buffer.push_str(&format!(
                                                        "<invoke name=\"{}\">",
                                                        tool_call.function.name
                                                    ));
                                                    // Convert arguments JSON to XML parameter format
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
                                                tracing::debug!("Ollama: converted tool_calls to XML: {}", xml_buffer);
                                                let _ = tx.send(Ok((xml_buffer, false))).await;
                                            }

                                            // Send thinking content first (reasoning process)
                                            if !ollama_chunk.message.thinking.is_empty() {
                                                tracing::debug!("Ollama thinking chunk: {}", ollama_chunk.message.thinking);
                                                println!("[ollama.rs] Sending thinking chunk (len={})", ollama_chunk.message.thinking.chars().count());
                                                let _ = tx.send(Ok((ollama_chunk.message.thinking.clone(), true))).await;
                                            }

                                            // Then send response content (final answer)
                                            if !ollama_chunk.message.content.is_empty() {
                                                tracing::debug!("Ollama content chunk: {}", ollama_chunk.message.content);
                                                println!("[ollama.rs] Sending content chunk (len={})", ollama_chunk.message.content.chars().count());
                                                let _ = tx.send(Ok((ollama_chunk.message.content.clone(), false))).await;
                                            }

                                            if ollama_chunk.done {
                                                // Ollama has finished generation - log and close stream
                                                // Log what content was in this final chunk
                                                let final_content = ollama_chunk.message.content.clone();
                                                let final_thinking = ollama_chunk.message.thinking.clone();
                                                println!("[ollama.rs] Ollama sent done=true, total_bytes: {}, final_content_len: {}, final_thinking_len: {}, closing stream",
                                                    total_bytes, final_content.chars().count(), final_thinking.chars().count());
                                                if !final_content.is_empty() {
                                                    println!("[ollama.rs] Final content: '{}'", final_content);
                                                }
                                                if !final_thinking.is_empty() {
                                                    println!("[ollama.rs] Final thinking: '{}'", final_thinking);
                                                }
                                                tracing::info!("Ollama stream complete: {} bytes transferred", total_bytes);
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
                        println!("[ollama.rs] Stream closed without done=true signal, total_bytes: {}", total_bytes);
                        tracing::warn!("Ollama stream closed prematurely without done signal, {} bytes transferred", total_bytes);
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
        // Ollama's default context window varies by model
        4096
    }

    fn supports_multimodal(&self) -> bool {
        true
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::builder()
            .streaming()
            .multimodal()
            .thinking_display()
            .max_context(4096)
            .build()
    }

    fn metrics(&self) -> BackendMetrics {
        self.metrics.read()
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
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
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
        let config = OllamaConfig::new("qwen3-vl:2b")
            .with_endpoint("http://192.168.1.100:11434");
        assert_eq!(config.endpoint, "http://192.168.1.100:11434");
    }
}
