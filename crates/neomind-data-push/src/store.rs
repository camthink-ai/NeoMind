//! Redb persistence for push targets and delivery logs.

use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;

use crate::types::{DeliveryLog, PushTarget};

const TARGETS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("push_targets");
const LOGS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("delivery_logs");

/// Persistent store for data push configuration and logs.
#[derive(Clone)]
pub struct DataPushStore {
    db: Arc<Database>,
}

impl DataPushStore {
    /// Open or create the store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::create(path)?;
        // Create tables
        let write_tx = db.begin_write()?;
        write_tx.open_table(TARGETS_TABLE)?;
        write_tx.open_table(LOGS_TABLE)?;
        write_tx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Create an in-memory store for testing.
    pub fn memory() -> Result<Self> {
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;
        let write_tx = db.begin_write()?;
        write_tx.open_table(TARGETS_TABLE)?;
        write_tx.open_table(LOGS_TABLE)?;
        write_tx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    // ========== Target CRUD ==========

    pub fn save_target(&self, target: &PushTarget) -> Result<()> {
        let write_tx = self.db.begin_write()?;
        {
            let mut table = write_tx.open_table(TARGETS_TABLE)?;
            let json = serde_json::to_string(target)?;
            table.insert(target.id.as_str(), json.as_str())?;
        }
        write_tx.commit()?;
        Ok(())
    }

    pub fn load_target(&self, id: &str) -> Result<Option<PushTarget>> {
        let read_tx = self.db.begin_read()?;
        let table = read_tx.open_table(TARGETS_TABLE)?;
        Ok(table
            .get(id)?
            .and_then(|v| serde_json::from_str(v.value()).ok()))
    }

    pub fn list_targets(&self) -> Result<Vec<PushTarget>> {
        let read_tx = self.db.begin_read()?;
        let table = read_tx.open_table(TARGETS_TABLE)?;
        let mut targets = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            if let Ok(t) = serde_json::from_str::<PushTarget>(value.value()) {
                targets.push(t);
            }
        }
        Ok(targets)
    }

    pub fn delete_target(&self, id: &str) -> Result<bool> {
        let write_tx = self.db.begin_write()?;
        let deleted = {
            let mut table = write_tx.open_table(TARGETS_TABLE)?;
            let existed = table.get(id)?.is_some();
            if existed {
                table.remove(id)?;
            }
            existed
        };
        write_tx.commit()?;
        Ok(deleted)
    }

    // ========== Delivery Logs ==========

    pub fn save_delivery_log(&self, log: &DeliveryLog) -> Result<()> {
        let write_tx = self.db.begin_write()?;
        {
            let mut table = write_tx.open_table(LOGS_TABLE)?;
            let json = serde_json::to_string(log)?;
            table.insert(log.id.as_str(), json.as_str())?;
        }
        write_tx.commit()?;
        Ok(())
    }

    pub fn update_delivery_log(&self, log: &DeliveryLog) -> Result<()> {
        self.save_delivery_log(log)
    }

    pub fn list_delivery_logs(
        &self,
        target_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<DeliveryLog>, usize)> {
        let read_tx = self.db.begin_read()?;
        let table = read_tx.open_table(LOGS_TABLE)?;
        let mut all = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            if let Ok(l) = serde_json::from_str::<DeliveryLog>(value.value()) {
                if l.target_id == target_id {
                    all.push(l);
                }
            }
        }
        // LOGS_TABLE is keyed by UUID (not time), so iteration order is
        // unrelated to recency. Sort newest-first by `created_at` so pagination
        // (skip/take) returns the most recent deliveries first.
        all.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        let total = all.len();
        let logs: Vec<DeliveryLog> = all.into_iter().skip(offset).take(limit).collect();
        Ok((logs, total))
    }

    /// Remove logs older than the given unix timestamp.
    pub fn cleanup_logs(&self, before_ts: i64) -> Result<usize> {
        let write_tx = self.db.begin_write()?;
        let count = {
            let mut table = write_tx.open_table(LOGS_TABLE)?;
            let mut to_remove = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                if let Ok(l) = serde_json::from_str::<DeliveryLog>(value.value()) {
                    if l.created_at < before_ts {
                        to_remove.push(key.value().to_string());
                    }
                }
            }
            let count = to_remove.len();
            for key in &to_remove {
                table.remove(key.as_str())?;
            }
            count
        };
        write_tx.commit()?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_target_crud() {
        let store = DataPushStore::memory().unwrap();
        let target = PushTarget {
            id: "test-1".to_string(),
            name: "Test Target".to_string(),
            enabled: true,
            target_type: PushTargetType::Webhook,
            config: serde_json::json!({"url": "http://example.com"}),
            schedule: PushSchedule::EventDriven {
                event_types: vec!["device_metric".to_string()],
            },
            data_filter: DataSourceFilter {
                source_patterns: vec!["device:s1:".to_string()],
                only_changes: false,
            },
            template: None,
            retry_config: RetryConfig::default(),
            batch_config: BatchConfig::default(),
            created_at: 1700000000,
            updated_at: 1700000000,
        };

        store.save_target(&target).unwrap();
        let loaded = store.load_target("test-1").unwrap().unwrap();
        assert_eq!(loaded.name, "Test Target");

        let targets = store.list_targets().unwrap();
        assert_eq!(targets.len(), 1);

        assert!(store.delete_target("test-1").unwrap());
        assert!(store.load_target("test-1").unwrap().is_none());
    }

    #[test]
    fn test_delivery_logs() {
        let store = DataPushStore::memory().unwrap();
        let log = DeliveryLog {
            id: "log-1".to_string(),
            target_id: "t-1".to_string(),
            status: DeliveryStatus::Success,
            data_source_id: "device:s1:temp".to_string(),
            payload_sent: "{}".to_string(),
            response: None,
            attempts: 1,
            created_at: 1700000000,
            completed_at: Some(1700000001),
            error: None,
        };

        store.save_delivery_log(&log).unwrap();
        let (logs, total) = store.list_delivery_logs("t-1", 10, 0).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(total, 1);

        // Cleanup old logs
        let cleaned = store.cleanup_logs(1700000001).unwrap();
        assert_eq!(cleaned, 1);
    }

    #[test]
    fn test_delivery_logs_sorted_newest_first() {
        // LOGS_TABLE is keyed by UUID (not time), so ordering must come from
        // created_at. Insert with lexicographically-ascending ids but
        // non-monotonic timestamps to prove the sort is by time, not key.
        let store = DataPushStore::memory().unwrap();
        let mk = |id: &str, ts: i64| DeliveryLog {
            id: id.to_string(),
            target_id: "t-1".to_string(),
            status: DeliveryStatus::Success,
            data_source_id: "device:s1:temp".to_string(),
            payload_sent: "{}".to_string(),
            response: None,
            attempts: 1,
            created_at: ts,
            completed_at: None,
            error: None,
        };
        store.save_delivery_log(&mk("a", 1000)).unwrap(); // oldest, but sorts first lexicographically
        store.save_delivery_log(&mk("b", 3000)).unwrap(); // newest
        store.save_delivery_log(&mk("c", 2000)).unwrap(); // middle

        let (logs, total) = store.list_delivery_logs("t-1", 10, 0).unwrap();
        assert_eq!(total, 3);
        // newest-first by created_at — NOT key order (which would be a, b, c)
        assert_eq!(logs[0].id, "b");
        assert_eq!(logs[1].id, "c");
        assert_eq!(logs[2].id, "a");
    }
}
