//! Remote Instance Storage
//!
//! Stores metadata about remote NeoMind backend instances for multi-backend switching.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Instances table: key = instance_id, value = InstanceRecord (serialized)
const INSTANCES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("instances");

/// Singleton for instance storage
static INSTANCE_STORE_SINGLETON: Mutex<Option<Arc<InstanceStore>>> = Mutex::new(None);

/// A remote NeoMind backend instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRecord {
    /// Unique instance ID ("local-default" for the local instance, UUID for remote)
    pub id: String,

    /// Display name (e.g., "Factory-1")
    pub name: String,

    /// Backend URL (e.g., "http://192.168.1.50:9375")
    pub url: String,

    /// API key for remote instance authentication (nmk_xxx)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Whether this is the local instance (cannot be deleted)
    #[serde(default)]
    pub is_local: bool,

    /// Last known status: "online" | "offline" | "unknown"
    #[serde(default = "default_status")]
    pub last_status: String,

    /// Timestamp of last health check
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,

    /// Creation timestamp
    pub created_at: i64,
}

fn default_status() -> String {
    "unknown".to_string()
}

/// Instance data returned in API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Masked API key for display (e.g. "nmk_abc1****"). The full key is
    /// NEVER returned by the API — the frontend keeps its own copy from
    /// when the user entered it (a hardcoded-XOR "encrypted" round-trip
    /// previously leaked every instance's credential to any caller who
    /// could list instances).
    pub api_key: Option<String>,
    pub is_local: bool,
    pub last_status: String,
    pub last_checked_at: Option<i64>,
    pub created_at: i64,
}

impl InstanceRecord {
    /// Mask the API key for safe display in API responses.
    /// Returns a copy with api_key replaced by "nmk_abc1****" or similar.
    pub fn masked(&self) -> Self {
        Self {
            api_key: self.api_key.as_ref().map(|k| {
                if k.len() > 8 {
                    let end = k.floor_char_boundary(8);
                    format!("{}****", &k[..end])
                } else {
                    "****".to_string()
                }
            }),
            ..self.clone()
        }
    }

    /// Return a response-safe copy with masked api_key. The full key never
    /// leaves the backend (see [`InstanceResponse`]).
    pub fn for_response(&self) -> InstanceResponse {
        InstanceResponse {
            id: self.id.clone(),
            name: self.name.clone(),
            url: self.url.clone(),
            api_key: self.api_key.as_ref().map(|k| {
                if k.len() > 8 {
                    let end = k.floor_char_boundary(8);
                    format!("{}****", &k[..end])
                } else {
                    "****".to_string()
                }
            }),
            is_local: self.is_local,
            last_status: self.last_status.clone(),
            last_checked_at: self.last_checked_at,
            created_at: self.created_at,
        }
    }

    /// Create a new remote instance record
    pub fn new(name: String, url: String, api_key: Option<String>) -> Self {
        let id = format!("inst_{}", uuid::Uuid::new_v4().to_string().split_at(8).0);
        Self {
            id,
            name,
            url,
            api_key,
            is_local: false,
            last_status: default_status(),
            last_checked_at: None,
            created_at: Utc::now().timestamp(),
        }
    }

    /// Create the default local instance record
    pub fn local_default() -> Self {
        Self {
            id: "local-default".to_string(),
            name: "Local".to_string(),
            url: "http://localhost:9375".to_string(),
            api_key: None,
            is_local: true,
            last_status: "online".to_string(),
            last_checked_at: Some(Utc::now().timestamp()),
            created_at: Utc::now().timestamp(),
        }
    }

    /// Validate the instance record
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Name cannot be empty".to_string());
        }

        if self.url.is_empty() {
            return Err("URL cannot be empty".to_string());
        }

        // Validate URL format
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Err("URL must start with http:// or https://".to_string());
        }

        Ok(())
    }
}

/// Instance storage backed by redb
pub struct InstanceStore {
    db: Arc<Database>,
    path: String,
}

impl InstanceStore {
    /// Open or create the instance store
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = INSTANCE_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;

        let store = Arc::new(InstanceStore {
            db: Arc::new(db),
            path: path_str,
        });

        store.ensure_tables()?;

        // Ensure the local default instance exists
        store.ensure_local_default()?;

        *INSTANCE_STORE_SINGLETON.lock() = Some(store.clone());

        Ok(store)
    }

    /// Create an in-memory store (for testing)
    pub fn memory() -> Result<Arc<Self>, Error> {
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;

        let store = Arc::new(InstanceStore {
            db: Arc::new(db),
            path: ":memory:".to_string(),
        });

        store.ensure_tables()?;
        store.ensure_local_default()?;

        Ok(store)
    }

    fn ensure_tables(&self) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let _ = write_txn.open_table(INSTANCES_TABLE)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Ensure the local default instance exists
    fn ensure_local_default(&self) -> Result<(), Error> {
        let exists = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(INSTANCES_TABLE)?;
            table.get("local-default")?.is_some()
        };

        if !exists {
            let local = InstanceRecord::local_default();
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(INSTANCES_TABLE)?;
                let value =
                    serde_json::to_vec(&local).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert("local-default", value.as_slice())?;
            }
            write_txn.commit()?;
        }

        Ok(())
    }

    /// Save an instance record (create or update)
    pub fn save_instance(&self, instance: &InstanceRecord) -> Result<(), Error> {
        instance
            .validate()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(INSTANCES_TABLE)?;
            let value =
                serde_json::to_vec(instance).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(instance.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load an instance by ID
    pub fn load_instance(&self, id: &str) -> Result<Option<InstanceRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(INSTANCES_TABLE)?;

        if let Some(data) = table.get(id)? {
            let instance: InstanceRecord = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(instance))
        } else {
            Ok(None)
        }
    }

    /// Load all instances
    pub fn load_all(&self) -> Result<Vec<InstanceRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(INSTANCES_TABLE)?;

        let mut instances = Vec::new();
        for result in table.iter()? {
            let (_, data) = result?;
            let instance: InstanceRecord = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            instances.push(instance);
        }

        Ok(instances)
    }

    /// Delete an instance by ID. Returns true if the instance existed.
    /// The local instance cannot be deleted.
    pub fn delete_instance(&self, id: &str) -> Result<bool, Error> {
        // Prevent deletion of local instance
        if id == "local-default" {
            return Err(Error::InvalidInput(
                "Cannot delete the local instance".to_string(),
            ));
        }

        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(INSTANCES_TABLE)?;
            let removed = table.remove(id)?;
            removed.is_some()
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Update the status of an instance after a health check
    pub fn update_status(&self, id: &str, status: &str) -> Result<(), Error> {
        let mut instance = self
            .load_instance(id)?
            .ok_or_else(|| Error::NotFound(format!("Instance {}", id)))?;

        instance.last_status = status.to_string();
        instance.last_checked_at = Some(Utc::now().timestamp());
        self.save_instance(&instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_record_validation() {
        let instance = InstanceRecord::new(
            "Test".to_string(),
            "http://192.168.1.50:9375".to_string(),
            None,
        );
        assert!(instance.validate().is_ok());
        assert!(!instance.is_local);
        assert!(instance.id.starts_with("inst_"));
    }

    #[test]
    fn test_local_default() {
        let local = InstanceRecord::local_default();
        assert!(local.is_local);
        assert_eq!(local.id, "local-default");
        assert_eq!(local.url, "http://localhost:9375");
    }

    #[test]
    fn test_store_crud() {
        let store = InstanceStore::memory().unwrap();

        // Local default exists
        let all = store.load_all().unwrap();
        assert!(all.iter().any(|i| i.id == "local-default"));

        // Add remote instance
        let remote = InstanceRecord::new(
            "Factory".to_string(),
            "http://192.168.1.50:9375".to_string(),
            Some("nmk_test123".to_string()),
        );
        let remote_id = remote.id.clone();
        store.save_instance(&remote).unwrap();

        // Load it back
        let loaded = store.load_instance(&remote_id).unwrap().unwrap();
        assert_eq!(loaded.name, "Factory");
        assert_eq!(loaded.api_key, Some("nmk_test123".to_string()));

        // Update status
        store.update_status(&remote_id, "online").unwrap();
        let updated = store.load_instance(&remote_id).unwrap().unwrap();
        assert_eq!(updated.last_status, "online");
        assert!(updated.last_checked_at.is_some());

        // Delete remote
        assert!(store.delete_instance(&remote_id).unwrap());

        // Cannot delete local
        assert!(store.delete_instance("local-default").is_err());
    }
}
