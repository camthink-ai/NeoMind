//! MQTT subscription management handlers.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::json;


use super::models::{MqttSubscriptionDto, SubscribeRequest};
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// List MQTT subscriptions.
///
/// GET /api/mqtt/subscriptions
pub async fn list_subscriptions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Use new DeviceService to get devices
    let configs = state.device_service.list_devices().await;

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
    for config in configs {
        subscriptions.push(MqttSubscriptionDto {
            topic: format!("device/{}/uplink", config.device_id),
            qos: 1,
            device_id: Some(config.device_id.clone()),
        });
        subscriptions.push(MqttSubscriptionDto {
            topic: format!("device/{}/downlink", config.device_id),
            qos: 1,
            device_id: Some(config.device_id),
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
    State(_state): State<ServerState>,
    Json(_req): Json<SubscribeRequest>,
) -> HandlerResult<serde_json::Value> {
    // Check MQTT connection status from adapter
    
    // For now, just return a message - custom topic subscription not implemented
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
    // Validate the device exists using DeviceService
    let device_opt = state.device_service.get_device(&device_id).await;
    let device = device_opt.ok_or_else(|| {
        ErrorResponse::not_found(format!("Device not found: {}", device_id))
    })?;

    // Get the adapter for this device and subscribe
    if let Some(ref adapter_id) = device.adapter_id
        && let Some(adapter) = state.device_service.get_adapter(adapter_id).await {
            adapter
                .subscribe_device(&device_id)
                .await
                .map_err(|e| ErrorResponse::internal(format!("Failed to subscribe: {}", e)))?;
        }

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
    // Validate the device exists using DeviceService
    let configs = state.device_service.list_devices().await;
    let device_exists = configs.iter().any(|d| d.device_id == device_id);

    if !device_exists {
        return Err(ErrorResponse::not_found(format!(
            "Device not found: {}",
            device_id
        )));
    }

    ok(json!({
        "message": format!("Device {} uses wildcard subscription - no individual subscription to remove", device_id),
        "device_id": device_id,
    }))
}
