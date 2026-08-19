//! Built-in bundled LLM model support (pure logic — no HTTP dependency).
//!
//! Task 1 adds `manifest` (tracks the downloaded GGUF's id/version/file/
//! sha256/quant). Later tasks in this feature add `variant` (quantization
//! selection) and `find` (llama-server binary discovery) — each adds its own
//! `pub mod` line here when its module file is created.

pub mod manifest;
pub mod variant;
