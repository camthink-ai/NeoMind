//! System-level admin handlers: web-triggered server self-upgrade.
//!
//! Three endpoints (admin-only, JWT auth — the `admin_routes` group):
//! - `GET  /api/system/upgrade/check`  — deployment support + latest release
//! - `POST /api/system/upgrade`        — kick off the staged upgrade
//! - `GET  /api/system/upgrade/status` — in-flight task progress
//!
//! The heavy lifting lives in `crate::upgrade` (see its module docs for the
//! two-phase staging design). Handlers only gate on the admin role and
//! translate `UpgradeState` results into responses.

use axum::extract::{Extension, State};
use axum::Json;
use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::types::ServerState;

use crate::auth_users::{SessionInfo, UserRole};

/// `GET /api/system/upgrade/check` — is web upgrade possible + what's latest.
///
/// `?force=true` bypasses the release-check cache (GitHub API is rate-limited;
/// the cache is 5 minutes).
#[derive(serde::Deserialize, Default)]
pub struct UpgradeCheckQuery {
    pub force: Option<bool>,
}

pub async fn upgrade_check_handler(
    State(state): State<ServerState>,
    Extension(admin): Extension<SessionInfo>,
    axum::extract::Query(query): axum::extract::Query<UpgradeCheckQuery>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }

    let info = state.upgrade.check(query.force.unwrap_or(false)).await;
    ok(json!({
        "supported": info.supported,
        "deployment": info.deployment,
        "helper_available": info.helper_available,
        "current_version": info.current_version,
        "latest_version": info.latest_version,
        "release_notes": info.release_notes,
        "available": info.available,
        "notes": info.notes,
    }))
}

/// `POST /api/system/upgrade` — start the staged upgrade (body optional:
/// `{"version": "0.9.22"}` to pin; default latest release).
///
/// Returns immediately; progress flows via `SystemUpgradeProgress` events
/// (WS `category=all`) and `GET /api/system/upgrade/status`.
pub async fn start_upgrade_handler(
    State(state): State<ServerState>,
    Extension(admin): Extension<SessionInfo>,
    payload: Option<axum::Json<serde_json::Value>>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }

    let version = payload.and_then(|Json(v)| {
        v.get("version")
            .and_then(|x| x.as_str())
            .map(str::to_string)
    });

    let (started, reason) = state
        .upgrade
        .start(state.data_dir.clone(), state.event_bus(), version)
        .await;

    if !started && reason == "already_running" {
        return ok(json!({ "started": false, "already_running": true }));
    }
    if !started {
        // Environment/helper gates refused with an actionable reason.
        return Err(ErrorResponse::conflict(reason));
    }
    tracing::info!(
        admin = %admin.username,
        "Web-triggered server upgrade started"
    );
    ok(json!({ "started": true, "already_running": false }))
}

/// `GET /api/system/upgrade/status` — snapshot of the in-flight task.
pub async fn upgrade_status_handler(
    State(state): State<ServerState>,
    Extension(admin): Extension<SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }
    let s = state.upgrade.status();
    ok(json!({
        "running": s.running,
        "phase": s.phase,
        "target_version": s.target_version,
        "downloaded": s.downloaded,
        "total": s.total,
        "error": s.error,
    }))
}
