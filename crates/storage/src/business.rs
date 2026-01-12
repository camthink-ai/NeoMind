//! Business data storage for logs, rules, workflows, and alerts.
//!
//! Provides storage for:
//! - Event logs with circular buffer and retention
//! - Rule execution history
//! - Workflow execution history with step details
//! - Alert records with status management

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use chrono::Utc;

use crate::Result;

// Table definitions
const EVENT_LOG_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("event_log");
const RULE_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("rule_history");
const WORKFLOW_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("workflow_history");
const ALERT_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("alerts");

// Default retention period: 7 days
const DEFAULT_RETENTION_DAYS: i64 = 7;

/// Event log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLog {
    /// Unique event ID
    pub id: String,
    /// Event type (e.g., "device_connected", "rule_triggered", "error")
    pub event_type: String,
    /// Event source (e.g., device_id, rule_id)
    pub source: Option<String>,
    /// Event severity (info, warning, error, critical)
    pub severity: EventSeverity,
    /// Event timestamp
    pub timestamp: i64,
    /// Event message
    pub message: String,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

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

/// Event log filter.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by event type
    pub event_types: Vec<String>,
    /// Filter by severity
    pub severities: Vec<EventSeverity>,
    /// Filter by source
    pub source: Option<String>,
    /// Start timestamp (inclusive)
    pub start_time: Option<i64>,
    /// End timestamp (inclusive)
    pub end_time: Option<i64>,
    /// Maximum results
    pub limit: Option<usize>,
}

impl EventFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add event type filter.
    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_types.push(event_type.into());
        self
    }

    /// Add severity filter.
    pub fn with_severity(mut self, severity: EventSeverity) -> Self {
        self.severities.push(severity);
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

/// Event log store with circular retention.
pub struct EventLogStore {
    db: Arc<Database>,
    retention_days: i64,
}

impl EventLogStore {
    /// Open or create an event log store.
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
            retention_days: DEFAULT_RETENTION_DAYS,
        })
    }

    /// Set retention period in days.
    pub fn with_retention_days(mut self, days: i64) -> Self {
        self.retention_days = days.max(1);
        self
    }

    /// Write an event log entry.
    pub fn write(&self, event: &EventLog) -> Result<()> {
        let key = format!("{}:{}", event.timestamp, event.id);
        let value = serde_json::to_vec(event)?;

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(EVENT_LOG_TABLE)?;
            table.insert(&*key, &*value)?;
        }
        txn.commit()?;

        Ok(())
    }

    /// Query events with filter.
    pub fn query(&self, filter: &EventFilter) -> Result<Vec<EventLog>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EVENT_LOG_TABLE)?;

        let cutoff_timestamp = Utc::now().timestamp() - (self.retention_days * 86400);
        let mut results = Vec::new();

        let start_key = filter.start_time.unwrap_or(cutoff_timestamp).to_string();
        let end_key = filter.end_time.unwrap_or(i64::MAX).to_string();

        for result in table.range(&*start_key..=&*end_key)? {
            let (_key, value) = result?;
            if let Ok(event) = serde_json::from_slice::<EventLog>(value.value()) {
                if self.matches_filter(&event, filter) {
                    results.push(event);
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

    /// Get events by type.
    pub fn get_by_type(&self, event_type: &str, limit: Option<usize>) -> Result<Vec<EventLog>> {
        let filter = EventFilter::new()
            .with_event_type(event_type)
            .with_limit(limit.unwrap_or(100));
        self.query(&filter)
    }

    /// Get events by source.
    pub fn get_by_source(&self, source: &str, limit: Option<usize>) -> Result<Vec<EventLog>> {
        let filter = EventFilter::new()
            .with_source(source)
            .with_limit(limit.unwrap_or(100));
        self.query(&filter)
    }

    /// Clean up old events beyond retention period.
    pub fn cleanup_old_events(&self) -> Result<usize> {
        let cutoff = Utc::now().timestamp() - (self.retention_days * 86400);

        let write_txn = self.db.begin_write()?;
        let mut count = 0;

        {
            let mut table = write_txn.open_table(EVENT_LOG_TABLE)?;
            let start_key = i64::MIN.to_string();
            let end_key = cutoff.to_string();

            let mut keys_to_delete: Vec<String> = Vec::new();
            let mut range = table.range(&*start_key..=&*end_key)?;
            while let Some(result) = range.next() {
                let (key_ref, _) = result?;
                keys_to_delete.push(key_ref.value().to_string());
            }
            drop(range);

            for key in &keys_to_delete {
                table.remove(&**key)?;
                count += 1;
            }
        }

        write_txn.commit()?;
        Ok(count)
    }

    fn matches_filter(&self, event: &EventLog, filter: &EventFilter) -> bool {
        if !filter.event_types.is_empty() && !filter.event_types.contains(&event.event_type) {
            return false;
        }

        if !filter.severities.is_empty() && !filter.severities.contains(&event.severity) {
            return false;
        }

        if let Some(ref source) = filter.source {
            if event.source.as_ref().map(|s| s == source).unwrap_or(false) {
                // matches
            } else {
                return false;
            }
        }

        true
    }
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
    PartialSuccess { actions_executed: u32, actions_failed: u32 },
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
}

/// Workflow execution history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Unique execution ID
    pub id: String,
    /// Workflow ID
    pub workflow_id: String,
    /// Workflow name
    pub workflow_name: String,
    /// Execution timestamp
    pub timestamp: i64,
    /// Execution status
    pub status: WorkflowStatus,
    /// Trigger source
    pub trigger_source: String,
    /// Step executions
    pub steps: Vec<StepExecution>,
    /// Input data
    pub input: Option<serde_json::Value>,
    /// Output data
    pub output: Option<serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Workflow execution status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Running
    Running,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
    /// Partially completed
    Partial,
}

/// Step execution detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecution {
    /// Step ID
    pub step_id: String,
    /// Step name
    pub name: String,
    /// Step type
    pub step_type: String,
    /// Step status
    pub status: WorkflowStatus,
    /// Start timestamp
    pub start_time: i64,
    /// End timestamp
    pub end_time: Option<i64>,
    /// Input data
    pub input: Option<serde_json::Value>,
    /// Output data
    pub output: Option<serde_json::Value>,
    /// Error message
    pub error: Option<String>,
}

/// Workflow history store.
pub struct WorkflowHistoryStore {
    db: Arc<Database>,
}

impl WorkflowHistoryStore {
    /// Open or create a workflow history store.
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
        })
    }

    /// Record a workflow execution.
    pub fn record_execution(&self, execution: &WorkflowExecution) -> Result<()> {
        let key = format!("{}:{}", execution.workflow_id, execution.id);
        let value = serde_json::to_vec(execution)?;

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(WORKFLOW_HISTORY_TABLE)?;
            table.insert(&*key, &*value)?;
        }
        txn.commit()?;

        Ok(())
    }

    /// Update an existing workflow execution.
    pub fn update_execution(&self, execution: &WorkflowExecution) -> Result<()> {
        self.record_execution(execution)
    }

    /// Get execution history for a workflow.
    pub fn get_history(&self, workflow_id: &str, limit: usize) -> Result<Vec<WorkflowExecution>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(WORKFLOW_HISTORY_TABLE)?;

        let start_key = format!("{}:", workflow_id);
        let end_key = format!("{}:\u{FFFF}", workflow_id);

        let mut results = Vec::new();
        for result in table.range(&*start_key..=&*end_key)?.rev().take(limit) {
            let (_key, value) = result?;
            if let Ok(execution) = serde_json::from_slice::<WorkflowExecution>(value.value()) {
                results.push(execution);
            }
        }

        Ok(results)
    }

    /// Get a specific execution by ID.
    pub fn get_execution(&self, workflow_id: &str, execution_id: &str) -> Result<Option<WorkflowExecution>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(WORKFLOW_HISTORY_TABLE)?;

        let key = format!("{}:{}", workflow_id, execution_id);
        match table.get(&*key)? {
            Some(value) => {
                let execution = serde_json::from_slice(value.value())?;
                Ok(Some(execution))
            }
            None => Ok(None),
        }
    }

    /// Get running executions.
    pub fn get_running(&self) -> Result<Vec<WorkflowExecution>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(WORKFLOW_HISTORY_TABLE)?;

        let mut results = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(execution) = serde_json::from_slice::<WorkflowExecution>(value.value()) {
                if execution.status == WorkflowStatus::Running {
                    results.push(execution);
                }
            }
        }

        Ok(results)
    }

    /// List all workflows.
    pub fn list_workflows(&self) -> Result<Vec<String>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(WORKFLOW_HISTORY_TABLE)?;

        let mut workflows = std::collections::HashSet::new();
        for result in table.iter()? {
            let (key, _) = result?;
            let key_str = key.value();
            if let Some(workflow_id) = key_str.split(':').next() {
                workflows.insert(workflow_id.to_string());
            }
        }

        Ok(workflows.into_iter().collect())
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

        Ok(Self {
            db: Arc::new(db),
        })
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
        let filter = AlertFilter::new()
            .with_status(AlertStatus::Active);
        self.query(&filter)
    }

    /// Get alerts by source.
    pub fn get_by_source(&self, source: &str) -> Result<Vec<Alert>> {
        let filter = AlertFilter::new()
            .with_source(source);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_log_store() {
        // Clean up any existing test database
        let _ = std::fs::remove_file("/tmp/test_events.redb");

        let store = EventLogStore::open("/tmp/test_events.redb").unwrap();

        let event = EventLog {
            id: "evt-1".to_string(),
            event_type: "device_connected".to_string(),
            source: Some("device-1".to_string()),
            severity: EventSeverity::Info,
            timestamp: Utc::now().timestamp(),
            message: "Device connected".to_string(),
            data: None,
        };

        store.write(&event).unwrap();

        let events = store.get_by_source("device-1", Some(10)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "evt-1");
    }

    #[test]
    fn test_event_filter() {
        let filter = EventFilter::new()
            .with_event_type("test")
            .with_severity(EventSeverity::Error)
            .with_source("device-1")
            .with_limit(10);

        assert_eq!(filter.event_types.len(), 1);
        assert_eq!(filter.severities.len(), 1);
        assert_eq!(filter.limit, Some(10));
    }

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
