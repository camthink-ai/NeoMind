//! `logs` handlers — split from the former extensions.rs monolith.

use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

/// Extension log entry DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionLogEntryDto {
    /// Timestamp (milliseconds since epoch)
    pub timestamp: i64,
    /// Log level (trace/debug/info/warn/error)
    pub level: String,
    /// Log message
    pub message: String,
}

/// GET /api/extensions/:id/logs
/// Get extension log entries.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/logs",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Recent extension log lines"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_extension_logs_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<ExtensionLogEntryDto>> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let logs = runtime
        .get_logs(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get logs: {}", e)))?;

    let dtos: Vec<ExtensionLogEntryDto> = logs
        .into_iter()
        .map(|entry| ExtensionLogEntryDto {
            timestamp: entry.timestamp,
            level: entry.level,
            message: entry.message,
        })
        .collect();

    ok(dtos)
}

/// DELETE /api/extensions/:id/logs
/// Clear extension log entries.
#[utoipa::path(
    delete,
    path = "/api/extensions/{id}/logs",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension logs cleared"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn clear_extension_logs_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    runtime
        .clear_logs(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to clear logs: {}", e)))?;

    ok(serde_json::json!({ "message": "Logs cleared" }))
}
