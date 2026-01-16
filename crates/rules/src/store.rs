//! Rule persistence using redb database.
//!
//! Provides persistent storage for rule definitions and execution history.

use crate::engine::{CompiledRule, RuleId};
use crate::history::RuleHistoryEntry;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

// Table definitions
const RULES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rules");
const RULE_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rule_history");

/// Error type for rule storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Rule not found: {0}")]
    RuleNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// Implement From for all redb error types
impl From<redb::Error> for StoreError {
    fn from(e: redb::Error) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::StorageError> for StoreError {
    fn from(e: redb::StorageError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::DatabaseError> for StoreError {
    fn from(e: redb::DatabaseError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::TableError> for StoreError {
    fn from(e: redb::TableError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::TransactionError> for StoreError {
    fn from(e: redb::TransactionError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::CommitError> for StoreError {
    fn from(e: redb::CommitError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<redb::CompactionError> for StoreError {
    fn from(e: redb::CompactionError) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Serialization(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Configuration for RuleStore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleStoreConfig {
    /// Path to the database file.
    pub path: String,

    /// Create parent directories if they don't exist.
    #[serde(default = "default_create_dirs")]
    pub create_dirs: bool,
}

fn default_create_dirs() -> bool {
    true
}

impl RuleStoreConfig {
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

    /// Create a config for in-memory database (using temp file).
    pub fn memory() -> Self {
        Self {
            path: ":memory:".to_string(),
            create_dirs: false,
        }
    }
}

/// Persistent storage for rules.
pub struct RuleStore {
    db: Arc<Database>,
    /// Storage path for singleton tracking.
    path: String,
    /// Temp file path for cleanup (if using memory mode).
    temp_path: Option<PathBuf>,
}

/// Global rule store singleton to prevent multiple opens.
static RULE_STORE_SINGLETON: StdMutex<Option<Arc<RuleStore>>> = StdMutex::new(None);

impl RuleStore {
    /// Open or create a rule store at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check singleton
        {
            let singleton = RULE_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Open database
        let (db, temp_path) = Self::open_db(&path_str)?;

        let store = Arc::new(RuleStore {
            db: Arc::new(db),
            path: path_str,
            temp_path,
        });

        *RULE_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory store.
    pub fn memory() -> Result<Arc<Self>> {
        let temp_path = std::env::temp_dir().join(format!("rules_store_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    fn open_db(path_str: &str) -> Result<(Database, Option<PathBuf>)> {
        if path_str == ":memory:" {
            // Use temp file for in-memory mode
            let temp_path = std::env::temp_dir().join(format!("rules_store_{}.redb", uuid::Uuid::new_v4()));
            let db = Database::create(&temp_path)?;
            Ok((db, Some(temp_path)))
        } else {
            let path_ref = Path::new(path_str);
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let db = if path_ref.exists() {
                Database::open(path_ref)?
            } else {
                Database::create(path_ref)?
            };
            Ok((db, None))
        }
    }

    /// Save a rule.
    pub fn save(&self, rule: &CompiledRule) -> Result<()> {
        let key = format!("rule:{}", rule.id);
        let value = serde_json::to_vec(rule)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RULES_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a rule by ID.
    pub fn load(&self, id: &RuleId) -> Result<Option<CompiledRule>> {
        let key = format!("rule:{}", id);

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULES_TABLE)?;

        match table.get(key.as_str())? {
            Some(value) => {
                let rule = serde_json::from_slice(value.value())?;
                Ok(Some(rule))
            }
            None => Ok(None),
        }
    }

    /// Delete a rule by ID.
    pub fn delete(&self, id: &RuleId) -> Result<bool> {
        let key = format!("rule:{}", id);

        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(RULES_TABLE)?;
            table.remove(key.as_str())?.is_some()
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// List all rules.
    pub fn list_all(&self) -> Result<Vec<CompiledRule>> {
        let mut rules = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULES_TABLE)?;

        let mut iter = table.iter()?;
        while let Some(result) = iter.next() {
            let (_, value) = result?;
            let rule: CompiledRule = serde_json::from_slice(value.value())?;
            rules.push(rule);
        }

        Ok(rules)
    }

    /// List all rule IDs.
    pub fn list_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULES_TABLE)?;

        let mut iter = table.iter()?;
        while let Some(result) = iter.next() {
            let (key, _) = result?;
            let key_str = key.value();
            if let Some(id) = key_str.strip_prefix("rule:") {
                ids.push(id.to_string());
            }
        }

        Ok(ids)
    }

    /// Get the count of rules.
    pub fn count(&self) -> Result<usize> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULES_TABLE)?;

        // Count manually since len() might not be available
        let mut count = 0;
        let mut iter = table.iter()?;
        while iter.next().is_some() {
            count += 1;
        }
        Ok(count)
    }

    /// Check if a rule exists.
    pub fn exists(&self, id: &RuleId) -> Result<bool> {
        let key = format!("rule:{}", id);

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULES_TABLE)?;

        Ok(table.get(key.as_str())?.is_some())
    }

    /// Save history entry.
    pub fn save_history(&self, entry: &RuleHistoryEntry) -> Result<()> {
        let key = format!("history:{}", entry.id);
        let value = serde_json::to_vec(entry)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RULE_HISTORY_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load history entries for a specific rule.
    pub fn load_history(
        &self,
        rule_id: &RuleId,
        limit: Option<usize>,
    ) -> Result<Vec<RuleHistoryEntry>> {
        let mut entries = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RULE_HISTORY_TABLE)?;

        let rule_id_str = rule_id.to_string();
        let mut iter = table.iter()?;
        while let Some(result) = iter.next() {
            let (_, value) = result?;
            let entry: RuleHistoryEntry = serde_json::from_slice(value.value())?;
            if entry.rule_id == rule_id_str {
                entries.push(entry);
            }
        }

        // Sort by timestamp descending
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if let Some(limit) = limit {
            entries.truncate(limit);
        }

        Ok(entries)
    }

    /// Get history statistics for a rule.
    pub fn get_history_stats(&self, rule_id: &RuleId) -> Result<RuleHistoryStats> {
        let entries = self.load_history(rule_id, None)?;

        if entries.is_empty() {
            return Ok(RuleHistoryStats::default());
        }

        let total = entries.len() as u64;
        let successful = entries.iter().filter(|e| e.success).count() as u64;
        let failed = total - successful;

        let durations: Vec<u64> = entries.iter().map(|e| e.duration_ms).collect();
        let avg = durations.iter().map(|&d| d as f64).sum::<f64>() / durations.len() as f64;
        let min = *durations.iter().min().unwrap_or(&0);
        let max = *durations.iter().max().unwrap_or(&0);

        let last = entries.first().and_then(|e| Some(e.timestamp));
        let first = entries.last().and_then(|e| Some(e.timestamp));

        Ok(RuleHistoryStats {
            total_executions: total,
            successful_executions: successful,
            failed_executions: failed,
            avg_duration_ms: avg,
            min_duration_ms: min,
            max_duration_ms: max,
            last_execution: last,
            first_execution: first,
        })
    }

    /// Clear all history entries for a rule.
    pub fn clear_history(&self, rule_id: &RuleId) -> Result<usize> {
        let rule_id_str = rule_id.to_string();
        let mut removed = 0;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RULE_HISTORY_TABLE)?;
            let mut iter = table.iter()?;
            let mut keys_to_remove = Vec::new();

            while let Some(result) = iter.next() {
                let (key, value) = result?;
                let entry: RuleHistoryEntry = serde_json::from_slice(value.value())?;
                if entry.rule_id == rule_id_str {
                    keys_to_remove.push(key.value().to_string());
                }
            }

            // Drop iterator before mutating
            drop(iter);

            for key in keys_to_remove {
                if table.remove(key.as_str())?.is_some() {
                    removed += 1;
                }
            }
        }
        write_txn.commit()?;

        Ok(removed)
    }

    /// Clear all rules.
    pub fn clear_all(&self) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut rules_table = write_txn.open_table(RULES_TABLE)?;
            let mut history_table = write_txn.open_table(RULE_HISTORY_TABLE)?;

            // Collect keys to remove from rules table
            let mut rule_keys = Vec::new();
            {
                let mut iter = rules_table.iter()?;
                while let Some(result) = iter.next() {
                    let (key, _) = result?;
                    rule_keys.push(key.value().to_string());
                }
            }

            // Remove all rule entries
            for key in rule_keys {
                rules_table.remove(key.as_str())?;
            }

            // Collect keys to remove from history table
            let mut history_keys = Vec::new();
            {
                let mut iter = history_table.iter()?;
                while let Some(result) = iter.next() {
                    let (key, _) = result?;
                    history_keys.push(key.value().to_string());
                }
            }

            // Remove all history entries
            for key in history_keys {
                history_table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Export all rules as JSON.
    pub fn export_json(&self) -> Result<String> {
        let rules = self.list_all()?;
        let export = RulesExport {
            version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            count: rules.len(),
            rules,
        };
        serde_json::to_string_pretty(&export)
            .map_err(|e| StoreError::Serialization(e.to_string()))
    }

    /// Import rules from JSON.
    pub fn import_json(&self, json: &str) -> Result<ImportResult> {
        let import: RulesExport = serde_json::from_str(json)?;
        let mut imported = 0;
        let mut skipped = 0;
        let mut errors = Vec::new();

        for rule in import.rules {
            match self.save(&rule) {
                Ok(_) => imported += 1,
                Err(e) => {
                    errors.push(format!("Rule {}: {}", rule.name, e));
                    skipped += 1;
                }
            }
        }

        Ok(ImportResult {
            imported,
            skipped,
            errors,
        })
    }
}

impl Drop for RuleStore {
    fn drop(&mut self) {
        // Clean up temp file if using memory mode
        if let Some(ref temp_path) = self.temp_path {
            let _ = std::fs::remove_file(temp_path);
        }
    }
}

/// Statistics for rule history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleHistoryStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub avg_duration_ms: f64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub last_execution: Option<chrono::DateTime<chrono::Utc>>,
    pub first_execution: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for RuleHistoryStats {
    fn default() -> Self {
        Self {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            avg_duration_ms: 0.0,
            min_duration_ms: 0,
            max_duration_ms: 0,
            last_execution: None,
            first_execution: None,
        }
    }
}

impl RuleHistoryStats {
    /// Calculate success rate as percentage.
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            return 0.0;
        }
        (self.successful_executions as f64 / self.total_executions as f64) * 100.0
    }
}

/// Export structure for rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesExport {
    pub version: String,
    pub exported_at: String,
    pub count: usize,
    pub rules: Vec<CompiledRule>,
}

/// Result of importing rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// Export format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Yaml,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store() {
        let store = RuleStore::memory().unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_export_import_empty() {
        let store = RuleStore::memory().unwrap();

        // Export empty
        let json = store.export_json().unwrap();
        assert!(json.contains("\"count\": 0"));

        // Import should work even if empty
        let result = store.import_json(&json).unwrap();
        assert_eq!(result.imported, 0);
    }
}
