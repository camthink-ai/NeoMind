//! Command history API handlers.

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use neomind_commands::CommandManager;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Command DTO for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct CommandDto {
    /// Command ID
    pub id: String,
    /// Device ID
    pub device_id: String,
    /// Command name
    pub command: String,
    /// Parameters
    pub params: serde_json::Value,
    /// Command status
    pub status: String,
    /// Priority
    pub priority: String,
    /// Source type
    pub source_type: String,
    /// Source ID
    pub source_id: String,
    /// Creation timestamp
    pub created_at: i64,
    /// Execution timestamp
    pub executed_at: Option<i64>,
    /// Number of attempts
    pub attempt: u32,
    /// Result data
    pub result: Option<CommandResultDto>,
}

/// Command result DTO.
#[derive(Debug, Clone, Serialize)]
pub struct CommandResultDto {
    /// Success status
    pub success: bool,
    /// Result message
    pub message: String,
    /// Response data
    pub response_data: Option<serde_json::Value>,
    /// Completed timestamp
    pub completed_at: i64,
}

/// Query parameters for command listing.
#[derive(Debug, Deserialize)]
pub struct CommandQueryParams {
    /// Filter by device ID
    pub device_id: Option<String>,
    /// Filter by status
    pub status: Option<String>,
    /// Filter by source type
    pub source: Option<String>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl From<Arc<neomind_commands::command::CommandRequest>> for CommandDto {
    fn from(cmd: Arc<neomind_commands::command::CommandRequest>) -> Self {
        Self {
            id: cmd.id.clone(),
            device_id: cmd.device_id.clone(),
            command: cmd.command_name.clone(),
            params: cmd.parameters.clone(),
            status: format!("{:?}", cmd.status),
            priority: cmd.priority.type_name().to_string(),
            source_type: cmd.source.type_name().to_string(),
            source_id: cmd.source.id(),
            created_at: cmd.created_at.timestamp(),
            executed_at: cmd.result.as_ref().map(|r| r.completed_at.timestamp()),
            attempt: cmd.attempt,
            result: cmd.result.as_ref().map(CommandResultDto::from),
        }
    }
}

impl From<&neomind_commands::command::CommandResult> for CommandResultDto {
    fn from(result: &neomind_commands::command::CommandResult) -> Self {
        Self {
            success: result.success,
            message: result.message.clone(),
            response_data: result.response_data.clone(),
            completed_at: result.completed_at.timestamp(),
        }
    }
}

/// Get command manager from server state.
fn get_command_manager(state: &ServerState) -> Result<Arc<CommandManager>, ErrorResponse> {
    state
        .core
        .command_manager
        .as_ref()
        .cloned()
        .ok_or_else(|| ErrorResponse::service_unavailable("Command manager not initialized"))
}

/// List all commands with optional filtering.
///
/// GET /api/commands
pub async fn list_commands_handler(
    State(state): State<ServerState>,
    Query(params): Query<CommandQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    // Get all commands from state store
    let mut all_commands: Vec<neomind_commands::command::CommandRequest> = Vec::new();
    for status in [
        neomind_commands::command::CommandStatus::Pending,
        neomind_commands::command::CommandStatus::Queued,
        neomind_commands::command::CommandStatus::Sending,
        neomind_commands::command::CommandStatus::WaitingAck,
        neomind_commands::command::CommandStatus::Completed,
        neomind_commands::command::CommandStatus::Failed,
        neomind_commands::command::CommandStatus::Cancelled,
        neomind_commands::command::CommandStatus::Timeout,
    ] {
        all_commands.extend(manager.state.list_by_status(status).await);
    }
    let all_commands: Vec<CommandDto> = all_commands
        .into_iter()
        .map(Arc::new)
        .map(CommandDto::from)
        .collect();

    // Apply filters
    let mut filtered: Vec<CommandDto> = all_commands
        .into_iter()
        .filter(|cmd| {
            if let Some(ref device_id) = params.device_id {
                if &cmd.device_id != device_id {
                    return false;
                }
            }
            if let Some(ref status) = params.status {
                if cmd.status != *status && cmd.status != format!("{:?}", status) {
                    return false;
                }
            }
            if let Some(ref source) = params.source {
                if cmd.source_type != *source {
                    return false;
                }
            }
            true
        })
        .collect();

    // Sort by created_at descending
    filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // Apply pagination
    let total = filtered.len();
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50);

    let paginated: Vec<CommandDto> = filtered.into_iter().skip(offset).take(limit).collect();

    ok(json!({
        "commands": paginated,
        "count": paginated.len(),
        "total": total,
        "offset": offset,
        "limit": limit,
    }))
}

/// Get a specific command by ID.
///
/// GET /api/commands/:id
pub async fn get_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    // Need to be explicit about the type since CommandId is a type alias
    use neomind_commands::{
        command::CommandRequest,
        state::{CommandStateStore, StateError},
    };
    let cmd: Result<CommandRequest, StateError> = CommandStateStore::get(&manager.state, &id).await;
    let cmd = cmd.map_err(|e| {
        if matches!(e, StateError::NotFound(_)) {
            ErrorResponse::not_found(format!("Command not found: {}", id))
        } else {
            ErrorResponse::internal(format!("Failed to get command: {}", e))
        }
    })?;

    let dto = CommandDto::from(Arc::new(cmd));
    ok(json!({
        "command": dto,
    }))
}

/// Retry a failed command.
///
/// POST /api/commands/:id/retry
pub async fn retry_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    // Check if command exists
    use neomind_commands::{
        command::CommandRequest,
        state::{CommandStateStore, StateError},
    };
    let cmd: Result<CommandRequest, StateError> = CommandStateStore::get(&manager.state, &id).await;
    let cmd = cmd.map_err(|e| {
        if matches!(e, StateError::NotFound(_)) {
            ErrorResponse::not_found(format!("Command not found: {}", id))
        } else {
            ErrorResponse::internal(format!("Failed to get command: {}", e))
        }
    })?;

    // Check if command can be retried
    if !cmd.can_retry() {
        return ok(json!({
            "success": false,
            "message": format!("Command {} cannot be retried. Status: {:?}", id, cmd.status),
        }));
    }

    // Retry the command
    manager
        .retry(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to retry command: {}", e)))?;

    ok(json!({
        "message": format!("Command {} queued for retry", id),
        "command_id": id,
    }))
}

/// Cancel a pending command.
///
/// POST /api/commands/:id/cancel
pub async fn cancel_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    manager.cancel(&id).await.map_err(|e| {
        if matches!(e, neomind_commands::state::StateError::NotFound(_)) {
            ErrorResponse::not_found(format!("Command not found: {}", id))
        } else {
            ErrorResponse::internal(format!("Failed to cancel command: {}", e))
        }
    })?;

    ok(json!({
        "message": format!("Command {} cancelled", id),
        "command_id": id,
    }))
}

/// Get command statistics.
///
/// GET /api/commands/stats
pub async fn get_command_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    let state_stats = manager.state.stats().await;
    let queue_stats = manager.queue_stats().await;

    // Convert stats to API format
    let by_status: Vec<serde_json::Value> = state_stats
        .by_status
        .into_iter()
        .map(|(status, count)| {
            json!({
                "status": format!("{:?}", status),
                "count": count,
            })
        })
        .collect();

    ok(json!({
        "stats": {
            "total_commands": state_stats.total_count,
            "cache_size": state_stats.cache_size,
            "by_status": by_status,
            "queue": {
                "total_queued": queue_stats.total_count,
                "by_priority": queue_stats.by_priority
                    .into_iter()
                    .map(|(p, c)| json!({
                        "priority": p,
                        "count": c,
                    }))
                    .collect::<Vec<_>>(),
            }
        }
    }))
}

/// Clean up old completed commands.
///
/// POST /api/commands/cleanup
pub async fn cleanup_commands_handler(
    State(state): State<ServerState>,
    Json(body): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_command_manager(&state)?;

    // Get cleanup age (default 7 days)
    let older_than_secs = body
        .get("older_than_days")
        .and_then(|v| v.as_i64())
        .map(|d| d * 24 * 60 * 60)
        .unwrap_or(7 * 24 * 60 * 60);

    let count = manager.cleanup(older_than_secs).await;

    ok(json!({
        "cleaned_count": count,
        "message": format!("Cleaned up {} old commands", count),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_query_params_default() {
        let params: CommandQueryParams = serde_json::from_str("{}").unwrap();
        assert!(params.device_id.is_none());
        assert!(params.status.is_none());
        assert!(params.source.is_none());
        assert_eq!(params.limit, None);
        assert_eq!(params.offset, None);
    }

    #[test]
    fn test_command_query_params_with_filters() {
        let params: CommandQueryParams =
            serde_json::from_str(r#"{"device_id":"test","status":"pending","limit":10}"#).unwrap();
        assert_eq!(params.device_id, Some("test".to_string()));
        assert_eq!(params.status, Some("pending".to_string()));
        assert_eq!(params.limit, Some(10));
    }
}
