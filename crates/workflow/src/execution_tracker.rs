//! Workflow execution state tracking and management.
//!
//! This module provides runtime tracking of workflow executions,
//! including the ability to cancel running workflows.

use crate::error::{Result, WorkflowError};
use crate::store::{ExecutionRecord, ExecutionStatus, StepResult, ExecutionLog};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore, SemaphorePermit};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Execution state for tracking workflow runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionState {
    /// Unique execution ID
    pub id: String,
    /// Workflow ID
    pub workflow_id: String,
    /// Current status
    pub status: ExecutionStatus,
    /// Started at
    pub started_at: i64,
    /// Completed at
    pub completed_at: Option<i64>,
    /// Current step index
    pub current_step: Option<String>,
    /// Step results
    pub step_results: HashMap<String, StepResult>,
    /// Execution logs
    pub logs: Vec<ExecutionLog>,
    /// Error message
    pub error: Option<String>,
}

impl ExecutionState {
    /// Create a new execution state.
    pub fn new(execution_id: impl Into<String>, workflow_id: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: execution_id.into(),
            workflow_id: workflow_id.into(),
            status: ExecutionStatus::Running,
            started_at: now,
            completed_at: None,
            current_step: None,
            step_results: HashMap::new(),
            logs: Vec::new(),
            error: None,
        }
    }

    /// Add a log entry.
    pub fn log(&mut self, level: impl Into<String>, message: impl Into<String>) {
        self.logs.push(ExecutionLog {
            timestamp: chrono::Utc::now().timestamp(),
            level: level.into(),
            message: message.into(),
        });
    }

    /// Record a step result.
    pub fn record_step(&mut self, step_id: String, result: StepResult) {
        self.current_step = Some(step_id.clone());
        self.step_results.insert(step_id, result);
    }

    /// Mark as completed.
    pub fn complete(&mut self) {
        self.status = ExecutionStatus::Completed;
        self.completed_at = Some(chrono::Utc::now().timestamp());
    }

    /// Mark as failed.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = ExecutionStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(chrono::Utc::now().timestamp());
    }

    /// Mark as cancelled.
    pub fn cancel(&mut self) {
        self.status = ExecutionStatus::Cancelled;
        self.completed_at = Some(chrono::Utc::now().timestamp());
    }

    /// Check if execution is finished.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled
        )
    }

    /// Check if execution is running.
    pub fn is_running(&self) -> bool {
        self.status == ExecutionStatus::Running
    }

    /// Get progress percentage.
    pub fn progress(&self, total_steps: usize) -> f64 {
        if total_steps == 0 {
            return 0.0;
        }
        let completed = self.step_results.len();
        (completed as f64 / total_steps as f64) * 100.0
    }
}

impl From<ExecutionState> for ExecutionRecord {
    fn from(state: ExecutionState) -> Self {
        ExecutionRecord {
            id: state.id,
            workflow_id: state.workflow_id,
            status: state.status,
            started_at: state.started_at,
            completed_at: state.completed_at,
            step_results: state.step_results,
            error: state.error,
            logs: state.logs,
        }
    }
}

/// Execution permit for controlling workflow execution.
#[derive(Debug, Clone)]
pub struct ExecutionPermit {
    /// Execution ID
    pub execution_id: String,
    /// Workflow ID
    pub workflow_id: String,
}

impl ExecutionPermit {
    /// Create a new execution permit.
    fn new(execution_id: String, workflow_id: String) -> Self {
        Self {
            execution_id,
            workflow_id,
        }
    }

    /// Get the execution ID.
    pub fn id(&self) -> &str {
        &self.execution_id
    }

    /// Get the workflow ID.
    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }
}

/// Running execution handle.
#[derive(Debug)]
pub struct RunningExecution {
    /// Execution state
    state: Arc<RwLock<ExecutionState>>,
    /// Join handle
    handle: JoinHandle<Result<()>>,
    /// Total steps (for progress calculation)
    total_steps: usize,
}

impl RunningExecution {
    /// Create a new running execution.
    fn new(
        state: Arc<RwLock<ExecutionState>>,
        handle: JoinHandle<Result<()>>,
        total_steps: usize,
    ) -> Self {
        Self {
            state,
            handle,
            total_steps,
        }
    }

    /// Get the execution ID.
    pub async fn id(&self) -> String {
        let state = self.state.read().await;
        state.id.clone()
    }

    /// Get the current status.
    pub async fn status(&self) -> ExecutionStatus {
        let state = self.state.read().await;
        state.status
    }

    /// Get the current progress percentage.
    pub async fn progress(&self) -> f64 {
        let state = self.state.read().await;
        state.progress(self.total_steps)
    }

    /// Check if the execution is finished.
    pub async fn is_finished(&self) -> bool {
        let state = self.state.read().await;
        state.is_finished()
    }

    /// Get the execution state snapshot.
    pub async fn snapshot(&self) -> ExecutionState {
        let state = self.state.read().await;
        state.clone()
    }

    /// Wait for the execution to complete.
    pub async fn wait(self) -> Result<()> {
        self.handle.await.map_err(|e| {
            WorkflowError::ExecutionError(format!("Join error: {}", e))
        })?
    }
}

/// Execution tracker for managing workflow executions.
///
/// Tracks all running executions and provides methods to query status
/// and cancel executions.
pub struct ExecutionTracker {
    /// Maximum concurrent executions
    max_concurrent: usize,
    /// Semaphore for limiting concurrency
    semaphore: Arc<Semaphore>,
    /// Running executions
    running: Arc<RwLock<HashMap<String, RunningExecution>>>,
    /// Historical states (completed/cancelled/failed)
    history: Arc<RwLock<HashMap<String, ExecutionState>>>,
}

impl ExecutionTracker {
    /// Create a new execution tracker.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            running: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the maximum concurrent executions.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Start tracking an execution.
    ///
    /// Returns a permit that must be held for the duration of the execution.
    pub async fn start_execution(
        &self,
        execution_id: impl Into<String>,
        workflow_id: impl Into<String>,
        total_steps: usize,
    ) -> Result<ExecutionPermit> {
        let execution_id = execution_id.into();
        let workflow_id = workflow_id.into();

        // Acquire semaphore permit - we forget it to extend lifetime
        // The permit will be released when execution completes via complete_execution/fail_execution
        let _permit = self.semaphore.acquire().await.map_err(|e| {
            WorkflowError::ExecutionError(format!("Semaphore error: {}", e))
        })?;
        std::mem::forget(_permit);

        // Create initial state
        let state = Arc::new(RwLock::new(ExecutionState::new(
            execution_id.clone(),
            workflow_id.clone(),
        )));

        // Create a placeholder handle (will be replaced by the caller)
        let handle = tokio::spawn(async move {
            // Placeholder - will be replaced
            Ok(())
        });

        // Store in running map
        let running_exec = RunningExecution::new(state, handle, total_steps);

        let mut running = self.running.write().await;
        running.insert(execution_id.clone(), running_exec);

        info!("Started execution {} for workflow {}", execution_id, workflow_id);

        Ok(ExecutionPermit::new(execution_id, workflow_id))
    }

    /// Register the actual task handle for an execution.
    pub async fn register_handle(
        &self,
        execution_id: &str,
        handle: JoinHandle<Result<()>>,
    ) -> Result<()> {
        let mut running = self.running.write().await;

        if let Some(mut exec) = running.remove(execution_id) {
            // Create new RunningExecution with the real handle
            let snapshot = exec.snapshot().await;
            let state = exec.state.clone();
            let total_steps = exec.total_steps;

            let new_exec = RunningExecution::new(state, handle, total_steps);
            running.insert(execution_id.to_string(), new_exec);

            Ok(())
        } else {
            warn!("Execution {} not found when registering handle", execution_id);
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Update the current step for an execution.
    pub async fn update_step(&self, execution_id: &str, step_id: String) -> Result<()> {
        let mut running = self.running.write().await;

        if let Some(exec) = running.get_mut(execution_id) {
            let mut state = exec.state.write().await;
            state.current_step = Some(step_id);
            Ok(())
        } else {
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Record a step result.
    pub async fn record_step_result(
        &self,
        execution_id: &str,
        step_id: String,
        result: StepResult,
    ) -> Result<()> {
        let running = self.running.read().await;

        if let Some(exec) = running.get(execution_id) {
            let mut state = exec.state.write().await;
            state.record_step(step_id, result);
            Ok(())
        } else {
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Add a log entry to an execution.
    pub async fn log(&self, execution_id: &str, level: String, message: String) -> Result<()> {
        let running = self.running.read().await;

        if let Some(exec) = running.get(execution_id) {
            let mut state = exec.state.write().await;
            state.log(level, message);
            Ok(())
        } else {
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Mark an execution as completed.
    pub async fn complete_execution(&self, execution_id: &str) -> Result<()> {
        let mut running = self.running.write().await;

        if let Some(exec) = running.remove(execution_id) {
            let mut state = exec.state.write().await;
            state.complete();

            // Move to history
            let state_snapshot = state.clone();
            drop(state);

            let mut history = self.history.write().await;
            history.insert(execution_id.to_string(), state_snapshot);

            // Release semaphore permit
            self.semaphore.add_permits(1);

            info!("Execution {} completed", execution_id);
            Ok(())
        } else {
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Mark an execution as failed.
    pub async fn fail_execution(&self, execution_id: &str, error: String) -> Result<()> {
        let mut running = self.running.write().await;

        if let Some(exec) = running.remove(execution_id) {
            let mut state = exec.state.write().await;
            state.fail(error);

            // Move to history
            let state_snapshot = state.clone();
            drop(state);

            let mut history = self.history.write().await;
            history.insert(execution_id.to_string(), state_snapshot);

            // Release semaphore permit
            self.semaphore.add_permits(1);

            error!("Execution {} failed", execution_id);
            Ok(())
        } else {
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Cancel a running execution.
    pub async fn cancel_execution(&self, execution_id: &str) -> Result<bool> {
        let mut running = self.running.write().await;

        if let Some(exec) = running.remove(execution_id) {
            let mut state = exec.state.write().await;
            state.cancel();

            // Move to history
            let state_snapshot = state.clone();
            drop(state);

            let mut history = self.history.write().await;
            history.insert(execution_id.to_string(), state_snapshot);

            // Abort the task
            exec.handle.abort();

            // Release semaphore permit
            self.semaphore.add_permits(1);

            info!("Execution {} cancelled", execution_id);
            Ok(true)
        } else {
            // Check if it's in history (already finished)
            let history = self.history.read().await;
            if history.contains_key(execution_id) {
                return Ok(false);
            }
            Err(WorkflowError::ExecutionError(format!(
                "Execution {} not found",
                execution_id
            )))
        }
    }

    /// Get the status of an execution.
    pub async fn get_status(&self, execution_id: &str) -> Result<ExecutionStatus> {
        let running = self.running.read().await;

        if let Some(exec) = running.get(execution_id) {
            Ok(exec.status().await)
        } else {
            let history = self.history.read().await;
            if let Some(state) = history.get(execution_id) {
                Ok(state.status)
            } else {
                Err(WorkflowError::ExecutionError(format!(
                    "Execution {} not found",
                    execution_id
                )))
            }
        }
    }

    /// Get an execution snapshot.
    pub async fn get_execution(&self, execution_id: &str) -> Result<Option<ExecutionState>> {
        let running = self.running.read().await;

        if let Some(exec) = running.get(execution_id) {
            Ok(Some(exec.snapshot().await))
        } else {
            let history = self.history.read().await;
            Ok(history.get(execution_id).cloned())
        }
    }

    /// Get all running executions.
    pub async fn list_running(&self) -> Vec<ExecutionState> {
        let running = self.running.read().await;
        let mut states = Vec::new();

        for exec in running.values() {
            states.push(exec.snapshot().await);
        }

        states.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        states
    }

    /// Get execution history.
    pub async fn list_history(&self, limit: usize) -> Vec<ExecutionState> {
        let history = self.history.read().await;
        let mut states: Vec<_> = history.values().cloned().collect();

        states.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        states.truncate(limit);
        states
    }

    /// Get executions for a workflow.
    pub async fn get_workflow_executions(&self, workflow_id: &str) -> Vec<ExecutionState> {
        let mut states = Vec::new();

        // Check running
        let running = self.running.read().await;
        for exec in running.values() {
            let snapshot = exec.snapshot().await;
            if snapshot.workflow_id == workflow_id {
                states.push(snapshot);
            }
        }

        // Check history
        let history = self.history.read().await;
        for state in history.values() {
            if state.workflow_id == workflow_id {
                states.push(state.clone());
            }
        }

        states.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        states
    }

    /// Get the number of running executions.
    pub async fn running_count(&self) -> usize {
        let running = self.running.read().await;
        running.len()
    }

    /// Clear execution history older than the given timestamp.
    pub async fn cleanup_history(&self, older_than: i64) -> usize {
        let mut history = self.history.write().await;
        let mut to_remove = Vec::new();

        for (id, state) in history.iter() {
            if state.started_at < older_than && state.is_finished() {
                to_remove.push(id.clone());
            }
        }

        for id in &to_remove {
            history.remove(id);
        }

        to_remove.len()
    }
}

impl Default for ExecutionTracker {
    fn default() -> Self {
        Self::new(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execution_state() {
        let mut state = ExecutionState::new("exec1", "workflow1");

        assert_eq!(state.id, "exec1");
        assert_eq!(state.workflow_id, "workflow1");
        assert!(state.is_running());
        assert!(!state.is_finished());

        state.log("info", "Test log");
        assert_eq!(state.logs.len(), 1);

        state.complete();
        assert!(state.is_finished());
        assert!(matches!(state.status, ExecutionStatus::Completed));
    }

    #[tokio::test]
    async fn test_execution_state_progress() {
        let mut state = ExecutionState::new("exec1", "workflow1");

        state.step_results.insert("step1".to_string(), StepResult {
            step_id: "step1".to_string(),
            started_at: 1000,
            completed_at: Some(1100),
            status: ExecutionStatus::Completed,
            output: None,
            error: None,
        });

        assert_eq!(state.progress(5), 20.0); // 1/5 = 20%
    }

    #[tokio::test]
    async fn test_execution_tracker() {
        let tracker = ExecutionTracker::new(5);

        // Start an execution
        let permit = tracker.start_execution("exec1", "workflow1", 3).await.unwrap();

        assert_eq!(tracker.running_count().await, 1);

        // Check status
        let status = tracker.get_status("exec1").await.unwrap();
        assert!(matches!(status, ExecutionStatus::Running));

        // Complete execution
        tracker.complete_execution("exec1").await.unwrap();

        assert_eq!(tracker.running_count().await, 0);

        // Should now be in history
        let exec = tracker.get_execution("exec1").await.unwrap();
        assert!(exec.is_some());
        assert!(exec.unwrap().is_finished());
    }

    #[tokio::test]
    async fn test_cancel_execution() {
        let tracker = ExecutionTracker::new(5);

        tracker.start_execution("exec1", "workflow1", 3).await.unwrap();

        // Cancel execution
        let cancelled = tracker.cancel_execution("exec1").await.unwrap();
        assert!(cancelled);

        // Check status
        let status = tracker.get_status("exec1").await.unwrap();
        assert!(matches!(status, ExecutionStatus::Cancelled));
    }

    #[tokio::test]
    async fn test_list_running() {
        let tracker = ExecutionTracker::new(5);

        tracker.start_execution("exec1", "workflow1", 3).await.unwrap();
        tracker.start_execution("exec2", "workflow1", 3).await.unwrap();

        let running = tracker.list_running().await;
        assert_eq!(running.len(), 2);
    }

    #[tokio::test]
    async fn test_workflow_executions() {
        let tracker = ExecutionTracker::new(5);

        tracker.start_execution("exec1", "workflow1", 3).await.unwrap();
        tracker.start_execution("exec2", "workflow2", 3).await.unwrap();

        // Complete exec1
        tracker.complete_execution("exec1").await.unwrap();

        // Get executions for workflow1
        let executions = tracker.get_workflow_executions("workflow1").await;
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].id, "exec1");

        // Get executions for workflow2
        let executions = tracker.get_workflow_executions("workflow2").await;
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].id, "exec2");
    }

    #[tokio::test]
    async fn test_record_step_result() {
        let tracker = ExecutionTracker::new(5);

        tracker.start_execution("exec1", "workflow1", 3).await.unwrap();

        let result = StepResult {
            step_id: "step1".to_string(),
            started_at: 1000,
            completed_at: Some(1100),
            status: ExecutionStatus::Completed,
            output: Some(serde_json::json!(42)),
            error: None,
        };

        tracker.record_step_result("exec1", "step1".to_string(), result).await.unwrap();

        let exec = tracker.get_execution("exec1").await.unwrap();
        assert!(exec.is_some());
        assert_eq!(exec.unwrap().step_results.len(), 1);
    }
}
