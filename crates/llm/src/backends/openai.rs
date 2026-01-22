//! OpenAI-compatible cloud LLM backend implementation.
//!
//! Supports cloud APIs that are compatible with OpenAI's format:
//! - OpenAI (GPT-4, GPT-3.5, o1, etc.)
//! - Anthropic Claude (via compatibility layer)
//! - Google Gemini (via compatibility layer)
//! - xAI Grok
//! - Other OpenAI-compatible providers

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
use edge_ai_core::message::{Content, ContentPart, ImageDetail, Message, MessageRole};

use crate::rate_limited_client::{ProviderRateLimits, RateLimitedClient};

/// Cloud API provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudProvider {
    /// OpenAI (https://api.openai.com)
    OpenAI,

    /// Anthropic Claude (https://api.anthropic.com)
    Anthropic,

    /// Google Gemini (https://generativelanguage.googleapis.com)
    Google,

    /// xAI Grok (https://api.x.ai)
    Grok,

    /// Custom OpenAI-compatible endpoint
    Custom,
}

impl CloudProvider {
    /// Get the base URL for this provider.
    fn base_url(&self) -> &str {
        match self {
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta",
            Self::Grok => "https://api.x.ai/v1",
            Self::Custom => "",
        }
    }

    /// Get the default model for this provider.
    fn default_model(&self) -> &str {
        match self {
            Self::OpenAI => "gpt-4o-mini",
            Self::Anthropic => "claude-3-5-sonnet-20241022",
            Self::Google => "gemini-1.5-flash",
            Self::Grok => "grok-beta",
            Self::Custom => "unknown",
        }
    }

    /// Get the chat completion path.
    fn chat_path(&self) -> &str {
        match self {
            Self::OpenAI => "/chat/completions",
            Self::Anthropic => "/messages",
            Self::Google => "/chat/completions", // Using OpenAI compatibility
            Self::Grok => "/chat/completions",
            Self::Custom => "/chat/completions",
        }
    }
}

/// Configuration for cloud LLM backend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CloudConfig {
    /// API key for authentication.
    pub api_key: String,

    /// Cloud provider.
    pub provider: CloudProvider,

    /// Model to use (overrides provider default).
    pub model: Option<String>,

    /// Base URL (for custom providers).
    pub base_url: Option<String>,

    /// Request timeout in seconds (default: 60).
    #[serde(default = "default_cloud_timeout_secs")]
    pub timeout_secs: u64,
}

/// Default timeout in seconds for cloud backends.
fn default_cloud_timeout_secs() -> u64 {
    60
}

impl CloudConfig {
    /// Get the timeout as a Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Create a new OpenAI config.
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::OpenAI,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a new Anthropic config.
    pub fn anthropic(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::Anthropic,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a new Google config.
    pub fn google(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::Google,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a new xAI Grok config.
    pub fn grok(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::Grok,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a custom config.
    pub fn custom(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::Custom,
            model: None,
            base_url: Some(base_url.into()),
            timeout_secs: 60,
        }
    }

    /// Set the model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_secs = timeout.as_secs();
        self
    }

    /// Get the effective base URL.
    fn get_base_url(&self) -> String {
        if let Some(base) = &self.base_url {
            base.clone()
        } else {
            self.provider.base_url().to_string()
        }
    }

    /// Get the effective model name.
    fn get_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| self.provider.default_model().to_string())
    }
}

/// Cloud LLM runtime backend.
pub struct CloudRuntime {
    config: CloudConfig,
    client: RateLimitedClient,
    model: String,
    metrics: Arc<RwLock<BackendMetrics>>,
}

impl CloudRuntime {
    /// Create a new cloud runtime.
    pub fn new(config: CloudConfig) -> Result<Self, LlmError> {
        let http_client = Client::builder()
            .timeout(config.timeout())
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        // Configure rate limits based on provider
        let limits = ProviderRateLimits::default();
        let (max_requests, window_duration) = match config.provider {
            CloudProvider::Anthropic => limits.anthropic,
            CloudProvider::OpenAI => limits.openai,
            CloudProvider::Google => limits.google,
            CloudProvider::Grok => (50, Duration::from_secs(60)),
            CloudProvider::Custom => (10, Duration::from_secs(1)),
        };

        let client =
            RateLimitedClient::with_rate_limits(http_client, max_requests, window_duration);

        let model = config.get_model();

        Ok(Self {
            config,
            client,
            model,
            metrics: Arc::new(RwLock::new(BackendMetrics::default())),
        })
    }

    /// Convert messages to API format (provider-specific).
    /// For Anthropic, uses their image format. For OpenAI/Google, uses OpenAI-style format.
    fn messages_to_api(&self, messages: &[Message]) -> Vec<ApiMessage> {
        let is_anthropic = matches!(self.config.provider, CloudProvider::Anthropic);

        messages
            .iter()
            .map(|msg| {
                let content = match &msg.content {
                    Content::Text(text) => ApiContent::Text(text.clone()),
                    Content::Parts(parts) => {
                        let api_parts: Vec<ApiContentPart> = parts
                            .iter()
                            .map(|part| match part {
                                ContentPart::Text { text } => {
                                    ApiContentPart::Text { text: text.clone() }
                                }
                                ContentPart::ImageUrl { url, detail } => {
                                    if is_anthropic {
                                        // Anthropic format: {"type": "image", "source": {...}}
                                        let (media_type, data) = extract_data_url(url);
                                        ApiContentPart::AnthropicImage {
                                            source: AnthropicImageSource {
                                                typ: "base64".to_string(),
                                                media_type,
                                                data,
                                            },
                                        }
                                    } else {
                                        // OpenAI/Google format
                                        ApiContentPart::ImageUrl {
                                            url: url.clone(),
                                            detail: image_detail_to_string(
                                                detail.as_ref().unwrap_or(&ImageDetail::Auto),
                                            ),
                                        }
                                    }
                                }
                                ContentPart::ImageBase64 {
                                    data,
                                    mime_type,
                                    detail: _,
                                } => {
                                    if is_anthropic {
                                        // Anthropic format: raw base64 data
                                        ApiContentPart::AnthropicImage {
                                            source: AnthropicImageSource {
                                                typ: "base64".to_string(),
                                                media_type: mime_type.clone(),
                                                data: data.clone(),
                                            },
                                        }
                                    } else {
                                        // OpenAI/Google format: data URL
                                        ApiContentPart::ImageUrl {
                                            url: format!("data:{};base64,{}", mime_type, data),
                                            detail: "auto".to_string(),
                                        }
                                    }
                                }
                            })
                            .collect();

                        ApiContent::Parts(api_parts)
                    }
                };

                ApiMessage {
                    role: match msg.role {
                        MessageRole::System => "system",
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                    }
                    .to_string(),
                    content,
                }
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl LlmRuntime for CloudRuntime {
    fn backend_id(&self) -> BackendId {
        // Return backend ID based on the cloud provider
        match self.config.provider {
            CloudProvider::OpenAI => BackendId::new(BackendId::OPENAI),
            CloudProvider::Anthropic => BackendId::new("anthropic"),
            CloudProvider::Google => BackendId::new("google"),
            CloudProvider::Grok => BackendId::new("grok"),
            CloudProvider::Custom => BackendId::new("custom"),
        }
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        !self.config.api_key.is_empty()
    }

    async fn generate(
        &self,
        input: edge_ai_core::llm::backend::LlmInput,
    ) -> Result<LlmOutput, LlmError> {
        let start_time = Instant::now();
        let model = input.model.unwrap_or_else(|| self.model.clone());

        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );

        let request = ChatCompletionRequest {
            model: model.clone(),
            messages: self.messages_to_api(&input.messages),
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            max_tokens: input.params.max_tokens,
            stop: input.params.stop.clone(),
            frequency_penalty: input.params.frequency_penalty,
            presence_penalty: input.params.presence_penalty,
            stream: false,
        };

        // Create rate limit key based on provider and API key hash
        let rate_limit_key = format!(
            "{:?}:{:x}",
            self.config.provider,
            hash_api_key(&self.config.api_key)
        );

        // Build the request
        let req = self
            .client
            .inner()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&request);

        let response = self
            .client
            .execute_request(&rate_limit_key, req.build().unwrap())
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        if !status.is_success() {
            self.metrics.write().unwrap().record_failure();
            return Err(LlmError::Generation(format!(
                "API error {}: {}",
                status.as_u16(),
                body
            )));
        }

        let chat_response: ChatCompletionResponse =
            serde_json::from_str(&body).map_err(LlmError::Serialization)?;

        let choice = chat_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::Generation("No choices in response".to_string()))?;

        let result = Ok(LlmOutput {
            text: choice.message.content,
            finish_reason: match choice.finish_reason.as_str() {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                _ => FinishReason::Error,
            },
            usage: chat_response.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            // OpenAI doesn't have thinking field in this format
            thinking: None,
        });

        // Record metrics
        let latency_ms = start_time.elapsed().as_millis() as u64;
        match &result {
            Ok(output) => {
                let tokens = output.usage.map_or(0, |u| u.completion_tokens as u64);
                self.metrics
                    .write()
                    .unwrap()
                    .record_success(tokens, latency_ms);
            }
            Err(_) => {
                self.metrics.write().unwrap().record_failure();
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
        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );
        let api_key = self.config.api_key.clone();
        let rate_limiter = self.client.clone();
        let inner_client = self.client.inner().clone();
        let provider = self.config.provider;

        let request = ChatCompletionRequest {
            model: model.clone(),
            messages: Vec::new(), // Will be filled by caller
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            max_tokens: input.params.max_tokens,
            stop: input.params.stop.clone(),
            frequency_penalty: input.params.frequency_penalty,
            presence_penalty: input.params.presence_penalty,
            stream: true,
        };

        tokio::spawn(async move {
            // Create rate limit key
            let rate_limit_key = format!("{:?}:{:x}", provider, hash_api_key(&api_key));

            // Acquire rate limit permit before making request
            rate_limiter.acquire(&rate_limit_key).await;

            let result = inner_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&request)
                .send()
                .await;

            match result {
                Ok(response) => {
                    let status = response.status();

                    // Handle rate limit response
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let _ = tx
                            .send(Err(LlmError::Generation("Rate limited by API".to_string())))
                            .await;
                        return;
                    }

                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx
                            .send(Err(LlmError::Generation(format!(
                                "API error {}: {}",
                                status.as_u16(),
                                body
                            ))))
                            .await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                buffer.extend_from_slice(&chunk);
                                let data = String::from_utf8_lossy(&buffer);

                                for line in data.lines() {
                                    let line = line.trim();
                                    if line.is_empty() {
                                        continue;
                                    }
                                    if line == "data: [DONE]" {
                                        let _ = tx.send(Ok((String::new(), false))).await;
                                        continue;
                                    }
                                    if let Some(json) = line.strip_prefix("data: ") {
                                        if let Ok(evt) =
                                            serde_json::from_str::<StreamChunkEvent>(json)
                                            && let Some(choice) = evt.choices.first() {
                                                let delta = &choice.delta.content;
                                                if !delta.is_empty() {
                                                    let _ =
                                                        tx.send(Ok((delta.clone(), false))).await;
                                                }
                                            }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::Network(e.to_string()))).await;
                            }
                        }
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
        match self.config.provider {
            CloudProvider::OpenAI => 128000,
            CloudProvider::Anthropic => 200000,
            CloudProvider::Google => 1000000,
            CloudProvider::Grok => 128000,
            CloudProvider::Custom => 4096,
        }
    }

    fn supports_multimodal(&self) -> bool {
        matches!(
            self.config.provider,
            CloudProvider::OpenAI | CloudProvider::Anthropic | CloudProvider::Google
        )
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            streaming: true,
            multimodal: self.supports_multimodal(),
            function_calling: matches!(self.config.provider, CloudProvider::OpenAI),
            multiple_models: true,
            max_context: Some(self.max_context_length()),
            modalities: vec!["text".to_string()],
            thinking_display: false,
            supports_images: self.supports_multimodal(),
            supports_audio: false,
        }
    }

    fn metrics(&self) -> BackendMetrics {
        self.metrics.read().unwrap().clone()
    }
}

// Helper functions

/// Extract media type and base64 data from a data URL.
/// Returns (media_type, base64_data).
fn extract_data_url(url: &str) -> (String, String) {
    if url.starts_with("data:") {
        // Format: data:image/png;base64,iVBORw0KGgo...
        if let Some(rest) = url.strip_prefix("data:") {
            if let Some((mime_and_encoding, data)) = rest.split_once(',') {
                // mime_and_encoding is like "image/png;base64"
                let media_type = mime_and_encoding
                    .split(';')
                    .next()
                    .unwrap_or("image/png")
                    .to_string();
                return (media_type, data.to_string());
            }
        }
    }
    // Fallback
    ("image/png".to_string(), url.to_string())
}

fn image_detail_to_string(detail: &ImageDetail) -> String {
    match detail {
        ImageDetail::Auto => "auto".to_string(),
        ImageDetail::Low => "low".to_string(),
        ImageDetail::High => "high".to_string(),
    }
}

/// Hash an API key for use as a rate limit key.
/// This avoids exposing actual API keys in logs.
fn hash_api_key(api_key: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    api_key.hash(&mut hasher);
    hasher.finish()
}

// API types

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: ApiContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ApiContent {
    Text(String),
    Parts(Vec<ApiContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ApiContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { url: String, detail: String },
    /// Anthropic-style image format: {"type": "image", "source": {"type": "base64", "media_type": "...", "data": "..."}}
    #[serde(rename = "image")]
    AnthropicImage {
        #[serde(rename = "source")]
        source: AnthropicImageSource,
    },
}

/// Anthropic image source format
#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    typ: String,  // "base64"
    #[serde(rename = "media_type")]
    media_type: String,  // "image/png", "image/jpeg", etc.
    data: String,  // base64 data without prefix
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ApiMessageResponse,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
struct ApiMessageResponse {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct StreamChunkEvent {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: String,
}

use tokio_stream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_config_openai() {
        let config = CloudConfig::openai("sk-test");
        assert_eq!(config.provider, CloudProvider::OpenAI);
        assert_eq!(config.api_key, "sk-test");
    }

    #[test]
    fn test_cloud_config_with_model() {
        let config = CloudConfig::openai("sk-test").with_model("gpt-4o");
        assert_eq!(config.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_cloud_provider_urls() {
        assert_eq!(
            CloudProvider::OpenAI.base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            CloudProvider::Anthropic.base_url(),
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            CloudProvider::Google.base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(CloudProvider::Grok.base_url(), "https://api.x.ai/v1");
    }
}
