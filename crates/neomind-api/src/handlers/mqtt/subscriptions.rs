//! MQTT subscription management handlers.

use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::json;

use super::models::{MqttSubscribeRequest, MqttSubscriptionDto, MqttUnsubscribeRequest};
use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

// Import MqttAdapter for downcasting
use neomind_devices::adapters::mqtt::MqttAdapter;

/// List MQTT subscriptions.
///
/// GET /api/mqtt/subscriptions
#[utoipa::path(
    get,
    path = "/api/mqtt/subscriptions",
    tag = "mqtt",
    responses(
        (status = 200, description = "Active device telemetry subscriptions"),
    )
)]
pub async fn list_subscriptions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Use new DeviceService to get devices
    let configs = state.devices.service.list_devices();

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
#[utoipa::path(
    post,
    path = "/api/mqtt/subscribe",
    tag = "mqtt",
    request_body = MqttSubscribeRequest,
    responses(
        (status = 200, description = "Topic filter subscribed"),
    )
)]
pub async fn subscribe_handler(
    State(state): State<ServerState>,
    Json(req): Json<MqttSubscribeRequest>,
) -> HandlerResult<serde_json::Value> {
    use crate::validator::{
        validate_numeric_range, validate_required_string, validate_string_length,
    };

    // Validate topic
    validate_required_string(&req.topic, "topic")?;
    validate_string_length(&req.topic, "topic", 1, 200)?;

    // Validate QoS range (0-2)
    validate_numeric_range(req.qos as f64, "qos", 0.0, 2.0)?;

    // Get the first MQTT adapter
    let adapters = state.devices.service.list_adapters().await;
    let mqtt_adapter = adapters.iter().find(|a| a.adapter_type == "mqtt");

    let adapter_id = match mqtt_adapter {
        Some(a) => &a.id,
        None => {
            return ok(json!({
                "success": false,
                "message": "No MQTT adapter available",
            }));
        }
    };

    // Get the actual adapter instance
    let adapter_instance = state
        .devices
        .service
        .get_adapter(adapter_id)
        .await
        .ok_or_else(|| ErrorResponse::internal("MQTT adapter not found".to_string()))?;

    // Downcast to MqttAdapter to access subscribe_topic
    let mqtt_adapter = adapter_instance
        .as_any()
        .downcast_ref::<MqttAdapter>()
        .ok_or_else(|| ErrorResponse::internal("Adapter is not an MQTT adapter".to_string()))?;

    // Subscribe to the custom topic
    match mqtt_adapter.subscribe_topic(&req.topic).await {
        Ok(_) => {
            tracing::info!(
                category = "mqtt",
                topic = %req.topic,
                qos = req.qos,
                "Successfully subscribed to custom topic"
            );
            ok(json!({
                "success": true,
                "message": "Subscribed to topic",
                "topic": req.topic,
                "qos": req.qos,
            }))
        }
        Err(e) => {
            tracing::error!(
                category = "mqtt",
                topic = %req.topic,
                error = %e,
                "Failed to subscribe to custom topic"
            );
            ok(json!({
                "success": false,
                "message": format!("Failed to subscribe: {}", e),
                "topic": req.topic,
            }))
        }
    }
}

/// Unsubscribe from a topic.
///
/// POST /api/mqtt/unsubscribe
#[utoipa::path(
    post,
    path = "/api/mqtt/unsubscribe",
    tag = "mqtt",
    request_body = MqttUnsubscribeRequest,
    responses(
        (status = 200, description = "Topic filter unsubscribed"),
    )
)]
pub async fn unsubscribe_handler(
    State(state): State<ServerState>,
    Json(req): Json<MqttUnsubscribeRequest>,
) -> HandlerResult<serde_json::Value> {
    use crate::validator::{validate_required_string, validate_string_length};

    // Validate topic
    validate_required_string(&req.topic, "topic")?;
    validate_string_length(&req.topic, "topic", 1, 200)?;

    // Get the first MQTT adapter
    let adapters = state.devices.service.list_adapters().await;
    let mqtt_adapter = adapters.iter().find(|a| a.adapter_type == "mqtt");

    let adapter_id = match mqtt_adapter {
        Some(a) => &a.id,
        None => {
            return ok(json!({
                "success": false,
                "message": "No MQTT adapter available",
            }));
        }
    };

    // Get the actual adapter instance
    let adapter_instance = state
        .devices
        .service
        .get_adapter(adapter_id)
        .await
        .ok_or_else(|| ErrorResponse::internal("MQTT adapter not found".to_string()))?;

    // Downcast to MqttAdapter to access unsubscribe_topic
    let mqtt_adapter = adapter_instance
        .as_any()
        .downcast_ref::<MqttAdapter>()
        .ok_or_else(|| ErrorResponse::internal("Adapter is not an MQTT adapter".to_string()))?;

    // Unsubscribe from the custom topic
    match mqtt_adapter.unsubscribe_topic(&req.topic).await {
        Ok(_) => {
            tracing::info!(
                category = "mqtt",
                topic = %req.topic,
                "Successfully unsubscribed from custom topic"
            );
            ok(json!({
                "success": true,
                "message": "Unsubscribed from topic",
                "topic": req.topic,
            }))
        }
        Err(e) => {
            tracing::error!(
                category = "mqtt",
                topic = %req.topic,
                error = %e,
                "Failed to unsubscribe from custom topic"
            );
            ok(json!({
                "success": false,
                "message": format!("Failed to unsubscribe: {}", e),
                "topic": req.topic,
            }))
        }
    }
}

/// Subscribe to a device's metrics.
///
/// POST /api/mqtt/subscribe/:device_id
#[utoipa::path(
    post,
    path = "/api/mqtt/subscribe/{device_id}",
    tag = "mqtt",
    params(
        ("device_id" = String, Path, description = "Device id"),
    ),
    responses(
        (status = 200, description = "Device marked for telemetry ingestion"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn subscribe_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Validate the device exists using DeviceService
    let device_opt = state.devices.service.get_device(&device_id);
    let device = device_opt
        .ok_or_else(|| ErrorResponse::not_found(format!("Device not found: {}", device_id)))?;

    // Get the adapter for this device and subscribe
    if let Some(ref adapter_id) = device.adapter_id {
        if let Some(adapter) = state.devices.service.get_adapter(adapter_id).await {
            adapter
                .subscribe_device(&device_id)
                .await
                .map_err(|e| ErrorResponse::internal(format!("Failed to subscribe: {}", e)))?;
        }
    }

    ok(json!({
        "message": format!("Subscribed to device: {}", device_id),
        "device_id": device_id,
    }))
}

/// Unsubscribe from a device's metrics.
///
/// POST /api/mqtt/unsubscribe/:device_id
#[utoipa::path(
    post,
    path = "/api/mqtt/unsubscribe/{device_id}",
    tag = "mqtt",
    params(
        ("device_id" = String, Path, description = "Device id"),
    ),
    responses(
        (status = 200, description = "Device telemetry ingestion stopped"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn unsubscribe_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Validate the device exists using DeviceService
    let configs = state.devices.service.list_devices();
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
