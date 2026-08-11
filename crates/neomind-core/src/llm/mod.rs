//! Core LLM traits and types.
//!
//! This module provides abstractions for LLM inference backends.

pub mod backend;
pub mod capability;
pub mod compaction;
pub mod modality;
pub mod models;
pub mod registry;

pub use backend::{
    BackendCapabilities, BackendId, FinishReason, GenerationParams, LlmError, LlmInput, LlmOutput,
    LlmRuntime, ReasoningCapabilities, ReasoningControl, StreamChunk, ThinkingEffort, TokenUsage,
};
pub use capability::{
    detect_thinking, detect_vision_capability, get_max_context, model_supports,
    CapabilityDetectionResult, CapabilityDetector,
};
pub use compaction::{
    compact_messages, estimate_tokens, CompactionConfig, CompactionResult, MessagePriority,
};
pub use modality::{ImageContent, ImageInput, ModalityContent};
pub use models::*;
