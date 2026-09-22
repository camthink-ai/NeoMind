#![allow(clippy::needless_return)]

//! Unified Data Source System
//!
//! This module provides a unified way to identify and query data from:
//! - Devices (telemetry/metrics)
//! - Extensions (command outputs)
//! - Transforms (processed data)
//!
//! All data sources use the same `DataSourceId` format.

use crate::event::MetricValue;
use serde::{Deserialize, Serialize};

// ============================================================================
// Unified Data Source ID
// ============================================================================

/// Unified data source identifier
///
/// Format: `type:id:field` (3 parts, unified for all sources)
/// - Device: `device:sensor1:temperature`
/// - Extension: `extension:weather:temperature` (same format as device)
/// - Transform: `transform:my_processor:output`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataSourceId {
    pub source_type: DataSourceType,
    pub source_id: String,
    pub field_path: String,
}

/// Data source type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSourceType {
    #[serde(rename = "device")]
    Device,
    #[serde(rename = "extension")]
    Extension,
    #[serde(rename = "transform")]
    Transform,
    /// An AI agent's published output field. First-class like the rest
    /// (M1/M2-2): dashboards, rules and data-push bind to it exactly as they
    /// do to a device metric. Namespace `ai:{agent_id}`, field = metric.
    #[serde(rename = "ai")]
    Ai,
}

impl DataSourceId {
    /// Create an extension data source ID (V2 - unified with device format)
    ///
    /// # Arguments
    /// * `extension_id` - Extension identifier (e.g., "weather")
    /// * `metric` - Metric name (e.g., "temperature")
    pub fn extension(extension_id: &str, metric: &str) -> Self {
        Self {
            source_type: DataSourceType::Extension,
            source_id: extension_id.to_string(),
            field_path: metric.to_string(),
        }
    }

    /// Create a device data source ID
    ///
    /// # Arguments
    /// * `device_id` - Device identifier
    /// * `metric` - Metric name
    pub fn device(device_id: &str, metric: &str) -> Self {
        Self {
            source_type: DataSourceType::Device,
            source_id: device_id.to_string(),
            field_path: metric.to_string(),
        }
    }

    /// Create a transform data source ID
    pub fn transform(transform_id: &str, field: &str) -> Self {
        Self {
            source_type: DataSourceType::Transform,
            source_id: transform_id.to_string(),
            field_path: field.to_string(),
        }
    }

    /// Create an AI-output data source ID (`ai:{agent_id}:{field}`).
    pub fn ai(agent_id: &str, field: &str) -> Self {
        Self {
            source_type: DataSourceType::Ai,
            source_id: agent_id.to_string(),
            field_path: field.to_string(),
        }
    }

    /// Parse from string representation
    ///
    /// Expected format: "type:id:field" (3 parts, unified)
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let source_type = match parts[0] {
            "device" => DataSourceType::Device,
            "extension" => DataSourceType::Extension,
            "transform" => DataSourceType::Transform,
            "ai" => DataSourceType::Ai,
            _ => return None,
        };

        Some(DataSourceId {
            source_type,
            source_id: parts[1].to_string(),
            field_path: parts[2].to_string(),
        })
    }

    /// Convert to storage key
    pub fn storage_key(&self) -> String {
        match &self.source_type {
            DataSourceType::Device => {
                format!("device:{}:{}", self.source_id, self.field_path)
            }
            DataSourceType::Extension => {
                format!("extension:{}:{}", self.source_id, self.field_path)
            }
            DataSourceType::Transform => {
                format!("transform:{}:{}", self.source_id, self.field_path)
            }
            DataSourceType::Ai => format!("ai:{}:{}", self.source_id, self.field_path),
        }
    }

    /// Get display name
    pub fn display_name(&self) -> String {
        match &self.source_type {
            DataSourceType::Device => format!("Device {} / {}", self.source_id, self.field_path),
            DataSourceType::Extension => {
                format!("Extension {} / {}", self.source_id, self.field_path)
            }
            DataSourceType::Transform => {
                format!("Transform {} / {}", self.source_id, self.field_path)
            }
            DataSourceType::Ai => format!("AI {} / {}", self.source_id, self.field_path),
        }
    }

    /// Get the source_id part for TimeSeriesStorage API
    ///
    /// All types now return a prefixed format for consistency:
    /// - Devices: "device:{device_id}"
    /// - Extensions: "extension:{extension_id}"
    /// - Transforms: "transform:{transform_id}"
    pub fn source_part(&self) -> String {
        match &self.source_type {
            DataSourceType::Device => format!("device:{}", self.source_id),
            DataSourceType::Extension => format!("extension:{}", self.source_id),
            DataSourceType::Transform => format!("transform:{}", self.source_id),
            DataSourceType::Ai => format!("ai:{}", self.source_id),
        }
    }

    /// Get the metric name part for TimeSeriesStorage API
    ///
    /// Returns the field_path (metric name)
    pub fn metric_part(&self) -> &str {
        &self.field_path
    }

    // ========================================================================
    // Extension Command Format Support
    // ========================================================================
    // Extension commands produce output fields that are stored with a
    // four-part identifier: "extension:extension_id:command:field"
    // This is mapped to DataSourceId as:
    // - source_type: Extension
    // - source_id: extension_id
    // - field_path: "command.field" (nested path)

    /// Create an extension command data source ID
    ///
    /// Extension commands produce output with format: "extension:id:command:field"
    /// This is stored as a nested field path: "command.field"
    ///
    /// # Arguments
    /// * `extension_id` - Extension identifier (e.g., "weather")
    /// * `command` - Command name (e.g., "get_current_weather")
    /// * `field` - Output field name (e.g., "temperature_c")
    ///
    /// # Examples
    /// ```
    /// use neomind_core::datasource::{DataSourceId, DataSourceType};
    /// let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
    /// assert_eq!(id.source_type, DataSourceType::Extension);
    /// assert_eq!(id.source_id, "weather");
    /// assert_eq!(id.field_path, "get_current_weather.temperature_c");
    /// ```
    pub fn extension_command(extension_id: &str, command: &str, field: &str) -> Self {
        Self {
            source_type: DataSourceType::Extension,
            source_id: extension_id.to_string(),
            field_path: format!("{}.{}", command, field),
        }
    }

    /// Parse extension command format from string
    ///
    /// Handles the legacy four-part format: "extension:id:command:field"
    /// Converts it to the DataSourceId with nested field path.
    ///
    /// # Arguments
    /// * `s` - String in format "extension:id:command:field"
    ///
    /// # Returns
    /// * `Some(DataSourceId)` if format is valid
    /// * `None` if format is invalid
    ///
    /// # Examples
    /// ```
    /// use neomind_core::datasource::DataSourceId;
    /// let id = DataSourceId::parse_extension_command("extension:weather:get_current_weather:temperature_c");
    /// assert!(id.is_some());
    /// let id = id.unwrap();
    /// assert_eq!(id.source_id, "weather");
    /// assert_eq!(id.field_path, "get_current_weather.temperature_c");
    /// ```
    pub fn parse_extension_command(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 4 || parts[0] != "extension" {
            return None;
        }

        Some(Self {
            source_type: DataSourceType::Extension,
            source_id: parts[1].to_string(),
            field_path: format!("{}.{}", parts[2], parts[3]),
        })
    }

    /// Parse as extension command, returning (extension_id, command, field)
    ///
    /// # Returns
    /// * `Some((extension_id, command, field))` if this is an extension command
    /// * `None` if not an extension command or field_path is not in "command.field" format
    pub fn as_extension_command_parts(&self) -> Option<(&str, &str, &str)> {
        if self.source_type != DataSourceType::Extension {
            return None;
        }

        let parts: Vec<&str> = self.field_path.splitn(2, '.').collect();
        if parts.len() == 2 {
            Some((&self.source_id, parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl std::fmt::Display for DataSourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}:{}:{}",
            self.source_type, self.source_id, self.field_path
        )
    }
}

// ============================================================================
// Data Point and Query Types
// ============================================================================

/// A single data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: MetricValue,
    pub quality: Option<f32>,
}

impl DataPoint {
    pub fn new(timestamp: i64, value: MetricValue) -> Self {
        Self {
            timestamp,
            value,
            quality: None,
        }
    }

    pub fn with_quality(mut self, quality: f32) -> Self {
        self.quality = Some(quality);
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_source_id_extension() {
        let id = DataSourceId::extension("weather", "temperature"); // V2 format
        assert_eq!(id.source_type, DataSourceType::Extension);
        assert_eq!(id.source_id, "weather");
        assert_eq!(id.field_path, "temperature");
        assert_eq!(id.storage_key(), "extension:weather:temperature");
    }

    #[test]
    fn test_data_source_id_device() {
        let id = DataSourceId::device("sensor1", "temperature");
        assert_eq!(id.source_type, DataSourceType::Device);
        assert_eq!(id.source_id, "sensor1");
        assert_eq!(id.field_path, "temperature");
        assert_eq!(id.storage_key(), "device:sensor1:temperature");
    }

    #[test]
    fn test_data_source_id_parse() {
        // Format: type:id:field (unified V2)
        let id = DataSourceId::parse("device:sensor1:temp").unwrap();
        assert_eq!(id.source_type, DataSourceType::Device);
        assert_eq!(id.source_id, "sensor1");
        assert_eq!(id.field_path, "temp");

        // Extension uses same format
        let id = DataSourceId::parse("extension:weather:temperature").unwrap();
        assert_eq!(id.source_type, DataSourceType::Extension);
        assert_eq!(id.source_id, "weather");
        assert_eq!(id.field_path, "temperature");
    }

    #[test]
    fn test_data_source_id_display_name() {
        let id = DataSourceId::extension("weather", "temperature");
        assert_eq!(id.display_name(), "Extension weather / temperature");

        let id = DataSourceId::device("sensor1", "temperature");
        assert_eq!(id.display_name(), "Device sensor1 / temperature");
    }

    #[test]
    fn test_source_part() {
        // Device: "device:" prefix (unified)
        let id = DataSourceId::device("sensor1", "temperature");
        assert_eq!(id.source_part(), "device:sensor1");
        assert_eq!(id.metric_part(), "temperature");

        // Extension: "extension:" prefix
        let id = DataSourceId::extension("weather", "temperature");
        assert_eq!(id.source_part(), "extension:weather");
        assert_eq!(id.metric_part(), "temperature");

        // Transform: "transform:" prefix
        let id = DataSourceId::transform("processor", "output");
        assert_eq!(id.source_part(), "transform:processor");
        assert_eq!(id.metric_part(), "output");
    }

    #[test]
    fn test_data_point() {
        let dp = DataPoint::new(123456, MetricValue::Float(23.5));
        assert_eq!(dp.timestamp, 123456);
        assert_eq!(dp.quality, None);

        let dp = dp.with_quality(0.95);
        assert_eq!(dp.quality, Some(0.95));
    }

    #[test]
    fn test_data_source_id_serialization() {
        let id = DataSourceId::extension("weather", "temperature"); // V2 format
        let json = serde_json::to_string(&id).unwrap();
        let parsed: DataSourceId = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.source_type, id.source_type);
        assert_eq!(parsed.source_id, id.source_id);
        assert_eq!(parsed.field_path, id.field_path);
    }

    // ========================================================================
    // Extension Command Format Tests
    // ========================================================================

    #[test]
    fn test_extension_command_creation() {
        let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
        assert_eq!(id.source_type, DataSourceType::Extension);
        assert_eq!(id.source_id, "weather");
        assert_eq!(id.field_path, "get_current_weather.temperature_c");
    }

    #[test]
    fn test_parse_extension_command() {
        // Valid four-part format
        let id = DataSourceId::parse_extension_command(
            "extension:weather:get_current_weather:temperature_c",
        );
        assert!(id.is_some());
        let id = id.unwrap();
        assert_eq!(id.source_type, DataSourceType::Extension);
        assert_eq!(id.source_id, "weather");
        assert_eq!(id.field_path, "get_current_weather.temperature_c");

        // Invalid format - wrong prefix
        assert!(DataSourceId::parse_extension_command(
            "device:weather:get_current_weather:temperature_c"
        )
        .is_none());

        // Invalid format - wrong part count
        assert!(
            DataSourceId::parse_extension_command("extension:weather:get_current_weather")
                .is_none()
        );

        // Invalid format - too many parts
        assert!(DataSourceId::parse_extension_command(
            "extension:weather:get_current_weather:temperature_c:extra"
        )
        .is_none());
    }

    #[test]
    fn test_as_extension_command_parts() {
        let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
        let parts = id.as_extension_command_parts();
        assert!(parts.is_some());
        let (ext_id, cmd, field) = parts.unwrap();
        assert_eq!(ext_id, "weather");
        assert_eq!(cmd, "get_current_weather");
        assert_eq!(field, "temperature_c");

        // Simple extension metric - no command parts
        let id = DataSourceId::extension("weather", "temperature");
        assert!(id.as_extension_command_parts().is_none());

        // Device - no command parts
        let id = DataSourceId::device("sensor1", "temperature");
        assert!(id.as_extension_command_parts().is_none());
    }

    #[test]
    fn test_extension_command_display_name() {
        let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
        assert_eq!(
            id.display_name(),
            "Extension weather / get_current_weather.temperature_c"
        );
    }
}
