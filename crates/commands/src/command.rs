//! Command data structures.
//!
//! Defines the core types for device command management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique command identifier.
pub type CommandId = String;

/// Device identifier.
pub type DeviceId = String;

/// Command source origin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandSource {
    /// Command from user interaction.
    User {
        /// User ID
        user_id: String,
        /// Session ID
        session_id: Option<String>,
    },
    /// Command from LLM decision.
    Llm {
        /// LLM model name
        model: String,
        /// Decision ID
        decision_id: Option<String>,
    },
    /// Command from rule engine.
    Rule {
        /// Rule ID
        rule_id: String,
        /// Rule name
        rule_name: String,
    },
    /// Command from workflow.
    Workflow {
        /// Workflow ID
        workflow_id: String,
        /// Step ID
        step_id: String,
    },
    /// Scheduled command.
    Schedule {
        /// Schedule ID
        schedule_id: String,
    },
    /// System command.
    System {
        /// Reason
        reason: String,
    },
}

impl CommandSource {
    /// Get a string identifier for the source.
    pub fn id(&self) -> String {
        match self {
            CommandSource::User { user_id, .. } => format!("user:{}", user_id),
            CommandSource::Llm { decision_id, .. } => {
                format!("llm:{}", decision_id.as_ref().unwrap_or(&"unknown".to_string()))
            }
            CommandSource::Rule { rule_id, .. } => format!("rule:{}", rule_id),
            CommandSource::Workflow { workflow_id, step_id } => {
                format!("workflow:{}:{}", workflow_id, step_id)
            }
            CommandSource::Schedule { schedule_id } => format!("schedule:{}", schedule_id),
            CommandSource::System { .. } => "system".to_string(),
        }
    }

    /// Get the source type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            CommandSource::User { .. } => "user",
            CommandSource::Llm { .. } => "llm",
            CommandSource::Rule { .. } => "rule",
            CommandSource::Workflow { .. } => "workflow",
            CommandSource::Schedule { .. } => "schedule",
            CommandSource::System { .. } => "system",
        }
    }
}

/// Command priority levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandPriority {
    /// Low priority - background operations
    Low = 1,
    /// Normal priority - regular operations
    Normal = 2,
    /// High priority - user-initiated operations
    High = 3,
    /// Critical priority - urgent operations
    Critical = 4,
    /// Emergency priority - safety-critical operations
    Emergency = 5,
}

impl CommandPriority {
    /// Get the priority value.
    pub fn value(&self) -> u8 {
        *self as u8
    }

    /// Get priority from integer value.
    pub fn from_value(value: u8) -> Option<Self> {
        match value {
            1 => Some(CommandPriority::Low),
            2 => Some(CommandPriority::Normal),
            3 => Some(CommandPriority::High),
            4 => Some(CommandPriority::Critical),
            5 => Some(CommandPriority::Emergency),
            _ => None,
        }
    }

    /// Get the priority type name.
    pub fn type_name(&self) -> &str {
        match self {
            CommandPriority::Low => "low",
            CommandPriority::Normal => "normal",
            CommandPriority::High => "high",
            CommandPriority::Critical => "critical",
            CommandPriority::Emergency => "emergency",
        }
    }
}

impl Default for CommandPriority {
    fn default() -> Self {
        CommandPriority::Normal
    }
}

/// Command status tracking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandStatus {
    /// Command created, waiting to be queued
    Pending,
    /// Command queued, waiting to be sent
    Queued,
    /// Command is being sent to device
    Sending,
    /// Command sent, waiting for acknowledgment
    WaitingAck,
    /// Command completed successfully
    Completed,
    /// Command failed
    Failed,
    /// Command cancelled
    Cancelled,
    /// Command timed out
    Timeout,
}

impl CommandStatus {
    /// Check if command is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            CommandStatus::Completed | CommandStatus::Failed | CommandStatus::Cancelled | CommandStatus::Timeout
        )
    }

    /// Check if command is in a success state.
    pub fn is_success(&self) -> bool {
        matches!(self, CommandStatus::Completed)
    }

    /// Check if command can be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(self, CommandStatus::Failed | CommandStatus::Timeout)
    }
}

/// Command execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    /// Whether command succeeded
    pub success: bool,
    /// Result message
    pub message: String,
    /// Device response data
    pub response_data: Option<serde_json::Value>,
    /// Timestamp of result
    pub completed_at: DateTime<Utc>,
}

impl CommandResult {
    /// Create a successful result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            response_data: None,
            completed_at: Utc::now(),
        }
    }

    /// Create a successful result with data.
    pub fn success_with_data(message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            response_data: Some(data),
            completed_at: Utc::now(),
        }
    }

    /// Create a failed result.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            response_data: None,
            completed_at: Utc::now(),
        }
    }
}

/// Command retry policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial retry delay in milliseconds
    pub initial_delay_ms: u64,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Maximum delay between retries
    pub max_delay_ms: u64,
    /// Retryable error codes/patterns
    pub retryable_errors: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 1000,
            backoff_multiplier: 2.0,
            max_delay_ms: 30000,
            retryable_errors: vec![
                "timeout".to_string(),
                "connection".to_string(),
                "temporary".to_string(),
            ],
        }
    }
}

impl RetryPolicy {
    /// Calculate retry delay for a given attempt.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay = (self.initial_delay_ms as f64
            * self.backoff_multiplier.powi(attempt.saturating_sub(1) as i32))
            .min(self.max_delay_ms as f64) as u64;
        Duration::from_millis(delay)
    }

    /// Check if an error is retryable.
    pub fn is_retryable(&self, error: &str) -> bool {
        let error_lower = error.to_lowercase();
        for pattern in &self.retryable_errors {
            if error_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }
        false
    }

    /// No retry policy.
    pub fn none() -> Self {
        Self {
            max_attempts: 0,
            ..Default::default()
        }
    }

    /// Retry forever (for critical commands).
    pub fn forever() -> Self {
        Self {
            max_attempts: u32::MAX,
            ..Default::default()
        }
    }
}

use std::time::Duration;

/// Device command request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    /// Unique command ID
    pub id: CommandId,
    /// Target device ID
    pub device_id: DeviceId,
    /// Command name (e.g., "turn_on", "set_brightness")
    pub command_name: String,
    /// Command parameters
    pub parameters: serde_json::Value,
    /// Command priority
    pub priority: CommandPriority,
    /// Timeout in seconds
    pub timeout_secs: u64,
    /// Command source
    pub source: CommandSource,
    /// Retry policy
    pub retry_policy: RetryPolicy,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Scheduled execution time (None = immediate)
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Current status
    pub status: CommandStatus,
    /// Current attempt number
    pub attempt: u32,
    /// Execution result
    pub result: Option<CommandResult>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl CommandRequest {
    /// Create a new command request.
    pub fn new(
        device_id: DeviceId,
        command_name: String,
        source: CommandSource,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            device_id,
            command_name,
            parameters: serde_json::json!({}),
            priority: CommandPriority::Normal,
            timeout_secs: 30,
            source,
            retry_policy: RetryPolicy::default(),
            created_at: Utc::now(),
            scheduled_at: None,
            status: CommandStatus::Pending,
            attempt: 0,
            result: None,
            metadata: None,
        }
    }

    /// Set command parameters.
    pub fn with_parameters(mut self, parameters: serde_json::Value) -> Self {
        self.parameters = parameters;
        self
    }

    /// Set command priority.
    pub fn with_priority(mut self, priority: CommandPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set command timeout.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set retry policy.
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Set scheduled execution time.
    pub fn with_schedule(mut self, scheduled_at: DateTime<Utc>) -> Self {
        self.scheduled_at = Some(scheduled_at);
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Check if command is ready to execute.
    pub fn is_ready(&self) -> bool {
        if let Some(scheduled) = self.scheduled_at {
            Utc::now() >= scheduled
        } else {
            true
        }
    }

    /// Check if command has expired.
    pub fn is_expired(&self) -> bool {
        // Command expires after creation + timeout + some buffer
        let expiry = self.created_at + chrono::Duration::seconds(self.timeout_secs as i64 + 60);
        Utc::now() > expiry
    }

    /// Check if command can be retried.
    pub fn can_retry(&self) -> bool {
        self.status.is_retryable()
            && self.attempt < self.retry_policy.max_attempts
    }

    /// Increment attempt counter.
    pub fn increment_attempt(&mut self) {
        self.attempt += 1;
    }

    /// Update command status.
    pub fn update_status(&mut self, status: CommandStatus) {
        self.status = status;
    }

    /// Set command result.
    pub fn set_result(&mut self, result: CommandResult) {
        let success = result.success;
        self.result = Some(result);
        if success {
            self.status = CommandStatus::Completed;
        } else {
            self.status = CommandStatus::Failed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_priority_ord() {
        assert!(CommandPriority::Emergency > CommandPriority::Critical);
        assert!(CommandPriority::High > CommandPriority::Normal);
        assert!(CommandPriority::Normal > CommandPriority::Low);
    }

    #[test]
    fn test_priority_from_value() {
        assert_eq!(CommandPriority::from_value(1), Some(CommandPriority::Low));
        assert_eq!(CommandPriority::from_value(3), Some(CommandPriority::High));
        assert_eq!(CommandPriority::from_value(99), None);
    }

    #[test]
    fn test_status_is_terminal() {
        assert!(CommandStatus::Completed.is_terminal());
        assert!(CommandStatus::Failed.is_terminal());
        assert!(CommandStatus::Cancelled.is_terminal());
        assert!(CommandStatus::Timeout.is_terminal());
        assert!(!CommandStatus::Pending.is_terminal());
        assert!(!CommandStatus::Queued.is_terminal());
    }

    #[test]
    fn test_status_is_retryable() {
        assert!(CommandStatus::Failed.is_retryable());
        assert!(CommandStatus::Timeout.is_retryable());
        assert!(!CommandStatus::Cancelled.is_retryable());
        assert!(!CommandStatus::Completed.is_retryable());
    }

    #[test]
    fn test_retry_policy_delay() {
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_delay_ms: 1000,
            backoff_multiplier: 2.0,
            max_delay_ms: 10000,
            retryable_errors: vec!["timeout".to_string()],
        };

        assert_eq!(policy.delay_for_attempt(1).as_millis(), 1000);
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 2000);
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 4000);
        assert_eq!(policy.delay_for_attempt(4).as_millis(), 8000);
        assert_eq!(policy.delay_for_attempt(5).as_millis(), 10000); // maxed
    }

    #[test]
    fn test_command_creation() {
        let source = CommandSource::User {
            user_id: "user1".to_string(),
            session_id: Some("session1".to_string()),
        };
        let cmd = CommandRequest::new("device1".to_string(), "turn_on".to_string(), source);

        assert_eq!(cmd.device_id, "device1");
        assert_eq!(cmd.command_name, "turn_on");
        assert_eq!(cmd.status, CommandStatus::Pending);
        assert!(cmd.is_ready());
    }

    #[test]
    fn test_command_with_parameters() {
        let source = CommandSource::User {
            user_id: "user1".to_string(),
            session_id: None,
        };
        let cmd = CommandRequest::new("device1".to_string(), "set_brightness".to_string(), source)
            .with_parameters(serde_json::json!({"brightness": 80}))
            .with_priority(CommandPriority::High)
            .with_timeout(60);

        assert_eq!(cmd.parameters["brightness"], 80);
        assert_eq!(cmd.priority, CommandPriority::High);
        assert_eq!(cmd.timeout_secs, 60);
    }

    #[test]
    fn test_command_retry() {
        let source = CommandSource::System {
            reason: "test".to_string(),
        };
        let mut cmd = CommandRequest::new("device1".to_string(), "test".to_string(), source)
            .with_retry_policy(RetryPolicy::default());

        cmd.update_status(CommandStatus::Timeout);
        assert!(cmd.can_retry());

        cmd.increment_attempt();
        assert_eq!(cmd.attempt, 1);
    }
}
