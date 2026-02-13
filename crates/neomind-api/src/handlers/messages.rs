//! Message API handlers.
//!
//! GET    /api/messages              - List messages
//! POST   /api/messages              - Create message
//! GET    /api/messages/:id          - Get message
//! DELETE /api/messages/:id          - Delete message
//! POST   /api/messages/:id/acknowledge - Acknowledge message
//! POST   /api/messages/:id/resolve  - Resolve message
//! GET    /api/messages/stats        - Message statistics

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

use neomind_messages::{Message, MessageId, MessageSeverity};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};

// Import json macro for handler responses
use crate::models::ErrorResponse;
use serde_json::json;

/// List messages.
/// GET /api/messages
pub async fn list_messages_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let messages = state.core.message_manager.list_messages().await;
    ok(json!({
        "messages": messages,
        "count": messages.len(),
    }))
}

/// Create message request.
#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub category: String, // alert | system | business
    pub severity: String, // info | warning | critical | emergency
    pub title: String,
    pub message: String,
    pub source: Option<String>,
    pub source_type: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
}

/// Create a message.
/// POST /api/messages
pub async fn create_message_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateMessageRequest>,
) -> HandlerResult<serde_json::Value> {
    let severity = match req.severity.as_str() {
        "info" => MessageSeverity::Info,
        "warning" => MessageSeverity::Warning,
        "critical" => MessageSeverity::Critical,
        "emergency" => MessageSeverity::Emergency,
        _ => MessageSeverity::Info,
    };

    let source = req.source.unwrap_or_else(|| "api".to_string());

    tracing::info!("Creating message: {} - {}", req.title, req.severity);

    let mut msg = Message::new(req.category, severity, req.title, req.message, source);

    if let Some(source_type) = req.source_type {
        msg.source_type = source_type;
    }

    if let Some(metadata) = req.metadata {
        msg.metadata = Some(metadata);
    }

    if let Some(tags) = req.tags {
        msg.tags = tags;
    }

    let created = state
        .core
        .message_manager
        .create_message(msg)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": created.id.to_string(),
        "message": "Message created successfully",
        "message_zh": "消息创建成功",
    }))
}

/// Get a message.
/// GET /api/messages/:id
pub async fn get_message_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let msg_id = MessageId(
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorResponse::bad_request("Invalid message ID"))?,
    );

    let message = state
        .core
        .message_manager
        .get_message(&msg_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Message not found"))?;

    ok(json!(message))
}

/// Delete a message.
/// DELETE /api/messages/:id
pub async fn delete_message_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let msg_id = MessageId(
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorResponse::bad_request("Invalid message ID"))?,
    );

    state
        .core
        .message_manager
        .delete(&msg_id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "message": "Message deleted",
        "message_zh": "消息已删除",
    }))
}

/// Acknowledge a message.
/// POST /api/messages/:id/acknowledge
pub async fn acknowledge_message_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let msg_id = MessageId(
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorResponse::bad_request("Invalid message ID"))?,
    );

    state
        .core
        .message_manager
        .acknowledge(&msg_id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "acknowledged": true,
        "message_id": id,
    }))
}

/// Resolve a message.
/// POST /api/messages/:id/resolve
pub async fn resolve_message_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let msg_id = MessageId(
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorResponse::bad_request("Invalid message ID"))?,
    );

    state
        .core
        .message_manager
        .resolve(&msg_id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "resolved": true,
        "message_id": id,
    }))
}

/// Archive a message.
/// POST /api/messages/:id/archive
pub async fn archive_message_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let msg_id = MessageId(
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorResponse::bad_request("Invalid message ID"))?,
    );

    state
        .core
        .message_manager
        .archive(&msg_id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "archived": true,
        "message_id": id,
    }))
}

/// Message statistics.
/// GET /api/messages/stats
pub async fn message_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let stats = state.core.message_manager.get_stats().await;
    ok(json!(stats))
}

/// Bulk acknowledge messages.
/// POST /api/messages/acknowledge
#[derive(Debug, Deserialize)]
pub struct BulkAcknowledgeRequest {
    pub message_ids: Vec<String>,
}

pub async fn bulk_acknowledge_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut ids = Vec::new();
    for id_str in &req.message_ids {
        let msg_id =
            MessageId(uuid::Uuid::parse_str(id_str).map_err(|_| {
                ErrorResponse::bad_request(format!("Invalid message ID: {}", id_str))
            })?);
        ids.push(msg_id);
    }

    let count = state
        .core
        .message_manager
        .acknowledge_multiple(&ids)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "acknowledged": count,
    }))
}

/// Bulk resolve messages.
/// POST /api/messages/resolve
pub async fn bulk_resolve_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut ids = Vec::new();
    for id_str in &req.message_ids {
        let msg_id =
            MessageId(uuid::Uuid::parse_str(id_str).map_err(|_| {
                ErrorResponse::bad_request(format!("Invalid message ID: {}", id_str))
            })?);
        ids.push(msg_id);
    }

    let count = state
        .core
        .message_manager
        .resolve_multiple(&ids)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "resolved": count,
    }))
}

/// Bulk delete messages.
/// POST /api/messages/delete
pub async fn bulk_delete_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut ids = Vec::new();
    for id_str in &req.message_ids {
        let msg_id =
            MessageId(uuid::Uuid::parse_str(id_str).map_err(|_| {
                ErrorResponse::bad_request(format!("Invalid message ID: {}", id_str))
            })?);
        ids.push(msg_id);
    }

    let count = state
        .core
        .message_manager
        .delete_multiple(&ids)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "deleted": count,
    }))
}

/// Cleanup old messages.
/// POST /api/messages/cleanup
#[derive(Debug, Deserialize)]
pub struct CleanupRequest {
    pub older_than_days: u32,
}

pub async fn cleanup_handler(
    State(state): State<ServerState>,
    Json(req): Json<CleanupRequest>,
) -> HandlerResult<serde_json::Value> {
    let count = state
        .core
        .message_manager
        .cleanup_old(req.older_than_days as i64)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "cleaned": count,
        "message": format!("Cleaned up {} old messages", count),
        "message_zh": format!("清理了 {} 条旧消息", count),
    }))
}

/// Router for message endpoints.
pub fn messages_router() -> axum::Router<ServerState> {
    use axum::routing::{delete, get, post};

    axum::Router::new()
        .route(
            "/messages",
            get(list_messages_handler).post(create_message_handler),
        )
        .route("/messages/stats", get(message_stats_handler))
        .route("/messages/cleanup", post(cleanup_handler))
        .route("/messages/acknowledge", post(bulk_acknowledge_handler))
        .route("/messages/resolve", post(bulk_resolve_handler))
        .route("/messages/delete", post(bulk_delete_handler))
        .route("/messages/:id", get(get_message_handler))
        .route("/messages/:id", delete(delete_message_handler))
        .route(
            "/messages/:id/acknowledge",
            post(acknowledge_message_handler),
        )
        .route("/messages/:id/resolve", post(resolve_message_handler))
        .route("/messages/:id/archive", post(archive_message_handler))
}
