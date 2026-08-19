//! LLM runtime implementation.
//!
//! This module provides LLM inference capabilities with support for:
//! - Ollama (local LLM runner) - default, enabled with `ollama` feature
//! - OpenAI - enabled with `openai` feature
//! - Anthropic - enabled with `anthropic` feature
//! - Google - enabled with `google` feature
//! - xAI - enabled with `xai` feature
//!
//! Merged from neomind-llm crate as part of the lightweight-5-crates refactoring.

pub mod backend_plugin;
pub mod backends;
pub mod instance_manager;
pub mod rate_limited_client;

// Re-export backend types - available unconditionally for backward compatibility
// (actual instantiation requires appropriate feature)
pub use backends::ollama::{OllamaConfig, OllamaRuntime};

#[cfg(feature = "cloud")]
pub use backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

// Instance manager
pub use instance_manager::{
    get_instance_manager, set_instance_manager, BackendTypeDefinition, LlmBackendInstanceManager,
};

// Backend creation utilities
pub use backends::create_backend;
