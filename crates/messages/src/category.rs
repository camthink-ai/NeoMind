//! Message category.

use serde::{Deserialize, Serialize};

/// Message category determines the type and handling of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum MessageCategory {
    /// Alert - requires attention and action
    #[serde(rename = "alert")]
    Alert,
    /// System - informational messages
    #[serde(rename = "system")]
    #[default]
    System,
    /// Business - workflow and business events
    #[serde(rename = "business")]
    Business,
}

impl MessageCategory {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Alert => "alert",
            Self::System => "system",
            Self::Business => "business",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Alert => "告警",
            Self::System => "系统",
            Self::Business => "业务",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s {
            "alert" | "告警" => Some(Self::Alert),
            "system" | "系统" => Some(Self::System),
            "business" | "业务" => Some(Self::Business),
            _ => None,
        }
    }
}

impl std::fmt::Display for MessageCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_from_str() {
        assert_eq!(MessageCategory::from_string("alert"), Some(MessageCategory::Alert));
        assert_eq!(MessageCategory::from_string("system"), Some(MessageCategory::System));
        assert_eq!(MessageCategory::from_string("business"), Some(MessageCategory::Business));
        assert_eq!(MessageCategory::from_string("invalid"), None);
    }

    #[test]
    fn test_category_display() {
        assert_eq!(MessageCategory::Alert.as_str(), "alert");
        assert_eq!(MessageCategory::Alert.display_name(), "告警");
    }
}
