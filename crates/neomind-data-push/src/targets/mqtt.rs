//! MQTT push target (publish to external broker).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::PushDestination;

/// MQTT target configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    pub broker: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub topic: String,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_qos")]
    pub qos: u8,
    /// Client ID prefix (random suffix appended).
    #[serde(default = "default_client_id")]
    pub client_id: String,
}

fn default_port() -> u16 {
    1883
}

fn default_qos() -> u8 {
    1
}

fn default_client_id() -> String {
    "neomind-push".to_string()
}

/// MQTT push destination with lazy connection.
pub struct MqttTarget {
    config: MqttConfig,
    client: Arc<Mutex<Option<rumqttc::AsyncClient>>>,
    eventloop: Arc<Mutex<Option<rumqttc::EventLoop>>>,
}

impl MqttTarget {
    pub fn from_config(config: &serde_json::Value) -> Result<Self> {
        let mc: MqttConfig = serde_json::from_value(config.clone())
            .map_err(|e| anyhow!("Invalid MQTT config: {}", e))?;
        if mc.broker.is_empty() {
            return Err(anyhow!("MQTT broker is required"));
        }
        if mc.topic.is_empty() {
            return Err(anyhow!("MQTT topic is required"));
        }
        Ok(Self {
            config: mc,
            client: Arc::new(Mutex::new(None)),
            eventloop: Arc::new(Mutex::new(None)),
        })
    }

    /// Ensure a connection exists, creating one lazily if needed.
    async fn ensure_connected(&self) -> Result<()> {
        let mut client_guard = self.client.lock().await;
        if client_guard.is_none() {
            let client_id = format!(
                "{}-{}",
                self.config.client_id,
                uuid::Uuid::new_v4().as_simple()
            );

            let mut mqtt_opts =
                rumqttc::MqttOptions::new(client_id, &self.config.broker, self.config.port);
            mqtt_opts.set_keep_alive(std::time::Duration::from_secs(30));

            if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
                mqtt_opts.set_credentials(user, pass);
            }

            let (client, eventloop) = rumqttc::AsyncClient::new(mqtt_opts, 10);
            *client_guard = Some(client);

            let mut el_guard = self.eventloop.lock().await;
            *el_guard = Some(eventloop);
        }
        Ok(())
    }
}

#[async_trait]
impl PushDestination for MqttTarget {
    async fn send(&self, payload: &str) -> std::result::Result<(), super::DeliveryError> {
        self.ensure_connected()
            .await
            .map_err(|e| super::DeliveryError::Other(anyhow!("MQTT connect failed: {}", e)))?;

        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| super::DeliveryError::Other(anyhow!("MQTT client not initialized")))?;

        let qos = match self.config.qos {
            0 => rumqttc::QoS::AtMostOnce,
            2 => rumqttc::QoS::ExactlyOnce,
            _ => rumqttc::QoS::AtLeastOnce,
        };

        // [bounded publish] client.publish() waits for channel capacity —
        // with a dead broker the bounded (10) request queue fills and this
        // await blocks FOREVER, wedging the target task so its select!
        // cancel arm is unreachable and stop()/update/delete hang the API
        // handler. A timeout converts the wedge into a retriable error.
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.publish(&self.config.topic, qos, false, payload.as_bytes()),
        )
        .await
        .map_err(|_| {
            super::DeliveryError::Other(anyhow!(
                "MQTT publish timed out — broker unreachable or request queue full"
            ))
        })?
        .map_err(|e| super::DeliveryError::Other(anyhow!("MQTT publish failed: {}", e)))?;

        // Poll eventloop to process the publish — and PROPAGATE errors.
        // The old `let _ = timeout(5s, poll())` discarded connection
        // errors entirely: deliveries to an unreachable broker were
        // logged as Success and never retried (the whole
        // deliver_with_retry machinery was dead code for MQTT targets).
        drop(client_guard);
        let mut el_guard = self.eventloop.lock().await;
        if let Some(ref mut eventloop) = *el_guard {
            match tokio::time::timeout(std::time::Duration::from_secs(5), eventloop.poll()).await {
                Err(_elapsed) => {
                    // No event within 5s: not proof of failure (QoS0 sends
                    // nothing back), leave as success.
                }
                Ok(Err(e)) => {
                    return Err(super::DeliveryError::Other(anyhow!(
                        "MQTT broker error: {}",
                        e
                    )))
                }
                Ok(Ok(_)) => {}
            }
        }

        Ok(())
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let mc: MqttConfig = serde_json::from_value(config.clone())
            .map_err(|e| anyhow!("Invalid MQTT config: {}", e))?;
        if mc.broker.is_empty() {
            return Err(anyhow!("MQTT broker is required"));
        }
        if mc.topic.is_empty() {
            return Err(anyhow!("MQTT topic is required"));
        }
        Ok(())
    }
}
