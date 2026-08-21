//! Built-in bundled LLM model support (pure logic — no HTTP dependency).
//!
//! `manifest` tracks the downloaded GGUF's id/version/file/sha256/quant,
//! `variant` resolves quantization selection, and `find` locates the bundled
//! llama-server binary.

pub mod find;
pub mod manifest;
pub mod runtime;
pub mod variant;
