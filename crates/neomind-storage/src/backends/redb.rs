//! Redb storage backend implementation.
//!
//! Provides persistent storage using the redb embedded database.

use lru::LruCache;
use neomind_core::storage::{Result as CoreResult, StorageBackend, StorageError};
use redb::{Database, ReadableTable, TableDefinition};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock as StdRwLock};

type Result<T> = CoreResult<T>;

// Single unified table for all data - using namespaced keys
// Format: "table_name:key"
const UNIFIED_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("unified_storage");

// Default cache capacity - number of entries
const DEFAULT_CACHE_CAPACITY: usize = 1024;

/// Configuration for RedbBackend.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RedbBackendConfig {
    /// Path to the database file.
    pub path: String,

    /// Create parent directories if they don't exist.
    #[serde(default = "default_create_dirs")]
    pub create_dirs: bool,

    /// LRU cache capacity (number of entries). 0 to disable caching.
    #[serde(default = "default_cache_capacity")]
    pub cache_capacity: usize,
}

fn default_create_dirs() -> bool {
    true
}

fn default_cache_capacity() -> usize {
    DEFAULT_CACHE_CAPACITY
}

impl RedbBackendConfig {
    /// Create a new config with the given path.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            create_dirs: true,
            cache_capacity: DEFAULT_CACHE_CAPACITY,
        }
    }

    /// Set whether to create parent directories.
    pub fn with_create_dirs(mut self, create_dirs: bool) -> Self {
        self.create_dirs = create_dirs;
        self
    }

    /// Set the cache capacity.
    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.cache_capacity = capacity;
        self
    }

    /// Create a config for in-memory database.
    pub fn memory() -> Self {
        Self {
            path: ":memory:".to_string(),
            create_dirs: false,
            cache_capacity: 512, // Smaller cache for in-memory
        }
    }
}

/// Create a namespaced key for the unified table.
/// Optimized with pre-allocated capacity to reduce reallocations.
fn make_key(table: &str, key: &str) -> String {
    // Pre-allocate exact capacity needed: table + ':' + key
    let mut result = String::with_capacity(table.len() + key.len() + 1);
    result.push_str(table);
    result.push(':');
    result.push_str(key);
    result
}

/// redb-based persistent storage backend with optional LRU cache.
pub struct RedbBackend {
    /// redb database instance.
    db: Arc<Database>,
    /// Storage path (":memory:" for in-memory).
    path: String,
    /// Actual file path for temporary databases (for cleanup).
    temp_path: Option<PathBuf>,
    /// LRU cache for frequently accessed keys (namespaced).
    /// Uses std::sync::RwLock for compatibility with sync trait methods.
    cache: Arc<StdRwLock<LruCache<String, Vec<u8>>>>,
}

impl RedbBackend {
    /// Create a new RedbBackend with the given configuration.
    pub fn new(config: RedbBackendConfig) -> Result<Self> {
        let path = &config.path;

        let (db, temp_path) = if path == ":memory:" {
            // redb doesn't support true in-memory databases.
            // Use a temporary file instead.
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("redb_{}", uuid::Uuid::new_v4()));
            let db =
                Database::create(&temp_path).map_err(|e| StorageError::Backend(e.to_string()))?;
            (db, Some(temp_path))
        } else {
            let path_ref = Path::new(path);
            if config.create_dirs {
                if let Some(parent) = path_ref.parent() {
                    std::fs::create_dir_all(parent).map_err(StorageError::Io)?;
                }
            }

            let db = if path_ref.exists() {
                Database::open(path_ref).map_err(|e| StorageError::Backend(e.to_string()))?
            } else {
                Database::create(path_ref).map_err(|e| StorageError::Backend(e.to_string()))?
            };
            (db, None)
        };

        // Initialize LRU cache with configured capacity
        let cache = if config.cache_capacity > 0 {
            LruCache::new(NonZeroUsize::new(config.cache_capacity).expect("capacity > 0"))
        } else {
            LruCache::new(NonZeroUsize::new(1).expect("1 > 0"))
        };

        let cache = Arc::new(StdRwLock::new(cache));

        Ok(Self {
            db: Arc::new(db),
            path: config.path,
            temp_path,
            cache,
        })
    }

    /// Open or create a redb backend at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new(RedbBackendConfig::new(
            path.as_ref().to_string_lossy().to_string(),
        ))
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

        // Update cache (write-through)
        if let Ok(mut cache) = self.cache.write() {
            cache.put(namespaced.clone(), value.to_vec());
        }

        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        {
            let mut t = txn
                .open_table(UNIFIED_TABLE)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            t.insert(&*namespaced, value)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(())
    }

    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let namespaced = make_key(table, key);

        // Try cache first - use write lock since get() updates LRU position
        if let Ok(mut cache) = self.cache.write() {
            if let Some(cached) = cache.get(&namespaced) {
                return Ok(Some(cached.clone()));
            }
        }

        // Cache miss - read from database
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let t = txn
            .open_table(UNIFIED_TABLE)
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        match t
            .get(&*namespaced)
            .map_err(|e| StorageError::Backend(e.to_string()))?
        {
            Some(value) => {
                let data = value.value().to_vec();
                // Populate cache for future reads
                if let Ok(mut cache) = self.cache.write() {
                    cache.put(namespaced, data.clone());
                }
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let namespaced = make_key(table, key);

        // Remove from cache
        if let Ok(mut cache) = self.cache.write() {
            cache.pop(&namespaced);
        }

        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let removed = {
            let mut t = txn
                .open_table(UNIFIED_TABLE)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            let result = t
                .remove(&*namespaced)
                .map_err(|e| StorageError::Backend(e.to_string()))?
                .is_some();
            result
        };
        txn.commit()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(removed)
    }

    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let table_prefix = format!("{}:{}", table, prefix);
        let table_prefix_len = table.len() + 1; // "table:"

        let txn = self
            .db
            .begin_read()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let t = txn
            .open_table(UNIFIED_TABLE)
            .map_err(|e| StorageError::Backend(e.to_string()))?;

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
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        {
            let mut t = txn
                .open_table(UNIFIED_TABLE)
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            for (key, value) in items {
                let namespaced = make_key(table, &key);
                t.insert(&*namespaced, &*value)
                    .map_err(|e| StorageError::Backend(e.to_string()))?;
            }
        }
        txn.commit()
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        self.path != ":memory:"
    }
}

/// Cleanup temporary database file when RedbBackend is dropped.
impl Drop for RedbBackend {
    fn drop(&mut self) {
        if let Some(temp_path) = &self.temp_path {
            // Clean up the temporary file
            if let Err(e) = std::fs::remove_file(temp_path) {
                // Log warning but don't panic - cleanup failures are non-critical
                tracing::debug!(
                    "Failed to remove temporary database file {}: {}",
                    temp_path.display(),
                    e
                );
            }
            // Also try to remove the -journal file if it exists
            let journal_path = temp_path.with_extension("redb-journal");
            if journal_path.exists() {
                let _ = std::fs::remove_file(journal_path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = RedbBackendConfig::new("./data/test.db").with_create_dirs(false);

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
