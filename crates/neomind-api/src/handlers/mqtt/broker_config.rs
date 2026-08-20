//! Embedded MQTT broker configuration API handlers.
//!
//! This module provides REST API endpoints for managing the embedded broker's
//! configuration, including authentication settings, TLS certificates, and
//! credential management.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use neomind_devices::EmbeddedBrokerConfig;
use neomind_storage::settings::MqttCredential;

use crate::config;
use crate::handlers::common::{ok, HandlerResult};
use crate::handlers::mqtt::cert_gen;
use crate::models::ErrorResponse;
use crate::server::types::ServerState;

/// DTO for embedded broker configuration response.
#[derive(Debug, Serialize)]
struct BrokerConfigDto {
    /// Listening address
    listen: String,
    /// Listening port
    port: u16,
    /// Maximum concurrent connections
    max_connections: usize,
    /// Maximum payload size in bytes
    max_payload_size: usize,
    /// Enable dynamic topic filters
    dynamic_filters: bool,
    /// Authentication enabled
    auth_enabled: bool,
    /// TLS enabled
    tls_enabled: bool,
    /// TLS certificate path (if configured)
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_cert_path: Option<String>,
    /// TLS private key path (if configured)
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_key_path: Option<String>,
    /// TLS CA certificate path (if configured)
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_ca_path: Option<String>,
    /// Payload field used as device identity when auto-discovery can't
    /// uniquely identify a device from the topic
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id_field: Option<String>,
    /// User credentials (excluding internal system credentials)
    credentials: Vec<CredentialDto>,
}

/// DTO for a single credential (response only, password masked).
#[derive(Debug, Serialize)]
struct CredentialDto {
    /// Username
    username: String,
    /// Masked password indicator
    password: String,
}

impl From<MqttCredential> for CredentialDto {
    fn from(cred: MqttCredential) -> Self {
        Self {
            username: cred.username,
            password: "*****".to_string(),
        }
    }
}

/// Request body for updating broker configuration.
#[derive(Debug, Deserialize)]
pub struct UpdateBrokerConfigRequest {
    /// Listening address
    #[serde(default)]
    listen: Option<String>,
    /// Listening port (1024-65535)
    #[serde(default)]
    port: Option<u16>,
    /// Enable authentication
    #[serde(default)]
    auth_enabled: Option<bool>,
    /// Enable TLS
    #[serde(default)]
    tls_enabled: Option<bool>,
    /// Payload field used as device identity when auto-discovery can't
    /// uniquely identify a device from the topic (gateway forwarding many
    /// devices on one topic). None/empty → auto-detect common fields.
    #[serde(default)]
    device_id_field: Option<String>,
}

/// Request body for adding a new credential.
#[derive(Debug, Deserialize)]
pub struct AddCredentialRequest {
    /// Username (1-64 chars, cannot start with "__neomind")
    pub username: String,
    /// Password (min 4 chars)
    pub password: String,
}

/// Request body for uploading TLS certificates.
#[derive(Debug, Deserialize)]
pub struct UploadTlsRequest {
    /// Certificate PEM content
    pub cert_pem: String,
    /// Private key PEM content
    pub key_pem: String,
    /// Optional CA certificate PEM content
    #[serde(default)]
    pub ca_pem: Option<String>,
}

/// Get embedded broker configuration.
///
/// GET /api/mqtt/broker-config
///
/// Returns the current broker configuration with credentials.
/// Passwords are masked. Internal system credentials (starting with `__neomind`)
/// are filtered out from the credentials list.
pub async fn get_broker_config_handler() -> HandlerResult<serde_json::Value> {
    let config = config::get_embedded_broker_config();

    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let all_creds = store
        .list_mqtt_credentials()
        .map_err(|e| ErrorResponse::internal(format!("Failed to load credentials: {}", e)))?;

    // Filter out internal system credentials
    let credentials: Vec<CredentialDto> = all_creds
        .into_iter()
        .filter(|c| !c.username.starts_with("__neomind"))
        .map(CredentialDto::from)
        .collect();

    let dto = BrokerConfigDto {
        listen: config.listen,
        port: config.port,
        max_connections: config.max_connections,
        max_payload_size: config.max_payload_size,
        dynamic_filters: config.dynamic_filters,
        auth_enabled: config.auth_enabled,
        tls_enabled: config.tls_enabled,
        tls_cert_path: config.tls_cert_path,
        tls_key_path: config.tls_key_path,
        tls_ca_path: config.tls_ca_path,
        device_id_field: config.device_id_field,
        credentials,
    };

    ok(json!({ "config": dto }))
}

/// Update embedded broker configuration.
///
/// PUT /api/mqtt/broker-config
///
/// Updates listen address, port, and authentication/TLS settings.
/// Port must be in range 1024-65535. If TLS is enabled, certificates must
/// already be uploaded. When enabling authentication for the first time,
/// a system credential is auto-generated if none exists.
pub async fn update_broker_config_handler(
    #[cfg(feature = "embedded-broker")] State(_state): State<ServerState>,
    #[cfg(not(feature = "embedded-broker"))] State(_state): State<ServerState>,
    Json(req): Json<UpdateBrokerConfigRequest>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    // Load existing config (fall back to config.toml/defaults if never saved to redb)
    let config = if let Some(config_value) = store
        .load_embedded_broker_config()
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker config: {}", e)))?
    {
        serde_json::from_value::<EmbeddedBrokerConfig>(config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to parse broker config: {}", e)))?
    } else {
        // First time: use config.toml or defaults, then persist to redb
        let default_config = config::get_embedded_broker_config();
        let config_value = serde_json::to_value(&default_config)
            .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;
        store
            .save_embedded_broker_config(&config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;
        default_config
    };

    let mut config = config;
    #[cfg(feature = "embedded-broker")]
    let old_config = config.clone();

    // Update fields if provided
    if let Some(listen) = req.listen {
        // Validate listen address is a valid IP (or "0.0.0.0" / "::")
        if listen.parse::<std::net::IpAddr>().is_err() {
            return Err(ErrorResponse::bad_request(
                format!("Invalid listen address: '{}'. Must be a valid IP address (e.g. 0.0.0.0, 127.0.0.1, ::)", listen)
            ));
        }
        config.listen = listen;
    }
    if let Some(port) = req.port {
        if !(1024..=65535).contains(&port) {
            return Err(ErrorResponse::bad_request(
                "Port must be between 1024 and 65535".to_string(),
            ));
        }
        config.port = port;
    }
    if let Some(auth_enabled) = req.auth_enabled {
        config.auth_enabled = auth_enabled;
    }
    if let Some(tls_enabled) = req.tls_enabled {
        // Block enabling TLS when certificates are not configured
        if tls_enabled && config.tls_cert_path.is_none() {
            return Err(ErrorResponse::bad_request(
                "Cannot enable TLS: no certificates configured. Upload or generate certificates first.".to_string()
            ));
        }
        config.tls_enabled = tls_enabled;
    }
    if let Some(field) = req.device_id_field.clone() {
        if !field.trim().is_empty() {
            config.device_id_field = Some(field.trim().to_string());
        } else {
            config.device_id_field = None;
        }
    }

    // All changes require a broker restart.
    // When auth_enabled changes, external_auth is only set when enabled,
    // so the broker must restart to add/remove the auth handler.
    #[cfg(feature = "embedded-broker")]
    let needs_restart = old_config.listen != config.listen
        || old_config.port != config.port
        || old_config.tls_enabled != config.tls_enabled;

    // Auto-generate system credential if enabling auth for the first time
    if config.auth_enabled {
        let system_creds = store.get_system_mqtt_credential().map_err(|e| {
            ErrorResponse::internal(format!("Failed to check system credential: {}", e))
        })?;

        if system_creds.is_none() {
            // Generate random password
            let system_password = generate_random_password(16);
            store
                .set_system_mqtt_credential(&system_password)
                .map_err(|e| {
                    ErrorResponse::internal(format!("Failed to create system credential: {}", e))
                })?;

            tracing::info!("Auto-generated system credential for embedded broker");
        }
    }

    // Save to redb
    let auth_enabled = config.auth_enabled;
    let config_value = serde_json::to_value(config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;

    store
        .save_embedded_broker_config(&config_value)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;

    tracing::info!(
        listen = %config_value["listen"],
        port = %config_value["port"],
        auth_enabled = %config_value["auth_enabled"],
        tls_enabled = %config_value["tls_enabled"],
        "Updated embedded broker configuration"
    );

    // When port/listen/tls changed, the broker must restart.
    // rmqtt restart is fast (abort old task + start new task).
    // auth_enabled changes take effect dynamically via the shared AtomicBool
    // — no broker restart needed, just update the flag.
    #[cfg(feature = "embedded-broker")]
    if needs_restart {
        tracing::info!("Broker restart required (port/TLS changed). Applying restart...");
        match _state.restart_embedded_broker().await {
            Ok(()) => {
                return ok(json!({
                    "message": "Broker configuration updated and applied successfully",
                    "config": config_value,
                }));
            }
            Err(e) => {
                tracing::warn!("Broker restart failed: {}", e);
                return ok(json!({
                    "message": "Configuration saved but broker restart failed. Try restarting the server.",
                    "config": config_value,
                    "restart_required": true,
                }));
            }
        }
    }

    // Update the dynamic auth_enabled flag (takes effect immediately on new connections)
    #[cfg(feature = "embedded-broker")]
    if let Some(broker) = _state.embedded_broker() {
        broker.set_auth_enabled(auth_enabled);
    }

    ok(json!({
        "message": "Broker configuration updated and applied successfully",
        "config": config_value,
    }))
}

/// Add a new user credential.
///
/// POST /api/mqtt/broker-config/credentials
///
/// Validates username (1-64 chars, cannot start with `__neomind`),
/// password (min 4 chars), and maximum credential count (100).
/// Passwords are hashed with bcrypt before storage.
pub async fn add_credential_handler(
    #[cfg(feature = "embedded-broker")] State(state): State<crate::server::types::ServerState>,
    #[cfg(not(feature = "embedded-broker"))] State(_state): State<
        crate::server::types::ServerState,
    >,
    Json(req): Json<AddCredentialRequest>,
) -> HandlerResult<serde_json::Value> {
    // Validate username
    if req.username.is_empty() || req.username.len() > 64 {
        return Err(ErrorResponse::bad_request(
            "Username must be 1-64 characters".to_string(),
        ));
    }
    if req.username.starts_with("__neomind") {
        return Err(ErrorResponse::bad_request(
            "Username cannot start with '__neomind' (reserved for system use)".to_string(),
        ));
    }

    // Validate password
    if req.password.len() < 4 {
        return Err(ErrorResponse::bad_request(
            "Password must be at least 4 characters".to_string(),
        ));
    }

    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    // Hash password first so we can do the uniqueness check + insert + count
    // cap atomically in a single transaction (see `try_add_mqtt_credential`).
    // The previous implementation used separate `list_mqtt_credentials` and
    // `add_mqtt_credential` transactions, leaving a TOCTOU window where two
    // concurrent same-username requests could both pass the uniqueness check
    // and silently overwrite each other's bcrypt hash via redb upsert.
    let password_hash = bcrypt::hash(&req.password, 12)
        .map_err(|e| ErrorResponse::internal(format!("Failed to hash password: {}", e)))?;

    let (inserted, count_after) = store
        .try_add_mqtt_credential(&req.username, &password_hash)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save credential: {}", e)))?;

    if !inserted {
        return Err(ErrorResponse::bad_request(format!(
            "Username '{}' already exists",
            req.username
        )));
    }

    if count_after > 100 {
        // We just exceeded the cap — roll back this insert to honor the limit.
        // (The atomic path doesn't pre-check the count because doing so would
        // reintroduce a race; instead we add then trim if over.)
        let _ = store.delete_mqtt_credential(&req.username);
        return Err(ErrorResponse::bad_request(
            "Maximum credential limit (100) reached".to_string(),
        ));
    }

    // Refresh in-memory credential cache
    #[cfg(feature = "embedded-broker")]
    {
        if let Ok(store) = config::open_settings_store() {
            match crate::server::types::CredentialCache::load_from_store(&store) {
                Ok(cache) => {
                    *state.credential_cache.write().unwrap() = cache;
                }
                Err(e) => tracing::error!("Failed to refresh credential cache: {}", e),
            }
        }
    }

    tracing::info!(username = %req.username, "Added MQTT credential");

    ok(json!({
        "message": "Credential added successfully",
        "username": req.username,
    }))
}

/// Delete a user credential.
///
/// POST /api/mqtt/broker-config/credentials/delete
///
/// Deletes a credential by username. Returns 404 if not found.
/// System credentials (starting with `__neomind`) cannot be deleted via this API.
pub async fn delete_credential_handler(
    #[cfg(feature = "embedded-broker")] State(state): State<crate::server::types::ServerState>,
    #[cfg(not(feature = "embedded-broker"))] State(_state): State<
        crate::server::types::ServerState,
    >,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let username = req
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'username' field".to_string()))?;

    if username.starts_with("__neomind") {
        return Err(ErrorResponse::bad_request(
            "Cannot delete system credentials (starting with '__neomind')".to_string(),
        ));
    }

    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let deleted = store
        .delete_mqtt_credential(username)
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete credential: {}", e)))?;

    if !deleted {
        return Err(ErrorResponse::not_found(format!(
            "Credential not found: {}",
            username
        )));
    }

    // Refresh in-memory credential cache
    #[cfg(feature = "embedded-broker")]
    {
        if let Ok(store) = config::open_settings_store() {
            match crate::server::types::CredentialCache::load_from_store(&store) {
                Ok(cache) => {
                    *state.credential_cache.write().unwrap() = cache;
                }
                Err(e) => tracing::error!("Failed to refresh credential cache: {}", e),
            }
        }
    }

    tracing::info!(username = %username, "Deleted MQTT credential");

    ok(json!({
        "message": "Credential deleted successfully",
        "username": username,
    }))
}

/// Upload TLS certificates.
///
/// PUT /api/mqtt/broker-config/tls
///
/// Validates PEM format and writes certificates to `data/tls/`.
/// Updates broker configuration with certificate paths.
pub async fn upload_tls_handler(
    Json(req): Json<UploadTlsRequest>,
) -> HandlerResult<serde_json::Value> {
    // Validate PEM format (basic check for PEM headers)
    validate_pem(&req.cert_pem, "certificate")?;
    validate_pem(&req.key_pem, "private key")?;
    if let Some(ref ca_pem) = req.ca_pem {
        validate_pem(ca_pem, "CA certificate")?;
    }

    // Ensure TLS directory exists
    let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let tls_dir = std::path::Path::new(&data_dir).join("tls");
    std::fs::create_dir_all(&tls_dir)
        .map_err(|e| ErrorResponse::internal(format!("Failed to create TLS directory: {}", e)))?;

    let cert_path = tls_dir.join("mqtt-server.crt");
    let key_path = tls_dir.join("mqtt-server.key");
    let ca_path = tls_dir.join("mqtt-ca.crt");

    // Write certificate
    std::fs::write(&cert_path, &req.cert_pem)
        .map_err(|e| ErrorResponse::internal(format!("Failed to write certificate: {}", e)))?;

    // Write private key
    std::fs::write(&key_path, &req.key_pem)
        .map_err(|e| ErrorResponse::internal(format!("Failed to write private key: {}", e)))?;

    // Restrict private key file permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| ErrorResponse::internal(format!("Failed to set key file permissions: {}", e)),
        )?;
    }

    // Write CA certificate if provided
    if let Some(ca_pem) = &req.ca_pem {
        std::fs::write(&ca_path, ca_pem).map_err(|e| {
            ErrorResponse::internal(format!("Failed to write CA certificate: {}", e))
        })?;
    }

    // Update broker config
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let mut config = if let Some(config_value) = store
        .load_embedded_broker_config()
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker config: {}", e)))?
    {
        serde_json::from_value::<EmbeddedBrokerConfig>(config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to parse broker config: {}", e)))?
    } else {
        let default_config = config::get_embedded_broker_config();
        let config_value = serde_json::to_value(&default_config)
            .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;
        store
            .save_embedded_broker_config(&config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;
        default_config
    };

    config.tls_cert_path = Some(cert_path.to_string_lossy().to_string());
    config.tls_key_path = Some(key_path.to_string_lossy().to_string());
    let ca_path_option = if req.ca_pem.is_some() {
        Some(ca_path.to_string_lossy().to_string())
    } else {
        None
    };

    config.tls_ca_path = ca_path_option.clone();

    let config_value = serde_json::to_value(config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;

    store
        .save_embedded_broker_config(&config_value)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;

    tracing::info!(
        cert_path = %cert_path.display(),
        key_path = %key_path.display(),
        ca_path = %ca_path.display(),
        "Uploaded TLS certificates for embedded broker"
    );

    ok(json!({
        "message": "TLS certificates uploaded successfully",
        "cert_path": cert_path.to_string_lossy(),
        "key_path": key_path.to_string_lossy(),
        "ca_path": ca_path_option,
    }))
}

/// Validate PEM format for certificates and private keys.
///
/// Checks PEM headers and validates that the content matches the expected type:
/// - "certificate" expects `-----BEGIN CERTIFICATE-----`
/// - "private key" expects `-----BEGIN PRIVATE KEY-----` or `-----BEGIN RSA PRIVATE KEY-----`
fn validate_pem(pem: &str, label: &str) -> Result<(), ErrorResponse> {
    let trimmed = pem.trim();
    if trimmed.is_empty() {
        return Err(ErrorResponse::bad_request(format!(
            "{} PEM cannot be empty",
            label
        )));
    }

    // Check for PEM headers
    if !trimmed.contains("-----BEGIN") || !trimmed.contains("-----END") {
        return Err(ErrorResponse::bad_request(format!(
            "{} must be in PEM format (-----BEGIN ...-----END ...-----)",
            label
        )));
    }

    // Validate PEM type matches expected content
    let lower = trimmed.to_lowercase();
    match label {
        "certificate" | "CA certificate" => {
            if !lower.contains("-----begin certificate-----") {
                return Err(ErrorResponse::bad_request(format!(
                    "{} must contain a valid certificate block (-----BEGIN CERTIFICATE-----)",
                    label
                )));
            }
        }
        "private key" => {
            if !lower.contains("-----begin private key-----")
                && !lower.contains("-----begin rsa private key-----")
                && !lower.contains("-----begin ec private key-----")
            {
                return Err(ErrorResponse::bad_request(format!(
                    "{} must contain a valid private key block (-----BEGIN PRIVATE KEY-----, RSA PRIVATE KEY, or EC PRIVATE KEY)",
                    label
                )));
            }
        }
        _ => {}
    }

    Ok(())
}

/// Generate a random password for system credentials.
fn generate_random_password(length: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Auto-generate self-signed TLS certificates.
///
/// POST /api/mqtt/broker-config/tls/generate
///
/// Generates a CA certificate and a server certificate signed by it.
/// Writes PEM files to `data/tls/` and updates the broker configuration.
pub async fn generate_tls_handler() -> HandlerResult<serde_json::Value> {
    let paths = cert_gen::generate_self_signed_certs()
        .map_err(|e| ErrorResponse::internal(format!("Certificate generation failed: {}", e)))?;

    // Load broker config (same fallback pattern as update handler)
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let mut config = if let Some(config_value) = store
        .load_embedded_broker_config()
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker config: {}", e)))?
    {
        serde_json::from_value::<EmbeddedBrokerConfig>(config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to parse broker config: {}", e)))?
    } else {
        let default_config = config::get_embedded_broker_config();
        let config_value = serde_json::to_value(&default_config)
            .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;
        store
            .save_embedded_broker_config(&config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;
        default_config
    };

    config.tls_cert_path = Some(paths.server_cert_path.clone());
    config.tls_key_path = Some(paths.server_key_path.clone());
    config.tls_ca_path = Some(paths.ca_cert_path.clone());

    let config_value = serde_json::to_value(&config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to serialize config: {}", e)))?;

    store
        .save_embedded_broker_config(&config_value)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save broker config: {}", e)))?;

    tracing::info!(
        ca_path = %paths.ca_cert_path,
        cert_path = %paths.server_cert_path,
        "Generated self-signed TLS certificates for embedded broker"
    );

    ok(json!({
        "message": "Self-signed certificates generated successfully",
        "ca_path": paths.ca_cert_path,
        "cert_path": paths.server_cert_path,
        "key_path": paths.server_key_path,
    }))
}

/// Download the CA certificate file.
///
/// GET /api/mqtt/broker-config/tls/ca-cert
///
/// Returns the CA certificate PEM file as a downloadable attachment.
pub async fn download_ca_cert_handler() -> Result<axum::response::Response, ErrorResponse> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let config = if let Some(config_value) = store
        .load_embedded_broker_config()
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker config: {}", e)))?
    {
        serde_json::from_value::<EmbeddedBrokerConfig>(config_value)
            .map_err(|e| ErrorResponse::internal(format!("Failed to parse broker config: {}", e)))?
    } else {
        config::get_embedded_broker_config()
    };

    let ca_path = config
        .tls_ca_path
        .ok_or_else(|| ErrorResponse::not_found("No CA certificate configured".to_string()))?;

    let ca_pem = std::fs::read_to_string(&ca_path)
        .map_err(|e| ErrorResponse::not_found(format!("CA certificate file not found: {}", e)))?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-pem-file".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"mqtt-ca.crt\"".to_string(),
            ),
        ],
        ca_pem,
    )
        .into_response())
}
