//! Device adapter management API handlers.
//!
//! This module provides REST API endpoints for managing device adapters.
//!
//! Note: The legacy plugin system has been migrated to the Extension system.
//! For dynamically loaded extensions (.so/.wasm files), use /api/extensions/* endpoints.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::json;

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// Adapter type definition for frontend.
#[derive(Debug, serde::Serialize)]
pub struct AdapterTypeDto {
    /// Type ID (e.g., "mqtt", "http", "webhook")
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Icon name (for frontend)
    pub icon: String,
    /// Icon background color class
    pub icon_bg: String,
    /// Connection mode (push/pull/hybrid)
    pub mode: String,
    /// Whether multiple instances can be created
    pub can_add_multiple: bool,
    /// Whether this is a built-in adapter
    pub builtin: bool,
}

/// Adapter plugin DTO with device information.
#[derive(Debug, serde::Serialize)]
pub struct AdapterPluginDto {
    /// Plugin ID
    pub id: String,
    /// Plugin name
    pub name: String,
    /// Adapter type (mqtt, http, etc.)
    pub adapter_type: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
    /// Whether the plugin is running
    pub running: bool,
    /// Number of devices managed
    pub device_count: usize,
    /// Plugin state
    pub state: String,
    /// Version
    pub version: String,
    /// Uptime in seconds
    pub uptime_secs: Option<u64>,
    /// Last activity timestamp
    pub last_activity: i64,
}

/// Response for devices managed by an adapter plugin.
#[derive(Debug, serde::Serialize)]
pub struct AdapterDevicesResponse {
    /// Plugin ID
    pub plugin_id: String,
    /// Plugin name
    pub plugin_name: String,
    /// Device list
    pub devices: Vec<AdapterDeviceDto>,
    /// Device count
    pub count: usize,
}

/// Device DTO managed by an adapter.
#[derive(Debug, serde::Serialize)]
pub struct AdapterDeviceDto {
    /// Device ID
    pub id: String,
    /// Device name (if available)
    pub name: Option<String>,
    /// Device type
    pub device_type: String,
    /// Status
    pub status: String,
    /// Last seen timestamp
    pub last_seen: i64,
}

/// Get all device adapter plugins.
///
/// GET /api/plugins/device-adapters
pub async fn list_device_adapter_plugins_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let stats = state.device_service.get_adapter_stats().await;

    let adapters: Vec<AdapterPluginDto> = stats
        .adapters
        .into_iter()
        .map(|adapter| AdapterPluginDto {
            id: adapter.id,
            name: adapter.name,
            adapter_type: adapter.adapter_type,
            enabled: true,
            running: adapter.running,
            device_count: adapter.device_count,
            state: adapter.status,
            version: "1.0.0".to_string(),
            uptime_secs: None,
            last_activity: adapter.last_activity,
        })
        .collect();

    ok(json!({
        "total_adapters": stats.total_adapters,
        "running_adapters": stats.running_adapters,
        "total_devices": stats.total_devices,
        "adapters": adapters,
    }))
}

/// Register a new device adapter plugin.
///
/// POST /api/plugins/device-adapters
pub async fn register_device_adapter_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_devices::adapters::create_adapter;

    // Parse request
    let id = req
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'id' field"))?;

    let adapter_type = req
        .get("adapter_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'adapter_type' field"))?;

    let config = req
        .get("config")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let auto_start = req
        .get("auto_start")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Get event bus for adapter creation
    let event_bus = state.event_bus.as_ref().ok_or_else(|| {
        ErrorResponse::internal("Event bus not initialized")
    })?;

    // Create the adapter using the factory
    let adapter = create_adapter(adapter_type, &config, event_bus)
        .map_err(|e| ErrorResponse::internal(format!("Failed to create adapter: {}", e)))?;

    // Register with DeviceService
    state
        .device_service
        .register_adapter(id.to_string(), adapter.clone())
        .await;

    // Auto-start if requested
    if auto_start
        && let Err(e) = adapter.start().await {
            tracing::warn!("Failed to auto-start adapter {}: {}", id, e);
        }

    ok(json!({
        "message": "Device adapter registered successfully",
        "adapter_id": id,
    }))
}

/// Get devices managed by a device adapter plugin.
///
/// GET /api/plugins/:id/devices
pub async fn get_adapter_devices_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let devices = state
        .device_service
        .get_adapter_device_ids(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Adapter {}", id)))?;

    let device_dtos: Vec<AdapterDeviceDto> = devices
        .into_iter()
        .map(|device_id| AdapterDeviceDto {
            id: device_id.clone(),
            name: None,
            device_type: "unknown".to_string(),
            status: "online".to_string(),
            last_seen: chrono::Utc::now().timestamp(),
        })
        .collect();

    ok(json!({
        "adapter_id": id,
        "devices": device_dtos,
        "count": device_dtos.len(),
    }))
}

/// Get device adapter plugin statistics.
///
/// GET /api/plugins/device-adapters/stats
pub async fn get_device_adapter_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let stats = state.device_service.get_adapter_stats().await;

    ok(json!({
        "total_adapters": stats.total_adapters,
        "running_adapters": stats.running_adapters,
        "total_devices": stats.total_devices,
        "adapters": stats.adapters,
    }))
}

/// Get available device adapter types.
///
/// Similar to /llm-backends/types, this returns the available adapter types
/// that can be used to create new connections.
///
/// GET /api/device-adapters/types
pub async fn list_adapter_types_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_devices::adapters::available_adapters;

    let available = available_adapters();

    // Define adapter type metadata (this could be moved to a config file in the future)
    let type_info: Vec<AdapterTypeDto> = available
        .into_iter()
        .map(|adapter_type| match adapter_type {
            "mqtt" => AdapterTypeDto {
                id: "mqtt".to_string(),
                name: "MQTT".to_string(),
                description: "MQTT broker connections (built-in + external)".to_string(),
                icon: "Server".to_string(),
                icon_bg: "bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400".to_string(),
                mode: "push".to_string(),
                can_add_multiple: true,
                builtin: true,
            },
            "http" => AdapterTypeDto {
                id: "http".to_string(),
                name: "HTTP (Polling)".to_string(),
                description: "Poll data from device REST APIs on a schedule".to_string(),
                icon: "Radio".to_string(),
                icon_bg: "bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400".to_string(),
                mode: "pull".to_string(),
                can_add_multiple: true,
                builtin: true,
            },
            "webhook" => AdapterTypeDto {
                id: "webhook".to_string(),
                name: "Webhook".to_string(),
                description: "Devices push data via HTTP POST to your server".to_string(),
                icon: "Webhook".to_string(),
                icon_bg: "bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400".to_string(),
                mode: "push".to_string(),
                can_add_multiple: false,
                builtin: true,
            },
            _ => AdapterTypeDto {
                id: adapter_type.to_string(),
                name: adapter_type.to_uppercase(),
                description: format!("{} device adapter", adapter_type),
                icon: "Server".to_string(),
                icon_bg: "bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400".to_string(),
                mode: "unknown".to_string(),
                can_add_multiple: true,
                builtin: true,
            },
        })
        .collect();

    ok(json!({
        "types": type_info,
        "count": type_info.len(),
    }))
}

/// Deprecated: List all plugins.
///
/// GET /api/plugins
///
/// This endpoint is deprecated. Use /api/extensions for dynamically loaded extensions.
pub async fn list_plugins_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Return device adapters as the only "plugins"
    let stats = state.device_service.get_adapter_stats().await;

    let adapters: Vec<serde_json::Value> = stats
        .adapters
        .into_iter()
        .map(|adapter| json!({
            "id": adapter.id,
            "name": adapter.name,
            "type": format!("device_adapter_{}", adapter.adapter_type),
            "category": "devices",
            "state": adapter.status,
            "enabled": true,
            "running": adapter.running,
            "device_count": adapter.device_count,
        }))
        .collect();

    ok(json!({
        "plugins": adapters,
        "count": adapters.len(),
        "notice": "Plugin API is deprecated. Use /api/extensions for dynamically loaded extensions.",
    }))
}

/// Deprecated: Get plugin by ID.
///
/// GET /api/plugins/:id
pub async fn get_plugin_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(format!(
        "Plugin API is deprecated. Use /api/extensions/{} instead.",
        id
    )))
}

/// Deprecated: Register a plugin.
///
/// POST /api/plugins
pub async fn register_plugin_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin registration API is deprecated. Use POST /api/extensions to register extensions.",
    ))
}

/// Deprecated: Unregister a plugin.
///
/// DELETE /api/plugins/:id
pub async fn unregister_plugin_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(format!(
        "Plugin API is deprecated. Use DELETE /api/extensions/{} instead.",
        id
    )))
}

/// Deprecated: Enable a plugin.
///
/// POST /api/plugins/:id/enable
pub async fn enable_plugin_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use Extension API instead.",
    ))
}

/// Deprecated: Disable a plugin.
///
/// POST /api/plugins/:id/disable
pub async fn disable_plugin_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use Extension API instead.",
    ))
}

/// Deprecated: Start a plugin.
///
/// POST /api/plugins/:id/start
pub async fn start_plugin_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use Extension API instead.",
    ))
}

/// Deprecated: Stop a plugin.
///
/// POST /api/plugins/:id/stop
pub async fn stop_plugin_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use Extension API instead.",
    ))
}

/// Deprecated: Health check for a plugin.
///
/// GET /api/plugins/:id/health
pub async fn plugin_health_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(format!(
        "Plugin API is deprecated. Use GET /api/extensions/{}/health instead.",
        id
    )))
}

/// Deprecated: Get plugin configuration.
///
/// GET /api/plugins/:id/config
pub async fn get_plugin_config_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(format!(
        "Plugin API is deprecated. Use Extension API instead (requested: {}).",
        id
    )))
}

/// Deprecated: Update plugin configuration.
///
/// PUT /api/plugins/:id/config
pub async fn update_plugin_config_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use Extension API instead.",
    ))
}

/// Deprecated: Execute a plugin command.
///
/// POST /api/plugins/:id/command
pub async fn execute_plugin_command_handler(
    State(_state): State<ServerState>,
    Path(_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use POST /api/extensions/:id/command instead.",
    ))
}

/// Deprecated: Get plugin statistics.
///
/// GET /api/plugins/:id/stats
pub async fn get_plugin_stats_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(format!(
        "Plugin API is deprecated. Use GET /api/extensions/{}/stats instead.",
        id
    )))
}

/// Deprecated: Discover plugins.
///
/// POST /api/plugins/discover
pub async fn discover_plugins_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin discovery API is deprecated. Use POST /api/extensions/discover instead.",
    ))
}

/// Deprecated: List plugins by type.
///
/// GET /api/plugins/type/:type
pub async fn list_plugins_by_type_handler(
    State(_state): State<ServerState>,
    Path(_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin API is deprecated. Use GET /api/extensions with type filter instead.",
    ))
}

/// Deprecated: Get plugin types.
///
/// GET /api/plugins/types
pub async fn get_plugin_types_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    Err(ErrorResponse::gone(
        "Plugin types API is deprecated. Use GET /api/extensions/types instead.",
    ))
}
