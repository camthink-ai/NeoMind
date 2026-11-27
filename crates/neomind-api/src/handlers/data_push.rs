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

/// The manager reports a missing target as anyhow "Target not found: X" —
/// map it to 404 so consumers don't see 500s for plain not-found ids.
fn push_error_to_response(e: anyhow::Error) -> ErrorResponse {
    let msg = e.to_string();
    if msg.starts_with("Target not found") {
        ErrorResponse::not_found(msg)
    } else if msg.starts_with("Unknown target type") {
        // an unrecognized `type` discriminator is a client mistake
        ErrorResponse::bad_request(msg)
    } else {
        ErrorResponse::internal(msg)
    }
}

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
        .map_err(push_error_to_response)?;

    ok(json!({
        "id": target.id,
        "name": target.name,
        "target_type": target.target_type.to_string(),
        "enabled": target.enabled,
    }))
}

/// Sentinel string substituted for secret fields in GET responses.
/// Matches the convention used by MQTT broker credential handlers.
const SECRET_MASK: &str = "*****";

fn is_sensitive_header_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "apikey"
            | "x-auth-token"
            | "x-access-token"
            | "x-token"
            | "x-secret"
            | "cookie"
            | "set-cookie"
    ) || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("api-key")
        || lower.contains("apikey")
}

/// Mask secrets inside a serialized `PushTarget`'s `config` object before
/// returning it from GET endpoints (`list_push_targets_handler`,
/// `get_push_target_handler`).
fn mask_target_config(config: &mut serde_json::Value) {
    let Some(obj) = config.as_object_mut() else {
        return;
    };

    // Webhook Bearer token
    if let Some(serde_json::Value::String(tok)) = obj.get_mut("auth_token") {
        if !tok.is_empty() {
            *tok = SECRET_MASK.to_string();
        }
    }

    // Webhook Basic auth password
    if let Some(serde_json::Value::Object(basic)) = obj.get_mut("auth_basic") {
        if let Some(serde_json::Value::String(pw)) = basic.get_mut("password") {
            if !pw.is_empty() {
                *pw = SECRET_MASK.to_string();
            }
        }
    }

    // MQTT password
    if let Some(serde_json::Value::String(pw)) = obj.get_mut("password") {
        if !pw.is_empty() {
            *pw = SECRET_MASK.to_string();
        }
    }

    // Sensitive webhook headers (Authorization, X-API-Key, etc.)
    if let Some(serde_json::Value::Object(headers)) = obj.get_mut("headers") {
        for (k, v) in headers.iter_mut() {
            if is_sensitive_header_name(k) {
                if let serde_json::Value::String(val) = v {
                    if !val.is_empty() {
                        *val = SECRET_MASK.to_string();
                    }
                }
            }
        }
    }
}

/// Serialize a `PushTarget` to JSON with sensitive credentials masked.
fn target_to_masked_json<T: serde::Serialize>(target: &T) -> serde_json::Value {
    let mut v = json!(target);
    if let Some(cfg) = v.get_mut("config") {
        mask_target_config(cfg);
    }
    v
}

/// Restore any fields still carrying `SECRET_MASK` from the previously stored
/// config so a UI GET -> edit -> PUT round-trip preserves untouched secrets.
fn restore_masked_secrets(new_config: &mut serde_json::Value, existing_config: &serde_json::Value) {
    let (Some(new_obj), Some(old_obj)) = (new_config.as_object_mut(), existing_config.as_object())
    else {
        return;
    };

    for field in ["auth_token", "password"] {
        if new_obj.get(field).and_then(|v| v.as_str()) == Some(SECRET_MASK) {
            if let Some(old_val) = old_obj.get(field) {
                new_obj.insert(field.to_string(), old_val.clone());
            }
        }
    }

    if let (
        Some(serde_json::Value::Object(new_basic)),
        Some(serde_json::Value::Object(old_basic)),
    ) = (new_obj.get_mut("auth_basic"), old_obj.get("auth_basic"))
    {
        if new_basic.get("password").and_then(|v| v.as_str()) == Some(SECRET_MASK) {
            if let Some(old_pw) = old_basic.get("password") {
                new_basic.insert("password".to_string(), old_pw.clone());
            }
        }
    }

    if let (
        Some(serde_json::Value::Object(new_headers)),
        Some(serde_json::Value::Object(old_headers)),
    ) = (new_obj.get_mut("headers"), old_obj.get("headers"))
    {
        for (k, v) in new_headers.iter_mut() {
            if is_sensitive_header_name(k) && v.as_str() == Some(SECRET_MASK) {
                if let Some(old_val) = old_headers.get(k) {
                    *v = old_val.clone();
                }
            }
        }
    }
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

    let masked: Vec<serde_json::Value> = targets.iter().map(target_to_masked_json).collect();

    ok(json!({
        "targets": masked,
        "total": masked.len(),
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

    ok(target_to_masked_json(&target))
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
    Json(mut request): Json<UpdateTargetRequest>,
) -> HandlerResult<serde_json::Value> {
    let data_push = state.data_push.read().await;
    let manager = data_push
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("Data push manager not initialized"))?;

    if let Some(ref mut config) = request.config {
        if let Ok(Some(existing)) = manager.get_target(&id) {
            restore_masked_secrets(config, &existing.config);
        }
        if let Some(msg) = webhook_self_loop_error(config) {
            return Err(ErrorResponse::bad_request(msg));
        }
    }
    let target = manager
        .update_target(&id, request)
        .await
        .map_err(push_error_to_response)?;

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
        .map_err(push_error_to_response)?;

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
        .map_err(push_error_to_response)?;

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
        .map_err(push_error_to_response)?;

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
        .map_err(push_error_to_response)?;

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

    #[test]
    fn mask_and_restore_webhook_and_mqtt_secrets() {
        let existing_webhook = serde_json::json!({
            "url": "https://example.com/hook",
            "auth_token": "tok-secret-123",
            "auth_basic": { "username": "alice", "password": "pw-secret-456" },
            "headers": {
                "Authorization": "Bearer hdr-secret",
                "X-API-Key": "key-789",
                "X-Custom": "public-val"
            }
        });

        let mut masked = existing_webhook.clone();
        mask_target_config(&mut masked);
        assert_eq!(masked["auth_token"], SECRET_MASK);
        assert_eq!(masked["auth_basic"]["username"], "alice");
        assert_eq!(masked["auth_basic"]["password"], SECRET_MASK);
        assert_eq!(masked["headers"]["Authorization"], SECRET_MASK);
        assert_eq!(masked["headers"]["X-API-Key"], SECRET_MASK);
        assert_eq!(masked["headers"]["X-Custom"], "public-val");

        // Round-trip update with untouched masked values preserves original secrets.
        restore_masked_secrets(&mut masked, &existing_webhook);
        assert_eq!(masked["auth_token"], "tok-secret-123");
        assert_eq!(masked["auth_basic"]["password"], "pw-secret-456");
        assert_eq!(masked["headers"]["Authorization"], "Bearer hdr-secret");
        assert_eq!(masked["headers"]["X-API-Key"], "key-789");

        // MQTT password masking + restore.
        let existing_mqtt = serde_json::json!({
            "broker": "mqtt.example.com",
            "username": "u",
            "password": "mqtt-secret-password"
        });
        let mut masked_mqtt = existing_mqtt.clone();
        mask_target_config(&mut masked_mqtt);
        assert_eq!(masked_mqtt["password"], SECRET_MASK);
        restore_masked_secrets(&mut masked_mqtt, &existing_mqtt);
        assert_eq!(masked_mqtt["password"], "mqtt-secret-password");
    }
}
