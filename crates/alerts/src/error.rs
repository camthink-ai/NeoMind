//! Error types for the alerts crate.

// Re-export the core error type
pub use edge_ai_core::error::Error as NeoTalkError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Channel not found: {0}")]
    NotFound(String),

    #[error("Channel disabled: {0}")]
    ChannelDisabled(String),

    #[error("Send error: {0}")]
    SendError(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),
}

/// Result type for alerts operations.
pub type Result<T> = std::result::Result<T, Error>;

// Convert local Error to NeoTalkError
impl From<Error> for NeoTalkError {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound(s) => NeoTalkError::NotFound(s),
            Error::ChannelDisabled(s) => NeoTalkError::Validation(s),
            Error::SendError(s) => NeoTalkError::Internal(s),
            Error::Validation(s) => NeoTalkError::Validation(s),
            Error::Io(e) => NeoTalkError::Storage(e.to_string()),
            Error::Serialization(s) => NeoTalkError::Serialization(s),
            Error::Uuid(e) => NeoTalkError::Validation(e.to_string()),
        }
    }
}
