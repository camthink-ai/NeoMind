//! Error types for the storage crate.

use thiserror::Error;

// Re-export the core error type
pub use neomind_core::error::Error as NeoMindError;

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Storage error types.
#[derive(Debug, Error)]
pub enum Error {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Storage/Database error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Invalid dimension error.
    #[error("Invalid dimension: expected {expected}, found {found}")]
    InvalidDimension { expected: usize, found: usize },

    /// Not found error.
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Invalid input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

// Convert to NeoMindError
impl From<Error> for NeoMindError {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => NeoMindError::Storage(e.to_string()),
            Error::Serialization(s) => NeoMindError::Serialization(s),
            Error::Storage(s) => NeoMindError::Storage(s),
            Error::InvalidDimension { .. } => NeoMindError::Validation(e.to_string()),
            Error::NotFound(s) => NeoMindError::NotFound(s),
            Error::InvalidInput(s) => NeoMindError::Validation(s),
        }
    }
}

// External error conversions
impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Error::Serialization(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serialization(e.to_string())
    }
}

impl From<redb::Error> for Error {
    fn from(e: redb::Error) -> Self {
        Error::Storage(format!("Redb error: {}", e))
    }
}

impl From<redb::TransactionError> for Error {
    fn from(e: redb::TransactionError) -> Self {
        Error::Storage(format!("Redb transaction error: {}", e))
    }
}

impl From<redb::TableError> for Error {
    fn from(e: redb::TableError) -> Self {
        Error::Storage(format!("Redb table error: {}", e))
    }
}

impl From<redb::StorageError> for Error {
    fn from(e: redb::StorageError) -> Self {
        Error::Storage(format!("Redb storage error: {}", e))
    }
}

impl From<redb::CommitError> for Error {
    fn from(e: redb::CommitError) -> Self {
        Error::Storage(format!("Redb commit error: {}", e))
    }
}

impl From<redb::DatabaseError> for Error {
    fn from(e: redb::DatabaseError) -> Self {
        Error::Storage(format!("Redb database error: {}", e))
    }
}

impl From<redb::CompactionError> for Error {
    fn from(e: redb::CompactionError) -> Self {
        Error::Storage(format!("Redb compaction error: {}", e))
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        Error::Storage(format!("Task join error: {}", e))
    }
}
