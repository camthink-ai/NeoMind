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
    /// Adapter type (mqtt, modbus, hass, etc.)
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
    /// Adapter type (mqtt, modbus, hass, etc.)
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

/// HASS Discovery configuration.
#[derive(Debug, Deserialize)]
pub struct HassDiscoveryRequest {
    /// MQTT broker address with HASS discovery devices
    pub broker: Option<String>,
    /// Broker port (default: 1883)
    pub port: Option<u16>,
    /// Components to discover (empty = all supported)
    pub components: Option<Vec<String>>,
    /// Auto-register discovered devices
    #[serde(default)]
    pub auto_register: bool,
}

/// HASS discovered device info.
#[derive(Debug, Serialize)]
pub struct HassDiscoveredDeviceDto {
    /// Device type identifier
    pub device_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// HASS component
    pub component: String,
    /// HASS entity ID
    pub entity_id: String,
    /// Discovery topic
    pub discovery_topic: String,
    /// Device info
    pub device_info: HashMap<String, String>,
    /// Metric count
    pub metric_count: usize,
    /// Command count
    pub command_count: usize,
}

/// Process a HASS discovery message.
#[derive(Debug, Deserialize)]
pub struct HassDiscoveryMessageRequest {
    /// MQTT topic (e.g., "homeassistant/sensor/temperature/config")
    pub topic: String,
    /// Discovery message payload (JSON)
    pub payload: serde_json::Value,
}

/// Register an aggregated HASS device request.
#[derive(Debug, Deserialize)]
pub struct RegisterAggregatedHassDeviceRequest {
    /// Device ID (aggregated device identifier)
    pub device_id: String,
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
