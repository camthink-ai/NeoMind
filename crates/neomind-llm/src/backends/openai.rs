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

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput, LlmRuntime,
    StreamChunk, TokenUsage,
};
use neomind_core::message::{Content, ContentPart, ImageDetail, Message, MessageRole};

use crate::rate_limited_client::{ProviderRateLimits, RateLimitedClient};

/// Cloud API provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    #[default]
    Custom,

    /// Qwen (Alibaba DashScope)
    Qwen,

    /// DeepSeek (https://api.deepseek.com)
    DeepSeek,

    /// Zhipu GLM (智谱)
    GLM,

    /// MiniMax (https://api.minimax.chat)
    MiniMax,
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
            Self::Qwen => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Self::DeepSeek => "https://api.deepseek.com/v1",
            Self::GLM => "https://open.bigmodel.cn/api/paas/v4",
            Self::MiniMax => "https://api.minimax.chat/v1",
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
            Self::Qwen => "qwen-max-latest",
            Self::DeepSeek => "deepseek-v3",
            Self::GLM => "glm-4-plus",
            Self::MiniMax => "m2-1-19b",
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
            Self::Qwen => "/chat/completions",
            Self::DeepSeek => "/chat/completions",
            Self::GLM => "/chat/completions",
            Self::MiniMax => "/chat/completions",
        }
    }
}

/// Configuration for cloud LLM backend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CloudConfig {
    /// API key for authentication.
    pub api_key: String,

    /// Cloud provider (optional during deserialization, will be set by backend creation code).
    #[serde(default)]
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

    /// Create a Qwen (Alibaba DashScope) config.
    pub fn qwen(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::Qwen,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a DeepSeek config.
    pub fn deepseek(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::DeepSeek,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a Zhipu GLM config.
    pub fn glm(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::GLM,
            model: None,
            base_url: None,
            timeout_secs: 60,
        }
    }

    /// Create a MiniMax config.
    pub fn minimax(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            provider: CloudProvider::MiniMax,
            model: None,
            base_url: None,
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

    /// Set the base URL (optional, for custom endpoints).
    /// This allows overriding the default API endpoint while keeping the provider type.
    pub fn with_base_url_opt(mut self, base_url: Option<String>) -> Self {
        self.base_url = base_url;
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
            .pool_max_idle_per_host(10) // Performance: Keep 10 idle connections for concurrent requests
            .pool_idle_timeout(Duration::from_secs(120)) // Close after 120s idle
            .connect_timeout(Duration::from_secs(10)) // Cloud services: 10s connection timeout
            .http2_keep_alive_interval(Duration::from_secs(30)) // Keep HTTP/2 alive
            .http2_keep_alive_timeout(Duration::from_secs(10)) // Keep-alive timeout
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        // Configure rate limits based on provider
        let limits = ProviderRateLimits::default();
        let (max_requests, window_duration) = match config.provider {
            CloudProvider::Anthropic => limits.anthropic,
            CloudProvider::OpenAI => limits.openai,
            CloudProvider::Google => limits.google,
            CloudProvider::Grok => (50, Duration::from_secs(60)),
            CloudProvider::Qwen => (100, Duration::from_secs(60)),
            CloudProvider::DeepSeek => (100, Duration::from_secs(60)),
            CloudProvider::GLM => (100, Duration::from_secs(60)),
            CloudProvider::MiniMax => (100, Duration::from_secs(60)),
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
                                        // OpenAI/Google format: {"type": "image_url", "image_url": {"url": "...", "detail": "auto"}}
                                        ApiContentPart::ImageUrl {
                                            image_url: ImageUrlContent {
                                                url: url.clone(),
                                                detail: Some(image_detail_to_string(
                                                    detail.as_ref().unwrap_or(&ImageDetail::Auto),
                                                )),
                                            },
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
                                            image_url: ImageUrlContent {
                                                url: format!("data:{};base64,{}", mime_type, data),
                                                detail: Some("auto".to_string()),
                                            },
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
                        MessageRole::Tool => "user", // OpenAI uses "user" role for tool results
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
            CloudProvider::OpenAI => BackendId::new("openai"),
            CloudProvider::Anthropic => BackendId::new("anthropic"),
            CloudProvider::Google => BackendId::new("google"),
            CloudProvider::Grok => BackendId::new("grok"),
            CloudProvider::Custom => BackendId::new("custom"),
            CloudProvider::Qwen => BackendId::new("qwen"),
            CloudProvider::DeepSeek => BackendId::new("deepseek"),
            CloudProvider::GLM => BackendId::new("glm"),
            CloudProvider::MiniMax => BackendId::new("minimax"),
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
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<LlmOutput, LlmError> {
        let start_time = Instant::now();
        let model = input.model.unwrap_or_else(|| self.model.clone());

        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );

        // Handle max_tokens: cap at reasonable limit for cloud APIs
        const MAX_TOKENS_CAP: u32 = 32768; // 32k tokens - reasonable for most models
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => Some(MAX_TOKENS_CAP),
            Some(v) => Some((v as u32).min(MAX_TOKENS_CAP)),
            None => None, // Let API use its default
        };

        let request = ChatCompletionRequest {
            model: model.clone(),
            messages: self.messages_to_api(&input.messages),
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            max_tokens,
            stop: input.params.stop.clone(),
            frequency_penalty: input.params.frequency_penalty,
            presence_penalty: input.params.presence_penalty,
            stream: false,
            tools: input.tools.map(|tools| tools.into_iter().map(OpenAiTool::from).collect()),
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

        // Build response text, including tool calls if present
        let mut response_text = choice.message.content.unwrap_or_default();

        // Handle native tool calls from OpenAI - preserve JSON format to keep tool ID
        if let Some(ref tool_calls) = choice.message.tool_calls {
            if !tool_calls.is_empty() {
                tracing::debug!(
                    "OpenAI: received {} native tool calls",
                    tool_calls.len()
                );
                // Build JSON array to preserve tool IDs (OpenAI-compatible format)
                let tool_calls_json: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        // Parse arguments from JSON string to Value
                        let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({}));
                        serde_json::json!({
                            "id": tc.id,
                            "name": tc.function.name,
                            "arguments": args
                        })
                    })
                    .collect();
                let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
                response_text.push_str(&json_str);
            }
        }

        let result = Ok(LlmOutput {
            text: response_text,
            finish_reason: match choice.finish_reason.as_str() {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                "tool_calls" => FinishReason::Stop, // Tool calls are a valid stop reason
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
        input: neomind_core::llm::backend::LlmInput,
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

        // Handle max_tokens: cap at reasonable limit for cloud APIs
        const MAX_TOKENS_CAP: u32 = 32768; // 32k tokens - reasonable for most models
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => Some(MAX_TOKENS_CAP),
            Some(v) => Some((v as u32).min(MAX_TOKENS_CAP)),
            None => None, // Let API use its default
        };

        let request = ChatCompletionRequest {
            model: model.clone(),
            messages: self.messages_to_api(&input.messages),
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            max_tokens,
            stop: input.params.stop.clone(),
            frequency_penalty: input.params.frequency_penalty,
            presence_penalty: input.params.presence_penalty,
            stream: true,
            tools: input.tools.map(|tools| tools.into_iter().map(OpenAiTool::from).collect()),
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
                    // Accumulate tool calls across chunks
                    let mut accumulated_tool_calls: std::collections::HashMap<u32, AccumulatedToolCall> = std::collections::HashMap::new();

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                buffer.extend_from_slice(&chunk);

                                // Process complete lines from buffer
                                let mut search_start = 0;
                                loop {
                                    // Find newline in remaining buffer
                                    if let Some(nl_pos) = buffer[search_start..].iter().position(|&b| b == b'\n') {
                                        let line_end = search_start + nl_pos;
                                        let line_bytes = &buffer[..line_end];
                                        let line = String::from_utf8_lossy(line_bytes).trim().to_string();

                                        // Remove processed line from buffer
                                        buffer = buffer[line_end + 1..].to_vec();
                                        search_start = 0;

                                        if line.is_empty() {
                                            continue;
                                        }
                                        if line == "data: [DONE]" {
                                            // Flush any accumulated tool calls
                                            if !accumulated_tool_calls.is_empty() {
                                                let tool_calls_json: Vec<serde_json::Value> = accumulated_tool_calls
                                                    .values()
                                                    .map(|tc| {
                                                        let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                                                            .unwrap_or_else(|_| serde_json::json!({}));
                                                        serde_json::json!({
                                                            "id": tc.id,
                                                            "name": tc.name,
                                                            "arguments": args
                                                        })
                                                    })
                                                    .collect();
                                                let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
                                                let _ = tx.send(Ok((json_str, false))).await;
                                            }
                                            let _ = tx.send(Ok((String::new(), false))).await;
                                            continue;
                                        }
                                        if let Some(json) = line.strip_prefix("data: ") {
                                            if let Ok(evt) =
                                                serde_json::from_str::<StreamChunkEvent>(json)
                                            {
                                                if let Some(choice) = evt.choices.first() {
                                                    // Handle content
                                                    if let Some(ref content) = choice.delta.content {
                                                        if !content.is_empty() {
                                                            let _ = tx.send(Ok((content.clone(), false))).await;
                                                        }
                                                    }

                                                    // Handle tool calls (incremental)
                                                    if let Some(ref tool_calls) = choice.delta.tool_calls {
                                                        for tc in tool_calls {
                                                            let entry = accumulated_tool_calls.entry(tc.index).or_insert(AccumulatedToolCall {
                                                                id: None,
                                                                name: None,
                                                                arguments: String::new(),
                                                            });

                                                            // Update ID if present
                                                            if let Some(ref id) = tc.id {
                                                                entry.id = Some(id.clone());
                                                            }

                                                            // Update function details
                                                            if let Some(ref func) = tc.function {
                                                                if let Some(ref name) = func.name {
                                                                    entry.name = Some(name.clone());
                                                                }
                                                                if let Some(ref args) = func.arguments {
                                                                    entry.arguments.push_str(args);
                                                                }
                                                            }
                                                        }
                                                    }

                                                    // Check for finish reason - flush tool calls
                                                    if choice.finish_reason.as_deref() == Some("tool_calls")
                                                        && !accumulated_tool_calls.is_empty() {
                                                            let tool_calls_json: Vec<serde_json::Value> = accumulated_tool_calls
                                                                .values()
                                                                .map(|tc| {
                                                                    let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                                                                        .unwrap_or_else(|_| serde_json::json!({}));
                                                                    serde_json::json!({
                                                                        "id": tc.id,
                                                                        "name": tc.name,
                                                                        "arguments": args
                                                                    })
                                                                })
                                                                .collect();
                                                            let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
                                                            let _ = tx.send(Ok((json_str, false))).await;
                                                            accumulated_tool_calls.clear();
                                                        }
                                                }
                                            }
                                        }
                                    } else {
                                        // No more complete lines, break the loop
                                        break;
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
            CloudProvider::Qwen => 128000,
            CloudProvider::DeepSeek => 128000,
            CloudProvider::GLM => 128000,
            CloudProvider::MiniMax => 512000,
            CloudProvider::Custom => 4096,
        }
    }

    fn supports_multimodal(&self) -> bool {
        // Check if the specific model supports vision based on model name
        let model = self.model.to_lowercase();
        is_vision_model(&self.config.provider, &model)
    }

    fn capabilities(&self) -> BackendCapabilities {
        let supports_multimodal = self.supports_multimodal();
        let supports_function_calling = matches!(
            self.config.provider,
            CloudProvider::OpenAI
                | CloudProvider::Qwen
                | CloudProvider::DeepSeek
                | CloudProvider::GLM
                | CloudProvider::MiniMax
                | CloudProvider::Google
                | CloudProvider::Grok
        );

        BackendCapabilities {
            streaming: true,
            multimodal: supports_multimodal,
            function_calling: supports_function_calling,
            multiple_models: true,
            max_context: Some(self.max_context_length()),
            modalities: vec!["text".to_string()],
            thinking_display: false,
            supports_images: supports_multimodal,
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
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    stream: bool,
    /// Tools for function calling (OpenAI-compatible format)
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

/// Tool definition in OpenAI format
#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String, // Always "function"
    function: OpenAiFunction,
}

/// Function definition for tool calling
#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl From<neomind_core::llm::backend::ToolDefinition> for OpenAiTool {
    fn from(tool: neomind_core::llm::backend::ToolDefinition) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: OpenAiFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }
    }
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
    ImageUrl {
        #[serde(rename = "image_url")]
        image_url: ImageUrlContent,
    },
    /// Anthropic-style image format: {"type": "image", "source": {"type": "base64", "media_type": "...", "data": "..."}}
    #[serde(rename = "image")]
    AnthropicImage {
        #[serde(rename = "source")]
        source: AnthropicImageSource,
    },
}

/// Image URL content for OpenAI format
#[derive(Debug, Serialize)]
struct ImageUrlContent {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Anthropic image source format
#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    typ: String, // "base64"
    #[serde(rename = "media_type")]
    media_type: String, // "image/png", "image/jpeg", etc.
    data: String, // base64 data without prefix
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
    /// Content can be null when model makes tool calls
    #[serde(default)]
    content: Option<String>,
    /// Tool calls made by the model (for function calling)
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallResponse>>,
}

/// Tool call in OpenAI response format
#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCallResponse {
    /// Tool call ID
    id: Option<String>,
    /// Tool type (always "function")
    #[allow(dead_code)]
    #[serde(rename = "type")]
    call_type: Option<String>,
    /// Function call details
    function: OpenAiFunctionCall,
}

/// Function call details in response
#[derive(Debug, Clone, Deserialize)]
struct OpenAiFunctionCall {
    /// Function name
    name: String,
    /// Function arguments as JSON string
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Accumulated tool call from streaming chunks
#[derive(Debug, Clone)]
struct AccumulatedToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct StreamChunkEvent {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    /// Content can be null when model makes tool calls
    #[serde(default)]
    content: Option<String>,
    /// Tool calls in streaming format (incremental updates)
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

/// Tool call in streaming response (incremental)
#[derive(Debug, Clone, Deserialize)]
struct StreamToolCall {
    /// Index of this tool call in the array
    index: u32,
    /// Tool call ID (only in first chunk)
    id: Option<String>,
    /// Tool type (only in first chunk)
    #[allow(dead_code)]
    #[serde(rename = "type")]
    call_type: Option<String>,
    /// Function call details (incremental)
    function: Option<StreamFunctionCall>,
}

/// Function call in streaming response (incremental)
#[derive(Debug, Clone, Deserialize)]
struct StreamFunctionCall {
    /// Function name (only in first chunk)
    name: Option<String>,
    /// Function arguments (incremental, JSON string fragments)
    arguments: Option<String>,
}

use tokio_stream;

/// Check if a model supports vision (image input) based on provider and model name.
/// This uses name-based heuristic detection for common vision-capable models.
fn is_vision_model(provider: &CloudProvider, model_name: &str) -> bool {
    let name_lower = model_name.to_lowercase();

    match provider {
        CloudProvider::OpenAI => {
            // OpenAI vision-capable models:
            // - gpt-4o, gpt-4o-mini (all GPT-4o models support vision)
            // - gpt-4-turbo, gpt-4-1106-vision-preview, gpt-4-vision-preview
            // - gpt-4.*vision
            // - o1 models (o1, o1-mini, o1-preview) - some support vision
            // NOT: gpt-4 (base), gpt-4-32k, gpt-3.5-turbo, gpt-3.5
            name_lower.contains("gpt-4o")
                || name_lower.contains("gpt-4-turbo")
                || name_lower.contains("gpt-4-vision")
                || name_lower.contains("gpt-4.1") // gpt-4.1 models
                || (name_lower.starts_with("gpt-4") && name_lower.contains("vision"))
                || (name_lower.starts_with("o1") && !name_lower.contains("o1-preview")) // o1 and o1-mini support vision
        }
        CloudProvider::Anthropic => {
            // All Claude 3 and later models support vision
            // claude-3-opus, claude-3-sonnet, claude-3-haiku, claude-3.5-sonnet, etc.
            name_lower.contains("claude-3") || name_lower.contains("claude-4")
        }
        CloudProvider::Google => {
            // All Gemini models support vision
            // gemini-1.5-flash, gemini-1.5-pro, gemini-pro-vision, etc.
            name_lower.contains("gemini")
        }
        CloudProvider::Qwen => {
            // Qwen vision models:
            // - qwen-vl, qwen2-vl, qwen3-vl, qwen-max-vl (explicit VL models)
            // - qwen3.5-* series (all support vision: qwen3.5-turbo, qwen3.5-plus, qwen3.5-max)
            // - qwen3-* series (all support vision: qwen3-turbo, qwen3-plus, qwen3-max)
            // - qwen-max, qwen-plus, qwen-turbo (newer versions support vision)
            // Note: Model names can be formatted as "qwen3.5-plus" or "qwen-3.5-plus"
            name_lower.contains("vl")
                || name_lower.contains("vision")
                || name_lower.contains("qwen3.5")
                || name_lower.contains("qwen-3.5")
                || name_lower.contains("qwen3-")
                || name_lower.contains("qwen-3-")
                || name_lower.contains("qwen3_")
                || name_lower.contains("qwen-max")
                || name_lower.contains("qwen-plus")
                || name_lower.contains("qwen-turbo")
        }
        CloudProvider::DeepSeek => {
            // DeepSeek vision models
            name_lower.contains("vl") || name_lower.contains("vision")
        }
        CloudProvider::GLM => {
            // GLM vision models: glm-4v, glm-4v-plus, etc.
            name_lower.contains("4v") || name_lower.contains("vision") || name_lower.contains("vl")
        }
        CloudProvider::MiniMax => {
            // MiniMax vision models (check documentation for specific models)
            name_lower.contains("vl")
                || name_lower.contains("vision")
                || name_lower.contains("multimodal")
        }
        CloudProvider::Grok => {
            // Grok vision support (check xAI documentation)
            // Currently grok-2-vision supports vision
            name_lower.contains("vision")
        }
        CloudProvider::Custom => {
            // For custom providers, assume vision support if model name suggests it
            // Include patterns from all major providers since custom endpoints may proxy any model
            name_lower.contains("vision")
                || name_lower.contains("vl")
                || name_lower.contains("multimodal")
                // OpenAI patterns
                || name_lower.contains("gpt-4o")
                || name_lower.contains("gpt-4-turbo")
                // Anthropic patterns
                || name_lower.contains("claude-3")
                || name_lower.contains("claude-4")
                // Google patterns
                || name_lower.contains("gemini")
                // Qwen patterns (qwen3.5, qwen3, qwen-max, qwen-plus, qwen-turbo all support vision)
                || name_lower.contains("qwen3.5")
                || name_lower.contains("qwen-3.5")
                || name_lower.contains("qwen3-")
                || name_lower.contains("qwen-3-")
                || name_lower.contains("qwen-max")
                || name_lower.contains("qwen-plus")
                || name_lower.contains("qwen-turbo")
                // DeepSeek patterns
                || name_lower.contains("deepseek-vl")
                // GLM patterns
                || name_lower.contains("glm-4v")
                // MiniMax patterns
                || name_lower.contains("minimax-vl")
                // Grok patterns
                || name_lower.contains("grok-vision")
        }
    }
}

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

    #[test]
    fn test_is_vision_model_openai() {
        // OpenAI vision models
        assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4o"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4o-mini"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4-turbo"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4-vision-preview"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4-1106-vision-preview"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "o1"));
        assert!(is_vision_model(&CloudProvider::OpenAI, "o1-mini"));

        // OpenAI non-vision models
        assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-4"));
        assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-4-32k"));
        assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-3.5-turbo"));
        assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-3.5"));
    }

    #[test]
    fn test_is_vision_model_anthropic() {
        // Anthropic vision models (all Claude 3+)
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-opus"));
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-sonnet"));
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-haiku"));
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-5-sonnet"));
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3.5-sonnet"));

        // Anthropic non-vision models
        assert!(!is_vision_model(&CloudProvider::Anthropic, "claude-2"));
        assert!(!is_vision_model(&CloudProvider::Anthropic, "claude-instant"));
    }

    #[test]
    fn test_is_vision_model_google() {
        // Google vision models (all Gemini)
        assert!(is_vision_model(&CloudProvider::Google, "gemini-1.5-flash"));
        assert!(is_vision_model(&CloudProvider::Google, "gemini-1.5-pro"));
        assert!(is_vision_model(&CloudProvider::Google, "gemini-pro-vision"));
        assert!(is_vision_model(&CloudProvider::Google, "gemini-2.0-flash"));

        // Non-gemini models
        assert!(!is_vision_model(&CloudProvider::Google, "palm-2"));
    }

    #[test]
    fn test_is_vision_model_qwen() {
        // Qwen explicit VL models
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-vl"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen2-vl"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3-vl"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-max-vl"));

        // Qwen3.5 series (all support vision)
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-turbo"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-plus"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-max"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-3.5-plus"));

        // Qwen3 series (all support vision)
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3-turbo"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3-plus"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3-max"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-3-plus"));

        // Qwen max/plus/turbo (newer versions support vision)
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-max"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-plus"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen-turbo"));

        // Non-vision models (older qwen versions without vision support)
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-7b"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-14b"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-72b"));
    }
}
