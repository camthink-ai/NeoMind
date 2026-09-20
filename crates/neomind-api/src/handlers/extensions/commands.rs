//! `commands` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use super::*;

/// Request to execute an extension command.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    /// Command name
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: serde_json::Value,
}

/// POST /api/extensions/:id/command
/// Execute a command on an extension.
///
/// Includes panic protection to prevent server crashes from buggy extensions.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/command",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = ExecuteCommandRequest,
    responses(
        (status = 200, description = "Command dispatched; result payload returned"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn execute_extension_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists first
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!(
            "Extension '{}' not found",
            id
        )));
    }

    // Validate command name (non-empty, max 256 chars, no control characters)
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate args payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.args).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Command arguments too large (max 1 MB)",
            ));
        }
    }

    // Execute command with panic protection
    let result = match runtime.execute_command(&id, &req.command, &req.args).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                error = %e,
                "Extension command execution failed"
            );
            return Err(ErrorResponse::internal(format!(
                "Command execution failed: {}",
                e
            )));
        }
    };

    // Publish ExtensionOutput events for agent triggers and dashboard subscriptions
    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(result)
}

/// Request to invoke an extension.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct InvokeExtensionRequest {
    /// Command/function to invoke
    pub command: String,
    /// Input parameters
    #[serde(default)]
    pub params: serde_json::Value,
}

/// POST /api/extensions/:id/invoke
/// Invoke an extension and get JSON response.
///
/// This is a simplified version of execute_extension_command_handler
/// that returns results in a more JSON-friendly format for AI agents.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/invoke",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = InvokeExtensionRequest,
    responses(
        (status = 200, description = "Tool-style invocation of an extension entry point"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn invoke_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<InvokeExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Validate command name
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate params payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.params).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Invoke parameters too large (max 1 MB)",
            ));
        }
    }

    // Execute command with proper error logging
    let result = match runtime
        .execute_command(&id, &req.command, &req.params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                error = %e,
                "Extension invoke failed"
            );
            return Err(ErrorResponse::internal(format!("Invoke failed: {}", e)));
        }
    };

    // Publish ExtensionOutput events with timeout protection
    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(serde_json::json!({
        "extension_id": id,
        "command": req.command,
        "result": result,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// GET /api/extensions/:id/stream
/// Get streaming output from an extension.
///
/// Returns information about the extension's stream capability.
/// For actual streaming, clients should use WebSocket or SSE endpoints.
///
/// V2: Streaming capability not declared in command config.
/// This endpoint always returns false for streaming support.
pub async fn stream_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if extension exists and get its info
    let info = state.extensions.runtime.get_info(&id).await;
    let Some(info) = info else {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    };

    // Isolated extensions support streaming via WebSocket
    let has_streaming = info.is_isolated && info.is_running;
    let stream_url = if has_streaming {
        format!("/api/extensions/{}/stream", id)
    } else {
        String::new()
    };

    ok(serde_json::json!({
        "extension_id": id,
        "supports_streaming": has_streaming,
        "stream_url": stream_url,
        "output_mode": if has_streaming { "stream" } else { "once" },
    }))
}

// ============================================================================
// Command-Based Extension DTOs and Handlers
// ============================================================================

/// Command descriptor DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptorDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_fields: Vec<OutputFieldDto>,
    pub config: CommandConfigDto,
    /// True when this command is excluded from the LLM tool registry — either
    /// named in the parent extension's `disabled_commands` list, or the parent
    /// extension's master `enabled` flag is off.
    #[serde(default)]
    pub disabled: bool,
}

/// Output field DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFieldDto {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub is_primary: bool,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// Command configuration DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfigDto {
    pub requires_auth: bool,
    pub timeout_ms: u64,
    pub is_stream: bool,
    pub expected_duration_ms: Option<u64>,
}

/// Data source info DTO
/// Matches frontend ExtensionDataSourceInfo interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceInfoDto {
    pub id: String, // Format: "extension:{extension_id}:{command}:{field}"
    pub extension_id: String,
    pub command: String,
    pub field: String,
    pub display_name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// GET /api/extensions/:id/commands
///
/// List all commands for an extension (V2 format)
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/commands",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Toolbox commands exposed by an extension"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn list_extension_commands_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<CommandDescriptorDto>> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Load persisted disable state so `disabled` flag reflects current setting.
    let (ext_enabled, disabled_cmds): (bool, std::collections::HashSet<String>) =
        match state.extensions.store.load(&id).ok().flatten() {
            Some(r) => (r.enabled, r.disabled_commands.iter().cloned().collect()),
            None => (true, Default::default()),
        };

    let result: Vec<CommandDescriptorDto> = info
        .commands
        .iter()
        .map(|cmd| CommandDescriptorDto {
            id: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.description.clone(),
            input_schema: build_parameters_schema(&cmd.parameters),
            output_fields: vec![], // V2: Commands don't declare output fields
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 300000,
                is_stream: false,
                expected_duration_ms: None,
            },
            disabled: !ext_enabled || disabled_cmds.contains(&cmd.name),
        })
        .collect();

    ok(result)
}

/// Request body for tool-enable toggles.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct SetToolEnabledRequest {
    pub enabled: bool,
}

/// Rebuild the ToolRegistry disabled set from the on-disk state of all
/// extensions and push it live. Called from both PATCH handlers below so the
/// LLM picks up the change on the next tool-calling round without a server
/// restart. Cheap: O(n_extensions × n_commands) string formatting.
pub(crate) async fn refresh_tool_registry_disabled(state: &ServerState) {
    let Some(registry) = state.agents.session_manager.get_tool_registry().await else {
        return;
    };

    let mut disabled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let store = state.extensions.store.clone();
    {
        if let Ok(records) = store.load_all() {
            for r in records {
                if !r.enabled {
                    // Master off: every command of this extension is hidden.
                    // We don't know all command names here cheaply, so we mark
                    // by prefix-lookup at the registry level using the disabled
                    // set as exact names; the registry's `definitions_for_llm`
                    // also needs a prefix mode. Simpler: walk the loaded
                    // extension runtime to enumerate command names.
                    if let Some(info) = state.extensions.runtime.get(&r.id).await {
                        for cmd in &info.commands {
                            disabled.insert(format!("{}:{}", r.id, cmd.name));
                        }
                    }
                    continue;
                }
                for cmd_name in &r.disabled_commands {
                    disabled.insert(format!("{}:{}", r.id, cmd_name));
                }
            }
        }
    }
    registry.set_disabled(disabled);
}

/// PATCH /api/extensions/:id/commands/:cmd/enabled
///
/// Per-command tool-toggle. `enabled=false` adds the command to
/// `disabled_commands`; `enabled=true` removes it. Master `enabled` flag is
/// untouched. Storage is the source of truth; the live ToolRegistry is
/// refreshed from storage after the write.
#[utoipa::path(
    patch,
    path = "/api/extensions/{id}/commands/{cmd}/enabled",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
        ("cmd" = String, Path, description = "Command name"),
    ),
    request_body = SetToolEnabledRequest,
    responses(
        (status = 200, description = "Per-command enable flag updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn set_extension_command_enabled_handler(
    State(state): State<ServerState>,
    Path((id, cmd)): Path<(String, String)>,
    Json(req): Json<SetToolEnabledRequest>,
) -> HandlerResult<serde_json::Value> {
    validate_extension_id(&id)?;
    if cmd.is_empty() {
        return Err(ErrorResponse::bad_request(
            "Command name required".to_string(),
        ));
    }

    // Shared pre-opened store from ServerState (was: per-request
    // ExtensionStore::open, whose failure swallowed into a 500).
    let store = state.extensions.store.clone();
    let mut record = store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Load extension: {e}")))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {id}")))?;

    if req.enabled {
        record.disabled_commands.retain(|c| c != &cmd);
    } else if !record.disabled_commands.iter().any(|c| c == &cmd) {
        record.disabled_commands.push(cmd.clone());
    }
    record.touch();
    store
        .save(&record)
        .map_err(|e| ErrorResponse::internal(format!("Save extension: {e}")))?;

    refresh_tool_registry_disabled(&state).await;

    tracing::info!(
        extension = %id,
        command = %cmd,
        enabled = req.enabled,
        "extension command tool-toggle updated"
    );
    ok(json!({ "id": id, "command": cmd, "enabled": req.enabled }))
}
