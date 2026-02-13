//! Command acknowledgment handling.
//!
//! Manages acknowledgment tracking for commands sent to devices.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::command::{CommandId, CommandResult, CommandStatus};
use crate::state::CommandStateStore;

/// Acknowledgment status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AckStatus {
    /// Waiting for acknowledgment
    Waiting,
    /// Acknowledgment received
    Acknowledged,
    /// Acknowledgment timeout
    Timeout,
    /// Acknowledgment failed
    Failed,
}

/// Command acknowledgment info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAck {
    /// Command ID
    pub command_id: CommandId,
    /// Current acknowledgment status
    pub status: AckStatus,
    /// Timestamp when command was sent
    pub sent_at: DateTime<Utc>,
    /// Timestamp when acknowledgment was received
    pub acknowledged_at: Option<DateTime<Utc>>,
    /// Acknowledgment timeout in seconds
    pub timeout_secs: u64,
    /// Number of retry attempts
    pub retry_count: u32,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Additional acknowledgment data
    pub data: Option<serde_json::Value>,
}

impl CommandAck {
    /// Create a new command acknowledgment tracker.
    pub fn new(command_id: CommandId, timeout_secs: u64, max_retries: u32) -> Self {
        Self {
            command_id,
            status: AckStatus::Waiting,
            sent_at: Utc::now(),
            acknowledged_at: None,
            timeout_secs,
            retry_count: 0,
            max_retries,
            data: None,
        }
    }

    /// Check if acknowledgment has timed out.
    pub fn is_timed_out(&self) -> bool {
        if self.status != AckStatus::Waiting {
            return false;
        }

        let elapsed = Utc::now() - self.sent_at;
        elapsed > Duration::seconds(self.timeout_secs as i64)
    }

    /// Check if can retry.
    pub fn can_retry(&self) -> bool {
        (self.status == AckStatus::Timeout || self.status == AckStatus::Failed)
            && self.retry_count < self.max_retries
    }

    /// Mark as acknowledged.
    pub fn acknowledge(&mut self, data: Option<serde_json::Value>) {
        self.status = AckStatus::Acknowledged;
        self.acknowledged_at = Some(Utc::now());
        self.data = data;
    }

    /// Mark as timed out.
    pub fn timeout(&mut self) {
        self.status = AckStatus::Timeout;
    }

    /// Mark as failed.
    pub fn fail(&mut self) {
        self.status = AckStatus::Failed;
    }

    /// Increment retry count and reset to waiting.
    pub fn retry(&mut self) {
        self.retry_count += 1;
        self.status = AckStatus::Waiting;
        self.sent_at = Utc::now();
        self.acknowledged_at = None;
    }

    /// Get elapsed time since sent.
    pub fn elapsed(&self) -> Duration {
        Utc::now() - self.sent_at
    }

    /// Get remaining timeout.
    pub fn remaining_timeout(&self) -> Duration {
        let elapsed = self.elapsed();
        let timeout_duration = Duration::seconds(self.timeout_secs as i64);
        if elapsed > timeout_duration {
            Duration::seconds(0)
        } else {
            timeout_duration - elapsed
        }
    }
}

/// Acknowledgment handler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckHandlerConfig {
    /// Default acknowledgment timeout in seconds
    pub default_timeout_secs: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Check interval for timeouts
    pub check_interval_ms: u64,
    /// Enable automatic retry on timeout
    pub auto_retry: bool,
}

impl Default for AckHandlerConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
            max_retries: 3,
            check_interval_ms: 1000,
            auto_retry: true,
        }
    }
}

/// Acknowledgment event.
#[derive(Debug, Clone)]
pub enum AckEvent {
    /// Command sent, waiting for acknowledgment
    Sent {
        command_id: CommandId,
        timeout_secs: u64,
    },
    /// Acknowledgment received
    Received {
        command_id: CommandId,
        data: Option<serde_json::Value>,
    },
    /// Acknowledgment timeout
    Timeout { command_id: CommandId },
    /// Acknowledgment failed
    Failed {
        command_id: CommandId,
        error: String,
    },
    /// Command completed with result
    Completed {
        command_id: CommandId,
        result: CommandResult,
    },
}

/// Acknowledgment handler for tracking command acknowledgments.
pub struct AckHandler {
    /// Pending acknowledgments
    pending: Arc<RwLock<HashMap<CommandId, CommandAck>>>,
    /// State store
    state: Arc<CommandStateStore>,
    /// Configuration
    config: AckHandlerConfig,
    /// Event sender
    event_tx: mpsc::Sender<AckEvent>,
    /// Running flag
    running: Arc<RwLock<bool>>,
}

impl AckHandler {
    /// Create a new acknowledgment handler.
    pub fn new(state: Arc<CommandStateStore>, config: AckHandlerConfig) -> Self {
        let (event_tx, _event_rx) = mpsc::channel(1000);

        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            state,
            config,
            event_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the acknowledgment handler.
    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let mut running = self.running.write().await;
        if *running {
            drop(running);
            return tokio::spawn(async move {});
        }
        *running = true;
        drop(running);

        let pending = self.pending.clone();
        let state = self.state.clone();
        let running_flag = self.running.clone();
        let event_tx = self.event_tx.clone();
        let check_interval = tokio::time::Duration::from_millis(self.config.check_interval_ms);
        let auto_retry = self.config.auto_retry;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);

            loop {
                // Check if still running
                {
                    let r = running_flag.read().await;
                    if !*r {
                        break;
                    }
                }

                interval.tick().await;

                // Check for timed out acknowledgments
                let mut pending_lock = pending.write().await;
                let mut to_retry = vec![];

                for (command_id, ack) in pending_lock.iter_mut() {
                    if ack.is_timed_out() {
                        ack.timeout();

                        let _ = event_tx.try_send(AckEvent::Timeout {
                            command_id: command_id.clone(),
                        });

                        // Update state
                        let _ = state
                            .update_status(command_id, CommandStatus::Timeout)
                            .await;

                        if auto_retry && ack.can_retry() {
                            to_retry.push(command_id.clone());
                        }
                    }
                }

                // Auto-retry timed out commands
                for command_id in to_retry {
                    if let Some(ack) = pending_lock.get_mut(&command_id) {
                        ack.retry();
                        let _ = event_tx.try_send(AckEvent::Sent {
                            command_id: command_id.clone(),
                            timeout_secs: ack.timeout_secs,
                        });
                        // Update state back to sending
                        let _ = state
                            .update_status(&command_id, CommandStatus::Sending)
                            .await;
                    }
                }
            }
        })
    }

    /// Stop the acknowledgment handler.
    pub async fn stop(&self) {
        *self.running.write().await = false;
    }

    /// Register a command for acknowledgment tracking.
    pub async fn register(&self, command_id: CommandId, timeout_secs: Option<u64>) {
        let timeout = timeout_secs.unwrap_or(self.config.default_timeout_secs);
        let ack = CommandAck::new(command_id.clone(), timeout, self.config.max_retries);

        let mut pending = self.pending.write().await;
        pending.insert(command_id.clone(), ack);

        let _ = self.event_tx.try_send(AckEvent::Sent {
            command_id,
            timeout_secs: timeout,
        });
    }

    /// Handle acknowledgment received.
    pub async fn acknowledge(
        &self,
        command_id: &CommandId,
        data: Option<serde_json::Value>,
    ) -> Result<(), AckError> {
        let mut pending = self.pending.write().await;

        if let Some(ack) = pending.get_mut(command_id) {
            ack.acknowledge(data.clone());

            let _ = self.event_tx.try_send(AckEvent::Received {
                command_id: command_id.clone(),
                data,
            });

            // Update state
            let _ = self
                .state
                .update_status(command_id, CommandStatus::WaitingAck)
                .await;

            Ok(())
        } else {
            Err(AckError::NotFound(command_id.clone()))
        }
    }

    /// Handle command completion.
    pub async fn complete(
        &self,
        command_id: &CommandId,
        result: CommandResult,
    ) -> Result<(), AckError> {
        let mut pending = self.pending.write().await;

        if let Some(ack) = pending.get_mut(command_id) {
            ack.acknowledge(result.response_data.clone());

            // Remove from pending
            pending.remove(command_id);

            let _ = self.event_tx.try_send(AckEvent::Completed {
                command_id: command_id.clone(),
                result: result.clone(),
            });

            // Update state with result
            let _ = self.state.set_result(command_id, result).await;

            Ok(())
        } else {
            Err(AckError::NotFound(command_id.clone()))
        }
    }

    /// Handle acknowledgment failure.
    pub async fn fail(&self, command_id: &CommandId, error: String) -> Result<(), AckError> {
        let mut pending = self.pending.write().await;

        if let Some(ack) = pending.get_mut(command_id) {
            ack.fail();

            let _ = self.event_tx.try_send(AckEvent::Failed {
                command_id: command_id.clone(),
                error,
            });

            // Update state
            let _ = self
                .state
                .update_status(command_id, CommandStatus::Failed)
                .await;

            Ok(())
        } else {
            Err(AckError::NotFound(command_id.clone()))
        }
    }

    /// Get acknowledgment status for a command.
    pub async fn get_status(&self, command_id: &CommandId) -> Option<AckStatus> {
        let pending = self.pending.read().await;
        pending.get(command_id).map(|ack| ack.status.clone())
    }

    /// Get acknowledgment info for a command.
    pub async fn get(&self, command_id: &CommandId) -> Option<CommandAck> {
        let pending = self.pending.read().await;
        pending.get(command_id).cloned()
    }

    /// Get all pending acknowledgments.
    pub async fn list_pending(&self) -> Vec<CommandAck> {
        let pending = self.pending.read().await;
        pending.values().cloned().collect()
    }

    /// Get count of pending acknowledgments.
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Subscribe to acknowledgment events.
    pub fn subscribe(&self) -> mpsc::Receiver<AckEvent> {
        let (_tx, rx) = mpsc::channel(1000);
        // Note: In a real implementation, you'd want to replace the event_tx
        rx
    }

    /// Clean up completed acknowledgments.
    pub async fn cleanup(&self, older_than_secs: i64) -> usize {
        let cutoff = Utc::now() - Duration::seconds(older_than_secs);
        let mut pending = self.pending.write().await;
        let initial_len = pending.len();

        pending.retain(|_, ack| {
            // Keep if still waiting or if acknowledged recently
            ack.status == AckStatus::Waiting
                || (ack.status == AckStatus::Acknowledged
                    && ack.acknowledged_at.is_none_or(|t| t > cutoff))
        });

        initial_len - pending.len()
    }
}

/// Acknowledgment handler error types.
#[derive(Debug, thiserror::Error)]
pub enum AckError {
    #[error("Command not found: {0}")]
    NotFound(CommandId),

    #[error("State error: {0}")]
    State(String),

    #[error("Acknowledgment already completed")]
    AlreadyCompleted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CommandStateStore;
    use std::sync::Arc;

    #[test]
    fn test_command_ack_new() {
        let ack = CommandAck::new("cmd1".to_string(), 30, 3);

        assert_eq!(ack.command_id, "cmd1");
        assert_eq!(ack.status, AckStatus::Waiting);
        assert!(ack.acknowledged_at.is_none());
        assert_eq!(ack.timeout_secs, 30);
        assert_eq!(ack.retry_count, 0);
        assert!(!ack.is_timed_out());
    }

    #[test]
    fn test_command_ack_acknowledge() {
        let mut ack = CommandAck::new("cmd1".to_string(), 30, 3);

        ack.acknowledge(Some(serde_json::json!({"status": "ok"})));

        assert_eq!(ack.status, AckStatus::Acknowledged);
        assert!(ack.acknowledged_at.is_some());
        assert!(ack.data.is_some());
    }

    #[test]
    fn test_command_ack_timeout() {
        let mut ack = CommandAck::new("cmd1".to_string(), 30, 3);

        assert!(!ack.is_timed_out());

        // Set sent_at to past
        ack.sent_at = Utc::now() - Duration::seconds(31);

        assert!(ack.is_timed_out());
    }

    #[test]
    fn test_command_ack_retry() {
        let mut ack = CommandAck::new("cmd1".to_string(), 30, 3);

        ack.timeout();
        assert_eq!(ack.status, AckStatus::Timeout);
        assert!(ack.can_retry());

        ack.retry();
        assert_eq!(ack.status, AckStatus::Waiting);
        assert_eq!(ack.retry_count, 1);
    }

    #[tokio::test]
    async fn test_ack_handler_register() {
        let state = Arc::new(CommandStateStore::new(100));
        let handler = AckHandler::new(state, AckHandlerConfig::default());

        handler.register("cmd1".to_string(), Some(30)).await;

        let status = handler.get_status(&"cmd1".to_string()).await;
        assert_eq!(status, Some(AckStatus::Waiting));

        let ack = handler.get(&"cmd1".to_string()).await;
        assert!(ack.is_some());
        assert_eq!(ack.unwrap().command_id, "cmd1");
    }

    #[tokio::test]
    async fn test_ack_handler_acknowledge() {
        let state = Arc::new(CommandStateStore::new(100));
        let handler = AckHandler::new(state, AckHandlerConfig::default());

        handler.register("cmd1".to_string(), Some(30)).await;

        let result = handler
            .acknowledge(
                &"cmd1".to_string(),
                Some(serde_json::json!({"status": "ok"})),
            )
            .await;
        assert!(result.is_ok());

        let status = handler.get_status(&"cmd1".to_string()).await;
        assert_eq!(status, Some(AckStatus::Acknowledged));
    }

    #[tokio::test]
    async fn test_ack_handler_complete() {
        let state = Arc::new(CommandStateStore::new(100));
        let handler = AckHandler::new(state, AckHandlerConfig::default());

        handler.register("cmd1".to_string(), Some(30)).await;

        let result = CommandResult::success("Command completed");
        handler.complete(&"cmd1".to_string(), result).await.unwrap();

        let status = handler.get_status(&"cmd1".to_string()).await;
        // Command is removed from pending after completion
        assert_eq!(status, None);

        assert_eq!(handler.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_ack_handler_not_found() {
        let state = Arc::new(CommandStateStore::new(100));
        let handler = AckHandler::new(state, AckHandlerConfig::default());

        let result = handler.acknowledge(&"nonexistent".to_string(), None).await;
        assert!(matches!(result, Err(AckError::NotFound(_))));
    }
}
