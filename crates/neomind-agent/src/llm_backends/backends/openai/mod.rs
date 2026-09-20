//! OpenAI-compatible cloud LLM backend implementation.
//!
//! Supports cloud APIs that are compatible with OpenAI's format:
//! - OpenAI (GPT-4, GPT-3.5, o1, etc.)
//! - Anthropic Claude (native Messages API)
//! - Google Gemini (via compatibility layer)
//! - xAI Grok
//! - Other OpenAI-compatible providers

use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, LlmError, LlmOutput, LlmRuntime,
    ReasoningCapabilities, ReasoningControl, StreamChunk, ThinkingEffort,
};
use neomind_core::message::ImageDetail;

use super::super::rate_limited_client::RateLimitedClient;

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
    /// Get the base URL for this provider. Only used when the backend carries
    /// no explicit endpoint — every other default in the codebase points GLM
    /// at the public paas endpoint, so the earlier coding-endpoint default
    /// here silently routed endpoint-less GLM instances elsewhere.
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

    /// Get the default model for this provider. Effectively unreachable in
    /// production (every construction path sets a model) — kept current with
    /// the fresh-model list so it can't resurface as a stale default.
    fn default_model(&self) -> &str {
        match self {
            Self::OpenAI => "gpt-4.1-mini",
            Self::Anthropic => "claude-sonnet-4-5",
            Self::Google => "gemini-2.5-flash",
            Self::Grok => "grok-3-mini",
            Self::Custom => "unknown",
            Self::Qwen => "qwen-plus",
            Self::DeepSeek => "deepseek-chat",
            Self::GLM => "glm-4.5-flash",
            Self::MiniMax => "MiniMax-M2",
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

/// Cloud LLM runtime backend.
pub struct CloudRuntime {
    pub(crate) config: CloudConfig,
    pub(crate) client: RateLimitedClient,
    pub(crate) model: String,
    pub(crate) metrics: Arc<RwLock<BackendMetrics>>,
    /// Optional override for capabilities (from storage/API detection)
    /// If None, capabilities are detected from model name heuristics
    pub(crate) capabilities_override: Option<CloudCapabilities>,
}

/// Capabilities override for cloud runtime.
#[derive(Debug, Clone)]
pub(crate) struct CloudCapabilities {
    supports_multimodal: bool,
    supports_thinking: bool,
    supports_tools: bool,
    max_context: usize,
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
        // Explicit per-endpoint override wins (CloudConfig field / stored
        // instance settings) — provider tables are guesses.
        if let Some(max) = self.config.max_context {
            return max;
        }
        match self.config.provider {
            CloudProvider::OpenAI => 128000,
            CloudProvider::Anthropic => 200000,
            CloudProvider::Google => 1000000,
            CloudProvider::Grok => 128000,
            CloudProvider::Qwen => 128000,
            CloudProvider::DeepSeek => 128000,
            CloudProvider::GLM => 128000,
            CloudProvider::MiniMax => 512000,
            // 32k floor: virtually every model served behind a custom
            // OpenAI-compatible endpoint today is >=32k, and under-guessing
            // truncates conversation history to nothing (the Custom=4096 bug
            // made chat memory silently dead). A too-large guess degrades
            // gracefully — overflow is rescued by the compact-retry ladder.
            CloudProvider::Custom => 32768,
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
        let (supports_multimodal, supports_function_calling, supports_thinking, max_context) =
            if let Some(ref caps) = self.capabilities_override {
                (
                    caps.supports_multimodal,
                    caps.supports_tools,
                    caps.supports_thinking,
                    caps.max_context,
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
                (
                    supports_multimodal,
                    supports_function_calling,
                    false, // thinking not detected by name
                    self.max_context_length(),
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
            // param_provider: the persisted reasoning control must match what
            // requests actually do — an openai-typed DashScope/DeepSeek
            // endpoint is Boolean-controlled, not effort-leveled.
            reasoning: reasoning_capabilities_for(self.param_provider(), supports_thinking),
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

/// Declare the reasoning/thinking capabilities for an OpenAI-compatible
/// cloud provider, based on what `build_chat_request` actually emits:
/// - OpenAI/Custom/GLM/Google → `reasoning_effort` (discrete levels, incl. none)
/// - DeepSeek → `thinking: {enabled|disabled}` + `reasoning_effort`
/// - Qwen → `enable_thinking: bool`
/// - Anthropic → `thinking: {enabled|disabled}` (native /messages path)
/// - MiniMax/Grok → no request-side control (read-only)
fn reasoning_capabilities_for(
    provider: CloudProvider,
    supports_thinking: bool,
) -> ReasoningCapabilities {
    use ReasoningControl::{Boolean, Effort, ReadOnly};
    let control = match provider {
        CloudProvider::OpenAI
        | CloudProvider::Custom
        | CloudProvider::GLM
        | CloudProvider::Google => Effort,
        CloudProvider::DeepSeek | CloudProvider::Anthropic | CloudProvider::Qwen => Boolean,
        CloudProvider::MiniMax | CloudProvider::Grok => ReadOnly,
    };
    let supported_efforts = if control != ReadOnly && supports_thinking {
        match control {
            Effort => vec![
                ThinkingEffort::None,
                ThinkingEffort::Low,
                ThinkingEffort::Medium,
                ThinkingEffort::High,
                ThinkingEffort::XHigh,
                ThinkingEffort::Max,
            ],
            _ => vec![ThinkingEffort::None, ThinkingEffort::High],
        }
    } else {
        Vec::new()
    };
    ReasoningCapabilities {
        supported_efforts,
        default_effort: if supports_thinking {
            Some(ThinkingEffort::High)
        } else {
            None
        },
        mandatory: false,
        control,
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

mod config;
mod protocol;
mod runtime;
mod vision;
pub use config::*;
pub(crate) use protocol::*;
pub(crate) use vision::*;

#[cfg(test)]
mod tests;
