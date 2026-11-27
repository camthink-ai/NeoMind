//! System Memory API handlers
//!
//! Provides endpoints for managing the Markdown-based system memory.
//! Supports both file-based API (legacy) and category-based API (new).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use neomind_storage::{
    CategoryStats, MarkdownMemoryStore, MemoryCategory, MemoryConfig, MemoryFileInfo,
};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

// ============================================================================
// Response Types
// ============================================================================

/// Response for memory file list
#[derive(Debug, Serialize)]
pub struct MemoryFileListResponse {
    pub files: Vec<MemoryFileInfo>,
    pub total: usize,
}

/// Response for single file content
#[derive(Debug, Serialize)]
pub struct MemoryFileContentResponse {
    pub id: String,
    pub source_type: String,
    pub content: String,
}

/// Request to update file content
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct UpdateMemoryRequest {
    pub content: String,
}

// ============================================================================
// Category-based API Types (New)
// ============================================================================

/// Response for category content
#[derive(Debug, Serialize)]
pub struct CategoryContentResponse {
    pub category: String,
    pub content: String,
    pub stats: CategoryStats,
}

/// Response for all stats
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub categories: HashMap<String, CategoryStats>,
    pub config: Option<MemoryConfig>,
}

/// Request to update config
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub config: MemoryConfig,
}

/// Request to add a manual memory entry
#[derive(Debug, Deserialize)]
pub struct AddMemoryRequest {
    /// Category to add to
    pub category: String,
    /// Memory content
    pub content: String,
    /// Importance score (0-100)
    pub importance: u8,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Maximum character count allowed on legacy memory write endpoints.
const LEGACY_MEMORY_MAX_CHARS: usize = 100_000;

/// Get the memory store pinned to the server's canonical memory base path
/// (never trusting a user-modified `storage_path`).
fn get_memory_store(state: &ServerState) -> MarkdownMemoryStore {
    let mut config = neomind_storage::MemoryConfig::load();
    let base_path = state.agents.system_memory_store.base_path().to_path_buf();
    config.storage_path = base_path.to_string_lossy().to_string();
    config.clamp_bounds();
    MarkdownMemoryStore::with_config(base_path, config)
}

/// Create error response
fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": message.into()
        })),
    )
        .into_response()
}

// ============================================================================
// Category-based API Handlers (New)
// ============================================================================

/// GET /api/memory/category/:category - Get category content
#[allow(deprecated)]
#[utoipa::path(
    get,
    path = "/api/memory/category/{category}",
    tag = "memory",
    params(
        ("category" = String, Path, description = "Memory category"),
    ),
    responses(
        (status = 200, description = "Markdown of one memory category"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_category(
    State(state): State<ServerState>,
    Path(category): Path<String>,
) -> Response {
    let cat = match MemoryCategory::parse_category(&category) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Invalid category: {}. Valid: user_profile, domain_knowledge, task_patterns, system_evolution", category),
            )
        }
    };

    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.read_category(&cat) {
        Ok(content) => {
            let stats = store.category_stats(&cat).unwrap_or_default();
            Json(CategoryContentResponse {
                category: cat.to_string(),
                content,
                stats,
            })
            .into_response()
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read category: {}", e),
        ),
    }
}

/// PUT /api/memory/category/:category - Update category content
#[allow(deprecated)]
#[utoipa::path(
    put,
    path = "/api/memory/category/{category}",
    tag = "memory",
    params(
        ("category" = String, Path, description = "Memory category"),
    ),
    request_body = UpdateMemoryRequest,
    responses(
        (status = 200, description = "Category markdown saved"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_category(
    State(state): State<ServerState>,
    Path(category): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    let cat = MemoryCategory::parse_category(&category).ok_or_else(|| {
        ErrorResponse::bad_request(format!("Invalid category: {}", category)).with_hint(
            "Valid categories: user_profile, domain_knowledge, task_patterns, system_evolution",
        )
    })?;

    if req.content.chars().count() > LEGACY_MEMORY_MAX_CHARS {
        return Err(ErrorResponse::bad_request(format!(
            "Content exceeds maximum allowed length of {} characters",
            LEGACY_MEMORY_MAX_CHARS
        )));
    }

    let store = get_memory_store(&state);
    store
        .init()
        .map_err(|e| ErrorResponse::internal(format!("Failed to initialize store: {}", e)))?;

    store
        .write_category(&cat, &req.content)
        .map_err(|e| ErrorResponse::internal(format!("Failed to write category: {}", e)))?;

    ok(serde_json::json!({
        "message": format!("Category {} updated", cat.display_name())
    }))
}

/// GET /api/memory/stats - Get all file statistics (unified response)
#[utoipa::path(
    get,
    path = "/api/memory/stats",
    tag = "memory",
    responses(
        (status = 200, description = "Memory size/entry counters"),
    )
)]
pub async fn get_stats(State(state): State<ServerState>) -> Response {
    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.stats().await {
        Ok(mem_stats) => {
            use std::collections::HashMap as Map;
            let mut files: Map<String, serde_json::Value> = Map::new();
            files.insert(
                "user".to_string(),
                serde_json::json!({
                    "chars": mem_stats.user.chars,
                    "modified_at": 0,
                }),
            );
            files.insert(
                "knowledge".to_string(),
                serde_json::json!({
                    "chars": mem_stats.knowledge.chars,
                    "modified_at": 0,
                }),
            );
            files.insert(
                "procedures".to_string(),
                serde_json::json!({
                    "chars": mem_stats.procedures.chars,
                    "modified_at": 0,
                }),
            );

            let custom_files: Vec<serde_json::Value> = mem_stats
                .custom_files
                .into_iter()
                .map(|cf| {
                    serde_json::json!({
                        "name": cf.name,
                        "chars": cf.chars,
                    })
                })
                .collect();

            Json(serde_json::json!({
                "files": files,
                "custom_files": custom_files,
            }))
            .into_response()
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get stats: {}", e),
        ),
    }
}

/// GET /api/memory/config - Get memory configuration
#[utoipa::path(
    get,
    path = "/api/memory/config",
    tag = "memory",
    responses(
        (status = 200, description = "Memory subsystem configuration"),
    )
)]
pub async fn get_config(State(state): State<ServerState>) -> Response {
    let mut config = MemoryConfig::load();
    config.storage_path = state
        .agents
        .system_memory_store
        .base_path()
        .to_string_lossy()
        .to_string();
    config.clamp_bounds();
    Json(config).into_response()
}

/// PUT /api/memory/config - Update memory configuration
#[utoipa::path(
    put,
    path = "/api/memory/config",
    tag = "memory",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "Memory configuration saved"),
    )
)]
pub async fn update_config(
    State(state): State<ServerState>,
    Json(mut req): Json<UpdateConfigRequest>,
) -> HandlerResult<serde_json::Value> {
    // Always pin storage_path to the server's canonical memory store path so
    // callers cannot redirect reads/writes to arbitrary filesystem directories.
    req.config.storage_path = state
        .agents
        .system_memory_store
        .base_path()
        .to_string_lossy()
        .to_string();
    // Clamp numeric bounds to prevent memory exhaustion, chrono overflow, or tight loops.
    req.config.clamp_bounds();

    req.config
        .save()
        .map_err(|e| ErrorResponse::internal(format!("Failed to save config: {}", e)))?;

    ok(serde_json::json!({
        "config": req.config
    }))
}

/// POST /api/memory/add - Manually add a memory entry
#[allow(deprecated)]
pub async fn add_memory_entry(
    State(state): State<ServerState>,
    Json(req): Json<AddMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    // Parse category
    let category = MemoryCategory::parse_category(&req.category).ok_or_else(|| {
        ErrorResponse::bad_request(format!("Invalid category: {}", req.category)).with_hint(
            "Valid categories: user_profile, domain_knowledge, task_patterns, system_evolution",
        )
    })?;

    if req.content.chars().count() > LEGACY_MEMORY_MAX_CHARS {
        return Err(ErrorResponse::bad_request(format!(
            "Content exceeds maximum allowed length of {} characters",
            LEGACY_MEMORY_MAX_CHARS
        )));
    }

    let store = state.agents.system_memory_store.clone();

    // Read existing content
    let mut content = store
        .read_category(&category)
        .map_err(|e| ErrorResponse::internal(format!("Failed to read category: {}", e)))?;

    // Format the entry
    let timestamp = chrono::Utc::now().format("%Y-%m-%d");
    let entry = format!(
        "- [{}] {} [importance: {}]\n",
        timestamp, req.content, req.importance
    );

    // Append to content
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&entry);

    if content.chars().count() > LEGACY_MEMORY_MAX_CHARS {
        return Err(ErrorResponse::bad_request(format!(
            "Memory category would exceed maximum allowed length of {} characters",
            LEGACY_MEMORY_MAX_CHARS
        )));
    }

    // Write back
    store
        .write_category(&category, &content)
        .map_err(|e| ErrorResponse::internal(format!("Failed to write memory: {}", e)))?;

    ok(serde_json::json!({
        "message": format!("Memory added to {}", category.display_name())
    }))
}

/// POST /api/memory/compress - Trigger manual eviction
#[utoipa::path(
    post,
    path = "/api/memory/compress",
    tag = "memory",
    responses(
        (status = 200, description = "Memory compression triggered"),
    )
)]
pub async fn trigger_compress(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_agent::memory::compressor::evict_to_limit;

    tracing::info!("Starting memory eviction request");

    let config = neomind_storage::MemoryConfig::load();
    // Clone the Arc handle, keeping identity with the canonical store the
    // background jobs write through (store clones share the locks since the
    // manual Clone, but the Arc leaves no room for future drift).
    let store = state.agents.system_memory_store.clone();

    let mut total_evicted = 0usize;
    for (target, limit) in [
        ("user", config.user_char_limit),
        ("knowledge", config.knowledge_char_limit),
        ("procedures", config.procedures_char_limit),
    ] {
        // Atomic read-evict-write under the shared write lock — a plain
        // read_file→write_file pair could revert a concurrent background
        // marker write landing between its two halves.
        let mut lines_removed = 0usize;
        let result = store
            .modify_file(target, |current| {
                let r = evict_to_limit(current, limit);
                lines_removed = r.lines_removed;
                r.content
            })
            .await;
        match result {
            Ok(true) => total_evicted += lines_removed,
            Ok(false) => {} // within limit, nothing evicted
            Err(e) => {
                tracing::warn!(target, error = %e, "Failed to evict memory file");
            }
        }
    }

    let message = if total_evicted == 0 {
        "No eviction needed — all files within char limits.".to_string()
    } else {
        format!(
            "Eviction completed: {} lines removed from files exceeding limits",
            total_evicted
        )
    };

    ok(serde_json::json!({
        "lines_evicted": total_evicted,
        "message": message
    }))
}

/// GET /api/memory/export - Export all categories as Markdown
#[allow(deprecated)]
#[utoipa::path(
    get,
    path = "/api/memory/export",
    tag = "memory",
    responses(
        (status = 200, description = "Full memory export (JSON)"),
    )
)]
pub async fn export_all(State(state): State<ServerState>) -> Response {
    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.export_all() {
        Ok(markdown) => (
            StatusCode::OK,
            [("Content-Type", "text/markdown; charset=utf-8")],
            markdown,
        )
            .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to export: {}", e),
        ),
    }
}

// ============================================================================
// File-based API Handlers (Legacy - for backward compatibility)
// ============================================================================

/// GET /api/memory - List all memory files
#[allow(deprecated)]
#[utoipa::path(
    get,
    path = "/api/memory",
    tag = "memory",
    responses(
        (status = 200, description = "All memory files by category"),
    )
)]
pub async fn get_all_memory(State(state): State<ServerState>) -> Response {
    let store = get_memory_store(&state);

    // Ensure initialized
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize memory store: {}", e),
        );
    }

    match store.list_files() {
        Ok(files) => {
            let total = files.len();
            Json(MemoryFileListResponse { files, total }).into_response()
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list memory files: {}", e),
        ),
    }
}

/// GET /api/memory/:source_type/:id - Get raw markdown content
#[allow(deprecated)]
#[utoipa::path(
    get,
    path = "/api/memory/{source_type}/{id}",
    tag = "memory",
    params(
        ("source_type" = String, Path, description = "Memory source type (agent | session | skill)"),
        ("id" = String, Path, description = "Resource id"),
    ),
    responses(
        (status = 200, description = "Markdown content of one memory entry"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_memory_content(
    State(state): State<ServerState>,
    Path((source_type, id)): Path<(String, String)>,
) -> Response {
    let store = get_memory_store(&state);

    match store.read_raw_markdown(&source_type, &id) {
        Ok(content) => Json(MemoryFileContentResponse {
            id,
            source_type,
            content,
        })
        .into_response(),
        Err(e) => error_response(
            StatusCode::NOT_FOUND,
            format!("Memory file not found: {}", e),
        ),
    }
}

/// PUT /api/memory/:source_type/:id - Update markdown content
#[allow(deprecated)]
#[utoipa::path(
    put,
    path = "/api/memory/{source_type}/{id}",
    tag = "memory",
    params(
        ("source_type" = String, Path, description = "Memory source type (agent | session | skill)"),
        ("id" = String, Path, description = "Resource id"),
    ),
    request_body = UpdateMemoryRequest,
    responses(
        (status = 200, description = "Memory entry markdown saved"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_memory_content(
    State(state): State<ServerState>,
    Path((source_type, id)): Path<(String, String)>,
    Json(request): Json<UpdateMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    if request.content.chars().count() > LEGACY_MEMORY_MAX_CHARS {
        return Err(ErrorResponse::bad_request(format!(
            "Content exceeds maximum allowed length of {} characters",
            LEGACY_MEMORY_MAX_CHARS
        )));
    }

    let store = get_memory_store(&state);

    store
        .write_raw_markdown(&source_type, &id, &request.content)
        .map_err(|e| match e {
            neomind_storage::Error::InvalidInput(msg) => ErrorResponse::bad_request(msg),
            other => ErrorResponse::internal(format!("Failed to update memory: {}", other)),
        })?;

    ok(serde_json::json!({
        "message": "Memory updated"
    }))
}

/// DELETE /api/memory/:source_type/:id - Delete a memory file
#[allow(deprecated)]
#[utoipa::path(
    delete,
    path = "/api/memory/{source_type}/{id}",
    tag = "memory",
    params(
        ("source_type" = String, Path, description = "Memory source type (agent | session | skill)"),
        ("id" = String, Path, description = "Resource id"),
    ),
    responses(
        (status = 200, description = "Memory entry deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_memory_file(
    State(state): State<ServerState>,
    Path((source_type, id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let store = get_memory_store(&state);

    store.delete_file(&source_type, &id).map_err(|e| {
        use neomind_storage::Error as StoreError;
        match e {
            // "system" (and any other non-agent/chat source_type) is a
            // client mistake — 400, not 500.
            StoreError::Storage(ref msg) if msg.contains("Cannot delete system memory") => {
                ErrorResponse::bad_request(format!("Failed to delete memory: {e}"))
            }
            StoreError::NotFound(ref msg) => {
                ErrorResponse::not_found(format!("Memory file ({msg})"))
            }
            other => ErrorResponse::internal(format!("Failed to delete memory: {other}")),
        }
    })?;

    ok(serde_json::json!({
        "message": "Memory file deleted"
    }))
}

/// GET /api/memory/export - Export all memory as a single Markdown (Legacy)
pub async fn export_memory(State(state): State<ServerState>) -> Response {
    // Use the new export_all internally
    export_all(State(state)).await
}

/// GET /api/memory/file/:target - Get memory file content (user, knowledge, or procedures)
#[utoipa::path(
    get,
    path = "/api/memory/file/{target}",
    tag = "memory",
    params(
        ("target" = String, Path, description = "Memory file target"),
    ),
    responses(
        (status = 200, description = "One memory file (2-file layout)"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_memory_file(
    State(state): State<ServerState>,
    Path(target): Path<String>,
) -> HandlerResult<serde_json::Value> {
    if !matches!(target.as_str(), "user" | "knowledge" | "procedures") {
        return Err(
            ErrorResponse::bad_request(format!("Invalid target: {}", target))
                .with_hint("Target must be 'user', 'knowledge', or 'procedures'."),
        );
    }

    let store = get_memory_store(&state);
    store
        .init()
        .map_err(|e| ErrorResponse::internal(format!("Failed to initialize store: {}", e)))?;

    let content = store
        .read_file(&target)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to read {}: {}", target, e)))?;

    ok(serde_json::json!({
        "target": target,
        "content": content,
        "chars": content.chars().count(),
    }))
}

/// PUT /api/memory/file/:target - Update memory file content (user, knowledge, or procedures)
#[utoipa::path(
    put,
    path = "/api/memory/file/{target}",
    tag = "memory",
    params(
        ("target" = String, Path, description = "Memory file target"),
    ),
    request_body = UpdateMemoryRequest,
    responses(
        (status = 200, description = "Memory file saved"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_memory_file(
    State(state): State<ServerState>,
    Path(target): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    if !matches!(target.as_str(), "user" | "knowledge" | "procedures") {
        return Err(
            ErrorResponse::bad_request(format!("Invalid target: {}", target))
                .with_hint("Target must be 'user', 'knowledge', or 'procedures'."),
        );
    }

    let store = get_memory_store(&state);
    store
        .init()
        .map_err(|e| ErrorResponse::internal(format!("Failed to initialize store: {}", e)))?;

    store
        .write_file(&target, &req.content)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to write {}: {}", target, e)))?;

    tracing::info!(target = %target, chars = req.content.chars().count(), "Memory file updated via API");
    ok(serde_json::json!({
        "message": format!("{} updated", target),
        "chars": req.content.chars().count(),
    }))
}

// ============================================================================
// Custom Files API
// ============================================================================

/// GET /api/memory/custom - List all custom files
#[utoipa::path(
    get,
    path = "/api/memory/custom",
    tag = "memory",
    responses(
        (status = 200, description = "Custom memory files"),
    )
)]
pub async fn list_custom_files(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = get_memory_store(&state);
    store
        .init()
        .map_err(|e| ErrorResponse::internal(format!("Failed to initialize store: {}", e)))?;

    let files = store
        .list_custom_files()
        .map_err(|e| ErrorResponse::internal(format!("Failed to list custom files: {}", e)))?;

    let files_json: Vec<serde_json::Value> = files
        .into_iter()
        .map(|(name, chars)| {
            serde_json::json!({
                "name": name,
                "chars": chars,
            })
        })
        .collect();
    ok(serde_json::json!({
        "files": files_json,
    }))
}

/// GET /api/memory/custom/:name - Read a custom file
#[utoipa::path(
    get,
    path = "/api/memory/custom/{name}",
    tag = "memory",
    params(
        ("name" = String, Path, description = "Custom memory file name"),
    ),
    responses(
        (status = 200, description = "One custom memory file"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_memory_store(&state);

    let content = store
        .read_custom_file(&name)
        .map_err(|e| ErrorResponse::not_found(format!("Custom file '{}' ({})", name, e)))?;

    ok(serde_json::json!({
        "name": name,
        "content": content,
        "chars": content.chars().count(),
    }))
}

/// PUT /api/memory/custom/:name - Create or update a custom file
#[utoipa::path(
    put,
    path = "/api/memory/custom/{name}",
    tag = "memory",
    params(
        ("name" = String, Path, description = "Custom memory file name"),
    ),
    request_body = UpdateMemoryRequest,
    responses(
        (status = 200, description = "Custom memory file saved"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    let store = get_memory_store(&state);

    store
        .write_custom_file(&name, &req.content)
        .await
        .map_err(|e| {
            ErrorResponse::bad_request(format!("Failed to write custom file '{}': {}", name, e))
        })?;

    tracing::info!(name = %name, chars = req.content.chars().count(), "Custom memory file updated via API");
    ok(serde_json::json!({
        "message": format!("Custom file '{}' updated", name),
        "chars": req.content.chars().count(),
    }))
}

/// DELETE /api/memory/custom/:name - Delete a custom file
#[utoipa::path(
    delete,
    path = "/api/memory/custom/{name}",
    tag = "memory",
    params(
        ("name" = String, Path, description = "Custom memory file name"),
    ),
    responses(
        (status = 200, description = "Custom memory file deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_memory_store(&state);

    store.delete_custom_file(&name).map_err(|e| {
        ErrorResponse::internal(format!("Failed to delete custom file '{}': {}", name, e))
    })?;

    ok(serde_json::json!({
        "message": format!("Custom file '{}' deleted", name),
    }))
}
