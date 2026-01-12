//! Session management handlers.

use axum::{extract::{Path, State, Query}, response::Json};
use axum::extract::ws::{WebSocketUpgrade, WebSocket, Message as AxumMessage};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use edge_ai_agent::AgentEvent;
use crate::models::{
    ChatRequest, ChatResponse, ErrorResponse,
    pagination::Pagination,
    common::ApiResponse,
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
    let session_id = state.session_manager.create_session().await
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

fn default_page() -> u32 { 1 }
fn default_page_size() -> u32 { 20 }

/// List all sessions with pagination.
pub async fn list_sessions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, ErrorResponse> {
    let pagination = Pagination {
        page: query.page.max(1),
        page_size: query.page_size.clamp(1, 100),
    };

    let all_sessions = state.session_manager.list_sessions_with_info().await;
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

    Ok(Json(ApiResponse::paginated(
        paginated_sessions,
        meta,
    )))
}

/// Get session info.
pub async fn get_session_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let agent = state.session_manager.get_session(&id).await
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
    let history = state.session_manager.get_history(&id).await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

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
    state.session_manager.remove_session(&id).await
        .map_err(|_| ErrorResponse::not_found("Session"))?;

    Ok(Json(ApiResponse::success(json!({
        "deleted": true,
        "sessionId": id,
    }))))
}

/// Chat handler (REST).
pub async fn chat_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ErrorResponse> {
    let response = state.session_manager.process_message(&id, &req.message).await
        .map_err(|e| ErrorResponse::with_message(e.to_string()))?;

    Ok(Json(ChatResponse {
        response: response.message.content,
        session_id: id,
        tools_used: response.tools_used,
        processing_time_ms: response.processing_time_ms,
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
        Some(token) => {
            match state.auth_user_state.validate_token(token) {
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
                    return ws.on_upgrade(|mut socket| {
                        async move {
                            let _ = socket.send(AxumMessage::Text(
                                json!({"type": "Error", "message": "Invalid or expired token"}).to_string()
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

/// Handle WebSocket connection.
async fn handle_ws_socket(
    mut socket: WebSocket,
    state: ServerState,
    session_id: Option<String>,
    _session_info: Option<crate::auth_users::SessionInfo>,
) {
    // Track the current session for this connection
    let current_session_id = Arc::new(tokio::sync::RwLock::new(session_id.clone()));

    // Subscribe to device status updates
    let mut device_update_rx = state.device_update_tx.subscribe();

    // Heartbeat interval
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

    // Send welcome message
    let welcome = json!({
        "type": "system",
        "content": "Connected to Edge AI Agent",
        "sessionId": session_id,
    }).to_string();

    if socket.send(AxumMessage::Text(welcome)).await.is_err() {
        return;
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
                                if let Ok(chat_req) = serde_json::from_str::<ChatRequest>(&text) {
                                    // Use the sessionId from the request if provided, otherwise use current
                                    let requested_session_id = chat_req.session_id;
                                    let mut session_id_guard = current_session_id.write().await;

                                    // If request has a different sessionId, switch to it
                                    if let Some(req_id) = &requested_session_id {
                                        if req_id != session_id_guard.as_ref().unwrap_or(&String::new()) {
                                            // Verify session exists
                                            if state.session_manager.get_session(req_id).await.is_ok() {
                                                *session_id_guard = Some(req_id.to_string());
                                                // Notify client of session switch
                                                let msg = json!({
                                                    "type": "session_switched",
                                                    "sessionId": req_id,
                                                }).to_string();
                                                if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                                    return;
                                                }
                                            } else {
                                                // Session doesn't exist, clear it
                                                *session_id_guard = None;
                                            }
                                        }
                                    }

                                    // Ensure we have a session
                                    if session_id_guard.is_none() {
                                        // Create new session for this message
                                        let new_id = state.session_manager.create_session().await.unwrap_or_else(|_| {
                                            uuid::Uuid::new_v4().to_string()
                                        });
                                        *session_id_guard = Some(new_id.clone());

                                        // Notify client of the new session
                                        let msg = json!({
                                            "type": "session_created",
                                            "sessionId": new_id,
                                        }).to_string();
                                        if socket.send(AxumMessage::Text(msg)).await.is_err() {
                                            return;
                                        }
                                    }
                                    // At this point session_id_guard is guaranteed to be Some
                                    let session_id = session_id_guard.as_ref()
                                        .expect("session_id should be set after check above")
                                        .clone();
                                    drop(session_id_guard);

                                    // Try event streaming first (rich response with tool calls)
                                    match state.session_manager.process_message_events(&session_id, &chat_req.message).await {
                                        Ok(mut stream) => {
                                            let mut error_occurred = false;

                                            while let Some(event) = StreamExt::next(&mut stream).await {
                                                // Convert AgentEvent to JSON manually
                                                let event_json = match &event {
                                                    AgentEvent::Thinking { content } => {
                                                        json!({
                                                            "type": "Thinking",
                                                            "content": content,
                                                            "sessionId": session_id,
                                                        })
                                                    }
                                                    AgentEvent::Content { content } => {
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
                                                    AgentEvent::ToolCallEnd { tool, result, success } => {
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
                                                    AgentEvent::End => {
                                                        json!({
                                                            "type": "end",
                                                            "sessionId": session_id,
                                                        })
                                                    }
                                                };

                                                if socket.send(AxumMessage::Text(event_json.to_string())).await.is_err() {
                                                    error_occurred = true;
                                                    break;
                                                }
                                            }

                                            if !error_occurred {
                                                let end_msg = json!({
                                                    "type": "end",
                                                    "sessionId": session_id,
                                                }).to_string();
                                                if socket.send(AxumMessage::Text(end_msg)).await.is_err() {
                                                    // ignore
                                                }

                                                // Persist history after stream completes
                                                if let Err(e) = state.session_manager.persist_history(&session_id).await {
                                                    tracing::warn!(category = "session", error = %e, "Failed to persist history");
                                                }
                                            }
                                        }
                                        Err(_e) => {
                                            // Fallback to non-streaming on error
                                            let response = match state.session_manager.process_message(&session_id, &chat_req.message).await {
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
            // Handle heartbeat - send periodic ping to detect dead connections
            _ = heartbeat_interval.tick() => {
                let ping = json!({
                    "type": "ping",
                    "timestamp": chrono::Utc::now().timestamp(),
                }).to_string();

                if socket.send(AxumMessage::Text(ping)).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
        }

        // Cleanup: persist session history before closing
        if let Some(session_id) = current_session_id.read().await.as_ref() {
            if let Err(e) = state.session_manager.persist_history(session_id).await {
                tracing::warn!(category = "session", error = %e, "Failed to persist history on disconnect");
            }
        }
    }
}
