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
pub mod intent_parser;
pub mod scheduler;
pub mod llm_pool;

use edge_ai_storage::{AiAgent, AgentExecutionRecord, AgentSchedule, AgentStatus, ExecutionStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

pub use executor::{AgentExecutor, AgentExecutorConfig, ExecutionContext, AgentExecutionResult};
pub use scheduler::{AgentScheduler, ScheduledTask, SchedulerConfig, SchedulerError};

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
    pub async fn new(config: AgentExecutorConfig) -> Result<Arc<Self>, crate::error::NeoTalkError> {
        let executor = Arc::new(AgentExecutor::new(config.clone()).await?);
        let scheduler = Arc::new(AgentScheduler::new(SchedulerConfig::default()).await?);

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
    ) -> Result<String, crate::error::NeoTalkError> {
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
            conversation_history: Default::default(),
            user_messages: Default::default(),
            conversation_summary: Default::default(),
            context_window_size: Default::default(),
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
    ) -> Result<AgentExecutionSummary, crate::error::NeoTalkError> {
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Get agent from storage
        let agent = self
            .executor
            .store()
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| crate::NeoTalkError::NotFound(format!("Agent: {}", agent_id)))?;

        // Update status to executing
        self.executor
            .store()
            .update_agent_status(agent_id, AgentStatus::Executing, None)
            .await?;

        // Execute the agent
        let result = self.executor.execute_agent(agent.clone()).await;

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
            AgentStatus::Active
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
    pub async fn get_agent(&self, id: &str) -> Result<Option<AiAgent>, crate::error::NeoTalkError> {
        Ok(self.executor.store().get_agent(id).await?)
    }

    /// List all agents with optional filter.
    pub async fn list_agents(
        &self,
        filter: edge_ai_storage::AgentFilter,
    ) -> Result<Vec<AiAgent>, crate::error::NeoTalkError> {
        Ok(self.executor.store().query_agents(filter).await?)
    }

    /// Get recent executions for an agent.
    pub async fn get_agent_executions(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentExecutionRecord>, crate::error::NeoTalkError> {
        Ok(self
            .executor
            .store()
            .get_agent_executions(agent_id, limit)
            .await?)
    }

    /// Update agent status.
    pub async fn update_agent_status(
        &self,
        id: &str,
        status: AgentStatus,
    ) -> Result<(), crate::error::NeoTalkError> {
        Ok(self
            .executor
            .store()
            .update_agent_status(id, status, None)
            .await?)
    }

    /// Delete an agent.
    pub async fn delete_agent(&self, id: &str) -> Result<(), crate::error::NeoTalkError> {
        // Unschedule first
        self.scheduler.unschedule_agent(id).await?;

        // Delete from storage
        Ok(self.executor.store().delete_agent(id).await?)
    }

    /// Start the scheduler for periodic execution.
    pub async fn start(&self) -> Result<(), crate::error::NeoTalkError> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        // Start the scheduler
        self.scheduler.start(self.executor.clone()).await?;

        tracing::info!("AiAgentManager started");
        Ok(())
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<(), crate::error::NeoTalkError> {
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

    /// Get the scheduler for direct access.
    pub fn scheduler(&self) -> &Arc<AgentScheduler> {
        &self.scheduler
    }

    /// Set the global default timezone for the scheduler.
    pub async fn set_global_timezone(&self, timezone: String) -> Result<(), crate::error::NeoTalkError> {
        self.scheduler
            .set_default_timezone(timezone)
            .await
            .map_err(|e| crate::NeoTalkError::Config(format!("Failed to set timezone: {}", e)))
    }

    /// Get the current global default timezone.
    pub async fn get_global_timezone(&self) -> Option<String> {
        self.scheduler.get_default_timezone().await
    }

    /// Build resources from request.
    fn build_resources(request: &CreateAgentRequest) -> Vec<edge_ai_storage::AgentResource> {
        use edge_ai_storage::{AgentResource, ResourceType};

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
