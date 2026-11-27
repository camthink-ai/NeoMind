//! API server for Edge AI Agent.
//!
//! This crate provides the HTTP/WebSocket API server for the Edge AI Agent system.

pub mod agent_defaults;
pub mod agent_events;
pub mod agent_ports;
pub mod ai_agent_runtime;
pub mod auth;
pub mod auth_users;
pub mod automation;
pub mod builtin_llm;
pub mod cache;
pub mod capability_providers;
pub mod config;
pub mod crypto;
pub mod event_services;
pub mod handlers;
pub mod metrics;
pub mod models;

pub mod rate_limit;
pub mod server;
pub mod shutdown;
pub mod startup;
pub mod upgrade;
pub mod validator;

// Re-export server entry points for binary crates (neomind-cli, neomind-tauri)
pub use server::{
    new_shutdown_handle, run, run_with_shutdown, start_server, start_server_with_shutdown,
    ShutdownHandle,
};
