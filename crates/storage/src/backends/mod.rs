//! Storage backend implementations.
//!
//! This module contains implementations of the `StorageBackend` trait
//! for various storage engines, feature-gated for conditional compilation.

use edge_ai_core::storage::StorageBackend;
use serde_json::Value;
use std::sync::Arc;

// Redb backend (feature-gated)
#[cfg(feature = "redb")]
pub mod redb;

// Memory backend (feature-gated)
#[cfg(feature = "memory")]
pub mod memory;

// Re-exports
#[cfg(feature = "redb")]
pub use redb::{RedbBackend, RedbBackendConfig};

#[cfg(feature = "memory")]
pub use memory::{MemoryBackend, MemoryBackendConfig};

/// Create a storage backend by type identifier.
///
/// This function provides a unified way to create storage backends
/// based on configuration, with feature-gated compilation.
///
/// # Example
/// ```no_run
/// use edge_ai_storage::backends::create_backend;
/// use serde_json::json;
///
/// # fn main() -> anyhow::Result<()> {
/// // Create a redb backend
/// let config = json!({
///     "path": "./data/storage"
/// });
/// let backend = create_backend("redb", &config)?;
/// # Ok(())
/// # }
/// ```
pub fn create_backend(
    backend_type: &str,
    config: &Value,
) -> edge_ai_core::storage::Result<Arc<dyn StorageBackend>> {
    match backend_type {
        #[cfg(feature = "redb")]
        "redb" => {
            let cfg: RedbBackendConfig = serde_json::from_value(config.clone())
                .map_err(|e| edge_ai_core::storage::StorageError::Configuration(
                    format!("Invalid redb config: {}", e)
                ))?;
            Ok(Arc::new(redb::RedbBackend::new(cfg)?))
        }

        #[cfg(feature = "memory")]
        "memory" => {
            let cfg: MemoryBackendConfig = serde_json::from_value(config.clone())
                .map_err(|e| edge_ai_core::storage::StorageError::Configuration(
                    format!("Invalid memory config: {}", e)
                ))?;
            Ok(Arc::new(memory::MemoryBackend::new(cfg)))
        }

        _ => Err(edge_ai_core::storage::StorageError::Configuration(
            format!("Unknown backend type: {}. Available backends: {}",
                backend_type, available_backends().join(", "))
        )),
    }
}

/// Get list of available backend types (based on enabled features).
///
/// # Example
/// ```
/// use edge_ai_storage::backends::available_backends;
///
/// let backends = available_backends();
/// println!("Available backends: {:?}", backends);
/// ```
pub fn available_backends() -> Vec<&'static str> {
    let mut backends = Vec::new();

    #[cfg(feature = "redb")]
    backends.push("redb");

    #[cfg(feature = "memory")]
    backends.push("memory");

    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_backends() {
        let backends = available_backends();
        // At least redb should be available (default feature)
        assert!(!backends.is_empty());
    }

    #[test]
    fn test_create_backend_unknown() {
        let result = create_backend("unknown", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[cfg(feature = "memory")]
    #[test]
    fn test_create_memory_backend() {
        let config = serde_json::json!({});
        let backend = create_backend("memory", &config);
        assert!(backend.is_ok());
        assert!(!backend.unwrap().is_persistent());
    }
}
