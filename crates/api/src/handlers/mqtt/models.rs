//! MQTT DTOs and request structures.

use serde::{Deserialize, Serialize};

/// DTO for MQTT settings response.
#[derive(Debug, Serialize)]
pub struct MqttSettingsDto {
    pub listen: String,
    pub port: u16,
    pub discovery_prefix: String,
    pub auto_discovery: bool,
    pub updated_at: Option<i64>,
}

impl From<&edge_ai_storage::MqttSettings> for MqttSettingsDto {
    fn from(s: &edge_ai_storage::MqttSettings) -> Self {
        Self {
            listen: s.listen.clone(),
            port: s.port,
            discovery_prefix: s.discovery_prefix.clone(),
            auto_discovery: s.auto_discovery,
            updated_at: Some(s.updated_at),
        }
    }
}

/// Request body for updating MQTT settings.
#[derive(Debug, Deserialize)]
pub struct MqttSettingsRequest {
    #[serde(default)]
    pub listen: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub discovery_prefix: Option<String>,
    #[serde(default)]
    pub auto_discovery: Option<bool>,
}

/// DTO for MQTT connection status.
#[derive(Debug, Serialize)]
pub struct MqttStatusDto {
    pub connected: bool,
    pub listen_address: String,
    pub subscriptions_count: usize,
    pub devices_count: usize,
    pub clients_count: usize,
    /// Server IP address (actual accessible IP, not 0.0.0.0)
    pub server_ip: String,
    pub listen_port: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_brokers: Vec<ExternalBrokerConnectionDto>,
    pub last_error: Option<String>,
}

/// DTO for external broker connection status in MQTT status.
#[derive(Debug, Serialize)]
pub struct ExternalBrokerConnectionDto {
    pub id: String,
    pub name: String,
    pub broker: String,
    pub port: u16,
    pub tls: bool,
    pub connected: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// DTO for MQTT subscription.
#[derive(Debug, Serialize)]
pub struct MqttSubscriptionDto {
    pub topic: String,
    pub qos: u8,
    pub device_id: Option<String>,
}

/// Request body for subscribing to a topic.
#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub topic: String,
    #[serde(default = "default_qos")]
    pub qos: u8,
}

fn default_qos() -> u8 { 1 }

/// Get the actual local IP address of the server.
/// Returns the first non-loopback IPv4 address, or localhost as fallback.
pub fn get_server_ip() -> String {
    use std::net::IpAddr;

    // Try to get local IP by creating a socket
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if let IpAddr::V4(ipv4) = ip {
                    // Check if it's a private network address
                    let octets = ipv4.octets();
                    if (octets[0] == 192 && octets[1] == 168) ||
                       (octets[0] == 10) ||
                       (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31) {
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
                    // Prefer LAN addresses
                    if (octets[0] == 192 && octets[1] == 168) ||
                       (octets[0] == 10) ||
                       (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31) {
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
