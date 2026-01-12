//! Rule execution history storage and query.
//!
//! This module provides persistent storage for rule execution history,
//! enabling historical analysis and statistics.

use crate::engine::{RuleId, RuleExecutionResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

/// Rule execution history entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleHistoryEntry {
    /// Unique history entry ID
    pub id: String,
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub rule_name: String,
    /// Whether execution was successful
    pub success: bool,
    /// Actions executed
    pub actions_executed: Vec<String>,
    /// Error message if any
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Timestamp when the rule was executed
    pub timestamp: DateTime<Utc>,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl RuleHistoryEntry {
    /// Create a new history entry from a rule execution result.
    pub fn from_result(result: &RuleExecutionResult) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            rule_id: result.rule_id.to_string(),
            rule_name: result.rule_name.clone(),
            success: result.success,
            actions_executed: result.actions_executed.clone(),
            error: result.error.clone(),
            duration_ms: result.duration_ms,
            timestamp: Utc::now(),
            metadata: None,
        }
    }
}

/// History query filter.
#[derive(Debug, Clone, Default)]
pub struct HistoryFilter {
    /// Filter by rule ID
    pub rule_id: Option<String>,
    /// Filter by success status
    pub success: Option<bool>,
    /// Start timestamp (inclusive)
    pub start: Option<DateTime<Utc>>,
    /// End timestamp (exclusive)
    pub end: Option<DateTime<Utc>>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl HistoryFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by rule ID.
    pub fn with_rule_id(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }

    /// Filter by success status.
    pub fn with_success(mut self, success: bool) -> Self {
        self.success = Some(success);
        self
    }

    /// Filter by time range.
    pub fn with_time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start = Some(start);
        self.end = Some(end);
        self
    }

    /// Set result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set offset.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Rule history statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleHistoryStats {
    /// Total number of executions
    pub total_executions: u64,
    /// Number of successful executions
    pub successful_executions: u64,
    /// Number of failed executions
    pub failed_executions: u64,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: f64,
    /// Min execution duration in milliseconds
    pub min_duration_ms: u64,
    /// Max execution duration in milliseconds
    pub max_duration_ms: u64,
    /// Last execution timestamp
    pub last_execution: Option<DateTime<Utc>>,
    /// First execution timestamp
    pub first_execution: Option<DateTime<Utc>>,
}

impl RuleHistoryStats {
    /// Calculate success rate as a percentage.
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            return 0.0;
        }
        (self.successful_executions as f64 / self.total_executions as f64) * 100.0
    }
}

/// In-memory rule history storage.
///
/// For production, use a proper database backend.
pub struct RuleHistoryStorage {
    /// History entries
    entries: Arc<RwLock<Vec<RuleHistoryEntry>>>,
    /// Maximum history size
    max_size: usize,
}

impl RuleHistoryStorage {
    /// Create a new history storage with default capacity.
    pub fn new() -> Self {
        Self::with_capacity(10000)
    }

    /// Create a new history storage with specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::with_capacity(capacity))),
            max_size: capacity,
        }
    }

    /// Add a history entry.
    pub async fn add(&self, entry: RuleHistoryEntry) -> Result<(), HistoryError> {
        let mut entries = self.entries.write().await;
        entries.push(entry);

        // Enforce max size
        if entries.len() > self.max_size {
            entries.remove(0);
        }

        Ok(())
    }

    /// Query history with filters.
    pub async fn query(&self, filter: &HistoryFilter) -> Result<Vec<RuleHistoryEntry>, HistoryError> {
        let entries = self.entries.read().await;

        let mut results: Vec<_> = entries
            .iter()
            .filter(|entry| {
                if let Some(ref rule_id) = filter.rule_id {
                    if &entry.rule_id != rule_id {
                        return false;
                    }
                }
                if let Some(success) = filter.success {
                    if entry.success != success {
                        return false;
                    }
                }
                if let Some(start) = filter.start {
                    if entry.timestamp < start {
                        return false;
                    }
                }
                if let Some(end) = filter.end {
                    if entry.timestamp >= end {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Reverse to get newest first
        results.reverse();

        // Apply offset
        if let Some(offset) = filter.offset {
            if offset < results.len() {
                results = results[offset..].to_vec();
            } else {
                results.clear();
            }
        }

        // Apply limit
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    /// Get statistics for a rule.
    pub async fn get_stats(&self, rule_id: &RuleId) -> Result<RuleHistoryStats, HistoryError> {
        let entries = self.entries.read().await;

        let rule_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.rule_id == rule_id.to_string())
            .collect();

        if rule_entries.is_empty() {
            return Ok(RuleHistoryStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0.0,
                min_duration_ms: 0,
                max_duration_ms: 0,
                last_execution: None,
                first_execution: None,
            });
        }

        let total = rule_entries.len() as u64;
        let successful = rule_entries.iter().filter(|e| e.success).count() as u64;
        let failed = total - successful;

        let durations: Vec<u64> = rule_entries.iter().map(|e| e.duration_ms).collect();
        let avg = durations.iter().map(|&d| d as f64).sum::<f64>() / durations.len() as f64;
        let min = *durations.iter().min().unwrap_or(&0);
        let max = *durations.iter().max().unwrap_or(&0);

        let timestamps: Vec<_> = rule_entries.iter().map(|e| e.timestamp).collect();
        let last = timestamps.first().cloned();
        let first = timestamps.last().cloned();

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

    /// Get global statistics across all rules.
    pub async fn get_global_stats(&self) -> Result<RuleHistoryStats, HistoryError> {
        let entries = self.entries.read().await;

        if entries.is_empty() {
            return Ok(RuleHistoryStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0.0,
                min_duration_ms: 0,
                max_duration_ms: 0,
                last_execution: None,
                first_execution: None,
            });
        }

        let total = entries.len() as u64;
        let successful = entries.iter().filter(|e| e.success).count() as u64;
        let failed = total - successful;

        let durations: Vec<u64> = entries.iter().map(|e| e.duration_ms).collect();
        let avg = durations.iter().map(|&d| d as f64).sum::<f64>() / durations.len() as f64;
        let min = *durations.iter().min().unwrap_or(&0);
        let max = *durations.iter().max().unwrap_or(&0);

        let timestamps: Vec<_> = entries.iter().map(|e| e.timestamp).collect();
        let last = timestamps.first().cloned();
        let first = timestamps.last().cloned();

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

    /// Clear all history.
    pub async fn clear(&self) -> Result<(), HistoryError> {
        let mut entries = self.entries.write().await;
        entries.clear();
        Ok(())
    }

    /// Get the number of entries.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if the storage is empty.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }
}

impl Default for RuleHistoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for history operations.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Query error
    #[error("Query error: {0}")]
    Query(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::RuleId;

    #[tokio::test]
    async fn test_history_storage() {
        let storage = RuleHistoryStorage::new();

        // Initially empty
        assert!(storage.is_empty().await);
        assert_eq!(storage.len().await, 0);

        // Add entry
        let entry = RuleHistoryEntry {
            id: "test-1".to_string(),
            rule_id: RuleId::new().to_string(),
            rule_name: "Test Rule".to_string(),
            success: true,
            actions_executed: vec!["notify:test".to_string()],
            error: None,
            duration_ms: 100,
            timestamp: Utc::now(),
            metadata: None,
        };

        storage.add(entry).await.unwrap();

        // Not empty anymore
        assert!(!storage.is_empty().await);
        assert_eq!(storage.len().await, 1);
    }

    #[tokio::test]
    async fn test_history_query() {
        let storage = RuleHistoryStorage::new();
        let rule_id = RuleId::new();

        // Add some entries
        for i in 0..5 {
            let entry = RuleHistoryEntry {
                id: format!("test-{}", i),
                rule_id: rule_id.to_string(),
                rule_name: "Test Rule".to_string(),
                success: i % 2 == 0,
                actions_executed: vec![],
                error: None,
                duration_ms: 100,
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        // Query all
        let filter = HistoryFilter::new().with_rule_id(rule_id.to_string());
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_history_filter_by_success() {
        let storage = RuleHistoryStorage::new();
        let rule_id = RuleId::new();

        // Add entries with different success statuses
        for i in 0..3 {
            let entry = RuleHistoryEntry {
                id: format!("test-{}", i),
                rule_id: rule_id.to_string(),
                rule_name: "Test Rule".to_string(),
                success: i == 0, // Only first is successful
                actions_executed: vec![],
                error: None,
                duration_ms: 100,
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        // Query only successful
        let filter = HistoryFilter::new()
            .with_rule_id(rule_id.to_string())
            .with_success(true);
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_history_stats() {
        let storage = RuleHistoryStorage::new();
        let rule_id = RuleId::new();

        // Add some entries
        for i in 0..10 {
            let entry = RuleHistoryEntry {
                id: format!("test-{}", i),
                rule_id: rule_id.to_string(),
                rule_name: "Test Rule".to_string(),
                success: i % 2 == 0,
                actions_executed: vec![],
                error: if i % 2 == 0 { None } else { Some("Error".to_string()) },
                duration_ms: 100 + (i as u64 * 10),
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        let stats = storage.get_stats(&rule_id).await.unwrap();
        assert_eq!(stats.total_executions, 10);
        assert_eq!(stats.successful_executions, 5);
        assert_eq!(stats.failed_executions, 5);
        assert_eq!(stats.success_rate(), 50.0);
    }

    #[tokio::test]
    async fn test_global_stats() {
        let storage = RuleHistoryStorage::new();

        // Add entries for multiple rules
        for i in 0..5 {
            let entry = RuleHistoryEntry {
                id: format!("test-{}", i),
                rule_id: RuleId::new().to_string(),
                rule_name: format!("Rule {}", i),
                success: true,
                actions_executed: vec![],
                error: None,
                duration_ms: 100,
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        let stats = storage.get_global_stats().await.unwrap();
        assert_eq!(stats.total_executions, 5);
        assert_eq!(stats.successful_executions, 5);
        assert_eq!(stats.success_rate(), 100.0);
    }

    #[tokio::test]
    async fn test_max_capacity() {
        let storage = RuleHistoryStorage::with_capacity(3);
        let rule_id = RuleId::new();

        // Add more entries than capacity
        for i in 0..5 {
            let entry = RuleHistoryEntry {
                id: format!("test-{}", i),
                rule_id: rule_id.to_string(),
                rule_name: "Test Rule".to_string(),
                success: true,
                actions_executed: vec![],
                error: None,
                duration_ms: 100,
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        // Should only keep max_capacity entries
        assert_eq!(storage.len().await, 3);
    }

    #[tokio::test]
    async fn test_clear() {
        let storage = RuleHistoryStorage::new();

        // Add entry
        let entry = RuleHistoryEntry {
            id: "test-1".to_string(),
            rule_id: RuleId::new().to_string(),
            rule_name: "Test Rule".to_string(),
            success: true,
            actions_executed: vec![],
            error: None,
            duration_ms: 100,
            timestamp: Utc::now(),
            metadata: None,
        };
        storage.add(entry).await.unwrap();

        assert!(!storage.is_empty().await);

        storage.clear().await.unwrap();

        assert!(storage.is_empty().await);
    }

    #[tokio::test]
    async fn test_history_filter_builder() {
        let filter = HistoryFilter::new()
            .with_rule_id("rule-1")
            .with_success(true)
            .with_limit(10)
            .with_offset(0);

        assert_eq!(filter.rule_id, Some("rule-1".to_string()));
        assert_eq!(filter.success, Some(true));
        assert_eq!(filter.limit, Some(10));
    }

    #[tokio::test]
    async fn test_entry_from_result() {
        let result = RuleExecutionResult {
            rule_id: RuleId::new(),
            rule_name: "Test".to_string(),
            success: true,
            actions_executed: vec!["action1".to_string()],
            error: None,
            duration_ms: 50,
        };

        let entry = RuleHistoryEntry::from_result(&result);
        assert_eq!(entry.rule_name, "Test");
        assert_eq!(entry.success, true);
        assert_eq!(entry.duration_ms, 50);
    }

    #[tokio::test]
    async fn test_stats_duration_range() {
        let storage = RuleHistoryStorage::new();
        let rule_id = RuleId::new();

        // Add entries with varying durations
        for duration in [50, 100, 150, 200] {
            let entry = RuleHistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                rule_id: rule_id.to_string(),
                rule_name: "Test Rule".to_string(),
                success: true,
                actions_executed: vec![],
                error: None,
                duration_ms: duration,
                timestamp: Utc::now(),
                metadata: None,
            };
            storage.add(entry).await.unwrap();
        }

        let stats = storage.get_stats(&rule_id).await.unwrap();
        assert_eq!(stats.min_duration_ms, 50);
        assert_eq!(stats.max_duration_ms, 200);
    }
}
