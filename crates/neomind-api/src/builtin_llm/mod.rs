//! Built-in bundled LLM model support (HTTP + orchestration).
//!
//! - `download`:        resumable GGUF downloads with SHA256 verification.
//! - `runtime`:         llama-server resolution (newest-valid-wins) + install.
//! - `runtime_manager`: standalone runtime upgrades — enumeration, update
//!   channel (`runtimes.json`), upgrade/revert, variant override resolution
//!   (env > setting > auto-detect).
//! - `server`:          spawn + health-poll the bundled llama-server.
//! - `sources`:          download-source mirror layer (HF/catalog/channel).
//! - `vram`:            spawn-time GPU offload sizing (discrete NVIDIA GPUs).

pub mod catalog;
pub mod config;
pub mod download;
pub mod gguf;
pub mod handlers;
pub mod runtime;
pub mod runtime_manager;
pub mod server;
pub mod sources;
pub mod state;
pub mod vram;
