//! Trigger management for workflows

use crate::error::{Result, WorkflowError};
use crate::executor::Executor;
use crate::workflow::Trigger;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trigger manager
pub struct TriggerManager {
    /// Registered triggers by workflow ID
    triggers: Arc<RwLock<HashMap<String, Vec<Trigger>>>>,
    /// Trigger callbacks by trigger ID
    callbacks: Arc<RwLock<HashMap<String, TriggerCallback>>>,
}

/// Trigger callback information
#[derive(Clone)]
struct TriggerCallback {
    workflow_id: String,
    trigger_id: String,
    executor: Arc<Executor>,
}

impl TriggerManager {
    /// Create a new trigger manager
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(RwLock::new(HashMap::new())),
            callbacks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a trigger
    pub async fn register(
        &self,
        workflow_id: String,
        trigger: Trigger,
        executor: Arc<Executor>,
    ) -> Result<()> {
        let trigger_id = trigger.id().to_string();

        // Add to workflow triggers
        let mut triggers = self.triggers.write().await;
        triggers.entry(workflow_id.clone())
            .or_insert_with(Vec::new)
            .push(trigger.clone());

        // Add callback
        let callback = TriggerCallback {
            workflow_id,
            trigger_id: trigger_id.clone(),
            executor,
        };
        let mut callbacks = self.callbacks.write().await;
        callbacks.insert(trigger_id, callback);

        Ok(())
    }

    /// Unregister all triggers for a workflow
    pub async fn unregister_workflow(&self, workflow_id: &str) {
        let mut triggers = self.triggers.write().await;
        if let Some(trigger_list) = triggers.remove(workflow_id) {
            let mut callbacks = self.callbacks.write().await;
            for trigger in trigger_list {
                callbacks.remove(trigger.id());
            }
        }
    }

    /// Trigger a workflow by trigger ID
    pub async fn trigger(&self, trigger_id: &str) -> Result<()> {
        let callbacks = self.callbacks.read().await;
        let callback = callbacks.get(trigger_id)
            .ok_or_else(|| WorkflowError::ExecutionError(format!("Trigger not found: {}", trigger_id)))?;

        // Execute the workflow
        // This would normally call through the workflow engine
        // For now, we just log
        tracing::info!("Triggered workflow {} via trigger {}", callback.workflow_id, trigger_id);

        Ok(())
    }

    /// Get triggers for a workflow
    pub async fn get_workflow_triggers(&self, workflow_id: &str) -> Vec<Trigger> {
        let triggers = self.triggers.read().await;
        triggers.get(workflow_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Fire an event trigger
    pub async fn fire_event(&self, event_type: &str, data: Option<serde_json::Value>) -> Result<()> {
        let callbacks = self.callbacks.read().await;

        for (trigger_id, callback) in callbacks.iter() {
            if let Trigger::Event { id: _, event_type: et, filters } = &callback.get_trigger() {
                if et == event_type {
                    // Check filters
                    let matches = if let Some(filters) = filters {
                        Self::check_filters(filters, &data.clone().unwrap_or_default())
                    } else {
                        true
                    };

                    if matches {
                        self.trigger(trigger_id).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if event data matches filters
    fn check_filters(filters: &HashMap<String, serde_json::Value>, data: &serde_json::Value) -> bool {
        if let Some(obj) = data.as_object() {
            for (key, expected_value) in filters {
                if let Some(actual_value) = obj.get(key) {
                    if actual_value != expected_value {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}

impl TriggerCallback {
    /// Get the trigger definition
    fn get_trigger(&self) -> Trigger {
        // This would normally look up the trigger from storage
        // For now, return a placeholder
        Trigger::Manual {
            id: self.trigger_id.clone(),
        }
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Cron trigger specifics
pub struct CronTrigger {
    pub id: String,
    pub expression: String,
    pub timezone: Option<String>,
}

/// Event trigger specifics
pub struct EventTrigger {
    pub id: String,
    pub event_type: String,
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Manual trigger specifics
pub struct ManualTrigger {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_trigger_manager() {
        let manager = TriggerManager::new();

        let workflow_id = "test_workflow".to_string();
        let executor = Arc::new(Executor::new());

        let trigger = Trigger::Manual {
            id: "manual1".to_string(),
        };

        manager.register(workflow_id.clone(), trigger, executor).await.unwrap();

        let triggers = manager.get_workflow_triggers(&workflow_id).await;
        assert_eq!(triggers.len(), 1);
    }

    #[tokio::test]
    async fn test_unregister_workflow() {
        let manager = TriggerManager::new();

        let workflow_id = "test_workflow".to_string();
        let executor = Arc::new(Executor::new());

        let trigger = Trigger::Manual {
            id: "manual1".to_string(),
        };

        manager.register(workflow_id.clone(), trigger, executor).await.unwrap();
        manager.unregister_workflow(&workflow_id).await;

        let triggers = manager.get_workflow_triggers(&workflow_id).await;
        assert_eq!(triggers.len(), 0);
    }
}
