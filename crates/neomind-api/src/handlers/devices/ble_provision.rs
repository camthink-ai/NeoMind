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

/// Device-defined MQTT broker fields (host/port/TLS/creds/topic).
/// Used when `broker_id` is `"custom"` or when this object is present.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct CustomMqttRequest {
    /// Broker hostname or IP.
    pub host: String,
    /// Broker port (1–65535).
    pub port: u16,
    /// Optional MQTT username.
    #[serde(default)]
    pub username: String,
    /// Optional MQTT password.
    #[serde(default)]
    pub password: String,
    /// Whether the device should use TLS (mqtts).
    #[serde(default)]
    pub tls: bool,
    /// Optional topic prefix override (default `device/{type}/{id}`).
    /// May already end with `/uplink`; the CT write path normalizes it.
    #[serde(default)]
    pub topic_prefix: Option<String>,
    /// Optional MQTT client id override (default = device_id from SN).
    #[serde(default)]
    pub client_id: Option<String>,
}

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
    /// Broker identifier – "embedded" for the built-in broker, a UUID for a
    /// NeoMind-subscribed external broker, or `"custom"` with `custom_mqtt`.
    pub broker_id: String,
    /// Device-custom broker details. Required when `broker_id` is `"custom"`;
    /// when present, these fields win over NeoMind broker lookup.
    #[serde(default)]
    pub custom_mqtt: Option<CustomMqttRequest>,
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
    /// Explicit TLS flag for the device CT `mqtt.tls` field.
    pub tls: bool,
}

// ---------------------------------------------------------------------------
// Helper: resolve broker configuration
// ---------------------------------------------------------------------------

/// Resolve broker configuration from broker_id (NeoMind managed / subscribed).
fn resolve_broker_config(
    broker_id: &str,
) -> Result<(String, u16, String, String, bool), ErrorResponse> {
    if broker_id == "embedded" {
        let host = crate::handlers::common::get_server_host();
        let broker_config = config::get_embedded_broker_config();
        Ok((
            host,
            broker_config.port,
            String::new(),
            String::new(),
            broker_config.tls_enabled,
        ))
    } else if broker_id == "custom" {
        Err(ErrorResponse::bad_request(
            "broker_id \"custom\" requires custom_mqtt fields",
        ))
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
            broker.tls,
        ))
    }
}

/// Build the MQTT config written to the device (managed broker or custom).
fn resolve_mqtt_config(
    req: &BleProvisionRequest,
    device_type: &str,
    device_id: &str,
) -> Result<MqttConfigResponse, ErrorResponse> {
    let default_prefix = format!("device/{}/{}", device_type, device_id);

    if let Some(custom) = req.custom_mqtt.as_ref() {
        let host = custom.host.trim();
        if host.is_empty() {
            return Err(ErrorResponse::bad_request(
                "custom_mqtt.host must not be empty",
            ));
        }
        if custom.port == 0 {
            return Err(ErrorResponse::bad_request(
                "custom_mqtt.port must be between 1 and 65535",
            ));
        }
        let topic_prefix = custom
            .topic_prefix
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(default_prefix);
        let client_id = custom
            .client_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| device_id.to_string());
        return Ok(MqttConfigResponse {
            host: host.to_string(),
            port: custom.port,
            username: custom.username.clone(),
            password: custom.password.clone(),
            topic_prefix,
            client_id,
            tls: custom.tls,
        });
    }

    if req.broker_id == "custom" {
        return Err(ErrorResponse::bad_request(
            "broker_id \"custom\" requires custom_mqtt fields",
        ));
    }

    let (host, port, username, password, tls) = resolve_broker_config(&req.broker_id)?;
    Ok(MqttConfigResponse {
        host,
        port,
        username,
        password,
        topic_prefix: default_prefix,
        client_id: device_id.to_string(),
        tls,
    })
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

        let mqtt_config = resolve_mqtt_config(&req, &existing.device_type, &device_id)?;
        let topic_prefix = mqtt_config.topic_prefix.trim_end_matches('/').to_string();
        let topic_prefix = topic_prefix
            .strip_suffix("/uplink")
            .unwrap_or(&topic_prefix)
            .to_string();

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
            if req.custom_mqtt.is_some() {
                extra.insert(
                    "mqtt_broker_source".to_string(),
                    serde_json::Value::String("custom".to_string()),
                );
            }

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

    // 4. Resolve broker config (NeoMind subscribed or device-custom)
    let mqtt_config = resolve_mqtt_config(&req, &req.device_type, &device_id)?;
    let topic_prefix = mqtt_config.topic_prefix.trim_end_matches('/').to_string();
    let topic_prefix = topic_prefix
        .strip_suffix("/uplink")
        .unwrap_or(&topic_prefix)
        .to_string();

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
    if req.custom_mqtt.is_some() {
        extra.insert(
            "mqtt_broker_source".to_string(),
            serde_json::Value::String("custom".to_string()),
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_req(custom: Option<CustomMqttRequest>) -> BleProvisionRequest {
        BleProvisionRequest {
            model: "NE101".into(),
            sn: "NE101-A2F003".into(),
            device_type: "ne101_camera".into(),
            device_name: "cam".into(),
            broker_id: if custom.is_some() {
                "custom".into()
            } else {
                "embedded".into()
            },
            custom_mqtt: custom,
            resolve_only: true,
        }
    }

    #[test]
    fn custom_mqtt_builds_device_config() {
        let req = base_req(Some(CustomMqttRequest {
            host: "  broker.lan ".into(),
            port: 8883,
            username: "u".into(),
            password: "p".into(),
            tls: true,
            topic_prefix: Some("acme/devices/cam1/uplink".into()),
            client_id: Some("cam1".into()),
        }));
        let cfg = resolve_mqtt_config(&req, "ne101_camera", "ne101_a2f003").unwrap();
        assert_eq!(cfg.host, "broker.lan");
        assert_eq!(cfg.port, 8883);
        assert!(cfg.tls);
        assert_eq!(cfg.topic_prefix, "acme/devices/cam1/uplink");
        assert_eq!(cfg.client_id, "cam1");
        assert_eq!(cfg.username, "u");
    }

    #[test]
    fn custom_mqtt_defaults_topic_and_client_id() {
        let req = base_req(Some(CustomMqttRequest {
            host: "10.0.0.5".into(),
            port: 1883,
            username: String::new(),
            password: String::new(),
            tls: false,
            topic_prefix: None,
            client_id: None,
        }));
        let cfg = resolve_mqtt_config(&req, "ne101_camera", "ne101_a2f003").unwrap();
        assert_eq!(cfg.topic_prefix, "device/ne101_camera/ne101_a2f003");
        assert_eq!(cfg.client_id, "ne101_a2f003");
        assert!(!cfg.tls);
    }

    #[test]
    fn custom_broker_id_without_fields_errors() {
        let req = base_req(None);
        let mut req = req;
        req.broker_id = "custom".into();
        let err = resolve_mqtt_config(&req, "ne101_camera", "id").unwrap_err();
        assert!(err.message.contains("custom_mqtt"));
    }

    #[test]
    fn custom_mqtt_rejects_empty_host() {
        let req = base_req(Some(CustomMqttRequest {
            host: "   ".into(),
            port: 1883,
            username: String::new(),
            password: String::new(),
            tls: false,
            topic_prefix: None,
            client_id: None,
        }));
        assert!(resolve_mqtt_config(&req, "ne101_camera", "id").is_err());
    }
}
