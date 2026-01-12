//! Unified storage backend for all data types.
//!
//! Provides a common interface for different storage implementations
//! (redb for persistent, in-memory for testing).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock as StdRwLock};

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Result;

// Single unified table for all data - using namespaced keys
// Format: "table_name:key"
const UNIFIED_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("unified_storage");

/// Storage backend trait for unified data access.
pub trait StorageBackend: Send + Sync {
    /// Write a value to a key.
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()>;

    /// Read a value by key.
    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete a key.
    fn delete(&self, table: &str, key: &str) -> Result<bool>;

    /// Scan keys with a prefix.
    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>>;

    /// Batch write multiple values.
    fn write_batch(&self, table: &str, items: Vec<(String, Vec<u8>)>) -> Result<()>;

    /// Check if backend supports persistence.
    fn is_persistent(&self) -> bool;
}

/// Create a namespaced key for the unified table.
fn make_key(table: &str, key: &str) -> String {
    format!("{}:{}", table, key)
}

/// Key-value pair for batch operations.
#[derive(Debug, Clone)]
pub struct KvPair {
    /// Key
    pub key: String,
    /// Value
    pub value: Vec<u8>,
}

impl KvPair {
    /// Create a new key-value pair.
    pub fn new(key: impl Into<String>, value: Vec<u8>) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    /// Create from serializable value.
    pub fn from_value<T: Serialize>(key: impl Into<String>, value: &T) -> Result<Self> {
        Ok(Self {
            key: key.into(),
            value: serde_json::to_vec(value)?,
        })
    }
}

/// redb-based persistent storage backend.
pub struct RedbBackend {
    /// redb database instance.
    db: Database,
    /// Storage path.
    path: String,
}

impl RedbBackend {
    /// Open or create a redb backend at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();

        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            // Create parent directory if needed
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };

        Ok(Self {
            db,
            path: path_ref.to_string_lossy().to_string(),
        })
    }

    /// Get the storage path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Compact the database.
    pub fn compact(&mut self) -> Result<()> {
        self.db.compact()?;
        Ok(())
    }
}

impl StorageBackend for RedbBackend {
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(UNIFIED_TABLE)?;
            t.insert(&*namespaced, value)?;
        }
        txn.commit()?;
        Ok(())
    }

    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_read()?;
        let t = txn.open_table(UNIFIED_TABLE)?;

        match t.get(&*namespaced)? {
            Some(value) => Ok(Some(value.value().to_vec())),
            None => Ok(None),
        }
    }

    fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let namespaced = make_key(table, key);
        let txn = self.db.begin_write()?;
        let removed = {
            let mut t = txn.open_table(UNIFIED_TABLE)?;
            t.remove(&*namespaced)?.is_some()
        };
        txn.commit()?;
        Ok(removed)
    }

    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let table_prefix = format!("{}:{}", table, prefix);
        let table_prefix_len = table.len() + 1; // "table:"

        let txn = self.db.begin_read()?;
        let t = txn.open_table(UNIFIED_TABLE)?;

        let mut results = Vec::new();
        for item in t.iter()? {
            let (key, value) = item?;
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
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(UNIFIED_TABLE)?;
            for (key, value) in items {
                let namespaced = make_key(table, &key);
                t.insert(&*namespaced, &*value)?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

/// In-memory storage backend for testing.
pub struct MemoryBackend {
    /// In-memory data storage.
    data: Arc<StdRwLock<HashMap<String, HashMap<String, Vec<u8>>>>>,
}

impl MemoryBackend {
    /// Create a new in-memory backend.
    pub fn new() -> Self {
        Self {
            data: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Get the number of entries in a table.
    pub fn count(&self, table: &str) -> usize {
        let data = self.data.read().unwrap();
        data.get(table).map(|m| m.len()).unwrap_or(0)
    }

    /// Clear all data.
    pub fn clear(&self) {
        let mut data = self.data.write().unwrap();
        data.clear();
    }

    /// Clear a specific table.
    pub fn clear_table(&self, table: &str) {
        let mut data = self.data.write().unwrap();
        data.remove(table);
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for MemoryBackend {
    fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let mut data = self.data.write().unwrap();
        let table_data = data.entry(table.to_string()).or_insert_with(HashMap::new);
        table_data.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let data = self.data.read().unwrap();
        Ok(data
            .get(table)
            .and_then(|t| t.get(key))
            .cloned())
    }

    fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let mut data = self.data.write().unwrap();
        Ok(data
            .get_mut(table)
            .and_then(|t| t.remove(key))
            .is_some())
    }

    fn scan(&self, table: &str, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let data = self.data.read().unwrap();
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
        let mut data = self.data.write().unwrap();
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

/// Unified storage manager that routes to appropriate backend.
pub struct UnifiedStorage {
    /// Backend implementation.
    backend: Arc<dyn StorageBackend>,
}

impl UnifiedStorage {
    /// Create with redb backend.
    pub fn with_redb<P: AsRef<Path>>(path: P) -> Result<Self> {
        let backend = RedbBackend::open(path)?;
        Ok(Self {
            backend: Arc::new(backend),
        })
    }

    /// Create with memory backend.
    pub fn with_memory() -> Self {
        Self {
            backend: Arc::new(MemoryBackend::new()),
        }
    }

    /// Write JSON-serializable data.
    pub fn write_json<T: Serialize>(
        &self,
        table: &str,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let data = serde_json::to_vec(value)?;
        self.backend.write(table, key, &data)
    }

    /// Read and deserialize JSON data.
    pub fn read_json<T: for<'de> Deserialize<'de>>(
        &self,
        table: &str,
        key: &str,
    ) -> Result<Option<T>> {
        match self.backend.read(table, key)? {
            Some(data) => {
                let value = serde_json::from_slice(&data)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Get the underlying backend.
    pub fn backend(&self) -> &Arc<dyn StorageBackend> {
        &self.backend
    }

    /// Check if storage is persistent.
    pub fn is_persistent(&self) -> bool {
        self.backend.is_persistent()
    }
}

/// Device state storage.
pub struct DeviceStateStore {
    storage: UnifiedStorage,
}

impl DeviceStateStore {
    /// Create a new device state store.
    pub fn new(storage: UnifiedStorage) -> Self {
        Self { storage }
    }

    /// Save device state.
    pub fn save_state(&self, device_id: &str, state: &DeviceState) -> Result<()> {
        let key = format!("state:{}", device_id);
        self.storage.write_json("device_state", &key, state)
    }

    /// Load device state.
    pub fn load_state(&self, device_id: &str) -> Result<Option<DeviceState>> {
        let key = format!("state:{}", device_id);
        self.storage.read_json("device_state", &key)
    }

    /// List all device states.
    pub fn list_states(&self) -> Result<Vec<(String, DeviceState)>> {
        let items = self.storage.backend().scan("device_state", "state:")?;
        let mut results = Vec::new();

        for (key, value) in items {
            if let Ok(state) = serde_json::from_slice::<DeviceState>(&value) {
                results.push((key, state));
            }
        }

        Ok(results)
    }
}

/// Device state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// Device ID
    pub device_id: String,
    /// Online status
    pub online: bool,
    /// Last seen timestamp
    pub last_seen: i64,
    /// Additional properties
    #[serde(flatten)]
    pub properties: HashMap<String, serde_json::Value>,
}

impl DeviceState {
    /// Create a new device state.
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            online: false,
            last_seen: 0,
            properties: HashMap::new(),
        }
    }

    /// Set online status.
    pub fn with_online(mut self, online: bool) -> Self {
        self.online = online;
        self
    }

    /// Set last seen timestamp.
    pub fn with_last_seen(mut self, timestamp: i64) -> Self {
        self.last_seen = timestamp;
        self
    }

    /// Add a property.
    pub fn with_property(mut self, key: String, value: serde_json::Value) -> Self {
        self.properties.insert(key, value);
        self
    }
}

/// Configuration storage.
pub struct ConfigStore {
    storage: UnifiedStorage,
}

impl ConfigStore {
    /// Create a new config store.
    pub fn new(storage: UnifiedStorage) -> Self {
        Self { storage }
    }

    /// Set a configuration value.
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.storage.write_json("config", key, value)
    }

    /// Get a configuration value.
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        self.storage.read_json("config", key)
    }

    /// Get a value with default.
    pub fn get_or_default<T: for<'de> Deserialize<'de> + Default>(
        &self,
        key: &str,
    ) -> Result<T> {
        Ok(self.get(key)?.unwrap_or_default())
    }

    /// Delete a configuration value.
    pub fn delete(&self, key: &str) -> Result<bool> {
        self.storage.backend().delete("config", key)
    }

    /// List all configuration keys.
    pub fn list_keys(&self) -> Result<Vec<String>> {
        let items = self.storage.backend().scan("config", "")?;
        Ok(items.into_iter().map(|(k, _)| k).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_backend_basic() {
        let backend = MemoryBackend::new();

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
        let backend = MemoryBackend::new();

        backend.write("test", "prefix:key1", b"value1").unwrap();
        backend.write("test", "prefix:key2", b"value2").unwrap();
        backend.write("test", "other:key3", b"value3").unwrap();

        let results = backend.scan("test", "prefix:").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_memory_backend_batch() {
        let backend = MemoryBackend::new();

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
        let backend = MemoryBackend::new();

        assert_eq!(backend.count("test"), 0);

        backend.write("test", "key1", b"value1").unwrap();
        assert_eq!(backend.count("test"), 1);

        backend.clear_table("test");
        assert_eq!(backend.count("test"), 0);
    }

    #[test]
    fn test_unified_storage_json() {
        let storage = UnifiedStorage::with_memory();

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write_json("test", "key1", &data).unwrap();
        let loaded = storage.read_json::<TestData>("test", "key1").unwrap();

        assert_eq!(loaded, Some(data));
    }

    #[test]
    fn test_device_state_store() {
        let storage = UnifiedStorage::with_memory();
        let store = DeviceStateStore::new(storage);

        let state = DeviceState::new("sensor-1".to_string())
            .with_online(true)
            .with_last_seen(1234567890);

        store.save_state("sensor-1", &state).unwrap();

        let loaded = store.load_state("sensor-1").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.device_id, "sensor-1");
        assert!(loaded.online);
        assert_eq!(loaded.last_seen, 1234567890);
    }

    #[test]
    fn test_config_store() {
        let storage = UnifiedStorage::with_memory();
        let config = ConfigStore::new(storage);

        config.set("timeout", &30).unwrap();
        assert_eq!(config.get::<i32>("timeout").unwrap(), Some(30));

        assert_eq!(config.get_or_default::<i32>("timeout").unwrap(), 30);
        assert_eq!(config.get_or_default::<i32>("notexist").unwrap(), 0);

        let keys = config.list_keys().unwrap();
        assert!(keys.contains(&"timeout".to_string()));

        assert!(config.delete("timeout").unwrap());
        assert_eq!(config.get::<i32>("timeout").unwrap(), None);
    }

    #[test]
    fn test_kv_pair_from_value() {
        #[derive(Serialize)]
        struct Data {
            field: String,
        }

        let data = Data { field: "test".to_string() };
        let kv = KvPair::from_value("key", &data).unwrap();

        assert_eq!(kv.key, "key");
        assert!(!kv.value.is_empty());
    }
}
