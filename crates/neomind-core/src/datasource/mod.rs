#![allow(clippy::needless_return)]

//! Unified Data Source System
//!
//! This module provides a unified way to identify and query data from:
//! - Devices (telemetry/metrics)
//! - Extensions (command outputs)
//! - Transforms (processed data)
//!
//! All data sources use the same `DataSourceId` format and can be queried
//! through the same `UnifiedQueryService`.

pub mod query;

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
        }
    }

    /// Parse field path into components
    pub fn field_components(&self) -> Vec<&str> {
        self.field_path.split('.').collect()
    }

    /// Get the device_id part for TimeSeriesStorage API
    ///
    /// For extensions, returns "extension:{extension_id}"
    /// For devices, returns "{device_id}"
    /// For transforms, returns "transform:{transform_id}"
    pub fn device_part(&self) -> String {
        match &self.source_type {
            DataSourceType::Device => self.source_id.clone(),
            DataSourceType::Extension => format!("extension:{}", self.source_id),
            DataSourceType::Transform => format!("transform:{}", self.source_id),
        }
    }

    /// Get the metric name part for TimeSeriesStorage API
    ///
    /// Returns the field_path (metric name)
    pub fn metric_part(&self) -> &str {
        &self.field_path
    }

    /// Create DataSourceId from device_id and metric (TimeSeriesStorage format)
    ///
    /// This is the inverse of device_part() and metric_part()
    /// Handles parsing both "device:id" and "extension:id" formats
    pub fn from_storage_parts(device_id: &str, metric: &str) -> Option<Self> {
        if let Some(rest) = device_id.strip_prefix("extension:") {
            // Extension: "extension:weather" + "temperature"
            return Some(Self::extension(rest, metric));
        } else if let Some(rest) = device_id.strip_prefix("transform:") {
            // Transform: "transform:processor" + "output"
            return Some(Self::transform(rest, metric));
        } else {
            // Device: "sensor1" + "temperature"
            return Some(Self::device(device_id, metric));
        }
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

    /// Check if this is an extension command data source (has nested field path)
    ///
    /// Extension command sources have field paths like "command.field"
    /// while simple extension metrics have field paths like "temperature"
    pub fn is_extension_command(&self) -> bool {
        self.source_type == DataSourceType::Extension && self.field_path.contains('.')
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

/// Query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub source_id: DataSourceId,
    pub datapoints: Vec<DataPoint>,
    pub aggregation: Option<AggregatedValue>,
}

/// Aggregated value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedValue {
    pub value: f64,
    pub count: usize,
    pub func: AggFunc,
}

/// Aggregation functions (defined here for query system)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggFunc {
    Avg,
    Sum,
    Min,
    Max,
    Count,
    Last,
}

impl Default for AggFunc {
    fn default() -> Self {
        Self::Last
    }
}

/// Query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParams {
    pub start: i64,
    pub end: i64,
    pub aggregation: Option<AggFunc>,
    pub limit: Option<usize>,
}

impl QueryParams {
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            aggregation: None,
            limit: None,
        }
    }

    pub fn with_aggregation(mut self, agg: AggFunc) -> Self {
        self.aggregation = Some(agg);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn last_minutes(minutes: u64) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        let start = now - (minutes as i64 * 60000);
        Self::new(start, now)
    }
}

// ============================================================================
// Data Source Metadata
// ============================================================================

/// Information about a queryable data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceInfo {
    pub id: DataSourceId,
    pub display_name: String,
    pub description: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub is_primary: bool,
    pub aggregatable: bool,
    pub available: bool,
    pub last_update: Option<i64>,
}

impl DataSourceInfo {
    /// Create from extension metric (V2 - device-standard compatible)
    pub fn from_extension_metric(
        extension_id: &str,
        extension_name: &str,
        metric: &crate::extension::MetricDefinition,
    ) -> Self {
        let id = DataSourceId::extension(extension_id, &metric.name);
        Self {
            display_name: format!("{}: {}", extension_name, metric.display_name),
            description: metric.display_name.clone(),
            data_type: format!("{:?}", metric.data_type),
            unit: if metric.unit.is_empty() {
                None
            } else {
                Some(metric.unit.clone())
            },
            is_primary: metric.required,
            aggregatable: true,
            id,
            available: false,
            last_update: None,
        }
    }

    /// Create from device metric
    pub fn from_device_metric(
        device_id: &str,
        device_name: &str,
        metric: &crate::extension::MetricDefinition,
    ) -> Self {
        let id = DataSourceId::device(device_id, &metric.name);
        Self {
            display_name: format!("{}: {}", device_name, metric.display_name),
            description: metric.display_name.clone(),
            data_type: format!("{:?}", metric.data_type),
            unit: if metric.unit.is_empty() {
                None
            } else {
                Some(metric.unit.clone())
            },
            is_primary: metric.required,
            aggregatable: true,
            id,
            available: false,
            last_update: None,
        }
    }
}

/// Collection of data sources grouped by type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceCatalog {
    pub devices: Vec<DataSourceInfo>,
    pub extensions: Vec<DataSourceInfo>,
    pub transforms: Vec<DataSourceInfo>,
}

impl DataSourceCatalog {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            extensions: Vec::new(),
            transforms: Vec::new(),
        }
    }

    pub fn add_device(&mut self, info: DataSourceInfo) {
        self.devices.push(info);
    }

    pub fn add_extension(&mut self, info: DataSourceInfo) {
        self.extensions.push(info);
    }

    pub fn add_transform(&mut self, info: DataSourceInfo) {
        self.transforms.push(info);
    }

    pub fn all(&self) -> Vec<&DataSourceInfo> {
        let mut all = Vec::new();
        all.extend(self.devices.iter());
        all.extend(self.extensions.iter());
        all.extend(self.transforms.iter());
        all
    }

    pub fn by_type(&self, source_type: &DataSourceType) -> Vec<&DataSourceInfo> {
        match source_type {
            DataSourceType::Device => self.devices.iter().collect(),
            DataSourceType::Extension => self.extensions.iter().collect(),
            DataSourceType::Transform => self.transforms.iter().collect(),
        }
    }
}

impl Default for DataSourceCatalog {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Export query service
// ============================================================================
pub use query::{QueryError, UnifiedQueryService};

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
    fn test_field_components() {
        let id = DataSourceId::extension("weather", "temperature");
        let parts = id.field_components();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], "temperature");
    }

    #[test]
    fn test_device_part() {
        // Device: no prefix
        let id = DataSourceId::device("sensor1", "temperature");
        assert_eq!(id.device_part(), "sensor1");
        assert_eq!(id.metric_part(), "temperature");

        // Extension: "extension:" prefix
        let id = DataSourceId::extension("weather", "temperature");
        assert_eq!(id.device_part(), "extension:weather");
        assert_eq!(id.metric_part(), "temperature");

        // Transform: "transform:" prefix
        let id = DataSourceId::transform("processor", "output");
        assert_eq!(id.device_part(), "transform:processor");
        assert_eq!(id.metric_part(), "output");
    }

    #[test]
    fn test_from_storage_parts() {
        // Extension storage format
        let id = DataSourceId::from_storage_parts("extension:weather", "temperature").unwrap();
        assert_eq!(id.source_type, DataSourceType::Extension);
        assert_eq!(id.source_id, "weather");
        assert_eq!(id.field_path, "temperature");

        // Device storage format
        let id = DataSourceId::from_storage_parts("sensor1", "temperature").unwrap();
        assert_eq!(id.source_type, DataSourceType::Device);
        assert_eq!(id.source_id, "sensor1");
        assert_eq!(id.field_path, "temperature");

        // Transform storage format
        let id = DataSourceId::from_storage_parts("transform:processor", "output").unwrap();
        assert_eq!(id.source_type, DataSourceType::Transform);
        assert_eq!(id.source_id, "processor");
        assert_eq!(id.field_path, "output");
    }

    #[test]
    fn test_round_trip_storage_parts() {
        // Test that device_part() + metric_part() round-trips through from_storage_parts()
        let original = DataSourceId::extension("weather", "temperature");
        let reconstructed =
            DataSourceId::from_storage_parts(&original.device_part(), original.metric_part())
                .unwrap();
        assert_eq!(original.source_type, reconstructed.source_type);
        assert_eq!(original.source_id, reconstructed.source_id);
        assert_eq!(original.field_path, reconstructed.field_path);
    }

    #[test]
    fn test_query_params() {
        let params = QueryParams::last_minutes(5);
        assert!(params.aggregation.is_none());
        assert!(params.limit.is_none());

        let params = params.with_aggregation(AggFunc::Avg);
        assert!(params.aggregation.is_some());

        let params = params.with_limit(100);
        assert_eq!(params.limit, Some(100));
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
    fn test_data_source_catalog() {
        let mut catalog = DataSourceCatalog::new();
        assert_eq!(catalog.all().len(), 0);

        let info = DataSourceInfo {
            id: DataSourceId::device("test", "value"),
            display_name: "Test".to_string(),
            description: "Test source".to_string(),
            data_type: "float".to_string(),
            unit: None,
            is_primary: true,
            aggregatable: true,
            available: true,
            last_update: None,
        };

        catalog.add_device(info);
        assert_eq!(catalog.devices.len(), 1);
        assert_eq!(catalog.all().len(), 1);
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
    fn test_is_extension_command() {
        // Extension command has nested field path
        let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
        assert!(id.is_extension_command());

        // Simple extension metric has no nesting
        let id = DataSourceId::extension("weather", "temperature");
        assert!(!id.is_extension_command());

        // Device is not an extension
        let id = DataSourceId::device("sensor1", "temperature");
        assert!(!id.is_extension_command());
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
    fn test_extension_command_storage_parts() {
        let id = DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");

        // device_part() should include "extension:" prefix
        assert_eq!(id.device_part(), "extension:weather");

        // metric_part() should be the full nested field path
        assert_eq!(id.metric_part(), "get_current_weather.temperature_c");

        // Should round-trip correctly
        let reconstructed =
            DataSourceId::from_storage_parts(&id.device_part(), id.metric_part()).unwrap();
        assert_eq!(reconstructed.source_type, id.source_type);
        assert_eq!(reconstructed.source_id, id.source_id);
        assert_eq!(reconstructed.field_path, id.field_path);
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
