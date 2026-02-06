//! Core storage abstractions for NeoMind.
//!
//! This module defines the foundational traits for storage backends.

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage error types.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Key not found.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Backend error.
    #[error("Backend error: {0}")]
    Backend(String),

    /// Other error.
    #[error("Storage error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Core storage backend trait.
///
/// This trait defines the interface for all storage implementations
/// (redb, memory, and future backends).
pub trait StorageBackend: Send + Sync {
    /// Write a value to a key in the specified table.
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()>;

    /// Read a value by key from the specified table.
    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete a key from the specified table.
    fn delete(&self, table: &str, key: &str) -> Result<bool>;

    /// Scan keys with a given prefix in the specified table.
    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>>;

    /// Batch write multiple values to the specified table.
    fn write_batch(&self, table: &str, items: Vec<(String, Vec<u8>)>) -> Result<()>;

    /// Check if this backend supports persistent storage.
    fn is_persistent(&self) -> bool;
}

/// Factory for creating storage backends.
pub trait StorageFactory: Send + Sync {
    /// Backend type identifier.
    fn backend_type(&self) -> &str;

    /// Create a new backend instance with the given configuration.
    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn StorageBackend>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StorageError::KeyNotFound("test_key".to_string());
        assert!(err.to_string().contains("test_key"));
    }
}
