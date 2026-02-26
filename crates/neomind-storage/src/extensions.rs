//! Extension Registry Storage
//!
//! This module provides persistent storage for dynamically loaded extensions,
//! ensuring that registered extensions survive server restarts.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Extensions table: key = extension_id, value = ExtensionRecord (serialized)
const EXTENSIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("extensions");

/// Singleton for extension storage
static EXTENSION_STORE_SINGLETON: StdMutex<Option<Arc<ExtensionStore>>> = StdMutex::new(None);

/// Extension record for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionRecord {
    /// Extension ID
    pub id: String,

    /// Display name
    pub name: String,

    /// File path to the extension binary
    pub file_path: String,

    /// Extension type
    #[serde(rename = "extension_type")]
    pub extension_type: String,

    /// Version
    pub version: String,

    /// Description
    pub description: Option<String>,

    /// Author
    pub author: Option<String>,

    /// Whether to auto-start the extension on server startup
    pub auto_start: bool,

    /// Whether the extension is enabled
    pub enabled: bool,

    /// Whether the extension has been uninstalled by the user
    /// This prevents auto-discovery from re-registering it
    #[serde(default)]
    pub uninstalled: bool,

    /// Extension configuration (key-value pairs)
    /// This config is passed to the extension when loaded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,

    /// Last updated timestamp
    pub updated_at: i64,

    /// Registered at timestamp
    pub registered_at: i64,
}

impl ExtensionRecord {
    /// Create a new extension record
    pub fn new(
        id: String,
        name: String,
        file_path: String,
        extension_type: String,
        version: String,
    ) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id,
            name,
            file_path,
            extension_type,
            version,
            description: None,
            author: None,
            auto_start: false,
            enabled: true,
            uninstalled: false,
            config: None,
            updated_at: now,
            registered_at: now,
        }
    }

    /// Builder pattern: set description
    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    /// Builder pattern: set author
    pub fn with_author(mut self, author: Option<String>) -> Self {
        self.author = author;
        self
    }

    /// Builder pattern: set auto_start
    pub fn with_auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }

    /// Builder pattern: set config
    pub fn with_config(mut self, config: serde_json::Value) -> Self {
        self.config = Some(config);
        self
    }

    /// Builder pattern: set checksum
    pub fn with_checksum(mut self, checksum: Option<String>) -> Self {
        // Note: checksum is stored in config for now
        // In the future, we might add a dedicated checksum field
        if let Some(checksum) = checksum {
            let mut config = self.config.unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = config.as_object_mut() {
                obj.insert("checksum".to_string(), serde_json::Value::String(checksum));
            }
            self.config = Some(config);
        }
        self
    }

    /// Builder pattern: set frontend path
    pub fn with_frontend_path(mut self, frontend_path: Option<String>) -> Self {
        // Note: frontend_path is stored in config for now
        // In the future, we might add a dedicated frontend_path field
        if let Some(frontend_path) = frontend_path {
            let mut config = self.config.unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = config.as_object_mut() {
                obj.insert("frontend_path".to_string(), serde_json::Value::String(frontend_path));
            }
            self.config = Some(config);
        }
        self
    }

    /// Update the timestamp
    pub fn touch(&mut self) {
        self.updated_at = Utc::now().timestamp();
    }

    /// Validate the record
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Extension ID cannot be empty".to_string());
        }

        if self.name.is_empty() {
            return Err("Extension name cannot be empty".to_string());
        }

        if self.file_path.is_empty() {
            return Err("File path cannot be empty".to_string());
        }

        if self.version.is_empty() {
            return Err("Version cannot be empty".to_string());
        }

        Ok(())
    }
}

/// Extension storage
pub struct ExtensionStore {
    db: Arc<Database>,
    /// Path to the database file
    path: String,
}

impl ExtensionStore {
    /// Get or create the extension store singleton
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = EXTENSION_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Use the same database as settings store
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(ExtensionStore {
            db: Arc::new(db),
            path: path_str,
        });

        // Ensure tables exist
        store.ensure_tables()?;

        // Update the singleton
        *EXTENSION_STORE_SINGLETON.lock().unwrap() = Some(store.clone());

        Ok(store)
    }

    /// Ensure all required tables exist
    fn ensure_tables(&self) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let _ = write_txn.open_table(EXTENSIONS_TABLE)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save an extension record
    pub fn save(&self, record: &ExtensionRecord) -> Result<(), Error> {
        record
            .validate()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;

        let mut record = record.clone();
        record.touch();

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(EXTENSIONS_TABLE)?;
            let value =
                serde_json::to_vec(&record).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(record.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load an extension record by ID
    pub fn load(&self, id: &str) -> Result<Option<ExtensionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXTENSIONS_TABLE)?;

        if let Some(data) = table.get(id)? {
            let record: ExtensionRecord = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// Load all extension records
    pub fn load_all(&self) -> Result<Vec<ExtensionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXTENSIONS_TABLE)?;

        let mut records = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            let record: ExtensionRecord = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            records.push(record);
        }

        Ok(records)
    }

    /// Load all auto-start extensions
    pub fn load_auto_start(&self) -> Result<Vec<ExtensionRecord>, Error> {
        let all = self.load_all()?;
        Ok(all
            .into_iter()
            .filter(|r| r.auto_start && r.enabled && !r.uninstalled)
            .collect())
    }

    /// Mark an extension as uninstalled (prevents auto-discovery from re-registering)
    pub fn mark_uninstalled(&self, id: &str) -> Result<bool, Error> {
        if let Some(mut record) = self.load(id)? {
            record.uninstalled = true;
            record.auto_start = false;
            record.touch();
            self.save(&record)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if an extension is marked as uninstalled
    pub fn is_uninstalled(&self, id: &str) -> Result<bool, Error> {
        match self.load(id)? {
            Some(record) => Ok(record.uninstalled),
            None => Ok(false),
        }
    }

    /// Delete an extension record
    pub fn delete(&self, id: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(EXTENSIONS_TABLE)?;
            let result = table.remove(id)?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Check if an extension exists
    pub fn contains(&self, id: &str) -> Result<bool, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXTENSIONS_TABLE)?;
        Ok(table.get(id)?.is_some())
    }

    /// Export all extension records
    pub fn export(&self) -> Result<Vec<ExtensionRecord>, Error> {
        self.load_all()
    }

    /// Import extension records
    pub fn import(&self, records: Vec<ExtensionRecord>) -> Result<(), Error> {
        for record in records {
            self.save(&record)?;
        }
        Ok(())
    }

    /// Get statistics about registered extensions
    pub fn get_stats(&self) -> Result<ExtensionStats, Error> {
        let all = self.load_all()?;

        let total_by_type = all
            .iter()
            .fold(std::collections::HashMap::new(), |mut acc, record| {
                *acc.entry(record.extension_type.clone()).or_insert(0) += 1;
                acc
            });

        let auto_start_count = all.iter().filter(|r| r.auto_start).count();
        let enabled_count = all.iter().filter(|r| r.enabled).count();

        Ok(ExtensionStats {
            total_extensions: all.len(),
            auto_start_count,
            enabled_count,
            total_by_type,
        })
    }
}

/// Statistics about registered extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionStats {
    pub total_extensions: usize,
    pub auto_start_count: usize,
    pub enabled_count: usize,
    pub total_by_type: std::collections::HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_record_creation() {
        let record = ExtensionRecord::new(
            "test.extension".to_string(),
            "Test Extension".to_string(),
            "/path/to/test.so".to_string(),
            "tool".to_string(),
            "0.1.0".to_string(),
        );

        assert_eq!(record.id, "test.extension");
        assert_eq!(record.name, "Test Extension");
        assert!(record.validate().is_ok());
    }

    #[test]
    fn test_extension_record_validation() {
        let mut record = ExtensionRecord::new(
            "test".to_string(),
            "Test".to_string(),
            "/path/to/test.so".to_string(),
            "tool".to_string(),
            "0.1.0".to_string(),
        );

        // Valid record
        assert!(record.validate().is_ok());

        // Empty ID
        record.id = "".to_string();
        assert!(record.validate().is_err());
    }
}
