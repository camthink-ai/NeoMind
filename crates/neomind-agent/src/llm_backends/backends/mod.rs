//! LLM backend implementations.
//!
//! This module provides concrete implementations of the `LlmRuntime` trait
//! for various inference backends.

#[cfg(feature = "cloud")]
use super::backend_plugin::BackendRegistry;

// Ollama backend (local LLM runner - uses llama.cpp internally)
#[cfg(feature = "ollama")]
pub mod ollama;
#[cfg(feature = "ollama")]
pub use ollama::{OllamaConfig, OllamaRuntime};

// llama.cpp standalone server backend
#[cfg(feature = "llamacpp")]
pub mod llamacpp;
#[cfg(feature = "llamacpp")]
pub use llamacpp::{LlamaCppConfig, LlamaCppRuntime};

// OpenAI-compatible cloud backends (OpenAI, Anthropic, Google, xAI, etc.)
#[cfg(feature = "cloud")]
pub mod openai;
#[cfg(feature = "cloud")]
pub use openai::{CloudConfig, CloudProvider, CloudRuntime};

/// Idle-bounded byte-stream read for backend HTTP SSE consumption.
///
/// Returns the next stream item, or `None` if the stream ends normally OR if
/// no chunk arrives within `idle` — a stalled upstream SSE connection (bytes
/// stop arriving but the socket stays open) is treated as end-of-stream so the
/// consumer loop force-completes instead of blocking `stream.next()` forever.
/// This is the backend-layer fix for the 8h-eval-hang class of bug; mirrors
/// `agent::streaming::next_chunk_or_timeout` for the raw HTTP byte stream.
pub(crate) async fn next_bytes_or_end<S>(
    stream: &mut S,
    idle: std::time::Duration,
) -> Option<S::Item>
where
    S: futures::Stream + Unpin,
{
    match tokio::time::timeout(idle, futures::StreamExt::next(stream)).await {
        Ok(Some(item)) => Some(item),
        Ok(None) => None, // stream ended normally
        Err(_elapsed) => {
            tracing::warn!(
                "Backend byte stream stalled: no chunks for {:?}, treating as end-of-stream",
                idle
            );
            None
        }
    }
}

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
) -> Result<std::sync::Arc<dyn neomind_core::llm::backend::LlmRuntime>, anyhow::Error> {
    // Try plugin registry first (for dynamically registered backends)
    #[cfg(feature = "cloud")]
    {
        if let Some(plugin) = BackendRegistry::global().get(backend_type) {
            return plugin
                .create_runtime(config)
                .map(|r| {
                    std::sync::Arc::from(r)
                        as std::sync::Arc<dyn neomind_core::llm::backend::LlmRuntime>
                })
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

        #[cfg(feature = "llamacpp")]
        "llamacpp" => {
            let cfg: LlamaCppConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid llama.cpp config: {}", e))?;
            Ok(std::sync::Arc::new(LlamaCppRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "openai" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid OpenAI config: {}", e))?;
            cfg.provider = CloudProvider::OpenAI;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "anthropic" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Anthropic config: {}", e))?;
            cfg.provider = CloudProvider::Anthropic;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "google" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Google config: {}", e))?;
            cfg.provider = CloudProvider::Google;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "xai" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid xAI config: {}", e))?;
            cfg.provider = CloudProvider::Grok;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "qwen" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid Qwen config: {}", e))?;
            cfg.provider = CloudProvider::Qwen;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "deepseek" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid DeepSeek config: {}", e))?;
            cfg.provider = CloudProvider::DeepSeek;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "glm" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid GLM config: {}", e))?;
            cfg.provider = CloudProvider::GLM;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        #[cfg(feature = "cloud")]
        "minimax" => {
            let mut cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid MiniMax config: {}", e))?;
            cfg.provider = CloudProvider::MiniMax;
            Ok(std::sync::Arc::new(CloudRuntime::new(cfg)?))
        }

        _ => Err(anyhow::anyhow!("Unknown backend type: {}", backend_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_backend() {
        let config = OllamaConfig::new("qwen3-vl:2b");
        assert_eq!(config.model, "qwen3-vl:2b");
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_cloud_backend_openai() {
        let config = CloudConfig::openai("sk-test");
        assert_eq!(config.provider, CloudProvider::OpenAI);
    }

    /// A stalled upstream byte stream (no chunk ever arrives) must break within
    /// `idle` and return `None`, not hang `stream.next()` forever. Regression for
    /// the llamacpp/ollama mid-stream hang (stream-hang-fix was applied only to
    /// openai/anthropic — commit 162c73ff — leaving these backends unbounded).
    /// The outer `tokio::time::timeout` turns a hang (the bug) into a clean
    /// assertion failure instead of a stuck test process.
    #[tokio::test]
    async fn next_bytes_or_end_breaks_idle_stall() {
        use std::time::{Duration, Instant};

        let mut stalled = futures::stream::pending::<u32>(); // never yields, never ends
        let idle = Duration::from_secs(1);
        let deadline = Duration::from_secs(5);

        let start = Instant::now();
        let outcome = tokio::time::timeout(deadline, next_bytes_or_end(&mut stalled, idle)).await;
        let elapsed = start.elapsed();

        // With the fix: returns None at ~idle (1s), well under the 5s deadline.
        // With the bug (no timeout): hangs → outer deadline fires → outcome is Err.
        assert!(
            elapsed < deadline,
            "stalled stream should break at idle (~{:?}), but hung for {:?}",
            idle,
            elapsed
        );
        assert!(outcome.is_ok(), "should return None before deadline, not hang");
        assert_eq!(outcome.unwrap(), None);
    }
}
