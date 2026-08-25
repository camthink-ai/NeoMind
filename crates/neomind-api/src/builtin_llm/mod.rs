//! Built-in bundled LLM model support (HTTP + orchestration).
//!
//! - `download`: resumable GGUF downloads with SHA256 verification.
//! - `runtime`:  on-demand llama-server (bundled first, else official prebuilt).
//! - `server`:   spawn + health-poll the bundled llama-server.

pub mod config;
pub mod download;
pub mod gguf;
pub mod handlers;
pub mod runtime;
pub mod server;
pub mod state;
