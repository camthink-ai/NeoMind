//! Data Push API handlers.
//!
//! POST   /api/data-push              - Create push target
//! GET    /api/data-push              - List push targets
//! GET    /api/data-push/stats        - Push statistics
//! GET    /api/data-push/:id          - Get push target
//! PUT    /api/data-push/:id          - Update push target
//! DELETE /api/data-push/:id          - Delete push target
//! POST   /api/data-push/:id/test     - Test push target
//! POST   /api/data-push/:id/start    - Start push target
//! POST   /api/data-push/:id/stop     - Stop push target
//! GET    /api/data-push/:id/logs     - List delivery logs

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use neomind_data_push::{CreateTargetRequest, UpdateTargetRequest};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Query parameters for listing push targets.
#[derive(Debug, Deserialize)]
pub struct ListTargetsQuery {
    pub enabled: Option<bool>,
}

/// Query parameters for listing delivery logs.
#[derive(Debug, Deserialize)]
pub struct ListLogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Detect a webhook self-loop: a push target whose URL points back at this
/// server's own device-webhook ingestion endpoint. Such a target forwards
/// data → the server ingests it → re-emits the metrics → forwards again, an
/// unbounded loop that quickly fills logs/storage. Returns an error message
/// when the risk is detected, else `None`. MQTT configs have no `url` field
/// and are skipped automatically.
fn webhook_self_loop_error(config: &serde_json::Value) -> Option<String> {
    let url_str = config.get("url")?.as_str()?;
    let url = reqwest::Url::parse(url_str).ok()?;
    let host = url.host_str()?;

    // Only flag ingestion endpoints — a webhook to some OTHER service on this
    // host is the user's business.
    let path = url.path();
    if !(path.contains("/api/devices") && path.ends_with("/webhook")) {
        return None;
    }

    // Host points back at us: loopback, or our own public/local host. Check
    // loopback first (short-circuit) so the common case never triggers a
    // local-IP detection lookup.
    let is_loopback = matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0");
    let points_at_us = is_loopback
        || host.eq_ignore_ascii_case(crate::handlers::common::get_server_host().as_str());
    if points_at_us {
        return Some(format!(
            "Webhook URL '{}' points back at this server's own ingestion endpoint — \
             this would create an infinite forward→ingest→forward loop. \
             Point it at a different destination.",
            url_str
        ));
    }
    None
}

/// Create a new push target.
/// POST /api/data-push
#[utoipa::path(
    post,
    path = "/api/data-push",
    tag = "data-push",
    request_body = CreateTargetRequest,
    responses(
        (status = 200, description = "Push target created"),
    )
)]
pub async fn create_push_target_handler(
    State(state): State<ServerState>,
    Json(request): Json<CreateTargetRequest>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    if let Some(msg) = webhook_self_loop_error(&request.config) {
        return Err(ErrorResponse::bad_request(msg));
    }
    let target = manager
        .create_target(request)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": target.id,
        "name": target.name,
        "target_type": target.target_type.to_string(),
        "enabled": target.enabled,
    }))
}

/// List all push targets.
/// GET /api/data-push
#[utoipa::path(
    get,
    path = "/api/data-push",
    tag = "data-push",
    params(
        ("enabled" = Option<bool>, Query, description = "Filter by enabled"),
    ),
    responses(
        (status = 200, description = "Configured push targets"),
    )
)]
pub async fn list_push_targets_handler(
    State(state): State<ServerState>,
    Query(_params): Query<ListTargetsQuery>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let targets = manager
        .list_targets()
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "targets": targets,
        "total": targets.len(),
    }))
}

/// Get a push target by ID.
/// GET /api/data-push/:id
#[utoipa::path(
    get,
    path = "/api/data-push/{id}",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    responses(
        (status = 200, description = "One push target"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let target = manager
        .get_target(&id)
        .map_err(|e| ErrorResponse::internal(e.to_string()))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Push target not found: {}", id)))?;

    ok(json!(target))
}

/// Update a push target.
/// PUT /api/data-push/:id
#[utoipa::path(
    put,
    path = "/api/data-push/{id}",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    request_body = UpdateTargetRequest,
    responses(
        (status = 200, description = "Push target updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateTargetRequest>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    if let Some(ref config) = request.config {
        if let Some(msg) = webhook_self_loop_error(config) {
            return Err(ErrorResponse::bad_request(msg));
        }
    }
    let target = manager
        .update_target(&id, request)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": target.id,
        "name": target.name,
        "enabled": target.enabled,
    }))
}

/// Delete a push target.
/// DELETE /api/data-push/:id
#[utoipa::path(
    delete,
    path = "/api/data-push/{id}",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    responses(
        (status = 200, description = "Push target deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let deleted = manager
        .delete_target(&id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    if !deleted {
        return Err(ErrorResponse::not_found(format!(
            "Push target not found: {}",
            id
        )));
    }

    ok(json!({"message": "Push target deleted"}))
}

/// Test a push target by sending sample data.
/// POST /api/data-push/:id/test
#[utoipa::path(
    post,
    path = "/api/data-push/{id}/test",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    responses(
        (status = 200, description = "Test payload delivered; delivery result returned"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn test_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let log = manager
        .test_target(&id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!(log))
}

/// Start a push target.
/// POST /api/data-push/:id/start
#[utoipa::path(
    post,
    path = "/api/data-push/{id}/start",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    responses(
        (status = 200, description = "Push target started"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn start_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    manager
        .start_target(&id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({"message": "Push target started"}))
}

/// Stop a push target.
/// POST /api/data-push/:id/stop
#[utoipa::path(
    post,
    path = "/api/data-push/{id}/stop",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
    ),
    responses(
        (status = 200, description = "Push target stopped"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn stop_push_target_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    manager
        .stop_target(&id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({"message": "Push target stopped"}))
}

/// List delivery logs for a push target.
/// GET /api/data-push/:id/logs
#[utoipa::path(
    get,
    path = "/api/data-push/{id}/logs",
    tag = "data-push",
    params(
        ("id" = String, Path, description = "Push target id"),
        ("limit" = Option<usize>, Query, description = "Max entries"),
        ("offset" = Option<usize>, Query, description = "Skip entries"),
    ),
    responses(
        (status = 200, description = "Delivery attempt log"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn list_delivery_logs_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Query(params): Query<ListLogsQuery>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);
    let (logs, total) = manager
        .list_delivery_logs(&id, limit, offset)
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "logs": logs,
        "total": total,
    }))
}

/// Get push statistics.
/// GET /api/data-push/stats
#[utoipa::path(
    get,
    path = "/api/data-push/stats",
    tag = "data-push",
    responses(
        (status = 200, description = "Delivery counters across targets"),
    )
)]
pub async fn get_push_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    let stats = manager
        .get_stats()
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!(stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_loop_flags_loopback_ingestion() {
        let cfg = serde_json::json!({"url": "http://localhost:9375/api/devices/dev1/webhook"});
        assert!(webhook_self_loop_error(&cfg).is_some());
    }

    #[test]
    fn self_loop_flags_loopback_ip_ingestion() {
        let cfg = serde_json::json!({"url": "http://127.0.0.1:9375/api/devices/webhook"});
        assert!(webhook_self_loop_error(&cfg).is_some());
    }

    #[test]
    fn self_loop_allows_non_ingestion_path_on_localhost() {
        // localhost but a different endpoint — not a self-loop.
        let cfg = serde_json::json!({"url": "http://localhost:9000/some/other/path"});
        assert!(webhook_self_loop_error(&cfg).is_none());
    }

    #[test]
    fn self_loop_ignores_config_without_url() {
        // MQTT-style config has no `url` field.
        let cfg = serde_json::json!({"broker": "localhost", "topic": "t"});
        assert!(webhook_self_loop_error(&cfg).is_none());
    }
}
