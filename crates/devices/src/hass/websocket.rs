//! Home Assistant WebSocket client for real-time state updates.

use super::entities::{HassConnectionConfig, HassEvent, HassEntityState, HassAuth};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};
use thiserror::Error;

/// Errors that can occur with the WebSocket connection.
#[derive(Debug, Error)]
pub enum HassWsError {
    #[error("Connection failed: {0}")]
    ConnectionError(String),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Message send failed: {0}")]
    SendError(String),

    #[error("Message receive failed: {0}")]
    ReceiveError(String),

    #[error("Invalid message format")]
    InvalidMessage,

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Result type for WebSocket operations.
pub type HassWsResult<T> = Result<T, HassWsError>;

/// Event received from Home Assistant with parsed state.
#[derive(Debug, Clone)]
pub enum HassWsEventData {
    /// Entity state changed
    StateChanged {
        entity_id: String,
        old_state: Option<HassEntityState>,
        new_state: Option<HassEntityState>,
    },
    /// Device triggered
    DeviceTriggered {
        device_id: String,
        data: JsonValue,
    },
    /// Service called
    ServiceCalled {
        domain: String,
        service: String,
        service_data: JsonValue,
    },
    /// Raw event (for any other event type)
    RawEvent {
        event_type: String,
        data: JsonValue,
    },
}

/// Subscription request for WebSocket events.
#[derive(Debug, Clone)]
pub enum HassSubscription {
    /// Subscribe to state changes for specific entities
    Entities(Vec<String>),

    /// Subscribe to all state changes
    AllStates,

    /// Subscribe to device trigger events
    Devices,

    /// Subscribe to service calls
    Services,
}

/// Home Assistant WebSocket client.
pub struct HassWebSocketClient {
    config: HassConnectionConfig,
    /// Sender for commands to the WebSocket task
    command_tx: mpsc::Sender<WsCommand>,
    /// Current connection state
    state: Arc<RwLock<WsState>>,
}

/// Internal state of the WebSocket connection.
#[derive(Debug, Clone, Default)]
struct WsState {
    connected: bool,
    authenticated: bool,
    subscription_id: Option<usize>,
}

/// Internal commands for the WebSocket task.
enum WsCommand {
    Subscribe(HassSubscription, mpsc::Sender<HassWsResult<usize>>),
    CallService {
        domain: String,
        service: String,
        data: JsonValue,
        response_tx: mpsc::Sender<HassWsResult<JsonValue>>,
    },
    Ping,
}

impl HassWebSocketClient {
    /// Create a new WebSocket client (doesn't connect until `start` is called).
    pub fn new(config: HassConnectionConfig) -> Self {
        let (command_tx, _) = mpsc::channel(100);

        Self {
            config,
            command_tx,
            state: Arc::new(RwLock::new(WsState::default())),
        }
    }

    /// Start the WebSocket connection and event processing.
    pub async fn start(&mut self) -> HassWsResult<mpsc::Receiver<HassWsEventData>> {
        let url = self.config.websocket_url();
        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| HassWsError::ConnectionError(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        // Wait for auth required message
        let auth_msg = read
            .next()
            .await
            .ok_or(HassWsError::ConnectionClosed)??;

        let auth_json: JsonValue = serde_json::from_str(&auth_msg.to_string())
            .map_err(|_| HassWsError::InvalidMessage)?;

        // Check if auth is required
        if auth_json["type"] != "auth_required" {
            return Err(HassWsError::InvalidMessage);
        }

        // Send auth token
        let auth_msg = match &self.config.auth {
            HassAuth::BearerToken { token } => serde_json::json!({
                "type": "auth",
                "access_token": token
            }),
            HassAuth::ApiKey { key } => serde_json::json!({
                "type": "auth",
                "api_key": key
            }),
            HassAuth::UsernamePassword { username, password } => serde_json::json!({
                "type": "auth",
                "username": username,
                "password": password
            }),
        };

        write
            .send(Message::Text(auth_msg.to_string()))
            .await
            .map_err(|e| HassWsError::SendError(e.to_string()))?;

        // Wait for auth response
        let auth_response = read
            .next()
            .await
            .ok_or(HassWsError::ConnectionClosed)??;

        let auth_response_json: JsonValue = serde_json::from_str(&auth_response.to_string())
            .map_err(|_| HassWsError::InvalidMessage)?;

        if auth_response_json["type"] != "auth_ok" {
            return Err(HassWsError::AuthenticationFailed);
        }

        // Update state
        let mut state = self.state.write().await;
        state.connected = true;
        state.authenticated = true;
        drop(state);

        // Create channels for event dispatching
        let (event_tx, event_rx) = mpsc::channel(100);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(100);

        self.command_tx = cmd_tx;

        // Spawn the WebSocket task
        let state_arc = self.state.clone();
        tokio::spawn(async move {
            let mut id_counter = 1usize;
            let mut event_subscribers = Vec::new();

            loop {
                tokio::select! {
                    // Handle incoming messages
                    Some(msg_result) = read.next() => {
                        match msg_result {
                            Ok(msg) => {
                                if msg.is_text() || msg.is_close() {
                                    if let Err(e) = Self::handle_message(
                                        msg,
                                        &event_tx,
                                        &state_arc,
                                    ).await {
                                        eprintln!("WebSocket message error: {}", e);
                                        break;
                                    }
                                }
                            }
                            Err(_) => {
                                let mut state = state_arc.write().await;
                                state.connected = false;
                                state.authenticated = false;
                                break;
                            }
                        }
                    }

                    // Handle outgoing commands
                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            WsCommand::Subscribe(subscription, response_tx) => {
                                let id = id_counter;
                                id_counter += 1;

                                let subscribe_msg = match subscription {
                                    HassSubscription::Entities(entities) => {
                                        event_subscribers = entities.clone();
                                        serde_json::json!({
                                            "id": id,
                                            "type": "subscribe_events",
                                            "event_type": "state_changed",
                                            "entity_ids": entities
                                        })
                                    }
                                    HassSubscription::AllStates => {
                                        serde_json::json!({
                                            "id": id,
                                            "type": "subscribe_events",
                                            "event_type": "state_changed"
                                        })
                                    }
                                    HassSubscription::Devices => {
                                        serde_json::json!({
                                            "id": id,
                                            "type": "subscribe_events",
                                            "event_type": "device_trigger"
                                        })
                                    }
                                    HassSubscription::Services => {
                                        serde_json::json!({
                                            "id": id,
                                            "type": "subscribe_events",
                                            "event_type": "call_service"
                                        })
                                    }
                                };

                                if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                                    let _ = response_tx.send(Err(HassWsError::SendError(e.to_string()))).await;
                                } else {
                                    let _ = response_tx.send(Ok(id)).await;
                                }
                            }
                            WsCommand::CallService { domain, service, data, response_tx } => {
                                let id = id_counter;
                                id_counter += 1;

                                let call_msg = serde_json::json!({
                                    "id": id,
                                    "type": "call_service",
                                    "domain": domain,
                                    "service": service,
                                    "service_data": data
                                });

                                if let Err(e) = write.send(Message::Text(call_msg.to_string())).await {
                                    let _ = response_tx.send(Err(HassWsError::SendError(e.to_string()))).await;
                                }
                            }
                            WsCommand::Ping => {
                                let ping_msg = serde_json::json!({ "type": "ping" });
                                if let Err(e) = write.send(Message::Text(ping_msg.to_string())).await {
                                    eprintln!("WebSocket ping error: {}", e);
                                }
                            }
                        }
                    }

                    // Periodic ping
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                        let ping_msg = serde_json::json!({ "type": "ping" });
                        if let Err(e) = write.send(Message::Text(ping_msg.to_string())).await {
                            eprintln!("WebSocket ping error: {}", e);
                        }
                    }
                }
            }
        });

        Ok(event_rx)
    }

    /// Handle an incoming WebSocket message.
    async fn handle_message(
        msg: Message,
        event_tx: &mpsc::Sender<HassWsEventData>,
        state: &Arc<RwLock<WsState>>,
    ) -> Result<(), HassWsError> {
        if msg.is_close() {
            let mut s = state.write().await;
            s.connected = false;
            s.authenticated = false;
            return Err(HassWsError::ConnectionClosed);
        }

        let text = msg.to_string();
        let json: JsonValue =
            serde_json::from_str(&text).map_err(|_| HassWsError::InvalidMessage)?;

        let event_type = json["type"]
            .as_str()
            .ok_or(HassWsError::InvalidMessage)?;

        match event_type {
            "event" => {
                // Parse and dispatch event
                if let Ok(event_data) = Self::parse_event(&json) {
                    let _ = event_tx.send(event_data).await;
                }
            }
            "result" => {
                // Command result - handle subscription confirmation
                if json["success"].as_bool() == Some(true) {
                    if let Some(id) = json["id"].as_u64() {
                        let mut s = state.write().await;
                        s.subscription_id = Some(id as usize);
                    }
                }
            }
            "pong" => {
                // Pong received, connection is alive
            }
            "auth_invalid" => {
                let mut s = state.write().await;
                s.authenticated = false;
                return Err(HassWsError::AuthenticationFailed);
            }
            _ => {}
        }

        Ok(())
    }

    /// Parse an event message into structured data.
    fn parse_event(json: &JsonValue) -> Result<HassWsEventData, HassWsError> {
        let event = json
            .get("event")
            .ok_or(HassWsError::InvalidMessage)?;

        let event_type = event["event_type"]
            .as_str()
            .unwrap_or("unknown");

        let data = event.get("data").cloned().unwrap_or(JsonValue::Null);

        match event_type {
            "state_changed" => {
                let entity_id = data["entity_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let old_state = data
                    .get("old_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                let new_state = data
                    .get("new_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                Ok(HassWsEventData::StateChanged {
                    entity_id,
                    old_state,
                    new_state,
                })
            }
            "device_triggered" => {
                let device_id = data["device_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                Ok(HassWsEventData::DeviceTriggered {
                    device_id,
                    data,
                })
            }
            "call_service" => {
                let domain = data["domain"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let service = data["service"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let service_data = data
                    .get("service_data")
                    .cloned()
                    .unwrap_or(JsonValue::Null);

                Ok(HassWsEventData::ServiceCalled {
                    domain,
                    service,
                    service_data,
                })
            }
            _ => Ok(HassWsEventData::RawEvent {
                event_type: event_type.to_string(),
                data,
            }),
        }
    }

    /// Subscribe to events.
    pub async fn subscribe(&self, subscription: HassSubscription) -> HassWsResult<usize> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        self.command_tx
            .send(WsCommand::Subscribe(subscription, response_tx))
            .await
            .map_err(|_| HassWsError::SendError("Command channel closed".to_string()))?;

        response_rx
            .recv()
            .await
            .ok_or(HassWsError::InvalidMessage)?
    }

    /// Subscribe to state changes for specific entities.
    pub async fn subscribe_entities(&self, entity_ids: Vec<String>) -> HassWsResult<usize> {
        self.subscribe(HassSubscription::Entities(entity_ids)).await
    }

    /// Subscribe to all state changes.
    pub async fn subscribe_all_states(&self) -> HassWsResult<usize> {
        self.subscribe(HassSubscription::AllStates).await
    }

    /// Call a service via WebSocket.
    pub async fn call_service(
        &self,
        domain: String,
        service: String,
        data: JsonValue,
    ) -> HassWsResult<JsonValue> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        self.command_tx
            .send(WsCommand::CallService {
                domain,
                service,
                data,
                response_tx,
            })
            .await
            .map_err(|_| HassWsError::SendError("Command channel closed".to_string()))?;

        response_rx
            .recv()
            .await
            .ok_or(HassWsError::InvalidMessage)?
    }

    /// Send a ping to keep the connection alive.
    pub async fn ping(&self) -> HassWsResult<()> {
        self.command_tx
            .send(WsCommand::Ping)
            .await
            .map_err(|_| HassWsError::SendError("Command channel closed".to_string()))?;
        Ok(())
    }

    /// Check if connected and authenticated.
    pub async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        state.connected && state.authenticated
    }

    /// Get connection state.
    pub async fn state(&self) -> (bool, bool) {
        let state = self.state.read().await;
        (state.connected, state.authenticated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_creation() {
        // This is just a compile-time check
        let entities = vec!["sensor.temp".to_string(), "switch.light".to_string()];
        let _ = HassSubscription::Entities(entities);
        let _ = HassSubscription::AllStates;
        let _ = HassSubscription::Devices;
        let _ = HassSubscription::Services;
    }

    #[test]
    fn test_event_data_types() {
        // Compile-time check for event types
        let _ = HassWsEventData::StateChanged {
            entity_id: "sensor.temp".to_string(),
            old_state: None,
            new_state: None,
        };

        let _ = HassWsEventData::DeviceTriggered {
            device_id: "abc123".to_string(),
            data: JsonValue::Null,
        };

        let _ = HassWsEventData::ServiceCalled {
            domain: "light".to_string(),
            service: "turn_on".to_string(),
            service_data: JsonValue::Null,
        };

        let _ = HassWsEventData::RawEvent {
            event_type: "custom".to_string(),
            data: JsonValue::Null,
        };
    }
}
