//! Business data storage for alerts.
//!
//! Provides storage for:
//! - Alert records with status management

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Result;

// Table definitions
const ALERT_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("alerts");

/// Event severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSeverity {
    /// Informational
    Info,
    /// Warning
    Warning,
    /// Error
    Error,
    /// Critical
    Critical,
}

/// Alert record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique alert ID
    pub id: String,
    /// Alert type
    pub alert_type: String,
    /// Alert severity
    pub severity: EventSeverity,
    /// Alert title
    pub title: String,
    /// Alert message
    pub message: String,
    /// Source that triggered the alert
    pub source: String,
    /// Creation timestamp
    pub created_at: i64,
    /// Alert status
    pub status: AlertStatus,
    /// Acknowledged timestamp
    pub acknowledged_at: Option<i64>,
    /// Acknowledged by
    pub acknowledged_by: Option<String>,
    /// Resolved timestamp
    pub resolved_at: Option<i64>,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

/// Alert status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    /// Active (not acknowledged)
    Active,
    /// Acknowledged but not resolved
    Acknowledged,
    /// Resolved
    Resolved,
}

/// Alert filter.
#[derive(Debug, Clone, Default)]
pub struct AlertFilter {
    /// Filter by alert type
    pub alert_types: Vec<String>,
    /// Filter by severity
    pub severities: Vec<EventSeverity>,
    /// Filter by status
    pub statuses: Vec<AlertStatus>,
    /// Filter by source
    pub source: Option<String>,
    /// Start timestamp
    pub start_time: Option<i64>,
    /// End timestamp
    pub end_time: Option<i64>,
    /// Maximum results
    pub limit: Option<usize>,
}

impl AlertFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add alert type filter.
    pub fn with_alert_type(mut self, alert_type: impl Into<String>) -> Self {
        self.alert_types.push(alert_type.into());
        self
    }

    /// Add severity filter.
    pub fn with_severity(mut self, severity: EventSeverity) -> Self {
        self.severities.push(severity);
        self
    }

    /// Add status filter.
    pub fn with_status(mut self, status: AlertStatus) -> Self {
        self.statuses.push(status);
        self
    }

    /// Set source filter.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set time range.
    pub fn with_time_range(mut self, start: i64, end: i64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// Set result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Alert store.
pub struct AlertStore {
    db: Arc<Database>,
}

impl AlertStore {
    /// Open or create an alert store.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| crate::error::Error::Storage(format!("schema version: {e}")))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create a new alert.
    pub fn create(&self, alert: &Alert) -> Result<()> {
        let key = format!("{}:{}", alert.created_at, alert.id);
        let value = serde_json::to_vec(alert)?;

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(ALERT_TABLE)?;
            table.insert(&*key, &*value)?;
        }
        txn.commit()?;

        Ok(())
    }

    /// Get an alert by ID.
    pub fn get(&self, alert_id: &str) -> Result<Option<Alert>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ALERT_TABLE)?;

        let start_key = format!("{}:", i64::MIN);
        let end_key = format!("{}:", i64::MAX);

        for result in table.range(&*start_key..=&*end_key)? {
            let (key, value) = result?;
            let key_str = key.value();
            if key_str.ends_with(&format!(":{}", alert_id)) {
                let alert = serde_json::from_slice(value.value())?;
                return Ok(Some(alert));
            }
        }

        Ok(None)
    }

    /// Query alerts with filter.
    pub fn query(&self, filter: &AlertFilter) -> Result<Vec<Alert>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ALERT_TABLE)?;

        let start_key = filter.start_time.unwrap_or(i64::MIN).to_string();
        let end_key = filter.end_time.unwrap_or(i64::MAX).to_string();

        let mut results = Vec::new();
        for result in table.range(&*start_key..=&*end_key)? {
            let (_key, value) = result?;
            if let Ok(alert) = serde_json::from_slice::<Alert>(value.value()) {
                if self.matches_filter(&alert, filter) {
                    results.push(alert);
                    if let Some(limit) = filter.limit {
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    /// Get active alerts.
    pub fn get_active(&self) -> Result<Vec<Alert>> {
        let filter = AlertFilter::new().with_status(AlertStatus::Active);
        self.query(&filter)
    }

    /// Get alerts by source.
    pub fn get_by_source(&self, source: &str) -> Result<Vec<Alert>> {
        let filter = AlertFilter::new().with_source(source);
        self.query(&filter)
    }

    /// Acknowledge an alert.
    pub fn acknowledge(&self, alert_id: &str, acknowledged_by: &str) -> Result<bool> {
        if let Some(mut alert) = self.get(alert_id)? {
            if alert.status == AlertStatus::Active {
                alert.status = AlertStatus::Acknowledged;
                alert.acknowledged_at = Some(Utc::now().timestamp());
                alert.acknowledged_by = Some(acknowledged_by.to_string());

                let key = format!("{}:{}", alert.created_at, alert.id);
                let value = serde_json::to_vec(&alert)?;

                let txn = self.db.begin_write()?;
                {
                    let mut table = txn.open_table(ALERT_TABLE)?;
                    table.insert(&*key, &*value)?;
                }
                txn.commit()?;

                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Resolve an alert.
    pub fn resolve(&self, alert_id: &str) -> Result<bool> {
        if let Some(mut alert) = self.get(alert_id)? {
            if alert.status != AlertStatus::Resolved {
                alert.status = AlertStatus::Resolved;
                alert.resolved_at = Some(Utc::now().timestamp());

                let key = format!("{}:{}", alert.created_at, alert.id);
                let value = serde_json::to_vec(&alert)?;

                let txn = self.db.begin_write()?;
                {
                    let mut table = txn.open_table(ALERT_TABLE)?;
                    table.insert(&*key, &*value)?;
                }
                txn.commit()?;

                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Delete an alert.
    pub fn delete(&self, alert_id: &str) -> Result<bool> {
        let txn = self.db.begin_write()?;
        let table = txn.open_table(ALERT_TABLE)?;

        let start_key = format!("{}:", i64::MIN);
        let end_key = format!("{}:", i64::MAX);

        let mut key_to_delete: Option<String> = None;
        for result in table.range(&*start_key..=&*end_key)? {
            let (key, _) = result?;
            let key_str = key.value();
            if key_str.ends_with(&format!(":{}", alert_id)) {
                key_to_delete = Some(key_str.to_string());
                break;
            }
        }
        drop(table);

        let found = key_to_delete.is_some();
        if let Some(key) = key_to_delete {
            let mut table = txn.open_table(ALERT_TABLE)?;
            table.remove(&*key)?;
        }

        txn.commit()?;
        Ok(found)
    }

    fn matches_filter(&self, alert: &Alert, filter: &AlertFilter) -> bool {
        if !filter.alert_types.is_empty() && !filter.alert_types.contains(&alert.alert_type) {
            return false;
        }

        if !filter.severities.is_empty() && !filter.severities.contains(&alert.severity) {
            return false;
        }

        if !filter.statuses.is_empty() && !filter.statuses.contains(&alert.status) {
            return false;
        }

        if let Some(ref source) = filter.source {
            if alert.source != *source {
                return false;
            }
        }

        true
    }

    /// Get count of alerts created since a given timestamp.
    pub fn count_since(&self, since_timestamp: i64) -> Result<u64> {
        let filter = AlertFilter::new().with_time_range(since_timestamp, i64::MAX);
        let alerts = self.query(&filter)?;
        Ok(alerts.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_store() {
        // Clean up any existing test database
        let _ = std::fs::remove_file("/tmp/test_alerts.redb");

        let store = AlertStore::open("/tmp/test_alerts.redb").unwrap();

        let alert = Alert {
            id: "alert-1".to_string(),
            alert_type: "high_temp".to_string(),
            severity: EventSeverity::Warning,
            title: "High Temperature".to_string(),
            message: "Temperature exceeds threshold".to_string(),
            source: "sensor-1".to_string(),
            created_at: Utc::now().timestamp(),
            status: AlertStatus::Active,
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            data: None,
        };

        store.create(&alert).unwrap();

        let active = store.get_active().unwrap();
        assert_eq!(active.len(), 1);

        let found = store.get("alert-1").unwrap();
        assert!(found.is_some());
    }
}
