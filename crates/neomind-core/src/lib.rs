//! Core traits and types for NeoMind.
//!
//! This crate defines the foundational abstractions used across the project.

// alerts module removed - use neomind_messages instead
pub mod brand;
pub mod builtin_llm;
pub mod config;
pub mod crypto;
pub mod dashboard;
pub mod datasource;
pub mod error;
pub mod event;
pub mod eventbus;
pub mod extension;
pub mod llm;
pub mod message;
pub mod net;
pub mod paths;
pub mod tools;

pub use llm::LlmError;

// Exports
pub use llm::backend::{
    BackendCapabilities, GenerationParams, LlmRuntime, ReasoningCapabilities, ReasoningControl,
    ThinkingEffort,
};

pub use message::{Content, ContentPart, Message, MessageRole};

// Event exports
pub use event::{MetricValue, NeoMindEvent};

// Event bus exports
pub use eventbus::EventBus;
