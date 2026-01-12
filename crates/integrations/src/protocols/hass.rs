//! Home Assistant (HASS) integration.
//!
//! Provides integration with Home Assistant via HTTP REST API and WebSocket.

#![cfg(feature = "http")]

use crate::{Integration, IntegrationMetadata, IntegrationType, IntegrationState};
use crate::protocols::BaseIntegration;
use edge_ai_core::integration::{
    IntegrationEvent, IntegrationCommand, IntegrationResponse, IntegrationError,
    Result as IntegrationResult, IntegrationConfig,
};
use edge_ai_core::eventbus::EventBus;
use async_trait::async_trait;
use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "http")]
use reqwest::Client;

/// HASS integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassConfig {
    /// HASS server URL (e.g., http://localhost:8123)
    pub url: String,

    /// Long-lived access token.
    pub token: String,

    /// Connection timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Use WebSocket for real-time updates.
    #[serde(default = "default_websocket")]
    pub use_websocket: bool,

    /// Polling interval in seconds (when WebSocket is disabled).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_timeout() -> u64 { 30 }
fn default_websocket() -> bool { true }
fn default_poll_interval() -> u64 { 5 }

impl HassConfig {
    /// Create a new HASS configuration.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            timeout_secs: default_timeout(),
            use_websocket: default_websocket(),
            poll_interval_secs: default_poll_interval(),
        }
    }

    /// Get the API base URL.
    pub fn api_url(&self) -> String {
        let url = self.url.trim_end_matches('/');
        format!("{}/api", url)
    }

    /// Get the WebSocket URL.
    pub fn websocket_url(&self) -> String {
        let url = self.url.trim_end_matches('/');
        url.replace("http://", "ws://")
            .replace("https://", "wss://")
            + "/api/websocket"
    }
}

/// HASS entity state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassEntity {
    /// Entity ID (e.g., "sensor.temperature").
    pub entity_id: String,

    /// State value.
    pub state: String,

    /// Attributes.
    pub attributes: serde_json::Value,

    /// Last changed timestamp.
    pub last_changed: String,

    /// Last updated timestamp.
    pub last_updated: String,

    /// Entity ID context.
    pub context: Option<serde_json::Value>,
}

/// HASS integration using HTTP and WebSocket.
pub struct HassIntegration {
    /// Base integration.
    base: BaseIntegration,

    /// Configuration.
    config: HassConfig,

    /// Event sender.
    sender: Arc<mpsc::Sender<IntegrationEvent>>,

    /// HTTP client.
    client: Client,

    /// Running flag.
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl HassIntegration {
    /// Create a new HASS integration.
    pub fn new(config: HassConfig) -> Self {
        let (sender, _) = mpsc::channel(1024);

        Self {
            base: BaseIntegration::new(
                format!("hass_{}", uuid::Uuid::new_v4()),
                "Home Assistant",
                IntegrationType::Hass,
            ),
            config,
            sender: Arc::new(sender),
            client: Client::new(),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &HassConfig {
        &self.config
    }

    /// Get all entities.
    pub async fn get_states(&self) -> IntegrationResult<Vec<HassEntity>> {
        let url = format!("{}/states", self.config.api_url());

        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(IntegrationError::AuthenticationFailed(
                response.status().to_string()
            ));
        }

        let states: Vec<HassEntity> = response
            .json()
            .await
            .map_err(|e| IntegrationError::TransformationFailed(e.to_string()))?;

        Ok(states)
    }

    /// Get a single entity state.
    pub async fn get_state(&self, entity_id: &str) -> IntegrationResult<HassEntity> {
        let url = format!("{}/states/{}", self.config.api_url(), entity_id);

        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        if response.status() == 404 {
            return Err(IntegrationError::NotFound(entity_id.to_string()));
        }

        if !response.status().is_success() {
            return Err(IntegrationError::Other(anyhow::anyhow!(
                "Failed to get state: {}",
                response.status()
            )));
        }

        let state: HassEntity = response
            .json()
            .await
            .map_err(|e| IntegrationError::TransformationFailed(e.to_string()))?;

        Ok(state)
    }

    /// Call a HASS service.
    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        entity_id: &str,
        data: serde_json::Value,
    ) -> IntegrationResult<serde_json::Value> {
        let url = format!("{}/services/{}/{}", self.config.api_url(), domain, service);

        let mut body = serde_json::json!({
            "entity_id": entity_id
        });

        if let Some(obj) = data.as_object() {
            if let Some(target) = obj.get("target") {
                body["target"] = target.clone();
            }
            for (key, value) in obj.iter() {
                if key != "target" {
                    body[key] = value.clone();
                }
            }
        }

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(IntegrationError::CommandFailed(format!(
                "Service call failed: {}",
                response.status()
            )));
        }

        Ok(serde_json::json!({}))
    }
}

#[async_trait]
impl Integration for HassIntegration {
    fn metadata(&self) -> &IntegrationMetadata {
        &self.base.metadata
    }

    fn state(&self) -> IntegrationState {
        self.base.to_integration_state()
    }

    async fn start(&self) -> IntegrationResult<()> {
        // Verify connection by fetching states
        self.get_states().await?;

        self.running.store(true, std::sync::atomic::Ordering::Relaxed);
        self.base.set_running(true);

        // Start polling task if WebSocket is not enabled
        if !self.config.use_websocket {
            let sender = self.sender.clone();
            let client = self.client.clone();
            let api_url = self.config.api_url();
            let token = self.config.token.clone();
            let poll_interval = std::time::Duration::from_secs(self.config.poll_interval_secs);
            let running = self.running.clone();

            tokio::spawn(async move {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(states) = fetch_states(&client, &api_url, &token).await {
                        for state in states {
                            let event = IntegrationEvent::Data {
                                source: "hass".to_string(),
                                data_type: "entity_state".to_string(),
                                payload: serde_json::to_vec(&state).unwrap_or_default(),
                                metadata: serde_json::json!({}),
                                timestamp: chrono::Utc::now().timestamp(),
                            };
                            let _ = sender.send(event);
                        }
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            });
        }

        Ok(())
    }

    async fn stop(&self) -> IntegrationResult<()> {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
        self.base.set_running(false);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>> {
        // Create a new channel for this subscriber
        let (_tx, rx) = mpsc::channel(1024);
        // We'd need to store the sender to broadcast events
        // For now, return empty stream as this is a simplified implementation
        Box::pin(ReceiverStream::new(rx))
    }

    async fn send_command(&self, command: IntegrationCommand) -> IntegrationResult<IntegrationResponse> {
        match command {
            IntegrationCommand::Query { target, .. } => {
                let state = self.get_state(&target).await?;
                Ok(IntegrationResponse::success(serde_json::to_value(state).unwrap()))
            }
            IntegrationCommand::CallService { target, service, params } => {
                // Parse service as "domain.service"
                let parts: Vec<&str> = service.split('.').collect();
                let (domain, svc) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    return Err(IntegrationError::CommandFailed(
                        "Invalid service format (expected 'domain.service')".to_string()
                    ));
                };

                self.call_service(domain, svc, &target, params).await?;
                Ok(IntegrationResponse::success(serde_json::json!({})))
            }
            IntegrationCommand::SendData { .. } => {
                Err(IntegrationError::CommandFailed(
                    "SendData not supported for HASS".to_string()
                ))
            }
        }
    }
}

/// Helper function to fetch states.
async fn fetch_states(
    client: &Client,
    api_url: &str,
    token: &str,
) -> IntegrationResult<Vec<HassEntity>> {
    let url = format!("{}/states", api_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

    if !response.status().is_success() {
        return Err(IntegrationError::Other(anyhow::anyhow!(
            "Failed to fetch states: {}",
            response.status()
        )));
    }

    let states: Vec<HassEntity> = response
        .json()
        .await
        .map_err(|e| IntegrationError::TransformationFailed(e.to_string()))?;

    Ok(states)
}

/// Create a HASS integration from a config.
pub fn create_hass_integration(
    id: impl Into<String>,
    config: HassConfig,
) -> IntegrationResult<HassIntegration> {
    let mut integration = HassIntegration::new(config);
    integration.base = BaseIntegration::new(id, "Home Assistant", IntegrationType::Hass);
    Ok(integration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hass_config() {
        let config = HassConfig::new("http://localhost:8123", "token123");
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.token, "token123");
        assert_eq!(config.api_url(), "http://localhost:8123/api");
        assert_eq!(config.websocket_url(), "ws://localhost:8123/api/websocket");
        assert!(config.use_websocket);
    }

    #[test]
    fn test_hass_integration() {
        let config = HassConfig::new("http://localhost:8123", "token");
        let integration = HassIntegration::new(config);
        assert_eq!(integration.metadata().integration_type, IntegrationType::Hass);
        assert!(!integration.base.is_running());
    }
}
