//! Core alert abstractions for NeoTalk.
//!
//! # DEPRECATED
//!
//! This module is deprecated and will be removed in a future version.
//! Alert functionality has been migrated to the `edge_ai_messages` crate.
//!
//! ## Migration Guide
//!
//! Instead of using `Alert`, `AlertSeverity`, `AlertStatus` from this module:
//! - Use `Message` and `MessageSeverity` from `edge_ai_messages`
//! - Use message channels from `edge_ai_messages::channels`
//!
//! This module is kept for backward compatibility only.

#![deprecated(since = "0.4.0", note = "Use edge_ai_messages crate instead")]
use async_trait::async_trait;

/// Result type for alert operations.
pub type Result<T> = std::result::Result<T, AlertError>;

/// Alert error types.
#[derive(Debug, thiserror::Error)]
pub enum AlertError {
    /// Channel is disabled.
    #[error("Channel '{0}' is disabled")]
    ChannelDisabled(String),

    /// Channel not found.
    #[error("Channel not found: {0}")]
    ChannelNotFound(String),

    /// Send failed.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Other error.
    #[error("Alert error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AlertSeverity {
    /// Informational.
    Info,
    /// Warning.
    Warning,
    /// Critical.
    Critical,
    /// Emergency.
    Emergency,
}

impl AlertSeverity {
    /// Get the severity level as a number (0-3).
    pub fn level(&self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Critical => 2,
            Self::Emergency => 3,
        }
    }

    /// Get the severity name.
    pub fn name(&self) -> &str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Alert status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AlertStatus {
    /// Alert is active.
    Active,
    /// Alert has been acknowledged.
    Acknowledged,
    /// Alert has been resolved.
    Resolved,
    /// Alert was a false positive.
    FalsePositive,
}

impl AlertStatus {
    /// Get the status name.
    pub fn name(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::FalsePositive => "false_positive",
        }
    }
}

impl std::fmt::Display for AlertStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Core alert data structure.
///
/// This is a minimal alert definition. Full alert functionality
/// is provided by the `edge-ai-alerts` crate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Alert {
    /// Unique alert identifier.
    pub id: String,
    /// Alert severity.
    pub severity: AlertSeverity,
    /// Alert title.
    pub title: String,
    /// Alert message.
    pub message: String,
    /// Source of the alert.
    pub source: String,
    /// Creation timestamp.
    pub timestamp: i64,
    /// Current status.
    pub status: AlertStatus,
    /// Additional metadata.
    #[serde(flatten)]
    pub metadata: serde_json::Value,
}

impl Alert {
    /// Create a new alert.
    pub fn new(
        id: impl Into<String>,
        severity: AlertSeverity,
        title: impl Into<String>,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            severity,
            title: title.into(),
            message: message.into(),
            source: source.into(),
            timestamp: chrono::Utc::now().timestamp(),
            status: AlertStatus::Active,
            metadata: serde_json::json!({}),
        }
    }

    /// Set the alert status.
    pub fn with_status(mut self, status: AlertStatus) -> Self {
        self.status = status;
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Notification channel trait.
///
/// All notification channels (console, webhook, email, etc.) implement
/// this trait to provide a unified interface for sending alerts.
#[async_trait]
pub trait AlertChannel: Send + Sync {
    /// Get the channel name.
    fn name(&self) -> &str;

    /// Get the channel type identifier.
    fn channel_type(&self) -> &str;

    /// Check if the channel is enabled.
    fn is_enabled(&self) -> bool;

    /// Send an alert through this channel.
    async fn send(&self, alert: &Alert) -> Result<()>;
}

/// Factory for creating alert channels.
pub trait ChannelFactory: Send + Sync {
    /// Channel type identifier.
    fn channel_type(&self) -> &str;

    /// Create a new channel instance with the given configuration.
    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn AlertChannel>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_severity() {
        assert_eq!(AlertSeverity::Info.level(), 0);
        assert_eq!(AlertSeverity::Emergency.level(), 3);
        assert_eq!(AlertSeverity::Warning.to_string(), "warning");
    }

    #[test]
    fn test_alert_status() {
        assert_eq!(AlertStatus::Active.name(), "active");
        assert_eq!(AlertStatus::Resolved.to_string(), "resolved");
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            "test-1",
            AlertSeverity::Warning,
            "Test Alert",
            "This is a test",
            "test-source",
        );

        assert_eq!(alert.id, "test-1");
        assert_eq!(alert.severity, AlertSeverity::Warning);
        assert_eq!(alert.status, AlertStatus::Active);

        let resolved = alert.with_status(AlertStatus::Resolved);
        assert_eq!(resolved.status, AlertStatus::Resolved);
    }
}
