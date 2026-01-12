//! Workflow and execution persistence

use crate::error::Result;
use crate::workflow::Workflow;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use redb::{Database, TableDefinition, ReadableTable};

// Table definitions
const WORKFLOW_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("workflows");
const EXECUTION_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("executions");
const WORKFLOW_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("workflow_executions");

/// Workflow store for persisting workflow definitions
pub struct WorkflowStore {
    db: Arc<Database>,
    /// Storage path for singleton
    path: String,
}

/// Global workflow store singleton (thread-safe).
static WORKFLOW_STORE_SINGLETON: StdMutex<Option<Arc<WorkflowStore>>> = StdMutex::new(None);

impl WorkflowStore {
    /// Open or create a workflow store
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = WORKFLOW_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(WorkflowStore {
            db: Arc::new(db),
            path: path_str,
        });

        *WORKFLOW_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory store
    pub fn memory() -> Result<Arc<Self>> {
        let temp_path = std::env::temp_dir()
            .join(format!("workflow_store_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Save a workflow
    pub fn save(&self, workflow: &Workflow) -> Result<()> {
        workflow.validate()?;
        let key = format!("workflow:{}", workflow.id);
        let value = serde_json::to_vec(workflow)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(WORKFLOW_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a workflow
    pub fn load(&self, id: &str) -> Result<Option<Workflow>> {
        let key = format!("workflow:{}", id);

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(WORKFLOW_TABLE)?;

        match table.get(key.as_str())? {
            Some(value) => {
                let workflow = serde_json::from_slice(value.value())?;
                Ok(Some(workflow))
            }
            None => Ok(None),
        }
    }

    /// Delete a workflow
    pub fn delete(&self, id: &str) -> Result<bool> {
        let key = format!("workflow:{}", id);

        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(WORKFLOW_TABLE)?;
            table.remove(key.as_str())?.is_some()
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// List all workflow IDs
    pub fn list_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(WORKFLOW_TABLE)?;

        let mut iter: redb::Range<&str, &[u8]> = table.iter()?;
        while let Some(result) = iter.next() {
            let (key, _) = result?;
            let key_str = key.value();
            if let Some(id) = key_str.strip_prefix("workflow:") {
                ids.push(id.to_string());
            }
        }

        Ok(ids)
    }

    /// Load all workflows
    pub fn load_all(&self) -> Result<Vec<Workflow>> {
        let mut workflows = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(WORKFLOW_TABLE)?;

        let mut iter: redb::Range<&str, &[u8]> = table.iter()?;
        while let Some(result) = iter.next() {
            let (_, value) = result?;
            let workflow: Workflow = serde_json::from_slice(value.value())?;
            workflows.push(workflow);
        }

        Ok(workflows)
    }
}

/// Execution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Workflow ID
    pub workflow_id: String,
    /// Execution status
    pub status: ExecutionStatus,
    /// Started at
    pub started_at: i64,
    /// Completed at (if finished)
    pub completed_at: Option<i64>,
    /// Execution results by step ID
    pub step_results: std::collections::HashMap<String, StepResult>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Execution logs
    pub logs: Vec<ExecutionLog>,
}

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Step execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Step ID
    pub step_id: String,
    /// Started at
    pub started_at: i64,
    /// Completed at
    pub completed_at: Option<i64>,
    /// Status
    pub status: ExecutionStatus,
    /// Output value
    pub output: Option<serde_json::Value>,
    /// Error message
    pub error: Option<String>,
}

/// Execution log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    /// Timestamp
    pub timestamp: i64,
    /// Log level
    pub level: String,
    /// Message
    pub message: String,
}

/// Execution store for persisting execution history
pub struct ExecutionStore {
    db: Arc<Database>,
    max_records: usize,
    /// Storage path for singleton
    path: String,
}

/// Global execution store singleton (thread-safe).
static EXECUTION_STORE_SINGLETON: StdMutex<Option<Arc<ExecutionStore>>> = StdMutex::new(None);

impl ExecutionStore {
    /// Open or create an execution store
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = EXECUTION_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(ExecutionStore {
            db: Arc::new(db),
            max_records: 1000,
            path: path_str,
        });

        *EXECUTION_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory store
    pub fn memory() -> Result<Arc<Self>> {
        let temp_path = std::env::temp_dir()
            .join(format!("execution_store_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Set max records to keep
    pub fn with_max_records(mut self, max: usize) -> Self {
        self.max_records = max;
        self
    }

    /// Save an execution record
    pub fn save(&self, record: &ExecutionRecord) -> Result<()> {
        let key = format!("execution:{}", record.id);
        let value = serde_json::to_vec(record)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut exec_table = write_txn.open_table(EXECUTION_TABLE)?;
            exec_table.insert(key.as_str(), value.as_slice())?;

            // Add to workflow index
            let workflow_key = format!("workflow_executions:{}", record.workflow_id);
            let mut executions: Vec<String> = match exec_table.get(workflow_key.as_str())? {
                Some(v) => serde_json::from_slice(v.value())?,
                None => Vec::new(),
            };
            executions.push(record.id.clone());

            // Trim if too many
            if executions.len() > self.max_records {
                executions = executions.into_iter().rev().take(self.max_records).collect();
            }

            let mut index_table = write_txn.open_table(WORKFLOW_EXECUTIONS_TABLE)?;
            index_table.insert(workflow_key.as_str(), serde_json::to_vec(&executions)?.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load an execution record
    pub fn load(&self, id: &str) -> Result<Option<ExecutionRecord>> {
        let key = format!("execution:{}", id);

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXECUTION_TABLE)?;

        match table.get(key.as_str())? {
            Some(value) => {
                let record = serde_json::from_slice(value.value())?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// Get executions for a workflow
    pub fn get_workflow_executions(&self, workflow_id: &str) -> Result<Vec<ExecutionRecord>> {
        let key = format!("workflow_executions:{}", workflow_id);

        let read_txn = self.db.begin_read()?;

        let execution_ids: Vec<String> = {
            let index_table = read_txn.open_table(WORKFLOW_EXECUTIONS_TABLE)?;
            match index_table.get(key.as_str())? {
                Some(v) => serde_json::from_slice(v.value())?,
                None => return Ok(Vec::new()),
            }
        };

        let mut executions = Vec::new();
        let exec_table = read_txn.open_table(EXECUTION_TABLE)?;

        for execution_id in execution_ids {
            let exec_key = format!("execution:{}", execution_id);
            if let Some(value) = exec_table.get(exec_key.as_str())? {
                let record: ExecutionRecord = serde_json::from_slice(value.value())?;
                executions.push(record);
            }
        }

        executions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(executions)
    }

    /// Get recent executions
    pub fn get_recent(&self, limit: usize) -> Result<Vec<ExecutionRecord>> {
        let mut executions = Vec::new();

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXECUTION_TABLE)?;

        let mut iter: redb::Range<&str, &[u8]> = table.iter()?;
        while let Some(result) = iter.next() {
            let (_, value) = result?;
            let record: ExecutionRecord = serde_json::from_slice(value.value())?;
            executions.push(record);
        }

        executions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        executions.truncate(limit);
        Ok(executions)
    }

    /// Delete old execution records
    pub fn cleanup_old(&self, older_than: i64) -> Result<usize> {
        let mut deleted = 0;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(EXECUTION_TABLE)?;
            let mut iter: redb::Range<&str, &[u8]> = table.iter()?;

            // Collect keys to delete
            let mut keys_to_delete = Vec::new();
            while let Some(result) = iter.next() {
                let (key, value) = result?;
                let record: ExecutionRecord = serde_json::from_slice(value.value())?;
                if record.started_at < older_than {
                    let key_str = key.value().to_string();
                    keys_to_delete.push(key_str);
                }
            }
            drop(iter);

            // Delete the keys
            for key_str in &keys_to_delete {
                if table.remove(key_str.as_str())?.is_some() {
                    deleted += 1;
                }
            }
        }
        write_txn.commit()?;

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::Step;

    #[tokio::test]
    async fn test_workflow_store() {
        let store = WorkflowStore::memory().unwrap();

        let workflow = Workflow::new("test", "Test Workflow")
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "test".to_string(),
                level: "info".to_string(),
            });

        store.save(&workflow).unwrap();

        let loaded = store.load("test").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "Test Workflow");

        let ids = store.list_ids().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "test");
    }

    #[tokio::test]
    async fn test_execution_store() {
        let store = ExecutionStore::memory().unwrap();

        let record = ExecutionRecord {
            id: "exec1".to_string(),
            workflow_id: "test_workflow".to_string(),
            status: ExecutionStatus::Completed,
            started_at: 1000,
            completed_at: Some(1100),
            step_results: std::collections::HashMap::new(),
            error: None,
            logs: Vec::new(),
        };

        store.save(&record).unwrap();

        let loaded = store.load("exec1").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().status, ExecutionStatus::Completed);

        let workflow_execs = store.get_workflow_executions("test_workflow").unwrap();
        assert_eq!(workflow_execs.len(), 1);
    }
}
