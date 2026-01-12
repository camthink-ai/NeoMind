//! MQTT subscription management handlers.

use axum::{extract::{Path, State}, Json};
use serde_json::json;

use edge_ai_devices::ConnectionStatus;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::{MqttSubscriptionDto, SubscribeRequest};

/// List MQTT subscriptions.
///
/// GET /api/mqtt/subscriptions
pub async fn list_subscriptions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = &state.mqtt_device_manager;

    // Get the list of devices, which are all subscribed via wildcard
    let devices = manager.list_devices().await;

    // Build subscriptions list
    let mut subscriptions = vec![
        MqttSubscriptionDto {
            topic: "device/+/uplink".to_string(),
            qos: 1,
            device_id: None,
        },
        MqttSubscriptionDto {
            topic: "device/+/downlink".to_string(),
            qos: 1,
            device_id: None,
        },
    ];

    // Add per-device subscriptions
    for device in devices {
        subscriptions.push(MqttSubscriptionDto {
            topic: format!("device/{}/uplink", device.device_id),
            qos: 1,
            device_id: Some(device.device_id.clone()),
        });
        subscriptions.push(MqttSubscriptionDto {
            topic: format!("device/{}/downlink", device.device_id),
            qos: 1,
            device_id: Some(device.device_id),
        });
    }

    // Check if HASS discovery is enabled
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    let hass_discovery_enabled = store.load_hass_discovery_enabled().unwrap_or(false);
    if hass_discovery_enabled {
        subscriptions.push(MqttSubscriptionDto {
            topic: "homeassistant/+/+/config".to_string(),
            qos: 1,
            device_id: None,
        });
        subscriptions.push(MqttSubscriptionDto {
            topic: "homeassistant/+/+/+/config".to_string(),
            qos: 1,
            device_id: None,
        });
    }

    ok(json!({
        "subscriptions": subscriptions,
        "count": subscriptions.len(),
    }))
}

/// Subscribe to a topic.
///
/// POST /api/mqtt/subscribe
pub async fn subscribe_handler(
    State(state): State<ServerState>,
    Json(_req): Json<SubscribeRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = &state.mqtt_device_manager;
    let status = manager.connection_status().await;

    if !matches!(status, ConnectionStatus::Connected { .. }) {
        return Err(ErrorResponse::service_unavailable("MQTT client is not connected"));
    }

    // TODO: Implement custom topic subscription
    ok(json!({
        "success": false,
        "message": "Custom topic subscription not yet implemented. Use subscribe_device for specific devices.",
    }))
}

/// Unsubscribe from a topic.
///
/// POST /api/mqtt/unsubscribe
pub async fn unsubscribe_handler(
    State(_state): State<ServerState>,
    Json(_req): Json<SubscribeRequest>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Implement custom topic unsubscription
    ok(json!({
        "success": false,
        "message": "Custom topic unsubscription not yet implemented.",
    }))
}

/// Subscribe to a device's metrics.
///
/// POST /api/mqtt/subscribe/:device_id
pub async fn subscribe_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = &state.mqtt_device_manager;
    let status = manager.connection_status().await;

    if !matches!(status, ConnectionStatus::Connected { .. }) {
        return Err(ErrorResponse::service_unavailable("MQTT client is not connected"));
    }

    manager.subscribe_device(&device_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to subscribe: {}", e)))?;

    ok(json!({
        "message": format!("Subscribed to device: {}", device_id),
        "device_id": device_id,
    }))
}

/// Unsubscribe from a device's metrics.
///
/// POST /api/mqtt/unsubscribe/:device_id
pub async fn unsubscribe_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = &state.mqtt_device_manager;

    // Validate the device exists
    let devices = manager.list_devices().await;
    let device_exists = devices.iter().any(|d| d.device_id == device_id);

    if !device_exists {
        return Err(ErrorResponse::not_found(format!("Device not found: {}", device_id)));
    }

    ok(json!({
        "message": format!("Device {} uses wildcard subscription - no individual subscription to remove", device_id),
        "device_id": device_id,
    }))
}
