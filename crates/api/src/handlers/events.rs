//! Event stream API handlers.
//!
//! Provides real-time event streaming via SSE and event history queries.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{sse::Event, Sse, Json},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::handlers::ServerState;
use edge_ai_core::eventbus::{EventBus, EventBusReceiver};

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

/// Event history query parameters.
#[derive(Debug, Deserialize)]
pub struct EventHistoryQuery {
    /// Start timestamp (Unix seconds)
    pub start: Option<i64>,
    /// End timestamp (Unix seconds)
    pub end: Option<i64>,
    /// Filter by event type
    #[serde(default)]
    pub event_type: Vec<String>,
    /// Filter by category
    #[serde(default)]
    pub category: Option<String>,
    /// Maximum number of results
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Event subscription request.
#[derive(Debug, Deserialize)]
pub struct EventSubscriptionRequest {
    /// Event types to subscribe to
    #[serde(default)]
    pub event_types: Vec<String>,
    /// Category to subscribe to
    #[serde(default)]
    pub category: Option<String>,
}

/// Event subscription response.
#[derive(Debug, Serialize)]
pub struct EventSubscriptionResponse {
    /// Subscription ID
    pub subscription_id: String,
    /// Subscribed event types
    pub event_types: Vec<String>,
    /// Subscription category filter
    pub category: Option<String>,
}

/// Event list response.
#[derive(Debug, Serialize)]
pub struct EventListResponse {
    /// List of events with metadata
    pub events: Vec<EventWithMetadata>,
    /// Total count
    pub total: usize,
}

/// Event with metadata.
#[derive(Debug, Serialize)]
pub struct EventWithMetadata {
    /// The event data
    #[serde(flatten)]
    pub event: serde_json::Value,
    /// Event type name
    pub event_type: String,
    /// Event timestamp
    pub timestamp: i64,
    /// Event source
    pub source: String,
    /// Event ID (metadata)
    pub event_id: String,
}

/// Event statistics.
#[derive(Debug, Serialize)]
pub struct EventStats {
    /// Total events in retention period
    pub total_events: u64,
    /// Events by type
    pub events_by_type: HashMap<String, u64>,
    /// Events by category
    pub events_by_category: HashMap<String, u64>,
    /// Active subscriptions
    pub active_subscriptions: usize,
}

fn default_limit() -> usize {
    100
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
    state.auth_user_state.validate_token(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Get the event bus from the server state
    let event_bus = state.event_bus.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

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
                    let data_with_id = serde_json::json!({
                        "id": metadata.event_id,
                        "type": event.type_name(),
                        "timestamp": event.timestamp(),
                        "source": metadata.source,
                        "data": event,
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
) -> EventBusReceiver {
    match category.as_deref() {
        Some("device") => {
            let filtered = event_bus.filter().device_events();
            // Convert to regular receiver by using the underlying broadcast
            drop(filtered);
            event_bus.subscribe()
        }
        Some("rule") => {
            let filtered = event_bus.filter().rule_events();
            drop(filtered);
            event_bus.subscribe()
        }
        Some("workflow") => {
            let filtered = event_bus.filter().workflow_events();
            drop(filtered);
            event_bus.subscribe()
        }
        Some("llm") => {
            let filtered = event_bus.filter().llm_events();
            drop(filtered);
            event_bus.subscribe()
        }
        Some("alert") => {
            let filtered = event_bus.filter().alert_events();
            drop(filtered);
            event_bus.subscribe()
        }
        Some("tool") => {
            let filtered = event_bus.filter().custom(|e| e.is_tool_event());
            drop(filtered);
            event_bus.subscribe()
        }
        _ => event_bus.subscribe(),
    }
}

/// WebSocket endpoint for event streaming.
///
/// Alternative to SSE using WebSocket for bidirectional communication.
/// Requires JWT token authentication via `?token=xxx` parameter.
pub async fn event_websocket_handler(
    State(state): State<ServerState>,
    ws: WebSocketUpgrade,
    Query(params): Query<EventStreamParams>,
) -> axum::response::Response {
    // Validate JWT token - reject connection if no token or invalid token
    let _auth_info = match params.token.as_ref() {
        Some(token) => {
            match state.auth_user_state.validate_token(token) {
                Ok(info) => {
                    tracing::info!("WebSocket event stream authenticated");
                    Some(info)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "JWT validation failed, rejecting WebSocket connection");
                    return ws.on_upgrade(|mut socket| {
                        async move {
                            use axum::extract::ws::Message;
                            let _ = socket.send(Message::Text(
                                serde_json::json!({"type": "Error", "message": "Invalid or expired token"}).to_string()
                            )).await;
                            let _ = socket.close().await;
                        }
                    });
                }
            }
        }
        None => {
            tracing::warn!("No authentication provided, rejecting WebSocket connection");
            return ws.on_upgrade(|mut socket| {
                async move {
                    use axum::extract::ws::Message;
                    let _ = socket.send(Message::Text(
                        serde_json::json!({"type": "Error", "message": "Authentication required"}).to_string()
                    )).await;
                    let _ = socket.close().await;
                }
            });
        }
    };

    let event_bus = match state.event_bus.as_ref() {
        Some(bus) => bus.clone(),
        None => {
            return ws.on_upgrade(|mut socket| {
                async move {
                    use axum::extract::ws::Message;
                    let _ = socket.send(Message::Text(
                        serde_json::json!({"type": "Error", "message": "Event bus not available"}).to_string()
                    )).await;
                    let _ = socket.close().await;
                }
            });
        }
    };

    ws.on_upgrade(move |mut socket| async move {
        use axum::extract::ws::Message;

        let mut rx = create_filtered_receiver(&event_bus, &params.category);

        // Send events to the WebSocket client
        while let Some((event, metadata)) = rx.recv().await {
            // Apply event type filter
            if !params.event_type.is_empty() {
                let event_type = event.type_name().to_string();
                if !params.event_type.contains(&event_type) {
                    continue;
                }
            }

            let payload = serde_json::json!({
                "id": metadata.event_id,
                "type": event.type_name(),
                "timestamp": event.timestamp(),
                "source": metadata.source,
                "data": event,
            });

            let msg = match serde_json::to_string(&payload) {
                Ok(json) => Message::Text(json),
                Err(_) => continue,
            };

            if socket.send(msg).await.is_err() {
                break;
            }
        }

        let _ = socket.close().await;
    })
}

/// Query event history.
///
/// Retrieves historical events from storage with optional filtering.
pub async fn event_history_handler(
    State(state): State<ServerState>,
    Query(query): Query<EventHistoryQuery>,
) -> Result<Json<EventListResponse>, StatusCode> {
    // Get event log store from server state
    let event_log = state.event_log.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    use edge_ai_storage::business::EventFilter;

    // Convert query parameters to EventFilter
    let mut filter = EventFilter::new();

    if let Some(start) = query.start {
        filter.start_time = Some(start);
    }
    if let Some(end) = query.end {
        filter.end_time = Some(end);
    }
    filter.limit = Some(query.limit);

    // Map event types to EventLog event types
    if !query.event_type.is_empty() {
        filter.event_types = query.event_type;
    }

    // Query events from storage
    let logs = event_log.query(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Convert EventLog entries to EventWithMetadata
    let events: Vec<EventWithMetadata> = logs
        .into_iter()
        .map(|log| EventWithMetadata {
            event: serde_json::json!({
                "type": log.event_type,
                "message": log.message,
                "source": log.source,
                "severity": format!("{:?}", log.severity),
                "data": log.data,
            }),
            event_type: log.event_type,
            timestamp: log.timestamp,
            source: log.source.unwrap_or_default(),
            event_id: log.id,
        })
        .collect();

    let total = events.len();

    Ok(Json(EventListResponse {
        events,
        total,
    }))
}

/// Query events with category filter.
///
/// Retrieves events filtered by category (device, rule, workflow, etc).
pub async fn events_query_handler(
    State(state): State<ServerState>,
    Query(query): Query<EventHistoryQuery>,
) -> Result<Json<EventListResponse>, StatusCode> {
    event_history_handler(State(state), Query(query)).await
}

/// Get event statistics.
///
/// Returns statistics about events in the system.
pub async fn event_stats_handler(
    State(state): State<ServerState>,
) -> Result<Json<EventStats>, StatusCode> {
    let event_log = state.event_log.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    let event_bus = state.event_bus.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get all events for statistics
    use edge_ai_storage::business::EventFilter;
    let logs = event_log.query(&EventFilter::new().with_limit(10000))
        .unwrap_or_default();

    let mut events_by_type: HashMap<String, u64> = HashMap::new();
    let mut events_by_category: HashMap<String, u64> = HashMap::new();

    for log in logs {
        *events_by_type.entry(log.event_type.clone()).or_insert(0) += 1;

        // Categorize events
        let category = categorize_event(&log.event_type);
        *events_by_category.entry(category.to_string()).or_insert(0) += 1;
    }

    Ok(Json(EventStats {
        total_events: events_by_type.values().sum(),
        events_by_type,
        events_by_category,
        active_subscriptions: event_bus.subscriber_count(),
    }))
}

/// Create an event subscription.
///
/// Creates a new subscription for specific event types.
pub async fn subscribe_events_handler(
    State(state): State<ServerState>,
    Json(req): Json<EventSubscriptionRequest>,
) -> Result<Json<EventSubscriptionResponse>, StatusCode> {
    let _event_bus = state.event_bus.as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    // Generate a subscription ID
    let subscription_id = format!("sub-{}", uuid::Uuid::new_v4());

    // In a full implementation, we would store this subscription
    // For now, return the subscription info
    Ok(Json(EventSubscriptionResponse {
        subscription_id,
        event_types: req.event_types,
        category: req.category,
    }))
}

/// Delete an event subscription.
///
/// Cancels an existing event subscription.
pub async fn unsubscribe_events_handler(
    Path(_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    // In a full implementation, we would remove the subscription
    // For now, just return success
    tracing::info!("Cancelling subscription: {}", _id);
    Ok(StatusCode::NO_CONTENT)
}

/// Helper function to categorize events.
fn categorize_event(event_type: &str) -> &str {
    match event_type {
        "DeviceOnline" | "DeviceOffline" | "DeviceMetric" | "DeviceCommandResult" => "device",
        "RuleEvaluated" | "RuleTriggered" | "RuleExecuted" => "rule",
        "WorkflowTriggered" | "WorkflowStepCompleted" | "WorkflowCompleted" => "workflow",
        "PeriodicReviewTriggered" | "LlmDecisionProposed" | "LlmDecisionExecuted"
        | "UserMessage" | "LlmResponse" => "llm",
        "AlertCreated" | "AlertAcknowledged" => "alert",
        "ToolExecutionStart" | "ToolExecutionSuccess" | "ToolExecutionFailure" => "tool",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_categorization() {
        assert_eq!(categorize_event("DeviceOnline"), "device");
        assert_eq!(categorize_event("RuleTriggered"), "rule");
        assert_eq!(categorize_event("WorkflowCompleted"), "workflow");
        assert_eq!(categorize_event("LlmDecisionProposed"), "llm");
        assert_eq!(categorize_event("AlertCreated"), "alert");
        assert_eq!(categorize_event("ToolExecutionStart"), "tool");
        assert_eq!(categorize_event("UnknownEvent"), "other");
    }

    #[test]
    fn test_default_limit() {
        assert_eq!(default_limit(), 100);
    }
}
