//! Event stream API handlers.
//!
//! Provides real-time event streaming via SSE and WebSocket.
//!
//! Performance optimization: Batches multiple events into single WebSocket message
//! to reduce network overhead in high-frequency scenarios.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{sse::Event, Sse},
    Json,
};
use chrono;
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

use crate::handlers::common::{ok, HandlerResult};
use crate::handlers::ServerState;
use crate::models::error::ErrorResponse;
use neomind_core::event::EventMetadata;
use neomind_core::eventbus::{EventBus, EventBusReceiver, FilteredReceiver};
use neomind_core::NeoMindEvent;

/// Batch configuration for WebSocket event streaming.
/// Reduces network overhead by sending multiple events in a single message.
struct BatchConfig {
    /// Maximum number of events to batch before sending
    batch_size: usize,
    /// Maximum time to wait before sending a partial batch
    max_delay: Duration,
    /// Events that should bypass batching (sent immediately)
    immediate_events: &'static [&'static str],
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            max_delay: Duration::from_millis(50),
            immediate_events: &[
                "AgentExecutionCompleted",
                "AgentExecutionFailed",
                "AlertCreated",
            ],
        }
    }
}

/// Heartbeat configuration for WebSocket connections.
/// Prevents connection drops due to idle timeouts.
const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60;

/// Wrapper for either filtered or unfiltered event receiver.
enum EventBusReceiverWrapper {
    Unfiltered(EventBusReceiver),
    FilteredDevice(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredRule(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredAgent(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredLlm(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredAlert(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredExtension(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
}

impl EventBusReceiverWrapper {
    async fn recv(&mut self) -> Option<(NeoMindEvent, EventMetadata)> {
        match self {
            EventBusReceiverWrapper::Unfiltered(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredDevice(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredRule(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredAgent(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredLlm(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredAlert(rx) => rx.recv().await,
            EventBusReceiverWrapper::FilteredExtension(rx) => rx.recv().await,
        }
    }
}

/// Extract event data without the nested `type` field for frontend compatibility.
///
/// The Rust enum uses `#[serde(tag = "type")]` which serializes as:
///   `{ "type": "AgentExecutionStarted", "agent_id": "...", ... }`
///
/// But the frontend expects `data` to contain just the fields without `type`:
///   `{ "agent_id": "...", "agent_name": "...", ... }`
fn extract_event_data(event: &NeoMindEvent) -> Value {
    match event {
        NeoMindEvent::AgentExecutionStarted {
            agent_id,
            agent_name,
            execution_id,
            trigger_type,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "agent_name": agent_name,
                "execution_id": execution_id,
                "trigger_type": trigger_type,
            })
        }
        NeoMindEvent::AgentExecutionCompleted {
            agent_id,
            execution_id,
            success,
            duration_ms,
            error,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "execution_id": execution_id,
                "success": success,
                "duration_ms": duration_ms,
                "error": error,
            })
        }
        NeoMindEvent::AgentThinking {
            agent_id,
            execution_id,
            step_number,
            step_type,
            description,
            details,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "execution_id": execution_id,
                "step_number": step_number,
                "step_type": step_type,
                "description": description,
                "details": details,
            })
        }
        NeoMindEvent::AgentDecision {
            agent_id,
            execution_id,
            description,
            rationale,
            action,
            confidence,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "execution_id": execution_id,
                "description": description,
                "rationale": rationale,
                "action": action,
                "confidence": confidence,
            })
        }
        NeoMindEvent::AgentMemoryUpdated {
            agent_id,
            memory_type,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "memory_type": memory_type,
            })
        }
        NeoMindEvent::AgentProgress {
            agent_id,
            execution_id,
            stage,
            stage_label,
            progress,
            details,
            ..
        } => {
            serde_json::json!({
                "agent_id": agent_id,
                "execution_id": execution_id,
                "stage": stage,
                "stage_label": stage_label,
                "progress": progress,
                "details": details,
            })
        }
        // DeviceMetric: payload must match frontend expectation (device_id, metric, value).
        // MetricValue serializes untagged (String => plain string, Float => number, etc.).
        // Json variant is serialized as a STRING to match the storage/API format:
        // storage converts MetricValue::Json → String(json_string) because
        // neomind-devices::MetricValue has no Object variant. If WS sent the
        // raw object, the frontend would receive an object via WS but a string
        // via API — causing "[object Object]" display in ValueCard.
        NeoMindEvent::DeviceMetric {
            device_id,
            metric,
            value,
            timestamp,
            quality,
            is_virtual,
        } => {
            let value_json = match value {
                neomind_core::MetricValue::Json(v) => Value::String(v.to_string()),
                _ => serde_json::to_value(value).unwrap_or(Value::Null),
            };
            let mut data = serde_json::json!({
                "device_id": device_id,
                "metric": metric,
                "value": value_json,
                "timestamp": timestamp,
                "quality": quality,
            });
            // Include is_virtual flag so frontend can skip store pollution
            // for transform/extension virtual metrics
            if *is_virtual == Some(true) {
                data["is_virtual"] = serde_json::json!(true);
            }
            data
        }
        NeoMindEvent::DeviceOnline {
            device_id,
            device_type,
            timestamp,
        } => {
            serde_json::json!({
                "device_id": device_id,
                "device_type": device_type,
                "timestamp": timestamp,
            })
        }
        NeoMindEvent::DeviceOffline {
            device_id,
            reason,
            timestamp,
        } => {
            serde_json::json!({
                "device_id": device_id,
                "reason": reason,
                "timestamp": timestamp,
            })
        }
        NeoMindEvent::DeviceTransportOnline {
            device_id,
            client_id,
            timestamp,
        } => {
            serde_json::json!({
                "device_id": device_id,
                "client_id": client_id,
                "timestamp": timestamp,
            })
        }
        NeoMindEvent::DeviceTransportOffline {
            device_id,
            client_id,
            reason,
            timestamp,
        } => {
            serde_json::json!({
                "device_id": device_id,
                "client_id": client_id,
                "reason": reason,
                "timestamp": timestamp,
            })
        }
        // ExtensionOutput: payload for extension metric/output events
        NeoMindEvent::ExtensionOutput {
            extension_id,
            output_name,
            value,
            timestamp,
            labels,
            quality,
        } => {
            serde_json::json!({
                "extension_id": extension_id,
                "output_name": output_name,
                "value": value,
                "timestamp": timestamp,
                "labels": labels,
                "quality": quality,
            })
        }
        // ExtensionLifecycle: payload for extension lifecycle events
        NeoMindEvent::ExtensionLifecycle {
            extension_id,
            state,
            message,
            timestamp,
        } => {
            serde_json::json!({
                "extension_id": extension_id,
                "state": state,
                "message": message,
                "timestamp": timestamp,
            })
        }
        // Custom events: flatten the custom event_type alongside the payload data
        // so the frontend can filter by custom_type and inspect the inner event_type
        NeoMindEvent::Custom { event_type, data } => {
            serde_json::json!({
                "custom_type": event_type,
                "data": data,
            })
        }
        // For other event types, serialize the full event (they may have the type field, but frontend handles them)
        _ => serde_json::to_value(event).unwrap_or(Value::Null),
    }
}

/// Event stream query parameters.
#[derive(Debug, Deserialize)]
pub struct EventStreamParams {
    /// Filter by event type (can be specified multiple times)
    #[serde(default)]
    pub event_type: Vec<String>,
    /// Filter by category: device, rule, llm, alert, tool
    #[serde(default)]
    pub category: Option<String>,
    /// Last event ID to resume from
    #[serde(default)]
    pub last_event_id: Option<String>,
    /// JWT authentication token
    #[serde(default)]
    pub token: Option<String>,
    /// API key authentication (for remote instance access)
    #[serde(default)]
    pub api_key: Option<String>,
}

/// POST /api/events
///
/// Publish a custom event to the event bus.
/// Requires authentication (API key or JWT).
#[derive(Debug, Deserialize)]
pub struct PublishEventRequest {
    /// Event type identifier (e.g., "my_extension.my_event")
    pub event_type: String,
    /// Event payload as JSON
    #[serde(default)]
    pub data: serde_json::Value,
    /// Optional source identifier
    #[serde(default)]
    pub source: Option<String>,
}

pub async fn publish_event_handler(
    State(state): State<ServerState>,
    Json(req): Json<PublishEventRequest>,
) -> HandlerResult<serde_json::Value> {
    // Validate event type
    if req.event_type.is_empty() || req.event_type.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "event_type must be 1-256 characters",
        ));
    }
    if req.event_type.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "event_type contains invalid characters",
        ));
    }

    // Validate payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.data).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Event payload too large (max 1 MB)",
            ));
        }
    }

    let event_bus = state
        .event_bus()
        .ok_or_else(|| ErrorResponse::internal("Event bus not available"))?;

    let event = NeoMindEvent::Custom {
        event_type: req.event_type.clone(),
        data: req.data,
    };

    let source = req.source.unwrap_or_else(|| "api".to_string());

    let published = event_bus.publish_with_source(event, &source).await;
    if !published {
        tracing::warn!(
            event_type = %req.event_type,
            "Event was not delivered (no active subscribers)"
        );
    }

    ok(serde_json::json!({
        "success": true,
        "event_type": req.event_type,
        "source": source,
    }))
}

/// SSE endpoint for streaming events.
///
/// Streams real-time events from the event bus using Server-Sent Events.
/// Clients can filter by event type or category.
/// Supports JWT token (`?token=xxx`) or API key (`?api_key=xxx`) authentication.
pub async fn event_stream_handler(
    State(state): State<ServerState>,
    Query(params): Query<EventStreamParams>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::Error>>>, StatusCode> {
    // Validate authentication: JWT token or API key
    let authenticated = if let Some(token) = params.token.as_ref() {
        state.auth.user_state.validate_token(token).is_ok()
    } else if let Some(api_key) = params.api_key.as_ref() {
        state.auth.api_key_state.validate_key(api_key)
    } else {
        false
    };

    if !authenticated {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Get the event bus from the server state
    let event_bus = state.core.event_bus.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    // Create a receiver for events
    let rx = create_filtered_receiver(event_bus, &params.category);

    // Create the SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        let mut _counter: u64 = 0;  // Event counter (reserved for future metrics)

        while let Some((event, metadata)) = rx.recv().await {
            // Apply event type filter if specified
            if !params.event_type.is_empty() {
                let event_type = event.type_name().to_string();
                if !params.event_type.contains(&event_type) {
                    continue;
                }
            }

            _counter += 1;

            // Build SSE event with all data
            // Use extract_event_data to remove nested type field for frontend compatibility
            let data_with_id = serde_json::json!({
                "id": metadata.event_id,
                "type": event.type_name(),
                "timestamp": event.timestamp(),
                "source": metadata.source,
                "data": extract_event_data(&event),
            });

            let sse_event = Event::default()
                .event(event.type_name())
                .json_data(data_with_id)
                .unwrap_or_else(|_| Event::default().data(""));

            yield Ok(sse_event);
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("keepalive"),
    ))
}

/// Create a filtered receiver based on category.
fn create_filtered_receiver(
    event_bus: &EventBus,
    category: &Option<String>,
) -> EventBusReceiverWrapper {
    match category.as_deref() {
        Some("device") => {
            EventBusReceiverWrapper::FilteredDevice(event_bus.filter().device_events())
        }
        Some("rule") => EventBusReceiverWrapper::FilteredRule(event_bus.filter().rule_events()),
        Some("agent") => EventBusReceiverWrapper::FilteredAgent(event_bus.filter().agent_events()),
        Some("llm") => EventBusReceiverWrapper::FilteredLlm(event_bus.filter().llm_events()),
        Some("alert") => EventBusReceiverWrapper::FilteredAlert(event_bus.filter().alert_events()),
        Some("extension") => {
            EventBusReceiverWrapper::FilteredExtension(event_bus.filter().extension_events())
        }
        Some("tool") => {
            // Custom filter for tool events - use the FilteredDevice variant as they have the same type
            EventBusReceiverWrapper::FilteredDevice(
                event_bus.filter().custom(|e| e.is_tool_event()),
            )
        }
        Some("all") | None => EventBusReceiverWrapper::Unfiltered(event_bus.subscribe()),
        _ => EventBusReceiverWrapper::Unfiltered(event_bus.subscribe()),
    }
}

/// WebSocket endpoint for event streaming.
///
/// Alternative to SSE using WebSocket for bidirectional communication.
/// Authentication is done via Auth message after connection is established
/// (more secure than putting token in URL parameter).
pub async fn event_websocket_handler(
    State(state): State<ServerState>,
    ws: WebSocketUpgrade,
    Query(params): Query<EventStreamParams>,
) -> axum::response::Response {
    let event_bus = match state.core.event_bus.as_ref() {
        Some(bus) => bus.clone(),
        None => {
            return ws.on_upgrade(|mut socket| async move {
                use axum::extract::ws::Message;
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({"type": "Error", "message": "Event bus not available"})
                            .to_string(),
                    ))
                    .await;
                let _ = socket.close().await;
            });
        }
    };

    ws.on_upgrade(move |mut socket| async move {
        use axum::extract::ws::Message;

        let mut rx = create_filtered_receiver(&event_bus, &params.category);
        let auth_user_state = state.auth.user_state.clone();
        let auth_api_key_state = state.auth.api_key_state.clone();

        // Check if API key was provided in query params — skip Auth message flow
        let pre_authenticated = params
            .api_key
            .as_ref()
            .is_some_and(|key| auth_api_key_state.validate_key(key));

        let authenticated = if pre_authenticated {
            let _ = socket.send(Message::Text(
                serde_json::json!({"type": "Authenticated", "message": "Authentication successful"}).to_string()
            )).await;
            true
        } else {
            // Wait for Auth message (JWT token). Track success explicitly so that
            // a natural loop exit (client half-closed / only sent Binary frames)
            // cannot fall through into the event-sending loop unauthenticated.
            let mut success = false;
            while let Some(msg) = socket.recv().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                            if data["type"] == "Auth" {
                                // Try JWT token
                                if let Some(token) = data["token"].as_str() {
                                    match auth_user_state.validate_token(token) {
                                        Ok(_) => {
                                            tracing::info!("WebSocket event stream authenticated");
                                            let _ = socket.send(Message::Text(
                                                serde_json::json!({"type": "Authenticated", "message": "Authentication successful"}).to_string()
                                            )).await;
                                            success = true;
                                            break;
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "JWT validation failed, rejecting WebSocket connection");
                                            let _ = socket.send(Message::Text(
                                                serde_json::json!({"type": "Error", "message": "Invalid or expired token"}).to_string()
                                            )).await;
                                            let _ = socket.close().await;
                                            return;
                                        }
                                    }
                                }
                                // Try API key via Auth message
                                if let Some(api_key) = data["api_key"].as_str() {
                                    if auth_api_key_state.validate_key(api_key) {
                                        tracing::info!("WebSocket event stream authenticated via API key");
                                        let _ = socket.send(Message::Text(
                                            serde_json::json!({"type": "Authenticated", "message": "Authentication successful"}).to_string()
                                        )).await;
                                        success = true;
                                        break;
                                    } else {
                                        tracing::warn!("Invalid API key, rejecting WebSocket connection");
                                        let _ = socket.send(Message::Text(
                                            serde_json::json!({"type": "Error", "message": "Invalid API key"}).to_string()
                                        )).await;
                                        let _ = socket.close().await;
                                        return;
                                    }
                                }
                            }
                        }

                        // If not authenticated after first message, close connection
                        tracing::warn!("No valid auth message received, closing WebSocket connection");
                        let _ = socket.send(Message::Text(
                            serde_json::json!({"type": "Error", "message": "Authentication required"})
                                .to_string(),
                        )).await;
                        let _ = socket.close().await;
                        return;
                    }
                    Ok(Message::Close(_)) | Err(_) => {
                        return;
                    }
                    _ => {}
                }
            }
            success
        };

        // Guard: never enter the event-sending loop without successful auth.
        // Covers the case where the while loop exited naturally (recv returned
        // None because the client half-closed or only sent non-Text frames).
        if !authenticated {
            tracing::warn!("WebSocket event stream closed before authentication completed");
            let _ = socket.close().await;
            return;
        }

        // Send events to the authenticated WebSocket client
        // Performance optimization: Batch events to reduce network overhead
        let config = BatchConfig::default();
        let mut event_buffer: Vec<Value> = Vec::with_capacity(config.batch_size);
        let mut last_flush = tokio::time::Instant::now();

        // Create a ticker for periodic flushing
        let mut flush_interval = tokio::time::interval(config.max_delay);

        // Heartbeat mechanism to keep connection alive
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        let mut last_pong = tokio::time::Instant::now();
        let heartbeat_timeout = Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);

        loop {
            tokio::select! {
                // Handle incoming messages (for pong responses)
                msg_result = socket.recv() => {
                    match msg_result {
                        Some(Ok(Message::Text(text))) => {
                            // Check for pong response
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                if value.get("type") == Some(&serde_json::json!("pong")) {
                                    last_pong = tokio::time::Instant::now();
                                    tracing::debug!("Received pong from events WebSocket client");
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                            tracing::debug!("Events WebSocket client disconnected");
                            return;
                        }
                        _ => {}
                    }
                }

                // Heartbeat - send periodic ping to detect dead connections
                _ = heartbeat_interval.tick() => {
                    // Check for heartbeat timeout
                    if last_pong.elapsed() > heartbeat_timeout {
                        tracing::warn!(
                            "Events WebSocket heartbeat timeout - no pong received for {:?}",
                            last_pong.elapsed()
                        );
                        let _ = socket.send(Message::Text(
                            serde_json::json!({"type": "Error", "message": "Heartbeat timeout"}).to_string()
                        )).await;
                        break;
                    }

                    // Send ping
                    let ping = serde_json::json!({
                        "type": "ping",
                        "timestamp": chrono::Utc::now().timestamp(),
                    });
                    if socket.send(Message::Text(ping.to_string())).await.is_err() {
                        tracing::debug!("Failed to send ping, client disconnected");
                        break;
                    }
                    tracing::debug!("Sent ping to events WebSocket client");
                }

                // Receive new events
                recv_result = rx.recv() => {
                    match recv_result {
                        Some((event, metadata)) => {
                            // Apply event type filter. type_name() returns &'static
                            // str, so compare directly without allocating a String
                            // and without calling type_name() twice per event.
                            let event_type = event.type_name();
                            if !params.event_type.is_empty()
                                && !params.event_type.iter().any(|t| t.as_str() == event_type)
                            {
                                continue;
                            }
                            let payload = serde_json::json!({
                                "id": metadata.event_id,
                                "type": event_type,
                                "timestamp": event.timestamp(),
                                "source": metadata.source,
                                "data": extract_event_data(&event),
                            });

                            // Check if this event should be sent immediately
                            let should_send_immediately = config.immediate_events.contains(&event_type);

                            if should_send_immediately {
                                // Flush any buffered events first
                                if !event_buffer.is_empty() {
                                    let batch_msg = serde_json::json!({ "batch": true, "events": event_buffer });
                                    if let Ok(json) = serde_json::to_string(&batch_msg) {
                                        let _ = socket.send(Message::Text(json)).await;
                                    }
                                    event_buffer.clear();
                                }

                                // Send immediate event
                                let msg = match serde_json::to_string(&payload) {
                                    Ok(json) => Message::Text(json),
                                    Err(e) => {
                                        tracing::warn!(error = %e, "Failed to serialize event for WebSocket");
                                        continue;
                                    }
                                };

                                if socket.send(msg).await.is_err() {
                                    break;
                                }
                            } else {
                                // Add to buffer for batching
                                event_buffer.push(payload);

                                // Check if buffer is full
                                if event_buffer.len() >= config.batch_size {
                                    let batch_msg = serde_json::json!({ "batch": true, "events": event_buffer });
                                    if let Ok(json) = serde_json::to_string(&batch_msg) {
                                        if socket.send(Message::Text(json)).await.is_err() {
                                            break;
                                        }
                                    }
                                    event_buffer.clear();
                                    last_flush = tokio::time::Instant::now();
                                }
                            }
                        }
                        None => {
                            // Channel closed, flush remaining events and exit
                            if !event_buffer.is_empty() {
                                let batch_msg = serde_json::json!({ "batch": true, "events": event_buffer });
                                if let Ok(json) = serde_json::to_string(&batch_msg) {
                                    let _ = socket.send(Message::Text(json)).await;
                                }
                            }
                            break;
                        }
                    }
                }
                // Periodic flush of buffered events
                _ = flush_interval.tick() => {
                    if !event_buffer.is_empty() && last_flush.elapsed() >= config.max_delay {
                        let batch_msg = serde_json::json!({ "batch": true, "events": event_buffer });
                        if let Ok(json) = serde_json::to_string(&batch_msg) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                        event_buffer.clear();
                        last_flush = tokio::time::Instant::now();
                    }
                }
            }
        }

        let _ = socket.close().await;
    })
}
