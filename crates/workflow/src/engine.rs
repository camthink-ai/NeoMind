//! Workflow engine - manages and executes workflows

use crate::error::{Result, WorkflowError};
use crate::executor::{Executor, ExecutionContext};
use crate::store::{WorkflowStore, ExecutionStore, ExecutionRecord, ExecutionStatus};
use crate::workflow::Workflow;
use crate::trigger::TriggerManager;
use crate::scheduler::Scheduler;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Workflow engine
pub struct WorkflowEngine {
    workflow_store: Arc<WorkflowStore>,
    execution_store: Arc<ExecutionStore>,
    executor: Arc<Executor>,
    trigger_manager: Arc<TriggerManager>,
    scheduler: Arc<Scheduler>,
    running_executions: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub async fn new<P: AsRef<std::path::Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;

        let workflow_store = WorkflowStore::open(data_dir.join("workflows.redb"))?;
        let execution_store = ExecutionStore::open(data_dir.join("executions.redb"))?;
        let executor = Arc::new(Executor::new());
        let trigger_manager = Arc::new(TriggerManager::new());
        let scheduler = Arc::new(Scheduler::new()?);

        Ok(Self {
            workflow_store,
            execution_store,
            executor,
            trigger_manager,
            scheduler,
            running_executions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Register a workflow
    pub async fn register_workflow(&self, workflow: Workflow) -> Result<()> {
        workflow.validate()?;
        self.workflow_store.save(&workflow)?;

        // Register triggers
        for trigger in &workflow.triggers {
            self.trigger_manager.register(
                workflow.id.clone(),
                trigger.clone(),
                self.executor.clone(),
            ).await?;
        }

        Ok(())
    }

    /// Unregister a workflow
    pub async fn unregister_workflow(&self, id: &str) -> Result<()> {
        self.workflow_store.delete(id)?;

        // Unregister triggers
        self.trigger_manager.unregister_workflow(id).await;

        Ok(())
    }

    /// Get a workflow
    pub async fn get_workflow(&self, id: &str) -> Result<Option<Workflow>> {
        Ok(self.workflow_store.load(id)?)
    }

    /// List all workflows
    pub async fn list_workflows(&self) -> Result<Vec<Workflow>> {
        Ok(self.workflow_store.load_all()?)
    }

    /// Execute a workflow
    pub async fn execute_workflow(&self, id: &str) -> Result<ExecutionResult> {
        let workflow = self.workflow_store.load(id)?
            .ok_or_else(|| WorkflowError::WorkflowNotFound(id.to_string()))?;

        if !workflow.enabled {
            return Err(WorkflowError::ExecutionError("Workflow is disabled".into()));
        }

        let execution_id = uuid::Uuid::new_v4().to_string();
        let mut context = ExecutionContext::new(workflow.id.clone(), execution_id.clone());

        // Create execution record
        let mut record = ExecutionRecord {
            id: execution_id.clone(),
            workflow_id: workflow.id.clone(),
            status: ExecutionStatus::Running,
            started_at: context.started_at,
            completed_at: None,
            step_results: HashMap::new(),
            error: None,
            logs: context.logs.clone(),
        };

        // Save initial record
        self.execution_store.save(&record)?;

        // Execute each step
        for step in &workflow.steps {
            let result = tokio::time::timeout(
                tokio::time::Duration::from_secs(workflow.timeout_seconds),
                self.executor.execute_step(step, &mut context),
            ).await;

            let step_result: crate::store::StepResult = match result {
                Ok(step_result) => step_result?,
                Err(e) => {
                    let step_result = crate::store::StepResult {
                        step_id: step.id().to_string(),
                        started_at: chrono::Utc::now().timestamp(),
                        completed_at: Some(chrono::Utc::now().timestamp()),
                        status: ExecutionStatus::Failed,
                        output: None,
                        error: Some(e.to_string()),
                    };
                    context.step_results.insert(step.id().to_string(), step_result.clone());

                    // Update record as failed
                    record.status = ExecutionStatus::Failed;
                    record.error = Some(e.to_string());
                    record.completed_at = Some(chrono::Utc::now().timestamp());
                    record.logs = context.logs.clone();
                    record.step_results = context.step_results.clone();
                    self.execution_store.save(&record)?;

                    return Ok(ExecutionResult {
                        execution_id: record.id.clone(),
                        status: ExecutionStatus::Failed,
                        step_results: context.step_results,
                        error: Some(e.to_string()),
                    });
                }
                Err(_) => {
                    let step_result = crate::store::StepResult {
                        step_id: step.id().to_string(),
                        started_at: chrono::Utc::now().timestamp(),
                        completed_at: Some(chrono::Utc::now().timestamp()),
                        status: ExecutionStatus::Failed,
                        output: None,
                        error: Some("Timeout".to_string()),
                    };
                    context.step_results.insert(step.id().to_string(), step_result.clone());

                    // Update record as failed
                    record.status = ExecutionStatus::Failed;
                    record.error = Some("Workflow execution timeout".to_string());
                    record.completed_at = Some(chrono::Utc::now().timestamp());
                    record.logs = context.logs.clone();
                    record.step_results = context.step_results.clone();
                    self.execution_store.save(&record)?;

                    return Ok(ExecutionResult {
                        execution_id: record.id.clone(),
                        status: ExecutionStatus::Failed,
                        step_results: context.step_results,
                        error: Some("Timeout".to_string()),
                    });
                }
            };

            context.step_results.insert(step.id().to_string(), step_result.clone());
        }

        // Update record as completed
        record.status = ExecutionStatus::Completed;
        record.completed_at = Some(chrono::Utc::now().timestamp());
        record.logs = context.logs.clone();
        record.step_results = context.step_results.clone();
        self.execution_store.save(&record)?;

        Ok(ExecutionResult {
            execution_id: record.id.clone(),
            status: ExecutionStatus::Completed,
            step_results: context.step_results,
            error: None,
        })
    }

    /// Get execution record
    pub async fn get_execution(&self, id: &str) -> Result<Option<ExecutionRecord>> {
        Ok(self.execution_store.load(id)?)
    }

    /// Get executions for a workflow
    pub async fn get_workflow_executions(&self, workflow_id: &str) -> Result<Vec<ExecutionRecord>> {
        Ok(self.execution_store.get_workflow_executions(workflow_id)?)
    }

    /// Get recent executions
    pub async fn get_recent_executions(&self, limit: usize) -> Result<Vec<ExecutionRecord>> {
        Ok(self.execution_store.get_recent(limit)?)
    }

    /// Set device manager for the executor
    pub async fn set_device_manager(&self, _manager: Arc<()>) {
        // Update executor with device manager
        // This would require internal mutability or a different structure
    }

    /// Set alert manager for the executor
    pub async fn set_alert_manager(&self, _manager: Arc<()>) {
        // Update executor with alert manager
    }

    /// Initialize WASM runtime
    pub async fn init_wasm_runtime(&self) -> Result<()> {
        self.executor.init_wasm_runtime().await
    }

    /// Start the scheduler
    pub async fn start_scheduler(&self) -> Result<()> {
        self.scheduler.start(self.trigger_manager.clone(), self.executor.clone()).await
    }

    /// Stop the scheduler
    pub async fn stop_scheduler(&self) -> Result<()> {
        self.scheduler.stop().await
    }
}

/// Execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub execution_id: String,
    pub status: ExecutionStatus,
    pub step_results: HashMap<String, crate::store::StepResult>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::Step;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_workflow_engine() {
        let temp_dir = TempDir::new().unwrap();
        let engine = WorkflowEngine::new(temp_dir.path()).await.unwrap();

        let workflow = Workflow::new("test", "Test Workflow")
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "Starting workflow".to_string(),
                level: "info".to_string(),
            })
            .with_step(Step::Log {
                id: "log2".to_string(),
                message: "Ending workflow".to_string(),
                level: "info".to_string(),
            });

        engine.register_workflow(workflow).await.unwrap();

        let result = engine.execute_workflow("test").await.unwrap();
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.step_results.len(), 2);
    }
}
