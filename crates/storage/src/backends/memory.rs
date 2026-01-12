//! In-memory storage backend implementation.
//!
//! Provides non-persistent storage for testing and development.

use edge_ai_core::storage::{StorageBackend, StorageError, Result as CoreResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};

type Result<T> = CoreResult<T>;

/// Configuration for MemoryBackend.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MemoryBackendConfig {
    /// Initial capacity hint (optional).
    #[serde(default)]
    pub capacity: Option<usize>,
}

impl MemoryBackendConfig {
    /// Create a new config with default settings.
    pub fn new() -> Self {
        Self { capacity: None }
    }

    /// Set initial capacity hint.
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = Some(capacity);
        self
    }
}

impl Default for MemoryBackendConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory storage backend for testing.
pub struct MemoryBackend {
    /// In-memory data storage.
    data: Arc<StdRwLock<HashMap<String, HashMap<String, Vec<u8>>>>>,
}

impl MemoryBackend {
    /// Create a new in-memory backend.
    pub fn new(config: MemoryBackendConfig) -> Self {
        let data = if let Some(capacity) = config.capacity {
            HashMap::with_capacity(capacity)
        } else {
            HashMap::new()
        };

        Self {
            data: Arc::new(StdRwLock::new(data)),
        }
    }

    /// Create with default configuration.
    pub fn default_config() -> Self {
        Self::new(MemoryBackendConfig::new())
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::default_config()
    }
}

impl StorageBackend for MemoryBackend {
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let mut data = self.data.write().map_err(|e| StorageError::Backend(e.to_string()))?;
        let table_data = data.entry(table.to_string()).or_insert_with(HashMap::new);
        table_data.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let data = self.data.read().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(data
            .get(table)
            .and_then(|t| t.get(key))
            .cloned())
    }

    fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let mut data = self.data.write().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(data
            .get_mut(table)
            .and_then(|t| t.remove(key))
            .is_some())
    }

    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let data = self.data.read().map_err(|e| StorageError::Backend(e.to_string()))?;
        let mut results = Vec::new();

        if let Some(table_data) = data.get(table) {
            for (key, value) in table_data {
                if key.starts_with(prefix) {
                    results.push((key.clone(), value.clone()));
                }
            }
        }

        Ok(results)
    }

    fn write_batch(&self, table: &str, items: Vec<(String, Vec<u8>)>) -> Result<()> {
        let mut data = self.data.write().map_err(|e| StorageError::Backend(e.to_string()))?;
        let table_data = data.entry(table.to_string()).or_insert_with(HashMap::new);

        for (key, value) in items {
            table_data.insert(key, value);
        }

        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

impl MemoryBackend {
    /// Get the number of entries in a table.
    pub fn count(&self, table: &str) -> usize {
        let data = self.data.read().ok();
        data.and_then(|d| d.get(table).map(|m| m.len())).unwrap_or(0)
    }

    /// Clear all data.
    pub fn clear(&self) {
        if let Ok(mut data) = self.data.write() {
            data.clear();
        }
    }

    /// Clear a specific table.
    pub fn clear_table(&self, table: &str) {
        if let Ok(mut data) = self.data.write() {
            data.remove(table);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = MemoryBackendConfig::new().with_capacity(100);
        assert_eq!(config.capacity, Some(100));
    }

    #[test]
    fn test_config_default() {
        let config = MemoryBackendConfig::default();
        assert_eq!(config.capacity, None);
    }

    #[test]
    fn test_memory_backend_basic() {
        let backend = MemoryBackend::default();

        backend.write("test", "key1", b"value1").unwrap();
        assert_eq!(
            backend.read("test", "key1").unwrap(),
            Some(b"value1".to_vec())
        );

        assert!(backend.delete("test", "key1").unwrap());
        assert_eq!(backend.read("test", "key1").unwrap(), None);
    }

    #[test]
    fn test_memory_backend_scan() {
        let backend = MemoryBackend::default();

        backend.write("test", "prefix:key1", b"value1").unwrap();
        backend.write("test", "prefix:key2", b"value2").unwrap();
        backend.write("test", "other:key3", b"value3").unwrap();

        let results = backend.scan("test", "prefix:").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_memory_backend_batch() {
        let backend = MemoryBackend::default();

        let items = vec![
            ("key1".to_string(), b"value1".to_vec()),
            ("key2".to_string(), b"value2".to_vec()),
        ];

        backend.write_batch("test", items).unwrap();
        assert_eq!(backend.read("test", "key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(backend.read("test", "key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_memory_backend_count() {
        let backend = MemoryBackend::default();

        assert_eq!(backend.count("test"), 0);

        backend.write("test", "key1", b"value1").unwrap();
        assert_eq!(backend.count("test"), 1);

        backend.clear_table("test");
        assert_eq!(backend.count("test"), 0);
    }

    #[test]
    fn test_is_not_persistent() {
        let backend = MemoryBackend::default();
        assert!(!backend.is_persistent());
    }
}
