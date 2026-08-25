//! Skill System API handlers
//!
//! Provides endpoints for managing and testing scenario-driven skills.
//! Builtin skills are read-only; user skills support full CRUD.
//! User skills are persisted to `data/skills/{id}.md`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use neomind_agent::skills::types::SkillOrigin;
use neomind_agent::skills::{match_skills, Skill, SkillCategory, SkillRegistry, TokenBudgetConfig};

use super::ServerState;

/// Query parameters for skill list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    /// Filter by origin: "builtin", "user", or empty/absent for all.
    pub origin: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    20
}

/// Validate a skill ID contains only safe characters (prevents path traversal).
fn is_safe_skill_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Helper: get the skills directory, creating it if needed.
fn skills_dir(data_dir: &std::path::Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("skills");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create skills dir: {}", e))?;
    Ok(dir)
}

/// Helper: persist a skill to disk. Returns error on failure.
fn persist_skill(data_dir: &std::path::Path, id: &str, content: &str) -> Result<(), String> {
    if !is_safe_skill_id(id) {
        return Err(format!(
            "Invalid skill ID '{}': must contain only alphanumeric, hyphens, underscores",
            id
        ));
    }
    let dir = skills_dir(data_dir)?;
    let path = dir.join(format!("{}.md", id));
    std::fs::write(&path, content).map_err(|e| format!("Failed to persist skill '{}': {}", id, e))
}

/// Helper: delete a skill file from disk.
fn delete_skill_file(data_dir: &std::path::Path, id: &str) -> Result<(), String> {
    if !is_safe_skill_id(id) {
        return Err(format!("Invalid skill ID '{}'", id));
    }
    let dir = skills_dir(data_dir)?;
    let path = dir.join(format!("{}.md", id));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete skill file '{}': {}", id, e))?;
    }
    Ok(())
}

// ============================================================================
// Response Types
// ============================================================================

/// Summary of a skill for list views.
#[derive(Debug, Serialize)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub origin: String,
    pub priority: u32,
    pub token_budget: usize,
    pub keywords: Vec<String>,
    pub body_length: usize,
}

impl From<&Skill> for SkillSummary {
    fn from(skill: &Skill) -> Self {
        Self {
            id: skill.metadata.id.clone(),
            name: skill.metadata.name.clone(),
            category: match skill.metadata.category {
                SkillCategory::Device => "device",
                SkillCategory::Rule => "rule",
                SkillCategory::Agent => "agent",
                SkillCategory::Message => "message",
                SkillCategory::Extension => "extension",
                SkillCategory::General => "general",
            }
            .to_string(),
            origin: skill.metadata.origin.as_str().to_string(),
            priority: skill.metadata.priority,
            token_budget: skill.metadata.token_budget,
            keywords: skill.metadata.triggers.keywords.clone(),
            body_length: skill.body.len(),
        }
    }
}

/// Full skill detail including body.
#[derive(Debug, Serialize)]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub category: String,
    pub origin: String,
    pub priority: u32,
    pub token_budget: usize,
    pub keywords: Vec<String>,
    pub tool_targets: Vec<ToolTargetInfo>,
    pub anti_trigger_keywords: Vec<String>,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct ToolTargetInfo {
    pub tool: String,
    pub actions: Vec<String>,
}

impl From<&Skill> for SkillDetail {
    fn from(skill: &Skill) -> Self {
        Self {
            id: skill.metadata.id.clone(),
            name: skill.metadata.name.clone(),
            category: match skill.metadata.category {
                SkillCategory::Device => "device",
                SkillCategory::Rule => "rule",
                SkillCategory::Agent => "agent",
                SkillCategory::Message => "message",
                SkillCategory::Extension => "extension",
                SkillCategory::General => "general",
            }
            .to_string(),
            origin: skill.metadata.origin.as_str().to_string(),
            priority: skill.metadata.priority,
            token_budget: skill.metadata.token_budget,
            keywords: skill.metadata.triggers.keywords.clone(),
            tool_targets: skill
                .metadata
                .triggers
                .tool_target
                .iter()
                .map(|t| ToolTargetInfo {
                    tool: t.tool.clone(),
                    actions: t.actions.clone(),
                })
                .collect(),
            anti_trigger_keywords: skill.metadata.anti_triggers.keywords.clone(),
            body: skill.body.clone(),
        }
    }
}

/// Response for skill list endpoint.
#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    pub skills: Vec<SkillSummary>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
}

/// Request to create or update a skill.
#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    /// Full skill file content (YAML frontmatter + Markdown body).
    pub content: String,
}

/// Request to test skill matching.
#[derive(Debug, Deserialize)]
pub struct MatchTestRequest {
    pub query: String,
    pub context_size: Option<usize>,
}

/// Response for skill match test.
#[derive(Debug, Serialize)]
pub struct MatchTestResponse {
    pub query: String,
    pub matches: Vec<MatchResult>,
}

#[derive(Debug, Serialize)]
pub struct MatchResult {
    pub skill_id: String,
    pub skill_name: String,
    pub score: f32,
    pub body_preview: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// List all skills with pagination and optional origin filter.
pub async fn list_skills_handler(
    State(state): State<ServerState>,
    Query(params): Query<SkillListQuery>,
) -> Response {
    let page = params.page.max(1) as usize;
    let page_size = params.page_size.clamp(1, 100) as usize;

    let registry = state.agents.session_manager.skill_registry();
    let guard = registry.read().await;

    let mut all: Vec<SkillSummary> = guard
        .list()
        .iter()
        .map(|s| SkillSummary::from(*s))
        .collect();

    // Filter by origin if specified
    if let Some(ref origin) = params.origin {
        if !origin.is_empty() {
            all.retain(|s| s.origin == *origin);
        }
    }

    let total = all.len();
    let total_pages = total.div_ceil(page_size);

    let offset = (page - 1) * page_size;
    let skills: Vec<SkillSummary> = all.into_iter().skip(offset).take(page_size).collect();

    (
        StatusCode::OK,
        Json(SkillListResponse {
            skills,
            total,
            page,
            page_size,
            total_pages,
        }),
    )
        .into_response()
}

/// Get a single skill by ID.
pub async fn get_skill_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    let registry = state.agents.session_manager.skill_registry();
    let guard = registry.read().await;

    match guard.get(&id) {
        Some(skill) => (StatusCode::OK, Json(SkillDetail::from(skill))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Skill '{}' not found", id) })),
        )
            .into_response(),
    }
}

/// Create a new user skill.
pub async fn create_skill_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateSkillRequest>,
) -> Response {
    let registry = state.agents.session_manager.skill_registry();
    let mut guard = registry.write().await;

    match guard.add_user_skill(&req.content) {
        Ok(id) => {
            if let Err(e) = persist_skill(&state.data_dir, &id, &req.content) {
                tracing::error!(error = %e, "Skill created in memory but not persisted to disk");
            }
            match guard.get(&id) {
                Some(skill) => {
                    (StatusCode::CREATED, Json(SkillDetail::from(skill))).into_response()
                }
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Failed to retrieve created skill" })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// Update a user skill.
pub async fn update_skill_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<CreateSkillRequest>,
) -> Response {
    let registry = state.agents.session_manager.skill_registry();
    let mut guard = registry.write().await;

    // Builtin skills are read-only — a shadow overwrite here would persist
    // forever and mask future builtin content shipped with NeoMind upgrades.
    if let Some(skill) = guard.get(&id) {
        if skill.metadata.origin == SkillOrigin::Builtin {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": format!("Skill '{}' is builtin and read-only", id) })),
            )
                .into_response();
        }
    }

    match guard.update_user_skill(&id, &req.content) {
        Ok(()) => {
            if let Err(e) = persist_skill(&state.data_dir, &id, &req.content) {
                tracing::error!(error = %e, "Skill updated in memory but not persisted to disk");
            }
            match guard.get(&id) {
                Some(skill) => (StatusCode::OK, Json(SkillDetail::from(skill))).into_response(),
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Failed to retrieve updated skill" })),
                )
                    .into_response(),
            }
        }
        Err(e) => {
            let status = if e.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(serde_json::json!({ "error": e }))).into_response()
        }
    }
}

/// Delete a user skill.
pub async fn delete_skill_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    let registry = state.agents.session_manager.skill_registry();
    let mut guard = registry.write().await;

    // Builtin skills are read-only (see update handler).
    if let Some(skill) = guard.get(&id) {
        if skill.metadata.origin == SkillOrigin::Builtin {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": format!("Skill '{}' is builtin and read-only", id) })),
            )
                .into_response();
        }
    }

    match guard.delete_skill(&id) {
        Ok(_) => {
            if let Err(e) = delete_skill_file(&state.data_dir, &id) {
                tracing::error!(error = %e, "Skill removed from memory but file not deleted");
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": format!("Skill '{}' deleted", id) })),
            )
                .into_response()
        }
        Err(e) => {
            let status = if e.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::FORBIDDEN
            };
            (status, Json(serde_json::json!({ "error": e }))).into_response()
        }
    }
}

/// Reload all skills from disk.
pub async fn reload_skills_handler(State(state): State<ServerState>) -> Response {
    let new_registry = SkillRegistry::load_all(Some(&state.data_dir));

    let registry = state.agents.session_manager.skill_registry();
    let mut guard = registry.write().await;
    *guard = new_registry;

    let total = guard.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Skills reloaded",
            "total": total,
        })),
    )
        .into_response()
}

/// Test skill matching against a query.
pub async fn match_skills_handler(
    State(state): State<ServerState>,
    Json(req): Json<MatchTestRequest>,
) -> Response {
    let registry = state.agents.session_manager.skill_registry();
    let guard = registry.read().await;

    let context_size = req.context_size.unwrap_or(8000);
    let budget = TokenBudgetConfig::for_context(context_size);
    let matches = match_skills(&guard, &req.query, budget);

    let results: Vec<MatchResult> = matches
        .iter()
        .map(|m| MatchResult {
            skill_id: m.skill_id.clone(),
            skill_name: m.skill_name.clone(),
            score: m.score,
            body_preview: m.body.chars().take(200).collect(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(MatchTestResponse {
            query: req.query,
            matches: results,
        }),
    )
        .into_response()
}
