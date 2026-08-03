//! OpenAI-compatible cloud LLM backend implementation.
//!
//! Supports cloud APIs that are compatible with OpenAI's format:
//! - OpenAI (GPT-4, GPT-3.5, o1, etc.)
//! - Anthropic Claude (native Messages API)
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
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmInput, LlmOutput,
    LlmRuntime, StreamChunk, TokenUsage,
};
use neomind_core::message::{Content, ContentPart, ImageDetail, Message, MessageRole};

use super::super::rate_limited_client::{ProviderRateLimits, RateLimitedClient};

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
            Self::GLM => "https://open.bigmodel.cn/api/coding/paas/v4",
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
    /// Optional override for capabilities (from storage/API detection)
    /// If None, capabilities are detected from model name heuristics
    capabilities_override: Option<CloudCapabilities>,
}

/// Capabilities override for cloud runtime.
#[derive(Debug, Clone)]
struct CloudCapabilities {
    supports_multimodal: bool,
    supports_thinking: bool,
    supports_tools: bool,
    max_context: usize,
    supports_audio: bool,
}

impl CloudRuntime {
    /// Create a new cloud runtime.
    pub fn new(config: CloudConfig) -> Result<Self, LlmError> {
        // Note: Don't set a global timeout — it kills long-running streaming responses
        // from thinking models that can take many minutes.
        // Instead, we use per-request timeouts only for non-streaming requests.
        // Streaming responses have their own timeout via stream_config.max_stream_duration_secs.
        let http_client = Client::builder()
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
            capabilities_override: None,
        })
    }

    /// Set capabilities override from storage/API detection.
    /// This allows using accurate capabilities from the backend instance storage
    /// instead of name-based heuristics.
    pub fn with_capabilities_override(
        mut self,
        supports_multimodal: bool,
        supports_thinking: bool,
        supports_tools: bool,
        max_context: usize,
        supports_audio: bool,
    ) -> Self {
        self.capabilities_override = Some(CloudCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context,
            supports_audio,
        });
        self
    }

    /// Convert messages to API format (provider-specific).
    /// For Anthropic, uses their image format. For OpenAI/Google, uses OpenAI-style format.
    fn messages_to_api(&self, messages: &[Message]) -> Vec<ApiMessage> {
        let is_anthropic = matches!(self.config.provider, CloudProvider::Anthropic);
        // Whether this model can accept image input. Image parts in history
        // (e.g. an earlier turn with a vision model, or a previous attachment)
        // are stripped for text-only models — otherwise the API rejects the
        // whole request with `unknown variant image_url, expected text`.
        let can_multimodal = self.supports_multimodal();

        messages
            .iter()
            .map(|msg| {
                let content = match &msg.content {
                    Content::Text(text) => ApiContent::Text(text.clone()),
                    Content::Parts(parts) => {
                        let mut api_parts: Vec<ApiContentPart> = parts
                            .iter()
                            .filter_map(|part| match part {
                                ContentPart::Text { text } => {
                                    Some(ApiContentPart::Text { text: text.clone() })
                                }
                                ContentPart::ImageUrl { url, detail } => {
                                    // Drop image parts entirely for text-only models.
                                    if !can_multimodal {
                                        return None;
                                    }
                                    if is_anthropic {
                                        // Anthropic format: {"type": "image", "source": {...}}
                                        let (media_type, data) = extract_data_url(url);
                                        Some(ApiContentPart::AnthropicImage {
                                            source: AnthropicImageSource {
                                                typ: "base64".to_string(),
                                                media_type,
                                                data,
                                            },
                                        })
                                    } else {
                                        // OpenAI/Google format: {"type": "image_url", "image_url": {"url": "...", "detail": "auto"}}
                                        Some(ApiContentPart::ImageUrl {
                                            image_url: ImageUrlContent {
                                                url: url.clone(),
                                                detail: Some(image_detail_to_string(
                                                    detail.as_ref().unwrap_or(&ImageDetail::Auto),
                                                )),
                                            },
                                        })
                                    }
                                }
                                ContentPart::ImageBase64 {
                                    data,
                                    mime_type,
                                    detail: _,
                                } => {
                                    if !can_multimodal {
                                        return None;
                                    }
                                    if is_anthropic {
                                        // Anthropic format: raw base64 data
                                        Some(ApiContentPart::AnthropicImage {
                                            source: AnthropicImageSource {
                                                typ: "base64".to_string(),
                                                media_type: mime_type.clone(),
                                                data: data.clone(),
                                            },
                                        })
                                    } else {
                                        // OpenAI/Google format: data URL
                                        Some(ApiContentPart::ImageUrl {
                                            image_url: ImageUrlContent {
                                                url: format!(
                                                    "data:{};base64,{}",
                                                    mime_type, data
                                                ),
                                                detail: Some("auto".to_string()),
                                            },
                                        })
                                    }
                                }
                            })
                            .collect();

                        // If every part was a stripped image (image-only message),
                        // leave a text placeholder so the message is non-empty (some
                        // APIs reject empty content) and the model knows context was
                        // dropped.
                        if api_parts.is_empty() {
                            api_parts.push(ApiContentPart::Text {
                                text: "[image content omitted — current model does not support image input]"
                                    .to_string(),
                            });
                        }

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
                    tool_name: msg.tool_name.clone(),
                }
            })
            .collect()
    }

    /// Build an Anthropic-native API request from LlmInput.
    /// Extracts system messages into the top-level `system` field
    /// and converts tool schemas from OpenAI to Anthropic format.
    fn build_anthropic_request(
        &self,
        input: &neomind_core::llm::backend::LlmInput,
        stream: bool,
    ) -> (AnthropicRequest, String) {
        let model = input.model.clone().unwrap_or_else(|| self.model.clone());

        // Handle max_tokens: Anthropic requires this field
        const MAX_TOKENS_CAP: u32 = 32768;
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => MAX_TOKENS_CAP,
            Some(v) => (v as u32).min(MAX_TOKENS_CAP),
            None => 8192, // Anthropic default
        };

        // Extract system messages and convert remaining messages
        let mut system_text = String::new();
        let mut messages: Vec<AnthropicApiMessage> = Vec::new();

        for msg in &input.messages {
            match msg.role {
                MessageRole::System => {
                    // Concatenate system messages
                    let text = match &msg.content {
                        Content::Text(t) => t.clone(),
                        Content::Parts(parts) => parts
                            .iter()
                            .filter_map(|p| match p {
                                ContentPart::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    if !system_text.is_empty() {
                        system_text.push('\n');
                    }
                    system_text.push_str(&text);
                }
                _ => {
                    let role = match msg.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "user",
                        MessageRole::System => unreachable!(),
                    };

                    // Convert content to Anthropic format
                    let content_value = match &msg.content {
                        Content::Text(t) => serde_json::Value::String(t.clone()),
                        Content::Parts(parts) => {
                            let api_parts: Vec<serde_json::Value> = parts
                                .iter()
                                .map(|part| match part {
                                    ContentPart::Text { text } => {
                                        serde_json::json!({"type": "text", "text": text})
                                    }
                                    ContentPart::ImageUrl { url, .. }
                                    | ContentPart::ImageBase64 { data: url, .. } => {
                                        let (media_type, data) = extract_data_url(url);
                                        serde_json::json!({
                                            "type": "image",
                                            "source": {
                                                "type": "base64",
                                                "media_type": media_type,
                                                "data": data
                                            }
                                        })
                                    }
                                })
                                .collect();
                            serde_json::Value::Array(api_parts)
                        }
                    };

                    messages.push(AnthropicApiMessage {
                        role: role.to_string(),
                        content: content_value,
                    });
                }
            }
        }

        // Convert tools from OpenAI format to Anthropic format
        let tools = input.tools.as_ref().map(|tools| {
            tools
                .iter()
                .map(|t| AnthropicTool {
                    name: t.name.clone(),
                    description: Some(t.description.clone()),
                    input_schema: t.parameters.clone(),
                })
                .collect::<Vec<_>>()
        });

        let request = AnthropicRequest {
            model: model.clone(),
            max_tokens,
            system: if system_text.is_empty() {
                None
            } else {
                Some(system_text)
            },
            messages,
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            stop_sequences: input.params.stop.clone(),
            stream,
            tools,
        };

        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );

        // === SFT trace hook ===
        // When NEOMIND_TRACE_DIR is set, dump the full Anthropic request
        // (system prompt + messages + tools) the LLM actually received, so
        // SFT training data can be reconstructed with exact input fidelity.
        // Zero overhead when the env var is unset. See memory: minicpm5-neomind-baseline.
        if let Ok(dir) = std::env::var("NEOMIND_TRACE_DIR") {
            if let Ok(json) = serde_json::to_string(&request) {
                let path = std::path::Path::new(&dir).join("anthropic_trace.jsonl");
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", json);
                }
            }
        }

        (request, url)
    }

    /// Build the OpenAI-compatible `ChatCompletionRequest` from `LlmInput`.
    ///
    /// Extracted from `generate_openai` / `generate_stream_openai` so the
    /// request body is constructable without performing HTTP — enables unit
    /// tests on the serialized payload (notably `enable_thinking` wiring).
    ///
    /// `stream` controls both the `stream` flag and whether `stream_options`
    /// is populated (OpenAI requires `include_usage: true` to receive token
    /// counts in the final chunk).
    fn build_chat_request(&self, input: LlmInput, stream: bool) -> ChatCompletionRequest {
        let model = input.model.unwrap_or_else(|| self.model.clone());

        // Handle max_tokens for cloud APIs.
        // MUST set explicitly — many providers (DeepSeek, GLM) default to only ~4096
        // when this field is omitted, which silently truncates tool call JSON mid-output.
        const MAX_TOKENS_CAP: u32 = 32768; // 32k — sufficient for agent reasoning + tool call JSON
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => Some(MAX_TOKENS_CAP),
            Some(v) => Some((v as u32).min(MAX_TOKENS_CAP)),
            None => Some(MAX_TOKENS_CAP),
        };

        // DashScope (Qwen) documents `enable_thinking: bool` for hybrid
        // thinking models (qwen3.x-plus). Without this knob, thinking defaults
        // ON — `thinking_enabled: Some(false)` set by analyzer.rs / intent.rs /
        // tool_result.rs (gotcha #7) was silently dropped on cloud, while the
        // Ollama path (ollama.rs:826-844) honored it. Other OpenAI-compatible
        // providers may reject unknown fields, so emit ONLY for Qwen.
        let enable_thinking = if matches!(self.config.provider, CloudProvider::Qwen) {
            input.params.thinking_enabled
        } else {
            None
        };

        let request = ChatCompletionRequest {
            model,
            messages: self.messages_to_api(&input.messages),
            temperature: input.params.temperature,
            top_p: input.params.top_p,
            max_tokens,
            stop: input.params.stop.clone(),
            frequency_penalty: input.params.frequency_penalty,
            presence_penalty: input.params.presence_penalty,
            stream,
            tools: input
                .tools
                .map(|tools| tools.into_iter().map(OpenAiTool::from).collect()),
            stream_options: if stream {
                Some(StreamOptions {
                    include_usage: true,
                })
            } else {
                None
            },
            enable_thinking,
        };

        // === SFT trace hook (OpenAI-compatible path) ===
        // Mirror of `build_anthropic_request`: when NEOMIND_TRACE_DIR is set,
        // dump the full ChatCompletionRequest the LLM received so SFT training
        // data can be reconstructed for OpenAI-compatible backends too. The
        // system prompt rides as `messages[0]` (role "system") on this path —
        // distinct from Anthropic's top-level `system` field. Written to a
        // SEPARATE file (`openai_trace.jsonl`) so teacher (Anthropic = golden
        // traces) and student (e.g. MiniCPM5 served via llama.cpp's /v1)
        // never collide. Zero overhead when the env var is unset.
        // See memory: minicpm5-neomind-baseline.
        if let Ok(dir) = std::env::var("NEOMIND_TRACE_DIR") {
            if let Ok(json) = serde_json::to_string(&request) {
                let path = std::path::Path::new(&dir).join("openai_trace.jsonl");
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", json);
                }
            }
        }

        request
    }

    /// Parse tool calls that leaked into the assistant `content` as XML.
    ///
    /// Some local runtimes (e.g. the Nanbeige llama.cpp fork) fail to lift
    /// tool calls into the OpenAI `tool_calls` field: their PEG parser's
    /// `content_before_tools` rule bails when the model emits preamble text
    /// before `<tool_call>`, so the whole call lands in `content` as
    /// `<tool_call><function=name><parameter=k>v</parameter></function></tool_call>`.
    /// This recovers those calls so the agent can still act.
    fn parse_xml_tool_calls(content: &str) -> Vec<serde_json::Value> {
        use regex::Regex;
        let block_re = Regex::new(r"(?s)<tool_call>\s*(.*?)\s*</tool_call>").unwrap();
        let func_re = Regex::new(r"(?s)<function=([\w-]+)>(.*?)</function>").unwrap();
        let param_re = Regex::new(r"(?s)<parameter=([\w-]+)>(.*?)</parameter>").unwrap();
        let mut out = Vec::new();
        for b in block_re.captures_iter(content) {
            let inner = b.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(fc) = func_re.captures(inner) {
                let name = fc.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                let body = fc.get(2).map(|m| m.as_str()).unwrap_or("");
                let mut args = serde_json::Map::new();
                for pc in param_re.captures_iter(body) {
                    let k = pc.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                    let v = pc.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
                    args.insert(k, serde_json::Value::String(v));
                }
                out.push(serde_json::json!({
                    "id": serde_json::Value::Null,
                    "name": name,
                    "arguments": serde_json::Value::Object(args),
                }));
            }
        }
        out
    }

    /// OpenAI-compatible non-streaming generation path.
    async fn generate_openai(
        &self,
        input: neomind_core::llm::backend::LlmInput,
        start_time: Instant,
    ) -> Result<LlmOutput, LlmError> {
        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );

        let request = self.build_chat_request(input, false);

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
            .timeout(self.config.timeout())
            .json(&request);

        // Build the request - reqwest::RequestBuilder::build() can fail if headers are invalid
        let built_request = req
            .build()
            .map_err(|e| LlmError::Network(format!("Failed to build HTTP request: {}", e)))?;

        let response = self
            .client
            .execute_request(&rate_limit_key, built_request)
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        if !status.is_success() {
            self.metrics
                .write()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to acquire write lock on metrics: {}", e);
                    e.into_inner()
                })
                .record_failure();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body,
            });
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
        let native_tool_calls = if let Some(ref tool_calls) = choice.message.tool_calls {
            if !tool_calls.is_empty() {
                tracing::debug!("OpenAI: received {} native tool calls", tool_calls.len());
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
                // Keep text serialization for backward compat
                let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
                response_text.push_str(&json_str);
                Some(tool_calls_json)
            } else {
                None
            }
        } else {
            None
        };

        // Fallback: recover tool calls that leaked into content as XML when the
        // upstream parser (e.g. llama.cpp fork) failed to populate `tool_calls`.
        // See `parse_xml_tool_calls` for the cause.
        let native_tool_calls = if native_tool_calls.is_none()
            && response_text.contains("<tool_call>")
        {
            let parsed = Self::parse_xml_tool_calls(&response_text);
            if !parsed.is_empty() {
                tracing::debug!(
                    "OpenAI: recovered {} tool call(s) from content XML fallback",
                    parsed.len()
                );
                let json_str = serde_json::to_string(&parsed).unwrap_or_default();
                response_text.push_str(&json_str);
                Some(parsed)
            } else {
                None
            }
        } else {
            native_tool_calls
        };

        let result = Ok(LlmOutput {
            text: response_text,
            finish_reason: match choice.finish_reason.as_str() {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                "tool_calls" => FinishReason::ToolCalls,
                _ => FinishReason::Error,
            },
            usage: chat_response.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            thinking: choice.message.reasoning_content,
            tool_calls: native_tool_calls,
        });

        // Record metrics
        let latency_ms = start_time.elapsed().as_millis() as u64;
        match &result {
            Ok(output) => {
                let tokens = output.usage.map_or(0, |u| u.completion_tokens as u64);
                self.metrics
                    .write()
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to acquire write lock on metrics: {}", e);
                        e.into_inner()
                    })
                    .record_success(tokens, latency_ms);
            }
            Err(_) => {
                self.metrics
                    .write()
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to acquire write lock on metrics: {}", e);
                        e.into_inner()
                    })
                    .record_failure();
            }
        }

        result
    }

    /// Anthropic-native non-streaming generation path.
    async fn generate_anthropic(
        &self,
        input: neomind_core::llm::backend::LlmInput,
        start_time: Instant,
    ) -> Result<LlmOutput, LlmError> {
        let (request, url) = self.build_anthropic_request(&input, false);

        let rate_limit_key = format!(
            "{:?}:{:x}",
            self.config.provider,
            hash_api_key(&self.config.api_key)
        );

        let req = self
            .client
            .inner()
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .timeout(self.config.timeout())
            .json(&request);

        // Build the request - reqwest::RequestBuilder::build() can fail if headers are invalid
        let built_request = req
            .build()
            .map_err(|e| LlmError::Network(format!("Failed to build HTTP request: {}", e)))?;

        let response = self
            .client
            .execute_request(&rate_limit_key, built_request)
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        if !status.is_success() {
            self.metrics
                .write()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to acquire write lock on metrics: {}", e);
                    e.into_inner()
                })
                .record_failure();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body,
            });
        }

        // Check if the response is an error payload wrapped in HTTP 200
        // (common with proxy/gateway services)
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
            if val.get("error").is_some()
                || (val.get("code").is_some() && val.get("msg").is_some())
                || (val.get("code").is_some() && val.get("success").is_some())
            {
                self.metrics
                    .write()
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to acquire write lock on metrics: {}", e);
                        e.into_inner()
                    })
                    .record_failure();
                return Err(LlmError::Api {
                    status: status.as_u16(),
                    body,
                });
            }
        }

        let api_response: AnthropicResponse = serde_json::from_str(&body).map_err(|e| {
            LlmError::Generation(format!(
                "Anthropic deserialization error: {} - body: {}",
                e, body
            ))
        })?;

        // Build response text from content blocks
        let mut response_text = String::new();
        let mut tool_calls_json: Vec<serde_json::Value> = Vec::new();

        for block in &api_response.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    response_text.push_str(text);
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls_json.push(serde_json::json!({
                        "id": id,
                        "name": name,
                        "arguments": input
                    }));
                }
            }
        }

        // Append tool calls as JSON if any
        if !tool_calls_json.is_empty() {
            let json_str = serde_json::to_string(&tool_calls_json).unwrap_or_default();
            response_text.push_str(&json_str);
        }

        let finish_reason = match api_response.stop_reason.as_deref() {
            Some("end_turn") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            Some("stop_sequence") => FinishReason::Stop,
            Some("tool_use") => FinishReason::ToolCalls,
            _ => FinishReason::Error,
        };

        let result = Ok(LlmOutput {
            text: response_text,
            finish_reason,
            usage: Some(TokenUsage {
                prompt_tokens: api_response.usage.input_tokens,
                completion_tokens: api_response.usage.output_tokens,
                total_tokens: api_response.usage.input_tokens + api_response.usage.output_tokens,
            }),
            thinking: None,
            tool_calls: if tool_calls_json.is_empty() {
                None
            } else {
                Some(tool_calls_json)
            },
        });

        // Record metrics
        let latency_ms = start_time.elapsed().as_millis() as u64;
        match &result {
            Ok(output) => {
                let tokens = output.usage.map_or(0, |u| u.completion_tokens as u64);
                self.metrics
                    .write()
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to acquire write lock on metrics: {}", e);
                        e.into_inner()
                    })
                    .record_success(tokens, latency_ms);
            }
            Err(_) => {
                self.metrics
                    .write()
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to acquire write lock on metrics: {}", e);
                        e.into_inner()
                    })
                    .record_failure();
            }
        }

        result
    }

    /// OpenAI-compatible streaming generation path.
    fn generate_stream_openai(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel(64);

        let url = format!(
            "{}{}",
            self.config.get_base_url(),
            self.config.provider.chat_path()
        );
        let api_key = self.config.api_key.clone();
        let rate_limiter = self.client.clone();
        let inner_client = self.client.inner().clone();
        let provider = self.config.provider;

        let request = self.build_chat_request(input, true);
        // Idle timeout for the streaming read (see the bytes_stream loop below):
        // computed OUTSIDE the async-move block so it's a plain owned Duration,
        // not a borrow of `self`.
        let read_idle_timeout = self.config.timeout();

        tokio::spawn(async move {
            // Create rate limit key
            let rate_limit_key = format!("{:?}:{:x}", provider, hash_api_key(&api_key));

            // Acquire rate limit permit before making request
            rate_limiter.acquire(&rate_limit_key).await;

            let result = inner_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&request)
                // Bounded request: `send()` waits for the response HEADERS. A
                // single-slot backend (e.g. llama.cpp) that accepts the
                // connection but queues the request behind an in-flight one may
                // never send headers → `send()` blocks forever. The non-streaming
                // paths use `.timeout()`; the streaming path missed it. Same
                // 60s budget as the idle read timeout below.
                .timeout(read_idle_timeout)
                .send()
                .await;

            match result {
                Ok(response) => {
                    let status = response.status();

                    // Handle rate limit response — read body for debugging
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let body = response.text().await.unwrap_or_default();
                        tracing::warn!(
                            "Rate limited (429) response body: {}",
                            &body[..body.len().min(500)]
                        );
                        let _ = tx
                            .send(Err(LlmError::Generation("Rate limited by API".to_string())))
                            .await;
                        return;
                    }

                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx
                            .send(Err(LlmError::Api {
                                status: status.as_u16(),
                                body,
                            }))
                            .await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    // Accumulate tool calls across chunks
                    let mut accumulated_tool_calls: std::collections::HashMap<
                        u32,
                        AccumulatedToolCall,
                    > = std::collections::HashMap::new();
                    // Accumulate content for fallback XML tool-call recovery at [DONE].
                    let mut accumulated_content = String::new();

                    // Idle timeout on the raw HTTP read: without this, a stalled
                    // upstream SSE connection (no chunk within `config.timeout()` —
                    // default 60s) blocks `stream.next()` forever, hanging every
                    // consumer (PermitStream in chat_stream_internal, the
                    // stream_core/multimodal loops) — observed as an 8h eval hang
                    // on Gemma4 QAT. Normal generation emits chunks continuously,
                    // so a 60s zero-chunk gap is unambiguously a dead connection.
                    // `unwrap_or(None)` treats a stall as end-of-stream: the loop
                    // exits and the [DONE] flush path below still runs.
                    while let Some(chunk_result) = tokio::time::timeout(
                        read_idle_timeout,
                        stream.next(),
                    )
                    .await
                    .unwrap_or(None)
                    {
                        // If the consumer dropped the receiver (chat UI closed,
                        // agent execution cancelled/timed out), stop draining the
                        // upstream HTTP body. Without this check we'd keep pulling
                        // chunks from the provider — burning output tokens and
                        // holding a connection-pool slot — until the model itself
                        // finishes or the upstream connection times out.
                        if tx.is_closed() {
                            tracing::debug!(
                                "Stream consumer dropped, aborting upstream consumption"
                            );
                            return;
                        }
                        match chunk_result {
                            Ok(chunk) => {
                                buffer.extend_from_slice(&chunk);

                                // Process complete lines from buffer
                                let mut search_start = 0;
                                while let Some(nl_pos) =
                                    buffer[search_start..].iter().position(|&b| b == b'\n')
                                {
                                    let line_end = search_start + nl_pos;
                                    let line_bytes = &buffer[..line_end];
                                    let line =
                                        String::from_utf8_lossy(line_bytes).trim().to_string();

                                    // Remove processed line from buffer
                                    buffer = buffer[line_end + 1..].to_vec();
                                    search_start = 0;

                                    if line.is_empty() {
                                        continue;
                                    }
                                    if line == "data: [DONE]" {
                                        // Flush any accumulated tool calls
                                        if !accumulated_tool_calls.is_empty() {
                                            let tool_calls_json: Vec<serde_json::Value> =
                                                accumulated_tool_calls
                                                    .values()
                                                    .map(|tc| {
                                                        let args: serde_json::Value =
                                                            serde_json::from_str(&tc.arguments)
                                                                .unwrap_or_else(|_| {
                                                                    serde_json::json!({})
                                                                });
                                                        serde_json::json!({
                                                            "id": tc.id,
                                                            "name": tc.name,
                                                            "arguments": args
                                                        })
                                                    })
                                                    .collect();
                                            let json_str = serde_json::to_string(&tool_calls_json)
                                                .unwrap_or_default();
                                            let _ = tx.send(Ok((json_str, false))).await;
                                        } else if accumulated_content.contains("<tool_call>") {
                                            // Fallback: upstream parser failed to populate
                                            // delta.tool_calls (preamble before <tool_call>
                                            // confused its content_before_tools), so recover
                                            // the calls from accumulated content XML.
                                            let parsed =
                                                Self::parse_xml_tool_calls(&accumulated_content);
                                            if !parsed.is_empty() {
                                                tracing::debug!(
                                                    "OpenAI stream: recovered {} tool call(s) from content XML fallback",
                                                    parsed.len()
                                                );
                                                let json_str = serde_json::to_string(&parsed)
                                                    .unwrap_or_default();
                                                let _ = tx.send(Ok((json_str, false))).await;
                                            }
                                        }
                                        let _ = tx.send(Ok((String::new(), false))).await;
                                        continue;
                                    }
                                    if let Some(json) = line.strip_prefix("data: ") {
                                        if let Ok(evt) =
                                            serde_json::from_str::<StreamChunkEvent>(json)
                                        {
                                            // Check for usage data in final chunk (stream_options.include_usage=true)
                                            if let Some(ref usage) = evt.usage {
                                                if usage.prompt_tokens > 0 {
                                                    let _ = tx
                                                        .send(Ok((
                                                            format!(
                                                                "\n__NEOMIND_TOKEN_PROMPT:{}__",
                                                                usage.prompt_tokens
                                                            ),
                                                            false,
                                                        )))
                                                        .await;
                                                }
                                            }

                                            if let Some(choice) = evt.choices.first() {
                                                // Handle content
                                                if let Some(ref content) = choice.delta.content {
                                                    if !content.is_empty() {
                                                        accumulated_content.push_str(content);
                                                        let _ = tx
                                                            .send(Ok((content.clone(), false)))
                                                            .await;
                                                    }
                                                }

                                                // Handle tool calls (incremental)
                                                if let Some(ref tool_calls) =
                                                    choice.delta.tool_calls
                                                {
                                                    for tc in tool_calls {
                                                        let entry = accumulated_tool_calls
                                                            .entry(tc.index)
                                                            .or_insert(AccumulatedToolCall {
                                                                id: None,
                                                                name: None,
                                                                arguments: String::new(),
                                                            });

                                                        if let Some(ref id) = tc.id {
                                                            entry.id = Some(id.clone());
                                                        }

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

                                                // Check for finish reason - flush tool calls.
                                                // Also flush on "length" (truncation) to recover
                                                // partial tool calls instead of silently dropping them.
                                                let should_flush = matches!(
                                                    choice.finish_reason.as_deref(),
                                                    Some("tool_calls") | Some("length")
                                                ) && !accumulated_tool_calls
                                                    .is_empty();

                                                if should_flush {
                                                    let tool_calls_json: Vec<serde_json::Value> =
                                                        accumulated_tool_calls
                                                            .values()
                                                            .map(|tc| {
                                                                let args: serde_json::Value =
                                                                    serde_json::from_str(
                                                                        &tc.arguments,
                                                                    )
                                                                    .unwrap_or_else(|_| {
                                                                        serde_json::json!({})
                                                                    });
                                                                serde_json::json!({
                                                                    "id": tc.id,
                                                                    "name": tc.name,
                                                                    "arguments": args
                                                                })
                                                            })
                                                            .collect();
                                                    let json_str =
                                                        serde_json::to_string(&tool_calls_json)
                                                            .unwrap_or_default();
                                                    let _ = tx.send(Ok((json_str, false))).await;
                                                    accumulated_tool_calls.clear();
                                                }
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

    /// Anthropic-native streaming generation path.
    fn generate_stream_anthropic(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel(64);

        let (request, url) = self.build_anthropic_request(&input, true);
        let api_key = self.config.api_key.clone();
        let rate_limiter = self.client.clone();
        let inner_client = self.client.inner().clone();
        // Idle timeout for the streaming read (see the bytes_stream loop below):
        // owned Duration moved into the async block (not a borrow of `self`).
        let read_idle_timeout = self.config.timeout();

        tokio::spawn(async move {
            let rate_limit_key = format!("Anthropic:{:x}", hash_api_key(&api_key));
            rate_limiter.acquire(&rate_limit_key).await;

            let result = inner_client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request)
                // Bounded request: see the openai streaming send — wait-for-headers
                // can block forever on a queued single-slot backend.
                .timeout(read_idle_timeout)
                .send()
                .await;

            match result {
                Ok(response) => {
                    let status = response.status();

                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let body = response.text().await.unwrap_or_default();
                        tracing::warn!(
                            "Rate limited (429) non-streaming body: {}",
                            &body[..body.len().min(500)]
                        );
                        let _ = tx
                            .send(Err(LlmError::Generation("Rate limited by API".to_string())))
                            .await;
                        return;
                    }

                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx
                            .send(Err(LlmError::Api {
                                status: status.as_u16(),
                                body,
                            }))
                            .await;
                        return;
                    }

                    // If we get JSON instead of an event stream, it's an error wrapped in HTTP 200
                    let content_type = response
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if content_type.contains("application/json") {
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx
                            .send(Err(LlmError::Generation(format!(
                                "Anthropic API error (unexpected JSON response): {}",
                                body
                            ))))
                            .await;
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    // Accumulate tool call arguments: (id, name, arguments_json)
                    let mut accumulated_tool_calls: std::collections::HashMap<
                        u32,
                        (Option<String>, Option<String>, String),
                    > = std::collections::HashMap::new();

                    while let Some(chunk_result) = tokio::time::timeout(
                        read_idle_timeout,
                        stream.next(),
                    )
                    .await
                    .unwrap_or(None)
                    {
                        // If the consumer dropped the receiver (chat UI closed,
                        // agent execution cancelled/timed out), stop draining the
                        // upstream HTTP body. Without this check we'd keep pulling
                        // chunks from the provider — burning output tokens and
                        // holding a connection-pool slot — until the model itself
                        // finishes or the upstream connection times out.
                        if tx.is_closed() {
                            tracing::debug!(
                                "Stream consumer dropped, aborting upstream consumption"
                            );
                            return;
                        }
                        match chunk_result {
                            Ok(chunk) => {
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

                                    if let Some(json) = line.strip_prefix("data: ") {
                                        if let Ok(evt) =
                                            serde_json::from_str::<AnthropicStreamEvent>(json)
                                        {
                                            match evt {
                                                AnthropicStreamEvent::ContentBlockStart {
                                                    index,
                                                    content_block,
                                                } => {
                                                    // For tool_use blocks, extract id and name
                                                    if content_block
                                                        .get("type")
                                                        .and_then(|v| v.as_str())
                                                        == Some("tool_use")
                                                    {
                                                        let id = content_block
                                                            .get("id")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| s.to_string());
                                                        let name = content_block
                                                            .get("name")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| s.to_string());
                                                        accumulated_tool_calls
                                                            .entry(index)
                                                            .or_insert((id, name, String::new()));
                                                    }
                                                }
                                                AnthropicStreamEvent::ContentBlockDelta {
                                                    index,
                                                    delta,
                                                } => {
                                                    match delta.delta_type.as_str() {
                                                        "text_delta" => {
                                                            if let Some(ref text) = delta.text {
                                                                if !text.is_empty() {
                                                                    let _ = tx
                                                                        .send(Ok((
                                                                            text.clone(),
                                                                            false,
                                                                        )))
                                                                        .await;
                                                                }
                                                            }
                                                        }
                                                        "input_json_delta" => {
                                                            // Accumulate tool call arguments
                                                            if let Some(ref partial) =
                                                                delta.partial_json
                                                            {
                                                                let entry = accumulated_tool_calls
                                                                    .entry(index)
                                                                    .or_insert((
                                                                        None,
                                                                        None,
                                                                        String::new(),
                                                                    ));
                                                                entry.2.push_str(partial);
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                AnthropicStreamEvent::ContentBlockStop {
                                                    index,
                                                } => {
                                                    // Flush accumulated tool call if present
                                                    if let Some((id, name, args_json)) =
                                                        accumulated_tool_calls.remove(&index)
                                                    {
                                                        if name.is_some() {
                                                            let args: serde_json::Value =
                                                                serde_json::from_str(&args_json)
                                                                    .unwrap_or_else(|_| {
                                                                        serde_json::json!({})
                                                                    });
                                                            // Wrap in array format for consistent parsing with OpenAI format
                                                            // This ensures detect_json_tool_calls can properly detect the tool call
                                                            let tc_json = serde_json::json!([{
                                                                "id": id,
                                                                "name": name,
                                                                "arguments": args
                                                            }]);
                                                            let json_str =
                                                                serde_json::to_string(&tc_json)
                                                                    .unwrap_or_default();
                                                            let _ = tx
                                                                .send(Ok((json_str, false)))
                                                                .await;
                                                        }
                                                    }
                                                }
                                                AnthropicStreamEvent::MessageStop => {
                                                    // Signal end of stream
                                                    let _ =
                                                        tx.send(Ok((String::new(), false))).await;
                                                }
                                                _ => {}
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

        // Anthropic-native API path
        if self.config.provider == CloudProvider::Anthropic {
            return self.generate_anthropic(input, start_time).await;
        }

        // OpenAI-compatible path (default)
        self.generate_openai(input, start_time).await
    }

    async fn generate_stream(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        // Anthropic-native streaming path
        if self.config.provider == CloudProvider::Anthropic {
            return self.generate_stream_anthropic(input);
        }

        // OpenAI-compatible streaming path (default)
        self.generate_stream_openai(input)
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
        // Use override if available, otherwise fall back to name-based detection
        if let Some(ref caps) = self.capabilities_override {
            caps.supports_multimodal
        } else {
            // Check if the specific model supports vision based on model name
            let model = self.model.to_lowercase();
            is_vision_model(&self.config.provider, &model)
        }
    }

    fn capabilities(&self) -> BackendCapabilities {
        // Use override if available (from storage), otherwise detect from name
        let (
            supports_multimodal,
            supports_function_calling,
            supports_thinking,
            max_context,
            supports_audio,
        ) = if let Some(ref caps) = self.capabilities_override {
            (
                caps.supports_multimodal,
                caps.supports_tools,
                caps.supports_thinking,
                caps.max_context,
                caps.supports_audio,
            )
        } else {
            // Fall back to name-based heuristics
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
            let model_lower = self.model.to_lowercase();
            let supports_audio = is_audio_model(&self.config.provider, &model_lower);
            (
                supports_multimodal,
                supports_function_calling,
                false, // thinking not detected by name
                self.max_context_length(),
                supports_audio,
            )
        };

        BackendCapabilities {
            streaming: true,
            multimodal: supports_multimodal,
            function_calling: supports_function_calling,
            multiple_models: true,
            max_context: Some(max_context),
            modalities: vec!["text".to_string()],
            thinking_display: supports_thinking,
            supports_images: supports_multimodal,
            supports_audio,
        }
    }

    fn metrics(&self) -> BackendMetrics {
        self.metrics
            .read()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to acquire read lock on metrics: {}", e);
                e.into_inner()
            })
            .clone()
    }
}

// Helper functions

/// Extract media type and base64 data from an image data URL or raw base64.
/// Returns (media_type, base64_data) — always non-empty.
///
/// Delegates to [`crate::image_utils::parse_image_data`] for canonical MIME
/// handling (jpg→jpeg aliasing, magic-prefix inference for raw base64).
fn extract_data_url(url: &str) -> (String, String) {
    match crate::image_utils::parse_image_data(url) {
        Some(parsed) => (parsed.mime_type.to_string(), parsed.base64.to_string()),
        // Empty input or utterly unrecognizable — last-resort fallback.
        None => ("image/png".to_string(), url.to_string()),
    }
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
    /// Request usage data in streaming response (OpenAI stream_options)
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    /// DashScope (Qwen) hybrid-thinking toggle. qwen3.x-plus defaults to
    /// thinking ON; without this knob the model burns tokens on hidden CoT
    /// during non-chat LLM calls (memory extraction, intent parsing, Phase 2
    /// fallback — gotcha #7) and risks gateway idle timeouts on long
    /// reasoning under non-streaming mode. Only emitted for
    /// `CloudProvider::Qwen`; other OpenAI-compatible servers may reject
    /// unknown fields. Mirrors the Ollama path's `thinking_enabled` handling
    /// (ollama.rs:826-844).
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
}

/// Stream options to request usage data in final chunk
#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
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
    /// Reasoning chain emitted by thinking/reasoning models (DeepSeek-R1,
    /// Qwen3.x-plus, GLM-4.6 thinking, Moonshot K2, etc.). This is the
    /// de-facto industry standard field originated by DeepSeek-R1 and adopted
    /// by vLLM/SGLang/LMDeploy/SiliconFlow. Silently dropping it loses the
    /// model's chain-of-thought — must be captured into `LlmOutput.thinking`
    /// to mirror the llamacpp path.
    #[serde(default)]
    reasoning_content: Option<String>,
}

/// Tool call in OpenAI response format
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAiToolCallResponse {
    /// Tool call ID
    id: Option<String>,
    /// Tool type (always "function")
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
    #[serde(default)]
    choices: Vec<StreamChoice>,
    /// Usage data - only present in the final chunk when stream_options.include_usage=true
    #[serde(default)]
    usage: Option<Usage>,
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
#[allow(dead_code)]
struct StreamToolCall {
    /// Index of this tool call in the array
    index: u32,
    /// Tool call ID (only in first chunk)
    id: Option<String>,
    /// Tool type (only in first chunk)
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

// --- Anthropic-native API types ---

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Debug, Serialize)]
struct AnthropicApiMessage {
    role: String,
    content: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicMessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: serde_json::Value,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: AnthropicDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDeltaBody,
        usage: Option<AnthropicUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: String,
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageStart {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    usage: Option<AnthropicUsage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct AnthropicMessageDeltaBody {
    stop_reason: Option<String>,
}

/// Check if a model supports vision (image input) based on provider and model name.
/// This uses name-based heuristic detection for common vision-capable models.
fn is_vision_model(_provider: &CloudProvider, model_name: &str) -> bool {
    // Primary: centralized layered detection (LiteLLM registry → conservative
    // heuristic). This is authoritative when the registry has an entry.
    if neomind_core::llm::detect_vision_capability(model_name) {
        return true;
    }

    // Fallback: well-known vision families the LiteLLM registry misses under
    // bare aliases (e.g. `claude-3-sonnet`, `gemini-1.5-flash`, `o1`).
    //
    // This MUST stay narrow. The previous version matched bare Qwen
    // text-only commercial tiers (`qwen-max`, `qwen-plus`, `qwen-turbo`,
    // `qwen3-*`, `qwen-3-*`) as vision-capable. Cloud backends built via the
    // instance manager do not receive a `capabilities_override`, so they fell
    // back to this function, reported `supports_multimodal == true` for text
    // models, the chat gating let `image_url` content parts through, and the
    // upstream API rejected the request with
    // `unknown variant image_url, expected text`.
    //
    // The Qwen text tiers are deliberately excluded below — only explicit
    // `-vl`/`vision` variants and the native-multimodal qwen3.5/3.6/3.7
    // series match.
    known_vision_family(model_name)
}

/// Narrow fallback of unambiguous vision-family name patterns. Used only when
/// the layered registry/heuristic detection returns false, to cover cloud
/// models whose bare aliases are absent from the LiteLLM registry.
fn known_vision_family(model_name: &str) -> bool {
    let m = model_name.to_lowercase();
    // Explicit vision markers (suffixes / branding) — unambiguous.
    if m.contains("-vl")
        || m.contains(":vl")
        || m.contains("_vl")
        || m.contains("vision")
        || m.contains("multimodal")
        || m.contains("glm-4v")
        || m.contains("glm-5v")
    {
        return true;
    }
    // OpenAI vision families. o1-preview and o1-mini are text-only.
    if m.contains("gpt-4o")
        || m.contains("gpt-4-turbo")
        || m.contains("gpt-4.1")
        || m.contains("gpt-4-vision")
        || (m.starts_with("gpt-4") && m.contains("vision"))
        || (m.starts_with("o1") && !m.contains("o1-preview") && !m.contains("o1-mini"))
    {
        return true;
    }
    // Anthropic Claude 3+ and Google Gemini are universally multimodal.
    if m.contains("claude-3") || m.contains("claude-4") || m.contains("gemini") {
        return true;
    }
    // Qwen native-multimodal early-fusion series. The bare text tiers
    // (`qwen-max`/`qwen-plus`/`qwen-turbo`, `qwen3-*`, `qwen-3-*`) are
    // intentionally NOT matched here.
    if m.starts_with("qwen3.5") || m.starts_with("qwen3.6") || m.starts_with("qwen3.7") {
        return true;
    }
    false
}

/// Detect audio-capable models by name. Used only when no override is set
/// (i.e. the instance manager / runtime was constructed without consulting
/// the LiteLLM registry or runtime API).
///
/// Match surface is intentionally narrow — only explicit audio branding.
/// `gpt-4o-audio` here is the audio variant family; OpenAI text tiers (`gpt-4`,
/// `gpt-4-turbo`, `gpt-4.1`, bare `gpt-4o`) are deliberately excluded.
fn is_audio_model(
    // `_provider` is currently unused but reserved for future per-provider
    // audio quirks (e.g. some providers expose audio only on specific
    // regional endpoints, or brand audio-capable tiers differently). Keeping
    // the param avoids a breaking signature churn at every call site when
    // that distinction is needed.
    _provider: &CloudProvider,
    model_name: &str,
) -> bool {
    // Primary: centralized detector. After the C1 fix this shortlists
    // `audio/tts/asr/whisper/gpt-4o-audio/qwen-audio/qwen-tts/qwen-omni` —
    // bare `gpt-4o` and `gpt-4o-mini` are intentionally NOT matched.
    // Authoritative for registered models.
    if neomind_core::llm::capability::model_supports(model_name, "audio") {
        return true;
    }
    let m = model_name.to_lowercase();
    // Fallback: explicit audio branding not in the registry shortlist.
    m.contains("qwen2-audio")
        || m.contains("qwen2.5-omni")
        || m.contains("qwen3-omni")
        || m.contains("qwen-omni")
        || m.contains("audio-preview")
        || m.contains("gpt-4o-audio")
        || m.contains("step-1o")
        || m.contains("step-audio")
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::llm::backend::GenerationParams;

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
        assert!(is_vision_model(
            &CloudProvider::OpenAI,
            "gpt-4-vision-preview"
        ));
        assert!(is_vision_model(
            &CloudProvider::OpenAI,
            "gpt-4-1106-vision-preview"
        ));
        assert!(is_vision_model(&CloudProvider::OpenAI, "o1"));
        // o1-mini is text-only (no vision) — must NOT be reported as multimodal,
        // otherwise image parts get sent and the API rejects them.
        assert!(!is_vision_model(&CloudProvider::OpenAI, "o1-mini"));
        assert!(!is_vision_model(&CloudProvider::OpenAI, "o1-preview"));

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
        assert!(is_vision_model(
            &CloudProvider::Anthropic,
            "claude-3-sonnet"
        ));
        assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-haiku"));
        assert!(is_vision_model(
            &CloudProvider::Anthropic,
            "claude-3-5-sonnet"
        ));
        assert!(is_vision_model(
            &CloudProvider::Anthropic,
            "claude-3.5-sonnet"
        ));

        // Anthropic non-vision models
        assert!(!is_vision_model(&CloudProvider::Anthropic, "claude-2"));
        assert!(!is_vision_model(
            &CloudProvider::Anthropic,
            "claude-instant"
        ));
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

        // Qwen 3.5/3.6/3.7 native-multimodal series (early fusion, all vision)
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-turbo"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-plus"));
        assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-max"));

        // Text-only commercial tiers — MUST stay text. Reporting these as
        // vision causes the API to reject image parts with
        // `unknown variant image_url, expected text`.
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-3.5-plus"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-turbo"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-plus"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-max"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-3-plus"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-max"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-plus"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-turbo"));

        // Non-vision models (older qwen versions without vision support)
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-7b"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-14b"));
        assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-72b"));
    }

    /// `is_audio_model` should match explicit audio-branding (qwen-omni /
    /// qwen2-audio / gpt-4o-audio / whisper / etc.) but NOT match generic
    /// text/vision tiers. The centralized detector (post-C1 fix) already
    /// excludes bare `gpt-4o` / `gpt-4o-mini` from the audio shortlist, so
    /// we verify both paths: the centralized shortlist via the positive
    /// audio-branded cases below, and the local fallback via `step-audio`
    /// and the dated `gpt-4o-audio-*` variants.
    #[test]
    fn test_is_audio_model_patterns() {
        // Explicit audio branding — should match.
        assert!(is_audio_model(&CloudProvider::Qwen, "qwen-omni-turbo"));
        assert!(is_audio_model(&CloudProvider::Qwen, "qwen2-audio-7b"));
        assert!(is_audio_model(&CloudProvider::Qwen, "qwen2.5-omni-7b"));
        assert!(is_audio_model(&CloudProvider::Qwen, "qwen3-omni"));
        assert!(is_audio_model(
            &CloudProvider::OpenAI,
            "gpt-4o-audio-preview"
        ));
        assert!(is_audio_model(&CloudProvider::OpenAI, "gpt-4o-audio"));
        assert!(is_audio_model(&CloudProvider::Custom, "step-audio-1"));

        // Text-only / vision-only — MUST stay false.
        assert!(!is_audio_model(&CloudProvider::OpenAI, "gpt-4-turbo"));
        assert!(!is_audio_model(&CloudProvider::OpenAI, "gpt-4.1"));
        assert!(!is_audio_model(
            &CloudProvider::Anthropic,
            "claude-3-5-sonnet"
        ));
        assert!(!is_audio_model(&CloudProvider::Qwen, "qwen-max"));
        assert!(!is_audio_model(&CloudProvider::Qwen, "qwen-plus"));
        assert!(!is_audio_model(&CloudProvider::Qwen, "qwen3-vl-plus"));
        assert!(!is_audio_model(&CloudProvider::DeepSeek, "deepseek-chat"));
    }

    /// Regression: a text-only model must never receive `image_url` content
    /// parts. The original production bug was DeepSeek (text-only) rejecting a
    /// whole request with `unknown variant image_url, expected text` because a
    /// earlier conversation turn contained an image and the history was replayed
    /// verbatim. `messages_to_api` now strips image parts when
    /// `supports_multimodal()` is false.
    #[test]
    fn test_messages_to_api_strips_images_for_text_model() {
        let runtime =
            CloudRuntime::new(CloudConfig::deepseek("sk-test").with_model("deepseek-chat"))
                .expect("runtime builds");
        // Sanity: this is a text-only model.
        assert!(
            !runtime.supports_multimodal(),
            "deepseek-chat must be detected as text-only for this test to be meaningful"
        );

        // History entry: a user turn with a text part + an image part (e.g. an
        // image that was attached earlier in the conversation).
        let history_msg = Message::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::Text {
                    text: "what is in this picture".to_string(),
                },
                ContentPart::ImageBase64 {
                    data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
                    mime_type: "image/png".to_string(),
                    detail: None,
                },
            ]),
        );
        // An image-only history turn (text was empty / dropped earlier).
        let image_only_msg = Message::new(
            MessageRole::User,
            Content::Parts(vec![ContentPart::ImageBase64 {
                data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
                mime_type: "image/png".to_string(),
                detail: None,
            }]),
        );
        // Current turn: plain text follow-up sent to the text-only model.
        let followup = Message::new(
            MessageRole::User,
            Content::Text("summarize our conversation".to_string()),
        );

        let api_msgs = runtime.messages_to_api(&[history_msg, image_only_msg, followup]);

        // Walk every content part of every message and assert no image variant
        // survives serialization for a text-only model.
        let mut saw_image = false;
        let mut saw_placeholder = false;
        let mut saw_history_text = false;
        for msg in &api_msgs {
            if let ApiContent::Parts(parts) = &msg.content {
                for part in parts {
                    match part {
                        ApiContentPart::ImageUrl { .. } | ApiContentPart::AnthropicImage { .. } => {
                            saw_image = true
                        }
                        ApiContentPart::Text { text } => {
                            if text.starts_with("[image content omitted") {
                                saw_placeholder = true;
                            }
                            if text == "what is in this picture" {
                                saw_history_text = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            !saw_image,
            "text-only model must not receive image parts in history replay"
        );
        // The text part of a mixed message is preserved (not dropped with the image).
        assert!(
            saw_history_text,
            "text part of a mixed text+image history turn must survive image stripping"
        );
        // The image-only turn collapses to a placeholder so the message is non-empty.
        assert!(
            saw_placeholder,
            "image-only message should be replaced with a text placeholder"
        );
    }

    /// Counter-test: a vision-capable model keeps the image parts intact.
    #[test]
    fn test_messages_to_api_keeps_images_for_vision_model() {
        let runtime = CloudRuntime::new(CloudConfig::openai("sk-test").with_model("gpt-4o"))
            .expect("runtime builds");
        assert!(
            runtime.supports_multimodal(),
            "gpt-4o must be detected as multimodal for this test to be meaningful"
        );

        let msg = Message::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::Text {
                    text: "describe this".to_string(),
                },
                ContentPart::ImageBase64 {
                    data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
                    mime_type: "image/png".to_string(),
                    detail: None,
                },
            ]),
        );

        let api_msgs = runtime.messages_to_api(&[msg]);
        let mut saw_image = false;
        if let ApiContent::Parts(parts) = &api_msgs[0].content {
            for part in parts {
                if matches!(part, ApiContentPart::ImageUrl { .. }) {
                    saw_image = true;
                }
            }
        }
        assert!(
            saw_image,
            "vision model must retain image parts in serialized output"
        );
    }

    // ── enable_thinking wiring for DashScope (Qwen) ──────────────────────
    //
    // Regression test for the silent-drop bug: `LlmInput.params.thinking_enabled`
    // was honored by the Ollama path (ollama.rs:826-844) but completely ignored
    // by the cloud OpenAI-compatible path. For qwen3.x-plus backends this meant
    // `thinking_enabled: Some(false)` set by analyzer.rs / intent.rs /
    // tool_result.rs (per gotcha #7) was silently discarded — the model kept
    // thinking on, burning tokens and risking DashScope gateway idle timeouts
    // on long reasoning under non-streaming mode.
    //
    // Fix: `ChatCompletionRequest` gained an `enable_thinking: Option<bool>`
    // field, populated ONLY for `CloudProvider::Qwen` (DashScope documents
    // this field for qwen3 hybrid thinking models). Other providers don't
    // accept it; sending it could break strict validators.

    #[test]
    fn test_qwen_request_emits_enable_thinking_when_disabled() {
        let runtime = CloudRuntime::new(CloudConfig::qwen("sk-test").with_model("qwen3.7-plus"))
            .expect("runtime builds");

        let input = LlmInput {
            messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
            params: GenerationParams {
                thinking_enabled: Some(false),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        let request = runtime.build_chat_request(input, false);
        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["enable_thinking"], serde_json::Value::Bool(false),
            "qwen backend must serialize enable_thinking:false when thinking_enabled is Some(false)");
    }

    #[test]
    fn test_qwen_request_omits_enable_thinking_when_default() {
        // When thinking_enabled is None, the field MUST be skipped — letting
        // the model use its default. Hard-coding enable_thinking:false would
        // silently turn off vision reasoning for qwen3.7-plus dashboards.
        let runtime = CloudRuntime::new(CloudConfig::qwen("sk-test").with_model("qwen3.7-plus"))
            .expect("runtime builds");

        let input = LlmInput {
            messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
            params: GenerationParams::default(),
            model: None,
            stream: false,
            tools: None,
        };

        let request = runtime.build_chat_request(input, false);
        let json = serde_json::to_value(&request).expect("serialize");
        assert!(
            json.get("enable_thinking")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "enable_thinking must be absent when thinking_enabled is None"
        );
    }

    #[test]
    fn test_non_qwen_request_never_emits_enable_thinking() {
        // DeepSeek / GLM / OpenAI / etc. don't accept `enable_thinking`.
        // Sending it could break strict validators on custom OpenAI-compatible
        // servers. The field is DashScope-specific.
        let runtime =
            CloudRuntime::new(CloudConfig::deepseek("sk-test").with_model("deepseek-chat"))
                .expect("runtime builds");

        let input = LlmInput {
            messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
            params: GenerationParams {
                thinking_enabled: Some(false),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        let request = runtime.build_chat_request(input, false);
        let json = serde_json::to_value(&request).expect("serialize");
        assert!(
            json.get("enable_thinking")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "non-Qwen providers must not receive enable_thinking field"
        );
    }

    /// SFT contract — OpenAI-compatible path. The `openai_trace.jsonl` hook
    /// dumps the ChatCompletionRequest; the system prompt MUST survive as
    /// `messages[0]` (role "system"). Without it, student traces (MiniCPM5
    /// via llama.cpp's /v1 endpoint) can't be used to diagnose why the model
    /// grabs file_write/skill/memory instead of shell. See memory:
    /// minicpm5-neomind-baseline.
    #[test]
    fn openai_request_carries_system_prompt_as_message_for_sft() {
        let runtime = CloudRuntime::new(CloudConfig::openai("sk-test").with_model("gpt-test"))
            .expect("runtime builds");

        let input = LlmInput {
            messages: vec![
                Message::new(MessageRole::System, Content::text("You are NeoMind.")),
                Message::new(MessageRole::User, Content::text("hi")),
            ],
            params: GenerationParams::default(),
            model: None,
            stream: false,
            tools: None,
        };

        let request = runtime.build_chat_request(input, false);
        let json = serde_json::to_value(&request).expect("serialize");
        let msgs = json["messages"].as_array().expect("messages array");
        assert!(!msgs.is_empty(), "messages must not be empty");
        assert_eq!(
            msgs[0]["role"], "system",
            "system prompt must be messages[0]"
        );
        assert_eq!(msgs[0]["content"], "You are NeoMind.");
    }

    /// SFT contract — Anthropic path (= golden teacher traces). The
    /// `anthropic_trace.jsonl` hook dumps the AnthropicRequest; the system
    /// prompt MUST survive serialization as a top-level `system` field. This
    /// is the entire reason the hook exists — history previously stored zero
    /// system messages, so SFT data had no prompt to train against.
    /// See memory: minicpm5-neomind-baseline.
    #[test]
    fn anthropic_request_carries_system_prompt_as_field_for_sft() {
        let runtime =
            CloudRuntime::new(CloudConfig::anthropic("sk-test").with_model("claude-test"))
                .expect("runtime builds");

        let input = LlmInput {
            messages: vec![
                Message::new(
                    MessageRole::System,
                    Content::text("You are NeoMind. Use the shell tool."),
                ),
                Message::new(MessageRole::User, Content::text("create a device")),
            ],
            params: GenerationParams::default(),
            model: None,
            stream: false,
            tools: None,
        };

        let (request, _url) = runtime.build_anthropic_request(&input, false);
        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(
            json["system"].as_str().unwrap(),
            "You are NeoMind. Use the shell tool.",
            "Anthropic trace must carry the system prompt as a top-level field"
        );
    }
}
