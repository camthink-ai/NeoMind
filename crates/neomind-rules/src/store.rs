//! Rule persistence using redb database.
//!
//! Provides persistent storage for rule definitions and execution history.

use crate::models::{CompiledRule, RuleId};
use parking_lot::Mutex;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Table definitions
const RULES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rules");
const HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rule_history");

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

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Serialization(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Persistent storage for rules.
pub struct RuleStore {
    db: Arc<Database>,
    /// Storage path for singleton tracking.
    path: String,
    /// Temp file path for cleanup (if using memory mode).
    temp_path: Option<PathBuf>,
}

/// Global rule store singleton to prevent multiple opens.
static RULE_STORE_SINGLETON: Mutex<Option<Arc<RuleStore>>> = Mutex::new(None);

impl RuleStore {
    /// Open or create a rule store at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check singleton
        {
            let singleton = RULE_STORE_SINGLETON.lock();
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

        *RULE_STORE_SINGLETON.lock() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory store.
    pub fn memory() -> Result<Arc<Self>> {
        let temp_path =
            std::env::temp_dir().join(format!("rules_store_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    fn open_db(path_str: &str) -> Result<(Database, Option<PathBuf>)> {
        let (db, temp_path) = if path_str == ":memory:" {
            // Use temp file for in-memory mode
            let temp_path =
                std::env::temp_dir().join(format!("rules_store_{}.redb", uuid::Uuid::new_v4()));
            let db = Database::create(&temp_path)?;
            (db, Some(temp_path))
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
            (db, None)
        };

        Ok((db, temp_path))
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
            let result = table.remove(key.as_str())?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// List all rules.
    pub fn list_all(&self) -> Result<Vec<CompiledRule>> {
        let mut rules = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(RULES_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(rules), // Table doesn't exist yet
        };

        let iter = table.iter()?;
        for result in iter {
            let (_, value) = result?;
            let rule: CompiledRule = serde_json::from_slice(value.value())?;
            rules.push(rule);
        }

        Ok(rules)
    }

    /// Save an execution result to history.
    pub fn save_history(&self, result: &crate::models::RuleExecutionResult) -> Result<()> {
        // Key: timestamp + rule_id for ordering
        let key = format!(
            "history:{}:{}",
            result.triggered_at.timestamp_millis(),
            result.rule_id
        );
        let value = serde_json::to_vec(result)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load execution history for a specific rule.
    pub fn load_history(
        &self,
        rule_id: &RuleId,
    ) -> Result<Vec<crate::models::RuleExecutionResult>> {
        let mut results = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(HISTORY_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(results), // Table doesn't exist yet
        };

        let iter = table.iter()?;
        for item in iter {
            let (_, value) = item?;
            let entry: crate::models::RuleExecutionResult = serde_json::from_slice(value.value())?;
            if &entry.rule_id == rule_id {
                results.push(entry);
            }
        }

        // Sort by triggered_at descending (most recent first)
        results.sort_by_key(|r| std::cmp::Reverse(r.triggered_at));
        Ok(results)
    }

    /// Count history entries since a timestamp (only actual triggers with executed actions).
    pub fn count_history_since(&self, since_timestamp: i64) -> Result<u64> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(HISTORY_TABLE) {
            Ok(t) => t,
            Err(_) => return Ok(0),
        };

        let mut count = 0u64;
        for result in table.iter()? {
            let (_, value) = result?;
            if let Ok(entry) =
                serde_json::from_slice::<crate::models::RuleExecutionResult>(value.value())
            {
                if entry.triggered_at.timestamp() >= since_timestamp
                    && !entry.actions_executed.is_empty()
                {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Clean up old history entries (older than the given number of days).
    pub fn cleanup_history(&self, older_than_days: u64) -> Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days as i64);
        let cutoff_key = format!("history:{}:", cutoff.timestamp_millis());

        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(HISTORY_TABLE)?;
            let keys_to_remove: Vec<String> = table
                .iter()?
                .filter_map(|item| {
                    let (key, _) = item.ok()?;
                    let key_str = key.value().to_string();
                    if key_str < cutoff_key {
                        Some(key_str)
                    } else {
                        None
                    }
                })
                .collect();

            let count = keys_to_remove.len();
            for key in &keys_to_remove {
                table.remove(key.as_str())?;
            }
            count
        };
        write_txn.commit()?;
        Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RuleAction, RuleExecutionResult, RuleTrigger};
    use chrono::Utc;

    /// A minimal valid rule: schedule-triggered, unconditional, one notify
    /// action. Exercises the serde round-trip through the store's bincode/
    /// JSON encoding without pulling in engine evaluation.
    fn sample_rule(name: &str) -> CompiledRule {
        let mut rule = CompiledRule::new(name);
        rule.trigger = RuleTrigger::Schedule {
            cron: "*/5 * * * *".to_string(),
        };
        rule.actions = vec![RuleAction::Notify {
            message: "temperature high".to_string(),
            severity: Default::default(),
        }];
        rule
    }

    #[test]
    fn save_load_roundtrip_preserves_rule() {
        let store = RuleStore::memory().unwrap();
        let rule = sample_rule("roundtrip");
        let id = rule.id.clone();

        store.save(&rule).unwrap();
        let loaded = store.load(&id).unwrap().expect("rule must load back");

        assert_eq!(loaded.name, "roundtrip");
        assert!(loaded.enabled);
        match (&loaded.trigger, &rule.trigger) {
            (RuleTrigger::Schedule { cron: a }, RuleTrigger::Schedule { cron: b }) => {
                assert_eq!(a, b)
            }
            _ => panic!("trigger variant changed across roundtrip"),
        }
        assert_eq!(loaded.actions.len(), 1);
    }

    #[test]
    fn rules_persist_across_store_reopen() {
        // The whole point of the redb store: a rule saved by one process
        // instance must survive into the next (server restart).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.redb");

        let rule = sample_rule("survivor");
        let id = rule.id.clone();
        {
            let store = RuleStore::open(&path).unwrap();
            store.save(&rule).unwrap();
        }
        let reopened = RuleStore::open(&path).unwrap();
        assert!(reopened.load(&id).unwrap().is_some());
        assert_eq!(reopened.list_all().unwrap().len(), 1);
    }

    #[test]
    fn enabled_flag_persists_through_save_load() {
        let store = RuleStore::memory().unwrap();
        let mut rule = sample_rule("toggleable");
        let id = rule.id.clone();
        store.save(&rule).unwrap();

        rule.enabled = false;
        store.save(&rule).unwrap();
        assert!(!store.load(&id).unwrap().unwrap().enabled);

        // list_all reflects the latest state, and there is exactly one row.
        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 1);
        assert!(!all[0].enabled);
    }

    #[test]
    fn delete_removes_once_and_load_missing_is_none() {
        let store = RuleStore::memory().unwrap();
        let rule = sample_rule("deletable");
        let id = rule.id.clone();
        store.save(&rule).unwrap();

        assert!(store.delete(&id).unwrap(), "first delete must report hit");
        assert!(!store.delete(&id).unwrap(), "second delete must miss");
        assert!(store.load(&id).unwrap().is_none());
    }

    #[test]
    fn history_save_load_counts_and_cleans_up() {
        let store = RuleStore::memory().unwrap();
        let rule = sample_rule("historical");
        store.save(&rule).unwrap();

        let now = Utc::now();
        let mk_result = |at: chrono::DateTime<Utc>, executed: bool| RuleExecutionResult {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            success: true,
            actions_executed: if executed {
                vec!["notify".to_string()]
            } else {
                vec![]
            },
            error: None,
            duration_ms: 3,
            triggered_at: at,
        };

        // Recent trigger WITH executed actions, old trigger with actions
        // (outside the count window), recent no-op (never counted).
        // Timestamps differ by ≥1 ms: the history key is
        // {millis}:{rule_id}, so same-millis entries for one rule collide
        // (last write wins) — deliberate here to exercise distinct rows.
        store.save_history(&mk_result(now, true)).unwrap();
        store
            .save_history(&mk_result(now - chrono::Duration::days(30), true))
            .unwrap();
        store
            .save_history(&mk_result(now - chrono::Duration::milliseconds(1), false))
            .unwrap();

        let history = store.load_history(&rule.id).unwrap();
        assert_eq!(history.len(), 3, "load_history returns all entries");
        assert!(
            history[0].triggered_at >= history[1].triggered_at,
            "history must sort most-recent-first"
        );

        let since = (now - chrono::Duration::days(1)).timestamp();
        assert_eq!(
            store.count_history_since(since).unwrap(),
            1,
            "only recent entries with executed actions count"
        );

        // Cleanup drops entries older than N days.
        let removed = store.cleanup_history(7).unwrap();
        assert_eq!(removed, 1, "the 30-day-old entry must be removed");
        assert_eq!(store.load_history(&rule.id).unwrap().len(), 2);
    }
}
