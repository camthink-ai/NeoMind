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

use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmOutput, LlmRuntime,
    ReasoningCapabilities, ReasoningControl, StreamChunk, ThinkingEffort, TokenUsage,
};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

use super::think_filter::{ThinkContentFilter, GRACE_CHARS};
use crate::llm_backends::text_tool_calls;

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

/// llama-server answers 503 {"error":{"message":"Loading model",...}} while
/// the model is still being loaded into memory — most commonly right after
/// switching to the builtin backend. That's not a failure; the server IS
/// coming up. Failing fast here surfaced a spurious error during backend
/// switches, so both call paths wait for readiness and resend once.
fn is_model_loading(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::SERVICE_UNAVAILABLE && body.contains("Loading model")
}

/// Hybrid llama.cpp templates (including renamed models) honor this per-request
/// switch. Do not infer the caller's intent from the display model name. Absent
/// overrides leave server defaults untouched; effort takes precedence over bool.
fn apply_thinking_override(
    body: &mut serde_json::Value,
    params: &neomind_core::llm::backend::GenerationParams,
) {
    let enabled = params
        .thinking_effort
        .map(|effort| effort != ThinkingEffort::None)
        .or(params.thinking_enabled);
    if let Some(enabled) = enabled {
        body["chat_template_kwargs"]["enable_thinking"] = serde_json::json!(enabled);
    }
}

/// Grace-window activation follows the FINAL value sent to the server: every
/// leak in the leakfix-r2 corpus (26/26) sat on requests that disabled
/// thinking, so only those buffer the opening run. Thinking-on requests keep
/// unbuffered first-token streaming; balanced blocks and unclosed opens are
/// repaired without any hold either way.
fn think_grace_for_request(req_body: &serde_json::Value) -> usize {
    if req_body["chat_template_kwargs"]["enable_thinking"] == serde_json::json!(false) {
        GRACE_CHARS
    } else {
        0
    }
}

/// Poll the server's /health until it answers 200 (model ready) or the
/// deadline passes. llama-server serves /health with 503 during load.
async fn wait_for_llama_model_ready(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &Option<String>,
    timeout: std::time::Duration,
) -> bool {
    let health = format!("{}/health", base_url.trim_end_matches('/'));
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let mut req = client
            .get(&health)
            .timeout(std::time::Duration::from_secs(2));
        if let Some(ref key) = api_key {
            req = req.bearer_auth(key);
        }
        if let Ok(r) = req.send().await {
            if r.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    false
}

/// One warn line per repaired response so acceptance runs can prove the
/// mechanism fired (grep `think_filter_strip` in server logs) instead of
/// mistaking a non-reproducing trial for a fix.
fn log_think_filter_stats(filter: &ThinkContentFilter) {
    let stats = filter.stats();
    if stats.markers_stripped > 0 || stats.chars_reclassified > 0 {
        tracing::warn!(
            markers_stripped = stats.markers_stripped,
            chars_reclassified = stats.chars_reclassified,
            "think_filter_strip: repaired think markers misrouted into the content channel"
        );
    }
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
    /// Server slot count from /props (concurrency capability info).
    /// `max_context` above is the server's per-request truth (`slot_n_ctx`)
    /// and is reported verbatim; the platform budgets from it directly.
    pub total_slots: Option<usize>,
}

/// Per-slot context a single request can claim on a PARTITIONED server
/// (each slot gets `-c / slots`). Kept for callers that reason about
/// partitioned deployments — the builtin path no longer divides: its spawn
/// is single-slot by default, and /props n_ctx is reported verbatim
/// (dividing it silently shrank the window 4× when the server auto-slotted
/// — the 8K-wedge class). Servers that don't report `total_slots` (older
/// builds) are treated as single-slot.
pub fn per_slot_context(n_ctx: usize, total_slots: Option<usize>) -> usize {
    n_ctx / total_slots.unwrap_or(1).max(1)
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
    ) -> Self {
        self.capabilities_override = Some(LlamaCppCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context,
            total_slots: None,
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

        // `/props` n_ctx is `slot_n_ctx` — what ONE request can claim:
        // per-slot quota on `--parallel` (partitioned) servers, the full
        // unified pool on auto-slotted ones. It is the server's own
        // per-request truth and is what the platform budgets from,
        // verbatim.
        let total_slots = props.total_slots;

        let supports_multimodal = props.modalities.as_ref().map(|m| m.vision).unwrap_or(false);

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
            total_slots,
            supports_multimodal,
            supports_tools,
            supports_thinking,
            "Detected llama.cpp capabilities from /props"
        );

        Some(LlamaCppCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context: n_ctx,
            total_slots: props.total_slots,
        })
    }

    /// KV-cache reuse breaks mtmd image placement, so image requests opt out.
    fn cache_prompt_for(&self, messages: &[ApiMessage]) -> bool {
        if !self.config.cache_prompt {
            return false;
        }
        let has_image = messages.iter().any(|m| match &m.content {
            ApiContent::Parts(parts) => parts
                .iter()
                .any(|p| matches!(p, ApiContentPart::ImageUrl { .. })),
            ApiContent::Text(_) => false,
        });
        !has_image
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

        // Handle max_tokens. When the caller delegates (sentinel usize::MAX or
        // unset), apply a bounded generation cap instead of omitting the field —
        // omitted means UNLIMITED on llama-server, and a runaway generation
        // (observed: 22177 tokens / 7.4 min on prod T4) keeps the slot busy
        // even after the client disconnects (llama-server does not cancel
        // in-flight tasks). 8192 is ~4x a long legitimate answer and still cuts
        // a runaway short.
        //
        // "Per remaining context" needs no client-side arithmetic: empirically
        // (verified 2026-08-17 on b10360 AND prod's 2da6686) llama-server does
        // NOT error when max_tokens exceeds the available context — it clamps
        // generation at the actual context wall (finish=length). The effective
        // bound is therefore min(cap, remaining) with the precise part
        // enforced server-side. The old comment claimed llama.cpp "will error"
        // on overflow; no version in production use does.
        let delegated_cap = || {
            let ctx = self.max_context_length() as u32;
            let cap = 8192u32;
            if ctx > 0 {
                Some(cap.min(ctx))
            } else {
                Some(cap)
            }
        };
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => delegated_cap(), // delegated → bounded default
            Some(v) => {
                let cap = self.max_context_length() as u32;
                if cap > 0 {
                    if (v as u32) > cap {
                        delegated_cap()
                    } else {
                        Some(v as u32)
                    }
                } else {
                    Some(v as u32) // context unknown → trust the caller's explicit value
                }
            }
            None => delegated_cap(),
        };

        // Text tool-calling teaching — see `llm_backends::text_tool_calls`.
        // The /props probe (or a user override) decides whether the server's
        // chat template lifts tools natively; when it reports no function
        // calling, teach the JSON protocol in the system message so the
        // agent-layer `tool_parser` can act on the reply.
        let messages = text_tool_calls::prepare_messages(
            input.messages,
            input.tools.as_deref(),
            self.capabilities().function_calling,
        );

        let api_messages = self.messages_to_api(&messages);
        let cache_prompt = self.cache_prompt_for(&api_messages);
        let mut req_body = serde_json::json!({
            "messages": api_messages,
            "stream": false,
            "cache_prompt": cache_prompt,
        });
        apply_thinking_override(&mut req_body, &input.params);
        let content_grace = think_grace_for_request(&req_body);

        if !model.is_empty() {
            req_body["model"] = serde_json::json!(model);
        }
        if let Some(temp) = input.params.temperature {
            req_body["temperature"] = serde_json::json!(temp);
        }
        if let Some(top_p) = input.params.top_p {
            req_body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(top_k) = input.params.top_k {
            req_body["top_k"] = serde_json::json!(top_k);
        }
        if let Some(max_tokens) = max_tokens {
            req_body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(ref stop) = input.params.stop {
            req_body["stop"] = serde_json::json!(stop);
        }

        // Grammar-constrained JSON: llama.cpp converts json_object into a GBNF
        // grammar so the sampled output is ALWAYS a valid JSON object. Small
        // models (1B-3B) drift to `[key]: value` pseudo-formats without it.
        if input.params.json_mode == Some(true) {
            req_body["response_format"] = serde_json::json!({"type": "json_object"});
        }

        // Tools
        if let Some(ref tools) = input.tools {
            if !tools.is_empty() {
                let openai_tools: Vec<OpenAiTool> =
                    tools.iter().map(|t| OpenAiTool::from(t.clone())).collect();
                req_body["tools"] = serde_json::json!(openai_tools);
            }
        }

        // Model-loading gate: on 503 "Loading model" wait for /health (≤60s)
        // and resend once before surfacing an error.
        let (status, body) = {
            let mut retried = false;
            loop {
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
                if !retried && is_model_loading(status, &body) {
                    retried = true;
                    tracing::info!(
                        "llama.cpp model still loading — waiting for readiness (up to 60s)"
                    );
                    let client = Client::builder()
                        .timeout(self.config.timeout())
                        .build()
                        .map_err(|e| LlmError::Network(e.to_string()))?;
                    if wait_for_llama_model_ready(
                        &client,
                        self.config.base_url(),
                        &self.config.api_key,
                        std::time::Duration::from_secs(60),
                    )
                    .await
                    {
                        continue;
                    }
                }
                break (status, body);
            }
        };

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

        // Repair think markers the server misrouted into content (stray
        // `</think>` after a template-consumed opening tag): the opening run
        // belongs to the thinking channel. Run BEFORE tool-call JSON is
        // appended below — the appended JSON must never pass through the
        // filter.
        let raw_content = choice.message.content.unwrap_or_default();
        let mut think_filter = ThinkContentFilter::with_grace(content_grace);
        let (mut repaired_thinking, mut clean_content) = think_filter.feed(&raw_content);
        {
            let (more_thinking, more_content) = think_filter.finish();
            repaired_thinking.push_str(&more_thinking);
            clean_content.push_str(&more_content);
        }
        log_think_filter_stats(&think_filter);

        // Build response text, including tool calls if present
        let mut response_text = clean_content;

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

        // Server-split reasoning first, then any reasoning the filter
        // recovered out of the content channel.
        let mut thinking = choice.message.reasoning_content.unwrap_or_default();
        thinking.push_str(&repaired_thinking);
        let thinking = if thinking.is_empty() {
            None
        } else {
            Some(thinking)
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

        // Handle max_tokens: llama.cpp will error if max_tokens exceeds the model's
        // context window. When the caller delegates (sentinel usize::MAX or unset),
        // apply a bounded generation cap instead of omitting the field — omitted
        // means UNLIMITED on llama-server and a runaway generation (observed:
        // 22177 tokens / 7.4 min on prod T4) blocks a slot even after the client
        // disconnects. See the non-streaming `generate` above for the rationale.
        let max_context = self.max_context_length() as u32;
        let delegated_cap = || {
            let cap = 8192u32;
            if max_context > 0 {
                Some(cap.min(max_context))
            } else {
                Some(cap)
            }
        };
        let max_tokens = match input.params.max_tokens {
            Some(v) if v >= usize::MAX - 1000 => delegated_cap(), // delegated → bounded default
            Some(v) => {
                if max_context > 0 {
                    if (v as u32) > max_context {
                        delegated_cap()
                    } else {
                        Some(v as u32)
                    }
                } else {
                    Some(v as u32) // context unknown → trust the caller's explicit value
                }
            }
            None => delegated_cap(),
        };

        // Same teaching gate as non-streaming `generate` — see
        // `llm_backends::text_tool_calls`.
        let messages = text_tool_calls::prepare_messages(
            input.messages,
            input.tools.as_deref(),
            self.capabilities().function_calling,
        );
        let api_messages = self.messages_to_api(&messages);
        let msg_count = api_messages.len();
        // Image turns must not reuse KV. llama.cpp's mtmd path reports
        // `find_slot: non-consecutive token position` and the model replies
        // as if the picture was never sent.
        let cache_prompt = self.cache_prompt_for(&api_messages);

        let mut req_body = serde_json::json!({
            "messages": api_messages,
            "stream": true,
            "cache_prompt": cache_prompt,
            "stream_options": { "include_usage": true },
        });
        apply_thinking_override(&mut req_body, &input.params);
        let content_grace = think_grace_for_request(&req_body);

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
        if let Some(top_k) = input.params.top_k {
            req_body["top_k"] = serde_json::json!(top_k);
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
        // Idle timeout for the streaming byte read (see `next_bytes_or_end`): a
        // stalled upstream SSE connection must force-complete the loop instead
        // of hanging `bytes_stream().next()` forever. openai already had this
        // (commit 162c73ff); llamacpp was missed — root cause of the eval
        // mid-stream wedge on thinking-loop stalls.
        let read_idle_timeout = self.config.timeout();

        tokio::spawn(async move {
            let mut req_builder = client.post(&url).json(&req_body);
            if let Some(ref key) = api_key {
                req_builder = req_builder.bearer_auth(key);
            }

            // Model-loading gate (mirrors the non-stream path): on 503
            // "Loading model" wait for /health (≤60s) and resend once —
            // a freshly switched builtin backend is loading, not broken.
            let mut result = req_builder.send().await;
            // Take ownership only when the gate actually fires; every path
            // through the block either returns or reassigns `result`, so the
            // match below always sees a valid value.
            let gate_needed = matches!(
                &result,
                Ok(r) if r.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
            );
            if gate_needed {
                let resp = match result {
                    Ok(r) => r,
                    Err(_) => unreachable!("gate only fires on Ok"),
                };
                let status_now = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                if is_model_loading(status_now, &body_text) {
                    tracing::info!("llama.cpp model still loading (stream) — waiting for readiness (up to 60s)");
                    let base = url
                        .trim_end_matches('/')
                        .trim_end_matches("/v1/chat/completions")
                        .to_string();
                    if wait_for_llama_model_ready(
                        &client,
                        &base,
                        &api_key,
                        std::time::Duration::from_secs(60),
                    )
                    .await
                    {
                        let mut rb = client.post(&url).json(&req_body);
                        if let Some(ref key) = api_key {
                            rb = rb.bearer_auth(key);
                        }
                        result = rb.send().await;
                    } else {
                        let _ = tx
                            .send(Err(LlmError::Generation(
                                "llama.cpp API error 503: model still loading after 60s"
                                    .to_string(),
                            )))
                            .await;
                        return;
                    }
                } else {
                    let _ = tx
                        .send(Err(LlmError::Generation(format!(
                            "llama.cpp API error {}: {}",
                            status_now.as_u16(),
                            body_text
                        ))))
                        .await;
                    return;
                }
            }

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
                    // Repairs think markers the server misrouted into the
                    // content channel (see think_filter.rs); spans the whole
                    // response, flushed at [DONE] and again as a backstop.
                    // Grace hold only when this request disabled thinking.
                    let mut content_filter = ThinkContentFilter::with_grace(content_grace);
                    // Accumulate tool calls across chunks
                    let mut accumulated_tool_calls: std::collections::HashMap<
                        u32,
                        AccumulatedToolCall,
                    > = std::collections::HashMap::new();

                    while let Some(chunk_result) =
                        super::next_bytes_or_end(&mut stream, read_idle_timeout).await
                    {
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
                                        // Flush think-marker repair still held back
                                        let (repaired, visible) = content_filter.finish();
                                        if !repaired.is_empty() {
                                            let _ = tx.send(Ok((repaired, true))).await;
                                        }
                                        if !visible.is_empty() {
                                            let _ = tx.send(Ok((visible, false))).await;
                                        }
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
                                            // Emit usage even when 0/MAX so End can
                                            // distinguish invalid meters from missing usage.
                                            if let Some(ref usage) = evt.usage {
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

                                            if let Some(choice) = evt.choices.first() {
                                                // Handle content
                                                if let Some(ref content) = choice.delta.content {
                                                    if !content.is_empty() {
                                                        let (repaired, visible) =
                                                            content_filter.feed(content);
                                                        if !repaired.is_empty() {
                                                            let _ =
                                                                tx.send(Ok((repaired, true))).await;
                                                        }
                                                        if !visible.is_empty() {
                                                            let _ =
                                                                tx.send(Ok((visible, false))).await;
                                                        }
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

                    // Backstop for streams that end without [DONE]: flush
                    // any think-marker repair still held back and log the
                    // repair once per response (idempotent after [DONE]).
                    let (repaired, visible) = content_filter.finish();
                    if !repaired.is_empty() {
                        let _ = tx.send(Ok((repaired, true))).await;
                    }
                    if !visible.is_empty() {
                        let _ = tx.send(Ok((visible, false))).await;
                    }
                    log_think_filter_stats(&content_filter);
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
        let (supports_multimodal, supports_function_calling, supports_thinking, max_context) =
            if let Some(ref caps) = self.capabilities_override {
                (
                    caps.supports_multimodal,
                    caps.supports_tools,
                    caps.supports_thinking,
                    caps.max_context,
                )
            } else {
                // Default: llama.cpp supports streaming and tools via --jinja flag
                (false, true, true, 4096)
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
            // Discovery does not yet advertise template-specific toggles.
            // Explicit per-request overrides are forwarded for hybrid templates;
            // do not promise every model can disable mandatory reasoning.
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
    fn test_llamacpp_thinking_unspecified_preserves_server_default() {
        let mut body = serde_json::json!({"cache_prompt": true});
        apply_thinking_override(&mut body, &Default::default());
        assert_eq!(body, serde_json::json!({"cache_prompt": true}));
    }

    #[test]
    fn test_llamacpp_thinking_effort_precedes_legacy_boolean() {
        use neomind_core::llm::backend::GenerationParams;
        for (legacy, effort, expected) in [
            (Some(false), None, false),
            (Some(true), None, true),
            (Some(true), Some(ThinkingEffort::None), false),
            (Some(false), Some(ThinkingEffort::High), true),
        ] {
            let params = GenerationParams {
                thinking_enabled: legacy,
                thinking_effort: effort,
                ..Default::default()
            };
            let mut body = serde_json::json!({"chat_template_kwargs": {"other": 1}});
            apply_thinking_override(&mut body, &params);
            assert_eq!(body["chat_template_kwargs"]["enable_thinking"], expected);
            assert_eq!(body["chat_template_kwargs"]["other"], 1);
        }
    }

    #[tokio::test]
    async fn test_llamacpp_thinking_reaches_http_in_stream_and_nonstream() {
        use futures::StreamExt;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for streaming in [false, true] {
            for enabled in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let server = std::thread::spawn(move || {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        socket.read_exact(&mut byte).unwrap();
                        headers.push(byte[0]);
                        assert!(headers.len() < 8192);
                    }
                    let headers = String::from_utf8(headers).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    let mut bytes = vec![0; length];
                    socket.read_exact(&mut bytes).unwrap();
                    let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let (mime, response) = if streaming {
                        ("text/event-stream", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
                    } else {
                        ("application/json", "{\"choices\":[{\"message\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}")
                    };
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                    request
                });
                let runtime = LlamaCppRuntime::new(
                    LlamaCppConfig::new("renamed-hybrid-model")
                        .with_endpoint(format!("http://{address}"))
                        .with_timeout_secs(5),
                )
                .unwrap();
                let mut input = neomind_core::llm::backend::LlmInput::new("hello");
                input.params.thinking_enabled = Some(enabled);
                if streaming {
                    let chunks = runtime
                        .generate_stream(input)
                        .await
                        .unwrap()
                        .collect::<Vec<_>>()
                        .await;
                    assert!(!chunks.is_empty());
                    assert!(chunks.iter().all(|chunk| chunk.is_ok()));
                    assert_eq!(
                        chunks
                            .into_iter()
                            .map(|chunk| chunk.unwrap().0)
                            .collect::<String>(),
                        "ok"
                    );
                } else {
                    runtime.generate(input).await.unwrap();
                }
                let request = server.join().unwrap();
                assert_eq!(request["stream"], streaming);
                assert_eq!(request["chat_template_kwargs"]["enable_thinking"], enabled);
            }
        }
    }

    /// Mock SSE/JSON server for the think-filter wiring tests: accepts one
    /// request, replies with `response`, returns the parsed request body.
    fn spawn_once_server(
        response: String,
    ) -> (
        std::net::SocketAddr,
        std::thread::JoinHandle<serde_json::Value>,
    ) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
                assert!(headers.len() < 8192);
            }
            let headers = String::from_utf8(headers).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut bytes = vec![0; length];
            socket.read_exact(&mut bytes).unwrap();
            let response = response.clone();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        });
        (address, server)
    }

    #[tokio::test]
    async fn test_llamacpp_stream_repairs_stray_think_closer_into_thinking_channel() {
        use futures::StreamExt;
        // The stray closer is split across two content deltas to prove the
        // chunk-boundary hold-back works end to end. Thinking DISABLED (the
        // leak mode) reclassifies the opening run into the thinking channel;
        // thinking ON streams without the grace hold, so the marker is
        // stripped but the already-streamed prefix stays visible.
        for (thinking, want_thinking, want_content) in [
            (
                false,
                "Plan first: check the list.</think>",
                "The answer is 42.",
            ),
            (true, "", "Plan first: check the list.The answer is 42."),
        ] {
            let sse = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Plan first: check the list.\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"</th\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"ink>The answer is 42.\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            )
            .to_string();
            let (address, _server) = spawn_once_server(sse);
            let runtime = LlamaCppRuntime::new(
                LlamaCppConfig::new("spark-test").with_endpoint(format!("http://{address}")),
            )
            .unwrap();
            let mut input = neomind_core::llm::backend::LlmInput::new("hello");
            input.params.thinking_enabled = Some(thinking);
            let chunks = runtime
                .generate_stream(input)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await;
            assert!(chunks.iter().all(|chunk| chunk.is_ok()));
            let thinking_out: String = chunks
                .iter()
                .filter(|chunk| chunk.as_ref().unwrap().1)
                .map(|chunk| chunk.as_ref().unwrap().0.clone())
                .collect();
            let content: String = chunks
                .iter()
                .filter(|chunk| !chunk.as_ref().unwrap().1)
                .map(|chunk| chunk.as_ref().unwrap().0.clone())
                .collect();
            assert_eq!(thinking_out, want_thinking);
            assert_eq!(content, want_content);
            assert!(!content.contains("</think>"));
        }
    }

    #[tokio::test]
    async fn test_llamacpp_nonstream_repairs_stray_think_closer() {
        let body = "{\"choices\":[{\"message\":{\"content\":\"Plan</think>Answer\",\"reasoning_content\":\"server-reasoning\"},\"finish_reason\":\"stop\"}]}".to_string();
        let (address, _server) = spawn_once_server(body);
        let runtime = LlamaCppRuntime::new(
            LlamaCppConfig::new("spark-test").with_endpoint(format!("http://{address}")),
        )
        .unwrap();
        let output = runtime
            .generate(neomind_core::llm::backend::LlmInput::new("hello"))
            .await
            .unwrap();
        assert_eq!(output.text, "Answer");
        assert_eq!(
            output.thinking.as_deref(),
            Some("server-reasoningPlan</think>")
        );
    }

    /// Paced mock SSE server: accepts one request, then performs the given
    /// `(delay_ms, raw bytes)` writes in order, ignoring client disconnects.
    /// The body is close-delimited (no Content-Length) so each write reaches
    /// the client immediately — assertions below therefore measure NeoMind's
    /// own emission timing, not the mock's.
    fn spawn_paced_server(writes: Vec<(u64, String)>) -> std::net::SocketAddr {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => return,
            };
            let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
            let mut headers = Vec::new();
            loop {
                let mut byte = [0];
                if socket.read_exact(&mut byte).is_err() {
                    return;
                }
                headers.push(byte[0]);
                if headers.ends_with(b"\r\n\r\n") {
                    break;
                }
                if headers.len() > 8192 {
                    return;
                }
            }
            let header_str = String::from_utf8_lossy(&headers);
            let length = header_str
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if length > 0 {
                let mut bytes = vec![0; length];
                if socket.read_exact(&mut bytes).is_err() {
                    return;
                }
            }
            let _ = write!(
                socket,
                "HTTP/1.0 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            );
            let _ = socket.flush();
            for (delay_ms, data) in writes {
                std::thread::sleep(Duration::from_millis(delay_ms));
                if socket.write_all(data.as_bytes()).is_err() {
                    return;
                }
                let _ = socket.flush();
            }
        });
        address
    }

    fn paced_input(thinking: Option<bool>) -> neomind_core::llm::backend::LlmInput {
        let mut input = neomind_core::llm::backend::LlmInput::new("hello");
        input.params.thinking_enabled = thinking;
        input
    }

    /// Thinking ON: a short marker-free reply must stream progressively —
    /// the first chunk leaves while the server is still mid-stream, not
    /// buffered until completion.
    #[tokio::test]
    async fn thinking_on_short_reply_streams_before_completion() {
        use futures::StreamExt;
        let address = spawn_paced_server(vec![
            (0, "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string()),
            (
                3000,
                "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n"
                    .to_string(),
            ),
            (0, "data: [DONE]\n\n".to_string()),
        ]);
        let runtime = LlamaCppRuntime::new(
            LlamaCppConfig::new("spark-test").with_endpoint(format!("http://{address}")),
        )
        .unwrap();
        let mut stream = runtime
            .generate_stream(paced_input(Some(true)))
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_millis(1500), stream.next())
            .await
            .expect("thinking-on first chunk must not wait for stream end")
            .expect("stream open")
            .expect("chunk ok");
        assert_eq!(first, ("Hello".to_string(), false));
    }

    /// Thinking OFF: the grace hold is active, so a short reply stays held
    /// until the stream completes — no chunk may arrive mid-generation.
    #[tokio::test]
    async fn thinking_off_short_reply_holds_until_completion() {
        use futures::StreamExt;
        let address = spawn_paced_server(vec![
            (
                0,
                "data: {\"choices\":[{\"delta\":{\"content\":\"short answer\"}}]}\n\n".to_string(),
            ),
            (
                3000,
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                    .to_string(),
            ),
        ]);
        let runtime = LlamaCppRuntime::new(
            LlamaCppConfig::new("spark-test").with_endpoint(format!("http://{address}")),
        )
        .unwrap();
        let mut stream = runtime
            .generate_stream(paced_input(Some(false)))
            .await
            .unwrap();
        let none = tokio::time::timeout(Duration::from_millis(1500), stream.next()).await;
        assert!(
            none.is_err(),
            "grace hold must keep a short thinking-off reply invisible mid-generation"
        );
    }

    /// Thinking OFF with a reply longer than the grace budget: content is
    /// released as soon as the budget is exceeded — before completion.
    #[tokio::test]
    async fn thinking_off_long_reply_releases_after_grace_budget() {
        use futures::StreamExt;
        let long: String = "a".repeat(GRACE_CHARS + 400);
        let first_write = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
            long
        );
        let address = spawn_paced_server(vec![
            (0, first_write),
            (
                3000,
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                    .to_string(),
            ),
        ]);
        let runtime = LlamaCppRuntime::new(
            LlamaCppConfig::new("spark-test").with_endpoint(format!("http://{address}")),
        )
        .unwrap();
        let mut stream = runtime
            .generate_stream(paced_input(Some(false)))
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_millis(1500), stream.next())
            .await
            .expect("long reply must release after the grace budget, not at completion")
            .expect("stream open")
            .expect("chunk ok");
        assert!(!first.1);
        assert!(first.0.starts_with("aaa"));
    }

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
    fn per_slot_context_divides_total_kv_pool() {
        // Auto-slotted server: 128K total pool, 4 slots → 32K per request.
        assert_eq!(per_slot_context(131_072, Some(4)), 32_768);
        assert_eq!(per_slot_context(65_536, Some(2)), 32_768);
        // Odd division floors; a degenerate 0-slot report is clamped to 1.
        assert_eq!(per_slot_context(8_192, Some(3)), 2_730);
        assert_eq!(per_slot_context(8_192, Some(0)), 8_192);
        // Older servers without total_slots report: single-slot, full pool.
        assert_eq!(per_slot_context(131_072, None), 131_072);
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
            .with_capabilities_override(true, true, true, 32768);
        let caps = runtime.capabilities();
        assert!(caps.streaming);
        assert!(caps.multimodal);
        assert!(caps.function_calling);
        assert!(caps.thinking_display);
        assert_eq!(caps.max_context, Some(32768));
    }
}

#[cfg(test)]
mod per_slot_context_boundary_tests {
    use super::per_slot_context;

    #[test]
    fn missing_slot_report_is_single_slot() {
        // Older llama-server builds don't report total_slots → full window.
        assert_eq!(per_slot_context(32_768, None), 32_768);
    }

    #[test]
    fn zero_slot_report_degenerates_to_single_slot() {
        // A bogus 0 must not divide by zero — max(1) clamps it.
        assert_eq!(per_slot_context(32_768, Some(0)), 32_768);
    }

    #[test]
    fn four_slots_divide_the_pool() {
        assert_eq!(per_slot_context(32_768, Some(4)), 8_192);
        assert_eq!(per_slot_context(131_072, Some(4)), 32_768);
    }

    #[test]
    fn non_dividing_pool_floors() {
        // Integer division floors — 32769/2 = 16384 (conservative, never over).
        assert_eq!(per_slot_context(32_769, Some(2)), 16_384);
    }
}
