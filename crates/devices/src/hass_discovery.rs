//! Home Assistant MQTT Discovery protocol support.
//!
//! This module implements the HASS MQTT Discovery protocol to automatically
//! discover and integrate HASS-ecosystem devices (Tasmota, Shelly, ESPHome, etc.)
//! without requiring Home Assistant software.
//!
//! ## Discovery Protocol
//!
//! HASS devices publish their configuration to MQTT topics:
//! - `homeassistant/<component>/<object_id>/config`
//!
//! ## Example
//!
//! ```json
//! // Topic: homeassistant/sensor/temperature/config
//! {
//!   "name": "Temperature",
//!   "device": {
//!     "identifiers": ["tasmota_4234"],
//!     "name": "Living Room Sensor",
//!     "manufacturer": "Tasmota"
//!   },
//!   "state_topic": "tele/sensor/SENSOR",
//!   "unit_of_measurement": "°C",
//!   "value_template": "{{ value_json.TEMP }}"
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during HASS discovery.
#[derive(Debug, Error)]
pub enum HassDiscoveryError {
    #[error("Invalid discovery message: {0}")]
    InvalidMessage(String),

    #[error("Unsupported component: {0}")]
    UnsupportedComponent(String),

    #[error("Mapping error: {0}")]
    MappingError(String),

    #[error("MQTT error: {0}")]
    MqttError(String),
}

/// Result type for HASS discovery operations.
pub type HassDiscoveryResult<T> = Result<T, HassDiscoveryError>;

fn default_component_type() -> String {
    "sensor".to_string()
}

/// HASS MQTT Discovery configuration message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassDiscoveryConfig {
    /// Component type (sensor, switch, light, etc.)
    /// Note: In HASS discovery, this is derived from the topic, not the payload
    #[serde(default = "default_component_type")]
    #[serde(rename = "component")]
    pub component_type: String,

    /// Unique identifier for this entity
    pub object_id: Option<String>,

    /// Friendly name
    pub name: Option<String>,

    /// Device information
    pub device: Option<HassDeviceInfo>,

    /// MQTT topic for state updates
    pub state_topic: Option<String>,

    /// MQTT topic for commands
    pub command_topic: Option<String>,

    /// Payload for "on" state
    pub payload_on: Option<String>,

    /// Payload for "off" state
    pub payload_off: Option<String>,

    /// Unit of measurement
    #[serde(rename = "unit_of_measurement")]
    pub unit: Option<String>,

    /// Device class
    #[serde(rename = "device_class")]
    pub device_class: Option<String>,

    /// Value template (Jinja2)
    #[serde(rename = "value_template")]
    pub value_template: Option<String>,

    /// JSON attributes path
    #[serde(rename = "json_attributes_topic")]
    pub json_attributes_topic: Option<String>,

    /// JSON attributes template
    #[serde(rename = "json_attributes_template")]
    pub json_attributes_template: Option<String>,

    /// Availability topic
    #[serde(rename = "availability_topic")]
    pub availability_topic: Option<String>,

    /// Payload for available state
    #[serde(rename = "payload_available")]
    pub payload_available: Option<String>,

    /// Payload for not available state
    #[serde(rename = "payload_not_available")]
    pub payload_not_available: Option<String>,

    /// Unique ID
    #[serde(rename = "unique_id")]
    pub unique_id: Option<String>,

    /// Schema for advanced config
    pub schema: Option<String>,

    /// All other fields
    #[serde(flatten)]
    pub extra: HashMap<String, JsonValue>,
}

/// Device information from HASS discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassDeviceInfo {
    /// Device identifiers (array of strings)
    pub identifiers: Vec<String>,

    /// Device name
    pub name: Option<String>,

    /// Device model
    pub model: Option<String>,

    /// Device manufacturer
    pub manufacturer: Option<String>,

    /// SW version
    #[serde(rename = "sw_version")]
    pub sw_version: Option<String>,

    /// Connection info
    pub connections: Option<Vec<Vec<String>>>,

    /// Configuration URL
    #[serde(rename = "configuration_url")]
    pub configuration_url: Option<String>,
}

/// HASS discovery message with topic context.
#[derive(Debug, Clone)]
pub struct HassDiscoveryMessage {
    /// MQTT topic (e.g., "homeassistant/sensor/temperature/config")
    pub topic: String,

    /// Parsed topic components
    pub topic_parts: HassTopicParts,

    /// Configuration payload
    pub config: HassDiscoveryConfig,
}

/// Parsed HASS MQTT topic components.
#[derive(Debug, Clone)]
pub struct HassTopicParts {
    /// Full topic prefix (usually "homeassistant")
    pub prefix: String,

    /// Component type (sensor, switch, light, etc.)
    pub component: String,

    /// Object ID (unique identifier)
    pub object_id: String,

    /// Suffix (usually "config")
    pub suffix: String,
}

impl HassTopicParts {
    /// Parse a HASS discovery topic.
    /// Supports:
    /// - Standard format: homeassistant/<component>/<object_id>/config (4 parts)
    /// - Extended format: homeassistant/<component>/<device_id>/<entity_id>/config (5 parts)
    pub fn parse(topic: &str) -> Option<Self> {
        let parts: Vec<&str> = topic.split('/').collect();

        if parts.len() < 4 {
            return None;
        }

        let prefix = parts[0];
        if prefix != "homeassistant" {
            return None;
        }

        let component = parts[1];
        let (object_id, suffix) = match parts.len() {
            // Standard format: homeassistant/<component>/<object_id>/config
            4 if parts[3] == "config" => {
                (parts[2].to_string(), parts[3].to_string())
            }
            // Extended format: homeassistant/<component>/<device_id>/<entity_id>/config
            5 if parts[4] == "config" => {
                // Combine device_id and entity_id for unique object_id
                (format!("{}_{}", parts[2], parts[3]), parts[4].to_string())
            }
            _ => return None,
        };

        Some(Self {
            prefix: prefix.to_string(),
            component: component.to_string(),
            object_id,
            suffix,
        })
    }

    /// Get the entity ID in HASS format.
    pub fn entity_id(&self) -> String {
        format!("{}.{}", self.component, self.object_id)
    }
}

/// Parse a HASS discovery message from topic and payload.
pub fn parse_discovery_message(topic: &str, payload: &[u8]) -> HassDiscoveryResult<HassDiscoveryMessage> {
    // Parse topic
    let topic_parts = HassTopicParts::parse(topic)
        .ok_or_else(|| HassDiscoveryError::InvalidMessage(format!("Invalid topic: {}", topic)))?;

    // Parse payload as JSON
    let mut config: HassDiscoveryConfig = serde_json::from_slice(payload)
        .map_err(|e| HassDiscoveryError::InvalidMessage(format!("Invalid JSON: {}", e)))?;

    // Override component_type from topic (HASS discovery derives component from topic)
    config.component_type = topic_parts.component.clone();

    Ok(HassDiscoveryMessage {
        topic: topic.to_string(),
        topic_parts,
        config,
    })
}

/// Check if a topic is a HASS discovery topic.
pub fn is_discovery_topic(topic: &str) -> bool {
    topic.starts_with("homeassistant/") && topic.ends_with("/config")
}

/// Get the subscription topic patterns for HASS discovery.
///
/// Returns multiple patterns because HASS discovery supports two topic formats:
/// - `homeassistant/<component>/<object_id>/config` (without node_id, 4 parts)
/// - `homeassistant/<component>/<node_id>/<object_id>/config` (with node_id, 5 parts)
///
/// The MQTT `+` wildcard matches exactly ONE level, so we need separate patterns:
/// - 4-part format: `homeassistant/+/+/config` (2 wildcards)
/// - 5-part format: `homeassistant/+/+/+/config` (3 wildcards)
///
/// Returns a Vec of patterns that should all be subscribed to.
pub fn discovery_subscription_patterns(component: Option<&str>) -> Vec<String> {
    match component {
        Some(comp) => vec![
            format!("homeassistant/{}/+/+/config", comp),  // 4-part format
            format!("homeassistant/{}/+/+/+/config", comp), // 5-part format
        ],
        None => vec![
            "homeassistant/+/+/config".to_string(),      // 4-part format
            "homeassistant/+/+/+/config".to_string(),    // 5-part format
        ],
    }
}

/// Get a single subscription topic pattern for HASS discovery (legacy).
///
/// This returns the 5-part format pattern for maximum compatibility.
/// Prefer using `discovery_subscription_patterns()` which returns both patterns.
pub fn discovery_subscription_pattern(component: Option<&str>) -> String {
    match component {
        Some(comp) => format!("homeassistant/{}/+/+/+/config", comp),
        None => "homeassistant/+/+/+/config".to_string(),
    }
}

/// Supported HASS components that we can map to MDL.
pub fn is_supported_component(component: &str) -> bool {
    matches!(component,
        "sensor" | "binary_sensor" | "switch" | "light" | "cover" | "climate" |
        "fan" | "lock" | "camera" | "vacuum" | "media_player"
    )
}

/// Get MDL device type for a HASS component.
pub fn component_to_device_type(component: &str) -> Option<&'static str> {
    match component {
        "sensor" | "binary_sensor" => Some("sensor"),
        "switch" => Some("switch"),
        "light" => Some("light"),
        "cover" => Some("cover"),
        "climate" => Some("thermostat"),
        "fan" => Some("fan"),
        "lock" => Some("lock"),
        "camera" => Some("camera"),
        "vacuum" => Some("vacuum"),
        "media_player" => Some("media_player"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_parsing() {
        // Standard format: homeassistant/<component>/<object_id>/config
        let topic = "homeassistant/sensor/temperature/config";
        let parts = HassTopicParts::parse(topic).unwrap();

        assert_eq!(parts.prefix, "homeassistant");
        assert_eq!(parts.component, "sensor");
        assert_eq!(parts.object_id, "temperature");
        assert_eq!(parts.entity_id(), "sensor.temperature");

        // Extended format: homeassistant/<component>/<device_id>/<entity_id>/config
        let topic_ext = "homeassistant/sensor/hass-simulator-001/temperature/config";
        let parts_ext = HassTopicParts::parse(topic_ext).unwrap();

        assert_eq!(parts_ext.prefix, "homeassistant");
        assert_eq!(parts_ext.component, "sensor");
        assert_eq!(parts_ext.object_id, "hass-simulator-001_temperature");
        assert_eq!(parts_ext.entity_id(), "sensor.hass-simulator-001_temperature");
    }

    #[test]
    fn test_discovery_topic_detection() {
        // Standard format
        assert!(is_discovery_topic("homeassistant/sensor/temp/config"));
        assert!(is_discovery_topic("homeassistant/switch/light1/config"));

        // Extended format (used by some simulators)
        assert!(is_discovery_topic("homeassistant/sensor/hass-simulator-001/temperature/config"));
        assert!(is_discovery_topic("homeassistant/switch/hass-simulator-001/switch/config"));

        // Not discovery topics
        assert!(!is_discovery_topic("homeassistant/sensor/temp/state"));
        assert!(!is_discovery_topic("tele/sensor/SENSOR"));
    }

    #[test]
    fn test_subscription_patterns() {
        // Test the legacy pattern function (returns 5-part format for compatibility)
        assert_eq!(discovery_subscription_pattern(None), "homeassistant/+/+/+/config");
        assert_eq!(discovery_subscription_pattern(Some("sensor")), "homeassistant/sensor/+/+/+/config");
        assert_eq!(discovery_subscription_pattern(Some("switch")), "homeassistant/switch/+/+/+/config");

        // Test the new patterns function (returns both formats)
        let patterns = discovery_subscription_patterns(None);
        assert_eq!(patterns.len(), 2);
        assert!(patterns.contains(&"homeassistant/+/+/config".to_string()));
        assert!(patterns.contains(&"homeassistant/+/+/+/config".to_string()));
    }

    #[test]
    fn test_component_support() {
        assert!(is_supported_component("sensor"));
        assert!(is_supported_component("switch"));
        assert!(is_supported_component("light"));
        assert!(!is_supported_component("automation"));
        assert!(!is_supported_component("script"));
    }

    #[test]
    fn test_component_to_device_type() {
        assert_eq!(component_to_device_type("sensor"), Some("sensor"));
        assert_eq!(component_to_device_type("switch"), Some("switch"));
        assert_eq!(component_to_device_type("light"), Some("light"));
        assert_eq!(component_to_device_type("cover"), Some("cover"));
        assert_eq!(component_to_device_type("unknown"), None);
    }

    #[test]
    fn test_parse_discovery_message() {
        let topic = "homeassistant/sensor/temperature/config";
        let payload = r#"{
            "name": "Living Room Temperature",
            "device": {
                "identifiers": ["tasmota_4234"],
                "name": "Living Room Sensor",
                "manufacturer": "Tasmota"
            },
            "state_topic": "tele/sensor/SENSOR",
            "unit_of_measurement": "°C",
            "value_template": "{{ value_json.TEMP }}"
        }"#.as_bytes();

        let msg = parse_discovery_message(topic, payload).unwrap();

        assert_eq!(msg.topic_parts.component, "sensor");
        assert_eq!(msg.topic_parts.object_id, "temperature");
        assert_eq!(msg.config.name, Some("Living Room Temperature".to_string()));
        assert_eq!(msg.config.state_topic, Some("tele/sensor/SENSOR".to_string()));
        assert_eq!(msg.config.unit, Some("°C".to_string()));
    }
}
