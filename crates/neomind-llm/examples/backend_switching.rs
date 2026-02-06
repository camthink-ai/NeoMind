//! Example: LLM Backend Switching
//!
//! Demonstrates how to:
//! 1. Load different LLM backends from config
//! 2. Switch between backends at runtime
//! 3. Use environment variables for configuration

use neomind_llm::config::{LlmBackendConfig, LlmConfig, LlmRuntimeManager};

#[cfg(feature = "cloud")]
use neomind_llm::backends::OllamaConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== NeoMind LLM Backend Switching Example ===\n");

    // Example 1: Create Ollama config directly
    #[cfg(feature = "cloud")]
    {
        println!("1. Creating Ollama backend from code...");
        let ollama_config = LlmConfig {
            backend: LlmBackendConfig::Ollama(
                OllamaConfig::new("qwen3-vl:2b").with_endpoint("http://localhost:11434"),
            ),
            generation: neomind_llm::config::GenerationParams::default(),
        };

        println!("   Backend ID: {:?}\n", ollama_config.backend_id());
    }

    #[cfg(not(feature = "cloud"))]
    {
        println!("1. Cloud features not enabled. Enable with --features cloud\n");
    }

    // Example 2: Load from environment variables
    println!("2. Loading config from environment...");
    println!("   Set LLM_BACKEND and LLM_MODEL to test");

    match LlmConfig::from_env() {
        Ok(config) => {
            println!("   Loaded config: {:?} backend\n", config.backend_id());
        }
        Err(e) => {
            println!("   Failed to load from env: {}\n", e);
        }
    }

    // Example 3: Use RuntimeManager for dynamic backend switching
    println!("3. Using LlmRuntimeManager for backend management...");
    let mut manager = LlmRuntimeManager::new();

    // Try to load from environment variables
    match manager.load_from_env().await {
        Ok(()) => {
            if let Some(runtime) = manager.runtime() {
                println!("   Loaded runtime from env");
                println!("   Model: {}", runtime.model_name());
                println!("   Max context: {}", runtime.max_context_length());
                println!("   Supports multimodal: {}", runtime.supports_multimodal());
            }
        }
        Err(e) => {
            println!("   Failed to load from env: {}", e);
            println!("   Set LLM_PROVIDER=ollama to use Ollama");
        }
    }

    println!();

    // Example 4: Show backend capabilities
    println!("4. Backend capabilities:");
    println!("   Ollama (local):");
    println!("     - Streaming: Yes");
    println!("     - Multimodal: Yes (qwen3-vl)");
    println!("     - Function calling: Depends on model");
    println!("   Cloud (OpenAI/Anthropic/Google/xAI):");
    println!("     - Streaming: Yes");
    println!("     - Multimodal: Yes");
    println!("     - Function calling: Yes");

    println!();
    println!("=== Example Configuration ===");
    println!("To use a specific backend, set the config.toml:");
    println!();
    println!("[backend.ollama]");
    println!("endpoint = \"http://localhost:11434\"");
    println!("model = \"qwen3-vl:2b\"");
    println!();
    println!("Or for cloud:");
    println!();
    println!("[backend.cloud]");
    println!("api_key = \"sk-...\"");
    println!("provider = \"openai\"");
    println!("model = \"gpt-4o-mini\"");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "cloud")]
    #[test]
    fn test_backend_config_type() {
        let config = LlmConfig {
            backend: LlmBackendConfig::Ollama(OllamaConfig::default()),
            generation: neomind_llm::config::GenerationParams::default(),
        };
        assert_eq!(config.backend_id().as_str(), "ollama");
    }

    #[test]
    fn test_manager_default() {
        let manager = LlmRuntimeManager::new();
        assert!(manager.runtime().is_none());
        assert!(manager.config().is_none());
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_config_serialize() {
        let config = LlmConfig {
            backend: LlmBackendConfig::Ollama(
                OllamaConfig::new("/models/qwen3-vl").with_endpoint("http://localhost:11434"),
            ),
            generation: neomind_llm::config::GenerationParams::default(),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("backend = \"ollama\""));
    }
}
