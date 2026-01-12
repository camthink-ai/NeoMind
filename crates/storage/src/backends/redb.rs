//! Redb storage backend implementation.
//!
//! Provides persistent storage using the redb embedded database.

use edge_ai_core::storage::{StorageBackend, StorageError, Result as CoreResult};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;

type Result<T> = CoreResult<T>;

// Single unified table for all data - using namespaced keys
// Format: "table_name:key"
const UNIFIED_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("unified_storage");

/// Configuration for RedbBackend.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RedbBackendConfig {
    /// Path to the database file.
    pub path: String,

    /// Create parent directories if they don't exist.
    #[serde(default = "default_create_dirs")]
    pub create_dirs: bool,
}

fn default_create_dirs() -> bool {
    true
}

impl RedbBackendConfig {
    /// Create a new config with the given path.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            create_dirs: true,
        }
    }

    /// Set whether to create parent directories.
    pub fn with_create_dirs(mut self, create_dirs: bool) -> Self {
        self.create_dirs = create_dirs;
        self
    }

    /// Create a config for in-memory database.
    pub fn memory() -> Self {
        Self {
            path: ":memory:".to_string(),
            create_dirs: false,
        }
    }
}

/// Create a namespaced key for the unified table.
fn make_key(table: &str, key: &str) -> String {
    format!("{}:{}", table, key)
}

/// redb-based persistent storage backend.
pub struct RedbBackend {
    /// redb database instance.
    db: Arc<Database>,
    /// Storage path.
    path: String,
}

impl RedbBackend {
    /// Create a new RedbBackend with the given configuration.
    pub fn new(config: RedbBackendConfig) -> Result<Self> {
        let path = &config.path;

        let db = if path == ":memory:" {
            // redb doesn't support true in-memory databases.
            // Use a temporary file instead.
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("redb_{}", uuid::Uuid::new_v4()));
            Database::create(&temp_path)
                .map_err(|e| StorageError::Backend(e.to_string()))?
        } else {
            let path_ref = Path::new(path);
            if config.create_dirs {
                if let Some(parent) = path_ref.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| StorageError::Io(e))?;
                }
            }

            if path_ref.exists() {
                Database::open(path_ref)
                    .map_err(|e| StorageError::Backend(e.to_string()))?
            } else {
                Database::create(path_ref)
                    .map_err(|e| StorageError::Backend(e.to_string()))?
            }
        };

        Ok(Self {
            db: Arc::new(db),
            path: config.path,
        })
    }

    /// Open or create a redb backend at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new(RedbBackendConfig::new(path.as_ref().to_string_lossy().to_string()))
    }

    /// Get the storage path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Compact the database.
    pub fn compact(&self) -> Result<()> {
        // Note: compact() is a method on the mutable Database reference,
        // but we store Arc<Database>. For now, we'll skip this.
        // In a future version, we might need to reconsider the Arc usage
        // or provide an alternative method.
        Ok(())
    }
}

impl StorageBackend for RedbBackend {
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_write().map_err(|e| StorageError::Backend(e.to_string()))?;
        {
            let mut t = txn.open_table(UNIFIED_TABLE).map_err(|e| StorageError::Backend(e.to_string()))?;
            t.insert(&*namespaced, value).map_err(|e| StorageError::Backend(e.to_string()))?;
        }
        txn.commit().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(())
    }

    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_read().map_err(|e| StorageError::Backend(e.to_string()))?;
        let t = txn.open_table(UNIFIED_TABLE).map_err(|e| StorageError::Backend(e.to_string()))?;

        match t.get(&*namespaced).map_err(|e| StorageError::Backend(e.to_string()))? {
            Some(value) => Ok(Some(value.value().to_vec())),
            None => Ok(None),
        }
    }

    fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_write().map_err(|e| StorageError::Backend(e.to_string()))?;
        let removed = {
            let mut t = txn.open_table(UNIFIED_TABLE).map_err(|e| StorageError::Backend(e.to_string()))?;
            t.remove(&*namespaced).map_err(|e| StorageError::Backend(e.to_string()))?.is_some()
        };
        txn.commit().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(removed)
    }

    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let table_prefix = format!("{}:{}", table, prefix);
        let table_prefix_len = table.len() + 1; // "table:"

        let txn = self.db.begin_read().map_err(|e| StorageError::Backend(e.to_string()))?;
        let t = txn.open_table(UNIFIED_TABLE).map_err(|e| StorageError::Backend(e.to_string()))?;

        let mut results = Vec::new();
        for item in t.iter().map_err(|e| StorageError::Backend(e.to_string()))? {
            let (key, value) = item.map_err(|e| StorageError::Backend(e.to_string()))?;
            let key_str = key.value();
            if key_str.starts_with(&table_prefix) {
                // Extract the original key (remove table: prefix)
                if let Some(rest) = key_str.get(table_prefix_len..) {
                    results.push((rest.to_string(), value.value().to_vec()));
                }
            }
        }

        Ok(results)
    }

    fn write_batch(&self, table: &str, items: Vec<(String, Vec<u8>)>) -> Result<()> {
        let txn = self.db.begin_write().map_err(|e| StorageError::Backend(e.to_string()))?;
        {
            let mut t = txn.open_table(UNIFIED_TABLE).map_err(|e| StorageError::Backend(e.to_string()))?;
            for (key, value) in items {
                let namespaced = make_key(table, &key);
                t.insert(&*namespaced, &*value).map_err(|e| StorageError::Backend(e.to_string()))?;
            }
        }
        txn.commit().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        self.path != ":memory:"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = RedbBackendConfig::new("./data/test.db")
            .with_create_dirs(false);

        assert_eq!(config.path, "./data/test.db");
        assert!(!config.create_dirs);
    }

    #[test]
    fn test_config_memory() {
        let config = RedbBackendConfig::memory();
        assert_eq!(config.path, ":memory:");
    }

    #[test]
    fn test_make_key() {
        assert_eq!(make_key("users", "123"), "users:123");
    }
}
