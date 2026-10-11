// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NeoMind Extension Runner
//!
//! This is a standalone process that loads and runs a single extension.
//! It communicates with the main NeoMind process via stdin/stdout using
//! the IPC protocol.
//!
//! # Supported Extension Types
//!
//! - Native libraries (.so, .dylib, .dll)
//!
//! # Usage
//!
//! ```bash
//! neomind-extension-runner --extension-path /path/to/extension.dylib
//! ```
//!
//! # Protocol
//!
//! The runner reads IPC messages from stdin and writes responses to stdout.
//! All messages are framed with a 4-byte length prefix (little-endian).

use std::io::Write;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use neomind_extension_sdk::ExtensionDescriptor;
use tracing::{debug, error};

// Resource limits module
mod resource_limits;
use resource_limits::{setup_resource_limits, ResourceLimitsConfig};

// Dylib validation module
mod dylib_validation;

mod host;
pub(crate) use capabilities::*;
use host::SyncIpcClient;
pub(crate) use native::*;
pub(crate) use push::*;
pub(crate) use runner::*;

// IPC routing module
mod ipc_routing;
use ipc_routing::STDOUT_WRITE_MUTEX;

// ============================================================================
// Message routing for capability invocation
// ============================================================================

// IPC routing and event handling now live in their own modules

/// Extension runner arguments
#[derive(Parser, Debug)]
#[command(name = "neomind-extension-runner")]
#[command(about = "Run a NeoMind extension in isolated mode")]
struct Args {
    /// Path to the extension library (.so, .dylib, .dll)
    #[arg(long, short = 'e')]
    extension_path: PathBuf,

    /// Enable verbose logging
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Memory limit in MB (0 = no limit).
    ///
    /// Default is 0 (unlimited) so GPU extensions don't OOM on startup. If a
    /// deployment needs to bound extension memory, override via CLI flag or
    /// systemd `MemoryMax=`.
    #[arg(long = "memory-limit", default_value = "0")]
    memory_limit_mb: u64,

    /// Hard memory limit in MB (0 = 2x soft limit)
    #[arg(long = "memory-limit-hard", default_value = "0")]
    memory_limit_hard_mb: u64,

    /// Process nice level (priority, -20 to 19, use 10 for background)
    #[arg(long = "nice", default_value = "10")]
    nice_level: i32,
}

/// Extension runner state
struct Runner {
    /// Loaded native extension
    extension: Option<NativeExtensionBridge>,
    /// Extension descriptor (unified capabilities)
    descriptor: ExtensionDescriptor,
    /// Running flag
    running: bool,
}

type JsonFn0 = unsafe extern "C" fn() -> *mut c_char;
type JsonFn1 = unsafe extern "C" fn(*const u8, usize) -> *mut c_char;
type FreeStringFn = unsafe extern "C" fn(*mut c_char);
type SetCapabilityBridgeFn = unsafe extern "C" fn(HostCapabilityInvokeFn, HostCapabilityFreeFn);
type HostCapabilityInvokeFn = unsafe extern "C" fn(*const u8, usize) -> *mut c_char;
type HostCapabilityFreeFn = unsafe extern "C" fn(*mut c_char);

static GLOBAL_NATIVE_IPC_CLIENT: std::sync::OnceLock<Arc<SyncIpcClient>> =
    std::sync::OnceLock::new();

fn main() {
    let workers = runner_worker_threads();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .expect("failed to build runner tokio runtime")
        .block_on(async_main())
}

async fn async_main() {
    // Set up panic hook FIRST to capture any panics during loading
    // This ensures we can report errors to the parent process before dying
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        // Write to stderr in a format the parent process can parse
        eprintln!("[EXTENSION_RUNNER_PANIC] {} at {}", message, location);
        eprintln!("[EXTENSION_RUNNER_FATAL] Extension runner crashed - see panic details above");
        let _ = std::io::stderr().flush();
    }));

    let args = Args::parse();

    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .compact()
        .init();

    debug!("NeoMind Extension Runner starting");
    let _ = std::io::stderr().flush();
    debug!(extension_path = %args.extension_path.display(), "Extension path");

    // Set up resource limits BEFORE loading extension
    let memory_limit = if args.memory_limit_mb > 0 {
        Some(args.memory_limit_mb)
    } else {
        None
    };
    let hard_memory_limit = if args.memory_limit_hard_mb > 0 {
        Some(args.memory_limit_hard_mb)
    } else {
        None
    };

    let limits_config = ResourceLimitsConfig {
        memory_limit_mb: memory_limit,
        memory_limit_hard_mb: hard_memory_limit,
        cpu_affinity: None,
        nice_level: Some(args.nice_level),
    };

    if let Err(e) = setup_resource_limits(&limits_config) {
        error!("Failed to set resource limits: {}. Continuing anyway.", e);
    }

    if !args.extension_path.exists() {
        error!(path = %args.extension_path.display(), "Extension file not found");
        std::process::exit(1);
    }

    let mut runner = match Runner::load(&args.extension_path).await {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "Failed to load extension");
            std::process::exit(1);
        }
    };

    debug!("Starting runner main loop");
    runner.run().await;

    debug!("Extension runner exiting normally");
    std::process::exit(0);
}

// ============================================================================
// Tests
// ============================================================================

// Domain submodules — the runner binary was a 3.7k-line monolith.
mod capabilities;
mod native;
mod push;
mod runner;
