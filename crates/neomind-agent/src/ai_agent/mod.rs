//! User-defined AI Agent system for autonomous IoT automation.
//!
//! This module provides AI Agents that users can create with natural language
//! to monitor devices, analyze data, and take actions.
//!
//! Key features:
//! - Natural language intent parsing
//! - Scheduled and event-triggered execution
//! - Persistent memory across executions
//! - Full decision process recording for verification
//! - Error recovery for long-running stability

pub mod executor;
pub mod scheduler;

use neomind_storage::{AgentExecutionRecord, AgentSchedule, AgentStatus, AiAgent, ExecutionStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

pub use executor::{AgentExecutor, AgentExecutorConfig};
pub use scheduler::{AgentScheduler, BackendSemaphores, SchedulerConfig};

/// AI Agent manager - the main entry point for user-defined agents.
///
/// Manages the lifecycle of AI agents including:
/// - Creating agents from user input
/// - Scheduling executions
/// - Tracking execution history
/// - Managing persistent memory
pub struct AiAgentManager {
    /// Agent executor
    executor: Arc<AgentExecutor>,
    /// Agent scheduler
    scheduler: Arc<AgentScheduler>,
    /// Running state
    running: Arc<RwLock<bool>>,
}

/// Configuration for creating a new AI Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentRequest {
    /// Agent name
    pub name: String,
    /// Agent role
    #[serde(default)]
    pub role: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User's natural language description
    pub user_prompt: String,
    /// Selected device IDs
    pub device_ids: Vec<String>,
    /// Selected metrics
    pub metrics: Vec<MetricSelection>,
    /// Selected commands
    pub commands: Vec<CommandSelection>,
    /// Schedule configuration
    pub schedule: AgentSchedule,
    /// Optional LLM backend ID (uses default if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
}

/// A selected metric for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSelection {
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric_name: String,
    /// Display name
    pub display_name: String,
}

/// A selected command for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSelection {
    /// Device ID
    pub device_id: String,
    /// Command name
    pub command_name: String,
    /// Display name
    pub display_name: String,
    /// Parameters template
    pub parameters: serde_json::Value,
}

/// Input passed when invoking an agent (from API, Rule, or another Agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    /// Text content from the caller
    pub content: Option<String>,
    /// Structured data from the caller
    pub data: Option<serde_json::Value>,
    /// Source of the invocation (e.g., "api", "rule:{id}", "agent:{id}")
    pub source: Option<String>,
}

/// Agent execution summary for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionSummary {
    /// Execution ID
    pub execution_id: String,
    /// Agent ID
    pub agent_id: String,
    /// Agent name
    pub agent_name: String,
    /// Execution timestamp
    pub timestamp: i64,
    /// Status
    pub status: ExecutionStatus,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Has error
    pub has_error: bool,
    /// Summary message
    pub summary: String,
}

impl AiAgentManager {
    /// Create a new AI Agent manager.
    pub async fn new(config: AgentExecutorConfig) -> Result<Arc<Self>, crate::error::NeoMindError> {
        // Create scheduler first so we can share its backend semaphores with the executor
        let scheduler_config = SchedulerConfig::default().with_env_concurrency();
        let scheduler = Arc::new(AgentScheduler::new(scheduler_config).await?);

        // Share backend semaphores between scheduler and executor
        let mut executor_config = config;
        executor_config.backend_semaphores = Some(scheduler.backend_semaphores().clone());
        // Share the GLOBAL execution semaphore too, so event-triggered
        // executions (spawned from the executor, not the scheduler) are
        // accounted against the same global concurrency bound.
        executor_config.execution_semaphore = Some(scheduler.execution_semaphore().clone());

        let executor = Arc::new(AgentExecutor::new(executor_config).await?);

        Ok(Arc::new(Self {
            executor,
            scheduler,
            running: Arc::new(RwLock::new(false)),
        }))
    }

    /// Create a new AI Agent from user input.
    pub async fn create_agent(
        &self,
        request: CreateAgentRequest,
    ) -> Result<String, crate::error::NeoMindError> {
        // Parse user intent
        let intent = self.executor.parse_intent(&request.user_prompt).await?;

        // Build resources first (before moving anything from request)
        let resources = Self::build_resources(&request);

        // Build agent from request
        let agent = AiAgent {
            id: uuid::Uuid::new_v4().to_string(),
            name: request.name.clone(),
            description: request.description.clone(),
            user_prompt: request.user_prompt,
            llm_backend_id: request.llm_backend_id,
            parsed_intent: Some(intent.clone()),
            resources,
            schedule: request.schedule,
            status: AgentStatus::Active,
            priority: 128, // Default middle priority
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: Default::default(),
            memory: Default::default(),
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            conversation_history: Default::default(),
            user_messages: Default::default(),
            conversation_summary: Default::default(),
            context_window_size: Default::default(),
            enable_tool_chaining: false, // Default disabled for backward compatibility
            max_chain_depth: 3,          // Default max depth
            tool_config: None,
            execution_mode: Default::default(),
        };

        // Save agent to storage
        self.executor.store().save_agent(&agent).await?;

        // Schedule the agent
        if agent.status == AgentStatus::Active {
            self.scheduler.schedule_agent(agent.clone()).await?;
        }

        Ok(agent.id)
    }

    /// Execute an agent immediately (manual trigger).
    pub async fn execute_agent_now(
        &self,
        agent_id: &str,
        invocation_input: Option<AgentInput>,
    ) -> Result<AgentExecutionSummary, crate::error::NeoMindError> {
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Get agent from storage
        let agent = self
            .executor
            .store()
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| crate::NeoMindError::NotFound(format!("Agent: {}", agent_id)))?;

        // Update status to executing
        self.executor
            .store()
            .update_agent_status(agent_id, AgentStatus::Executing, None)
            .await?;

        // For event-type agents invoked manually without explicit input,
        // synthesize event data from the latest bound data point so the agent
        // runs with the same triggering context as a real event (instead of
        // having to discover its own inputs via tool calls).
        use neomind_storage::ScheduleType;
        let synthetic_event =
            if agent.schedule.schedule_type == ScheduleType::Event && invocation_input.is_none() {
                self.executor.build_synthetic_event_data(&agent).await
            } else {
                None
            };

        // Execute the agent with optional invocation input
        let result = self
            .executor
            .execute_agent(agent.clone(), synthetic_event, invocation_input)
            .await;

        // Cool the event-dedup window for this agent's bound sources so a
        // real event arriving within the 60 s window doesn't fire a
        // duplicate execution. No-op for non-event agents.
        self.executor.mark_manual_execution(&agent.id).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Build summary
        let summary = match &result {
            Ok(execution) => AgentExecutionSummary {
                execution_id: execution.id.clone(),
                agent_id: agent_id.to_string(),
                agent_name: agent.name.clone(),
                timestamp,
                status: execution.status,
                duration_ms,
                has_error: execution.error.is_some(),
                summary: execution
                    .result
                    .as_ref()
                    .map(|r| r.summary.clone())
                    .unwrap_or_else(|| "No result".to_string()),
            },
            Err(e) => AgentExecutionSummary {
                execution_id: uuid::Uuid::new_v4().to_string(),
                agent_id: agent_id.to_string(),
                agent_name: agent.name.clone(),
                timestamp,
                status: ExecutionStatus::Failed,
                duration_ms,
                has_error: true,
                summary: format!("Execution failed: {}", e),
            },
        };

        // Update agent status based on result
        let new_status = if result.is_ok() {
            // A one-shot task has done its job — Completed (not Active):
            // it is no longer "running duty", and the scheduler must not
            // pick it up again. Manual invoke still works and re-Completes.
            if agent.schedule.schedule_type == ScheduleType::Once {
                AgentStatus::Completed
            } else {
                AgentStatus::Active
            }
        } else {
            AgentStatus::Error
        };
        let error_msg = result.as_ref().err().map(|e| e.to_string());
        self.executor
            .store()
            .update_agent_status(agent_id, new_status, error_msg)
            .await?;

        Ok(summary)
    }

    /// Get an agent by ID.
    pub async fn get_agent(&self, id: &str) -> Result<Option<AiAgent>, crate::error::NeoMindError> {
        Ok(self.executor.store().get_agent(id).await?)
    }

    /// List all agents with optional filter.
    pub async fn list_agents(
        &self,
        filter: neomind_storage::AgentFilter,
    ) -> Result<Vec<AiAgent>, crate::error::NeoMindError> {
        Ok(self.executor.store().query_agents(filter).await?)
    }

    /// Get recent executions for an agent.
    pub async fn get_agent_executions(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentExecutionRecord>, crate::error::NeoMindError> {
        Ok(self
            .executor
            .store()
            .get_agent_executions(agent_id, limit)
            .await?)
    }

    /// Update agent status and sync with scheduler.
    ///
    /// When pausing: unschedules the agent so it stops executing.
    /// When activating: reschedules the agent if it has a non-Event schedule.
    pub async fn update_agent_status(
        &self,
        id: &str,
        status: AgentStatus,
    ) -> Result<(), crate::error::NeoMindError> {
        use neomind_storage::ScheduleType;

        // Persist the status change
        self.executor
            .store()
            .update_agent_status(id, status, None)
            .await?;

        match status {
            AgentStatus::Paused | AgentStatus::Stopped => {
                // Remove from scheduler so it stops running
                self.scheduler.unschedule_agent(id).await?;
                tracing::info!(agent_id = %id, status = ?status, "Agent unscheduled");
            }
            AgentStatus::Active => {
                // Reschedule: load the agent and re-register with the scheduler
                if let Some(agent) = self.executor.store().get_agent(id).await? {
                    if agent.schedule.schedule_type != ScheduleType::Event {
                        self.scheduler.schedule_agent(agent).await?;
                        tracing::info!(agent_id = %id, "Agent reactivated and rescheduled");
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Delete an agent.
    pub async fn delete_agent(&self, id: &str) -> Result<(), crate::error::NeoMindError> {
        // Unschedule first (removes interval/cron tasks)
        self.scheduler.unschedule_agent(id).await?;

        // Remove from event-triggered cache immediately
        self.executor.remove_event_agent(id).await;

        // Delete from storage
        Ok(self.executor.store().delete_agent(id).await?)
    }

    /// Start the scheduler for periodic execution.
    ///
    /// This will reload all active agents from storage and reschedule them,
    /// ensuring that scheduled tasks continue after server restarts.
    pub async fn start(&self) -> Result<(), crate::error::NeoMindError> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        // Reset any agents left in `Executing` status from a previous run.
        // On a clean startup no agent can be executing — `Executing` means the
        // prior process died mid-execution (kill -9, OOM, crash, power loss).
        // Without this sweep, `reload_active_agents` (which only loads Active)
        // silently drops those agents from the scheduler forever, and the UI
        // shows them as "running" with no signal that anything is wrong.
        self.reset_stale_executing_agents().await;

        // Reload and reschedule all active agents from storage
        // This ensures scheduled tasks continue after server restarts
        self.reload_active_agents().await?;

        // Start the scheduler
        self.scheduler.start(self.executor.clone()).await?;

        tracing::info!("AiAgentManager started");
        Ok(())
    }

    /// Reload all active agents from storage and reschedule them.
    ///
    /// This is called on startup to restore scheduled tasks after a restart.
    /// Only agents with Active status and non-Event schedule types are rescheduled.
    async fn reload_active_agents(&self) -> Result<(), crate::error::NeoMindError> {
        use neomind_storage::{AgentFilter, AgentStatus, ScheduleType};

        let agents = self
            .list_agents(AgentFilter {
                status: Some(AgentStatus::Active),
                ..Default::default()
            })
            .await?;

        let mut scheduled_count = 0;
        let mut skipped_count = 0;

        for agent in agents {
            // Skip event-triggered agents (they don't need scheduling)
            if agent.schedule.schedule_type == ScheduleType::Event {
                skipped_count += 1;
                continue;
            }

            let agent_id = agent.id.clone();
            let agent_name = agent.name.clone();
            let schedule_type = agent.schedule.schedule_type.clone();

            match self.scheduler.schedule_agent(agent).await {
                Ok(()) => {
                    scheduled_count += 1;
                    tracing::debug!(
                        agent_id = %agent_id,
                        name = %agent_name,
                        schedule_type = ?schedule_type,
                        "Rescheduled active agent after startup"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        agent_id = %agent_id,
                        name = %agent_name,
                        error = %e,
                        "Failed to reschedule active agent"
                    );
                }
            }
        }

        if scheduled_count > 0 || skipped_count > 0 {
            tracing::info!(
                scheduled = scheduled_count,
                skipped = skipped_count,
                "Reloaded active agents after startup"
            );
        }

        Ok(())
    }

    /// Sweep storage for agents stuck in `Executing` status and reset them
    /// to `Active`. Called once at startup before `reload_active_agents`.
    ///
    /// An `Executing` row on startup can only mean the previous process died
    /// mid-execution (the `StatusGuard` resets status on clean shutdown,
    /// cancel, panic, and timeout — but NOT across process death). Left
    /// untreated, these agents are silently dropped by `reload_active_agents`
    /// (which filters by `Active`) and never rescheduled, while the UI shows
    /// them as "currently running" with no signal that anything is wrong.
    ///
    /// Resets to `Active` (not `Error`) because the failure was environmental
    /// (process died), not a flaw in the agent itself — the user expects the
    /// agent to keep running on its schedule after a restart.
    ///
    /// Also reactivates agents in `Error` status. Without this, any agent
    /// that landed in `Error` before the restart (transient failure, config
    /// bug, OOM, etc.) is permanently dropped from the scheduler — the user
    /// has to manually reactivate each one from the UI, with no log clue
    /// about why it stopped. After restart, all `Active` + `Error` agents
    /// are re-scheduled; if the failure condition still exists the agent
    /// will fail again and land back in `Error`, surfacing the problem in
    /// logs/UI rather than silently disappearing.
    async fn reset_stale_executing_agents(&self) {
        use neomind_storage::{AgentFilter, AgentStatus};

        let stuck = match self
            .list_agents(AgentFilter {
                status: Some(AgentStatus::Executing),
                ..Default::default()
            })
            .await
        {
            Ok(agents) => agents,
            Err(e) => {
                tracing::warn!(
                    category = "agent",
                    error = %e,
                    "Failed to scan for stale Executing agents; skipping reset"
                );
                return;
            }
        };

        if !stuck.is_empty() {
            let count = stuck.len();
            tracing::info!(
                stuck_count = count,
                "Resetting agents stuck in Executing status from previous run"
            );

            for agent in &stuck {
                let id = agent.id.clone();
                if let Err(e) = self
                    .executor
                    .store()
                    .update_agent_status(&id, AgentStatus::Active, None)
                    .await
                {
                    tracing::warn!(
                        agent_id = %id,
                        error = %e,
                        "Failed to reset stale Executing agent"
                    );
                } else {
                    tracing::info!(
                        agent_id = %id,
                        name = %agent.name,
                        "Reset stale Executing agent to Active"
                    );
                }
            }

            tracing::info!(
                reset_count = count,
                "Stale Executing agents reset to Active"
            );
        }

        // Reactivate agents in Error status so they get rescheduled.
        // See function docstring for rationale.
        let errored = match self
            .list_agents(AgentFilter {
                status: Some(AgentStatus::Error),
                ..Default::default()
            })
            .await
        {
            Ok(agents) => agents,
            Err(e) => {
                tracing::warn!(
                    category = "agent",
                    error = %e,
                    "Failed to scan for Error agents; skipping reactivation"
                );
                return;
            }
        };

        if errored.is_empty() {
            return;
        }

        let count = errored.len();
        tracing::info!(
            error_count = count,
            "Reactivating agents in Error status at startup (will re-fail if condition persists)"
        );

        for agent in &errored {
            let id = agent.id.clone();
            if let Err(e) = self
                .executor
                .store()
                .update_agent_status(&id, AgentStatus::Active, None)
                .await
            {
                tracing::warn!(
                    agent_id = %id,
                    error = %e,
                    "Failed to reactivate Error agent"
                );
            } else {
                tracing::info!(
                    agent_id = %id,
                    name = %agent.name,
                    "Reactivated Error agent to Active"
                );
            }
        }

        tracing::info!(
            reactivated_count = count,
            "Error agents reactivated to Active"
        );
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<(), crate::error::NeoMindError> {
        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        self.scheduler.stop().await?;

        tracing::info!("AiAgentManager stopped");
        Ok(())
    }

    /// Get the executor for direct access.
    pub fn executor(&self) -> &Arc<AgentExecutor> {
        &self.executor
    }

    /// Update the tool registry in the executor (e.g. after tools are initialized).
    pub fn update_tool_registry(&self, registry: Arc<crate::toolkit::ToolRegistry>) {
        self.executor.set_tool_registry(registry);
    }

    /// Get the scheduler for direct access.
    pub fn scheduler(&self) -> &Arc<AgentScheduler> {
        &self.scheduler
    }

    /// Set the global default timezone for the scheduler.
    pub async fn set_global_timezone(
        &self,
        timezone: String,
    ) -> Result<(), crate::error::NeoMindError> {
        self.scheduler
            .set_default_timezone(timezone)
            .await
            .map_err(|e| crate::NeoMindError::Config(format!("Failed to set timezone: {}", e)))
    }

    /// Get the current global default timezone.
    pub async fn get_global_timezone(&self) -> Option<String> {
        self.scheduler.get_default_timezone().await
    }

    /// Build resources from request.
    fn build_resources(request: &CreateAgentRequest) -> Vec<neomind_storage::AgentResource> {
        use neomind_storage::{AgentResource, ResourceType};

        let mut resources = Vec::new();

        // Add devices
        for device_id in &request.device_ids {
            resources.push(AgentResource {
                resource_type: ResourceType::Device,
                resource_id: device_id.clone(),
                name: device_id.clone(),
                config: serde_json::json!({}),
            });
        }

        // Add metrics
        for metric in &request.metrics {
            resources.push(AgentResource {
                resource_type: ResourceType::Metric,
                resource_id: format!("{}:{}", metric.device_id, metric.metric_name),
                name: metric.display_name.clone(),
                config: serde_json::json!({
                    "device_id": metric.device_id,
                    "metric_name": metric.metric_name,
                }),
            });
        }

        // Add commands
        for command in &request.commands {
            resources.push(AgentResource {
                resource_type: ResourceType::Command,
                resource_id: format!("{}:{}", command.device_id, command.command_name),
                name: command.display_name.clone(),
                config: serde_json::json!({
                    "device_id": command.device_id,
                    "command_name": command.command_name,
                    "parameters": command.parameters,
                }),
            });
        }

        resources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_selection_serialization() {
        let metric = MetricSelection {
            device_id: "device-1".to_string(),
            metric_name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
        };

        let json = serde_json::to_string(&metric).unwrap();
        assert!(json.contains("temperature"));
    }
}
