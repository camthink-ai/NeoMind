//! Scheduler for time-based workflow triggers

use crate::error::{Result, WorkflowError};
use crate::executor::Executor;
use crate::trigger::TriggerManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Scheduled task
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    /// Task ID
    pub id: String,
    /// Workflow ID
    pub workflow_id: String,
    /// Trigger ID
    pub trigger_id: String,
    /// Cron expression
    pub cron_expression: String,
    /// Is active
    pub active: bool,
}

/// Scheduler for cron-based triggers
pub struct Scheduler {
    inner: Arc<RwLock<Option<JobScheduler>>>,
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(None)),
            tasks: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the scheduler
    pub async fn start(
        &self,
        _trigger_manager: Arc<TriggerManager>,
        _executor: Arc<Executor>,
    ) -> Result<()> {
        let scheduler = JobScheduler::new()
            .await
            .map_err(|e| WorkflowError::ExecutionError(format!("Failed to create scheduler: {}", e)))?;

        // Load tasks from trigger manager and register them
        // This would iterate through all cron triggers and create jobs

        let mut inner = self.inner.write().await;
        *inner = Some(scheduler);

        Ok(())
    }

    /// Stop the scheduler
    pub async fn stop(&self) -> Result<()> {
        let mut inner = self.inner.write().await;
        *inner = None;
        Ok(())
    }

    /// Add a scheduled task
    pub async fn add_task(&self, task: ScheduledTask) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        tasks.insert(task.id.clone(), task.clone());

        // If scheduler is running, add the job
        if let Some(_scheduler) = self.inner.read().await.as_ref() {
            // For now, just log the task - actual job creation depends on tokio_cron_scheduler API
            tracing::info!("Would schedule task {} with cron: {}", task.id, task.cron_expression);
        }

        Ok(())
    }

    /// Remove a scheduled task
    pub async fn remove_task(&self, task_id: &str) -> Result<bool> {
        let mut tasks = self.tasks.write().await;
        let removed = tasks.remove(task_id).is_some();

        // Remove from scheduler if running
        if removed {
            // Would need to track job UUIDs to remove specific jobs
        }

        Ok(removed)
    }

    /// Get a task
    pub async fn get_task(&self, task_id: &str) -> Option<ScheduledTask> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// List all tasks
    pub async fn list_tasks(&self) -> Vec<ScheduledTask> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }

    /// Check if scheduler is running
    pub async fn is_running(&self) -> bool {
        self.inner.read().await.is_some()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scheduler_creation() {
        let scheduler = Scheduler::new().unwrap();
        assert!(!scheduler.is_running().await);

        let trigger_manager = Arc::new(TriggerManager::new());
        let executor = Arc::new(Executor::new());

        scheduler.start(trigger_manager, executor).await.unwrap();
        assert!(scheduler.is_running().await);

        scheduler.stop().await.unwrap();
        assert!(!scheduler.is_running().await);
    }

    #[tokio::test]
    async fn test_scheduled_task() {
        let scheduler = Scheduler::new().unwrap();

        let task = ScheduledTask {
            id: "task1".to_string(),
            workflow_id: "workflow1".to_string(),
            trigger_id: "trigger1".to_string(),
            cron_expression: "0 * * * * *".to_string(),
            active: true,
        };

        scheduler.add_task(task).await.unwrap();

        let tasks = scheduler.list_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task1");
    }
}
