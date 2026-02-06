//! MQTT connector using rumqttc.
//!
//! Provides a MQTT client implementation that conforms to the Connector trait.

use neomind_core::integration::connector::{
    BaseConnector as CoreBaseConnector, Connector, ConnectorError, Result,
};
use futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// MQTT QoS level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Qos {
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

impl Default for Qos {
    fn default() -> Self {
        Self::AtLeastOnce
    }
}

impl From<Qos> for rumqttc::QoS {
    fn from(qos: Qos) -> Self {
        match qos {
            Qos::AtMostOnce => rumqttc::QoS::AtMostOnce,
            Qos::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            Qos::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }
}

/// MQTT connector configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    /// Broker address.
    pub broker: String,

    /// Broker port (default 1883 for non-TLS, 8883 for TLS).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Client ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Username.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Password.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// Keep-alive interval in seconds.
    #[serde(default = "default_keep_alive")]
    pub keep_alive: u64,

    /// Clean session flag.
    #[serde(default = "default_clean_session")]
    pub clean_session: bool,

    /// Use TLS/SSL.
    #[serde(default)]
    pub tls: bool,

    /// Default QoS level for subscriptions.
    #[serde(default)]
    pub qos: Qos,

    /// Connection timeout in seconds.
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Auto-reconnect interval in milliseconds.
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_ms: u64,
}

fn default_port() -> u16 {
    1883
}

fn default_keep_alive() -> u64 {
    60
}

fn default_clean_session() -> bool {
    true
}

fn default_connection_timeout() -> u64 {
    30
}

fn default_reconnect_interval() -> u64 {
    5000
}

impl MqttConfig {
    /// Create a new MQTT configuration.
    pub fn new(broker: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            port: default_port(),
            client_id: None,
            username: None,
            password: None,
            keep_alive: default_keep_alive(),
            clean_session: default_clean_session(),
            tls: false,
            qos: Qos::default(),
            connection_timeout_secs: default_connection_timeout(),
            reconnect_interval_ms: default_reconnect_interval(),
        }
    }

    /// Set the port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set authentication.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// Set the client ID.
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Set TLS.
    pub fn with_tls(mut self, tls: bool) -> Self {
        self.tls = tls;
        if tls && self.port == 1883 {
            self.port = 8883;
        }
        self
    }

    /// Get the full broker address.
    pub fn broker_addr(&self) -> String {
        format!("{}:{}", self.broker, self.port)
    }
}

/// MQTT connector using rumqttc.
pub struct MqttConnector {
    /// Base connector functionality.
    base: CoreBaseConnector,

    /// Configuration.
    config: MqttConfig,

    /// Data sender.
    sender: Arc<Mutex<Option<mpsc::Sender<MqttMessage>>>>,

    /// Data receiver.
    receiver: Arc<Mutex<Option<mpsc::Receiver<MqttMessage>>>>,

    /// Client handle (when connected).
    client: Arc<Mutex<Option<rumqttc::Client>>>,
}

/// MQTT message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttMessage {
    /// Topic.
    pub topic: String,

    /// Payload.
    pub payload: Vec<u8>,

    /// QoS.
    pub qos: Qos,

    /// Retain flag.
    pub retain: bool,
}

impl MqttConnector {
    /// Create a new MQTT connector.
    pub fn new(config: MqttConfig) -> Self {
        let (sender, receiver) = mpsc::channel(1000);

        Self {
            base: CoreBaseConnector::new("mqtt"),
            config,
            sender: Arc::new(Mutex::new(Some(sender))),
            receiver: Arc::new(Mutex::new(Some(receiver))),
            client: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &MqttConfig {
        &self.config
    }

    /// Subscribe to a topic.
    pub async fn subscribe(&self, topic: &str, qos: Qos) -> Result<()> {
        let mut client = self.client.lock();
        if let Some(ref mut cli) = *client {
            cli.subscribe(topic, qos.into())
                .await
                .map_err(|e| ConnectorError::Other(anyhow::anyhow!("Subscribe failed: {}", e)))?;
            Ok(())
        } else {
            Err(ConnectorError::NotConnected)
        }
    }

    /// Publish to a topic.
    pub async fn publish(
        &self,
        topic: &str,
        payload: Vec<u8>,
        qos: Qos,
        retain: bool,
    ) -> Result<()> {
        let mut client = self.client.lock();
        if let Some(ref mut cli) = *client {
            cli.publish(topic, qos.into(), retain, payload)
                .await
                .map_err(|e| ConnectorError::SendFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(ConnectorError::NotConnected)
        }
    }
}

#[async_trait::async_trait]
impl Connector for MqttConnector {
    fn connector_type(&self) -> &str {
        "mqtt"
    }

    fn is_connected(&self) -> bool {
        self.base.is_connected() && self.client.lock().is_some()
    }

    fn metrics(&self) -> Option<neomind_core::integration::connector::ConnectionMetrics> {
        Some(self.base.metrics())
    }

    async fn connect(&self) -> Result<()> {
        // Create rumqttc client options
        let client_id = self
            .config
            .client_id
            .clone()
            .unwrap_or_else(|| format!("neomind_{}", uuid::Uuid::new_v4()));

        let mut opts = rumqttc::MqttOptions::new(client_id, &self.config.broker, self.config.port);
        opts.set_keep_alive(Duration::from_secs(self.config.keep_alive));
        opts.set_clean_session(self.config.clean_session);

        if let (Some(username), Some(password)) = (&self.config.username, &self.config.password) {
            opts.set_credentials(username, password);
        }

        // Create client
        let (client, mut event_loop) = rumqttc::Client::new(opts, 10);

        // Store the client for later use
        *self.client.lock() = Some(client);
        self.base.set_connected(true);

        // Spawn a task to handle incoming events
        let sender = self.sender.clone();
        let base = self.base.clone();
        tokio::spawn(async move {
            while let Ok(notification) = event_loop.poll().await {
                match notification {
                    rumqttc::Event::Incoming(rumqttc::Packet::Publish(p)) => {
                        base.record_received(p.payload.len() as u64);
                        let msg = MqttMessage {
                            topic: p.topic,
                            payload: p.payload.into_vec(),
                            qos: match p.qos {
                                rumqttc::QoS::AtMostOnce => Qos::AtMostOnce,
                                rumqttc::QoS::AtLeastOnce => Qos::AtLeastOnce,
                                rumqttc::QoS::ExactlyOnce => Qos::ExactlyOnce,
                            },
                            retain: p.retain,
                        };
                        let mut s = sender.lock();
                        if let Some(ref mut tx) = *s {
                            let _ = tx.send(msg).await;
                        }
                    }
                    rumqttc::Event::Outgoing(rumqttc::Outgoing::Disconnect) => {
                        base.set_connected(false);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        let mut client_guard = self.client.lock();
        if let Some(client) = client_guard.take() {
            drop(client);
        }
        self.base.set_connected(false);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = Vec<u8>> + Send + '_>> {
        let receiver = self.receiver.lock().take();
        if let Some(rx) = receiver {
            Box::pin(rx.map(|msg| {
                // Convert MqttMessage to Vec<u8>
                // For now, serialize as JSON
                serde_json::to_vec(&msg).unwrap_or_default()
            }))
        } else {
            Box::pin(futures::stream::empty())
        }
    }

    async fn send(&self, data: Vec<u8>) -> Result<()> {
        // Try to parse as MqttMessage
        if let Ok(msg) = serde_json::from_slice::<MqttMessage>(&data) {
            self.publish(&msg.topic, msg.payload, msg.qos, msg.retain)
                .await?;
            self.base.record_sent(data.len() as u64);
            Ok(())
        } else {
            // Treat raw data as a simple publish to a default topic
            Err(ConnectorError::SendFailed(
                "Data must be a valid MqttMessage".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_config() {
        let config = MqttConfig::new("localhost")
            .with_port(1883)
            .with_auth("user", "pass")
            .with_client_id("test_client");

        assert_eq!(config.broker, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
        assert_eq!(config.client_id, Some("test_client".to_string()));
        assert_eq!(config.broker_addr(), "localhost:1883");
    }

    #[test]
    fn test_mqtt_config_tls() {
        let config = MqttConfig::new("localhost").with_tls(true);
        assert!(config.tls);
        assert_eq!(config.port, 8883);
    }
}
