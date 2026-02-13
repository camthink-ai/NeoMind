//! Session management handlers.

use super::ws::create_connection_metadata;

use axum::extract::ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade};
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use neomind_agent::AgentEvent;
use neomind_storage::{PendingStreamState, StreamStage};

/// Stream event sent from the LLM processing task to the WebSocket handler.
#[derive(Debug, Clone)]
struct StreamEvent {
    json: String,
    #[allow(dead_code)]
    session_id: String,
}

/// Process the LLM stream in a spawned task and send events through a channel.
///
/// This function runs asynchronously and doesn't block the WebSocket event loop,
/// allowing ping/pong frames to be handled properly.
async fn process_stream_to_channel(
    mut stream: Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    session_id: String,
    user_message: String,
    tx: mpsc::UnboundedSender<StreamEvent>,
    state: super::ServerState,
) {
    let mut end_event_sent = false;
    let mut event_count = 0u32;

    // Clone user_message for later use in memory consolidation
    let user_message_for_memory = user_message.clone();

    // P0.3: Create pending stream state for recovery
    let session_store = state.agents.session_manager.session_store();
    let mut pending_state = PendingStreamState::new(session_id.clone(), user_message);
    let _ = session_store.save_pending_stream(&pending_state);

    // Track stream start time for progress reporting
    let stream_start = std::time::Instant::now();

    // Stream timeout: 300 seconds (5 minutes) to support thinking models
    // This is synchronized with StreamConfig::max_stream_duration_secs
    // qwen3-vl:2b with extended thinking can take significant time for complex queries
    // with image analysis or multi-step reasoning.
    let stream_timeout = Duration::from_secs(300);
    let max_duration_secs = 300u64;

    loop {
        let next_event = tokio::time::timeout(stream_timeout, StreamExt::next(&mut stream)).await;

        match next_event {
            Ok(Some(event)) => {
                event_count += 1;
                let event_json = match &event {
                    AgentEvent::Thinking { content } => {
                        // P0.3: Update pending state with thinking content
                        pending_state.update_thinking(content);
                        let _ = session_store.save_pending_stream(&pending_state);

                        tracing::debug!(
                            "Sending Thinking event: {} chars",
                            content.chars().count()
                        );
                        json!({
                            "type": "Thinking",
                            "content": content,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Content { content } => {
                        // P0.3: Update pending state with content
                        pending_state.update_content(content);
                        pending_state.set_stage(StreamStage::Generating);
                        let _ = session_store.save_pending_stream(&pending_state);

                        json!({
                            "type": "Content",
                            "content": content,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::ToolCallStart { tool, arguments } => {
                        json!({
                            "type": "ToolCallStart",
                            "tool": tool,
                            "arguments": arguments,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::ToolCallEnd {
                        tool,
                        result,
                        success,
                    } => {
                        json!({
                            "type": "ToolCallEnd",
                            "tool": tool,
                            "result": result,
                            "success": success,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Error { message } => {
                        json!({
                            "type": "Error",
                            "message": message,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Intent {
                        category,
                        display_name,
                        confidence,
                        keywords,
                    } => {
                        json!({
                            "type": "Intent",
                            "category": category,
                            "displayName": display_name,
                            "confidence": confidence,
                            "keywords": keywords,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Plan { step, stage } => {
                        json!({
                            "type": "Plan",
                            "step": step,
                            "stage": stage,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::IntermediateEnd => {
                        // Intermediate end - for multi-round tool calling
                        // This indicates the current round is complete but more processing is coming
                        // Send this to frontend but don't exit the loop
                        tracing::debug!(
                            "*** Sending IntermediateEnd event (round {}) ***",
                            event_count
                        );
                        json!({
                            "type": "intermediate_end",
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::End => {
                        // P0.3: Delete pending state on successful completion
                        let _ = session_store.delete_pending_stream(&session_id);

                        // Auto-consolidate conversation to tiered memory
                        // This stores the conversation in mid-term memory for future retrieval
                        let assistant_response = pending_state.content.clone();
                        let thinking = if !pending_state.thinking.is_empty() {
                            Some(pending_state.thinking.clone())
                        } else {
                            None
                        };

                        // Spawn a background task to consolidate to memory with timeout
                        let session_id_clone = session_id.clone();
                        let user_message_clone = user_message_for_memory.clone();
                        let state_clone = state.clone();
                        let assistant_response_clone = assistant_response.clone();
                        let thinking_clone = thinking.clone();

                        tokio::spawn(async move {
                            // Memory consolidation timeout: 5 seconds
                            // Prevents background tasks from hanging indefinitely
                            let consolidate_timeout = Duration::from_secs(5);

                            let consolidate_result = tokio::time::timeout(
                                consolidate_timeout,
                                state_clone
                                    .agents
                                    .memory
                                    .write()
                                    .await
                                    .consolidate(&session_id_clone),
                            )
                            .await;

                            match consolidate_result {
                                Ok(Ok(())) => {
                                    // Also directly add to mid-term memory for immediate retrieval
                                    let response_with_thinking =
                                        if let Some(thinking_content) = thinking_clone {
                                            if !thinking_content.is_empty() {
                                                format!(
                                                    "{}\n\nThinking: {}",
                                                    assistant_response_clone, thinking_content
                                                )
                                            } else {
                                                assistant_response_clone.clone()
                                            }
                                        } else {
                                            assistant_response_clone.clone()
                                        };

                                    let add_result = tokio::time::timeout(
                                        Duration::from_secs(2),
                                        state_clone.agents.memory.write().await.add_conversation(
                                            &session_id_clone,
                                            &user_message_clone,
                                            &response_with_thinking,
                                        ),
                                    )
                                    .await;

                                    match add_result {
                                        Ok(Ok(())) => {
                                            tracing::debug!(
                                                "Conversation consolidated to memory: session={}",
                                                session_id_clone
                                            );
                                        }
                                        Ok(Err(e)) => {
                                            tracing::warn!(
                                                "Failed to add conversation to mid-term memory: {}",
                                                e
                                            );
                                        }
                                        Err(_) => {
                                            tracing::warn!(
                                                "Timeout adding conversation to mid-term memory: session={}",
                                                session_id_clone
                                            );
                                        }
                                    }
                                }
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        "Failed to consolidate short-term to mid-term memory: {}",
                                        e
                                    );
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        "Timeout consolidating memory for session={}",
                                        session_id_clone
                                    );
                                }
                            }
                        });

                        tracing::info!("*** Sending End event (total events: {}) ***", event_count);
                        end_event_sent = true;
                        json!({
                            "type": "end",
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Progress {
                        message,
                        stage,
                        elapsed_ms,
                        ..
                    } => {
                        let elapsed = elapsed_ms
                            .unwrap_or_else(|| stream_start.elapsed().as_millis() as u64)
                            / 1000;
                        let remaining = max_duration_secs.saturating_sub(elapsed);
                        tracing::debug!(
                            "Sending Progress event: {} ({})",
                            message,
                            stage.as_deref().unwrap_or("unknown")
                        );
                        json!({
                            "type": "Progress",
                            "message": message,
                            "stage": stage,
                            "elapsed": elapsed,
                            "remainingTime": remaining,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Heartbeat { timestamp } => {
                        json!({
                            "type": "Heartbeat",
                            "timestamp": timestamp,
                            "sessionId": session_id,
                        })
                    }
                    AgentEvent::Warning { message } => {
                        let elapsed = stream_start.elapsed().as_secs();
                        let remaining = max_duration_secs.saturating_sub(elapsed);
                        tracing::debug!("Sending Warning event: {}", message);
                        json!({
                            "type": "Warning",
                            "message": message,
                            "elapsed": elapsed,
                            "remainingTime": remaining,
                            "sessionId": session_id,
                        })
                    }
                };

                let stream_event = StreamEvent {
                    json: event_json.to_string(),
                    session_id: session_id.clone(),
                };

                // Try to send, but don't block if channel is closed
                if tx.send(stream_event).is_err() {
                    tracing::warn!("Failed to send stream event through channel");
                    break;
                }

                // If this was the End event, exit the loop
                // (IntermediateEnd does NOT exit the loop - more events are coming)
                if matches!(event, AgentEvent::End) {
                    break;
                }
            }
            Ok(None) => {
                // Stream ended naturally
                tracing::debug!("Stream ended naturally (total events: {})", event_count);
                // Send End event if not already sent
                if !end_event_sent {
                    let end_json = json!({
                        "type": "end",
                        "sessionId": session_id,
                    });
                    let _ = tx.send(StreamEvent {
                        json: end_json.to_string(),
                        session_id: session_id.clone(),
                    });
                }
                break;
            }
            Err(_) => {
                // Timeout occurred - CRITICAL: clean up pending state to prevent memory leak
                tracing::warn!(
                    "Stream timeout after {:?} - cleaning up pending state",
                    stream_timeout
                );
                let _ = session_store.delete_pending_stream(&session_id);

                let timeout_json = json!({
                    "type": "Error",
                    "message": "Stream timeout: response took too long",
                    "sessionId": session_id,
                });
                let _ = tx.send(StreamEvent {
                    json: timeout_json.to_string(),
                    session_id: session_id.clone(),
                });
                // Send end event after timeout
                if !end_event_sent {
                    let end_json = json!({
                        "type": "end",
                        "sessionId": session_id,
                    });
                    let _ = tx.send(StreamEvent {
                        json: end_json.to_string(),
                        session_id: session_id.clone(),
                    });
                }
                break;
            }
        }
    }

    // Persist history after stream completes
    if let Err(e) = state
        .agents
        .session_manager
        .persist_history(&session_id)
        .await
    {
        tracing::warn!(category = "session", error = %e, "Failed to persist history");
    }
}
use crate::models::{
    common::ApiResponse, pagination::Pagination, ChatRequest, ChatResponse, ErrorResponse,
};

use super::ServerState;

/// Heartbeat interval for WebSocket connections (seconds)
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Session list item.
#[derive(Debug, Clone, Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub message_count: usize,
    pub created_at: String,
}

/// Create a new session.
pub async fn create_session_handler(
    State(state): State<ServerState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let session_id = state
        .agents
        .session_manager
        .create_session()
        .await
        .map_err(|e| ErrorResponse::with_message(e.to_string()))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": session_id,
    }))))
}

/// Query parameters for listing sessions with pagination.
#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Page size
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    20
}

/// List all sessions with pagination.
pub async fn list_sessions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, ErrorResponse> {
    let pagination = Pagination {
        page: query.page.max(1),
        page_size: query.page_size.clamp(1, 100),
    };

    let all_sessions = state.agents.session_manager.list_sessions_with_info().await;
    let total_count = all_sessions.len() as u32;

    // Calculate pagination
    let offset = pagination.offset();
    let limit = pagination.limit();
    let paginated_sessions: Vec<serde_json::Value> = all_sessions
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|s| json!(s))
        .collect();

    let meta = pagination.meta(total_count);

    Ok(Json(ApiResponse::paginated(paginated_sessions, meta)))
}

/// Get session info.
pub async fn get_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let agent = state
        .agents
        .session_manager
        .get_session(&id)
        .await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": id,
        "state": agent.state().await,
    }))))
}

/// Get session history.
pub async fn get_session_history_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    // First check if session exists (get_history returns empty for NotFound)
    let _agent = state
        .agents
        .session_manager
        .get_session(&id)
        .await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

    let history = state
        .agents
        .session_manager
        .get_history(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get history: {}", e)))?;

    Ok(Json(ApiResponse::success(json!({
        "messages": history,
        "count": history.len(),
    }))))
}

/// Delete a session.
pub async fn delete_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    state
        .agents
        .session_manager
        .remove_session(&id)
        .await
        .map_err(|e| {
            // Check if it's a NotFound error
            if format!("{}", e).contains("Session:") || format!("{}", e).contains("not found") {
                ErrorResponse::not_found("Session")
            } else {
                // Other error - return the actual message
                ErrorResponse::with_message(e.to_string())
            }
        })?;

    Ok(Json(ApiResponse::success(json!({
        "deleted": true,
        "sessionId": id,
    }))))
}

/// P0.3: Get pending stream state for a session (for recovery after disconnection).
pub async fn get_pending_stream_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let session_store = state.agents.session_manager.session_store();

    match session_store.get_pending_stream(&id) {
        Ok(Some(pending)) => Ok(Json(ApiResponse::success(json!({
            "hasPending": true,
            "sessionId": id,
            "userMessage": pending.user_message,
            "content": pending.content,
            "thinking": pending.thinking,
            "stage": pending.stage,
            "elapsed": pending.elapsed_secs(),
            "startedAt": pending.started_at,
        })))),
        Ok(None) => Ok(Json(ApiResponse::success(json!({
            "hasPending": false,
            "sessionId": id,
        })))),
        Err(e) => Err(ErrorResponse::with_message(format!(
            "Failed to check pending stream: {}",
            e
        ))),
    }
}

/// P0.3: Clear pending stream state for a session (user chose to discard).
pub async fn clear_pending_stream_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let session_store = state.agents.session_manager.session_store();

    session_store.delete_pending_stream(&id).map_err(|e| {
        ErrorResponse::with_message(format!("Failed to clear pending stream: {}", e))
    })?;

    Ok(Json(ApiResponse::success(json!({
        "cleared": true,
        "sessionId": id,
    }))))
}

/// Request body for updating session.
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    /// Session title (optional)
    pub title: Option<String>,
}

/// Update a session (e.g., rename).
pub async fn update_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    state
        .agents
        .session_manager
        .update_session_title(&id, req.title)
        .await
        .map_err(|e| ErrorResponse::with_message(e.to_string()))?;

    Ok(Json(ApiResponse::success(json!({
        "sessionId": id,
        "updated": true,
    }))))
}

/// Clean up invalid sessions (dirty data).
/// Removes sessions that appear in the list but don't have valid data.
pub async fn cleanup_sessions_handler(
    State(state): State<ServerState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let cleaned_count = state
        .agents
        .session_manager
        .cleanup_invalid_sessions()
        .await;

    Ok(Json(ApiResponse::success(json!({
        "cleaned": cleaned_count,
        "message": format!("Cleaned up {} invalid session(s)", cleaned_count),
    }))))
}

/// Chat handler (REST).
pub async fn chat_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ErrorResponse> {
    use tokio::time::{timeout, Duration};

    println!(
        "[chat_handler] Received request for session {}, message: {}",
        id, req.message
    );

    // Add a 120-second timeout to support thinking models
    // QWEN3 with thinking enabled can take 60-90 seconds for complex queries
    // due to the model's repetitive thinking generation, especially with longer context
    let response = match timeout(
        Duration::from_secs(120),
        state
            .agents
            .session_manager
            .process_message(&id, &req.message),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            // Check if it's a NotFound error and return 404
            let err_msg = e.to_string();
            if err_msg.contains("Not found") || err_msg.contains("Session:") {
                return Err(ErrorResponse::not_found("Session"));
            }
            return Err(ErrorResponse::with_message(err_msg));
        }
        Err(_) => {
            // Timeout - return an error response instead of hanging
            return Ok(Json(ChatResponse {
                response: "请求超时。模型思考时间过长，请尝试简化问题或开启新对话。".to_string(),
                session_id: id,
                tools_used: vec![],
                processing_time_ms: 120000,
                thinking: None,
            }));
        }
    };

    Ok(Json(ChatResponse {
        response: response.message.content.clone(),
        session_id: id,
        tools_used: response.tools_used.clone(),
        processing_time_ms: response.processing_time_ms,
        thinking: response.message.thinking.clone(),
    }))
}

/// WebSocket chat handler.
///
/// Requires JWT token authentication via `?token=xxx` parameter.
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    // Extract and validate JWT token
    let session_info = match params.get("token") {
        Some(token) => match state.auth.user_state.validate_token(token) {
            Ok(info) => {
                tracing::info!(
                    username = %info.username,
                    role = info.role.as_str(),
                    "WebSocket authenticated via JWT"
                );
                Some(info)
            }
            Err(e) => {
                tracing::warn!(error = %e, "JWT validation failed, rejecting WebSocket connection");
                return ws.on_upgrade(|mut socket| async move {
                    let _ = socket
                        .send(AxumMessage::Text(
                            json!({"type": "Error", "message": "Invalid or expired token"})
                                .to_string(),
                        ))
                        .await;
                    let _ = socket.close().await;
                });
            }
        },
        None => {
            tracing::warn!("No authentication provided, rejecting WebSocket connection");
            return ws.on_upgrade(|mut socket| {
                async move {
                    let _ = socket.send(AxumMessage::Text(
                        json!({"type": "Error", "message": "Authentication required. Provide a valid JWT token."}).to_string()
                    )).await;
                    let _ = socket.close().await;
                }
            });
        }
    };

    let session_id = params.get("sessionId").cloned();
    ws.on_upgrade(|socket| handle_ws_socket(socket, state, session_id, session_info))
}

/// Send session history to the client.
///
/// This is called when a session is restored or switched to send the conversation
/// history to the frontend for display.
async fn send_session_history(
    socket: &mut WebSocket,
    session_id: &str,
    state: &ServerState,
) -> Result<(), Box<dyn std::error::Error>> {
    match state.agents.session_manager.get_history(session_id).await {
        Ok(messages) => {
            if !messages.is_empty() {
                tracing::info!(
                    session_id = %session_id,
                    count = messages.len(),
                    "Sending session history to client"
                );

                // Send each message as a separate event
                for msg in &messages {
                    let history_msg = json!({
                        "type": "history",
                        "sessionId": session_id,
                        "role": msg.role,
                        "content": msg.content,
                        "thinking": msg.thinking,
                        "toolCalls": msg.tool_calls,
                        "timestamp": msg.timestamp,
                    })
                    .to_string();

                    if socket.send(AxumMessage::Text(history_msg)).await.is_err() {
                        return Err("Failed to send history message".into());
                    }
                }

                // Send history complete marker
                let complete_msg = json!({
                    "type": "history_complete",
                    "sessionId": session_id,
                    "count": messages.len(),
                })
                .to_string();

                socket.send(AxumMessage::Text(complete_msg)).await?;
            }
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                "Failed to get session history"
            );
            Err(e.into())
        }
    }
}

/// Handle WebSocket connection.
async fn handle_ws_socket(
    mut socket: WebSocket,
    state: ServerState,
    session_id: Option<String>,
    _session_info: Option<crate::auth_users::SessionInfo>,
) {
    // Create connection metadata for tracking state and heartbeat
    let conn_meta = create_connection_metadata();
    conn_meta
        .set_state(super::ws::ConnectionState::Authenticated)
        .await;

    // Track the current session for this connection
    let current_session_id = Arc::new(tokio::sync::RwLock::new(session_id.clone()));

    // Subscribe to device status updates
    let mut device_update_rx = state.devices.update_tx.subscribe();

    // Heartbeat interval
    let mut heartbeat_interval =
        tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

    // Heartbeat timeout - 60 seconds without pong = disconnect
    let heartbeat_timeout = Duration::from_secs(60);

    // Channel for receiving LLM stream events from spawned tasks
    // This keeps the main event loop responsive to WebSocket pings
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamEvent>();

    // Send welcome message
    let welcome = json!({
        "type": "system",
        "content": "Connected to Edge AI Agent",
        "sessionId": session_id,
    })
    .to_string();

    if socket.send(AxumMessage::Text(welcome)).await.is_err() {
        conn_meta.mark_closed().await;
        return;
    }

    // Send session history if reconnecting with an existing session
    if let Some(ref sid) = session_id {
        if !sid.is_empty() {
            let _ = send_session_history(&mut socket, sid, &state).await;
        }
    }

    // Main event loop - handle client messages, device updates, and heartbeat
    loop {
        tokio::select! {
            // Handle incoming client messages
            msg_result = socket.next() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        match msg {
                            AxumMessage::Text(text) => {
                                // Track message received
                                conn_meta.increment_received();

                                // Check for pong response to our heartbeat ping
                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if value.get("type") == Some(&json!("pong")) {
                                        conn_meta.record_pong().await;
                                        tracing::debug!("Received pong from client");
                                        continue;
                                    }
                                }

                                if let Ok(chat_req) = serde_json::from_str::<ChatRequest>(&text) {
                                    // Use the sessionId from the request if provided, otherwise use current
                                    let requested_session_id = chat_req.session_id;

                                    // Session resolution helper - minimizes lock time
                                    let session_id = {
                                        let current_guard = current_session_id.read().await;
                                        let empty = String::new();
                                        let current = current_guard.as_ref().unwrap_or(&empty);

                                        // Check if we need to switch sessions
                                        let needs_switch = if let Some(req_id) = &requested_session_id {
                                            req_id != current && state.agents.session_manager.get_session(req_id).await.is_ok()
                                        } else {
                                            false
                                        };

                                        let has_valid_session = current_guard.as_ref()
                                            .map(|s| !s.is_empty())
                                            .unwrap_or(false);

                                        if needs_switch {
                                            // Switch to requested session
                                            if let Some(req_id) = &requested_session_id {
                                                drop(current_guard);
                                                let mut write_guard = current_session_id.write().await;
                                                *write_guard = Some(req_id.to_string());
                                                let id = req_id.to_string();
                                                drop(write_guard);

                                                // Notify client of session switch (outside lock)
                                                let msg = json!({
                                                    "type": "session_switched",
                                                    "sessionId": id,
                                                }).to_string();
                                                if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                                    return;
                                                }

                                                // Send session history after switching
                                                let _ = send_session_history(&mut socket, &id, &state).await;

                                                id
                                            } else {
                                                unreachable!()
                                            }
                                        } else if !has_valid_session {
                                            // Create new session - drop read lock before write
                                            drop(current_guard);
                                            let new_id = state.agents.session_manager.create_session().await
                                                .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

                                            let mut write_guard = current_session_id.write().await;
                                            *write_guard = Some(new_id.clone());
                                            drop(write_guard);

                                            // Notify client of the new session (outside lock)
                                            let msg = json!({
                                                "type": "session_created",
                                                "sessionId": new_id,
                                            }).to_string();
                                            if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                                return;
                                            }
                                            new_id
                                        } else {
                                            // Use current session - send history if this is a reconnection
                                            let id = current.to_string();
                                            let _ = send_session_history(&mut socket, &id, &state).await;
                                            id
                                        }
                                    };

                                    // Filter out control messages (commands starting with '/')
                                    // These are not user messages and should not be sent to the LLM
                                    let message = chat_req.message.trim();
                                    if message.starts_with('/') {
                                        tracing::info!(
                                            "Ignoring control message: '{}', length={}",
                                            message,
                                            message.chars().count()
                                        );
                                        // Control messages are handled by the sessionId field above,
                                        // no need to process them through the LLM
                                        continue;
                                    }

                                    // Try event streaming first (rich response with tool calls)
                                    // Spawn a task to process the stream asynchronously, keeping the main loop responsive
                                    let backend_id = chat_req.backend_id.as_deref();

                                    // Check if request contains images (multimodal input)
                                    let has_images = chat_req.images.as_ref().is_some_and(|i| !i.is_empty());

                                    if has_images {
                                        // Process multimodal message with images - now with streaming support!
                                        let images: Vec<String> = chat_req.images
                                            .as_ref()
                                            .unwrap_or(&vec![])
                                            .iter()
                                            .map(|img| img.data.clone())
                                            .collect();

                                        let backend_id_str = backend_id.map(|s| s.to_string());
                                        let task_session_id = session_id.clone();
                                        let task_state = state.clone();

                                        // Use streaming for multimodal messages
                                        match task_state.agents.session_manager.process_message_multimodal_with_backend_stream(
                                            &task_session_id,
                                            &chat_req.message,
                                            images.clone(),
                                            backend_id_str.as_deref(),
                                        ).await {
                                            Ok(stream) => {
                                                // Clone the channel sender and session ID for the spawned task
                                                let task_tx = stream_tx.clone();
                                                let task_session_id = session_id.clone();
                                                let task_state = state.clone();

                                                // Spawn a task to process the LLM stream and send events through the channel
                                                tokio::spawn(async move {
                                                    process_stream_to_channel(stream, task_session_id, chat_req.message.clone(), task_tx, task_state).await;
                                                });
                                            }
                                            Err(_e) => {
                                                // Fallback to non-streaming on error
                                                let response = match task_state.agents.session_manager.process_message_multimodal_with_backend(
                                                    &task_session_id,
                                                    &chat_req.message,
                                                    images,
                                                    backend_id_str.as_deref(),
                                                ).await {
                                                    Ok(resp) => json!({
                                                        "type": "response",
                                                        "content": resp.message.content,
                                                        "sessionId": task_session_id,
                                                        "toolsUsed": resp.tools_used,
                                                        "processingTimeMs": resp.processing_time_ms,
                                                    }).to_string(),
                                                    Err(inner_e) => json!({
                                                        "type": "Error",
                                                        "message": inner_e.to_string(),
                                                    }).to_string(),
                                                };

                                                if socket.send(AxumMessage::Text(response)).await.is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    } else {
                                        // Regular text-only message - use streaming
                                        match state.agents.session_manager.process_message_events_with_backend(&session_id, &chat_req.message, backend_id).await {
                                            Ok(stream) => {
                                                // Clone the channel sender and session ID for the spawned task
                                                let task_tx = stream_tx.clone();
                                                let task_session_id = session_id.clone();
                                                let task_state = state.clone();

                                                // Spawn a task to process the LLM stream and send events through the channel
                                                tokio::spawn(async move {
                                                    process_stream_to_channel(stream, task_session_id, chat_req.message.clone(), task_tx, task_state).await;
                                                });
                                            }
                                            Err(_e) => {
                                                // Fallback to non-streaming on error
                                                let backend_id = chat_req.backend_id.as_deref();
                                                let response = match state.agents.session_manager.process_message_with_backend(&session_id, &chat_req.message, backend_id).await {
                                                    Ok(resp) => json!({
                                                        "type": "response",
                                                        "content": resp.message.content,
                                                        "sessionId": session_id,
                                                        "toolsUsed": resp.tools_used,
                                                        "processingTimeMs": resp.processing_time_ms,
                                                    }).to_string(),
                                                    Err(inner_e) => json!({
                                                        "type": "Error",
                                                        "message": inner_e.to_string(),
                                                    }).to_string(),
                                                };

                                                if socket.send(AxumMessage::Text(response)).await.is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            AxumMessage::Close(_) => {
                                return;
                            }
                            _ => {}
                        }
                    }
                    Some(Err(_)) => {
                        return;
                    }
                    None => {
                        return;
                    }
                }
            }
            // Handle device status updates
            update_result = device_update_rx.recv() => {
                match update_result {
                    Ok(device_update) => {
                        let msg = json!({
                            "type": "device_update",
                            "updateType": device_update.update_type,
                            "deviceId": device_update.device_id,
                            "status": device_update.status,
                            "lastSeen": device_update.last_seen,
                        }).to_string();

                        if socket.send(AxumMessage::Text(msg)).await.is_err() {
                            // Client disconnected, stop listening to device updates
                            break;
                        }
                    }
                    Err(_) => {
                        // Channel closed, stop listening
                        break;
                    }
                }
            }
            // Handle LLM stream events from spawned tasks
            stream_event = stream_rx.recv() => {
                match stream_event {
                    Some(event) => {
                        // Normal operation - no need to log every message
                        if socket.send(AxumMessage::Text(event.json)).await.is_err() {
                            // Client disconnected, stop processing stream events
                            break;
                        }
                    }
                    None => {
                        // Channel closed (all tasks dropped their senders)
                        // This is normal - continue waiting for new messages
                    }
                }
            }
            // Handle heartbeat - send periodic ping to detect dead connections
            _ = heartbeat_interval.tick() => {
                // Check for heartbeat timeout before sending ping
                if conn_meta.check_heartbeat_timeout(heartbeat_timeout).await {
                    tracing::warn!(
                        "Heartbeat timeout - no pong received for {:?}",
                        heartbeat_timeout
                    );
                    break;
                }

                conn_meta.record_ping().await;

                let ping = json!({
                    "type": "ping",
                    "timestamp": chrono::Utc::now().timestamp(),
                })
                .to_string();

                if socket.send(AxumMessage::Text(ping)).await.is_err() {
                    // Client disconnected
                    break;
                }

                conn_meta.increment_sent();
            }
        }
    }

    // Mark connection as closed
    conn_meta.mark_closed().await;
    tracing::info!(
        "WebSocket connection closed. Sent: {}, Received: {}, Duration: {:?}",
        conn_meta.get_sent_count(),
        conn_meta.get_received_count(),
        conn_meta.connection_duration()
    );

    // Cleanup: persist session history AFTER loop ends (when connection closes)
    let session_id_opt = current_session_id.read().await.clone();
    if let Some(session_id) = session_id_opt.as_ref() {
        if let Err(e) = state
            .agents
            .session_manager
            .persist_history(session_id)
            .await
        {
            tracing::warn!(category = "session", error = %e, "Failed to persist history on disconnect");
        }
    }
}
