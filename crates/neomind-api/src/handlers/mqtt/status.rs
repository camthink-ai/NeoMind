//! MQTT connection status handlers.

use axum::extract::State;
use serde_json::json;

use super::models::ExternalBrokerConnectionDto;
use super::models::MqttStatusDto;
use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Get the actual local IP address of the server.
fn get_server_ip() -> String {
    use std::net::IpAddr;

    // Try to get local IP by creating a socket
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if let IpAddr::V4(ipv4) = ip {
                    let octets = ipv4.octets();
                    if (octets[0] == 192 && octets[1] == 168)
                        || (octets[0] == 10)
                        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                    {
                        return ip.to_string();
                    }
                }
            }
        }
    }

    // Fallback: try to get from network interfaces
    if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
        for iface in interfaces {
            if !iface.is_loopback() {
                if let get_if_addrs::IfAddr::V4(iface_addr) = iface.addr {
                    let ip = iface_addr.ip;
                    let octets = ip.octets();
                    if (octets[0] == 192 && octets[1] == 168)
                        || (octets[0] == 10)
                        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                    {
                        return ip.to_string();
                    }
                }
            }
        }
    }

    // Last fallback: return hostname or localhost
    std::env::var("HOSTNAME").unwrap_or_else(|_| {
        hostname::get()
            .ok()
            .and_then(|n| n.into_string().ok())
            .unwrap_or_else(|| "localhost".to_string())
    })
}

/// Get MQTT connection status.
///
/// GET /api/mqtt/status
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

    // Get MQTT settings
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    let settings = store.get_mqtt_settings();
    let listen_address = settings.listen_address();
    let listen_port = settings.port;

    // Get the actual server IP for embedded broker
    let server_ip = get_server_ip();

    // Count devices using DeviceService
    let configs = state.devices.service.list_devices().await;
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
            external_brokers,
            last_error,
        },
    }))
}
