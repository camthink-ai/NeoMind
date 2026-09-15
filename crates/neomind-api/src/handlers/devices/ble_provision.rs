//! BLE provisioning handler for zero-touch device setup.
//!
//! Two-phase provisioning to avoid phantom devices when BLE fails:
//!   Phase 1 (resolve_only=true):  Resolve MQTT config without registering.
//!   Phase 2 (resolve_only=false): Register device after BLE write succeeds.

use std::collections::HashMap;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config;
use crate::handlers::common::{ok, HandlerResult};
use crate::models::ErrorResponse;
use crate::server::types::ServerState;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// BLE provision request body.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct BleProvisionRequest {
    /// Device model identifier (e.g. "NE101").
    pub model: String,
    /// Device serial number (e.g. "NE101-A2F003").
    pub sn: String,
    /// Device type template identifier (e.g. "ne101_camera").
    pub device_type: String,
    /// Human-readable device name (e.g. "门口摄像头").
    pub device_name: String,
    /// Broker identifier – "embedded" for the built-in broker, or a UUID for an external broker.
    pub broker_id: String,
    /// If true, only resolve MQTT config without registering the device.
    /// Used for the two-phase BLE provisioning flow (resolve → BLE write → register).
    #[serde(default)]
    pub resolve_only: bool,
}

/// MQTT configuration returned to the BLE client so the device can connect.
#[derive(Debug, Serialize)]
pub struct MqttConfigResponse {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub topic_prefix: String,
    /// Device MQTT client ID (matches device_id)
    pub client_id: String,
}

// ---------------------------------------------------------------------------
// Helper: resolve broker configuration
// ---------------------------------------------------------------------------

/// Resolve broker configuration from broker_id.
fn resolve_broker_config(broker_id: &str) -> Result<(String, u16, String, String), ErrorResponse> {
    if broker_id == "embedded" {
        let store = config::open_settings_store().map_err(|e| {
            ErrorResponse::internal(format!("Failed to open settings store: {}", e))
        })?;
        let settings = store.get_mqtt_settings();
        let host = crate::handlers::common::get_server_host();
        Ok((host, settings.port, String::new(), String::new()))
    } else {
        let store = config::open_settings_store().map_err(|e| {
            ErrorResponse::internal(format!("Failed to open settings store: {}", e))
        })?;
        let broker = store
            .load_external_broker(broker_id)
            .map_err(|e| ErrorResponse::internal(format!("Failed to load broker: {}", e)))?
            .ok_or_else(|| ErrorResponse::not_found(format!("Broker not found: {}", broker_id)))?;
        Ok((
            broker.broker,
            broker.port,
            broker.username.unwrap_or_default(),
            broker.password.unwrap_or_default(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// BLE provision endpoint.
///
/// `POST /api/devices/ble-provision`
///
/// Two-phase provisioning:
///   Phase 1 (resolve_only=true): Validate, generate device_id, resolve MQTT config.
///     Does NOT register the device. Returns MQTT config for BLE write.
///   Phase 2 (resolve_only=false, default): Register the device after BLE write succeeds.
///     If device already exists, returns its config (idempotent).
#[utoipa::path(
    post,
    path = "/api/devices/ble-provision",
    tag = "devices",
    request_body = BleProvisionRequest,
    responses(
        (status = 200, description = "BLE provisioning session driven; result returned"),
    )
)]
pub async fn ble_provision_handler(
    State(state): State<ServerState>,
    Json(req): Json<BleProvisionRequest>,
) -> HandlerResult<serde_json::Value> {
    // 1. Validate device_type exists in registry
    if state
        .devices
        .service
        .get_template(&req.device_type)
        .is_none()
    {
        return Err(ErrorResponse::bad_request(format!(
            "Unknown device_type: {}",
            req.device_type
        )));
    }

    // 2. Generate device_id from SN
    let device_id = req.sn.to_lowercase().replace('-', "_");

    // 3. Check for duplicate — if device already exists, update or return config
    if let Some(existing) = state.devices.service.get_device(&device_id) {
        tracing::info!(
            category = "ble",
            device_id = %device_id,
            resolve_only = req.resolve_only,
            "BLE provision: device already exists"
        );

        let (host, port, username, password) = resolve_broker_config(&req.broker_id)?;
        let topic_prefix = format!("device/{}/{}", existing.device_type, device_id);

        let mqtt_config = MqttConfigResponse {
            host,
            port,
            username,
            password,
            topic_prefix: topic_prefix.clone(),
            client_id: device_id.clone(),
        };

        // Phase 2 (resolve_only=false): update existing device info
        if !req.resolve_only {
            let mut extra = existing.connection_config.extra.clone();
            extra.insert(
                "ble_reprovisioned".to_string(),
                serde_json::Value::Bool(true),
            );
            extra.insert(
                "provisioned_at".to_string(),
                serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
            );

            let updated = neomind_devices::DeviceConfig {
                device_id: device_id.clone(),
                name: req.device_name,
                device_type: existing.device_type.clone(),
                adapter_type: existing.adapter_type.clone(),
                connection_config: neomind_devices::ConnectionConfig {
                    telemetry_topic: Some(format!("{}/uplink", topic_prefix)),
                    command_topic: Some(format!("{}/downlink", topic_prefix)),
                    json_path: existing.connection_config.json_path.clone(),
                    entity_id: existing.connection_config.entity_id.clone(),
                    extra,
                },
                adapter_id: existing.adapter_id.clone(),
                last_seen: existing.last_seen,
                offline_timeout_secs: existing.offline_timeout_secs,
            };

            state
                .devices
                .service
                .update_device(&device_id, updated)
                .await
                .map_err(|e| {
                    ErrorResponse::internal(format!(
                        "Failed to update re-provisioned device: {}",
                        e
                    ))
                })?;

            tracing::info!(
                category = "ble",
                device_id = %device_id,
                broker_id = %req.broker_id,
                "BLE re-provisioned device updated"
            );
        }

        return ok(json!({
            "device_id": device_id,
            "mqtt_config": mqtt_config,
            "already_exists": true,
        }));
    }

    // 4. Resolve broker config
    let (host, port, username, password) = resolve_broker_config(&req.broker_id)?;
    let topic_prefix = format!("device/{}/{}", req.device_type, device_id);

    let mqtt_config = MqttConfigResponse {
        host,
        port,
        username,
        password,
        topic_prefix: topic_prefix.clone(),
        client_id: device_id.clone(),
    };

    // Phase 1: resolve_only — return MQTT config without registering
    if req.resolve_only {
        return ok(json!({
            "device_id": device_id,
            "mqtt_config": mqtt_config,
        }));
    }

    // Phase 2: register the device
    let mut extra = HashMap::new();
    extra.insert("ble_provisioned".to_string(), serde_json::Value::Bool(true));
    extra.insert(
        "provisioned_at".to_string(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );

    let config = neomind_devices::DeviceConfig {
        device_id: device_id.clone(),
        name: req.device_name,
        device_type: req.device_type,
        adapter_type: "mqtt".to_string(),
        connection_config: neomind_devices::ConnectionConfig {
            telemetry_topic: Some(format!("{}/uplink", topic_prefix)),
            command_topic: Some(format!("{}/downlink", topic_prefix)),
            json_path: None,
            entity_id: None,
            extra,
        },
        adapter_id: None,
        last_seen: 0,
        offline_timeout_secs: None,
    };

    state
        .devices
        .service
        .register_device(config)
        .await
        .map_err(|e| {
            ErrorResponse::internal(format!("Failed to register BLE provisioned device: {}", e))
        })?;

    tracing::info!(
        category = "ble",
        device_id = %device_id,
        broker_id = %req.broker_id,
        "BLE provisioned device registered"
    );

    ok(json!({
        "device_id": device_id,
        "mqtt_config": mqtt_config,
    }))
}
