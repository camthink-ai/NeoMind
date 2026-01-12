//! Decision history storage and query API.
//!
//! This module provides persistent storage for LLM-generated decisions,
//! including query APIs and statistical analysis.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use redb::{Database, ReadableTable, TableDefinition};

use crate::Error;

// Decisions table: key = decision_id, value = StoredDecision (serialized as JSON)
const DECISIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("decisions");

/// Decision store for persisting and querying LLM decisions.
pub struct DecisionStore {
    /// redb database
    db: Arc<Database>,
}

/// A stored decision record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDecision {
    /// Unique decision ID
    pub id: String,
    /// Decision title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Reasoning behind the decision
    pub reasoning: String,
    /// Suggested actions to take
    pub actions: Vec<StoredAction>,
    /// Confidence level (0-100)
    pub confidence: f32,
    /// Decision type
    pub decision_type: DecisionType,
    /// Priority level
    pub priority: DecisionPriority,
    /// Source review (if applicable)
    pub source_review: Option<String>,
    /// Timestamp when decision was created
    pub created_at: i64,
    /// Timestamp when decision was executed (if applicable)
    pub executed_at: Option<i64>,
    /// Execution result
    pub execution_result: Option<ExecutionResult>,
    /// Decision status
    pub status: DecisionStatus,
}

/// A stored action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAction {
    /// Action ID
    pub id: String,
    /// Action type
    pub action_type: String,
    /// Action description
    pub description: String,
    /// Parameters for the action
    pub parameters: serde_json::Value,
    /// Whether this action is required
    pub required: bool,
}

/// Execution result for a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether execution was successful
    pub success: bool,
    /// Number of actions executed
    pub actions_executed: usize,
    /// Number of actions that succeeded
    pub success_count: usize,
    /// Number of actions that failed
    pub failure_count: usize,
    /// Error message (if any)
    pub error: Option<String>,
    /// Execution timestamp
    pub timestamp: i64,
}

/// Type of decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionType {
    Rule,
    DeviceControl,
    Alert,
    Workflow,
    Configuration,
    DataCollection,
    HumanIntervention,
}

/// Priority level for decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Decision status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionStatus {
    Proposed,
    Approved,
    Rejected,
    Executed,
    Failed,
    Expired,
}

/// Query filter for decisions.
#[derive(Debug, Clone, Default)]
pub struct DecisionFilter {
    /// Filter by decision type
    pub decision_type: Option<DecisionType>,
    /// Filter by priority
    pub priority: Option<DecisionPriority>,
    /// Filter by status
    pub status: Option<DecisionStatus>,
    /// Filter by minimum confidence
    pub min_confidence: Option<f32>,
    /// Filter by creation time range (start)
    pub start_time: Option<i64>,
    /// Filter by creation time range (end)
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

/// Decision statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionStats {
    /// Total number of decisions
    pub total_count: usize,
    /// Count by type
    pub by_type: std::collections::HashMap<String, usize>,
    /// Count by priority
    pub by_priority: std::collections::HashMap<String, usize>,
    /// Count by status
    pub by_status: std::collections::HashMap<String, usize>,
    /// Average confidence
    pub avg_confidence: f32,
    /// Execution success rate
    pub success_rate: f32,
}

impl DecisionStore {
    /// Open or create a decision store at the given path.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>, Error> {
        let db = Database::create(path)?;
        let write_txn = db.begin_write()?;

        // Create tables if they don't exist
        write_txn.open_table(DECISIONS_TABLE)?;
        write_txn.commit()?;

        Ok(Arc::new(Self { db: Arc::new(db) }))
    }

    /// Create an in-memory decision store for testing.
    pub fn memory() -> Result<Arc<Self>, Error> {
        let temp_path = std::env::temp_dir()
            .join(format!("decisions_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Save a decision to the store.
    pub async fn save(&self, decision: &StoredDecision) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DECISIONS_TABLE)?;

            let value = serde_json::to_vec(decision)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(decision.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a decision by ID.
    pub async fn get(&self, id: &str) -> Result<Option<StoredDecision>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        match table.get(id)? {
            Some(bytes) => {
                let decision: StoredDecision = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(decision))
            }
            None => Ok(None),
        }
    }

    /// Query decisions with filters.
    pub async fn query(&self, filter: DecisionFilter) -> Result<Vec<StoredDecision>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        let mut decisions = Vec::new();

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let decision: StoredDecision = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            if self.matches_filter(&decision, &filter) {
                decisions.push(decision);
            }
        }

        // Sort by created_at descending (newest first)
        decisions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply pagination
        if let Some(offset) = filter.offset {
            if offset < decisions.len() {
                decisions = decisions.into_iter().skip(offset).collect();
            } else {
                decisions.clear();
            }
        }

        if let Some(limit) = filter.limit {
            decisions.truncate(limit);
        }

        Ok(decisions)
    }

    /// Update decision status.
    pub async fn update_status(
        &self,
        id: &str,
        status: DecisionStatus,
    ) -> Result<(), Error> {
        // First read the existing decision
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        let decision = match table.get(id)? {
            Some(bytes) => {
                let mut dec: StoredDecision = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                dec.status = status;
                dec
            }
            None => return Ok(()), // Decision doesn't exist, nothing to update
        };
        drop(table);
        drop(read_txn);

        // Then write the updated decision
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DECISIONS_TABLE)?;

            let value = serde_json::to_vec(&decision)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Record execution result for a decision.
    pub async fn record_execution(
        &self,
        id: &str,
        result: ExecutionResult,
    ) -> Result<(), Error> {
        // First read the existing decision
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        let decision = match table.get(id)? {
            Some(bytes) => {
                let mut dec: StoredDecision = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                dec.execution_result = Some(result.clone());
                dec.executed_at = Some(result.timestamp);
                dec.status = if result.success {
                    DecisionStatus::Executed
                } else {
                    DecisionStatus::Failed
                };
                dec
            }
            None => return Ok(()), // Decision doesn't exist
        };
        drop(table);
        drop(read_txn);

        // Then write the updated decision
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DECISIONS_TABLE)?;

            let value = serde_json::to_vec(&decision)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Delete a decision by ID.
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DECISIONS_TABLE)?;
            table.remove(id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get decision statistics.
    pub async fn stats(&self) -> Result<DecisionStats, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        let mut total_count = 0;
        let mut by_type = std::collections::HashMap::new();
        let mut by_priority = std::collections::HashMap::new();
        let mut by_status = std::collections::HashMap::new();
        let mut total_confidence = 0.0;
        let mut executed_count = 0;
        let mut success_count = 0;

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let decision: StoredDecision = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            total_count += 1;

            let type_name = format!("{:?}", decision.decision_type);
            *by_type.entry(type_name).or_insert(0) += 1;

            let priority_name = format!("{:?}", decision.priority);
            *by_priority.entry(priority_name).or_insert(0) += 1;

            let status_name = format!("{:?}", decision.status);
            *by_status.entry(status_name).or_insert(0) += 1;

            total_confidence += decision.confidence as f64;

            if decision.execution_result.is_some() || matches!(decision.status, DecisionStatus::Executed | DecisionStatus::Failed) {
                executed_count += 1;
                if matches!(decision.status, DecisionStatus::Executed) {
                    success_count += 1;
                }
            }
        }

        let avg_confidence = if total_count > 0 {
            (total_confidence / total_count as f64) as f32
        } else {
            0.0
        };

        let success_rate = if executed_count > 0 {
            (success_count as f32 / executed_count as f32) * 100.0
        } else {
            0.0
        };

        Ok(DecisionStats {
            total_count,
            by_type,
            by_priority,
            by_status,
            avg_confidence,
            success_rate,
        })
    }

    /// Clean up expired decisions.
    pub async fn cleanup_expired(&self, expire_before: i64) -> Result<usize, Error> {
        // First pass: collect the IDs to remove
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DECISIONS_TABLE)?;

        let mut to_remove: Vec<String> = Vec::new();
        for item in table.iter()? {
            let (id, bytes) = item?;
            let decision: StoredDecision = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            if decision.created_at < expire_before {
                to_remove.push(id.value().to_string());
            }
        }
        drop(table);
        drop(read_txn);

        // Second pass: remove the collected IDs
        if to_remove.is_empty() {
            return Ok(0);
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DECISIONS_TABLE)?;
            for key in &to_remove {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;

        Ok(to_remove.len())
    }

    /// Check if a decision matches the given filter.
    fn matches_filter(&self, decision: &StoredDecision, filter: &DecisionFilter) -> bool {
        if let Some(decision_type) = filter.decision_type {
            if decision.decision_type != decision_type {
                return false;
            }
        }

        if let Some(priority) = filter.priority {
            if decision.priority != priority {
                return false;
            }
        }

        if let Some(status) = filter.status {
            if decision.status != status {
                return false;
            }
        }

        if let Some(min_confidence) = filter.min_confidence {
            if decision.confidence < min_confidence {
                return false;
            }
        }

        if let Some(start_time) = filter.start_time {
            if decision.created_at < start_time {
                return false;
            }
        }

        if let Some(end_time) = filter.end_time {
            if decision.created_at > end_time {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Arc<DecisionStore> {
        DecisionStore::memory().unwrap()
    }

    #[tokio::test]
    async fn test_save_and_get() {
        let store = test_store();

        let decision = StoredDecision {
            id: "dec-1".to_string(),
            title: "Test Decision".to_string(),
            description: "Test description".to_string(),
            reasoning: "Test reasoning".to_string(),
            actions: vec![],
            confidence: 0.85,
            decision_type: DecisionType::Alert,
            priority: DecisionPriority::Medium,
            source_review: None,
            created_at: 1000,
            executed_at: None,
            execution_result: None,
            status: DecisionStatus::Proposed,
        };

        // Save and get should work
        store.save(&decision).await.unwrap();
        let retrieved = store.get("dec-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "dec-1");
        assert_eq!(retrieved.title, "Test Decision");
    }

    #[tokio::test]
    async fn test_query_with_filter() {
        let store = test_store();

        // Add multiple decisions
        for i in 0..5 {
            let decision = StoredDecision {
                id: format!("dec-{}", i),
                title: format!("Decision {}", i),
                description: format!("Description {}", i),
                reasoning: "Test".to_string(),
                actions: vec![],
                confidence: 0.5 + (i as f32) * 0.1,
                decision_type: if i % 2 == 0 { DecisionType::Alert } else { DecisionType::Rule },
                priority: if i < 2 { DecisionPriority::High } else { DecisionPriority::Medium },
                source_review: None,
                created_at: 1000 + i as i64 * 100,
                executed_at: None,
                execution_result: None,
                status: DecisionStatus::Proposed,
            };
            store.save(&decision).await.unwrap();
        }

        // Query by type
        let filter = DecisionFilter {
            decision_type: Some(DecisionType::Alert),
            ..Default::default()
        };
        let results = store.query(filter).await.unwrap();
        assert_eq!(results.len(), 3); // dec-0, dec-2, dec-4

        // Query by priority
        let filter = DecisionFilter {
            priority: Some(DecisionPriority::High),
            ..Default::default()
        };
        let results = store.query(filter).await.unwrap();
        assert_eq!(results.len(), 2); // dec-0, dec-1
    }

    #[tokio::test]
    async fn test_update_status() {
        let store = test_store();

        let decision = StoredDecision {
            id: "dec-1".to_string(),
            title: "Test".to_string(),
            description: "Test".to_string(),
            reasoning: "Test".to_string(),
            actions: vec![],
            confidence: 0.85,
            decision_type: DecisionType::Alert,
            priority: DecisionPriority::Medium,
            source_review: None,
            created_at: 1000,
            executed_at: None,
            execution_result: None,
            status: DecisionStatus::Proposed,
        };
        store.save(&decision).await.unwrap();

        store.update_status("dec-1", DecisionStatus::Approved).await.unwrap();

        let retrieved = store.get("dec-1").await.unwrap().unwrap();
        assert_eq!(retrieved.status, DecisionStatus::Approved);
    }

    #[tokio::test]
    async fn test_record_execution() {
        let store = test_store();

        let decision = StoredDecision {
            id: "dec-1".to_string(),
            title: "Test".to_string(),
            description: "Test".to_string(),
            reasoning: "Test".to_string(),
            actions: vec![],
            confidence: 0.85,
            decision_type: DecisionType::Alert,
            priority: DecisionPriority::Medium,
            source_review: None,
            created_at: 1000,
            executed_at: None,
            execution_result: None,
            status: DecisionStatus::Proposed,
        };
        store.save(&decision).await.unwrap();

        let result = ExecutionResult {
            success: true,
            actions_executed: 2,
            success_count: 2,
            failure_count: 0,
            error: None,
            timestamp: 2000,
        };

        store.record_execution("dec-1", result).await.unwrap();

        let retrieved = store.get("dec-1").await.unwrap().unwrap();
        assert_eq!(retrieved.status, DecisionStatus::Executed);
        assert_eq!(retrieved.executed_at, Some(2000));
        assert!(retrieved.execution_result.is_some());
    }

    #[tokio::test]
    async fn test_stats() {
        let store = test_store();

        // Add various decisions
        for i in 0..4 {
            let decision = StoredDecision {
                id: format!("dec-{}", i),
                title: format!("Decision {}", i),
                description: "Test".to_string(),
                reasoning: "Test".to_string(),
                actions: vec![],
                confidence: 0.5 + (i as f32) * 0.1,
                decision_type: if i % 2 == 0 { DecisionType::Alert } else { DecisionType::Rule },
                priority: if i < 2 { DecisionPriority::High } else { DecisionPriority::Medium },
                source_review: None,
                created_at: 1000 + i as i64 * 100,
                executed_at: None,
                execution_result: None,
                status: if i == 0 { DecisionStatus::Executed } else { DecisionStatus::Proposed },
            };
            store.save(&decision).await.unwrap();
        }

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total_count, 4);
        assert!(stats.avg_confidence > 0.0);
        assert!(stats.success_rate > 0.0); // 1 executed out of 4
    }
}
