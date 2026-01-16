//! Alert types and severity levels.
//!
//! This module defines the core alert structures used throughout
//! the NeoTalk platform.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for an alert.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlertId(pub Uuid);

impl AlertId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for AlertId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AlertId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational - no action required
    Info = 0,
    /// Warning - potential issue
    Warning = 1,
    /// Critical - action required
    Critical = 2,
    /// Emergency - immediate action required
    Emergency = 3,
}

impl AlertSeverity {
    /// Get the severity as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }

    /// Get the severity display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Info => "信息",
            Self::Warning => "警告",
            Self::Critical => "严重",
            Self::Emergency => "紧急",
        }
    }

    /// Get the severity from a string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "info" | "信息" => Some(Self::Info),
            "warning" | "warn" | "警告" => Some(Self::Warning),
            "critical" | "crit" | "严重" => Some(Self::Critical),
            "emergency" | "emerg" | "紧急" => Some(Self::Emergency),
            _ => None,
        }
    }
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl Default for AlertSeverity {
    fn default() -> Self {
        Self::Info
    }
}

/// Alert status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    /// Alert is active
    Active,
    /// Alert has been acknowledged
    Acknowledged,
    /// Alert has been resolved
    Resolved,
    /// Alert was a false positive
    FalsePositive,
}

impl AlertStatus {
    /// Get the status as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::FalsePositive => "false_positive",
        }
    }

    /// Get the status display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Active => "活跃",
            Self::Acknowledged => "已确认",
            Self::Resolved => "已解决",
            Self::FalsePositive => "误报",
        }
    }
}

impl std::fmt::Display for AlertStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl Default for AlertStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// An alert representing a notification about a system event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique alert identifier
    pub id: AlertId,
    /// Alert severity
    pub severity: AlertSeverity,
    /// Alert title
    pub title: String,
    /// Alert message/description
    pub message: String,
    /// Source of the alert (e.g., device ID, rule ID)
    pub source: String,
    /// Source type (e.g., "device", "rule", "system")
    pub source_type: String,
    /// When the alert was created
    pub timestamp: DateTime<Utc>,
    /// Current alert status
    pub status: AlertStatus,
    /// When the status was last updated
    pub status_updated_at: DateTime<Utc>,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// Associated tags
    pub tags: Vec<String>,
    /// Number of times this alert has occurred (for recurring alerts)
    pub occurrence_count: u64,
}

impl Alert {
    /// Create a new alert.
    pub fn new(severity: AlertSeverity, title: String, message: String, source: String) -> Self {
        let now = Utc::now();
        Self {
            id: AlertId::new(),
            severity,
            title,
            message,
            source,
            source_type: "system".to_string(),
            timestamp: now,
            status: AlertStatus::Active,
            status_updated_at: now,
            metadata: serde_json::Value::Object(Default::default()),
            tags: Vec::new(),
            occurrence_count: 1,
        }
    }

    /// Create a device alert.
    pub fn device(
        severity: AlertSeverity,
        title: String,
        message: String,
        device_id: String,
    ) -> Self {
        let mut alert = Self::new(severity, title, message, device_id.clone());
        alert.source_type = "device".to_string();
        alert.tags.push("device".to_string());
        alert
    }

    /// Create a rule alert.
    pub fn rule(severity: AlertSeverity, title: String, message: String, rule_id: String) -> Self {
        let mut alert = Self::new(severity, title, message, rule_id.clone());
        alert.source_type = "rule".to_string();
        alert.tags.push("rule".to_string());
        alert
    }

    /// Set the alert status.
    pub fn with_status(mut self, status: AlertStatus) -> Self {
        self.status = status;
        self.status_updated_at = Utc::now();
        self
    }

    /// Add metadata to the alert.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add tags to the alert.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag to the alert.
    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Acknowledge the alert.
    pub fn acknowledge(&mut self) {
        self.status = AlertStatus::Acknowledged;
        self.status_updated_at = Utc::now();
    }

    /// Resolve the alert.
    pub fn resolve(&mut self) {
        self.status = AlertStatus::Resolved;
        self.status_updated_at = Utc::now();
    }

    /// Mark as false positive.
    pub fn mark_false_positive(&mut self) {
        self.status = AlertStatus::FalsePositive;
        self.status_updated_at = Utc::now();
    }

    /// Increment the occurrence count.
    pub fn increment_occurrence(&mut self) {
        self.occurrence_count += 1;
    }

    /// Check if the alert is active.
    pub fn is_active(&self) -> bool {
        self.status == AlertStatus::Active
    }

    /// Get the duration since the alert was created.
    pub fn duration(&self) -> chrono::Duration {
        Utc::now() - self.timestamp
    }

    /// Get a summary of the alert.
    pub fn summary(&self) -> String {
        format!(
            "[{}] {} - {} (来源: {})",
            self.severity, self.title, self.message, self.source
        )
    }

    /// Convert to the core Alert type for use in notification channels.
    ///
    /// This converts the local full-featured Alert to the minimal core Alert
    /// type used by the AlertChannel trait. Extra fields like source_type,
    /// tags, and occurrence_count are merged into metadata.
    pub fn to_core_alert(&self) -> edge_ai_core::alerts::Alert {
        use edge_ai_core::alerts::{AlertSeverity as CoreSeverity, AlertStatus as CoreStatus};

        // Build metadata including extra fields
        let mut metadata = self.metadata.clone();
        if let serde_json::Value::Object(ref mut map) = metadata {
            map.insert("source_type".to_string(), serde_json::json!(self.source_type));
            map.insert("tags".to_string(), serde_json::json!(self.tags));
            map.insert("occurrence_count".to_string(), serde_json::json!(self.occurrence_count));
            map.insert("status_updated_at".to_string(), serde_json::json!(self.status_updated_at.timestamp()));
        }

        edge_ai_core::alerts::Alert {
            id: self.id.to_string(),
            severity: match self.severity {
                AlertSeverity::Info => CoreSeverity::Info,
                AlertSeverity::Warning => CoreSeverity::Warning,
                AlertSeverity::Critical => CoreSeverity::Critical,
                AlertSeverity::Emergency => CoreSeverity::Emergency,
            },
            title: self.title.clone(),
            message: self.message.clone(),
            source: self.source.clone(),
            timestamp: self.timestamp.timestamp(),
            status: match self.status {
                AlertStatus::Active => CoreStatus::Active,
                AlertStatus::Acknowledged => CoreStatus::Acknowledged,
                AlertStatus::Resolved => CoreStatus::Resolved,
                AlertStatus::FalsePositive => CoreStatus::FalsePositive,
            },
            metadata,
        }
    }
}

impl Default for Alert {
    fn default() -> Self {
        Self::new(
            AlertSeverity::Info,
            "默认告警".to_string(),
            "这是一个默认告警".to_string(),
            "system".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_id() {
        let id = AlertId::new();
        assert_eq!(id.0.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            AlertSeverity::Warning,
            "Test Alert".to_string(),
            "Test message".to_string(),
            "test_source".to_string(),
        );

        assert_eq!(alert.severity, AlertSeverity::Warning);
        assert_eq!(alert.title, "Test Alert");
        assert_eq!(alert.message, "Test message");
        assert_eq!(alert.source, "test_source");
        assert_eq!(alert.status, AlertStatus::Active);
        assert_eq!(alert.occurrence_count, 1);
    }

    #[test]
    fn test_device_alert() {
        let alert = Alert::device(
            AlertSeverity::Critical,
            "Device Offline".to_string(),
            "Temperature sensor is offline".to_string(),
            "sensor_1".to_string(),
        );

        assert_eq!(alert.source_type, "device");
        assert!(alert.tags.contains(&"device".to_string()));
    }

    #[test]
    fn test_rule_alert() {
        let alert = Alert::rule(
            AlertSeverity::Warning,
            "Rule Triggered".to_string(),
            "Temperature threshold exceeded".to_string(),
            "rule_1".to_string(),
        );

        assert_eq!(alert.source_type, "rule");
        assert!(alert.tags.contains(&"rule".to_string()));
    }

    #[test]
    fn test_alert_status() {
        let mut alert = Alert::new(
            AlertSeverity::Info,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
        );

        assert!(alert.is_active());

        alert.acknowledge();
        assert_eq!(alert.status, AlertStatus::Acknowledged);
        assert!(!alert.is_active());

        alert.resolve();
        assert_eq!(alert.status, AlertStatus::Resolved);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(AlertSeverity::Emergency > AlertSeverity::Critical);
        assert!(AlertSeverity::Critical > AlertSeverity::Warning);
        assert!(AlertSeverity::Warning > AlertSeverity::Info);
    }

    #[test]
    fn test_severity_from_str() {
        assert_eq!(AlertSeverity::from_str("info"), Some(AlertSeverity::Info));
        assert_eq!(
            AlertSeverity::from_str("warning"),
            Some(AlertSeverity::Warning)
        );
        assert_eq!(
            AlertSeverity::from_str("critical"),
            Some(AlertSeverity::Critical)
        );
        assert_eq!(
            AlertSeverity::from_str("emergency"),
            Some(AlertSeverity::Emergency)
        );
        assert_eq!(AlertSeverity::from_str("invalid"), None);
    }

    #[test]
    fn test_builder_pattern() {
        let alert = Alert::new(
            AlertSeverity::Warning,
            "Test".to_string(),
            "Message".to_string(),
            "src".to_string(),
        )
        .with_status(AlertStatus::Resolved)
        .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);

        assert_eq!(alert.status, AlertStatus::Resolved);
        assert_eq!(alert.tags.len(), 2);
    }

    #[test]
    fn test_occurrence_count() {
        let mut alert = Alert::new(
            AlertSeverity::Info,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
        );

        assert_eq!(alert.occurrence_count, 1);
        alert.increment_occurrence();
        assert_eq!(alert.occurrence_count, 2);
    }

    #[test]
    fn test_alert_summary() {
        let alert = Alert::new(
            AlertSeverity::Critical,
            "High Temp".to_string(),
            "Temperature is too high".to_string(),
            "sensor_1".to_string(),
        );

        let summary = alert.summary();
        assert!(summary.contains("[严重]"));
        assert!(summary.contains("High Temp"));
        assert!(summary.contains("Temperature is too high"));
        assert!(summary.contains("sensor_1"));
    }
}
