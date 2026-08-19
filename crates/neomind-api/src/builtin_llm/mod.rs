//! Built-in bundled LLM model support (HTTP + orchestration).
//!
//! - `download`: resumable GGUF downloads with SHA256 verification.
//! - `server`:   spawn + health-poll the bundled llama-server.

pub mod download;
pub mod server;
