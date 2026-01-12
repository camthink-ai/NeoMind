//! External MQTT broker management handlers.

use axum::{extract::{Path, State}, Json};
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

use edge_ai_storage::{ExternalBroker, SecurityLevel, SecurityWarning};
use edge_ai_devices::{MqttManagerConfig, MultiBrokerManager};

use crate::handlers::common::{HandlerResult, ok};
use crate::models::ErrorResponse;
use crate::config;
use crate::server::types::ServerState;

/// DTO for external broker response.
#[derive(Debug, serde::Serialize)]
struct ExternalBrokerDto {
    id: String,
    name: String,
    broker: String,
    port: u16,
    tls: bool,
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    /// Indicates if certificates are configured
    #[serde(default)]
    has_certs: bool,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    updated_at: i64,
}

impl From<ExternalBroker> for ExternalBrokerDto {
    fn from(b: ExternalBroker) -> Self {
        Self {
            id: b.id,
            name: b.name,
            broker: b.broker,
            port: b.port,
            tls: b.tls,
            username: b.username,
            // Mask password in response
            password: if b.password.is_some() { Some("*****".to_string()) } else { None },
            // Check if any certificates are configured
            has_certs: b.ca_cert.is_some() || b.client_cert.is_some() || b.client_key.is_some(),
            enabled: b.enabled,
            connected: Some(b.connected),
            last_error: b.last_error,
            updated_at: b.updated_at,
        }
    }
}

/// Request body for creating/updating an external broker.
#[derive(Debug, serde::Deserialize)]
pub struct ExternalBrokerRequest {
    pub id: Option<String>,
    pub name: String,
    pub broker: String,
    #[serde(default = "default_external_broker_port")]
    pub port: u16,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// CA certificate for TLS verification (PEM format)
    #[serde(default)]
    pub ca_cert: Option<String>,
    /// Client certificate for mTLS (PEM format)
    #[serde(default)]
    pub client_cert: Option<String>,
    /// Client private key for mTLS (PEM format)
    #[serde(default)]
    pub client_key: Option<String>,
    #[serde(default = "default_external_broker_enabled")]
    pub enabled: bool,
}

fn default_external_broker_port() -> u16 { 1883 }
fn default_external_broker_enabled() -> bool { true }

/// List all external brokers.
///
/// GET /api/brokers
pub async fn list_brokers_handler() -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let brokers = store.load_all_external_brokers()
        .map_err(|e| {
            tracing::warn!(category = "mqtt", error = %e, "Failed to load external brokers");
            ErrorResponse::internal(format!("Failed to load brokers: {}", e))
        })?;

    let dtos: Vec<ExternalBrokerDto> = brokers.into_iter().map(ExternalBrokerDto::from).collect();
    ok(json!({
        "brokers": dtos,
        "count": dtos.len(),
    }))
}

/// Get a specific external broker.
///
/// GET /api/brokers/:id
pub async fn get_broker_handler(
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let broker = store.load_external_broker(&id)
        .map_err(|e| {
            tracing::warn!(category = "mqtt", error = %e, "Failed to load broker");
            ErrorResponse::internal(format!("Failed to load broker: {}", e))
        })?
        .ok_or_else(|| ErrorResponse::not_found(format!("Broker not found: {}", id)))?;

    ok(json!({
        "broker": ExternalBrokerDto::from(broker),
    }))
}

/// Create a new external broker.
///
/// POST /api/brokers
pub async fn create_broker_handler(
    State(state): State<ServerState>,
    Json(req): Json<ExternalBrokerRequest>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    // Generate ID if not provided
    let id = req.id.unwrap_or_else(|| ExternalBroker::generate_id());

    // Check if broker already exists
    if store.load_external_broker(&id).is_ok_and(|b| b.is_some()) {
        return Err(ErrorResponse::bad_request(format!("Broker already exists: {}", id)));
    }

    let mut broker = ExternalBroker::new(id.clone(), req.name.clone(), req.broker.clone(), req.port);
    broker.username = req.username.clone();
    broker.password = req.password.clone();
    broker.tls = req.tls;
    broker.ca_cert = req.ca_cert.clone();
    broker.client_cert = req.client_cert.clone();
    broker.client_key = req.client_key.clone();
    broker.enabled = req.enabled;

    // Run security validation
    let warnings = broker.validate_security();
    for warning in &warnings {
        match warning.level {
            SecurityLevel::High | SecurityLevel::Critical => {
                tracing::warn!(
                    category = "mqtt",
                    broker = %broker.name,
                    url = %broker.broker_url(),
                    message = %warning.message,
                    recommendation = %warning.recommendation,
                    "Security warning for broker creation"
                );
            }
            SecurityLevel::Medium => {
                tracing::info!(
                    category = "mqtt",
                    broker = %broker.name,
                    message = %warning.message,
                    recommendation = %warning.recommendation,
                    "Security advisory for broker"
                );
            }
            SecurityLevel::Low => {
                tracing::debug!(
                    category = "mqtt",
                    broker = %broker.name,
                    message = %warning.message,
                    "Security info for broker"
                );
            }
        }
    }

    // Save broker configuration
    store.save_external_broker(&broker)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save broker: {}", e)))?;

    tracing::info!(category = "mqtt", name = %broker.name, url = %broker.broker_url(), "Created external broker");

    // If enabled, create MQTT connection through MultiBrokerManager
    let mut connected = false;
    let mut connection_error: Option<String> = None;
    if broker.enabled {
        // Create MQTT manager config
        let mut mqtt_config = MqttManagerConfig::new(&broker.broker)
            .with_port(broker.port)
            .with_client_id(format!("neotalk-external-{}", &id));

        // Add authentication if configured
        if let (Some(username), Some(password)) = (&broker.username, &broker.password) {
            mqtt_config = mqtt_config.with_auth(username, password);
        }

        // Add to multi-broker manager (this creates the MQTT connection)
        match state.multi_broker_manager.add_broker(&id, mqtt_config).await {
            Ok(()) => {
                // Start the broker connection
                match state.multi_broker_manager.start_broker(&id).await {
                    Ok(()) => {
                        connected = true;
                        tracing::info!(category = "mqtt", broker_id = %id, "MQTT connection established");
                    }
                    Err(e) => {
                        connection_error = Some(format!("MQTT connection failed: {}", e));
                        tracing::warn!(category = "mqtt", broker_id = %id, error = %e, "Failed to start MQTT connection");
                    }
                }
            }
            Err(e) => {
                connection_error = Some(format!("Failed to add broker: {}", e));
                tracing::warn!(category = "mqtt", broker_id = %id, error = %e, "Failed to add broker to manager");
            }
        }

        // Update broker connection status
        if let Err(e) = update_broker_connection_status(&store, &id, connected, connection_error.clone()).await {
            tracing::warn!("Failed to update broker status: {}", e);
        }

        // Reload broker to get updated status
        if let Ok(Some(updated_broker)) = store.load_external_broker(&id) {
            return ok(json!({
                "broker": ExternalBrokerDto::from(updated_broker),
                "message": if connected {
                    "External broker created and MQTT connection established successfully"
                } else {
                    "External broker created but MQTT connection failed"
                },
            }));
        }
    }

    // Add security warnings to response if any
    let warnings_json: Vec<serde_json::Value> = warnings.iter().map(|w| {
        json!({
            "level": format!("{:?}", w.level),
            "message": w.message,
            "recommendation": w.recommendation,
        })
    }).collect();

    let mut response = json!({
        "broker": ExternalBrokerDto::from(broker),
        "message": "External broker created successfully",
    });

    if !warnings_json.is_empty() {
        response["security_warnings"] = json!(warnings_json);
    }

    ok(response)
}

/// Update an existing external broker.
///
/// PUT /api/brokers/:id
pub async fn update_broker_handler(
    Path(id): Path<String>,
    Json(req): Json<ExternalBrokerRequest>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let mut broker = store.load_external_broker(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Broker not found: {}", id)))?;

    // Update fields
    broker.name = req.name;
    broker.broker = req.broker;
    broker.port = req.port;
    broker.tls = req.tls;
    broker.username = req.username;
    // Only update password if provided (non-empty)
    if let Some(pwd) = req.password {
        if !pwd.is_empty() {
            broker.password = Some(pwd);
        }
    }
    // Update certificates
    broker.ca_cert = req.ca_cert;
    broker.client_cert = req.client_cert;
    broker.client_key = req.client_key;
    broker.enabled = req.enabled;
    broker.touch();

    store.save_external_broker(&broker)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save broker: {}", e)))?;

    tracing::info!(category = "mqtt", name = %broker.name, url = %broker.broker_url(), "Updated external broker");

    // If enabled, automatically attempt to connect
    let mut connected = false;
    let mut connection_error: Option<String> = None;
    if broker.enabled {
        let addr = format!("{}:{}", broker.broker, broker.port);
        match timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => {
                connected = true;
                tracing::info!(category = "mqtt", broker_id = %id, "Auto-connected to broker after update");
            }
            Ok(Err(e)) => {
                connection_error = Some(format!("Connection failed: {}", e));
                tracing::warn!(category = "mqtt", broker_id = %id, error = %e, "Auto-connect failed after update");
            }
            Err(_) => {
                connection_error = Some("Connection timeout after 5 seconds".to_string());
                tracing::warn!(category = "mqtt", broker_id = %id, "Auto-connect timeout after update");
            }
        }

        // Update broker connection status
        if let Err(e) = update_broker_connection_status(&store, &id, connected, connection_error.clone()).await {
            tracing::warn!("Failed to update broker status: {}", e);
        }

        // Reload broker to get updated status
        if let Ok(Some(updated_broker)) = store.load_external_broker(&id) {
            return ok(json!({
                "broker": ExternalBrokerDto::from(updated_broker),
                "message": if connected {
                    "External broker updated and connected successfully"
                } else {
                    "External broker updated but connection failed"
                },
            }));
        }
    }

    ok(json!({
        "broker": ExternalBrokerDto::from(broker),
        "message": "External broker updated successfully",
    }))
}

/// Delete an external broker.
///
/// DELETE /api/brokers/:id
pub async fn delete_broker_handler(
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let existed = store.delete_external_broker(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete broker: {}", e)))?;

    if existed {
        tracing::info!(category = "mqtt", broker_id = %id, "Deleted external broker");
        ok(json!({
            "message": "External broker deleted successfully",
        }))
    } else {
        Err(ErrorResponse::not_found(format!("Broker not found: {}", id)))
    }
}

/// Test connection to an external broker.
///
/// POST /api/brokers/:id/test
pub async fn test_broker_handler(
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let broker = store.load_external_broker(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load broker: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Broker not found: {}", id)))?;

    let broker_url = broker.broker_url();
    let broker_host = broker.broker.clone();
    let broker_port = broker.port;

    // Basic validation: check if host and port are valid
    if broker_host.is_empty() {
        return ok(json!({
            "success": false,
            "message": "Invalid broker configuration: empty host",
            "broker_url": broker_url,
        }));
    }

    if broker_port == 0 {
        return ok(json!({
            "success": false,
            "message": "Invalid broker configuration: port cannot be 0",
            "broker_url": broker_url,
        }));
    }

    // Attempt actual TCP connection test
    let addr = format!("{}:{}", broker_host, broker_port);
    tracing::info!(category = "mqtt", broker_id = %id, addr = %addr, "Testing broker connection");

    // Use a timeout for the connection attempt
    match timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
        Ok(Ok(stream)) => {
            // Connection successful
            let local_addr = stream.local_addr().ok().map(|a| a.to_string());
            let peer_addr = stream.peer_addr().ok().map(|a| a.to_string());

            // Update broker status to connected
            if let Err(e) = update_broker_connection_status(&store, &id, true, None).await {
                tracing::warn!("Failed to update broker status: {}", e);
            }

            // Reload broker to get updated status
            let broker_dto = match store.load_external_broker(&id) {
                Ok(Some(updated_broker)) => {
                    tracing::info!(category = "mqtt", broker_id = %id, connected = updated_broker.connected, "Reloaded broker after update");
                    Some(ExternalBrokerDto::from(updated_broker))
                },
                Ok(None) => {
                    tracing::warn!(category = "mqtt", broker_id = %id, "Broker not found after update");
                    None
                },
                Err(e) => {
                    tracing::warn!(category = "mqtt", broker_id = %id, error = %e, "Failed to reload broker after status update");
                    None
                }
            };

            ok(json!({
                "success": true,
                "message": "Successfully connected to broker",
                "broker_url": broker_url,
                "host": broker_host,
                "port": broker_port,
                "tls": broker.tls,
                "local_addr": local_addr,
                "peer_addr": peer_addr,
                "validation_only": false,
                "broker": broker_dto,
            }))
        }
        Ok(Err(e)) => {
            // Connection failed
            let error_msg = format!("Connection failed: {}", e);

            // Update broker status to disconnected with error
            if let Err(err) = update_broker_connection_status(&store, &id, false, Some(error_msg.clone())).await {
                tracing::warn!("Failed to update broker status: {}", err);
            }

            // Reload broker to get updated status
            let broker_dto = match store.load_external_broker(&id) {
                Ok(Some(updated_broker)) => Some(ExternalBrokerDto::from(updated_broker)),
                Ok(None) => None,
                Err(err) => {
                    tracing::warn!("Failed to reload broker after status update: {}", err);
                    None
                }
            };

            ok(json!({
                "success": false,
                "message": error_msg,
                "broker_url": broker_url,
                "host": broker_host,
                "port": broker_port,
                "tls": broker.tls,
                "validation_only": false,
                "broker": broker_dto,
            }))
        }
        Err(_) => {
            // Timeout
            let error_msg = format!("Connection timeout after 5 seconds");

            // Update broker status to disconnected with error
            if let Err(err) = update_broker_connection_status(&store, &id, false, Some(error_msg.clone())).await {
                tracing::warn!("Failed to update broker status: {}", err);
            }

            // Reload broker to get updated status
            let broker_dto = match store.load_external_broker(&id) {
                Ok(Some(updated_broker)) => Some(ExternalBrokerDto::from(updated_broker)),
                Ok(None) => None,
                Err(err) => {
                    tracing::warn!("Failed to reload broker after status update: {}", err);
                    None
                }
            };

            ok(json!({
                "success": false,
                "message": error_msg,
                "broker_url": broker_url,
                "host": broker_host,
                "port": broker_port,
                "tls": broker.tls,
                "validation_only": false,
                "broker": broker_dto,
            }))
        }
    }
}

/// Update broker connection status in storage
async fn update_broker_connection_status(
    store: &edge_ai_storage::SettingsStore,
    id: &str,
    connected: bool,
    error: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut broker = store.load_external_broker(id)?
        .ok_or_else(|| anyhow::anyhow!("Broker not found"))?;

    tracing::info!(category = "mqtt", broker_id = %id, connected, "Updating broker connection status");

    broker.connected = connected;
    broker.last_error = error;
    broker.touch();

    store.save_external_broker(&broker)?;
    tracing::info!(category = "mqtt", broker_id = %id, connected = broker.connected, "Broker status saved");
    Ok(())
}
