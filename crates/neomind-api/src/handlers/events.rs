//! Event stream API handlers.
//!
//! Provides real-time event streaming via SSE and WebSocket.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{sse::Event, Sse},
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::Value;

use crate::handlers::ServerState;
use neomind_core::event::EventMetadata;
use neomind_core::eventbus::{EventBus, EventBusReceiver, FilteredReceiver};
use neomind_core::NeoMindEvent;

/// Wrapper for either filtered or unfiltered event receiver.
enum EventBusReceiverWrapper {
    Unfiltered(EventBusReceiver),
    FilteredDevice(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredRule(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
    FilteredWorkflow(FilteredReceiver<fn(&NeoMindEvent) -> bool>),
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
            EventBusReceiverWrapper::FilteredWorkflow(rx) => rx.recv().await,
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
        NeoMindEvent::DeviceMetric {
            device_id,
            metric,
            value,
            timestamp,
            quality,
            ..
        } => {
            serde_json::json!({
                "device_id": device_id,
                "metric": metric,
                "value": value,
                "timestamp": timestamp,
                "quality": quality,
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
    /// Filter by category: device, rule, workflow, llm, alert, tool
    #[serde(default)]
    pub category: Option<String>,
    /// Last event ID to resume from
    #[serde(default)]
    pub last_event_id: Option<String>,
    /// JWT authentication token
    #[serde(default)]
    pub token: Option<String>,
}

/// SSE endpoint for streaming events.
///
/// Streams real-time events from the event bus using Server-Sent Events.
/// Clients can filter by event type or category.
/// Requires JWT token authentication via `?token=xxx` parameter.
pub async fn event_stream_handler(
    State(state): State<ServerState>,
    Query(params): Query<EventStreamParams>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::Error>>>, StatusCode> {
    // Validate JWT token - must be provided
    let token = params.token.as_ref().ok_or(StatusCode::UNAUTHORIZED)?;
    state
        .auth
        .user_state
        .validate_token(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Get the event bus from the server state
    let event_bus = state.core.event_bus.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    // Create a receiver for events
    let rx = create_filtered_receiver(event_bus, &params.category);

    // Create the SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        let mut _counter: u64 = 0;  // Event counter (reserved for future metrics)

        loop {
            match rx.recv().await {
                Some((event, metadata)) => {
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
                None => {
                    // Channel closed
                    break;
                }
            }
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
        Some("workflow") => {
            EventBusReceiverWrapper::FilteredWorkflow(event_bus.filter().workflow_events())
        }
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
        let mut authenticated = false;
        let auth_user_state = state.auth.user_state.clone();

        // First, wait for authentication message
        while let Some(msg) = socket.recv().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Handle authentication message
                    if !authenticated {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                            if data["type"] == "Auth" {
                                if let Some(token) = data["token"].as_str() {
                                    match auth_user_state.validate_token(token) {
                                        Ok(_) => {
                                            authenticated = true;
                                            tracing::info!("WebSocket event stream authenticated");
                                            let _ = socket.send(Message::Text(
                                                serde_json::json!({"type": "Authenticated", "message": "Authentication successful"}).to_string()
                                            )).await;

                                            // Break out of recv loop to start sending events
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
                }
                Ok(Message::Close(_)) | Err(_) => {
                    return;
                }
                _ => {}
            }
        }

        // Send events to the authenticated WebSocket client
        while let Some((event, metadata)) = rx.recv().await {
            // Apply event type filter
            if !params.event_type.is_empty() {
                let event_type = event.type_name().to_string();
                if !params.event_type.contains(&event_type) {
                    continue;
                }
            }

            let event_type = event.type_name();
            let payload = serde_json::json!({
                "id": metadata.event_id,
                "type": event_type,
                "timestamp": event.timestamp(),
                "source": metadata.source,
                "data": extract_event_data(&event),
            });

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
        }

        let _ = socket.close().await;
    })
}
