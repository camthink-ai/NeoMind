//! MQTT connection status handlers.

use axum::extract::State;
use serde_json::json;

use super::models::ExternalBrokerConnectionDto;
use super::models::MqttStatusDto;
use crate::handlers::{
    common::{get_server_host, ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Get MQTT connection status.
///
/// GET /api/mqtt/status
#[utoipa::path(
    get,
    path = "/api/mqtt/status",
    tag = "mqtt",
    responses(
        (status = 200, description = "Embedded broker status and counters"),
    )
)]
pub async fn get_mqtt_status_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_devices::adapter::ConnectionStatus;

    // Get connection status from the MQTT adapter
    let connected = if let Some(adapter) = state.devices.service.get_adapter("internal-mqtt").await
    {
        matches!(adapter.connection_status(), ConnectionStatus::Connected)
    } else {
        false
    };

    // Set last error based on connection state
    let last_error = if !connected {
        Some("Disconnected".to_string())
    } else {
        None
    };

    // Get MQTT settings — prefer embedded broker config for port/listen
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let broker_config = crate::config::get_embedded_broker_config();
    let listen_address = broker_config.listen.clone();
    let listen_port = broker_config.port;
    let tls_enabled = broker_config.tls_enabled;

    // Get the actual server IP for embedded broker
    let server_ip = get_server_host();

    // Count devices using DeviceService
    let configs = state.devices.service.list_devices();
    let devices_count = configs.len();
    let subscriptions_count = devices_count;

    // For embedded broker, clients_count is the number of connected devices
    let clients_count = devices_count;

    // Load external brokers
    let external_brokers: Vec<ExternalBrokerConnectionDto> = match store.load_all_external_brokers()
    {
        Ok(brokers) => brokers
            .into_iter()
            .map(|b| ExternalBrokerConnectionDto {
                id: b.id,
                name: b.name,
                host: b.broker,
                port: b.port,
                tls: b.tls,
                connected: b.connected,
                enabled: b.enabled,
                last_error: b.last_error,
                client_id_prefix: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    ok(json!({
        "status": MqttStatusDto {
            connected,
            listen_address,
            subscriptions_count,
            devices_count,
            clients_count,
            server_ip,
            listen_port,
            tls_enabled,
            external_brokers,
            last_error,
        },
    }))
}
