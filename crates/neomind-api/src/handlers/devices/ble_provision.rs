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
use crate::handlers::devices::auto_onboard::get_auto_onboard_manager;
use crate::models::ErrorResponse;
use crate::server::types::ServerState;
use neomind_devices::DeviceError;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Device-defined MQTT broker fields matching the CT-Prov/1 mqtt section.
/// Used when `broker_id` is `"custom"` or when this object is present.
///
/// Protocol fields: enable, host, port, client_id, username, password, tls,
/// topic (via topic_prefix), qos. keep_alive / clean_session / will / alpn
/// are not part of CT-Prov/1 — do not invent them here.
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
    /// MQTT enable flag (default true).
    #[serde(default = "default_mqtt_enable")]
    pub enable: bool,
    /// Publish/subscribe QoS 0–2 (default 1).
    #[serde(default = "default_mqtt_qos")]
    pub qos: u8,
    /// Optional topic prefix override (default `device/{type}/{id}`).
    /// May already end with `/uplink`; the CT write path normalizes it.
    #[serde(default)]
    pub topic_prefix: Option<String>,
    /// Optional MQTT client id override (default = device_id from SN).
    #[serde(default)]
    pub client_id: Option<String>,
}

fn default_mqtt_enable() -> bool {
    true
}

fn default_mqtt_qos() -> u8 {
    1
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
/// Mirrors the CT-Prov/1 mqtt section (enable/host/port/client_id/username/
/// password/tls/topic/qos) — `topic` is derived from `topic_prefix` on write.
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
    /// CT `mqtt.enable` (normally true for provision).
    pub enable: bool,
    /// CT `mqtt.qos` (0–2).
    pub qos: u8,
}

// ---------------------------------------------------------------------------
// Helper: resolve broker configuration
// ---------------------------------------------------------------------------

/// Map an external broker's optional auth fields into CT mqtt username/password.
/// Empty / missing credentials stay empty so the write path can omit them.
fn external_broker_credentials(
    username: Option<String>,
    password: Option<String>,
) -> (String, String) {
    (username.unwrap_or_default(), password.unwrap_or_default())
}

/// Runtime adapter id NeoMind should bind the provisioned device to.
///
/// Embedded → `internal-mqtt`. Subscribed external → `external-{broker_id}` so
/// topic maps / presence track the same broker the device was written for.
/// Custom (device-only broker) → `None` (broadcast subscribe; NeoMind may not
/// own that broker).
fn adapter_id_for_broker(broker_id: &str) -> Option<String> {
    if broker_id == "embedded" {
        Some("internal-mqtt".to_string())
    } else if broker_id == "custom" || broker_id.is_empty() {
        None
    } else {
        Some(format!("external-{broker_id}"))
    }
}

/// Persist which NeoMind broker the BLE write targeted (for later matching).
fn record_broker_source(extra: &mut HashMap<String, serde_json::Value>, req: &BleProvisionRequest) {
    let source = if req.custom_mqtt.is_some() || req.broker_id == "custom" {
        "custom"
    } else {
        req.broker_id.as_str()
    };
    extra.insert(
        "mqtt_broker_source".to_string(),
        serde_json::Value::String(source.to_string()),
    );
    extra.insert(
        "mqtt_broker_id".to_string(),
        serde_json::Value::String(req.broker_id.clone()),
    );
}

/// Load a subscribed external broker and return host/port/creds/tls for the
/// device CT write — same fields NeoMind's `external-{id}` adapter uses to
/// connect, so device auth matches the bridge.
fn resolve_external_broker(
    store: &neomind_storage::SettingsStore,
    broker_id: &str,
) -> Result<(String, u16, String, String, bool), ErrorResponse> {
    let broker = store
        .load_external_broker(broker_id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Broker not found: {}", broker_id)))?;
    let (username, password) =
        external_broker_credentials(broker.username.clone(), broker.password.clone());
    Ok((broker.broker, broker.port, username, password, broker.tls))
}

/// Resolve broker configuration from broker_id (NeoMind managed / subscribed).
///
/// Embedded host/port/TLS come from the live broker config. Username/password
/// for embedded stay empty here — when auth is enabled, phase-1 resolve mints
/// a device-scoped credential via [`attach_embedded_auth_credentials`] so the
/// CT write receives plaintext the bcrypt store cannot echo.
///
/// External brokers copy stored username/password verbatim into mqtt_config
/// so the device authenticates the same way NeoMind's subscribed adapter does.
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
        resolve_external_broker(store.as_ref(), broker_id)
    }
}

/// Mint (or rotate) a device-scoped MQTT user for the embedded broker.
///
/// Embedded user passwords are bcrypt-only at rest, so BLE provision cannot
/// echo an operator-created credential. Each resolve_only phase gets a fresh
/// plaintext pair keyed by `device_id`; the hash is upserted for the broker.
fn mint_device_mqtt_credential(
    store: &neomind_storage::SettingsStore,
    device_id: &str,
) -> Result<(String, String), ErrorResponse> {
    let username = device_id.trim().to_string();
    if username.is_empty() || username.len() > 64 {
        return Err(ErrorResponse::bad_request(
            "device id is not a valid MQTT username (1-64 chars)",
        ));
    }
    if username.starts_with("__neomind") {
        return Err(ErrorResponse::bad_request(
            "device id collides with reserved MQTT system username prefix",
        ));
    }

    let password = generate_provision_password(16);
    let password_hash = bcrypt::hash(&password, 12)
        .map_err(|e| ErrorResponse::internal(format!("Failed to hash MQTT password: {e}")))?;
    store
        .add_mqtt_credential(&username, &password_hash)
        .map_err(|e| ErrorResponse::internal(format!("Failed to store MQTT credential: {e}")))?;
    Ok((username, password))
}

fn generate_provision_password(length: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

fn refresh_credential_cache(state: &ServerState) {
    #[cfg(feature = "embedded-broker")]
    {
        if let Ok(store) = config::open_settings_store() {
            match crate::server::types::CredentialCache::load_from_store(&store) {
                Ok(cache) => {
                    *state
                        .credential_cache
                        .write()
                        .unwrap_or_else(|e| e.into_inner()) = cache;
                }
                Err(e) => tracing::error!(
                    category = "ble",
                    error = %e,
                    "Failed to refresh MQTT credential cache after BLE provision mint"
                ),
            }
        }
    }
    #[cfg(not(feature = "embedded-broker"))]
    {
        let _ = state;
    }
}

/// When the embedded broker requires auth, attach a device credential on the
/// resolve_only (BLE write) phase so config.set includes username/password.
///
/// Phase 2 register must not rotate — the device already holds the phase-1
/// plaintext. External / custom brokers already carry their own credentials.
fn attach_embedded_auth_credentials(
    state: &ServerState,
    req: &BleProvisionRequest,
    device_id: &str,
    cfg: &mut MqttConfigResponse,
) -> Result<(), ErrorResponse> {
    if req.broker_id != "embedded" || !req.resolve_only {
        return Ok(());
    }
    if !config::get_embedded_broker_config().auth_enabled {
        return Ok(());
    }
    // Defensive: if somehow already populated, keep.
    if !cfg.username.is_empty() || !cfg.password.is_empty() {
        return Ok(());
    }

    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {e}")))?;
    let (username, password) = mint_device_mqtt_credential(store.as_ref(), device_id)?;
    cfg.username = username;
    cfg.password = password;
    refresh_credential_cache(state);

    tracing::info!(
        category = "ble",
        device_id = %device_id,
        "BLE provision minted embedded-broker MQTT credential for device"
    );
    Ok(())
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
        if custom.qos > 2 {
            return Err(ErrorResponse::bad_request(
                "custom_mqtt.qos must be 0, 1, or 2",
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
            enable: custom.enable,
            qos: custom.qos,
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
        enable: true,
        qos: default_mqtt_qos(),
    })
}

/// Resolve MQTT config and, when needed, attach embedded-broker device creds.
fn resolve_mqtt_config_for_provision(
    state: &ServerState,
    req: &BleProvisionRequest,
    device_type: &str,
    device_id: &str,
) -> Result<MqttConfigResponse, ErrorResponse> {
    let mut cfg = resolve_mqtt_config(req, device_type, device_id)?;
    attach_embedded_auth_credentials(state, req, device_id, &mut cfg)?;
    Ok(cfg)
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

        let mqtt_config =
            resolve_mqtt_config_for_provision(&state, &req, &existing.device_type, &device_id)?;
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
            record_broker_source(&mut extra, &req);

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
                // Re-bind to the broker selected for this write (may change).
                adapter_id: adapter_id_for_broker(&req.broker_id),
                last_seen: existing.last_seen,
                offline_timeout_secs: existing.offline_timeout_secs,
            };

            // Refresh adapter topic maps (plain update_device leaves runtime stale).
            state
                .devices
                .service
                .update_device_and_refresh_adapters(&device_id, updated)
                .await
                .map_err(ble_bind_error)?;

            clear_discovery_drafts_for_identity(&state, &device_id).await;

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
    let mqtt_config =
        resolve_mqtt_config_for_provision(&state, &req, &req.device_type, &device_id)?;
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

    // Phase 2: register the device on the same NeoMind adapter that owns
    // the selected broker (external-{id} / internal-mqtt) so topic maps match.
    let mut extra = HashMap::new();
    extra.insert("ble_provisioned".to_string(), serde_json::Value::Bool(true));
    extra.insert(
        "provisioned_at".to_string(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    record_broker_source(&mut extra, &req);

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
        adapter_id: adapter_id_for_broker(&req.broker_id),
        last_seen: 0,
        offline_timeout_secs: None,
    };

    state
        .devices
        .service
        .register_device(config)
        .await
        .map_err(ble_bind_error)?;

    // MQTT may have drafted this SN before BLE finished — drop Pending ghosts.
    clear_discovery_drafts_for_identity(&state, &device_id).await;

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

/// Best-effort: clear auto-onboard drafts for a formal BLE registration.
/// Manager may be absent in minimal test states — never fail the provision.
async fn clear_discovery_drafts_for_identity(state: &ServerState, device_id: &str) {
    match get_auto_onboard_manager(state).await {
        Ok(manager) => {
            let removed = manager.remove_drafts_for_identity(device_id).await;
            if removed > 0 {
                tracing::info!(
                    category = "ble",
                    device_id = %device_id,
                    removed,
                    "BLE provision cleared leftover discovery draft(s)"
                );
            }
        }
        Err(e) => {
            tracing::debug!(
                category = "ble",
                device_id = %device_id,
                error = %e.message,
                "BLE provision: auto-onboard manager unavailable; draft clear skipped"
            );
        }
    }
}

fn ble_bind_error(e: DeviceError) -> ErrorResponse {
    match e {
        DeviceError::BindIncomplete {
            message,
            config_applied,
            retry_hint,
        } => ErrorResponse::service_unavailable(format!(
            "{message}; config_applied={config_applied}"
        ))
        .with_hint(retry_hint),
        other => ErrorResponse::internal(format!("Failed to register/update BLE device: {other}")),
    }
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
            enable: true,
            qos: 2,
            topic_prefix: Some("acme/devices/cam1/uplink".into()),
            client_id: Some("cam1".into()),
        }));
        let cfg = resolve_mqtt_config(&req, "ne101_camera", "ne101_a2f003").unwrap();
        assert_eq!(cfg.host, "broker.lan");
        assert_eq!(cfg.port, 8883);
        assert!(cfg.tls);
        assert!(cfg.enable);
        assert_eq!(cfg.qos, 2);
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
            enable: true,
            qos: 1,
            topic_prefix: None,
            client_id: None,
        }));
        let cfg = resolve_mqtt_config(&req, "ne101_camera", "ne101_a2f003").unwrap();
        assert_eq!(cfg.topic_prefix, "device/ne101_camera/ne101_a2f003");
        assert_eq!(cfg.client_id, "ne101_a2f003");
        assert!(!cfg.tls);
        assert!(cfg.enable);
        assert_eq!(cfg.qos, 1);
    }

    #[test]
    fn custom_mqtt_rejects_invalid_qos() {
        let req = base_req(Some(CustomMqttRequest {
            host: "broker.lan".into(),
            port: 1883,
            username: String::new(),
            password: String::new(),
            tls: false,
            enable: true,
            qos: 3,
            topic_prefix: None,
            client_id: None,
        }));
        assert!(resolve_mqtt_config(&req, "ne101_camera", "id").is_err());
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
            enable: true,
            qos: 1,
            topic_prefix: None,
            client_id: None,
        }));
        assert!(resolve_mqtt_config(&req, "ne101_camera", "id").is_err());
    }

    #[test]
    fn external_broker_credentials_include_when_present() {
        let (u, p) = external_broker_credentials(Some("device".into()), Some("s3cret".into()));
        assert_eq!(u, "device");
        assert_eq!(p, "s3cret");
    }

    #[test]
    fn external_broker_credentials_empty_when_absent() {
        let (u, p) = external_broker_credentials(None, None);
        assert!(u.is_empty());
        assert!(p.is_empty());
        // Partial: username only still surfaces (password omitted on CT write).
        let (u2, p2) = external_broker_credentials(Some("only-user".into()), None);
        assert_eq!(u2, "only-user");
        assert!(p2.is_empty());
    }

    #[test]
    fn mint_device_mqtt_credential_stores_hash_and_returns_plaintext() {
        let tmp = tempfile::tempdir().unwrap();
        let store = neomind_storage::SettingsStore::open(tmp.path().join("settings.redb")).unwrap();
        let device_id = "ne101_a2f003";
        let (username, password) = mint_device_mqtt_credential(store.as_ref(), device_id).unwrap();
        assert_eq!(username, device_id);
        assert_eq!(password.len(), 16);
        let creds = store.list_mqtt_credentials().unwrap();
        let row = creds.iter().find(|c| c.username == device_id).unwrap();
        assert!(bcrypt::verify(&password, &row.password_hash).unwrap());

        // Rotate on re-provision: new plaintext, updated hash.
        let (u2, p2) = mint_device_mqtt_credential(store.as_ref(), device_id).unwrap();
        assert_eq!(u2, device_id);
        assert_ne!(p2, password);
        let creds2 = store.list_mqtt_credentials().unwrap();
        let row2 = creds2.iter().find(|c| c.username == device_id).unwrap();
        assert!(bcrypt::verify(&p2, &row2.password_hash).unwrap());
        assert!(!bcrypt::verify(&password, &row2.password_hash).unwrap());
    }

    #[test]
    fn custom_mqtt_preserves_username_and_password() {
        let req = base_req(Some(CustomMqttRequest {
            host: "broker.lan".into(),
            port: 1883,
            username: "cam".into(),
            password: "p@ss".into(),
            tls: false,
            enable: true,
            qos: 1,
            topic_prefix: None,
            client_id: None,
        }));
        let cfg = resolve_mqtt_config(&req, "ne101_camera", "ne101_a2f003").unwrap();
        assert_eq!(cfg.username, "cam");
        assert_eq!(cfg.password, "p@ss");
    }

    #[test]
    fn adapter_id_matches_neomind_broker_selection() {
        assert_eq!(
            adapter_id_for_broker("embedded").as_deref(),
            Some("internal-mqtt")
        );
        assert_eq!(
            adapter_id_for_broker("abc-uuid").as_deref(),
            Some("external-abc-uuid")
        );
        assert_eq!(adapter_id_for_broker("custom"), None);
    }

    #[test]
    fn subscribed_external_broker_creds_reach_device_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        let store = neomind_storage::SettingsStore::open(tmp.path().join("settings.redb")).unwrap();
        let mut broker = neomind_storage::ExternalBroker::new(
            "broker-1".into(),
            "Cloud MQTT".into(),
            "mqtt.example.com".into(),
            8883,
        );
        broker.tls = true;
        broker.username = Some("device-user".into());
        broker.password = Some("device-pass".into());
        store.save_external_broker(&broker).unwrap();

        let (host, port, username, password, tls) =
            resolve_external_broker(store.as_ref(), "broker-1").unwrap();
        assert_eq!(host, "mqtt.example.com");
        assert_eq!(port, 8883);
        assert!(tls);
        assert_eq!(username, "device-user");
        assert_eq!(password, "device-pass");
        // Adapter binding for this broker_id matches NeoMind's external adapter name.
        assert_eq!(
            adapter_id_for_broker("broker-1").as_deref(),
            Some("external-broker-1")
        );
    }
}
