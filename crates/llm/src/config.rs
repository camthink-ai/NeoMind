//! LLM backend configuration.
//!
//! Configuration for various LLM backends (Ollama, OpenAI, Anthropic, etc.).

use serde::{Deserialize, Serialize};

use edge_ai_core::llm::backend::{BackendId, LlmError, LlmRuntime};
use edge_ai_core::config::{self, endpoints, models, env_vars,
                            normalize_ollama_endpoint, normalize_openai_endpoint};

#[cfg(feature = "cloud")]
use crate::backends::{CloudConfig, CloudProvider, CloudRuntime, OllamaConfig, OllamaRuntime};

/// LLM backend configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "backend")]
pub enum LlmBackendConfig {
    /// Ollama (local LLM runner).
    #[serde(rename = "ollama")]
    #[cfg(feature = "cloud")]
    Ollama(OllamaConfig),

    /// Cloud API (OpenAI, Anthropic, Google, xAI, etc.).
    #[serde(rename = "cloud")]
    #[cfg(feature = "cloud")]
    Cloud(CloudConfig),
}

#[cfg(feature = "cloud")]
impl Default for LlmBackendConfig {
    fn default() -> Self {
        Self::Ollama(OllamaConfig::default())
    }
}

impl LlmBackendConfig {
    /// Get the backend identifier.
    pub fn backend_id(&self) -> BackendId {
        match self {
            #[cfg(feature = "cloud")]
            Self::Ollama(_) => BackendId::new(BackendId::OLLAMA),
            #[cfg(feature = "cloud")]
            Self::Cloud(_) => BackendId::new(BackendId::OPENAI),
            #[cfg(not(feature = "cloud"))]
            _ => unreachable!("No backends available without features"),
        }
    }

    /// Get the backend type (deprecated, use backend_id instead).
    #[deprecated(note = "Use backend_id instead")]
    pub fn backend_type(&self) -> String {
        self.backend_id().as_str().to_string()
    }

    /// Create a runtime from this configuration.
    pub async fn into_runtime(self) -> Result<Box<dyn LlmRuntime>, LlmError> {
        match self {
            #[cfg(feature = "cloud")]
            Self::Ollama(config) => {
                let runtime = OllamaRuntime::new(config)?;
                Ok(Box::new(runtime))
            }
            #[cfg(feature = "cloud")]
            Self::Cloud(config) => {
                let runtime = CloudRuntime::new(config)?;
                Ok(Box::new(runtime))
            }
            #[cfg(not(feature = "cloud"))]
            _ => Err(LlmError::BackendUnavailable("no backend".to_string())),
        }
    }
}

/// Top-level LLM configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmConfig {
    /// LLM backend configuration.
    pub backend: LlmBackendConfig,

    /// Default generation parameters.
    #[serde(default)]
    pub generation: GenerationParams,
}

impl LlmConfig {
    /// Load from environment variables.
    ///
    /// Reads:
    /// - `OLLAMA_ENDPOINT`: Ollama endpoint
    /// - `OPENAI_API_KEY`: API key for OpenAI
    /// - `LLM_PROVIDER`: Backend type ("ollama" or "cloud")
    /// - `LLM_MODEL`: model name
    pub fn from_env() -> Result<Self, LlmError> {
        #[cfg(feature = "cloud")]
        {
            let provider = std::env::var(env_vars::LLM_PROVIDER)
                .unwrap_or_else(|_| "ollama".to_string());

            match provider.to_lowercase().as_str() {
                "ollama" => {
                    let endpoint = std::env::var(env_vars::OLLAMA_ENDPOINT)
                        .unwrap_or_else(|_| endpoints::OLLAMA.to_string());
                    let endpoint = normalize_ollama_endpoint(endpoint);
                    let model = std::env::var(env_vars::LLM_MODEL)
                        .unwrap_or_else(|_| models::OLLAMA_DEFAULT.to_string());

                    let ollama_config = OllamaConfig::new(model).with_endpoint(endpoint);
                    let backend_config = LlmBackendConfig::Ollama(ollama_config);

                    return Ok(Self {
                        backend: backend_config,
                        generation: GenerationParams::default(),
                    });
                }
                "cloud" | "openai" => {
                    let api_key = std::env::var(env_vars::OPENAI_API_KEY)
                        .map_err(|_| LlmError::InvalidInput("OPENAI_API_KEY not set".into()))?;
                    let model = std::env::var(env_vars::LLM_MODEL)
                        .unwrap_or_else(|_| models::OPENAI_DEFAULT.to_string());

                    let cloud_config = CloudConfig::openai(api_key).with_model(model);
                    let backend_config = LlmBackendConfig::Cloud(cloud_config);

                    return Ok(Self {
                        backend: backend_config,
                        generation: GenerationParams::default(),
                    });
                }
                _ => Err(LlmError::InvalidInput(format!(
                    "Unknown LLM provider: {}",
                    provider
                ))),
            }
        }

        #[cfg(not(feature = "cloud"))]
        Err(LlmError::BackendUnavailable("cloud".to_string()))
    }

    /// Create a runtime from this configuration.
    pub async fn into_runtime(self) -> Result<Box<dyn LlmRuntime>, LlmError> {
        self.backend.into_runtime().await
    }

    /// Get the backend identifier.
    pub fn backend_id(&self) -> BackendId {
        self.backend.backend_id()
    }

    /// Get the backend type (deprecated, use backend_id instead).
    #[deprecated(note = "Use backend_id instead")]
    pub fn backend_type(&self) -> String {
        self.backend.backend_type()
    }
}

#[cfg(feature = "cloud")]
impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            backend: LlmBackendConfig::Ollama(OllamaConfig::default()),
            generation: GenerationParams::default(),
        }
    }
}

/// Generation parameters.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenerationParams {
    /// Temperature (0.0 to 2.0).
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Top-p sampling.
    #[serde(default = "default_top_p")]
    pub top_p: f32,

    /// Top-k sampling.
    #[serde(default)]
    pub top_k: Option<u32>,

    /// Maximum tokens to generate.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

fn default_temperature() -> f32 {
    0.7
}

fn default_top_p() -> f32 {
    0.9
}

fn default_max_tokens() -> usize {
    512
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            temperature: default_temperature(),
            top_p: default_top_p(),
            top_k: None,
            max_tokens: default_max_tokens(),
        }
    }
}

/// LLM runtime manager.
///
/// Manages the active LLM runtime with support for hot-reloading configuration.
pub struct LlmRuntimeManager {
    runtime: Option<Box<dyn LlmRuntime>>,
    config: Option<LlmConfig>,
}

impl LlmRuntimeManager {
    /// Create a new manager.
    pub fn new() -> Self {
        Self {
            runtime: None,
            config: None,
        }
    }

    /// Load configuration from environment.
    pub async fn load_from_env(&mut self) -> Result<(), LlmError> {
        let config = LlmConfig::from_env()?;
        self.runtime = Some(config.clone().into_runtime().await?);
        self.config = Some(config);
        Ok(())
    }

    /// Get the current runtime.
    pub fn runtime(&self) -> Option<&dyn LlmRuntime> {
        self.runtime.as_ref().map(|r| r.as_ref())
    }

    /// Get the current runtime as a mutable reference.
    pub fn runtime_mut(&mut self) -> Option<&mut Box<dyn LlmRuntime>> {
        self.runtime.as_mut()
    }

    /// Get the current configuration.
    pub fn config(&self) -> Option<&LlmConfig> {
        self.config.as_ref()
    }

    /// Reload the runtime from the current configuration.
    pub async fn reload(&mut self) -> Result<(), LlmError> {
        if let Some(config) = &self.config {
            self.runtime = Some(config.clone().into_runtime().await?);
            Ok(())
        } else {
            Err(LlmError::InvalidInput("No configuration loaded".into()))
        }
    }
}

impl Default for LlmRuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LlmConfig::default();
        assert_eq!(config.backend_id().as_str(), "ollama");
    }

    #[test]
    fn test_generation_params_default() {
        let params = GenerationParams::default();
        assert_eq!(params.temperature, 0.7);
        assert_eq!(params.top_p, 0.9);
        assert_eq!(params.max_tokens, 512);
    }
}
