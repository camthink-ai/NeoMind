//! `lifecycle` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use super::*;

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
    /// Filter by state
    pub state: Option<String>,
}

/// Request to register an extension.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct RegisterExtensionRequest {
    /// Path to the extension file
    pub file_path: String,
    /// Whether to auto-start the extension
    #[serde(default)]
    pub auto_start: bool,
}

/// GET /api/extensions
/// List all registered extensions (including failed to load).
#[utoipa::path(
    get,
    path = "/api/extensions",
    tag = "extensions",
    params(
        ("state" = Option<String>, Query, description = "Filter by lifecycle state"),
    ),
    responses(
        (status = 200, description = "Registered extensions"),
    )
)]
pub async fn list_extensions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListExtensionsQuery>,
) -> HandlerResult<Vec<ExtensionDto>> {
    // Use unified service to get successfully loaded extensions
    let loaded_extensions = state.extensions.runtime.list().await;

    // Also get all extension records from storage (including failed ones)
    let stored_records = state.extensions.store.load_all().unwrap_or_default();

    // Build a set of loaded extension IDs for quick lookup
    let loaded_ids: std::collections::HashSet<String> = loaded_extensions
        .iter()
        .map(|e| e.metadata.id.clone())
        .collect();

    let mut extensions: Vec<ExtensionDto> = Vec::new();

    // First, add all successfully loaded extensions
    for info in loaded_extensions {
        extensions.push(extension_info_to_dto(&info, &state.extensions.store));
    }

    // Then, add extensions from storage that failed to load
    for record in stored_records {
        // Skip if already in loaded extensions
        if loaded_ids.contains(&record.id) {
            continue;
        }

        // Skip uninstalled extensions
        if record.uninstalled {
            continue;
        }

        // Create DTO for failed extension
        extensions.push(ExtensionDto {
            id: record.id.clone(),
            name: record.name,
            version: record.version,
            description: record.description,
            author: record.author,
            state: "Failed".to_string(),
            file_path: Some(record.file_path),
            loaded_at: None,
            health_status: record.health_status,
            last_error: record.last_error,
            last_error_at: record.last_error_at,
            consecutive_crashes: 0,
            last_crash_reason: None,
            commands: Vec::new(),
            metrics: Vec::new(),
            config_parameters: None,
            enabled: record.enabled,
            disabled_commands: record.disabled_commands,
        });
    }

    // Filter by state
    if let Some(state_filter) = &query.state {
        extensions.retain(|e| e.state.to_lowercase() == state_filter.to_lowercase());
    }

    ok(extensions)
}

/// GET /api/extensions/:id
/// Get a specific extension.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension metadata"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionDto> {
    // Use unified service to get both in-process and isolated extensions
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    ok(extension_info_to_dto(&info, &state.extensions.store))
}

/// GET /api/extensions/types
/// List available extension types.
#[utoipa::path(
    get,
    path = "/api/extensions/types",
    tag = "extensions",
    responses(
        (status = 200, description = "Static list of built-in extension types"),
    )
)]
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

/// POST /api/extensions
/// Register a new extension from file path.
#[utoipa::path(
    post,
    path = "/api/extensions",
    tag = "extensions",
    request_body = RegisterExtensionRequest,
    responses(
        (status = 200, description = "Extension registered from a path"),
    )
)]
pub async fn register_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Store the resolved canonical path: load_from_storage replays
    // record.file_path verbatim on every boot, so persisting the raw
    // request string would keep an outside-data-dir path loadable forever
    // (the confinement would only ever check the write side).
    let path = resolve_confined_package_path(&req.file_path)?;

    let metadata = runtime.load(&path).await.map_err(|e| {
        // Check for specific error types to return appropriate HTTP status codes
        let error_msg = e.to_string();
        if error_msg.contains("already registered") || error_msg.contains("Already registered") {
            ErrorResponse::conflict(format!("Extension already registered: {}", error_msg))
        } else if error_msg.contains("not found") || error_msg.contains("NotFound") {
            ErrorResponse::not_found(format!("Extension file not found: {}", error_msg))
        } else if error_msg.contains("incompatible") || error_msg.contains("Incompatible") {
            ErrorResponse::validation(format!("Incompatible extension: {}", error_msg))
        } else {
            ErrorResponse::bad_request(format!("Failed to load extension: {}", error_msg))
        }
    })?;

    let ext_id = metadata.id.clone();
    let ext_name = metadata.name.clone();
    let ext_version = metadata.version.to_string();

    // Save to persistent storage for auto-load on server restart
    // V2: Use empty string for extension_type (storage API still requires it)
    let store = state.extensions.store.clone();
    {
        let record = neomind_storage::ExtensionRecord::new(
            ext_id.clone(),
            ext_name.clone(),
            // Canonical, data-dir-confined path (NOT the raw request value) —
            // replayed verbatim by load_from_storage on every boot.
            path.display().to_string(),
            String::new(), // V2: No extension_type, use empty string
            ext_version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_auto_start(req.auto_start);

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
            // Don't fail the request if storage fails, extension is already loaded in memory
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    ok(serde_json::json!({
        "message": "Extension registered successfully",
        "extension_id": ext_id,
        "name": ext_name,
        "version": ext_version,
        "auto_start": req.auto_start
    }))
}

/// DELETE /api/extensions/:id
/// Unregister an extension.
#[utoipa::path(
    delete,
    path = "/api/extensions/{id}",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension unregistered (files kept)"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn unregister_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists in memory or storage
    let in_memory = runtime.contains(&id).await;
    let in_storage = state.extensions.store.load(&id).ok().flatten().is_some();

    // Extension must exist somewhere to unregister
    if !in_memory && !in_storage {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Unregister from memory if present (handles both in-process and isolated)
    if in_memory {
        if let Err(e) = runtime.unregister(&id).await {
            tracing::warn!(
                extension_id = %id,
                error = %e,
                "Failed to unregister extension from memory (continuing with storage cleanup)"
            );
        }
    }

    // Mark as uninstalled in storage (instead of deleting) to prevent auto-discovery
    // from re-registering it on server restart
    let store = state.extensions.store.clone();
    {
        if let Err(e) = store.mark_uninstalled(&id) {
            tracing::warn!("Failed to mark extension as uninstalled: {}", e);
        }
    }

    // Clean up extension metrics data from telemetry.redb
    // Extension metrics are stored with source_part = "extension:{extension_id}"
    cleanup_extension_metrics(&state, &id).await;

    ok(serde_json::json!({
        "message": "Extension unregistered",
        "extension_id": id
    }))
}

/// POST /api/extensions/:id/start
/// Start an extension.
///
/// Note: In the new extension system, extensions are always active once registered.
/// This endpoint exists for API compatibility only.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/start",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension started"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn start_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Extensions are always active in the new system
    ok(serde_json::json!({
        "message": "Extension is active",
        "extension_id": id,
        "note": "Extensions are always active once registered"
    }))
}

/// POST /api/extensions/:id/stop
/// Stop an extension.
///
/// Note: In the new extension system, extensions cannot be stopped.
/// They remain active until unregistered. This endpoint exists for API compatibility only.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/stop",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension stopped"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn stop_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Extensions cannot be stopped in the new system
    ok(serde_json::json!({
        "message": "Extensions cannot be stopped",
        "extension_id": id,
        "note": "To deactivate an extension, unregister it instead"
    }))
}

/// GET /api/extensions/:id/health
/// Check extension health.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/health",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension health snapshot"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn extension_health_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let healthy = runtime
        .health_check(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Health check failed: {}", e)))?;

    ok(serde_json::json!({
        "extension_id": id,
        "healthy": healthy
    }))
}
