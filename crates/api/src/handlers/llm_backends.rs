//! LLM Backend Management API Handlers
//!
//! This module provides REST API endpoints for managing multiple LLM backend instances.

use axum::{extract::{Path, Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;

use edge_ai_llm::instance_manager::{
    LlmBackendInstanceManager, BackendTypeDefinition, get_instance_manager,
};
use edge_ai_storage::{
    LlmBackendInstance, BackendCapabilities, LlmBackendStore,
    LlmBackendType,
};

/// Query parameters for listing LLM backends
#[derive(Debug, Deserialize)]
pub struct ListBackendsQuery {
    /// Filter by backend type
    pub r#type: Option<String>,
    /// Show only active backend
    pub active_only: Option<bool>,
}

/// Request to create/update an LLM backend instance
#[derive(Debug, Deserialize)]
pub struct CreateBackendRequest {
    /// Display name
    pub name: String,

    /// Backend type
    pub backend_type: String,

    /// API endpoint URL
    pub endpoint: Option<String>,

    /// Model name
    pub model: String,

    /// API key (for cloud providers)
    pub api_key: Option<String>,

    /// Temperature (0.0 to 2.0)
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Top-P sampling (0.0 to 1.0)
    #[serde(default = "default_top_p")]
    pub top_p: f32,

    /// Maximum tokens to generate
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

fn default_temperature() -> f32 { 0.7 }
fn default_top_p() -> f32 { 0.9 }
fn default_max_tokens() -> usize { usize::MAX }

/// Request to update an LLM backend instance
#[derive(Debug, Deserialize)]
pub struct UpdateBackendRequest {
    /// Display name
    pub name: Option<String>,

    /// API endpoint URL
    pub endpoint: Option<String>,

    /// Model name
    pub model: Option<String>,

    /// API key
    pub api_key: Option<String>,

    /// Temperature
    pub temperature: Option<f32>,

    /// Top-P sampling
    pub top_p: Option<f32>,

    /// Maximum tokens
    pub max_tokens: Option<usize>,
}

/// Backend instance DTO for API responses
#[derive(Debug, Serialize)]
pub struct BackendInstanceDto {
    pub id: String,
    pub name: String,
    pub backend_type: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key_configured: bool,
    pub is_active: bool,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: usize,
    pub capabilities: BackendCapabilities,
    pub updated_at: i64,
    pub healthy: Option<bool>,
}

impl From<LlmBackendInstance> for BackendInstanceDto {
    fn from(instance: LlmBackendInstance) -> Self {
        let backend_type = instance.backend_name().to_string();
        Self {
            id: instance.id,
            name: instance.name,
            backend_type,
            endpoint: instance.endpoint,
            model: instance.model,
            api_key_configured: instance.api_key.is_some() && !instance.api_key.as_ref().map_or(false, |k| k.is_empty()),
            is_active: instance.is_active,
            temperature: instance.temperature,
            top_p: instance.top_p,
            max_tokens: instance.max_tokens,
            capabilities: instance.capabilities,
            updated_at: instance.updated_at,
            healthy: None, // Populated separately
        }
    }
}

/// Backend type definition DTO
#[derive(Debug, Serialize)]
pub struct BackendTypeDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_model: String,
    pub default_endpoint: Option<String>,
    pub requires_api_key: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub supports_multimodal: bool,
}

impl From<BackendTypeDefinition> for BackendTypeDto {
    fn from(def: BackendTypeDefinition) -> Self {
        Self {
            id: def.id,
            name: def.name,
            description: def.description,
            default_model: def.default_model,
            default_endpoint: def.default_endpoint,
            requires_api_key: def.requires_api_key,
            supports_streaming: def.supports_streaming,
            supports_thinking: def.supports_thinking,
            supports_multimodal: def.supports_multimodal,
        }
    }
}

/// Get the instance manager (returns error instead of panic)
fn get_manager() -> Result<Arc<LlmBackendInstanceManager>, ErrorResponse> {
    get_instance_manager()
        .map_err(|e| ErrorResponse::internal(e.to_string()))
}

/// Get backend statistics through the manager
async fn get_backend_stats() -> Result<serde_json::Value, ErrorResponse> {
    let manager = get_manager()?;
    // Get stats from the instances
    let instances = manager.list_instances();
    let active_id = manager.get_active_instance().map(|i| i.id);

    let stats = serde_json::json!({
        "total": instances.len(),
        "active_id": active_id,
        "by_type": {
            "ollama": instances.iter().filter(|i| i.backend_name() == "ollama").count(),
            "openai": instances.iter().filter(|i| i.backend_name() == "openai").count(),
            "anthropic": instances.iter().filter(|i| i.backend_name() == "anthropic").count(),
            "google": instances.iter().filter(|i| i.backend_name() == "google").count(),
            "xai": instances.iter().filter(|i| i.backend_name() == "xai").count(),
        }
    });

    Ok(stats)
}

/// List all LLM backend instances
///
/// GET /api/llm-backends
///
/// Query parameters:
/// - type: Filter by backend type (e.g., "ollama", "openai")
/// - active_only: Show only the active backend
pub async fn list_backends_handler(
    State(_state): State<ServerState>,
    Query(query): Query<ListBackendsQuery>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;
    let mut instances = manager.list_instances();

    // Apply filters
    if let Some(type_filter) = query.r#type {
        instances.retain(|inst| inst.backend_name() == type_filter);
    }

    if query.active_only.unwrap_or(false) {
        instances.retain(|inst| inst.is_active);
    }

    // Convert to DTOs
    let mut dtos: Vec<BackendInstanceDto> = instances
        .into_iter()
        .map(|inst| {
            let mut dto: BackendInstanceDto = inst.clone().into();
            // Add health status if available
            dto.healthy = manager.get_health_status(&inst.id);
            dto
        })
        .collect();

    // Mark active backend
    if let Some(active_id) = manager.get_active_instance().map(|inst| inst.id) {
        for dto in &mut dtos {
            dto.is_active = dto.id == active_id;
        }
    }

    let active_id = manager.get_active_instance().map(|i| i.id);

    ok(json!({
        "backends": dtos,
        "count": dtos.len(),
        "active_id": active_id,
    }))
}

/// Get a specific LLM backend instance
///
/// GET /api/llm-backends/:id
pub async fn get_backend_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    let instance = manager.get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    let mut dto: BackendInstanceDto = instance.clone().into();
    dto.healthy = manager.get_health_status(&id);
    dto.is_active = manager.get_active_instance()
        .map(|a| a.id == id)
        .unwrap_or(false);

    ok(json!({
        "backend": dto,
    }))
}

/// Create a new LLM backend instance
///
/// POST /api/llm-backends
pub async fn create_backend_handler(
    State(_state): State<ServerState>,
    Json(req): Json<CreateBackendRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    // Parse backend type
    let backend_type = match req.backend_type.as_str() {
        "ollama" => LlmBackendType::Ollama,
        "openai" => LlmBackendType::OpenAi,
        "anthropic" => LlmBackendType::Anthropic,
        "google" => LlmBackendType::Google,
        "xai" => LlmBackendType::XAi,
        _ => return Err(ErrorResponse::bad_request(format!("Unknown backend type: {}", req.backend_type))),
    };

    // Generate unique ID
    let id = LlmBackendStore::generate_id(&req.backend_type);

    // Get default capabilities for the backend type
    let capabilities = get_default_capabilities(&backend_type);

    let instance = LlmBackendInstance {
        id: id.clone(),
        name: req.name.clone(),
        backend_type,
        endpoint: req.endpoint,
        model: req.model,
        api_key: req.api_key,
        is_active: false,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        capabilities,
        updated_at: chrono::Utc::now().timestamp(),
    };

    // Validate
    instance.validate()
        .map_err(|e| ErrorResponse::bad_request(e))?;

    // Save
    manager.upsert_instance(instance).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": id,
        "message": "Backend instance created successfully",
    }))
}

/// Update an LLM backend instance
///
/// PUT /api/llm-backends/:id
pub async fn update_backend_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateBackendRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    // Get existing instance
    let mut instance = manager.get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    // Update fields
    if let Some(name) = req.name {
        instance.name = name;
    }
    if let Some(endpoint) = req.endpoint {
        instance.endpoint = Some(endpoint);
    }
    if let Some(model) = req.model {
        instance.model = model;
    }
    if let Some(api_key) = req.api_key {
        instance.api_key = Some(api_key);
    }
    if let Some(temperature) = req.temperature {
        instance.temperature = temperature;
    }
    if let Some(top_p) = req.top_p {
        instance.top_p = top_p;
    }
    if let Some(max_tokens) = req.max_tokens {
        instance.max_tokens = max_tokens;
    }
    instance.updated_at = chrono::Utc::now().timestamp();

    // Validate
    instance.validate()
        .map_err(|e| ErrorResponse::bad_request(e))?;

    // Save
    manager.upsert_instance(instance.clone()).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Clear cache
    manager.clear_cache();

    ok(json!({
        "id": id,
        "message": "Backend instance updated successfully",
    }))
}

/// Delete an LLM backend instance
///
/// DELETE /api/llm-backends/:id
pub async fn delete_backend_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    manager.remove_instance(&id).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "message": format!("Backend instance {} deleted", id),
    }))
}

/// Set a backend as active
///
/// POST /api/llm-backends/:id/activate
pub async fn activate_backend_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    // Get the backend instance to extract its configuration
    let instance = manager.get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    // Set active in the instance manager
    manager.set_active(&id).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Also update the SessionManager's LLM backend for existing sessions
    use edge_ai_agent::LlmBackend;
    let backend = match instance.backend_type {
        LlmBackendType::Ollama => {
            let endpoint = instance.endpoint.clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = instance.model.clone();
            LlmBackend::Ollama { endpoint, model }
        }
        LlmBackendType::OpenAi => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance.endpoint.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
        LlmBackendType::Anthropic => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance.endpoint.clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
        LlmBackendType::Google => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance.endpoint.clone()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
        LlmBackendType::XAi => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance.endpoint.clone()
                .unwrap_or_else(|| "https://api.x.ai/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
    };

    // Update all existing sessions to use the new backend
    state.session_manager.set_llm_backend(backend).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": id,
        "message": "Backend activated successfully",
    }))
}

/// Test connection to a backend
///
/// POST /api/llm-backends/:id/test
pub async fn test_backend_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    let result = manager.test_connection(&id).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "backend_id": id,
        "result": result,
    }))
}

/// Get available backend types
///
/// GET /api/llm-backends/types
pub async fn list_backend_types_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;
    let types = manager.get_available_types();

    let dtos: Vec<BackendTypeDto> = types.into_iter().map(BackendTypeDto::from).collect();

    ok(json!({
        "types": dtos,
    }))
}

/// Get configuration schema for a backend type
///
/// GET /api/llm-backends/types/:type/schema
pub async fn get_backend_schema_handler(
    State(_state): State<ServerState>,
    Path(backend_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;
    let schema = manager.get_config_schema(&backend_type);

    ok(json!({
        "backend_type": backend_type,
        "schema": schema,
    }))
}

/// Get backend statistics
///
/// GET /api/llm-backends/stats
pub async fn get_backend_stats_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let stats = get_backend_stats().await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(stats)
}

/// Get default capabilities for a backend type
fn get_default_capabilities(backend_type: &LlmBackendType) -> BackendCapabilities {
    match backend_type {
        LlmBackendType::Ollama => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: true,
            max_context: 8192,
        },
        LlmBackendType::OpenAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            max_context: 128000,
        },
        LlmBackendType::Anthropic => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            max_context: 200000,
        },
        LlmBackendType::Google => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            max_context: 1000000,
        },
        LlmBackendType::XAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: false,
            max_context: 128000,
        },
    }
}
