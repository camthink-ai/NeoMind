//! Error types for the scenarios crate.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Scenario not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Duplicate scenario: {0}")]
    Duplicate(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),
}

/// Result type for scenarios operations.
pub type Result<T> = std::result::Result<T, Error>;
