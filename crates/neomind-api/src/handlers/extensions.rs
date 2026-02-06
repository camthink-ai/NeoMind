//! Extension API handlers.
//!
//! Handlers for managing dynamically loaded extensions (.so/.dylib/.dll/.wasm).
//! Extensions are distinct from user configurations like LLM backends or device connections.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::models::error::ErrorResponse;
use crate::handlers::common::{ok, HandlerResult};
use crate::server::ServerState;


/// Extension DTO for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionDto {
    /// Extension ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Extension type
    pub extension_type: String,
    /// Version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// Author
    pub author: Option<String>,
    /// Current state
    pub state: String,
    /// File path
    pub file_path: Option<String>,
    /// Loaded at timestamp
    pub loaded_at: Option<i64>,
}

/// Extension statistics DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionStatsDto {
    /// Start count
    pub start_count: u64,
    /// Stop count
    pub stop_count: u64,
    /// Error count
    pub error_count: u64,
    /// Last error
    pub last_error: Option<String>,
}

/// Extension type DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionTypeDto {
    /// Type identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
}

/// Query parameters for listing extensions.
#[derive(Debug, Deserialize)]
pub struct ListExtensionsQuery {
    /// Filter by extension type
    pub extension_type: Option<String>,
    /// Filter by state
    pub state: Option<String>,
}

/// Request to register an extension.
#[derive(Debug, Deserialize)]
pub struct RegisterExtensionRequest {
    /// Path to the extension file
    pub file_path: String,
    /// Whether to auto-start the extension
    #[serde(default)]
    pub auto_start: bool,
}

/// Request to execute an extension command.
#[derive(Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    /// Command name
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: serde_json::Value,
}

/// GET /api/extensions
/// List all registered extensions.
pub async fn list_extensions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListExtensionsQuery>,
) -> HandlerResult<Vec<ExtensionDto>> {
    let registry = state.core.extension_registry.read().await;

    let mut extensions: Vec<ExtensionDto> = registry
        .list()
        .await
        .into_iter()
        .map(|info| ExtensionDto {
            id: info.metadata.id.clone(),
            name: info.metadata.name.clone(),
            extension_type: info.metadata.extension_type.as_str().to_string(),
            version: info.metadata.version.to_string(),
            description: info.metadata.description.clone(),
            author: info.metadata.author.clone(),
            state: info.state.to_string(),
            file_path: info.metadata.file_path.as_ref().map(|p| p.display().to_string()),
            loaded_at: info.loaded_at.map(|t| t.timestamp()),
        })
        .collect();

    // Filter by type
    if let Some(ext_type) = &query.extension_type {
        extensions.retain(|e| e.extension_type == *ext_type);
    }

    // Filter by state
    if let Some(state_filter) = &query.state {
        extensions.retain(|e| e.state.to_lowercase() == state_filter.to_lowercase());
    }

    ok(extensions)
}

/// GET /api/extensions/:id
/// Get a specific extension.
pub async fn get_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionDto> {
    let registry = state.core.extension_registry.read().await;

    let info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    ok(ExtensionDto {
        id: info.metadata.id.clone(),
        name: info.metadata.name.clone(),
        extension_type: info.metadata.extension_type.as_str().to_string(),
        version: info.metadata.version.to_string(),
        description: info.metadata.description.clone(),
        author: info.metadata.author.clone(),
        state: info.state.to_string(),
        file_path: info.metadata.file_path.as_ref().map(|p| p.display().to_string()),
        loaded_at: info.loaded_at.map(|t| t.timestamp()),
    })
}

/// GET /api/extensions/:id/stats
/// Get extension statistics.
pub async fn get_extension_stats_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionStatsDto> {
    let registry = state.core.extension_registry.read().await;

    let info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    ok(ExtensionStatsDto {
        start_count: info.stats.start_count,
        stop_count: info.stats.stop_count,
        error_count: info.stats.error_count,
        last_error: info.stats.last_error.clone(),
    })
}

/// GET /api/extensions/types
/// List available extension types.
pub async fn list_extension_types_handler() -> HandlerResult<Vec<ExtensionTypeDto>> {
    let types = vec![
        ExtensionTypeDto {
            id: "llm_provider".to_string(),
            name: "LLM Provider".to_string(),
            description: "Provides a new LLM backend implementation".to_string(),
        },
        ExtensionTypeDto {
            id: "device_protocol".to_string(),
            name: "Device Protocol".to_string(),
            description: "Implements a device communication protocol".to_string(),
        },
        ExtensionTypeDto {
            id: "alert_channel_type".to_string(),
            name: "Alert Channel Type".to_string(),
            description: "Provides a new alert notification channel type".to_string(),
        },
        ExtensionTypeDto {
            id: "tool".to_string(),
            name: "Tool".to_string(),
            description: "Provides AI function calling tools".to_string(),
        },
        ExtensionTypeDto {
            id: "generic".to_string(),
            name: "Generic".to_string(),
            description: "Generic extension".to_string(),
        },
    ];

    ok(types)
}

/// POST /api/extensions/discover
/// Discover extensions in configured directories.
pub async fn discover_extensions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<serde_json::Value>> {
    let registry = state.core.extension_registry.read().await;

    let discovered = registry.discover().await;

    let result: Vec<serde_json::Value> = discovered
        .into_iter()
        .map(|meta| {
            serde_json::json!({
                "id": meta.id,
                "name": meta.name,
                "version": meta.version.to_string(),
                "extension_type": meta.extension_type.as_str(),
                "file_path": meta.file_path.map(|p| p.display().to_string()),
            })
        })
        .collect();

    ok(result)
}

/// POST /api/extensions
/// Register a new extension from file path.
pub async fn register_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    let path = PathBuf::from(&req.file_path);

    // Load metadata from the extension file
    let metadata = registry
        .load_from_path(&path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to load extension: {}", e)))?;

    let ext_id = metadata.id.clone();

    // Note: Full registration with actual extension instance would require
    // loading the dynamic library and creating the extension object.
    // For now, we return the discovered metadata.

    ok(serde_json::json!({
        "message": "Extension metadata loaded",
        "extension_id": ext_id,
        "name": metadata.name,
        "version": metadata.version.to_string(),
        "extension_type": metadata.extension_type.as_str(),
        "note": "Full dynamic loading requires extension to implement the Extension trait"
    }))
}

/// DELETE /api/extensions/:id
/// Unregister an extension.
pub async fn unregister_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    // Check if extension exists
    if !registry.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    drop(registry);

    // Unregister (need write lock)
    let registry = state.core.extension_registry.write().await;
    registry
        .unregister(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to unregister: {}", e)))?;

    ok(serde_json::json!({
        "message": "Extension unregistered",
        "extension_id": id
    }))
}

/// POST /api/extensions/:id/start
/// Start an extension.
pub async fn start_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    registry
        .start(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to start extension: {}", e)))?;

    ok(serde_json::json!({
        "message": "Extension started",
        "extension_id": id
    }))
}

/// POST /api/extensions/:id/stop
/// Stop an extension.
pub async fn stop_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    registry
        .stop(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to stop extension: {}", e)))?;

    ok(serde_json::json!({
        "message": "Extension stopped",
        "extension_id": id
    }))
}

/// GET /api/extensions/:id/health
/// Check extension health.
pub async fn extension_health_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    let healthy = registry
        .health_check(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Health check failed: {}", e)))?;

    ok(serde_json::json!({
        "extension_id": id,
        "healthy": healthy
    }))
}

/// POST /api/extensions/:id/command
/// Execute a command on an extension.
pub async fn execute_extension_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.extension_registry.read().await;

    let result = registry
        .execute_command(&id, &req.command, &req.args)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Command execution failed: {}", e)))?;

    ok(result)
}
