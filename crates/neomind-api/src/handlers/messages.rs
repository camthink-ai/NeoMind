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
    extract::{Path, Query, State},
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

/// Query parameters for listing messages.
#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub severity: Option<String>,
    pub status: Option<String>,
    pub category: Option<String>,
}

/// List messages with pagination and filters.
/// GET /api/messages?limit=10&offset=0&severity=warning&status=active
#[utoipa::path(
    get,
    path = "/api/messages",
    tag = "messages",
    params(
        ("limit" = Option<usize>, Query, description = "Max items"),
        ("offset" = Option<usize>, Query, description = "Skip items"),
        ("severity" = Option<String>, Query, description = "Filter by severity"),
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("category" = Option<String>, Query, description = "Filter by category"),
    ),
    responses(
        (status = 200, description = "Notification feed"),
    )
)]
pub async fn list_messages_handler(
    State(state): State<ServerState>,
    Query(params): Query<ListMessagesQuery>,
) -> HandlerResult<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    // Use targeted queries when only a single filter is specified
    // to avoid loading all messages into memory
    let messages = if params.severity.is_none() {
        match (&params.status, &params.category) {
            // Single status filter → use indexed query
            (Some(st), None) => {
                let status = match st.to_lowercase().as_str() {
                    "active" => Some(neomind_messages::MessageStatus::Active),
                    "acknowledged" => Some(neomind_messages::MessageStatus::Acknowledged),
                    "resolved" => Some(neomind_messages::MessageStatus::Resolved),
                    "archived" => Some(neomind_messages::MessageStatus::Archived),
                    _ => None,
                };
                if let Some(s) = status {
                    state.core.message_manager.list_messages_by_status(s).await
                } else {
                    state.core.message_manager.list_messages().await
                }
            }
            // Single category filter → use indexed query
            (None, Some(cat)) => {
                state
                    .core
                    .message_manager
                    .list_messages_by_category(cat)
                    .await
            }
            // Both status + category → use status query, then filter category
            (Some(st), Some(_cat)) => {
                let status = match st.to_lowercase().as_str() {
                    "active" => Some(neomind_messages::MessageStatus::Active),
                    "acknowledged" => Some(neomind_messages::MessageStatus::Acknowledged),
                    "resolved" => Some(neomind_messages::MessageStatus::Resolved),
                    "archived" => Some(neomind_messages::MessageStatus::Archived),
                    _ => None,
                };
                if let Some(s) = status {
                    state.core.message_manager.list_messages_by_status(s).await
                } else {
                    state.core.message_manager.list_messages().await
                }
            }
            // No filters → list all
            (None, None) => state.core.message_manager.list_messages().await,
        }
    } else {
        state.core.message_manager.list_messages().await
    };

    // Apply remaining filters that couldn't be pushed down
    let filtered: Vec<&Message> = messages
        .iter()
        .filter(|m| {
            if let Some(ref sev) = params.severity {
                let msg_sev = format!("{:?}", m.severity).to_lowercase();
                if msg_sev != sev.to_lowercase().as_str() {
                    return false;
                }
            }
            if let Some(ref st) = params.status {
                let msg_st = format!("{:?}", m.status).to_lowercase();
                if msg_st != st.to_lowercase().as_str() {
                    return false;
                }
            }
            if let Some(ref cat) = params.category {
                if &m.category != cat {
                    return false;
                }
            }
            true
        })
        .collect();

    let total = filtered.len();

    // Sort by timestamp descending (newest first)
    let mut sorted = filtered;
    sorted.sort_by_key(|m| std::cmp::Reverse(m.timestamp));

    // Apply pagination
    let paginated: Vec<&Message> = sorted.into_iter().skip(offset).take(limit).collect();

    ok(json!({
        "messages": paginated,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
}

/// Create message request.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[utoipa::path(
    post,
    path = "/api/messages",
    tag = "messages",
    request_body = CreateMessageRequest,
    responses(
        (status = 200, description = "Message created and fanned out to channels"),
    )
)]
pub async fn create_message_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateMessageRequest>,
) -> HandlerResult<serde_json::Value> {
    // Unknown severities are REJECTED, not coerced to Info — a typo like
    // "severe" used to store Info silently, dropping the alert from
    // critical-tier channel filters with no signal to the caller.
    let severity = match req.severity.to_lowercase().as_str() {
        "info" => MessageSeverity::Info,
        "warning" => MessageSeverity::Warning,
        "critical" => MessageSeverity::Critical,
        "emergency" => MessageSeverity::Emergency,
        other => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid severity '{}' — expected info | warning | critical | emergency",
                other
            )))
        }
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
#[utoipa::path(
    get,
    path = "/api/messages/{id}",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "One message"),
        (status = 404, description = "Not found"),
    )
)]
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

/// The judgment chain behind one alert (M2-5, first hop).
///
/// GET /api/messages/:id/chain
///
/// A rule-generated alert records, in its own metadata, the execution that
/// produced it — `(rule_id, triggered_at)`, the pair the history row is keyed
/// by. This walks that one hop and returns the rule, the execution and the
/// evidence that tripped the condition, so the alert page can answer "why did
/// this fire" without the user guessing which agent to open.
///
/// A missing link is reported as `resolved: false` with a reason rather than an
/// error: most messages are not rule alerts at all, and rule history is pruned
/// after 30 days, so an alert can legitimately outlive its execution.
#[utoipa::path(
    get,
    path = "/api/messages/{id}/chain",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "The chain, resolved or with a reason it is not"),
        (status = 400, description = "Invalid message id"),
        (status = 404, description = "Message not found"),
    )
)]
pub async fn get_message_chain_handler(
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

    let Some(reference) = neomind_rules::chain::execution_reference(message.metadata.as_ref())
    else {
        return ok(json!({
            "message_id": id,
            "resolved": false,
            "reason": "no_rule_reference",
        }));
    };

    let rule = state
        .automation
        .rule_engine
        .get_rule(&reference.rule_id)
        .await;

    // Persistent history first, in-memory as the fallback — the same order the
    // rule-history endpoint uses.
    let history = match state.automation.rule_store.as_ref() {
        Some(store) => match store.load_history(&reference.rule_id) {
            Ok(h) if !h.is_empty() => h,
            _ => {
                state
                    .automation
                    .rule_engine
                    .get_rule_history(&reference.rule_id)
                    .await
            }
        },
        None => {
            state
                .automation
                .rule_engine
                .get_rule_history(&reference.rule_id)
                .await
        }
    };

    let execution = neomind_rules::chain::find_execution(&history, reference.execution_ms);

    ok(json!({
        "message_id": id,
        "resolved": execution.is_some(),
        "reason": if execution.is_some() { serde_json::Value::Null } else { json!("execution_not_found") },
        "rule": rule.as_ref().map(|r| json!({
            "id": r.id.to_string(),
            "name": r.name,
            "enabled": r.enabled,
        })),
        "trigger": {
            "source": reference.trigger_source,
            "value": reference.trigger_value,
        },
        "execution": execution.map(|e| json!({
            "triggered_at": e.triggered_at.to_rfc3339(),
            "success": e.success,
            "duration_ms": e.duration_ms,
            "actions_executed": e.actions_executed,
            "error": e.error,
        })),
    }))
}

/// Delete a message.
/// DELETE /api/messages/:id
#[utoipa::path(
    delete,
    path = "/api/messages/{id}",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "Message deleted"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/messages/{id}/acknowledge",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "Message acknowledged"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/messages/{id}/resolve",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "Message resolved"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/messages/{id}/archive",
    tag = "messages",
    params(
        ("id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "Message archived"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/messages/stats",
    tag = "messages",
    responses(
        (status = 200, description = "Counts by severity/status"),
    )
)]
pub async fn message_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let stats = state.core.message_manager.get_stats().await;
    ok(json!(stats))
}

/// Bulk acknowledge messages.
/// POST /api/messages/acknowledge
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct BulkAcknowledgeRequest {
    pub message_ids: Vec<String>,
}

/// Parse a bulk-request's message id strings into MessageIds. Shared by the
/// bulk acknowledge/resolve/delete handlers (was triplicated verbatim).
fn parse_message_ids(raw: &[String]) -> Result<Vec<MessageId>, ErrorResponse> {
    raw.iter()
        .map(|id_str| {
            Ok(MessageId(uuid::Uuid::parse_str(id_str).map_err(|_| {
                ErrorResponse::bad_request(format!("Invalid message ID: {}", id_str))
            })?))
        })
        .collect()
}

#[utoipa::path(
    post,
    path = "/api/messages/acknowledge",
    tag = "messages",
    request_body = BulkAcknowledgeRequest,
    responses(
        (status = 200, description = "Matching messages acknowledged"),
    )
)]
pub async fn bulk_acknowledge_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let ids = parse_message_ids(&req.message_ids)?;

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
#[utoipa::path(
    post,
    path = "/api/messages/resolve",
    tag = "messages",
    request_body = BulkAcknowledgeRequest,
    responses(
        (status = 200, description = "Matching messages resolved"),
    )
)]
pub async fn bulk_resolve_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let ids = parse_message_ids(&req.message_ids)?;

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
#[utoipa::path(
    post,
    path = "/api/messages/delete",
    tag = "messages",
    request_body = BulkAcknowledgeRequest,
    responses(
        (status = 200, description = "Matching messages deleted"),
    )
)]
pub async fn bulk_delete_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let ids = parse_message_ids(&req.message_ids)?;

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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct CleanupRequest {
    pub older_than_days: u32,
}

#[utoipa::path(
    post,
    path = "/api/messages/cleanup",
    tag = "messages",
    request_body = CleanupRequest,
    responses(
        (status = 200, description = "Old messages purged"),
    )
)]
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
