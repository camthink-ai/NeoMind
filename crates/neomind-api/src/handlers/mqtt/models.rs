//! MQTT DTOs and request structures.

use serde::{Deserialize, Serialize};

/// DTO for MQTT settings response.
#[derive(Debug, Serialize)]
pub struct MqttSettingsDto {
    pub broker_url: String,
    pub client_id: Option<String>,
    pub username: Option<String>,
}

impl From<&neomind_storage::MqttSettings> for MqttSettingsDto {
    fn from(s: &neomind_storage::MqttSettings) -> Self {
        // Build broker URL from listen address and port
        let broker_url = format!("mqtt://{}:{}", s.listen, s.port);
        Self {
            broker_url,
            client_id: None,
            username: None,
        }
    }
}

/// Request body for updating MQTT settings.
#[derive(Debug, Deserialize)]
pub struct MqttSettingsRequest {
    #[serde(default)]
    pub broker_url: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
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
    pub tls_enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_brokers: Vec<ExternalBrokerConnectionDto>,
    pub last_error: Option<String>,
}

/// DTO for external broker connection status in MQTT status.
#[derive(Debug, Serialize)]
pub struct ExternalBrokerConnectionDto {
    pub id: String,
    pub name: String,
    #[serde(rename = "broker")]
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub connected: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Client ID prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_prefix: Option<String>,
}

/// DTO for MQTT subscription.
#[derive(Debug, Serialize)]
pub struct MqttSubscriptionDto {
    pub topic: String,
    pub qos: u8,
    pub device_id: Option<String>,
}

/// Request body for subscribing to a custom topic.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct MqttSubscribeRequest {
    pub topic: String,
    #[serde(default = "default_qos")]
    pub qos: u8,
}

fn default_qos() -> u8 {
    1
}

/// Request body for unsubscribing from a custom topic.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct MqttUnsubscribeRequest {
    pub topic: String,
}
