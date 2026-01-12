//! LLM backend factory implementations.
//!
//! This module provides concrete implementations of BackendFactory
//! for various LLM backends, enabling dynamic backend registration.

use async_trait::async_trait;
use edge_ai_core::llm::backend::{
    BackendCapabilities, BackendFactory, BackendId, LlmError, LlmRuntime,
};
use serde_json::Value;

// Re-export backend configs for factory use
#[cfg(feature = "ollama")]
pub use crate::backends::ollama::{OllamaConfig, OllamaRuntime};

#[cfg(feature = "cloud")]
pub use crate::backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

/// Factory for Ollama backend (local LLM runner).
#[cfg(feature = "ollama")]
pub struct OllamaFactory;

#[cfg(feature = "ollama")]
impl OllamaFactory {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "ollama")]
#[async_trait]
impl BackendFactory for OllamaFactory {
    fn backend_id(&self) -> &str {
        BackendId::OLLAMA
    }

    fn display_name(&self) -> &str {
        "Ollama (Local LLM)"
    }

    fn create(&self, config: &Value) -> Result<Box<dyn LlmRuntime>, LlmError> {
        let model = config
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("qwen3-vl:2b")
            .to_string();

        let endpoint = config.get("endpoint").and_then(|v| v.as_str());

        let mut ollama_config = OllamaConfig::new(model);
        if let Some(ep) = endpoint {
            ollama_config = ollama_config.with_endpoint(ep);
        }

        let runtime = OllamaRuntime::new(ollama_config)?;
        Ok(Box::new(runtime))
    }

    fn validate_config(&self, config: &Value) -> Result<(), LlmError> {
        if let Some(endpoint) = config.get("endpoint").and_then(|v| v.as_str()) {
            if endpoint.is_empty() {
                return Err(LlmError::InvalidInput("endpoint cannot be empty".into()));
            }
        }
        Ok(())
    }

    fn default_config(&self) -> Value {
        serde_json::json!({
            "backend": "ollama",
            "model": "qwen3-vl:2b",
            "endpoint": "http://localhost:11434"
        })
    }

    async fn is_available(&self) -> bool {
        // Check if Ollama is responding
        #[cfg(feature = "ollama")]
        {
            if let Ok(client) = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
            {
                if let Ok(resp) = client.get("http://localhost:11434/api/tags").send().await {
                    return resp.status().is_success();
                }
            }
        }
        false
    }
}

#[cfg(feature = "ollama")]
impl Default for OllamaFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory for OpenAI-compatible cloud backends.
#[cfg(feature = "cloud")]
pub struct CloudFactory;

#[cfg(feature = "cloud")]
impl CloudFactory {
    pub fn new() -> Self {
        Self
    }

    fn parse_provider(s: &str) -> Result<CloudProvider, LlmError> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(CloudProvider::OpenAI),
            "anthropic" | "claude" => Ok(CloudProvider::Anthropic),
            "google" | "gemini" => Ok(CloudProvider::Google),
            "grok" | "xai" => Ok(CloudProvider::Grok),
            "custom" => Ok(CloudProvider::Custom),
            _ => Err(LlmError::InvalidInput(format!("Unknown provider: {}", s))),
        }
    }
}

#[cfg(feature = "cloud")]
#[async_trait]
impl BackendFactory for CloudFactory {
    fn backend_id(&self) -> &str {
        BackendId::OPENAI
    }

    fn display_name(&self) -> &str {
        "Cloud (OpenAI/Anthropic/Google/Grok)"
    }

    fn create(&self, config: &Value) -> Result<Box<dyn LlmRuntime>, LlmError> {
        let provider_str = config
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("openai");

        let provider = Self::parse_provider(provider_str)?;

        let api_key = config
            .get("api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::InvalidInput("api_key is required".into()))?
            .to_string();

        let model = config.get("model").and_then(|v| v.as_str()).map(String::from);
        let endpoint = config.get("endpoint").and_then(|v| v.as_str());

        let mut cloud_config = match provider {
            CloudProvider::OpenAI => CloudConfig::openai(api_key),
            CloudProvider::Anthropic => CloudConfig::anthropic(api_key),
            CloudProvider::Google => CloudConfig::google(api_key),
            CloudProvider::Grok => CloudConfig::grok(api_key),
            CloudProvider::Custom => {
                let url = endpoint.unwrap_or("https://api.example.com/v1");
                CloudConfig::custom(api_key, url)
            }
        };

        if let Some(m) = model {
            cloud_config = cloud_config.with_model(m);
        }

        let runtime = CloudRuntime::new(cloud_config)?;
        Ok(Box::new(runtime))
    }

    fn validate_config(&self, config: &Value) -> Result<(), LlmError> {
        if let Some(provider) = config.get("provider").and_then(|v| v.as_str()) {
            Self::parse_provider(provider)?;
        }

        let api_key = config.get("api_key").and_then(|v| v.as_str());
        if api_key.map_or(true, |k| k.is_empty()) {
            return Err(LlmError::InvalidInput("api_key is required for cloud backends".into()));
        }

        Ok(())
    }

    fn default_config(&self) -> Value {
        serde_json::json!({
            "backend": "openai",
            "provider": "openai",
            "model": "gpt-4o-mini"
        })
    }
}

#[cfg(feature = "cloud")]
impl Default for CloudFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory for Mock backend (useful for testing).
pub struct MockFactory;

impl MockFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BackendFactory for MockFactory {
    fn backend_id(&self) -> &str {
        BackendId::MOCK
    }

    fn display_name(&self) -> &str {
        "Mock (Testing)"
    }

    fn create(&self, _config: &Value) -> Result<Box<dyn LlmRuntime>, LlmError> {
        Ok(Box::new(MockRuntime::new()))
    }

    fn validate_config(&self, _config: &Value) -> Result<(), LlmError> {
        Ok(())
    }

    fn default_config(&self) -> Value {
        serde_json::json!({
            "backend": "mock"
        })
    }
}

impl Default for MockFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock LLM runtime for testing.
pub struct MockRuntime {
    model: String,
}

impl MockRuntime {
    pub fn new() -> Self {
        Self {
            model: "mock-model".to_string(),
        }
    }

    pub fn with_model(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

impl Default for MockRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LlmRuntime for MockRuntime {
    fn backend_id(&self) -> edge_ai_core::llm::backend::BackendId {
        edge_ai_core::llm::backend::BackendId::new(edge_ai_core::llm::backend::BackendId::MOCK)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn generate(
        &self,
        input: edge_ai_core::llm::backend::LlmInput,
    ) -> Result<edge_ai_core::llm::backend::LlmOutput, LlmError> {
        let last_msg = input.messages.last().map(|m| m.text()).unwrap_or_else(|| String::new());
        Ok(edge_ai_core::llm::backend::LlmOutput {
            text: format!("Mock response to: {}", last_msg),
            finish_reason: edge_ai_core::llm::backend::FinishReason::Stop,
            usage: Some(edge_ai_core::llm::backend::TokenUsage::new(10, 20)),
        })
    }

    async fn generate_stream(
        &self,
        input: edge_ai_core::llm::backend::LlmInput,
    ) -> Result<
        std::pin::Pin<Box<dyn futures::Stream<Item = edge_ai_core::llm::backend::StreamChunk> + Send>>,
        LlmError,
    > {
        use futures::stream;
        let last_msg = input.messages.last().map(|m| m.text()).unwrap_or_else(|| String::new());
        let response = format!("Mock stream response to: {}", last_msg);
        let chunks: Vec<_> = response
            .chars()
            .map(|c| Ok((c.to_string(), false)))
            .collect();
        Ok(Box::pin(stream::iter(chunks)))
    }

    fn max_context_length(&self) -> usize {
        4096
    }

    fn supports_multimodal(&self) -> bool {
        true
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::builder()
            .streaming()
            .multimodal()
            .function_calling()
            .thinking_display()
            .max_context(4096)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_factory() {
        let factory = OllamaFactory::new();
        assert_eq!(factory.backend_id(), "ollama");
        assert_eq!(factory.display_name(), "Ollama (Local LLM)");
    }

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_default_config() {
        let factory = OllamaFactory::new();
        let config = factory.default_config();
        assert_eq!(config["backend"], "ollama");
        assert_eq!(config["model"], "qwen3-vl:2b");
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_cloud_factory() {
        let factory = CloudFactory::new();
        assert_eq!(factory.backend_id(), "openai");
        assert!(factory.display_name().contains("Cloud"));
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_cloud_default_config() {
        let factory = CloudFactory::new();
        let config = factory.default_config();
        assert_eq!(config["backend"], "openai");
        assert_eq!(config["provider"], "openai");
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_cloud_factory_parse_provider() {
        assert_eq!(CloudFactory::parse_provider("openai").unwrap(), CloudProvider::OpenAI);
        assert_eq!(CloudFactory::parse_provider("claude").unwrap(), CloudProvider::Anthropic);
        assert_eq!(CloudFactory::parse_provider("gemini").unwrap(), CloudProvider::Google);
        assert_eq!(CloudFactory::parse_provider("grok").unwrap(), CloudProvider::Grok);
    }
}
