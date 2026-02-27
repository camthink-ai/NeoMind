//! Process-isolated extension system
//!
//! This module provides process-level isolation for extensions that need
//! additional safety guarantees. Extensions running in isolated mode
//! cannot crash the main NeoMind process.

mod ipc;
mod process;

pub use ipc::{IpcMessage, IpcResponse};
pub use process::{IsolatedExtension, IsolatedExtensionConfig};

/// Result type for isolated extension operations
pub type IsolatedResult<T> = std::result::Result<T, IsolatedExtensionError>;

/// Error type for isolated extension operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum IsolatedExtensionError {
    /// Failed to spawn process
    #[error("Failed to spawn extension process: {0}")]
    SpawnFailed(String),

    /// IPC communication error
    #[error("IPC communication error: {0}")]
    IpcError(String),

    /// Extension crashed
    #[error("Extension process crashed: {0}")]
    Crashed(String),

    /// Timeout
    #[error("Extension operation timed out after {0}ms")]
    Timeout(u64),

    /// Invalid response
    #[error("Invalid response from extension: {0}")]
    InvalidResponse(String),

    /// Extension not initialized
    #[error("Extension not initialized")]
    NotInitialized,

    /// Extension already running
    #[error("Extension already running")]
    AlreadyRunning,

    /// Extension not running
    #[error("Extension not running")]
    NotRunning,
}
