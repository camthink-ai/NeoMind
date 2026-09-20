//! `runtime` — split from the former openai.rs monolith.

use super::*;

use std::pin::Pin;

use std::sync::{Arc, RwLock};

use std::time::{Duration, Instant};

use futures::{Stream, StreamExt};

use reqwest::Client;

use neomind_core::llm::backend::{
    BackendMetrics, FinishReason, LlmError, LlmInput, LlmOutput, LlmRuntime, StreamChunk,
    ThinkingEffort, TokenUsage,
};

use neomind_core::message::{Content, ContentPart, ImageDetail, Message, MessageRole};

use crate::llm_backends::rate_limited_client::{ProviderRateLimits, RateLimitedClient};

use crate::llm_backends::text_tool_calls;

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
    ) -> Self {
        self.capabilities_override = Some(CloudCapabilities {
            supports_multimodal,
            supports_thinking,
            supports_tools,
            max_context,
        });
        self
    }

    /// Convert messages to API format (provider-specific).
    /// For Anthropic, uses their image format. For OpenAI/Google, uses OpenAI-style format.
    pub(crate) fn messages_to_api(&self, messages: &[Message]) -> Vec<ApiMessage> {
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
    pub(crate) fn build_anthropic_request(
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
            // Unified effort → Anthropic thinking. Explicit disable sends
            // `{type:"disabled"}`; any enable sends `{type:"enabled", budget}`.
            // Omitted → model default (adaptive thinking on modern Claude).
            thinking: match input.params.thinking_effort {
                Some(ThinkingEffort::None) => Some(AnthropicThinking::Disabled),
                Some(_) => Some(AnthropicThinking::Enabled {
                    // Rough budget: ~32K is safe headroom for thinking + answer.
                    budget_tokens: MAX_TOKENS_CAP.min(32000),
                }),
                None => input.params.thinking_enabled.map(|enabled| {
                    if enabled {
                        AnthropicThinking::Enabled {
                            budget_tokens: MAX_TOKENS_CAP.min(32000),
                        }
                    } else {
                        AnthropicThinking::Disabled
                    }
                }),
            },
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
    /// Declared provider refined by endpoint sniffing — the param-level view
    /// of where these requests actually go. Protocol-first Cloud AI (and
    /// `--type openai` + vendor endpoint on the CLI) reaches DashScope /
    /// DeepSeek as plain OpenAI-compatible backends, which would otherwise
    /// lose the vendor-specific param wiring (enable_thinking for DashScope
    /// hybrid models, the thinking on/off toggle for DeepSeek) and emit
    /// reasoning_effort where the vendor doesn't accept it. Sniff the base
    /// URL so the gates follow the endpoint regardless of how the backend
    /// was typed; native vendor types pass through unchanged.
    pub(crate) fn param_provider(&self) -> CloudProvider {
        if matches!(
            self.config.provider,
            CloudProvider::OpenAI | CloudProvider::Custom
        ) {
            let base = self.config.get_base_url();
            // Both DashScope regions: cn (dashscope.aliyuncs.com) and the
            // international site (dashscope-intl.aliyuncs.com) — the intl
            // host doesn't contain the cn substring.
            if base.contains("dashscope.aliyuncs.com")
                || base.contains("dashscope-intl.aliyuncs.com")
            {
                return CloudProvider::Qwen;
            }
            if base.contains("api.deepseek.com") {
                return CloudProvider::DeepSeek;
            }
        }
        self.config.provider
    }

    pub(crate) fn build_chat_request(
        &self,
        input: LlmInput,
        stream: bool,
    ) -> ChatCompletionRequest {
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
        // `param_provider()` also catches DashScope reached via --type openai
        // (protocol-first Cloud AI).
        //
        // Unified effort takes precedence: `None` → disable, any other → enable.
        let enable_thinking = if matches!(self.param_provider(), CloudProvider::Qwen) {
            input
                .params
                .thinking_effort
                .map(|e| !e.is_disabled())
                .or(input.params.thinking_enabled)
        } else {
            None
        };

        // OpenAI/GPT-5-style reasoning effort. Maps the unified effort enum to
        // the `reasoning_effort` string the OpenAI-compatible endpoints accept.
        // Emitted only for providers that accept it (OpenAI, Custom, GLM);
        // others reject unknown fields. Gemini via OpenAI-compat also accepts it.
        let reasoning_effort = if matches!(
            self.param_provider(),
            CloudProvider::OpenAI
                | CloudProvider::Custom
                | CloudProvider::GLM
                | CloudProvider::Google
        ) {
            input.params.thinking_effort.map(|e| e.as_str().to_string())
        } else {
            None
        };

        // DeepSeek thinking-mode toggle. DeepSeek defaults thinking ON at
        // `high` effort; an explicit `{"type":"disabled"}` is required to turn
        // it off. Only emitted for DeepSeek (`param_provider()` also catches
        // DeepSeek reached via --type openai).
        let thinking = if matches!(self.param_provider(), CloudProvider::DeepSeek) {
            // Effort takes precedence; otherwise honor thinking_enabled
            // (Some(false) from analyzer.rs / intent.rs — gotcha #7), and
            // default to enabled when unset (DeepSeek's own default).
            let disabled = input
                .params
                .thinking_effort
                .map(|e| e.is_disabled())
                .unwrap_or_else(|| !input.params.thinking_enabled.unwrap_or(true));
            Some(Thinking {
                thinking_type: if disabled { "disabled" } else { "enabled" }.to_string(),
            })
        } else {
            None
        };

        // Text tool-calling fallback (shared with Ollama / llama.cpp —
        // `llm_backends::text_tool_calls`): when the effective capability says
        // the model has no native function calling — `CloudProvider::Custom`
        // defaults to false in the `capabilities()` heuristic, as does any
        // stored override that turned tools off — teach the JSON protocol in
        // the system message so the agent-layer `tool_parser` can act on the
        // reply. Without this, custom OpenAI-compatible endpoints carried the
        // `tools` schema but the model was never taught how to answer, and
        // every tool-aware turn degraded to plain prose. Computed before
        // `input.tools` is moved into the request below. Native providers
        // produce byte-identical messages.
        let messages = text_tool_calls::prepare_messages(
            input.messages,
            input.tools.as_deref(),
            self.capabilities().function_calling,
        );

        let request = ChatCompletionRequest {
            model,
            messages: self.messages_to_api(&messages),
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
            reasoning_effort,
            thinking,
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
    pub(crate) fn parse_xml_tool_calls(content: &str) -> Vec<serde_json::Value> {
        static BLOCK_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?s)<tool_call>\s*(.*?)\s*</tool_call>").unwrap()
        });
        static FUNC_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?s)<function=([\w-]+)>(.*?)</function>").unwrap()
        });
        static PARAM_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?s)<parameter=([\w-]+)>(.*?)</parameter>").unwrap()
        });
        let mut out = Vec::new();
        for b in BLOCK_RE.captures_iter(content) {
            let inner = b.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(fc) = FUNC_RE.captures(inner) {
                let name = fc
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let body = fc.get(2).map(|m| m.as_str()).unwrap_or("");
                let mut args = serde_json::Map::new();
                for pc in PARAM_RE.captures_iter(body) {
                    let k = pc
                        .get(1)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    let v = pc
                        .get(2)
                        .map(|m| m.as_str().trim().to_string())
                        .unwrap_or_default();
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
    pub(crate) async fn generate_openai(
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
        let native_tool_calls =
            if native_tool_calls.is_none() && response_text.contains("<tool_call>") {
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
    pub(crate) async fn generate_anthropic(
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
                // Thinking blocks are model reasoning, not visible output.
                AnthropicContentBlock::Thinking { .. }
                | AnthropicContentBlock::RedactedThinking { .. }
                | AnthropicContentBlock::Unknown => {}
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
    pub(crate) fn generate_stream_openai(
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

            // Bound only the wait-for-HEADERS, not the whole request: a
            // single-slot backend may queue a request and never send headers,
            // but a long healthy generation must not be killed by a
            // request-wide budget. 30s to receive headers, then the
            // read-side idle timeout governs the body.
            let send_fut = inner_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&request)
                .send();
            let send_result =
                tokio::time::timeout(std::time::Duration::from_secs(30), send_fut).await;
            let result: Result<_, LlmError> = match send_result {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(e)) => Err(LlmError::Network(e.to_string())),
                Err(_elapsed) => Err(LlmError::Generation(
                    "Timed out waiting for streaming response headers".to_string(),
                )),
            };

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
                    while let Some(chunk_result) =
                        tokio::time::timeout(read_idle_timeout, stream.next())
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
    pub(crate) fn generate_stream_anthropic(
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

            // Same as the openai streaming path: bound only the header wait
            // (30s) with tokio::time, not the whole request (which would kill
            // long healthy generations).
            let send_fut = inner_client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request)
                .send();
            let send_result =
                tokio::time::timeout(std::time::Duration::from_secs(30), send_fut).await;
            let result: Result<_, LlmError> = match send_result {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(e)) => Err(LlmError::Network(e.to_string())),
                Err(_elapsed) => Err(LlmError::Generation(
                    "Timed out waiting for streaming response headers".to_string(),
                )),
            };

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

                    while let Some(chunk_result) =
                        tokio::time::timeout(read_idle_timeout, stream.next())
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
