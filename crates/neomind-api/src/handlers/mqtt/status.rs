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

    // Count devices truthfully:
    // - devices_count: total registered devices
    // - subscriptions_count: devices actually bound to an MQTT adapter
    // - clients_count: MQTT devices currently transport-connected (or within effective offline timeout)
    let configs = state.devices.service.list_devices();
    let devices_count = configs.len();
    let mut subscriptions_count = 0usize;
    let mut clients_count = 0usize;
    for cfg in &configs {
        let is_mqtt_device = cfg.adapter_type.eq_ignore_ascii_case("mqtt");
        if is_mqtt_device {
            subscriptions_count += 1;
            let status = state.devices.service.get_device_status(&cfg.device_id).await;
            let timeout = state
                .devices
                .service
                .effective_offline_timeout(&cfg.device_id);
            if status.transport_connected || status.is_connected_within(timeout) {
                clients_count += 1;
            }
        }
    }

    // Load external brokers and overlay live adapter connection state (matches /api/brokers)
    let external_brokers: Vec<ExternalBrokerConnectionDto> = match store.load_all_external_brokers()
    {
        Ok(brokers) => {
            let mut live_status = std::collections::HashMap::new();
            for b in &brokers {
                let aid = format!("external-{}", b.id);
                if let Some(adapter) = state.devices.service.get_adapter(&aid).await {
                    live_status.insert(
                        b.id.clone(),
                        matches!(adapter.connection_status(), ConnectionStatus::Connected),
                    );
                }
            }
            brokers
                .into_iter()
                .map(|b| {
                    let is_live = live_status.get(&b.id).copied().unwrap_or(false);
                    let last_error = if is_live { None } else { b.last_error };
                    ExternalBrokerConnectionDto {
                        id: b.id,
                        name: b.name,
                        host: b.broker,
                        port: b.port,
                        tls: b.tls,
                        connected: is_live,
                        enabled: b.enabled,
                        last_error,
                        client_id_prefix: None,
                    }
                })
                .collect()
        }
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
