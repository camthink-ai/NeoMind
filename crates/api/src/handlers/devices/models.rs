//! Device DTOs and request structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device info for API responses (with all fields needed by frontend)
#[derive(Debug, Serialize)]
pub struct DeviceDto {
    pub id: String,
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub adapter_type: String,
    pub status: String,
    pub last_seen: String,
    pub online: bool,
    /// Plugin ID that manages this device (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Plugin name that manages this device (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Adapter/Plugin ID that manages this device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    /// Metric and command counts (from template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_count: Option<usize>,
    /// Current metric values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_values: Option<HashMap<String, serde_json::Value>>,
    /// Legacy config field for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<HashMap<String, String>>,
}

/// Device type info for API responses.
#[derive(Debug, Serialize)]
pub struct DeviceTypeDto {
    pub device_type: String,
    pub name: String,
    pub description: String,
    pub categories: Vec<String>,
    pub mode: String,
    /// Metrics available for this device type
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<MetricDefinitionDto>,
    /// Commands available for this device type
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandDefinitionDto>,
    /// Metric count (for backward compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_count: Option<usize>,
    /// Command count (for backward compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_count: Option<usize>,
}

/// Metric definition for API responses
#[derive(Debug, Serialize)]
pub struct MetricDefinitionDto {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// Command definition for API responses
#[derive(Debug, Serialize)]
pub struct CommandDefinitionDto {
    pub name: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterDefinitionDto>,
}

/// Parameter definition for API responses
#[derive(Debug, Serialize)]
pub struct ParameterDefinitionDto {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
}

/// Query parameters for time range queries.
#[derive(Debug, Deserialize)]
pub struct TimeRangeQuery {
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub limit: Option<usize>,
}

/// Request to add a new device.
#[derive(Debug, Deserialize)]
pub struct AddDeviceRequest {
    /// Device type (must be registered)
    pub device_type: String,
    /// Optional device ID (auto-generated if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Device name
    pub name: String,
    /// Adapter type (mqtt, http, etc.)
    pub adapter_type: String,
    /// Connection configuration (protocol-specific)
    pub connection_config: serde_json::Value,
}

/// Request to update an existing device.
/// All fields are optional - only provided fields will be updated.
#[derive(Debug, Deserialize)]
pub struct UpdateDeviceRequest {
    /// Device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Adapter type (mqtt, http, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_type: Option<String>,
    /// Connection configuration (protocol-specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_config: Option<serde_json::Value>,
    /// Adapter/Plugin ID that manages this device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

/// Pagination query parameters
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    /// Page number (1-indexed)
    pub page: Option<usize>,
    /// Number of items per page
    pub limit: Option<usize>,
    /// Filter by device type
    pub device_type: Option<String>,
    /// Filter by connection status
    pub status: Option<String>,
}

/// Pagination metadata
#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    /// Current page number
    pub page: usize,
    /// Number of items per page
    pub limit: usize,
    /// Total number of items
    pub total: usize,
    /// Total number of pages
    pub total_pages: usize,
    /// Whether there is a next page
    pub has_next: bool,
    /// Whether there is a previous page
    pub has_prev: bool,
}

impl PaginationMeta {
    /// Create pagination metadata
    pub fn new(page: usize, limit: usize, total: usize) -> Self {
        let total_pages = if total == 0 {
            0
        } else {
            total.div_ceil(limit)
        };
        Self {
            page,
            limit,
            total,
            total_pages,
            has_next: page < total_pages,
            has_prev: page > 1,
        }
    }
}

/// Request to send a command to a device.
#[derive(Debug, Deserialize)]
pub struct SendCommandRequest {
    /// Command parameters
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

/// Discovery request for scanning a host for devices.
#[derive(Debug, Deserialize)]
pub struct DiscoveryRequest {
    /// Host to scan (IP address or hostname)
    pub host: String,
    /// Optional list of ports to scan (default: common ports)
    pub ports: Option<Vec<u16>>,
    /// Timeout per port in milliseconds (default: 500)
    pub timeout_ms: Option<u64>,
}

/// Discovered device info for API responses.
#[derive(Debug, Serialize)]
pub struct DiscoveredDeviceDto {
    pub id: String,
    pub device_type: Option<String>,
    pub host: String,
    pub port: u16,
    pub confidence: f32,
    pub info: HashMap<String, String>,
}

/// Request body for MDL generation from sample data.
#[derive(Debug, Deserialize)]
pub struct GenerateMdlRequest {
    /// Device name (used to generate device_type)
    pub device_name: String,
    /// Optional device description
    #[serde(default)]
    pub description: String,
    /// Sample uplink JSON data
    #[serde(default)]
    pub uplink_example: String,
    /// Optional sample downlink JSON data
    #[serde(default)]
    pub downlink_example: String,
}

/// Request to fetch current values for multiple devices.
#[derive(Debug, Deserialize)]
pub struct BatchCurrentValuesRequest {
    /// List of device IDs to fetch current values for
    pub device_ids: Vec<String>,
}

/// Batch current values response for a single device.
#[derive(Debug, Serialize)]
pub struct DeviceCurrentValues {
    /// Device ID
    pub device_id: String,
    /// Current metric values (flat key-value map)
    pub current_values: HashMap<String, serde_json::Value>,
}

/// Response for batch current values request.
#[derive(Debug, Serialize)]
pub struct BatchCurrentValuesResponse {
    /// Current values for each device (keyed by device_id)
    pub devices: HashMap<String, DeviceCurrentValues>,
    /// Count of devices with data
    pub count: usize,
}
