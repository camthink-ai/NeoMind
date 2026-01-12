//! Core LLM traits and types.
//!
//! This module provides abstractions for LLM inference backends.

pub mod backend;
pub mod modality;

pub use backend::{
    BackendCapabilities, BackendId, DynamicLlmRuntime, FinishReason, GenerationParams,
    LlmError, LlmInput, LlmOutput, LlmRuntime, StreamChunk, TokenUsage,
};
pub use modality::{ImageContent, ImageInput, ModalityContent};

use std::pin::Pin;
use std::time::Duration;

use futures::Stream;

use crate::message::Message;

/// Configuration for LLM backend.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Path to the model file.
    pub model_path: String,

    /// Path to the tokenizer file.
    pub tokenizer_path: Option<String>,

    /// Maximum sequence length.
    pub max_seq_len: Option<usize>,

    /// Temperature for sampling (0.0 to 2.0).
    pub temperature: Option<f32>,

    /// Top-p sampling threshold.
    pub top_p: Option<f32>,

    /// Top-k sampling threshold.
    pub top_k: Option<usize>,

    /// Repeat penalty.
    pub repeat_penalty: Option<f32>,

    /// Number of tokens to generate.
    pub sample_len: Option<usize>,

    /// Device to use (cpu, cuda, metal).
    pub device: String,

    /// Whether to use quantization.
    pub quantized: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            tokenizer_path: None,
            max_seq_len: None,
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            repeat_penalty: Some(1.1),
            sample_len: Some(512),
            device: "cpu".to_string(),
            quantized: true,
        }
    }
}

impl LlmConfig {
    /// Create a new config with the given model path.
    pub fn new(model_path: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            ..Default::default()
        }
    }

    /// Set the maximum sequence length.
    pub fn with_max_seq_len(mut self, len: usize) -> Self {
        self.max_seq_len = Some(len);
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the device.
    pub fn with_device(mut self, device: impl Into<String>) -> Self {
        self.device = device.into();
        self
    }

    /// Enable or disable quantization.
    pub fn with_quantization(mut self, quantized: bool) -> Self {
        self.quantized = quantized;
        self
    }

    /// Set the sample length.
    pub fn with_sample_len(mut self, len: usize) -> Self {
        self.sample_len = Some(len);
        self
    }
}

/// Result of text generation.
#[derive(Debug, Clone)]
pub struct GenerationResult {
    /// Generated text.
    pub text: String,

    /// Number of tokens generated.
    pub token_count: usize,

    /// Time taken for generation.
    pub duration: Duration,

    /// Stop reason (eos, length, etc).
    pub stop_reason: StopReason,
}

/// Reason why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// End of sequence token.
    Eos,
    /// Maximum length reached.
    Length,
    /// Stop sequence encountered.
    StopSequence,
    /// Manual stop.
    Manual,
}

/// Stream of generated tokens.
pub type GenerationStream =
    Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>;

/// Core trait for LLM backends (legacy, simple API).
///
/// This is the original simple trait for basic LLM operations.
/// For more advanced features (multimodal, multiple backends, etc.),
/// use the `LlmRuntime` trait from the `backend` module.
#[async_trait::async_trait]
pub trait LlmBackend: Send + Sync {
    /// Generate a response from a prompt.
    async fn generate(&self, prompt: &str) -> Result<GenerationResult, LlmError>;

    /// Generate with streaming output.
    async fn generate_stream(
        &self,
        prompt: &str,
    ) -> Result<GenerationStream, LlmError>;

    /// Chat completion with message history.
    async fn chat(
        &self,
        messages: &[Message],
    ) -> Result<GenerationResult, LlmError>;

    /// Chat completion with streaming.
    async fn chat_stream(
        &self,
        messages: &[Message],
    ) -> Result<GenerationStream, LlmError>;

    /// Get the model name/version.
    fn model_name(&self) -> &str;

    /// Get the maximum context length.
    fn max_context_length(&self) -> usize;
}
