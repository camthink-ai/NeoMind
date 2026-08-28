//! Channel filter for message routing.

use crate::{Message, MessageSeverity};
use serde::{Deserialize, Serialize};

/// Filter configuration for a channel to select which messages to receive.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelFilter {
    /// Source types to receive (empty = all)
    #[serde(default)]
    pub source_types: Vec<String>,

    /// Categories to receive (empty = all)
    #[serde(default)]
    pub categories: Vec<String>,

    /// Minimum severity level (None = all)
    #[serde(
        default,
        serialize_with = "serialize_option_severity",
        deserialize_with = "deserialize_option_severity"
    )]
    pub min_severity: Option<MessageSeverity>,
}

/// Serialize Option<MessageSeverity>
fn serialize_option_severity<S>(
    value: &Option<MessageSeverity>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(sev) => serializer.serialize_some(&sev.as_str()),
        None => serializer.serialize_none(),
    }
}

/// Deserialize Option<MessageSeverity>
fn deserialize_option_severity<'de, D>(deserializer: D) -> Result<Option<MessageSeverity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => MessageSeverity::from_string(&s)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid severity: {}", s))),
        None => Ok(None),
    }
}

impl ChannelFilter {
    /// Check if a message matches this filter.
    pub fn matches(&self, message: &Message) -> bool {
        // Filter by source_types
        if !self.source_types.is_empty() && !self.source_types.contains(&message.source_type) {
            return false;
        }

        // Filter by categories
        if !self.categories.is_empty() && !self.categories.contains(&message.category) {
            return false;
        }

        // Filter by min_severity
        if let Some(min_sev) = self.min_severity {
            if message.severity < min_sev {
                return false;
            }
        }

        true
    }

    /// Create a filter that accepts all messages.
    pub fn accept_all() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(source_type: &str, severity: MessageSeverity) -> Message {
        let mut msg = Message::system("Test".to_string(), "Test".to_string());
        msg.source_type = source_type.to_string();
        msg.severity = severity;
        msg
    }

    #[test]
    fn test_default_filter_matches_all() {
        let filter = ChannelFilter::default();
        let msg = make_test_message("device", MessageSeverity::Critical);
        assert!(filter.matches(&msg));
    }

    #[test]
    fn test_filter_by_source_type() {
        let filter = ChannelFilter {
            source_types: vec!["device".to_string(), "rule".to_string()],
            ..Default::default()
        };

        let device_msg = make_test_message("device", MessageSeverity::Info);
        let rule_msg = make_test_message("rule", MessageSeverity::Info);
        let system_msg = make_test_message("system", MessageSeverity::Info);

        assert!(filter.matches(&device_msg));
        assert!(filter.matches(&rule_msg));
        assert!(!filter.matches(&system_msg));
    }

    #[test]
    fn test_filter_by_min_severity() {
        let filter = ChannelFilter {
            min_severity: Some(MessageSeverity::Warning),
            ..Default::default()
        };

        let info = make_test_message("system", MessageSeverity::Info);
        let warning = make_test_message("system", MessageSeverity::Warning);
        let critical = make_test_message("system", MessageSeverity::Critical);

        assert!(!filter.matches(&info));
        assert!(filter.matches(&warning));
        assert!(filter.matches(&critical));
    }

    #[test]
    fn test_filter_combined() {
        let filter = ChannelFilter {
            source_types: vec!["device".to_string()],
            min_severity: Some(MessageSeverity::Warning),
            ..Default::default()
        };

        let matching = make_test_message("device", MessageSeverity::Critical);
        assert!(filter.matches(&matching));

        let wrong_source = make_test_message("rule", MessageSeverity::Critical);
        assert!(!filter.matches(&wrong_source));

        let wrong_severity = make_test_message("device", MessageSeverity::Info);
        assert!(!filter.matches(&wrong_severity));
    }
}
