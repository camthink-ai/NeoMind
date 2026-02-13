//! Business data storage for logs, rules, and alerts.
//!
//! Provides storage for:
//! - Rule execution history
//! - Alert records with status management

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::Result;

// Table definitions
const RULE_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rule_history");
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

/// Rule execution history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecution {
    /// Unique execution ID
    pub id: String,
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub rule_name: String,
    /// Execution timestamp
    pub timestamp: i64,
    /// Trigger source (device_id, manual, etc.)
    pub trigger_source: String,
    /// Execution result
    pub result: RuleExecutionResult,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
    /// Additional context
    pub context: Option<serde_json::Value>,
}

/// Rule execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleExecutionResult {
    /// Rule passed and actions executed
    Success { actions_executed: u32 },
    /// Rule passed but actions failed
    PartialSuccess {
        actions_executed: u32,
        actions_failed: u32,
    },
    /// Rule evaluation failed
    EvaluationFailed,
    /// Rule did not pass
    NotTriggered,
}

/// Rule execution statistics.
#[derive(Debug, Clone, Default)]
pub struct RuleExecutionStats {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful: u64,
    /// Failed executions
    pub failed: u64,
    /// Not triggered count
    pub not_triggered: u64,
    /// Average duration in milliseconds
    pub avg_duration_ms: f64,
}

/// Rule history store.
pub struct RuleHistoryStore {
    db: Arc<Database>,
    stats: Arc<RwLock<HashMap<String, RuleExecutionStats>>>,
}

impl RuleHistoryStore {
    /// Open or create a rule history store.
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

        Ok(Self {
            db: Arc::new(db),
            stats: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Record a rule execution.
    pub fn record_execution(&self, execution: &RuleExecution) -> Result<()> {
        let key = format!("{}:{}", execution.rule_id, execution.id);
        let value = serde_json::to_vec(execution)?;

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(RULE_HISTORY_TABLE)?;
            table.insert(&*key, &*value)?;
        }
        txn.commit()?;

        // Update stats
        let stats = Arc::clone(&self.stats);
        let rule_id = execution.rule_id.clone();
        let duration = execution.duration_ms;
        let result = execution.result.clone();

        tokio::spawn(async move {
            let mut stats = stats.write().await;
            let entry = stats.entry(rule_id).or_insert_with(Default::default);
            entry.total_executions += 1;
            match result {
                RuleExecutionResult::Success { .. } => entry.successful += 1,
                RuleExecutionResult::PartialSuccess { .. } => entry.failed += 1,
                RuleExecutionResult::EvaluationFailed => entry.failed += 1,
                RuleExecutionResult::NotTriggered => entry.not_triggered += 1,
            }
            entry.avg_duration_ms = if entry.total_executions == 1 {
                duration as f64
            } else {
                (entry.avg_duration_ms * (entry.total_executions - 1) as f64 + duration as f64)
                    / entry.total_executions as f64
            };
        });

        Ok(())
    }

    /// Get execution history for a rule.
    pub fn get_history(&self, rule_id: &str, limit: usize) -> Result<Vec<RuleExecution>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RULE_HISTORY_TABLE)?;

        let start_key = format!("{}:", rule_id);
        let end_key = format!("{}:\u{FFFF}", rule_id);

        let mut results = Vec::new();
        for result in table.range(&*start_key..=&*end_key)?.rev().take(limit) {
            let (_key, value) = result?;
            if let Ok(execution) = serde_json::from_slice::<RuleExecution>(value.value()) {
                results.push(execution);
            }
        }

        Ok(results)
    }

    /// Get trigger history for a rule.
    pub fn get_trigger_history(&self, rule_id: &str, limit: usize) -> Result<Vec<RuleExecution>> {
        let history = self.get_history(rule_id, limit)?;
        Ok(history
            .into_iter()
            .filter(|e| !matches!(e.result, RuleExecutionResult::NotTriggered))
            .collect())
    }

    /// Get execution statistics for a rule.
    pub async fn get_stats(&self, rule_id: &str) -> Option<RuleExecutionStats> {
        self.stats.read().await.get(rule_id).cloned()
    }

    /// Get all rules with execution history.
    pub fn list_rules(&self) -> Result<Vec<String>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RULE_HISTORY_TABLE)?;

        let mut rules = std::collections::HashSet::new();
        for result in table.iter()? {
            let (key, _) = result?;
            let key_str = key.value();
            if let Some(rule_id) = key_str.split(':').next() {
                rules.insert(rule_id.to_string());
            }
        }

        Ok(rules.into_iter().collect())
    }

    /// Get count of triggered rules since a given timestamp.
    pub fn count_since(&self, since_timestamp: i64) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RULE_HISTORY_TABLE)?;

        let mut count = 0u64;
        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(execution) = serde_json::from_slice::<RuleExecution>(value.value()) {
                if execution.timestamp >= since_timestamp {
                    // Count only triggered rules (not "NotTriggered")
                    if !matches!(execution.result, RuleExecutionResult::NotTriggered) {
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }
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
