//! `config` — split from the former openai.rs monolith.

use super::*;

use std::time::Duration;

use serde::{Deserialize, Serialize};

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

    /// Context window override. `None` = provider table default. Custom
    /// endpoints MUST be able to declare this — guessing too small (the old
    /// hardcoded 4096) collapses the history budget to zero and silently
    /// kills cross-turn memory for every model behind the endpoint.
    #[serde(default)]
    pub max_context: Option<usize>,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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
            max_context: None,
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

    /// Override the context window used for history budgeting. Set this for
    /// custom endpoints — the provider table cannot know what a proxy/vLLM
    /// deployment actually serves.
    pub fn with_max_context(mut self, max_context: usize) -> Self {
        self.max_context = Some(max_context);
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
    pub(crate) fn get_base_url(&self) -> String {
        let base = if let Some(base) = &self.base_url {
            base.clone()
        } else {
            self.provider.base_url().to_string()
        };
        // Anthropic path: requests join base + "/messages". The ecosystem
        // convention (Anthropic SDK / Claude Code) is a base WITHOUT /v1
        // that the client expands to /v1/messages — accept both forms so
        // users can paste either (also covers Anthropic-compatible entries
        // like GLM's open.bigmodel.cn/api/anthropic).
        if matches!(self.provider, CloudProvider::Anthropic) {
            let trimmed = base.trim_end_matches('/');
            if trimmed.ends_with("/v1") {
                trimmed.to_string()
            } else {
                format!("{}/v1", trimmed)
            }
        } else {
            base
        }
    }

    /// Get the effective model name.
    pub(crate) fn get_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| self.provider.default_model().to_string())
    }
}
