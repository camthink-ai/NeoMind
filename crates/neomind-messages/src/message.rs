//! Message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

/// Message type (currently only Notification, extensible for future types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Human-readable notification (stored long-term)
    #[default]
    Notification,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Notification => "notification",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "notification" => Some(Self::Notification),
            _ => None,
        }
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Message severity levels.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
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
        Self::from_string(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid severity: {}", s)))
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
            Self::Emergency => "Emergency",
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
    /// An operator dismissed this alert as a false positive (002 §3.4). Not a
    /// lifecycle step like the others: it is a verdict on the alert's quality,
    /// and it is what the feedback loop counts when it suggests a threshold
    /// change. Appended LAST — the wire form is a string, but stored rows are
    /// read back by name and the order has always been append-only.
    FalsePositive,
}

impl MessageStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::Archived => "archived",
            Self::FalsePositive => "false_positive",
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
        Self::from_string(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid status: {}", s)))
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Active => "Active",
            Self::Acknowledged => "Acknowledged",
            Self::Resolved => "Resolved",
            Self::Archived => "Archived",
            Self::FalsePositive => "False Positive",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" | "活跃" => Some(Self::Active),
            "acknowledged" | "已确认" => Some(Self::Acknowledged),
            "resolved" | "已解决" => Some(Self::Resolved),
            "archived" | "已归档" => Some(Self::Archived),
            "false_positive" | "误报" => Some(Self::FalsePositive),
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
    #[serde(
        serialize_with = "MessageSeverity::serialize",
        deserialize_with = "MessageSeverity::deserialize"
    )]
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
    #[serde(
        serialize_with = "MessageStatus::serialize",
        deserialize_with = "MessageStatus::deserialize"
    )]
    pub status: MessageStatus,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Explicit channel routing. `None` (the default) keeps broadcast
    /// semantics: every channel whose filter accepts the message gets it.
    /// `Some(names)` narrows delivery to exactly those channels (still
    /// respecting enabled state and per-channel filters) — the primitive
    /// per-agent notification routing builds on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_channels: Option<Vec<String>>,
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
            target_channels: None,
            tags: Vec::new(),
        }
    }

    /// Create an alert message.
    pub fn alert(
        severity: MessageSeverity,
        title: String,
        message: String,
        source: String,
    ) -> Self {
        Self::new("alert", severity, title, message, source)
    }

    /// Create a system message.
    pub fn system(title: String, message: String) -> Self {
        Self::new(
            "system",
            MessageSeverity::Info,
            title,
            message,
            "system".to_string(),
        )
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
    pub fn rule(
        severity: MessageSeverity,
        title: String,
        message: String,
        rule_id: String,
    ) -> Self {
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

    /// Acknowledge the message.
    pub fn acknowledge(&mut self) {
        self.status = MessageStatus::Acknowledged;
    }

    /// Resolve the message.
    pub fn resolve(&mut self) {
        self.status = MessageStatus::Resolved;
    }

    /// Mark this alert as a false positive.
    ///
    /// The verdict the feedback loop counts: 002 §3.4 turns "this should not
    /// have fired" into a correction sample, and enough of them into a
    /// suggested threshold change.
    pub fn mark_false_positive(&mut self) {
        self.status = MessageStatus::FalsePositive;
    }

    /// Archive the message.
    pub fn archive(&mut self) {
        self.status = MessageStatus::Archived;
    }

    /// Check if the message is active.
    pub fn is_active(&self) -> bool {
        self.status == MessageStatus::Active
    }
}

#[cfg(test)]
mod tests {
    /// M2-5 / 002 §3.4: the alert-level feedback loop needs a verdict that
    /// means "this should not have fired". It is its own status rather than a
    /// flag, because it replaces the alert's lifecycle state the way
    /// acknowledge/resolve do — and the web UI has been carrying a
    /// `false_positive` label for it since before the status existed.
    #[test]
    fn a_false_positive_is_its_own_status() {
        let mut message = Message::new(
            "alert",
            MessageSeverity::Warning,
            "Cold room".to_string(),
            "out of range".to_string(),
            "rule_engine".to_string(),
        );
        assert_eq!(message.status, MessageStatus::Active);

        message.mark_false_positive();
        assert_eq!(message.status, MessageStatus::FalsePositive);

        // The wire name is what every other consumer keys on: the i18n label,
        // the frontend status map, and the stored rows.
        assert_eq!(MessageStatus::FalsePositive.as_str(), "false_positive");
        assert_eq!(
            MessageStatus::from_string("false_positive"),
            Some(MessageStatus::FalsePositive),
            "a stored verdict must read back"
        );
    }

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
        let mut msg = Message::system("Test".to_string(), "Test message".to_string());

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
        assert_eq!(
            MessageSeverity::from_string("info"),
            Some(MessageSeverity::Info)
        );
        assert_eq!(
            MessageSeverity::from_string("warning"),
            Some(MessageSeverity::Warning)
        );
        assert_eq!(
            MessageSeverity::from_string("critical"),
            Some(MessageSeverity::Critical)
        );
        assert_eq!(
            MessageSeverity::from_string("emergency"),
            Some(MessageSeverity::Emergency)
        );
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
    fn test_message_type_from_string() {
        assert_eq!(
            MessageType::from_string("notification"),
            Some(MessageType::Notification)
        );
        assert_eq!(MessageType::from_string("invalid"), None);
    }

    #[test]
    fn test_message_type_as_str() {
        assert_eq!(MessageType::Notification.as_str(), "notification");
    }

    #[test]
    fn test_message_type_serialization() {
        let mt = MessageType::Notification;
        let json = serde_json::to_string(&mt).unwrap();
        assert_eq!(json, "\"notification\"");

        let parsed: MessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, MessageType::Notification);
    }

    #[test]
    fn test_message_id_default() {
        let id = MessageId::default();
        assert_eq!(id.0.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn test_message_id_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id = MessageId::from_string(uuid_str).unwrap();
        assert_eq!(id.to_string(), uuid_str);
    }

    #[test]
    fn test_message_id_from_string_invalid() {
        let result = MessageId::from_string("invalid-uuid");
        assert!(result.is_err());
    }

    #[test]
    fn test_message_id_display() {
        let id = MessageId::new();
        let display_str = format!("{}", id);
        assert_eq!(display_str, id.to_string());
    }

    #[test]
    fn test_message_status_from_string() {
        assert_eq!(
            MessageStatus::from_string("active"),
            Some(MessageStatus::Active)
        );
        assert_eq!(
            MessageStatus::from_string("acknowledged"),
            Some(MessageStatus::Acknowledged)
        );
        assert_eq!(
            MessageStatus::from_string("resolved"),
            Some(MessageStatus::Resolved)
        );
        assert_eq!(
            MessageStatus::from_string("archived"),
            Some(MessageStatus::Archived)
        );
        assert_eq!(MessageStatus::from_string("invalid"), None);
    }

    #[test]
    fn test_message_status_as_str() {
        assert_eq!(MessageStatus::Active.as_str(), "active");
        assert_eq!(MessageStatus::Acknowledged.as_str(), "acknowledged");
        assert_eq!(MessageStatus::Resolved.as_str(), "resolved");
        assert_eq!(MessageStatus::Archived.as_str(), "archived");
    }

    #[test]
    fn test_message_status_display() {
        assert_eq!(format!("{}", MessageStatus::Active), "Active");
        assert_eq!(format!("{}", MessageStatus::Acknowledged), "Acknowledged");
        assert_eq!(format!("{}", MessageStatus::Resolved), "Resolved");
        assert_eq!(format!("{}", MessageStatus::Archived), "Archived");
    }

    #[test]
    fn test_message_with_tags() {
        let msg = Message::alert(
            MessageSeverity::Warning,
            "Test".to_string(),
            "Test message".to_string(),
            "test_source".to_string(),
        )
        .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);

        assert_eq!(msg.tags.len(), 2);
        assert!(msg.tags.contains(&"tag1".to_string()));
    }

    #[test]
    fn test_message_with_metadata() {
        let metadata = serde_json::json!({
            "temperature": 85.5,
            "unit": "celsius"
        });

        let msg = Message::alert(
            MessageSeverity::Critical,
            "High Temp".to_string(),
            "Test".to_string(),
            "sensor1".to_string(),
        )
        .with_metadata(metadata.clone());

        assert_eq!(msg.metadata, Some(metadata));
    }

    #[test]
    fn test_message_with_status() {
        let msg = Message::system("Test".to_string(), "Test".to_string())
            .with_status(MessageStatus::Resolved);

        assert_eq!(msg.status, MessageStatus::Resolved);
    }

    #[test]
    fn test_message_archive() {
        let mut msg = Message::system("Test".to_string(), "Test".to_string());
        assert!(msg.is_active());

        msg.archive();
        assert_eq!(msg.status, MessageStatus::Archived);
        assert!(!msg.is_active());
    }

    #[test]
    fn test_message_severity_display() {
        assert_eq!(format!("{}", MessageSeverity::Info), "Info");
        assert_eq!(format!("{}", MessageSeverity::Warning), "Warning");
        assert_eq!(format!("{}", MessageSeverity::Critical), "Critical");
        assert_eq!(format!("{}", MessageSeverity::Emergency), "Emergency");
    }

    #[test]
    fn test_message_severity_level() {
        assert_eq!(MessageSeverity::Info.level(), 0);
        assert_eq!(MessageSeverity::Warning.level(), 1);
        assert_eq!(MessageSeverity::Critical.level(), 2);
        assert_eq!(MessageSeverity::Emergency.level(), 3);
    }

    #[test]
    fn test_message_type_display() {
        assert_eq!(format!("{}", MessageType::Notification), "notification");
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::alert(
            MessageSeverity::Critical,
            "High Temp".to_string(),
            "Temperature alert".to_string(),
            "sensor1".to_string(),
        );

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("High Temp"));
        assert!(json.contains("sensor1"));
        assert!(json.contains("critical"));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, msg.title);
        assert_eq!(parsed.severity, msg.severity);
    }

    #[test]
    fn test_message_system_with_severity() {
        let msg = Message::system_with_severity(
            MessageSeverity::Critical,
            "System Critical".to_string(),
            "Critical system event".to_string(),
        );

        assert_eq!(msg.category, "system");
        assert_eq!(msg.severity, MessageSeverity::Critical);
    }

    #[test]
    fn test_message_business() {
        let msg = Message::business(
            "Order Placed".to_string(),
            "New order #12345".to_string(),
            "shop_system".to_string(),
        );

        assert_eq!(msg.category, "business");
        assert_eq!(msg.source, "shop_system");
    }

    #[test]
    fn test_message_chaining_builders() {
        let metadata = serde_json::json!({"key": "value"});
        let tags = vec!["tag1".to_string(), "tag2".to_string()];

        let msg = Message::alert(
            MessageSeverity::Warning,
            "Test".to_string(),
            "Test message".to_string(),
            "source1".to_string(),
        )
        .with_status(MessageStatus::Acknowledged)
        .with_metadata(metadata.clone())
        .with_tags(tags);

        assert_eq!(msg.status, MessageStatus::Acknowledged);
        assert_eq!(msg.metadata, Some(metadata));
        assert_eq!(msg.tags.len(), 2);
    }
}
