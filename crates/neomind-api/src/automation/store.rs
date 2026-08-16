//! Storage backend for unified automations
//!
//! This module provides persistent storage for automations using redb.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};

use super::error::{AutomationError, Result};
use super::types::*;

// Table definitions
const AUTOMATIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("automations");
const EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("automation_executions");
const TEMPLATES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("automation_templates");

/// Storage for automations
pub struct AutomationStore {
    db: Arc<Database>,
}

impl AutomationStore {
    /// Open or create a store at the given path
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Create parent directory if needed
        if let Some(parent) = path.as_ref().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let db = Database::create(path.as_ref())?;

        // Write transaction to create tables
        let write_txn = db.begin_write()?;
        {
            write_txn.open_table(AUTOMATIONS_TABLE)?;
            write_txn.open_table(EXECUTIONS_TABLE)?;
            write_txn.open_table(TEMPLATES_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create a new in-memory store for testing
    pub fn memory() -> Result<Self> {
        // Use a temp file since redb doesn't support true in-memory mode
        let temp_path =
            std::env::temp_dir().join(format!("automation_store_{}.redb", uuid::Uuid::new_v4()));
        let db = Database::create(&temp_path)?;

        // Write transaction to create tables
        let write_txn = db.begin_write()?;
        {
            write_txn.open_table(AUTOMATIONS_TABLE)?;
            write_txn.open_table(EXECUTIONS_TABLE)?;
            write_txn.open_table(TEMPLATES_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Save an automation (TransformAutomation)
    pub fn save_automation(&self, automation: &Automation) -> Result<()> {
        let key = automation.id();
        // Add type tag for API compatibility
        let mut value = serde_json::to_value(automation)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("type".to_string(), serde_json::json!("transform"));
        }
        let bytes = serde_json::to_vec(&value)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AUTOMATIONS_TABLE)?;
            table.insert(key, bytes.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Get an automation by ID
    pub fn get_automation(&self, id: &str) -> Result<Option<Automation>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AUTOMATIONS_TABLE)?;

        match table.get(id)? {
            Some(value) => {
                let mut v: serde_json::Value = serde_json::from_slice(value.value())?;
                // Remove type tag for direct TransformAutomation deserialization
                if let Some(obj) = v.as_object_mut() {
                    obj.remove("type");
                }
                let automation: Automation = serde_json::from_value(v)?;
                Ok(Some(automation))
            }
            None => Ok(None),
        }
    }

    /// List all automations
    pub fn list_automations(&self) -> Result<Vec<Automation>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AUTOMATIONS_TABLE)?;

        let mut automations = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let mut v: serde_json::Value = serde_json::from_slice(value.value())?;
            // Remove type tag for direct TransformAutomation deserialization
            if let Some(obj) = v.as_object_mut() {
                obj.remove("type");
            }
            let automation: Automation = serde_json::from_value(v)?;
            automations.push(automation);
        }

        Ok(automations)
    }

    /// Delete an automation
    pub fn delete_automation(&self, id: &str) -> Result<bool> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(AUTOMATIONS_TABLE)?;
            let result = table.remove(id)?.is_some();
            result
        };
        // Also remove the automation's execution records — previously they
        // were orphaned and stayed forever (an unbounded-growth blind spot:
        // this table had no retention at all until the cleanup task).
        {
            let mut exec_table = write_txn.open_table(EXECUTIONS_TABLE)?;
            let prefix = format!("{}:", id);
            let stale_keys: Vec<String> = exec_table
                .iter()?
                .flatten()
                .map(|(k, _)| k.value().to_string())
                .filter(|k| k.starts_with(&prefix))
                .collect();
            for k in stale_keys {
                let _ = exec_table.remove(k.as_str());
            }
        }
        write_txn.commit()?;
        Ok(existed)
    }

    /// Save an execution record
    pub fn save_execution(&self, execution: &ExecutionRecord) -> Result<()> {
        let key = format!("{}:{}", execution.automation_id, execution.id);
        let value = serde_json::to_vec(execution)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(EXECUTIONS_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Get execution history for an automation
    pub fn get_executions(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<ExecutionRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXECUTIONS_TABLE)?;

        let prefix = format!("{}:", automation_id);
        let mut executions = Vec::new();

        // Collect ALL matching records before applying the limit: keys are
        // `{automation_id}:{uuid}` (random order), so breaking out of the
        // scan early returned an arbitrary subset once more records than
        // `limit` existed — the endpoint showed a random sample, not the
        // most recent executions.
        for result in table.iter()? {
            let (key, value) = result?;
            let key_str: &str = key.value(); // AccessGuard to &str
            if key_str.starts_with(prefix.as_str()) {
                let execution: ExecutionRecord = serde_json::from_slice(value.value())?;
                executions.push(execution);
            }
        }

        // Sort by started_at descending
        executions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        executions.truncate(limit);

        Ok(executions)
    }

    /// Remove execution records older than `days` (started_at is epoch
    /// millis). Returns the number removed. This table previously had NO
    /// retention at all — it was the one unbounded-growth table the 0.9.12
    /// sweep missed (messages/agent-executions/data-push/rule-history all
    /// got 30-day tasks; automations were forgotten).
    pub fn cleanup_executions(&self, days: u64) -> Result<usize> {
        let cutoff_ms = (chrono::Utc::now().timestamp()
            - (days as i64) * 24 * 60 * 60)
            * 1000;
        let mut to_remove: Vec<String> = Vec::new();

        let read_txn = self.db.begin_read()?;
        if let Ok(table) = read_txn.open_table(EXECUTIONS_TABLE) {
            for result in table.iter()? {
                let (key, value) = result?;
                match serde_json::from_slice::<ExecutionRecord>(value.value()) {
                    Ok(rec) if rec.started_at < cutoff_ms => {
                        to_remove.push(key.value().to_string());
                    }
                    Ok(_) => {}
                    // Undecodable row: prune it too (corrupt records are
                    // unreadable by the API anyway).
                    Err(_) => to_remove.push(key.value().to_string()),
                }
            }
        }
        drop(read_txn);

        if to_remove.is_empty() {
            return Ok(0);
        }
        let n = to_remove.len();
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(EXECUTIONS_TABLE)?;
            for k in &to_remove {
                let _ = table.remove(k.as_str());
            }
        }
        write_txn.commit()?;
        Ok(n)
    }

    /// Save a template
    pub fn save_template(&self, template: &AutomationTemplate) -> Result<()> {
        let key = format!("{}:{}", template.automation_type.as_str(), template.id);
        let value = serde_json::to_vec(template)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TEMPLATES_TABLE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Get a template by ID
    pub fn get_template(
        &self,
        id: &str,
        automation_type: AutomationType,
    ) -> Result<Option<AutomationTemplate>> {
        let key = format!("{}:{}", automation_type.as_str(), id);

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;

        match table.get(key.as_str())? {
            Some(value) => {
                let template: AutomationTemplate = serde_json::from_slice(value.value())?;
                Ok(Some(template))
            }
            None => Ok(None),
        }
    }

    /// List all templates
    pub fn list_templates(&self) -> Result<Vec<AutomationTemplate>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TEMPLATES_TABLE)?;

        let mut templates = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let template: AutomationTemplate = serde_json::from_slice(value.value())?;
            templates.push(template);
        }

        Ok(templates)
    }

    /// Delete a template
    pub fn delete_template(&self, id: &str, automation_type: AutomationType) -> Result<bool> {
        let key = format!("{}:{}", automation_type.as_str(), id);

        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(TEMPLATES_TABLE)?;
            let result = table.remove(key.as_str())?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }
}

/// Thread-safe wrapper for AutomationStore
#[derive(Clone)]
pub struct SharedAutomationStore {
    store: Arc<std::sync::RwLock<AutomationStore>>,
}

impl SharedAutomationStore {
    /// Create a new shared store
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let store = AutomationStore::open(path).await?;
        Ok(Self {
            store: Arc::new(std::sync::RwLock::new(store)),
        })
    }

    /// Create a new in-memory shared store
    pub fn memory() -> Result<Self> {
        let store = AutomationStore::memory()?;
        Ok(Self {
            store: Arc::new(std::sync::RwLock::new(store)),
        })
    }

    /// Save an automation
    pub async fn save_automation(&self, automation: &Automation) -> Result<()> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.save_automation(automation)
    }

    /// Remove execution records older than `days` (retention task).
    pub async fn cleanup_executions(&self, days: u64) -> Result<usize> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.cleanup_executions(days)
    }

    /// Get an automation by ID
    pub async fn get_automation(&self, id: &str) -> Result<Option<Automation>> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.get_automation(id)
    }

    /// List all automations
    pub async fn list_automations(&self) -> Result<Vec<Automation>> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.list_automations()
    }

    /// Delete an automation
    pub async fn delete_automation(&self, id: &str) -> Result<bool> {
        let store = self
            .store
            .write()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.delete_automation(id)
    }

    /// Save an execution record
    pub async fn save_execution(&self, execution: &ExecutionRecord) -> Result<()> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.save_execution(execution)
    }

    /// Get execution history for an automation
    pub async fn get_executions(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<ExecutionRecord>> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.get_executions(automation_id, limit)
    }

    /// Save a template
    pub fn save_template(&self, template: &AutomationTemplate) -> Result<()> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.save_template(template)
    }

    /// Get a template
    pub fn get_template(
        &self,
        id: &str,
        automation_type: AutomationType,
    ) -> Result<Option<AutomationTemplate>> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.get_template(id, automation_type)
    }

    /// List all templates
    pub fn list_templates(&self) -> Result<Vec<AutomationTemplate>> {
        let store = self
            .store
            .read()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.list_templates()
    }

    /// Delete a template
    pub fn delete_template(&self, id: &str, automation_type: AutomationType) -> Result<bool> {
        let store = self
            .store
            .write()
            .map_err(|_| AutomationError::StorageError("Poisoned lock".to_string()))?;
        store.delete_template(id, automation_type)
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{Automation, TransformScope};
    use super::*;

    #[test]
    fn test_memory_store() {
        let store = AutomationStore::memory().unwrap();

        let automation = Automation::new("test-1", "Test Transform", TransformScope::Global)
            .with_description("A test transform");

        store.save_automation(&automation).unwrap();
        let retrieved = store.get_automation("test-1").unwrap();
        assert!(retrieved.is_some());

        let all = store.list_automations().unwrap();
        assert_eq!(all.len(), 1);

        assert!(store.delete_automation("test-1").unwrap());
        assert!(!store.delete_automation("test-1").unwrap());
    }

    #[tokio::test]
    async fn test_shared_store() {
        let store = SharedAutomationStore::memory().unwrap();

        let automation = Automation::new(
            "test-2",
            "Test Transform 2",
            TransformScope::DeviceType("sensor".to_string()),
        )
        .with_description("Another test");

        store.save_automation(&automation).await.unwrap();
        let retrieved = store.get_automation("test-2").await.unwrap();
        assert!(retrieved.is_some());
    }
}
