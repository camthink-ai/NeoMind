//! Device DTOs and request structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simple device info for API responses.
#[derive(Debug, Serialize)]
pub struct DeviceDto {
    pub id: String,
    pub name: Option<String>,
    pub device_type: String,
    pub status: String,
    pub last_seen: String,
    /// Plugin ID that manages this device (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Plugin name that manages this device (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

/// Device type info for API responses.
#[derive(Debug, Serialize)]
pub struct DeviceTypeDto {
    pub device_type: String,
    pub name: String,
    pub description: String,
    pub categories: Vec<String>,
    pub metric_count: usize,
    pub command_count: usize,
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
    /// Optional device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
