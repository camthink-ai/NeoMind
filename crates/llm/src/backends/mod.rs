//! LLM backend implementations.
//!
//! This module provides concrete implementations of the `LlmRuntime` trait
//! for various inference backends.

#[cfg(feature = "cloud")]
use crate::backend_plugin::BackendRegistry;

// Ollama backend (local LLM runner)
#[cfg(feature = "ollama")]
pub mod ollama;
#[cfg(feature = "ollama")]
pub use ollama::{OllamaConfig, OllamaRuntime};

// OpenAI-compatible cloud backends (OpenAI, Anthropic, Google, xAI, etc.)
#[cfg(feature = "cloud")]
pub mod openai;
#[cfg(feature = "cloud")]
pub use openai::{CloudConfig, CloudProvider, CloudRuntime};

/// Create a backend by type identifier.
///
/// This function provides a unified way to create LLM backends
/// based on configuration, with feature-gated compilation.
///
/// First tries the plugin registry for dynamic backends,
/// then falls back to built-in implementations for backward compatibility.
pub fn create_backend(
    backend_type: &str,
    config: &serde_json::Value,
) -> Result<std::sync::Arc<dyn edge_ai_core::llm::backend::LlmRuntime>, anyhow::Error> {
    // Try plugin registry first (for dynamically registered backends)
    #[cfg(feature = "cloud")]
    {
        if let Some(plugin) = BackendRegistry::global().get(backend_type) {
            return plugin.create_runtime(config)
                .map(|r| std::sync::Arc::from(r) as std::sync::Arc<dyn edge_ai_core::llm::backend::LlmRuntime>)
                .map_err(|e| anyhow::anyhow!("Plugin runtime error: {}", e));
        }
    }

    // Fall back to built-in implementations
    match backend_type {
        #[cfg(feature = "ollama")]
        "ollama" => {
            let cfg: OllamaConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Ollama config: {}", e))?;
            Ok(std::sync::Arc::new(OllamaRuntime::new(cfg)?))
        }

        #[cfg(feature = "openai")]
        "openai" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid OpenAI config: {}", e))?;
            cfg.provider = CloudProvider::OpenAI;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "anthropic")]
        "anthropic" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Anthropic config: {}", e))?;
            cfg.provider = CloudProvider::Anthropic;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "google")]
        "google" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Google config: {}", e))?;
            cfg.provider = CloudProvider::Google;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "xai")]
        "xai" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid xAI config: {}", e))?;
            cfg.provider = CloudProvider::Grok;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        _ => Err(anyhow::anyhow!("Unknown backend type: {}", backend_type)),
    }
}

/// Get list of available backend types (based on enabled features).
pub fn available_backends() -> Vec<&'static str> {
    let mut backends = Vec::new();

    #[cfg(feature = "ollama")]
    backends.push("ollama");

    #[cfg(feature = "openai")]
    backends.push("openai");

    #[cfg(feature = "anthropic")]
    backends.push("anthropic");

    #[cfg(feature = "google")]
    backends.push("google");

    #[cfg(feature = "xai")]
    backends.push("xai");

    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_backends() {
        let backends = available_backends();
        // At least ollama should be available (default feature)
        assert!(!backends.is_empty());
    }

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_backend() {
        let config = OllamaConfig::new("qwen3-vl:2b");
        assert_eq!(config.model, "qwen3-vl:2b");
    }

    #[cfg(feature = "openai")]
    #[test]
    fn test_cloud_backend_openai() {
        let config = CloudConfig::openai("sk-test");
        assert_eq!(config.provider, CloudProvider::OpenAI);
    }
}
