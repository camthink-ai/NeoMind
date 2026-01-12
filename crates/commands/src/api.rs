//! Command API for external integration.
//!
//! Provides HTTP/gRPC API endpoints for command management.

use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::command::{
    CommandRequest, CommandId, DeviceId, CommandSource, CommandPriority,
    CommandStatus, CommandResult, RetryPolicy,
};
use crate::queue::{CommandQueue, QueueError};
use crate::state::{CommandManager, CommandStateStore, StateError, StoreStats};
use crate::ack::{AckHandler, AckStatus};

/// API request for submitting a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCommandRequest {
    /// Target device ID
    pub device_id: DeviceId,
    /// Command name
    pub command_name: String,
    /// Command parameters
    #[serde(default)]
    pub parameters: serde_json::Value,
    /// Command priority
    #[serde(default)]
    pub priority: Option<String>,
    /// Timeout in seconds
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Scheduled execution time
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Source information
    pub source: CommandSourceInfo,
    /// Metadata
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Command source information for API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSourceInfo {
    /// Source type
    pub source_type: String,
    /// Source ID
    pub source_id: Option<String>,
    /// Additional context
    pub context: Option<serde_json::Value>,
}

/// API response for command submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCommandResponse {
    /// Command ID
    pub command_id: CommandId,
    /// Current status
    pub status: CommandStatus,
    /// Estimated execution time
    pub estimated_at: Option<DateTime<Utc>>,
}

/// API response for command status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStatusResponse {
    /// Command ID
    pub command_id: CommandId,
    /// Device ID
    pub device_id: DeviceId,
    /// Command name
    pub command_name: String,
    /// Current status
    pub status: CommandStatus,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Result if available
    pub result: Option<CommandResult>,
    /// Current attempt
    pub attempt: u32,
    /// Acknowledgment status
    pub ack_status: Option<AckStatus>,
}

/// API response for command list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCommandsResponse {
    /// Commands
    pub commands: Vec<CommandStatusResponse>,
    /// Total count
    pub total_count: usize,
    /// Offset
    pub offset: usize,
    /// Limit
    pub limit: usize,
}

/// Statistics response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticsResponse {
    /// Queue statistics
    pub queue_stats: QueueStatistics,
    /// State statistics
    pub state_stats: StateStatistics,
    /// Acknowledgment statistics
    pub ack_stats: AckStatistics,
}

/// Queue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatistics {
    /// Total commands in queue
    pub total_count: usize,
    /// Count by priority
    pub by_priority: Vec<(String, usize)>,
    /// Processed count
    pub processed_count: u64,
    /// Failed count
    pub failed_count: u64,
}

/// State statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateStatistics {
    /// Total commands
    pub total_count: usize,
    /// Count by status
    pub by_status: Vec<(String, usize)>,
    /// Cache size
    pub cache_size: usize,
}

/// Acknowledgment statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckStatistics {
    /// Pending acknowledgments
    pub pending_count: usize,
    /// Acknowledged count
    pub acknowledged_count: usize,
    /// Timeout count
    pub timeout_count: usize,
}

/// Command API for handling command requests.
pub struct CommandApi {
    /// Command manager
    manager: Arc<CommandManager>,
    /// Acknowledgment handler
    ack_handler: Arc<AckHandler>,
}

impl CommandApi {
    /// Create a new command API.
    pub fn new(
        manager: Arc<CommandManager>,
        ack_handler: Arc<AckHandler>,
    ) -> Self {
        Self { manager, ack_handler }
    }

    /// Submit a command.
    pub async fn submit_command(
        &self,
        request: SubmitCommandRequest,
    ) -> Result<SubmitCommandResponse, ApiError> {
        // Convert API request to CommandRequest
        let source = self.convert_source(request.source);
        let priority = self.parse_priority(request.priority.as_deref());
        let retry_policy = RetryPolicy::default();

        let mut command = CommandRequest::new(
            request.device_id,
            request.command_name,
            source,
        )
        .with_parameters(request.parameters)
        .with_priority(priority)
        .with_timeout(request.timeout_secs.unwrap_or(30))
        .with_retry_policy(retry_policy);

        if let Some(scheduled) = request.scheduled_at {
            command = command.with_schedule(scheduled);
        }

        if let Some(metadata) = request.metadata {
            command = command.with_metadata(metadata);
        }

        // Submit through manager
        let id = self.manager.submit(command).await
            .map_err(|e| ApiError::SubmissionFailed(e.to_string()))?;

        // Get status
        let status = self.manager.get_status(&id).await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(SubmitCommandResponse {
            command_id: id,
            status,
            estimated_at: None,
        })
    }

    /// Get command status.
    pub async fn get_command_status(
        &self,
        command_id: &CommandId,
    ) -> Result<CommandStatusResponse, ApiError> {
        let cmd = self.manager.state.get(command_id).await
            .map_err(|e| ApiError::NotFound(command_id.clone()))?;

        let ack_status = self.ack_handler.get_status(command_id).await;

        Ok(CommandStatusResponse {
            command_id: cmd.id.clone(),
            device_id: cmd.device_id,
            command_name: cmd.command_name,
            status: cmd.status,
            created_at: cmd.created_at,
            result: cmd.result,
            attempt: cmd.attempt,
            ack_status,
        })
    }

    /// List commands for a device.
    pub async fn list_device_commands(
        &self,
        device_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ListCommandsResponse, ApiError> {
        let commands = self.manager.list_device_commands(device_id).await;

        let total_count = commands.len();
        let end = (offset + limit).min(total_count);

        let mut page = Vec::new();
        for cmd in commands.into_iter().skip(offset).take(end - offset) {
            let ack_status = self.ack_handler.get_status(&cmd.id).await;
            page.push(CommandStatusResponse {
                command_id: cmd.id.clone(),
                device_id: cmd.device_id,
                command_name: cmd.command_name,
                status: cmd.status,
                created_at: cmd.created_at,
                result: cmd.result,
                attempt: cmd.attempt,
                ack_status,
            });
        }

        Ok(ListCommandsResponse {
            commands: page,
            total_count,
            offset,
            limit,
        })
    }

    /// Cancel a command.
    pub async fn cancel_command(&self, command_id: &CommandId) -> Result<(), ApiError> {
        self.manager.cancel(command_id).await
            .map_err(|e| ApiError::CancelFailed(e.to_string()))?;
        Ok(())
    }

    /// Retry a failed command.
    pub async fn retry_command(&self, command_id: &CommandId) -> Result<(), ApiError> {
        self.manager.retry(command_id).await
            .map_err(|e| ApiError::RetryFailed(e.to_string()))?;
        Ok(())
    }

    /// Get statistics.
    pub async fn get_statistics(&self) -> Result<StatisticsResponse, ApiError> {
        let queue_stats = self.manager.queue_stats().await;
        let state_stats = self.manager.state_stats().await;
        let ack_count = self.ack_handler.pending_count().await;

        Ok(StatisticsResponse {
            queue_stats: QueueStatistics {
                total_count: queue_stats.total_count,
                by_priority: queue_stats.by_priority.iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
                processed_count: queue_stats.processed_count,
                failed_count: queue_stats.failed_count,
            },
            state_stats: StateStatistics {
                total_count: state_stats.total_count,
                by_status: state_stats.by_status.iter()
                    .map(|(s, c)| (format!("{:?}", s), *c))
                    .collect(),
                cache_size: state_stats.cache_size,
            },
            ack_stats: AckStatistics {
                pending_count: ack_count,
                acknowledged_count: 0, // Would need to track this
                timeout_count: 0,      // Would need to track this
            },
        })
    }

    /// Clean up old commands.
    pub async fn cleanup(&self, older_than_secs: i64) -> Result<usize, ApiError> {
        let count = self.manager.cleanup(older_than_secs).await;
        let _ = self.ack_handler.cleanup(older_than_secs).await;
        Ok(count)
    }

    /// Convert API source info to CommandSource.
    fn convert_source(&self, info: CommandSourceInfo) -> CommandSource {
        match info.source_type.as_str() {
            "user" => CommandSource::User {
                user_id: info.source_id.unwrap_or_default(),
                session_id: None,
            },
            "llm" => CommandSource::Llm {
                model: info.source_id.unwrap_or_default(),
                decision_id: None,
            },
            "rule" => CommandSource::Rule {
                rule_id: info.source_id.unwrap_or_default(),
                rule_name: "api_rule".to_string(),
            },
            "workflow" => {
                let step_id = info.context
                    .as_ref()
                    .and_then(|c| c.get("step_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                CommandSource::Workflow {
                    workflow_id: info.source_id.unwrap_or_default(),
                    step_id,
                }
            },
            "schedule" => CommandSource::Schedule {
                schedule_id: info.source_id.unwrap_or_default(),
            },
            _ => CommandSource::System {
                reason: format!("api: {}", info.source_type),
            },
        }
    }

    /// Parse priority string.
    fn parse_priority(&self, priority: Option<&str>) -> CommandPriority {
        match priority {
            Some("low") => CommandPriority::Low,
            Some("normal") => CommandPriority::Normal,
            Some("high") => CommandPriority::High,
            Some("critical") => CommandPriority::Critical,
            Some("emergency") => CommandPriority::Emergency,
            _ => CommandPriority::Normal,
        }
    }
}

/// API error types.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Command not found: {0}")]
    NotFound(CommandId),

    #[error("Failed to submit command: {0}")]
    SubmissionFailed(String),

    #[error("Failed to cancel command: {0}")]
    CancelFailed(String),

    #[error("Failed to retry command: {0}")]
    RetryFailed(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Bad request: {0}")]
    BadRequest(String),
}

/// Command event for webhook notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvent {
    /// Event type
    pub event_type: String,
    /// Command ID
    pub command_id: CommandId,
    /// Device ID
    pub device_id: DeviceId,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Event data
    pub data: serde_json::Value,
}

/// Event publisher for command events.
pub struct EventPublisher {
    /// Webhook URLs
    webhooks: Arc<RwLock<Vec<String>>>,
    /// Event channel
    event_tx: mpsc::Sender<CommandEvent>,
}

impl EventPublisher {
    /// Create a new event publisher.
    pub fn new() -> Self {
        let (event_tx, _event_rx) = mpsc::channel(1000);

        Self {
            webhooks: Arc::new(RwLock::new(Vec::new())),
            event_tx,
        }
    }

    /// Add a webhook URL.
    pub async fn add_webhook(&self, url: String) {
        let mut webhooks = self.webhooks.write().await;
        if !webhooks.contains(&url) {
            webhooks.push(url);
        }
    }

    /// Remove a webhook URL.
    pub async fn remove_webhook(&self, url: &str) {
        let mut webhooks = self.webhooks.write().await;
        webhooks.retain(|w| w != url);
    }

    /// Publish an event.
    pub async fn publish(&self, event: CommandEvent) {
        let _ = self.event_tx.try_send(event);
        // In a real implementation, this would send to webhooks
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> mpsc::Receiver<CommandEvent> {
        let (_tx, rx) = mpsc::channel(1000);
        rx
    }
}

impl Default for EventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::CommandQueue;
    use std::sync::Arc;

    #[test]
    fn test_parse_priority() {
        let api = CommandApi {
            manager: Arc::new(CommandManager::new(
                Arc::new(CommandQueue::new(100)),
                Arc::new(CommandStateStore::new(100)),
            )),
            ack_handler: Arc::new(AckHandler::new(
                Arc::new(CommandStateStore::new(100)),
                Default::default(),
            )),
        };

        assert_eq!(api.parse_priority(Some("low")), CommandPriority::Low);
        assert_eq!(api.parse_priority(Some("high")), CommandPriority::High);
        assert_eq!(api.parse_priority(Some("normal")), CommandPriority::Normal);
        assert_eq!(api.parse_priority(None), CommandPriority::Normal);
        assert_eq!(api.parse_priority(Some("invalid")), CommandPriority::Normal);
    }

    #[test]
    fn test_convert_source() {
        let api = CommandApi {
            manager: Arc::new(CommandManager::new(
                Arc::new(CommandQueue::new(100)),
                Arc::new(CommandStateStore::new(100)),
            )),
            ack_handler: Arc::new(AckHandler::new(
                Arc::new(CommandStateStore::new(100)),
                Default::default(),
            )),
        };

        let user_info = CommandSourceInfo {
            source_type: "user".to_string(),
            source_id: Some("user123".to_string()),
            context: None,
        };

        let source = api.convert_source(user_info);
        assert!(matches!(source, CommandSource::User { .. }));

        let system_info = CommandSourceInfo {
            source_type: "system".to_string(),
            source_id: None,
            context: None,
        };

        let source = api.convert_source(system_info);
        assert!(matches!(source, CommandSource::System { .. }));
    }
}
