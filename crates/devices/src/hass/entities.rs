//! Home Assistant entity types and structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Home Assistant authentication methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HassAuth {
    /// Long-lived access token (recommended)
    #[serde(rename = "bearer_token")]
    BearerToken { token: String },
    /// API key (deprecated, use bearer token instead)
    #[serde(rename = "api_key")]
    ApiKey { key: String },
    /// Username and password (not recommended for production)
    #[serde(rename = "password")]
    UsernamePassword { username: String, password: String },
}

impl HassAuth {
    /// Create a bearer token auth (recommended).
    pub fn bearer_token(token: String) -> Self {
        Self::BearerToken { token }
    }

    /// Get the authorization header value.
    pub fn auth_header(&self) -> String {
        match self {
            Self::BearerToken { token } => format!("Bearer {}", token),
            Self::ApiKey { key } => format!("Bearer {}", key),
            Self::UsernamePassword { .. } => {
                // For username/password, we need to use basic auth
                // This is handled in the client
                "Basic".to_string()
            }
        }
    }

    /// Get basic auth credentials if applicable.
    pub fn basic_auth(&self) -> Option<(String, String)> {
        match self {
            Self::UsernamePassword { username, password } => {
                Some((username.clone(), password.clone()))
            }
            _ => None,
        }
    }
}

/// Home Assistant connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassConnectionConfig {
    /// Home Assistant URL (e.g., http://home-assistant:8123)
    pub url: String,

    /// Authentication method
    pub auth: HassAuth,

    /// Whether to verify SSL certificates
    #[serde(default = "default_verify_ssl")]
    pub verify_ssl: bool,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_verify_ssl() -> bool { true }
fn default_timeout() -> u64 { 30 }

impl HassConnectionConfig {
    /// Create a new connection config with bearer token.
    pub fn with_bearer_token(url: String, token: String) -> Self {
        Self {
            url,
            auth: HassAuth::bearer_token(token),
            verify_ssl: true,
            timeout: 30,
        }
    }

    /// Get the WebSocket URL for this connection.
    pub fn websocket_url(&self) -> String {
        let url = self.url.trim_end_matches('/');
        let url = url.replace("http://", "ws://").replace("https://", "wss://");
        format!("{}/api/websocket", url)
    }

    /// Get the API base URL.
    pub fn api_base(&self) -> String {
        format!("{}/api", self.url.trim_end_matches('/'))
    }
}

/// Home Assistant entity state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassEntityState {
    /// Entity ID (e.g., sensor.temperature_188)
    pub entity_id: String,

    /// Current state value
    pub state: String,

    /// Entity attributes
    pub attributes: HassEntityAttributes,

    /// Last changed timestamp
    pub last_changed: String,

    /// Last updated timestamp
    pub last_updated: String,

    /// Context ID
    pub context: Option<HassContext>,
}

/// Context for tracking entity changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassContext {
    pub id: String,
    pub user_id: Option<String>,
    pub parent_id: Option<String>,
}

/// Home Assistant entity attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassEntityAttributes {
    /// Friendly name for display
    #[serde(default)]
    pub friendly_name: String,

    /// Device class (if applicable)
    #[serde(rename = "device_class", default)]
    pub device_class: Option<String>,

    /// Unit of measurement
    #[serde(rename = "unit_of_measurement", default)]
    pub unit_of_measurement: Option<String>,

    /// State class (measurement, total, etc.)
    #[serde(rename = "state_class", default)]
    pub state_class: Option<String>,

    /// Associated device info
    #[serde(rename = "device", default)]
    pub device: Option<HassDeviceInfo>,

    /// Entity category (config, diagnostic)
    #[serde(rename = "entity_category", default)]
    pub entity_category: Option<String>,

    /// Whether entity is enabled
    #[serde(default = "default_true")]
    pub disabled: bool,

    /// Icon
    #[serde(default)]
    pub icon: Option<String>,

    /// All other attributes
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_true() -> bool { true }

/// Device information from Home Assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassDeviceInfo {
    /// Device identifiers
    #[serde(rename = "identifiers", default)]
    pub identifiers: Vec<serde_json::Value>,

    /// Device name
    #[serde(default)]
    pub name: Option<String>,

    /// Device model
    #[serde(default)]
    pub model: Option<String>,

    /// Device manufacturer
    #[serde(default)]
    pub manufacturer: Option<String>,

    /// SW version
    #[serde(rename = "sw_version", default)]
    pub sw_version: Option<String>,

    /// Connection info
    #[serde(rename = "connections", default)]
    pub connections: Vec<Vec<String>>,

    /// Configuration URL
    #[serde(rename = "configuration_url", default)]
    pub configuration_url: Option<String>,
}

/// Home Assistant entity domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HassDomain {
    Sensor,
    BinarySensor,
    Switch,
    Light,
    Cover,
    Climate,
    Camera,
    InputBoolean,
    InputNumber,
    InputText,
    Automation,
    Script,
    Scene,
    Group,
    Fan,
    Lock,
    MediaPlayer,
    Vacuum,
    WaterHeater,
    #[serde(other)]
    Unknown,
}

impl HassDomain {
    /// Parse domain from entity ID.
    pub fn from_entity_id(entity_id: &str) -> Self {
        if let Some(domain) = entity_id.split('.').next() {
            match domain {
                "sensor" => HassDomain::Sensor,
                "binary_sensor" => HassDomain::BinarySensor,
                "switch" => HassDomain::Switch,
                "light" => HassDomain::Light,
                "cover" => HassDomain::Cover,
                "climate" => HassDomain::Climate,
                "camera" => HassDomain::Camera,
                "input_boolean" => HassDomain::InputBoolean,
                "input_number" => HassDomain::InputNumber,
                "input_text" => HassDomain::InputText,
                "automation" => HassDomain::Automation,
                "script" => HassDomain::Script,
                "scene" => HassDomain::Scene,
                "group" => HassDomain::Group,
                "fan" => HassDomain::Fan,
                "lock" => HassDomain::Lock,
                "media_player" => HassDomain::MediaPlayer,
                "vacuum" => HassDomain::Vacuum,
                "water_heater" => HassDomain::WaterHeater,
                _ => HassDomain::Unknown,
            }
        } else {
            HassDomain::Unknown
        }
    }

    /// Get the domain as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            HassDomain::Sensor => "sensor",
            HassDomain::BinarySensor => "binary_sensor",
            HassDomain::Switch => "switch",
            HassDomain::Light => "light",
            HassDomain::Cover => "cover",
            HassDomain::Climate => "climate",
            HassDomain::Camera => "camera",
            HassDomain::InputBoolean => "input_boolean",
            HassDomain::InputNumber => "input_number",
            HassDomain::InputText => "input_text",
            HassDomain::Automation => "automation",
            HassDomain::Script => "script",
            HassDomain::Scene => "scene",
            HassDomain::Group => "group",
            HassDomain::Fan => "fan",
            HassDomain::Lock => "lock",
            HassDomain::MediaPlayer => "media_player",
            HassDomain::Vacuum => "vacuum",
            HassDomain::WaterHeater => "water_heater",
            HassDomain::Unknown => "unknown",
        }
    }
}

/// Device class for sensors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HassDeviceClass {
    // Sensor classes
    Temperature,
    Humidity,
    Pressure,
    Battery,
    Illuminance,
    SignalStrength,
    Power,
    Energy,
    Voltage,
    Current,
    Frequency,
    PowerFactor,
    ApparentPower,
    ReactivePower,
    Timestamp,
    Duration,
    Distance,
    Speed,
    Precipitation,
    Moisture,
    WifiSsid,
    IpAddress,
    MacAddress,
    CarbonDioxide,
    CarbonMonoxide,
    Voc,
    Pm25,
    Pm10,
    Ozone,
    Aqi,
    Gas,
    #[serde(other)]
    Unknown,
}

/// Service call request for Home Assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassServiceCall {
    /// Domain (e.g., "homeassistant", "light", "switch")
    pub domain: String,

    /// Service name (e.g., "turn_on", "turn_off")
    pub service: String,

    /// Service data (entity_id and parameters)
    #[serde(rename = "service_data")]
    pub service_data: serde_json::Value,
}

impl HassServiceCall {
    /// Create a new service call.
    pub fn new(domain: String, service: String, entity_id: String) -> Self {
        let mut service_data = serde_json::Map::new();
        service_data.insert("entity_id".to_string(), serde_json::json!(entity_id));

        Self {
            domain,
            service,
            service_data: serde_json::Value::Object(service_data),
        }
    }

    /// Add a parameter to the service call.
    pub fn with_param(mut self, key: String, value: serde_json::Value) -> Self {
        if let Some(obj) = self.service_data.as_object_mut() {
            obj.insert(key, value);
        }
        self
    }
}

/// WebSocket event from Home Assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HassEvent {
    /// Authentication result (initial response)
    #[serde(rename = "auth_ok")]
    AuthOk,

    /// Authentication failed
    #[serde(rename = "auth_invalid")]
    AuthInvalid { message: String },

    /// Event received
    Event {
        event: usize,
        #[serde(rename = "type")]
        event_type: String,
        #[serde(default)]
        data: serde_json::Value,
        #[serde(rename = "time_fired")]
        time_fired: Option<String>,
        origin: Option<String>,
        context: Option<HassContext>,
    },

    /// Result of a subscription or command
    Result {
        #[serde(default)]
        success: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(rename = "id")]
        request_id: Option<usize>,
    },

    /// Ping message
    #[serde(rename = "ping")]
    Ping,

    /// Pong response
    #[serde(rename = "pong")]
    Pong,
}

/// Pong response with ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassPong {
    #[serde(default)]
    pub id: usize,
}

impl HassEvent {
    /// Check if this is a state changed event.
    pub fn is_state_changed(&self) -> bool {
        matches!(self, HassEvent::Event { event_type, .. } if event_type == "state_changed")
    }

    /// Extract entity_id from state_changed event.
    pub fn entity_id(&self) -> Option<String> {
        if let HassEvent::Event { data, .. } = self {
            data.get("entity_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Extract new state from state_changed event.
    pub fn new_state(&self) -> Option<HassEntityState> {
        if let HassEvent::Event { data, .. } = self {
            data.get("new_state")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_from_entity_id() {
        assert_eq!(HassDomain::from_entity_id("sensor.temperature_188"), HassDomain::Sensor);
        assert_eq!(HassDomain::from_entity_id("switch.living_room"), HassDomain::Switch);
        assert_eq!(HassDomain::from_entity_id("light.kitchen"), HassDomain::Light);
        assert_eq!(HassDomain::from_entity_id("unknown.something"), HassDomain::Unknown);
    }

    #[test]
    fn test_service_call_creation() {
        let call = HassServiceCall::new(
            "homeassistant".to_string(),
            "turn_on".to_string(),
            "switch.test".to_string(),
        );

        assert_eq!(call.domain, "homeassistant");
        assert_eq!(call.service, "turn_on");
        assert_eq!(
            call.service_data.get("entity_id"),
            Some(&serde_json::json!("switch.test"))
        );
    }

    #[test]
    fn test_websocket_url() {
        let config = HassConnectionConfig::with_bearer_token(
            "http://localhost:8123".to_string(),
            "test_token".to_string(),
        );

        assert_eq!(config.websocket_url(), "ws://localhost:8123/api/websocket");
        assert_eq!(config.api_base(), "http://localhost:8123/api");
    }
}
