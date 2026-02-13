//! LLM runtime implementation.
//!
//! This crate provides LLM inference capabilities with support for:
//! - Ollama (local LLM runner) - default, enabled with `ollama` feature
//! - OpenAI - enabled with `openai` feature
//! - Anthropic - enabled with `anthropic` feature
//! - Google - enabled with `google` feature
//! - xAI - enabled with `xai` feature
//!
//! ## Features
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `ollama` | Local Ollama LLM runner (default) |
//! | `cloud` | Cloud backend support (requires HTTP client) |
//! | `openai` | OpenAI API support |
//! | `anthropic` | Anthropic Claude API support |
//! | `google` | Google Gemini API support |
//! | `xai` | xAI Grok API support |
//! | `all` | All backends |

pub mod backend_plugin;
pub mod backends;
pub mod config;
pub mod factories;
pub mod instance_manager;
pub mod rate_limited_client;
pub mod tokenizer;

// Re-export backend types based on features
#[cfg(feature = "ollama")]
pub use backends::ollama::{OllamaConfig, OllamaRuntime};

#[cfg(feature = "cloud")]
pub use backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

// Config and utilities
pub use config::{
    GenerationParams as LlmGenerationParams, LlmBackendConfig, LlmConfig, LlmRuntimeManager,
};
pub use tokenizer::TokenizerWrapper;

// Plugin system
pub use backend_plugin::{BackendRegistry, DynBackendPlugin, LlmBackendPlugin};

// Instance manager
pub use instance_manager::{
    get_instance_manager, BackendTypeDefinition, LlmBackendInstanceManager,
};

#[cfg(feature = "cloud")]
pub use backend_plugin::register_builtin_backends;

// Factory exports
#[cfg(feature = "cloud")]
pub use factories::CloudFactory;
pub use factories::MockFactory;
#[cfg(feature = "ollama")]
pub use factories::OllamaFactory;

// Backend creation utilities
pub use backends::{available_backends, create_backend};

/// Re-exports.
pub mod prelude {
    #[cfg(feature = "ollama")]
    pub use crate::backends::ollama::{OllamaConfig, OllamaRuntime};

    #[cfg(feature = "openai")]
    pub use crate::backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

    pub use crate::config::{LlmBackendConfig, LlmConfig, LlmRuntimeManager};
    pub use crate::tokenizer::TokenizerWrapper;

    #[cfg(feature = "ollama")]
    pub use crate::factories::OllamaFactory;

    #[cfg(feature = "cloud")]
    pub use crate::factories::CloudFactory;

    pub use crate::backends::{available_backends, create_backend};
    pub use crate::factories::MockFactory;
}
