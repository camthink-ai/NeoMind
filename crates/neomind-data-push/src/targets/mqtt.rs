//! MQTT push target (publish to external broker).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch, Mutex};

use super::PushDestination;

/// MQTT target configuration.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for MqttConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MqttConfig")
            .field("broker", &self.broker)
            .field("port", &self.port)
            .field("topic", &self.topic)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("qos", &self.qos)
            .field("client_id", &self.client_id)
            .finish()
    }
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

/// Events surfaced from the background rumqttc EventLoop task to `send()`.
#[derive(Debug, Clone)]
enum MqttLoopEvent {
    /// Outgoing publish frame written to the broker connection (QoS 0 confirmation).
    Published,
    /// Broker acknowledged QoS 1 publish (`PUBACK`).
    PubAck,
    /// Broker completed QoS 2 publish (`PUBCOMP`).
    PubComp,
    /// Broker / network error observed by the eventloop.
    Error(String),
}

/// Active MQTT client + continuously driven background EventLoop task.
/// Dropping `MqttConnection` cancels and aborts the background task cleanly.
struct MqttConnection {
    client: rumqttc::AsyncClient,
    event_tx: broadcast::Sender<MqttLoopEvent>,
    shutdown_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MqttConnection {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.task.abort();
    }
}

/// MQTT push destination with lazy connection and a continuously driven
/// background eventloop (handles `PINGREQ`, reconnects, and publish ACKs).
pub struct MqttTarget {
    config: MqttConfig,
    conn: Arc<Mutex<Option<MqttConnection>>>,
    /// Serializes concurrent `send()` calls so each publish observes its own ACK.
    send_lock: Arc<Mutex<()>>,
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
            conn: Arc::new(Mutex::new(None)),
            send_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Ensure a connection and its background eventloop task exist.
    async fn ensure_connected(&self) -> Result<(rumqttc::AsyncClient, broadcast::Receiver<MqttLoopEvent>)> {
        let mut conn_guard = self.conn.lock().await;
        if conn_guard.is_none() {
            let client_id = format!(
                "{}-{}",
                self.config.client_id,
                uuid::Uuid::new_v4().as_simple()
            );

            let mut mqtt_opts =
                rumqttc::MqttOptions::new(client_id, &self.config.broker, self.config.port);
            mqtt_opts.set_keep_alive(Duration::from_secs(30));

            if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
                mqtt_opts.set_credentials(user, pass);
            }

            let (client, mut eventloop) = rumqttc::AsyncClient::new(mqtt_opts, 64);
            let (event_tx, _) = broadcast::channel(64);
            let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

            let tx_clone = event_tx.clone();
            let broker_label = format!("{}:{}", self.config.broker, self.config.port);
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        res = eventloop.poll() => {
                            match res {
                                Ok(rumqttc::Event::Incoming(rumqttc::Packet::PubAck(_))) => {
                                    let _ = tx_clone.send(MqttLoopEvent::PubAck);
                                }
                                Ok(rumqttc::Event::Incoming(rumqttc::Packet::PubComp(_))) => {
                                    let _ = tx_clone.send(MqttLoopEvent::PubComp);
                                }
                                Ok(rumqttc::Event::Outgoing(rumqttc::Outgoing::Publish(_))) => {
                                    let _ = tx_clone.send(MqttLoopEvent::Published);
                                }
                                Ok(_) => {
                                    // ConnAck, PingReq/PingResp, PubRec/PubRel, etc.
                                    // Background loop keeps driving these continuously.
                                }
                                Err(e) => {
                                    let msg = e.to_string();
                                    tracing::debug!(
                                        broker = %broker_label,
                                        error = %msg,
                                        "MQTT push target eventloop error; backing off before reconnect"
                                    );
                                    let _ = tx_clone.send(MqttLoopEvent::Error(msg));
                                    tokio::select! {
                                        _ = shutdown_rx.changed() => {
                                            if *shutdown_rx.borrow() {
                                                break;
                                            }
                                        }
                                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                                    }
                                }
                            }
                        }
                    }
                }
            });

            *conn_guard = Some(MqttConnection {
                client,
                event_tx,
                shutdown_tx,
                task,
            });
        }

        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| anyhow!("MQTT connection not initialized"))?;
        Ok((conn.client.clone(), conn.event_tx.subscribe()))
    }
}

#[async_trait]
impl PushDestination for MqttTarget {
    async fn send(&self, payload: &str) -> std::result::Result<(), super::DeliveryError> {
        let _send_guard = self.send_lock.lock().await;

        let (client, mut event_rx) = self
            .ensure_connected()
            .await
            .map_err(|e| super::DeliveryError::Other(anyhow!("MQTT connect failed: {}", e)))?;

        let qos = match self.config.qos {
            0 => rumqttc::QoS::AtMostOnce,
            2 => rumqttc::QoS::ExactlyOnce,
            _ => rumqttc::QoS::AtLeastOnce,
        };

        // Enqueue the publish with a bounded timeout so a stalled queue cannot
        // block cancellation or shutdown forever.
        tokio::time::timeout(
            Duration::from_secs(10),
            client.publish(&self.config.topic, qos, false, payload.as_bytes()),
        )
        .await
        .map_err(|_| {
            super::DeliveryError::Other(anyhow!(
                "MQTT publish timed out — broker unreachable or request queue full"
            ))
        })?
        .map_err(|e| super::DeliveryError::Other(anyhow!("MQTT publish failed: {}", e)))?;

        // Wait for real dispatch/broker acknowledgement from the background
        // eventloop (not a single `poll()` that merely returned `ConnAck`).
        // Timeout MUST return Err so delivery logs and retries stay honest.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(super::DeliveryError::Other(anyhow!(
                    "MQTT publish confirmation timed out after 10s"
                )));
            }

            match tokio::time::timeout(remaining, event_rx.recv()).await {
                Err(_) => {
                    return Err(super::DeliveryError::Other(anyhow!(
                        "MQTT publish confirmation timed out after 10s"
                    )));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(super::DeliveryError::Other(anyhow!(
                        "MQTT eventloop closed before publish confirmation"
                    )));
                }
                Ok(Ok(MqttLoopEvent::Error(e))) => {
                    return Err(super::DeliveryError::Other(anyhow!(
                        "MQTT broker error: {}",
                        e
                    )));
                }
                Ok(Ok(MqttLoopEvent::Published)) if matches!(qos, rumqttc::QoS::AtMostOnce) => {
                    return Ok(());
                }
                Ok(Ok(MqttLoopEvent::PubAck)) if matches!(qos, rumqttc::QoS::AtLeastOnce) => {
                    return Ok(());
                }
                Ok(Ok(MqttLoopEvent::PubComp)) if matches!(qos, rumqttc::QoS::ExactlyOnce) => {
                    return Ok(());
                }
                Ok(Ok(_)) => {
                    // Intermediate event (e.g., Outgoing::Publish while awaiting PubAck for QoS1)
                    continue;
                }
            }
        }
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
