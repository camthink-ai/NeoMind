//! Error types for the message system.

use thiserror::Error;

/// Result type for message operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in the message system.
#[derive(Debug, Error)]
pub enum Error {
    /// Message or channel not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Channel is disabled.
    #[error("Channel disabled: {0}")]
    ChannelDisabled(String),

    /// Send operation failed.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Storage operation failed.
    #[error("Storage failed: {0}")]
    Storage(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Other error.
    #[error("Other: {0}")]
    Other(#[from] anyhow::Error),
}

/// Convenient type alias for API responses with consistent error format.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NeoMindError {
    pub code: String,
    pub message: String,
}

impl NeoMindError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message)
    }
}

impl From<Error> for NeoMindError {
    fn from(err: Error) -> Self {
        match err {
            Error::NotFound(msg) => Self::not_found(msg),
            Error::Validation(msg) => Self::bad_request(msg),
            Error::InvalidConfiguration(msg) => Self::bad_request(msg),
            _ => Self::internal(err.to_string()),
        }
    }
}
