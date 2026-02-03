//! Message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer, Deserializer};
use uuid::Uuid;


/// Unique message identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Message severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MessageSeverity {
    /// Informational - no action required
    #[default]
    Info = 0,
    /// Warning - potential issue
    Warning = 1,
    /// Critical - action required
    Critical = 2,
    /// Emergency - immediate action required
    Emergency = 3,
}

impl MessageSeverity {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }

    // Serialize as lowercase string
    pub fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }

    // Deserialize from lowercase string
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_string(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid severity: {}", s))
        })
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Info => "信息",
            Self::Warning => "警告",
            Self::Critical => "严重",
            Self::Emergency => "紧急",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "info" | "信息" => Some(Self::Info),
            "warning" | "warn" | "警告" => Some(Self::Warning),
            "critical" | "crit" | "严重" => Some(Self::Critical),
            "emergency" | "emerg" | "紧急" => Some(Self::Emergency),
            _ => None,
        }
    }

    pub fn level(&self) -> u8 {
        *self as u8
    }
}

impl std::fmt::Display for MessageSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Message status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageStatus {
    /// Message is active and requires attention
    #[default]
    Active,
    /// Message has been acknowledged
    Acknowledged,
    /// Message has been resolved
    Resolved,
    /// Message has been archived
    Archived,
}

impl MessageStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::Archived => "archived",
        }
    }

    // Serialize as lowercase string
    pub fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }

    // Deserialize from lowercase string
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_string(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid status: {}", s))
        })
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Active => "活跃",
            Self::Acknowledged => "已确认",
            Self::Resolved => "已解决",
            Self::Archived => "已归档",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" | "活跃" => Some(Self::Active),
            "acknowledged" | "已确认" => Some(Self::Acknowledged),
            "resolved" | "已解决" => Some(Self::Resolved),
            "archived" | "已归档" => Some(Self::Archived),
            _ => None,
        }
    }
}

impl std::fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// A message representing a notification about a system event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier
    pub id: MessageId,
    /// Message category
    pub category: String,
    /// Message severity
    #[serde(serialize_with = "MessageSeverity::serialize", deserialize_with = "MessageSeverity::deserialize")]
    pub severity: MessageSeverity,
    /// Message title
    pub title: String,
    /// Message content
    pub message: String,
    /// Source of the message (e.g., device ID, rule ID)
    pub source: String,
    /// Source type (e.g., "device", "rule", "system")
    pub source_type: String,
    /// When the message was created
    pub timestamp: DateTime<Utc>,
    /// Current message status
    #[serde(serialize_with = "MessageStatus::serialize", deserialize_with = "MessageStatus::deserialize")]
    pub status: MessageStatus,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Associated tags
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Message {
    /// Create a new message.
    pub fn new(
        category: impl Into<String>,
        severity: MessageSeverity,
        title: String,
        message: String,
        source: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: MessageId::new(),
            category: category.into(),
            severity,
            title,
            message,
            source,
            source_type: "system".to_string(),
            timestamp: now,
            status: MessageStatus::Active,
            metadata: None,
            tags: Vec::new(),
        }
    }

    /// Create an alert message.
    pub fn alert(severity: MessageSeverity, title: String, message: String, source: String) -> Self {
        Self::new("alert", severity, title, message, source)
    }

    /// Create a system message.
    pub fn system(title: String, message: String) -> Self {
        Self::new("system", MessageSeverity::Info, title, message, "system".to_string())
    }

    /// Create a system message with severity.
    pub fn system_with_severity(severity: MessageSeverity, title: String, message: String) -> Self {
        Self::new("system", severity, title, message, "system".to_string())
    }

    /// Create a business message.
    pub fn business(title: String, message: String, source: String) -> Self {
        Self::new("business", MessageSeverity::Info, title, message, source)
    }

    /// Create a device message.
    pub fn device(
        severity: MessageSeverity,
        title: String,
        message: String,
        device_id: String,
    ) -> Self {
        let mut msg = Self::alert(severity, title, message, device_id.clone());
        msg.source_type = "device".to_string();
        msg.tags.push("device".to_string());
        msg
    }

    /// Create a rule message.
    pub fn rule(severity: MessageSeverity, title: String, message: String, rule_id: String) -> Self {
        let mut msg = Self::alert(severity, title, message, rule_id.clone());
        msg.source_type = "rule".to_string();
        msg.tags.push("rule".to_string());
        msg
    }

    /// Set the message status.
    pub fn with_status(mut self, status: MessageStatus) -> Self {
        self.status = status;
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add a tag.
    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Acknowledge the message.
    pub fn acknowledge(&mut self) {
        self.status = MessageStatus::Acknowledged;
    }

    /// Resolve the message.
    pub fn resolve(&mut self) {
        self.status = MessageStatus::Resolved;
    }

    /// Archive the message.
    pub fn archive(&mut self) {
        self.status = MessageStatus::Archived;
    }

    /// Check if the message is active.
    pub fn is_active(&self) -> bool {
        self.status == MessageStatus::Active
    }

    /// Get the duration since the message was created.
    pub fn duration(&self) -> chrono::Duration {
        Utc::now() - self.timestamp
    }

    /// Get a summary of the message.
    pub fn summary(&self) -> String {
        format!(
            "[{}] {} - {} (来源: {})",
            self.severity, self.title, self.message, self.source
        )
    }
}

impl Default for Message {
    fn default() -> Self {
        Self::system(
            "默认消息".to_string(),
            "这是一个默认消息".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id() {
        let id = MessageId::new();
        assert_eq!(id.0.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::new(
            "alert",
            MessageSeverity::Warning,
            "Test Message".to_string(),
            "Test message content".to_string(),
            "test_source".to_string(),
        );

        assert_eq!(msg.category, "alert");
        assert_eq!(msg.severity, MessageSeverity::Warning);
        assert_eq!(msg.title, "Test Message");
        assert_eq!(msg.message, "Test message content");
        assert_eq!(msg.source, "test_source");
        assert_eq!(msg.status, MessageStatus::Active);
    }

    #[test]
    fn test_alert_message() {
        let msg = Message::alert(
            MessageSeverity::Critical,
            "Device Offline".to_string(),
            "Temperature sensor is offline".to_string(),
            "sensor_1".to_string(),
        );

        assert_eq!(msg.category, "alert");
        assert_eq!(msg.severity, MessageSeverity::Critical);
    }

    #[test]
    fn test_device_message() {
        let msg = Message::device(
            MessageSeverity::Critical,
            "Device Offline".to_string(),
            "Temperature sensor is offline".to_string(),
            "sensor_1".to_string(),
        );

        assert_eq!(msg.source_type, "device");
        assert!(msg.tags.contains(&"device".to_string()));
    }

    #[test]
    fn test_rule_message() {
        let msg = Message::rule(
            MessageSeverity::Warning,
            "Rule Triggered".to_string(),
            "Temperature threshold exceeded".to_string(),
            "rule_1".to_string(),
        );

        assert_eq!(msg.source_type, "rule");
        assert!(msg.tags.contains(&"rule".to_string()));
    }

    #[test]
    fn test_message_status() {
        let mut msg = Message::system(
            "Test".to_string(),
            "Test message".to_string(),
        );

        assert!(msg.is_active());

        msg.acknowledge();
        assert_eq!(msg.status, MessageStatus::Acknowledged);
        assert!(!msg.is_active());

        msg.resolve();
        assert_eq!(msg.status, MessageStatus::Resolved);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(MessageSeverity::Emergency > MessageSeverity::Critical);
        assert!(MessageSeverity::Critical > MessageSeverity::Warning);
        assert!(MessageSeverity::Warning > MessageSeverity::Info);
    }

    #[test]
    fn test_severity_from_str() {
        assert_eq!(MessageSeverity::from_string("info"), Some(MessageSeverity::Info));
        assert_eq!(MessageSeverity::from_string("warning"), Some(MessageSeverity::Warning));
        assert_eq!(MessageSeverity::from_string("critical"), Some(MessageSeverity::Critical));
        assert_eq!(MessageSeverity::from_string("emergency"), Some(MessageSeverity::Emergency));
        assert_eq!(MessageSeverity::from_string("invalid"), None);
    }

    #[test]
    fn test_builder_pattern() {
        let msg = Message::new(
            "alert",
            MessageSeverity::Warning,
            "Test".to_string(),
            "Message".to_string(),
            "src".to_string(),
        )
        .with_status(MessageStatus::Resolved)
        .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);

        assert_eq!(msg.status, MessageStatus::Resolved);
        assert_eq!(msg.tags.len(), 2);
    }

    #[test]
    fn test_message_summary() {
        let msg = Message::new(
            "alert",
            MessageSeverity::Critical,
            "High Temp".to_string(),
            "Temperature is too high".to_string(),
            "sensor_1".to_string(),
        );

        let summary = msg.summary();
        assert!(summary.contains("[严重]"));
        assert!(summary.contains("High Temp"));
        assert!(summary.contains("Temperature is too high"));
        assert!(summary.contains("sensor_1"));
    }
}
