//! Device CRUD operations.

use axum::{extract::{Path, State}, Json};
use serde_json::json;
use uuid::Uuid;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::{DeviceDto, AddDeviceRequest};

/// Map adapter_id to plugin display name
fn get_plugin_info(adapter_id: &Option<String>) -> (Option<String>, Option<String>) {
    match adapter_id {
        None => (Some("internal-mqtt".to_string()), Some("内置MQTT".to_string())),
        Some(id) if id.starts_with("hass") => (Some(id.clone()), Some("Home Assistant".to_string())),
        Some(id) if id.starts_with("modbus") => (Some(id.clone()), Some(format!("Modbus: {}", id))),
        Some(id) if id.starts_with("external-mqtt") => (Some(id.clone()), Some(format!("外部MQTT: {}", id))),
        Some(id) => (Some(id.clone()), Some(id.clone())),
    }
}

/// List devices.
pub async fn list_devices_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let devices = state.mqtt_device_manager.list_devices().await;
    let dtos: Vec<DeviceDto> = devices.into_iter().map(|d| {
        let (plugin_id, plugin_name) = get_plugin_info(&d.adapter_id);
        DeviceDto {
            id: d.device_id.clone(),
            name: d.name.clone(),
            device_type: d.device_type.clone(),
            status: d.status.as_str().to_string(),
            last_seen: d.last_seen.to_rfc3339(),
            plugin_id,
            plugin_name,
        }
    }).collect();

    ok(json!({
        "devices": dtos,
        "count": dtos.len(),
    }))
}

/// Get device details.
pub async fn get_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let device = state.mqtt_device_manager.get_device(&device_id).await
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    let device_type = state.mqtt_device_manager.get_device_type(&device.device_type).await;
    let metric_count = device_type.as_ref().map(|dt| dt.uplink.metrics.len()).unwrap_or(0);
    let command_count = device_type.as_ref().map(|dt| dt.downlink.commands.len()).unwrap_or(0);

    // Get current metric values
    let mut current_values = std::collections::HashMap::new();
    for key in device.current_values.keys() {
        if let Ok(value) = state.mqtt_device_manager.read_metric(&device_id, key).await {
            current_values.insert(key.clone(), super::metrics::value_to_json(&value));
        }
    }

    // Get plugin info for display
    let (plugin_id, plugin_name) = get_plugin_info(&device.adapter_id);

    ok(json!({
        "id": device.device_id,
        "device_id": device.device_id,
        "name": device.name,
        "device_type": device.device_type,
        "status": device.status.as_str(),
        "last_seen": device.last_seen.to_rfc3339(),
        "metric_count": metric_count,
        "command_count": command_count,
        "current_values": current_values,
        "config": device.config,
        "plugin_id": plugin_id,
        "plugin_name": plugin_name,
    }))
}

/// Delete a device.
pub async fn delete_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state.mqtt_device_manager.remove_device(&device_id).await?;
    ok(json!({
        "device_id": device_id,
        "deleted": true,
    }))
}

/// Add a new device manually.
pub async fn add_device_handler(
    State(state): State<ServerState>,
    Json(req): Json<AddDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    // Generate device ID if not provided: {device_type}_{random_8_chars}
    let device_id = if let Some(id) = req.device_id {
        id
    } else {
        // Generate random 8 character string
        let random_str: String = Uuid::new_v4()
            .to_string()
            .replace('-', "")
            .chars()
            .take(8)
            .collect();
        format!("{}_{}", req.device_type, random_str)
    };

    state.mqtt_device_manager.add_device(device_id.clone(), req.device_type, req.name.clone(), None, std::collections::HashMap::new()).await?;
    ok(json!({
        "device_id": device_id,
        "added": true,
    }))
}
