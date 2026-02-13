//! Memory system handlers.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use neomind_memory::{
    ConversationEntry, KnowledgeCategory, KnowledgeEntry, MemoryMessage, SearchResult, TieredMemory,
};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Query parameters for memory search.
#[derive(Debug, Deserialize)]
pub struct MemoryQueryParams {
    /// Search query
    pub q: String,
    /// Maximum results per layer (default: 5)
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

/// DTO for memory stats.
#[derive(Debug, Serialize)]
struct MemoryStatsDto {
    short_term_messages: usize,
    short_term_tokens: usize,
    mid_term_entries: usize,
    long_term_entries: usize,
}

/// DTO for memory query results.
#[derive(Debug, Serialize)]
struct MemoryQueryResultDto {
    short_term: Vec<MemoryMessageDto>,
    mid_term: Vec<SearchResultDto>,
    long_term: Vec<KnowledgeEntryDto>,
}

/// DTO for short-term messages.
#[derive(Debug, Serialize)]
struct MemoryMessageDto {
    id: String,
    role: String,
    content: String,
    timestamp: i64,
    token_count: usize,
}

impl From<&MemoryMessage> for MemoryMessageDto {
    fn from(m: &MemoryMessage) -> Self {
        Self {
            id: m.id.clone(),
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
            token_count: m.token_count,
        }
    }
}

/// DTO for search results.
#[derive(Debug, Serialize)]
struct SearchResultDto {
    id: String,
    session_id: String,
    user_input: String,
    assistant_response: String,
    timestamp: i64,
    score: f32,
}

impl From<&SearchResult> for SearchResultDto {
    fn from(r: &SearchResult) -> Self {
        Self {
            id: r.entry.id.clone(),
            session_id: r.entry.session_id.clone(),
            user_input: r.entry.user_input.clone(),
            assistant_response: r.entry.assistant_response.clone(),
            timestamp: r.entry.timestamp,
            score: r.score,
        }
    }
}

/// DTO for knowledge entries.
#[derive(Debug, Serialize)]
struct KnowledgeEntryDto {
    id: String,
    title: String,
    content: String,
    category: String,
    tags: Vec<String>,
    device_ids: Vec<String>,
    created_at: i64,
    updated_at: i64,
    access_count: u64,
}

impl From<&KnowledgeEntry> for KnowledgeEntryDto {
    fn from(e: &KnowledgeEntry) -> Self {
        Self {
            id: e.id.clone(),
            title: e.title.clone(),
            content: e.content.clone(),
            category: e.category.as_str().to_string(),
            tags: e.tags.clone(),
            device_ids: e.device_ids.clone(),
            created_at: e.created_at,
            updated_at: e.updated_at,
            access_count: e.access_count,
        }
    }
}

/// DTO for conversation entries.
#[derive(Debug, Serialize)]
struct ConversationEntryDto {
    id: String,
    session_id: String,
    user_input: String,
    assistant_response: String,
    timestamp: i64,
}

impl From<&ConversationEntry> for ConversationEntryDto {
    fn from(e: &ConversationEntry) -> Self {
        Self {
            id: e.id.clone(),
            session_id: e.session_id.clone(),
            user_input: e.user_input.clone(),
            assistant_response: e.assistant_response.clone(),
            timestamp: e.timestamp,
        }
    }
}

/// Request body for adding short-term memory.
#[derive(Debug, Deserialize)]
pub struct AddShortTermMemoryRequest {
    pub role: String,
    pub content: String,
}

/// Request body for adding mid-term memory.
#[derive(Debug, Deserialize)]
pub struct AddMidTermMemoryRequest {
    pub session_id: String,
    pub user_input: String,
    pub assistant_response: String,
}

/// Request body for adding knowledge.
#[derive(Debug, Deserialize)]
pub struct AddKnowledgeRequest {
    pub title: String,
    pub content: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub device_ids: Vec<String>,
}

fn default_category() -> String {
    "best_practice".to_string()
}

/// Get memory from server state.
/// Note: This function is kept for backwards compatibility but handlers
/// should prefer using State(state).memory directly for proper configuration.
fn get_global_memory(state: &ServerState) -> Arc<tokio::sync::RwLock<TieredMemory>> {
    state.agents.memory.clone()
}

/// Get memory statistics.
///
/// GET /api/memory/stats
pub async fn get_memory_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let stats = mem.get_stats().await;

    ok(json!({
        "stats": MemoryStatsDto {
            short_term_messages: stats.short_term_messages,
            short_term_tokens: stats.short_term_tokens,
            mid_term_entries: stats.mid_term_entries,
            long_term_entries: stats.long_term_entries,
        }
    }))
}

/// Query all memory layers.
///
/// GET /api/memory/query?q=temperature&top_k=5
pub async fn query_memory_handler(
    State(state): State<ServerState>,
    Query(params): Query<MemoryQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let results = mem.query_all(&params.q, params.top_k).await;

    ok(json!({
        "query": params.q,
        "results": MemoryQueryResultDto {
            short_term: results.short_term.iter().map(MemoryMessageDto::from).collect(),
            mid_term: results.mid_term.iter().map(SearchResultDto::from).collect(),
            long_term: results.long_term.iter().map(KnowledgeEntryDto::from).collect(),
        }
    }))
}

/// Consolidate short-term to mid-term memory.
///
/// POST /api/memory/consolidate/:session_id
pub async fn consolidate_memory_handler(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    mem.consolidate(&session_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to consolidate memory: {}", e)))?;

    ok(json!({
        "message": "Memory consolidated",
        "session_id": session_id
    }))
}

// ===== Short-term memory endpoints =====

/// Get short-term memory.
///
/// GET /api/memory/short-term
pub async fn get_short_term_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let messages = mem.get_short_term();

    ok(json!({
        "messages": messages.iter().map(MemoryMessageDto::from).collect::<Vec<_>>(),
        "count": messages.len()
    }))
}

/// Add to short-term memory.
///
/// POST /api/memory/short-term
pub async fn add_short_term_handler(
    State(state): State<ServerState>,
    Json(req): Json<AddShortTermMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mut mem = memory.write().await;
    mem.add_message(&req.role, &req.content)
        .map_err(|e| ErrorResponse::internal(format!("Failed to add message: {}", e)))?;

    let messages = mem.get_short_term();

    ok(json!({
        "message": "Added to short-term memory",
        "count": messages.len()
    }))
}

/// Clear short-term memory.
///
/// DELETE /api/memory/short-term
pub async fn clear_short_term_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mut mem = memory.write().await;
    mem.clear_short_term();

    ok(json!({
        "message": "Short-term memory cleared"
    }))
}

// ===== Mid-term memory endpoints =====

/// Get session history from mid-term memory.
///
/// GET /api/memory/mid-term/:session_id
pub async fn get_session_history_handler(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let entries = mem.get_session_history(&session_id).await;

    ok(json!({
        "session_id": session_id,
        "entries": entries.iter().map(ConversationEntryDto::from).collect::<Vec<_>>(),
        "count": entries.len()
    }))
}

/// Add to mid-term memory.
///
/// POST /api/memory/mid-term
pub async fn add_mid_term_handler(
    State(state): State<ServerState>,
    Json(req): Json<AddMidTermMemoryRequest>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    mem.add_conversation(&req.session_id, &req.user_input, &req.assistant_response)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add conversation: {}", e)))?;

    ok(json!({
        "message": "Added to mid-term memory",
        "session_id": req.session_id
    }))
}

/// Search mid-term memory.
///
/// GET /api/memory/mid-term/search?q=query&top_k=5
pub async fn search_mid_term_handler(
    State(state): State<ServerState>,
    Query(params): Query<MemoryQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let results = mem.search_mid_term(&params.q, params.top_k).await;

    ok(json!({
        "query": params.q,
        "results": results.iter().map(SearchResultDto::from).collect::<Vec<_>>(),
        "count": results.len()
    }))
}

/// Clear mid-term memory.
///
/// DELETE /api/memory/mid-term
pub async fn clear_mid_term_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    mem.clear_mid_term().await;

    ok(json!({
        "message": "Mid-term memory cleared"
    }))
}

// ===== Long-term memory endpoints =====

/// Search knowledge base.
///
/// GET /api/memory/long-term/search?q=query
pub async fn search_knowledge_handler(
    State(state): State<ServerState>,
    Query(params): Query<MemoryQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let results = mem.search_knowledge(&params.q).await;

    ok(json!({
        "query": params.q,
        "results": results.iter().map(KnowledgeEntryDto::from).collect::<Vec<_>>(),
        "count": results.len()
    }))
}

/// Get knowledge by category.
///
/// GET /api/memory/long-term/category/:category
pub async fn get_knowledge_by_category_handler(
    State(state): State<ServerState>,
    Path(category): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let cat = KnowledgeCategory::from_str(&category);
    let results = mem.get_knowledge_by_category(&cat).await;

    ok(json!({
        "category": category,
        "results": results.iter().map(KnowledgeEntryDto::from).collect::<Vec<_>>(),
        "count": results.len()
    }))
}

/// Get device knowledge.
///
/// GET /api/memory/long-term/device/:device_id
pub async fn get_device_knowledge_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let results = mem.get_device_knowledge(&device_id).await;

    ok(json!({
        "device_id": device_id,
        "results": results.iter().map(KnowledgeEntryDto::from).collect::<Vec<_>>(),
        "count": results.len()
    }))
}

/// Get popular knowledge.
///
/// GET /api/memory/long-term/popular?n=10
pub async fn get_popular_knowledge_handler(
    State(state): State<ServerState>,
    Query(params): Query<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let n = params.get("n").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let results = mem.get_popular_knowledge(n).await;

    ok(json!({
        "results": results.iter().map(KnowledgeEntryDto::from).collect::<Vec<_>>(),
        "count": results.len()
    }))
}

/// Add knowledge entry.
///
/// POST /api/memory/long-term
pub async fn add_knowledge_handler(
    State(state): State<ServerState>,
    Json(req): Json<AddKnowledgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    let category = KnowledgeCategory::from_str(&req.category);

    let mut entry = KnowledgeEntry::new(&req.title, &req.content, category);
    entry.tags = req.tags;
    entry.device_ids = req.device_ids;

    mem.add_knowledge(entry)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add knowledge: {}", e)))?;

    ok(json!({
        "message": "Knowledge added"
    }))
}

/// Clear long-term memory.
///
/// DELETE /api/memory/long-term
pub async fn clear_long_term_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let memory = get_global_memory(&state);
    let mem = memory.read().await;
    mem.clear_long_term().await;

    ok(json!({
        "message": "Long-term memory cleared"
    }))
}
