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

        // Enable thinking by default for models that support it
        // This gives the model's reasoning process separately from the final answer
        let think = Some(OllamaThink::Bool(true));

        let request = OllamaChatRequest {
            model: model.clone(),
            messages: self.messages_to_ollama(&input.messages),
            stream: false,
            options,
            think,
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

        // Use content (final answer), fallback to thinking if content is empty
        let response_text = if ollama_response.message.content.is_empty() {
            ollama_response.message.thinking.clone()
        } else {
            ollama_response.message.content.clone()
        };

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

        // Enable thinking by default
        let think = Some(OllamaThink::Bool(true));

        // Convert messages to Ollama format before spawning the task
        let messages = self.messages_to_ollama(&input.messages);
        tracing::debug!("Ollama: generate_stream - URL: {}, messages count: {}", url, messages.len());

        tokio::spawn(async move {
            let request = OllamaChatRequest {
                model: model.clone(),
                messages,
                stream: true,
                options,
                think,
            };

            let request_json = match serde_json::to_string(&request) {
                Ok(json) => {
                    tracing::debug!("Ollama: stream request prepared for model: {}", model);
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
                    let status = response.status();
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
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

                    while let Some(chunk_result) = byte_stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                if chunk.is_empty() {
                                    break;
                                }
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

                                        if let Ok(ollama_chunk) =
                                            serde_json::from_str::<OllamaStreamResponse>(json_str)
                                        {
                                            // Send thinking content first
                                            if !ollama_chunk.message.thinking.is_empty() {
                                                let _ = tx.send(Ok((ollama_chunk.message.thinking.clone(), true))).await;
                                            }

                                            // Then send response content
                                            if !ollama_chunk.message.content.is_empty() {
                                                let _ = tx.send(Ok((ollama_chunk.message.content.clone(), false))).await;
                                            }

                                            if ollama_chunk.done {
                                                let _ = tx.send(Ok((String::new(), false))).await;
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
                        let _ = tx.send(Ok((String::new(), false))).await;
                    }
                }
                Err(e) => {
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
