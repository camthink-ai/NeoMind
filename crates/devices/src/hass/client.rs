//! Home Assistant REST API client.

use super::entities::{
    HassConnectionConfig, HassEntityState, HassServiceCall, HassDomain,
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur when interacting with Home Assistant.
#[derive(Debug, Error)]
pub enum HassClientError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    #[error("Service call failed: {0}")]
    ServiceCallFailed(String),

    #[error("Invalid response from Home Assistant: {0}")]
    InvalidResponse(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Timeout")]
    Timeout,
}

/// Result type for HASS client operations.
pub type HassResult<T> = Result<T, HassClientError>;

/// Home Assistant REST API client.
pub struct HassClient {
    config: HassConnectionConfig,
    http_client: reqwest::Client,
}

impl HassClient {
    /// Create a new Home Assistant client.
    pub fn new(config: HassConnectionConfig) -> HassResult<Self> {
        let builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout));

        let http_client = if !config.verify_ssl {
            builder
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| HassClientError::ConnectionError(e.to_string()))?
        } else {
            builder
                .build()
                .map_err(|e| HassClientError::ConnectionError(e.to_string()))?
        };

        Ok(Self {
            config,
            http_client,
        })
    }

    /// Get the base API URL.
    fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.config.api_base(), path.trim_start_matches('/'))
    }

    /// Add authorization headers to a request.
    fn add_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let header = self.config.auth.auth_header();
        request.header("Authorization", header)
    }

    /// Test the connection to Home Assistant.
    pub async fn test_connection(&self) -> HassResult<bool> {
        let response = self
            .add_auth(self.http_client.get(&self.api_url("/")))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// Get all entity states.
    pub async fn get_states(&self) -> HassResult<Vec<HassEntityState>> {
        let response = self
            .add_auth(self.http_client.get(&self.api_url("/states")))
            .send()
            .await?;

        if !response.status().is_success() {
            if response.status() == 401 {
                return Err(HassClientError::AuthenticationFailed);
            }
            return Err(HassClientError::InvalidResponse(format!(
                "Status: {}",
                response.status()
            )));
        }

        let states: Vec<HassEntityState> = response.json().await?;
        Ok(states)
    }

    /// Get state for a specific entity.
    pub async fn get_state(&self, entity_id: &str) -> HassResult<HassEntityState> {
        let url = self.api_url(&format!("/states/{}", entity_id));
        let response = self
            .add_auth(self.http_client.get(&url))
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let state: HassEntityState = response.json().await?;
                Ok(state)
            }
            reqwest::StatusCode::NOT_FOUND => {
                Err(HassClientError::EntityNotFound(entity_id.to_string()))
            }
            reqwest::StatusCode::UNAUTHORIZED => Err(HassClientError::AuthenticationFailed),
            _ => Err(HassClientError::InvalidResponse(format!(
                "Status: {}",
                response.status()
            ))),
        }
    }

    /// Get entities filtered by domain.
    pub async fn get_states_by_domain(&self, domain: HassDomain) -> HassResult<Vec<HassEntityState>> {
        let all_states = self.get_states().await?;
        let domain_str = domain.as_str();

        Ok(all_states
            .into_iter()
            .filter(|s| s.entity_id.starts_with(&format!("{}.", domain_str)))
            .collect())
    }

    /// Get all available services.
    pub async fn get_services(&self) -> HassResult<HashMap<String, Vec<String>>> {
        let response = self
            .add_auth(self.http_client.get(&self.api_url("/services")))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(HassClientError::InvalidResponse(format!(
                "Status: {}",
                response.status()
            )));
        }

        let services: HashMap<String, Vec<JsonValue>> = response.json().await?;

        // Convert to simple domain -> service names map
        Ok(services
            .into_iter()
            .map(|(domain, service_list)| {
                let names = service_list
                    .into_iter()
                    .filter_map(|s| s.get("service").and_then(|v| v.as_str()).map(String::from))
                    .collect();
                (domain, names)
            })
            .collect())
    }

    /// Call a service.
    pub async fn call_service(&self, call: HassServiceCall) -> HassResult<JsonValue> {
        let url = self.api_url(&format!(
            "/services/{}/{}",
            call.domain, call.service
        ));

        let response = self
            .add_auth(self.http_client.post(&url))
            .json(&call.service_data)
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => Ok(response.json().await.unwrap_or(JsonValue::Null)),
            reqwest::StatusCode::UNAUTHORIZED => Err(HassClientError::AuthenticationFailed),
            _ => Err(HassClientError::ServiceCallFailed(format!(
                "Status: {}",
                response.status()
            ))),
        }
    }

    /// Turn on a switch/light.
    pub async fn turn_on(&self, entity_id: &str) -> HassResult<()> {
        let domain = HassDomain::from_entity_id(entity_id);
        let service = match domain {
            HassDomain::Switch | HassDomain::Light | HassDomain::Fan | HassDomain::InputBoolean
            | HassDomain::Cover | HassDomain::Lock => "turn_on",
            HassDomain::MediaPlayer => "turn_on",
            _ => "turn_on",
        };

        let call = HassServiceCall::new(domain.as_str().to_string(), service.to_string(), entity_id.to_string());
        self.call_service(call).await?;
        Ok(())
    }

    /// Turn off a switch/light.
    pub async fn turn_off(&self, entity_id: &str) -> HassResult<()> {
        let domain = HassDomain::from_entity_id(entity_id);
        let service = match domain {
            HassDomain::Switch | HassDomain::Light | HassDomain::Fan | HassDomain::InputBoolean
            | HassDomain::Cover | HassDomain::Lock => "turn_off",
            HassDomain::MediaPlayer => "turn_off",
            _ => "turn_off",
        };

        let call = HassServiceCall::new(domain.as_str().to_string(), service.to_string(), entity_id.to_string());
        self.call_service(call).await?;
        Ok(())
    }

    /// Toggle a switch/light.
    pub async fn toggle(&self, entity_id: &str) -> HassResult<()> {
        let domain = HassDomain::from_entity_id(entity_id);
        let call = HassServiceCall::new(
            domain.as_str().to_string(),
            "toggle".to_string(),
            entity_id.to_string(),
        );
        self.call_service(call).await?;
        Ok(())
    }

    /// Get all devices.
    pub async fn get_devices(&self) -> HassResult<Vec<JsonValue>> {
        let response = self
            .add_auth(self.http_client.get(&self.api_url("/devices")))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(HassClientError::InvalidResponse(format!(
                "Status: {}",
                response.status()
            )));
        }

        response.json().await.map_err(Into::into)
    }

    /// Get all areas.
    pub async fn get_areas(&self) -> HassResult<Vec<JsonValue>> {
        let response = self
            .add_auth(self.http_client.get(&self.api_url("/areas")))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(HassClientError::InvalidResponse(format!(
                "Status: {}",
                response.status()
            )));
        }

        response.json().await.map_err(Into::into)
    }

    /// Get entities for a specific device.
    pub async fn get_device_entities(&self, device_id: &str) -> HassResult<Vec<HassEntityState>> {
        // First get the device to find its entities
        let devices = self.get_devices().await?;

        let target_device = devices
            .into_iter()
            .find(|d| {
                d.get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| id == device_id)
                    .unwrap_or(false)
            })
            .ok_or_else(|| HassClientError::EntityNotFound(device_id.to_string()))?;

        // Get all entity IDs for this device
        let entity_ids: Vec<String> = target_device
            .get("entities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.get("entity_id").and_then(|e| e.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Fetch all states and filter
        let all_states = self.get_states().await?;
        Ok(all_states
            .into_iter()
            .filter(|s| entity_ids.contains(&s.entity_id))
            .collect())
    }

    /// Subscribe to entity updates (returns info needed for WebSocket).
    pub fn get_websocket_url(&self) -> String {
        self.config.websocket_url()
    }

    /// Get the connection config.
    pub fn config(&self) -> &HassConnectionConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_url_construction() {
        let config = HassConnectionConfig::with_bearer_token(
            "http://localhost:8123".to_string(),
            "test_token".to_string(),
        );

        let client = HassClient::new(config).unwrap();

        assert_eq!(client.api_url("/states"), "http://localhost:8123/api/states");
        assert_eq!(client.api_url("states"), "http://localhost:8123/api/states");
        assert_eq!(
            client.api_url("/services/homeassistant/turn_on"),
            "http://localhost:8123/api/services/homeassistant/turn_on"
        );
    }

    #[test]
    fn test_websocket_url() {
        let config = HassConnectionConfig::with_bearer_token(
            "https://homeassistant.example.com".to_string(),
            "test_token".to_string(),
        );

        let client = HassClient::new(config).unwrap();

        assert_eq!(
            client.get_websocket_url(),
            "wss://homeassistant.example.com/api/websocket"
        );
    }
}
