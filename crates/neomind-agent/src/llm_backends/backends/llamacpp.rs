//! llama.cpp standalone server backend implementation.
//!
//! Supports the llama.cpp server (llama-server) which provides an OpenAI-compatible
//! API with additional llama.cpp-specific features:
//! - `/v1/chat/completions` for text generation (streaming and non-streaming)
//! - `/health` for health checks
//! - `/props` for server property discovery
//! - `reasoning_content` field for thinking/reasoning models
//! - `cache_prompt` for KV cache reuse

use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput, LlmRuntime,
    ReasoningCapabilities, ReasoningControl, StreamChunk, ThinkingEffort, TokenUsage,
};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

/// Default llama.cpp server endpoint.
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080";

/// Default timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Configuration for llama.cpp backend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlamaCppConfig {
    /// llama.cpp server endpoint (default: http://127.0.0.1:8080)
    #[serde(default = "default_endpoint")]
    pub endpoint: String,

    /// Model name (optional — llama.cpp loads the model at server startup).
    /// Leave empty to use the server's loaded model.
    #[serde(default)]
    pub model: String,

    /// Request timeout in seconds (default: 180).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Optional Bearer token for `--api-key` authentication.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Enable KV cache reuse via `cache_prompt` (default: true).
    #[serde(default = "default_true")]
    pub cache_prompt: bool,
}

fn default_endpoint() -> String {
    DEFAULT_ENDPOINT.to_string()
}

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_true() -> bool {
    true
}

impl LlamaCppConfig {
    /// Get the timeout as a Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Create a new config with the given model name.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: model.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            api_key: None,
            cache_prompt: true,
        }
    }

    /// Set a custom endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Set timeout in seconds.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set API key for Bearer token auth.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set cache_prompt option.
    pub fn with_cache_prompt(mut self, cache: bool) -> Self {
        self.cache_prompt = cache;
        self
    }

    /// Get the effective base URL (strip trailing slash).
    fn base_url(&self) -> &str {
        self.endpoint.trim_end_matches('/')
    }
}

impl Default for LlamaCppConfig {
    fn default() -> Self {
        Self::new("")
    }
}

/// Capabilities override detected from server or storage.
#[derive(Debug, Clone)]
pub struct LlamaCppCapabilities {
    pub supports_multimodal: bool,
    pub supports_thinking: bool,
    pub supports_tools: bool,
    pub max_context: usize,
    /// Reported by `/props` `modalities.audio`. Currently informational — the
    /// agent pipeline does not yet emit audio `ContentPart`s — but surfaced so
    /// the capability report is honest.
    pub supports_audio: bool,
}

/// llama.cpp runtime backend.
pub struct LlamaCppRuntime {
    config: LlamaCppConfig,
    client: Client,
    model: String,
    metrics: Arc<RwLock<BackendMetrics>>,
    capabilities_override: Option<LlamaCppCapabilities>,
}

impl LlamaCppRuntime {
    /// Create a new llama.cpp runtime.
    pub fn new(config: LlamaCppConfig) -> Result<Self, LlmError> {
        tracing::debug!(
            "Creating llama.cpp runtime with endpoint: {}",
            config.endpoint
        );

        let client = Client::builder()
            // Don't set a global timeout — it kills streaming responses.
            // The timeout field is only used for non-streaming requests via per-request timeout.
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let model = config.model.clone();

        Ok(Self {
            config,
            client,
            model,
            metrics: Arc::new(RwLock::new(BackendMetrics::default())),
            capabilities_override: None,
        })
    }

    /// Set capabilities override from storage or detection.
    pub fn with_capabilities_override(
        mut self,
        supports_multimodal: bool,
        supports_thinking: bool,
        supports_tools: bool,
        max_context: usize,
        supports_audio: bool,
    ) -> Self {
        self.capabilities_override = Some(LlamaCppCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context,
            supports_audio,
        });
        self
    }

    /// Fetch server properties from `/props` endpoint.
    pub async fn fetch_props(&self) -> Option<LlamaCppProps> {
        let url = format!("{}/props", self.config.base_url());

        let req = match &self.config.api_key {
            Some(key) => self.client.get(&url).bearer_auth(key),
            None => self.client.get(&url),
        };

        match req.send().await {
            Ok(resp) if resp.status().is_success() => resp.json::<LlamaCppProps>().await.ok(),
            _ => None,
        }
    }

    /// Detect capabilities from llama.cpp server `/props` endpoint.
    ///
    /// Queries the server for model modalities, context size, and tool support.
    /// Returns `LlamaCppCapabilities` if detection succeeds.
    pub async fn detect_capabilities(&self) -> Option<LlamaCppCapabilities> {
        let props = self.fetch_props().await?;
        let n_ctx = props
            .default_generation_settings
            .as_ref()
            .and_then(|s| s.n_ctx)
            .unwrap_or(128000);

        let supports_multimodal = props.modalities.as_ref().map(|m| m.vision).unwrap_or(false);
        let supports_audio = props.modalities.as_ref().map(|m| m.audio).unwrap_or(false);

        let supports_tools = props
            .chat_template_caps
            .as_ref()
            .map(|c| c.supports_tools)
            .unwrap_or(true);

        // Thinking support: detect from model name or chat template
        let model_name = props
            .model_alias
            .as_deref()
            .or(props.model_path.as_deref())
            .unwrap_or("");
        // Use the unified thinking detector (covers qwen3 / deepseek-r1 /
        // qwq / glm-z1 / gpt-oss / "thinking"-suffixed models). The old
        // inline rule only matched a subset and missed e.g. qwq-32b, so a
        // thinking model could be misdetected as non-thinking and the UI
        // would hide the thinking control entirely.
        let supports_thinking = neomind_core::llm::detect_thinking(model_name);

        tracing::info!(
            model = model_name,
            n_ctx,
            supports_multimodal,
            supports_audio,
            supports_tools,
            supports_thinking,
            "Detected llama.cpp capabilities from /props"
        );

        Some(LlamaCppCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context: n_ctx,
            supports_audio,
        })
    }

    /// Convert messages to OpenAI-compatible format.
    fn messages_to_api(&self, messages: &[Message]) -> Vec<ApiMessage> {
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
                                ContentPart::ImageUrl { url, .. } => ApiContentPart::ImageUrl {
                                    image_url: ImageUrlContent {
                                        url: url.clone(),
                                        detail: Some("auto".to_string()),
                                    },
                                },
                                ContentPart::ImageBase64 {
                                    data,
                                    mime_type,
                                    detail: _,
                                } => ApiContentPart::ImageUrl {
                                    image_url: ImageUrlContent {
                                        url: format!("data:{};base64,{}", mime_type, data),
                                        detail: Some("auto".to_string()),
                                    },
                                },
                            })
                            .collect();
                        ApiContent::Parts(api_parts)
                    }
                };

                let role = match msg.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                };

                ApiMessage {
                    role: role.to_string(),
                    content,
                    tool_name: msg.tool_name.clone(),
                }
            })
            .collect()
    }

    /// Build an authenticated request builder.
    fn auth_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let builder = self.client.request(method, url);
        match &self.config.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
    }
}

#[async_trait::async_trait]
impl LlmRuntime for LlamaCppRuntime {
    fn backend_id(&self) -> BackendId {
        BackendId::new("llamacpp")
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        let url = format!("{}/health", self.config.base_url());
        match self.auth_request(reqwest::Method::GET, &url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn generate(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<LlmOutput, LlmError> {
        let start_time = Instant::now();
        let model = input.model.unwrap_or_else(|| self.model.clone());
        let url = format!("{}/v1/chat/completions", self.config.base_url());

        // Handle max_tokens: llama.cpp will error if max_tokens exceeds the model's
        // context window. When the caller sends a sentinel value (usize::MAX), omit
        // max_tokens entirely and let the server use its own default.
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => None, // sentinel → omit
            Some(v) => {
                let cap = self.max_context_length() as u32;
                if cap > 0 && (v as u32) > cap {
                    None // would exceed context → omit
                } else {
                    Some((v as u32).min(cap))
                }
            }
            None => None,
        };

        let mut req_body = serde_json::json!({
            "messages": self.messages_to_api(&input.messages),
            "stream": false,
            "cache_prompt": self.config.cache_prompt,
        });

        if !model.is_empty() {
            req_body["model"] = serde_json::json!(model);
        }
        if let Some(temp) = input.params.temperature {
            req_body["temperature"] = serde_json::json!(temp);
        }
        if let Some(top_p) = input.params.top_p {
            req_body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(max_tokens) = max_tokens {
            req_body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(ref stop) = input.params.stop {
            req_body["stop"] = serde_json::json!(stop);
        }

        // Tools
        if let Some(ref tools) = input.tools {
            if !tools.is_empty() {
                let openai_tools: Vec<OpenAiTool> =
                    tools.iter().map(|t| OpenAiTool::from(t.clone())).collect();
                req_body["tools"] = serde_json::json!(openai_tools);
            }
        }

        let response = self
            .auth_request(reqwest::Method::POST, &url)
            .json(&req_body)
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
            self.metrics
                .write()
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to acquire write lock on metrics: {}", e);
                    e.into_inner()
                })
                .record_failure();

            // Check for context overflow and return specific error for retry
            if body.contains("exceed_context_size_error") {
                let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
                let err_obj = parsed.as_ref().and_then(|v| v.get("error"));
                let prompt_tokens = err_obj
                    .and_then(|e| e.get("n_prompt_tokens"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as usize;
                // Prefer the server-reported n_ctx — it reflects the ACTUAL
                // --ctx-size the server was started with. Our cached
                // max_context_length() may be stale (e.g. server restarted
                // with a different ctx-size but capabilities not re-detected)
                // or a theoretical default, leading to misleading messages
                // like "11958 < 32000" when the real limit is 8192.
                let max_context = err_obj
                    .and_then(|e| e.get("n_ctx"))
                    .and_then(|t| t.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or_else(|| self.max_context_length());
                return Err(LlmError::ContextOverflow {
                    prompt_tokens,
                    max_context,
                });
            }

            return Err(LlmError::Generation(format!(
                "llama.cpp API error {}: {}",
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

        // Handle tool calls
        let native_tool_calls = if let Some(ref tool_calls) = choice.message.tool_calls {
            if !tool_calls.is_empty() {
                tracing::debug!("llama.cpp: received {} native tool calls", tool_calls.len());
                let tool_calls_json: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
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

        // Extract thinking content from reasoning_content field
        let thinking = choice.message.reasoning_content;

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
            thinking,
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

    async fn generate_stream(
        &self,
        input: neomind_core::llm::backend::LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel(64);

        let model = input.model.unwrap_or_else(|| self.model.clone());
        let url = format!("{}/v1/chat/completions", self.config.base_url());
        let api_key = self.config.api_key.clone();
        let client = self.client.clone();
        let cache_prompt = self.config.cache_prompt;

        // Handle max_tokens: llama.cpp will error if max_tokens exceeds the model's
        // context window. When the caller sends a sentinel value (usize::MAX), omit
        // max_tokens entirely and let the server use its own default.
        let max_context = self.max_context_length() as u32;
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => None, // sentinel → omit
            Some(v) => {
                if max_context > 0 && (v as u32) > max_context {
                    None // would exceed context → omit
                } else {
                    Some((v as u32).min(max_context))
                }
            }
            None => None,
        };

        let api_messages = self.messages_to_api(&input.messages);
        let msg_count = api_messages.len();

        let mut req_body = serde_json::json!({
            "messages": api_messages,
            "stream": true,
            "cache_prompt": cache_prompt,
            "stream_options": { "include_usage": true },
        });

        tracing::info!(
            endpoint = %url,
            model = %model,
            message_count = msg_count,
            has_tools = input.tools.as_ref().is_some_and(|t| !t.is_empty()),
            "llama.cpp generate_stream: sending request"
        );

        if !model.is_empty() {
            req_body["model"] = serde_json::json!(model);
        }
        if let Some(temp) = input.params.temperature {
            req_body["temperature"] = serde_json::json!(temp);
        }
        if let Some(top_p) = input.params.top_p {
            req_body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(max_tokens) = max_tokens {
            req_body["max_tokens"] = serde_json::json!(max_tokens);
        }

        // Tools
        if let Some(ref tools) = input.tools {
            if !tools.is_empty() {
                let openai_tools: Vec<OpenAiTool> =
                    tools.iter().map(|t| OpenAiTool::from(t.clone())).collect();
                req_body["tools"] = serde_json::json!(openai_tools);
            }
        }

        // Capture max_context for error reporting inside spawned task
        let max_context_capture = self.max_context_length();

        tokio::spawn(async move {
            let mut req_builder = client.post(&url).json(&req_body);
            if let Some(ref key) = api_key {
                req_builder = req_builder.bearer_auth(key);
            }

            let result = req_builder.send().await;

            match result {
                Ok(response) => {
                    let status = response.status();
                    tracing::info!(
                        status = %status.as_u16(),
                        "llama.cpp generate_stream: received response"
                    );

                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let _ = tx
                            .send(Err(LlmError::Generation("Rate limited by API".to_string())))
                            .await;
                        return;
                    }

                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();

                        // Check for context overflow and return specific error for retry
                        if body.contains("exceed_context_size_error") {
                            let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
                            let err_obj = parsed.as_ref().and_then(|v| v.get("error"));
                            let prompt_tokens = err_obj
                                .and_then(|e| e.get("n_prompt_tokens"))
                                .and_then(|t| t.as_u64())
                                .unwrap_or(0)
                                as usize;
                            // Prefer server-reported n_ctx over cached value —
                            // see non-stream path above for rationale.
                            let max_context = err_obj
                                .and_then(|e| e.get("n_ctx"))
                                .and_then(|t| t.as_u64())
                                .map(|n| n as usize)
                                .unwrap_or(max_context_capture);
                            let _ = tx
                                .send(Err(LlmError::ContextOverflow {
                                    prompt_tokens,
                                    max_context,
                                }))
                                .await;
                            return;
                        }

                        let _ = tx
                            .send(Err(LlmError::Generation(format!(
                                "llama.cpp API error {}: {}",
                                status.as_u16(),
                                body
                            ))))
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

                    while let Some(chunk_result) = stream.next().await {
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
                                                        let _ = tx
                                                            .send(Ok((content.clone(), false)))
                                                            .await;
                                                    }
                                                }

                                                // Handle reasoning_content (thinking)
                                                if let Some(ref reasoning) =
                                                    choice.delta.reasoning_content
                                                {
                                                    if !reasoning.is_empty() {
                                                        let _ = tx
                                                            .send(Ok((reasoning.clone(), true)))
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

                                                // Check for finish reason - flush tool calls
                                                if choice.finish_reason.as_deref()
                                                    == Some("tool_calls")
                                                    && !accumulated_tool_calls.is_empty()
                                                {
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
                    tracing::error!(
                        error = %e,
                        "llama.cpp generate_stream: HTTP request failed"
                    );
                    let _ = tx.send(Err(LlmError::Network(e.to_string()))).await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn max_context_length(&self) -> usize {
        if let Some(ref caps) = self.capabilities_override {
            caps.max_context
        } else {
            4096
        }
    }

    fn supports_multimodal(&self) -> bool {
        if let Some(ref caps) = self.capabilities_override {
            caps.supports_multimodal
        } else {
            false
        }
    }

    fn capabilities(&self) -> BackendCapabilities {
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
            // Default: llama.cpp supports streaming and tools via --jinja flag
            (false, true, true, 4096, false)
        };

        BackendCapabilities {
            streaming: true,
            multimodal: supports_multimodal,
            function_calling: supports_function_calling,
            multiple_models: false, // Model is loaded at server startup
            max_context: Some(max_context),
            modalities: vec!["text".to_string()],
            thinking_display: supports_thinking,
            supports_images: supports_multimodal,
            supports_audio,
            // llama.cpp has no request-side thinking toggle — thinking follows
            // the model default and is only readable via `reasoning_content`.
            reasoning: ReasoningCapabilities {
                supported_efforts: Vec::new(),
                default_effort: if supports_thinking {
                    Some(ThinkingEffort::High)
                } else {
                    None
                },
                mandatory: false,
                control: ReasoningControl::ReadOnly,
            },
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

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

/// Server properties from `/props` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct LlamaCppProps {
    /// Default generation parameters
    #[serde(default)]
    pub default_generation_settings: Option<GenerationSettings>,
    /// Number of total slots
    #[serde(default)]
    pub total_slots: Option<usize>,
    /// Server software version
    #[serde(default)]
    pub version: Option<String>,
    /// Model alias (display name)
    #[serde(default)]
    pub model_alias: Option<String>,
    /// Model file path
    #[serde(default)]
    pub model_path: Option<String>,
    /// Supported modalities
    #[serde(default)]
    pub modalities: Option<Modalities>,
    /// Chat template capabilities
    #[serde(default)]
    pub chat_template_caps: Option<ChatTemplateCaps>,
}

/// Supported modalities reported by llama.cpp server.
#[derive(Debug, Clone, Deserialize)]
pub struct Modalities {
    /// Whether the model supports vision/image input
    #[serde(default)]
    pub vision: bool,
    /// Whether the model supports audio input
    #[serde(default)]
    pub audio: bool,
}

/// Chat template capabilities reported by llama.cpp server.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatTemplateCaps {
    /// Whether the template supports tool calls
    #[serde(default)]
    pub supports_tool_calls: bool,
    /// Whether the template supports tools
    #[serde(default)]
    pub supports_tools: bool,
    /// Whether the template supports parallel tool calls
    #[serde(default)]
    pub supports_parallel_tool_calls: bool,
    /// Whether the template supports system role
    #[serde(default)]
    pub supports_system_role: bool,
}

/// Generation settings from server props.
#[derive(Debug, Clone, Deserialize)]
pub struct GenerationSettings {
    /// Model file path
    #[serde(default)]
    pub model: Option<String>,
    /// Context size
    #[serde(default)]
    pub n_ctx: Option<usize>,
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
}

#[derive(Debug, Serialize)]
struct ImageUrlContent {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Tool definition in OpenAI format.
#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunction,
}

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

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<String>,
    /// Thinking/reasoning content (llama.cpp-specific)
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallResponse>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCallResponse {
    #[serde(default)]
    id: Option<String>,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Accumulated tool call from streaming chunks.
#[derive(Debug, Clone)]
struct AccumulatedToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

// ---------------------------------------------------------------------------
// Streaming types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StreamChunkEvent {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    /// Usage data - only present in the final chunk when stream_options.include_usage=true
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    /// Thinking/reasoning content (llama.cpp-specific)
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamToolCall {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    function: Option<StreamFunctionCall>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamFunctionCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llamacpp_config_default() {
        let config = LlamaCppConfig::default();
        assert_eq!(config.endpoint, "http://127.0.0.1:8080");
        assert!(config.model.is_empty());
        assert!(config.cache_prompt);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_llamacpp_config_builder() {
        let config = LlamaCppConfig::new("llama-3")
            .with_endpoint("http://192.168.1.100:8080")
            .with_api_key("secret")
            .with_cache_prompt(false)
            .with_timeout_secs(300);

        assert_eq!(config.model, "llama-3");
        assert_eq!(config.endpoint, "http://192.168.1.100:8080");
        assert_eq!(config.api_key, Some("secret".to_string()));
        assert!(!config.cache_prompt);
        assert_eq!(config.timeout_secs, 300);
    }

    #[test]
    fn test_llamacpp_config_base_url() {
        let config = LlamaCppConfig::default();
        assert_eq!(config.base_url(), "http://127.0.0.1:8080");

        let config_with_slash = LlamaCppConfig::default().with_endpoint("http://127.0.0.1:8080/");
        assert_eq!(config_with_slash.base_url(), "http://127.0.0.1:8080");
    }

    #[test]
    fn test_llamacpp_config_serialization() {
        let config = LlamaCppConfig::new("test-model");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: LlamaCppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "test-model");
        assert!(parsed.cache_prompt);
    }

    #[test]
    fn test_llamacpp_runtime_new() {
        let config = LlamaCppConfig::new("llama-3");
        let runtime = LlamaCppRuntime::new(config).unwrap();
        assert_eq!(runtime.model_name(), "llama-3");
        assert_eq!(runtime.backend_id().as_str(), "llamacpp");
    }

    #[test]
    fn test_llamacpp_capabilities() {
        let config = LlamaCppConfig::default();
        let runtime = LlamaCppRuntime::new(config).unwrap();
        let caps = runtime.capabilities();
        assert!(caps.streaming);
        assert!(caps.function_calling);
        assert!(caps.thinking_display);
    }

    #[test]
    fn test_llamacpp_capabilities_override() {
        let config = LlamaCppConfig::default();
        let runtime = LlamaCppRuntime::new(config)
            .unwrap()
            .with_capabilities_override(true, true, true, 32768, true);
        let caps = runtime.capabilities();
        assert!(caps.streaming);
        assert!(caps.multimodal);
        assert!(caps.function_calling);
        assert!(caps.thinking_display);
        assert_eq!(caps.max_context, Some(32768));
        assert!(caps.supports_audio);
    }
}
