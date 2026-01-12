//! Home Assistant integration API handlers.

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use axum::{extract::{Path, State}, Json};
use edge_ai_storage::HassSettings;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// DTO for HASS connection request.
#[derive(Debug, Deserialize)]
pub struct HassConnectRequest {
    /// HASS URL (e.g., http://localhost:8123)
    pub url: String,

    /// Long-lived access token
    pub token: String,

    /// Whether to verify SSL
    #[serde(default)]
    pub verify_ssl: bool,

    /// Auto-import all discovered devices
    #[serde(default)]
    pub auto_import: bool,
}

/// DTO for HASS status response.
#[derive(Debug, Serialize)]
pub struct HassStatusDto {
    /// Whether HASS integration is enabled
    pub enabled: bool,

    /// Whether connected to HASS
    pub connected: bool,

    /// HASS URL
    pub url: String,

    /// Last sync timestamp
    pub last_sync: Option<i64>,

    /// Number of imported entities
    pub entity_count: usize,
}

/// DTO for HASS entity.
#[derive(Debug, Serialize)]
pub struct HassEntityDto {
    /// Entity ID
    pub entity_id: String,

    /// Entity state
    pub state: String,

    /// Friendly name
    pub friendly_name: String,

    /// Domain
    pub domain: String,

    /// Device class
    pub device_class: Option<String>,

    /// Unit of measurement
    pub unit_of_measurement: Option<String>,
}

/// DTO for HASS device import request.
#[derive(Debug, Deserialize)]
pub struct HassImportRequest {
    /// List of entity IDs to import
    pub entity_ids: Vec<String>,

    /// Whether to enable auto-sync
    #[serde(default)]
    pub auto_sync: bool,
}

/// Get HASS integration settings and status.
///
/// GET /api/integration/hass
pub async fn get_hass_status_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let settings = store.get_hass_settings();

    // Check if connected (this would require an active connection check)
    let connected = settings.enabled && settings.token.is_some();

    ok(json!({
        "status": HassStatusDto {
            enabled: settings.enabled,
            connected,
            url: settings.url,
            last_sync: settings.last_sync,
            entity_count: 0, // TODO: Track imported entities
        },
    }))
}

/// Connect to Home Assistant.
///
/// POST /api/integration/hass/connect
pub async fn connect_hass_handler(
    State(_state): State<ServerState>,
    Json(req): Json<HassConnectRequest>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Actually test the connection to HASS
    // For now, just save the settings

    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    // Create new settings from request
    let mut settings = HassSettings {
        enabled: true,
        url: req.url,
        token: Some(req.token),
        verify_ssl: req.verify_ssl,
        auto_import: req.auto_import,
        ..Default::default()
    };

    settings.touch();

    store.save_hass_settings(&settings)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save settings: {}", e)))?;

    // TODO: Initialize HASS client and test connection
    // TODO: Start WebSocket connection for real-time updates

    ok(json!({
        "settings": {
            "enabled": true,
            "url": settings.url,
            "auto_import": settings.auto_import,
        }
    }))
}

/// Disconnect from Home Assistant.
///
/// DELETE /api/integration/hass/disconnect
pub async fn disconnect_hass_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let mut settings = store.get_hass_settings();
    settings.enabled = false;
    settings.touch();

    store.save_hass_settings(&settings)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save settings: {}", e)))?;

    // TODO: Close WebSocket connection

    ok(json!({
        "disconnected": true,
    }))
}

/// Get available HASS entities.
///
/// GET /api/integration/hass/entities
pub async fn get_hass_entities_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let settings = store.get_hass_settings();

    if !settings.enabled || settings.token.is_none() {
        return Err(ErrorResponse::service_unavailable("HASS integration is not connected"));
    }

    // TODO: Fetch entities from HASS using the client
    // For now, return empty list
    let entities: Vec<HassEntityDto> = vec![];
    ok(json!({
        "entities": entities,
        "count": 0,
    }))
}

/// Import HASS entities as NeoTalk devices.
///
/// POST /api/integration/hass/import
pub async fn import_hass_entities_handler(
    State(_state): State<ServerState>,
    Json(req): Json<HassImportRequest>,
) -> HandlerResult<serde_json::Value> {
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let settings = store.get_hass_settings();

    if !settings.enabled || settings.token.is_none() {
        return Err(ErrorResponse::service_unavailable("HASS integration is not connected"));
    }

    // TODO:
    // 1. Fetch entities from HASS
    // 2. Use HassEntityMapper to map them to NeoTalk devices
    // 3. Register devices with the device manager
    // 4. Start WebSocket sync for the entities

    let imported_count = req.entity_ids.len();

    ok(json!({
        "imported_count": imported_count,
        "entity_ids": req.entity_ids,
    }))
}

/// Get imported HASS devices.
///
/// GET /api/integration/hass/devices
pub async fn get_hass_devices_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Return devices imported from HASS
    // For now, return empty list
    let devices: Vec<serde_json::Value> = vec![];
    ok(json!({
        "devices": devices,
        "count": 0,
    }))
}

/// Sync a specific device state from HASS.
///
/// POST /api/integration/hass/sync/:entity_id
pub async fn sync_hass_entity_handler(
    State(_state): State<ServerState>,
    Path(entity_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Fetch and sync entity state from HASS
    ok(json!({
        "entity_id": entity_id,
        "synced": true,
    }))
}

/// Remove an imported HASS device.
///
/// DELETE /api/integration/hass/devices/:entity_id
pub async fn remove_hass_device_handler(
    State(_state): State<ServerState>,
    Path(entity_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Unregister device from device manager
    ok(json!({
        "entity_id": entity_id,
        "removed": true,
    }))
}
