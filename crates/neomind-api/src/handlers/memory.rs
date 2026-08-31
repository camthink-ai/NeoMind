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

use super::ServerState;

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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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

/// Get the memory store
fn get_memory_store(_state: &ServerState) -> MarkdownMemoryStore {
    MarkdownMemoryStore::new(neomind_core::paths::data_dir().join("memory"))
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
pub async fn update_category(
    State(state): State<ServerState>,
    Path(category): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> Response {
    let cat = match MemoryCategory::parse_category(&category) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Invalid category: {}", category),
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

    match store.write_category(&cat, &req.content) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "message": format!("Category {} updated", cat.display_name())
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write category: {}", e),
        ),
    }
}

/// GET /api/memory/stats - Get all file statistics (unified response)
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
pub async fn get_config(State(_state): State<ServerState>) -> Response {
    let config = MemoryConfig::load();
    Json(config).into_response()
}

/// PUT /api/memory/config - Update memory configuration
pub async fn update_config(
    State(_state): State<ServerState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Response {
    match req.config.save() {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "config": req.config
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save config: {}", e),
        ),
    }
}

/// POST /api/memory/add - Manually add a memory entry
#[allow(deprecated)]
pub async fn add_memory_entry(
    State(state): State<ServerState>,
    Json(req): Json<AddMemoryRequest>,
) -> Response {
    // Parse category
    let category = match MemoryCategory::parse_category(&req.category) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid category: {}. Valid: user_profile, domain_knowledge, task_patterns, system_evolution",
                    req.category
                ),
            )
        }
    };

    let store = state.agents.system_memory_store.clone();

    // Read existing content
    let mut content = match store.read_category(&category) {
        Ok(c) => c,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read category: {}", e),
            )
        }
    };

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

    // Write back
    match store.write_category(&category, &content) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "message": format!("Memory added to {}", category.display_name())
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write memory: {}", e),
        ),
    }
}

/// POST /api/memory/compress - Trigger manual eviction
pub async fn trigger_compress(State(state): State<ServerState>) -> Response {
    use neomind_agent::memory::compressor::evict_to_limit;

    tracing::info!("Starting memory eviction request");

    let config = neomind_storage::MemoryConfig::load();
    let store = (*state.agents.system_memory_store).clone();

    // Evict user file to limit
    let user_content = store.read_file("user").await.unwrap_or_default();
    let user_result = evict_to_limit(&user_content, config.user_char_limit);
    let mut total_evicted = 0;
    if user_result.evicted {
        if let Err(e) = store.write_file("user", &user_result.content).await {
            tracing::warn!(error = %e, "Failed to write evicted user content");
        } else {
            total_evicted += user_result.lines_removed;
        }
    }

    // Evict knowledge file to limit
    let knowledge_content = store.read_file("knowledge").await.unwrap_or_default();
    let knowledge_result = evict_to_limit(&knowledge_content, config.knowledge_char_limit);
    if knowledge_result.evicted {
        if let Err(e) = store
            .write_file("knowledge", &knowledge_result.content)
            .await
        {
            tracing::warn!(error = %e, "Failed to write evicted knowledge content");
        } else {
            total_evicted += knowledge_result.lines_removed;
        }
    }

    // Evict procedures file to limit
    let procedures_content = store.read_file("procedures").await.unwrap_or_default();
    let procedures_result = evict_to_limit(&procedures_content, config.procedures_char_limit);
    if procedures_result.evicted {
        if let Err(e) = store
            .write_file("procedures", &procedures_result.content)
            .await
        {
            tracing::warn!(error = %e, "Failed to write evicted procedures content");
        } else {
            total_evicted += procedures_result.lines_removed;
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

    Json(serde_json::json!({
        "success": true,
        "lines_evicted": total_evicted,
        "message": message
    }))
    .into_response()
}

/// GET /api/memory/export - Export all categories as Markdown
#[allow(deprecated)]
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
pub async fn update_memory_content(
    State(state): State<ServerState>,
    Path((source_type, id)): Path<(String, String)>,
    Json(request): Json<UpdateMemoryRequest>,
) -> Response {
    let store = get_memory_store(&state);

    match store.write_raw_markdown(&source_type, &id, &request.content) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "message": "Memory updated"
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update memory: {}", e),
        ),
    }
}

/// DELETE /api/memory/:source_type/:id - Delete a memory file
#[allow(deprecated)]
pub async fn delete_memory_file(
    State(state): State<ServerState>,
    Path((source_type, id)): Path<(String, String)>,
) -> Response {
    let store = get_memory_store(&state);

    match store.delete_file(&source_type, &id) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "message": "Memory file deleted"
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete memory: {}", e),
        ),
    }
}

/// GET /api/memory/export - Export all memory as a single Markdown (Legacy)
pub async fn export_memory(State(state): State<ServerState>) -> Response {
    // Use the new export_all internally
    export_all(State(state)).await
}

/// GET /api/memory/file/:target - Get memory file content (user, knowledge, or procedures)
pub async fn get_memory_file(
    State(state): State<ServerState>,
    Path(target): Path<String>,
) -> Response {
    if !matches!(target.as_str(), "user" | "knowledge" | "procedures") {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                target
            ),
        );
    }

    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.read_file(&target).await {
        Ok(content) => Json(serde_json::json!({
            "success": true,
            "target": target,
            "content": content,
            "chars": content.chars().count(),
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read {}: {}", target, e),
        ),
    }
}

/// PUT /api/memory/file/:target - Update memory file content (user, knowledge, or procedures)
pub async fn update_memory_file(
    State(state): State<ServerState>,
    Path(target): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> Response {
    if !matches!(target.as_str(), "user" | "knowledge" | "procedures") {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                target
            ),
        );
    }

    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.write_file(&target, &req.content).await {
        Ok(()) => {
            tracing::info!(target = %target, chars = req.content.len(), "Memory file updated via API");
            Json(serde_json::json!({
                "success": true,
                "message": format!("{} updated", target),
                "chars": req.content.chars().count(),
            }))
            .into_response()
        }
        Err(e) => error_response(
            StatusCode::BAD_REQUEST,
            format!("Failed to write {}: {}", target, e),
        ),
    }
}

// ============================================================================
// Custom Files API
// ============================================================================

/// GET /api/memory/custom - List all custom files
pub async fn list_custom_files(State(state): State<ServerState>) -> Response {
    let store = get_memory_store(&state);
    if let Err(e) = store.init() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to initialize store: {}", e),
        );
    }

    match store.list_custom_files() {
        Ok(files) => {
            let files_json: Vec<serde_json::Value> = files
                .into_iter()
                .map(|(name, chars)| {
                    serde_json::json!({
                        "name": name,
                        "chars": chars,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "success": true,
                "files": files_json,
            }))
            .into_response()
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list custom files: {}", e),
        ),
    }
}

/// GET /api/memory/custom/:name - Read a custom file
pub async fn get_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> Response {
    let store = get_memory_store(&state);

    match store.read_custom_file(&name) {
        Ok(content) => Json(serde_json::json!({
            "success": true,
            "name": name,
            "content": content,
            "chars": content.chars().count(),
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::NOT_FOUND,
            format!("Custom file not found: {}", e),
        ),
    }
}

/// PUT /api/memory/custom/:name - Create or update a custom file
pub async fn update_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateMemoryRequest>,
) -> Response {
    let store = get_memory_store(&state);

    match store.write_custom_file(&name, &req.content) {
        Ok(()) => {
            tracing::info!(name = %name, chars = req.content.len(), "Custom memory file updated via API");
            Json(serde_json::json!({
                "success": true,
                "message": format!("Custom file '{}' updated", name),
                "chars": req.content.chars().count(),
            }))
            .into_response()
        }
        Err(e) => error_response(
            StatusCode::BAD_REQUEST,
            format!("Failed to write custom file '{}': {}", name, e),
        ),
    }
}

/// DELETE /api/memory/custom/:name - Delete a custom file
pub async fn delete_custom_file(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> Response {
    let store = get_memory_store(&state);

    match store.delete_custom_file(&name) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "message": format!("Custom file '{}' deleted", name),
        }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete custom file '{}': {}", name, e),
        ),
    }
}
