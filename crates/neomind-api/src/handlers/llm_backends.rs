//! LLM Backend Management API Handlers
//!
//! This module provides REST API endpoints for managing multiple LLM backend instances.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

use neomind_llm::instance_manager::{
    BackendTypeDefinition, LlmBackendInstanceManager, get_instance_manager,
};
use neomind_storage::{BackendCapabilities, LlmBackendInstance, LlmBackendStore, LlmBackendType};

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

    /// Top-K sampling (0 = disabled)
    #[serde(default = "default_top_k")]
    pub top_k: Option<usize>,

    /// Enable thinking/reasoning mode for models that support it
    #[serde(default = "default_thinking_enabled")]
    pub thinking_enabled: bool,

    /// Model capabilities (optional, from Ollama model detection)
    #[serde(default)]
    pub capabilities: Option<BackendCapabilities>,
}

fn default_temperature() -> f32 {
    0.6  // Lowered for faster responses
}
fn default_top_p() -> f32 {
    0.85  // Lowered to reduce thinking time
}
fn default_top_k() -> Option<usize> {
    Some(20)  // Lowered for faster sampling
}
fn default_max_tokens() -> usize {
    usize::MAX
}
fn default_thinking_enabled() -> bool {
    true
}

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

    /// Top-K sampling
    pub top_k: Option<usize>,

    /// Enable thinking/reasoning mode for models that support it
    pub thinking_enabled: Option<bool>,

    /// Model capabilities (optional, from Ollama model detection)
    #[serde(default)]
    pub capabilities: Option<BackendCapabilities>,
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
    pub top_k: usize,
    pub max_tokens: usize,
    pub thinking_enabled: bool,
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
            api_key_configured: instance.api_key.is_some()
                && !instance.api_key.as_ref().is_some_and(|k| k.is_empty()),
            is_active: instance.is_active,
            temperature: instance.temperature,
            top_p: instance.top_p,
            top_k: instance.top_k,
            max_tokens: instance.max_tokens,
            thinking_enabled: instance.thinking_enabled,
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
    /// Configuration schema for dynamic form generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
}

impl From<BackendTypeDefinition> for BackendTypeDto {
    fn from(def: BackendTypeDefinition) -> Self {
        Self {
            id: def.id.clone(),
            name: def.name,
            description: def.description,
            default_model: def.default_model,
            default_endpoint: def.default_endpoint,
            requires_api_key: def.requires_api_key,
            supports_streaming: def.supports_streaming,
            supports_thinking: def.supports_thinking,
            supports_multimodal: def.supports_multimodal,
            config_schema: None, // Will be populated by the handler
        }
    }
}

impl BackendTypeDto {
    /// Create from BackendTypeDefinition and include config schema
    pub fn with_schema(def: BackendTypeDefinition, schema: serde_json::Value) -> Self {
        let mut dto: Self = def.into();
        dto.config_schema = Some(schema);
        dto
    }
}

/// Get the instance manager (returns error instead of panic)
fn get_manager() -> Result<Arc<LlmBackendInstanceManager>, ErrorResponse> {
    get_instance_manager().map_err(|e| ErrorResponse::internal(e.to_string()))
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

    let instance = manager
        .get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    let mut dto: BackendInstanceDto = instance.clone().into();
    dto.healthy = manager.get_health_status(&id);
    dto.is_active = manager
        .get_active_instance()
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
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Unknown backend type: {}",
                req.backend_type
            )));
        }
    };

    // Generate unique ID
    let id = LlmBackendStore::generate_id(&req.backend_type);

    // Use provided capabilities or get defaults for the backend type
    let mut capabilities = req
        .capabilities
        .unwrap_or_else(|| get_default_capabilities(&backend_type));

    // Adjust capabilities based on actual model name
    adjust_capabilities_for_model(&req.model, &mut capabilities);

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
        max_tokens: default_max_tokens(),
        top_k: req.top_k.unwrap_or(20),  // Default to 20 for faster responses
        thinking_enabled: req.thinking_enabled,
        capabilities,
        updated_at: chrono::Utc::now().timestamp(),
    };

    // Validate
    instance
        .validate()
        .map_err(ErrorResponse::bad_request)?;

    // Save
    manager
        .upsert_instance(instance)
        .await
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
    let mut instance = manager
        .get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    // Update fields
    if let Some(name) = req.name {
        instance.name = name;
    }
    if let Some(endpoint) = req.endpoint {
        instance.endpoint = Some(endpoint);
    }
    if let Some(model) = req.model {
        instance.model = model.clone();
        // Re-detect capabilities when model changes
        adjust_capabilities_for_model(&model, &mut instance.capabilities);
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
    if let Some(thinking_enabled) = req.thinking_enabled {
        instance.thinking_enabled = thinking_enabled;
    }
    if let Some(mut capabilities) = req.capabilities {
        // Adjust capabilities based on model name
        adjust_capabilities_for_model(&instance.model, &mut capabilities);
        instance.capabilities = capabilities;
    }
    instance.updated_at = chrono::Utc::now().timestamp();

    // Validate
    instance
        .validate()
        .map_err(ErrorResponse::bad_request)?;

    // Save
    manager
        .upsert_instance(instance.clone())
        .await
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

    manager
        .remove_instance(&id)
        .await
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
    let instance = manager
        .get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    // Set active in the instance manager
    manager
        .set_active(&id)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Also update the SessionManager's LLM backend for existing sessions
    use neomind_agent::LlmBackend;
    let backend = match instance.backend_type {
        LlmBackendType::Ollama => {
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = instance.model.clone();
            LlmBackend::Ollama { endpoint, model }
        }
        LlmBackendType::OpenAi => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
            }
        }
        LlmBackendType::Anthropic => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
            }
        }
        LlmBackendType::Google => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
            }
        }
        LlmBackendType::XAi => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.x.ai/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
            }
        }
    };

    // Update all existing sessions to use the new backend
    state
        .session_manager
        .set_llm_backend(backend.clone())
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Also save to SettingsStore for persistence across server restarts
    // This ensures init_llm() will load the correct backend on startup
    let settings_request = match instance.backend_type {
        LlmBackendType::Ollama => crate::config::LlmSettingsRequest {
            backend: "ollama".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: None,
        },
        LlmBackendType::OpenAi => crate::config::LlmSettingsRequest {
            backend: "openai".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::Anthropic => crate::config::LlmSettingsRequest {
            backend: "anthropic".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::Google => crate::config::LlmSettingsRequest {
            backend: "google".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::XAi => crate::config::LlmSettingsRequest {
            backend: "xai".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
    };

    if let Err(e) = state.save_llm_config(&settings_request).await {
        tracing::warn!(category = "settings", error = %e, "Failed to save LLM config to SettingsStore after backend activation");
    }

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

    let result = manager
        .test_connection(&id)
        .await
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

    let dtos: Vec<BackendTypeDto> = types
        .into_iter()
        .map(|def| {
            let schema = manager.get_config_schema(&def.id);
            BackendTypeDto::with_schema(def, schema)
        })
        .collect();

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
    let stats = get_backend_stats()
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(stats)
}

/// Fetch available models from an Ollama server
///
/// GET /api/llm-backends/ollama/models?endpoint=http://localhost:11434
///
/// Returns the list of available models from the specified Ollama server,
/// along with their detected capabilities (multimodal, thinking, tools, etc.)
///
/// Uses /api/show endpoint to get accurate capabilities from Ollama's response.
/// The capabilities field contains "vision" for multimodal models.
pub async fn list_ollama_models_handler(
    Query(params): Query<OllamaModelsQuery>,
) -> HandlerResult<serde_json::Value> {
    use reqwest::Client;

    let endpoint = params.endpoint.unwrap_or_else(|| "http://localhost:11434".to_string());
    let base_url = endpoint.trim_end_matches('/');

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to create HTTP client: {}", e)))?;

    // First, get the list of models from /api/tags
    let tags_url = format!("{}/api/tags", base_url);
    let response = client
        .get(&tags_url)
        .send()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to connect to Ollama: {}", e)))?;

    if !response.status().is_success() {
        return Err(ErrorResponse::internal(format!(
            "Ollama returned status: {}",
            response.status()
        )));
    }

    let ollama_response: OllamaTagsResponse = response
        .json()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to parse Ollama response: {}", e)))?;

    // Enrich models with capabilities from /api/show
    let mut models_with_caps = Vec::new();
    for model in ollama_response.models {
        // Get detailed info including capabilities from /api/show
        let show_url = format!("{}/api/show", base_url);
        let caps = match get_model_capabilities_from_show(&client, &show_url, &model.name).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to get capabilities for {}: {}, using fallback detection", model.name, e);
                // Fallback to name-based detection if /api/show fails
                detect_ollama_model_capabilities_from_name(&model.name)
            }
        };

        models_with_caps.push(OllamaModelWithCapabilities {
            name: model.name,
            size: model.size,
            modified_at: model.modified_at,
            digest: model.digest,
            details: model.details,
            supports_multimodal: caps.supports_multimodal,
            supports_thinking: caps.supports_thinking,
            supports_tools: caps.supports_tools,
            max_context: caps.max_context,
        });
    }

    ok(json!({
        "models": models_with_caps,
        "count": models_with_caps.len(),
    }))
}

/// Get model capabilities from Ollama /api/show endpoint
///
/// The /api/show endpoint returns accurate capabilities including "vision" for multimodal models.
/// Only thinking capability needs to be inferred from model name since Ollama doesn't provide it.
async fn get_model_capabilities_from_show(
    client: &reqwest::Client,
    show_url: &str,
    model_name: &str,
) -> Result<BackendCapabilities, String> {
    use serde_json::Value;

    let response = client
        .post(show_url)
        .json(&serde_json::json!({ "model": model_name }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP status: {}", response.status()));
    }

    let show_response: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Extract capabilities from the response
    // Ollama returns capabilities as an array like ["completion", "vision", "tools"]
    let supports_multimodal = show_response["capabilities"]
        .as_array()
        .map(|caps| caps.iter().any(|c| c.as_str() == Some("vision")))
        .unwrap_or(false);

    // Extract details for context size detection
    let _details = show_response["details"].as_object();

    // Thinking capability is NOT provided by Ollama's API, need to infer from model name
    let supports_thinking = detect_thinking_from_name(model_name);

    // Tools support - most models support tools except very small ones
    let name_lower = model_name.to_lowercase();
    let supports_tools = !name_lower.contains("270m")
        && !name_lower.contains("1b")
        && !name_lower.contains("tiny")
        && !name_lower.contains("micro")
        && !name_lower.contains("nano");

    // Detect max context from model info or details
    let max_context = if let Some(model_info) = show_response["model_info"].as_object() {
        // Try to get context length from model_info
        model_info
            .iter()
            .find_map(|(k, v)| {
                if k.contains("context") || k.contains("context_length") {
                    v.as_u64().map(|u| u as usize)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| detect_ollama_model_context(model_name))
    } else {
        detect_ollama_model_context(model_name)
    };

    Ok(BackendCapabilities {
        supports_streaming: true,
        supports_multimodal,
        supports_thinking,
        supports_tools,
        max_context,
    })
}

/// Detect thinking capability from model name only
///
/// Ollama's API doesn't provide thinking capability, so we infer it from model naming patterns.
fn detect_thinking_from_name(model_name: &str) -> bool {
    let name_lower = model_name.to_lowercase();

    // Vision models typically don't support extended thinking
    if name_lower.contains("-vl") || name_lower.ends_with("vl") {
        return false;
    }

    name_lower.starts_with("qwen3")
        || name_lower.contains("qwen3-")
        || name_lower.contains("gpt-oss")
        || name_lower.contains("deepseek-r1")
        || name_lower.contains("deepseek-r")
        || name_lower.contains("deepseek v3.1")
        || name_lower.contains("deepseek-v3.1")
        || name_lower.contains("thinking")
}

/// Fallback: Detect capabilities from model name when /api/show is not available
fn detect_ollama_model_capabilities_from_name(model_name: &str) -> BackendCapabilities {
    let name_lower = model_name.to_lowercase();

    // Vision capability from name patterns (fallback only)
    let supports_multimodal = name_lower.contains("-vl")
        || name_lower.ends_with("vl")
        || name_lower.contains("vision")
        || name_lower.contains("llava")
        || name_lower.contains("bakllava")
        || name_lower.contains("minicpm-v");

    let supports_thinking = detect_thinking_from_name(model_name);

    let supports_tools = !name_lower.contains("270m")
        && !name_lower.contains("1b")
        && !name_lower.contains("tiny")
        && !name_lower.contains("micro")
        && !name_lower.contains("nano");

    let max_context = detect_ollama_model_context(model_name);

    BackendCapabilities {
        supports_streaming: true,
        supports_multimodal,
        supports_thinking,
        supports_tools,
        max_context,
    }
}

/// Query parameters for fetching Ollama models
#[derive(Debug, Deserialize)]
pub struct OllamaModelsQuery {
    /// Ollama server endpoint (default: http://localhost:11434)
    pub endpoint: Option<String>,
}

/// Ollama /api/tags response structure
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    modified_at: Option<String>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaModelDetails {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    families: Option<Vec<String>>,
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}

/// Ollama model with detected capabilities
#[derive(Debug, Serialize)]
struct OllamaModelWithCapabilities {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<OllamaModelDetails>,
    supports_multimodal: bool,
    supports_thinking: bool,
    supports_tools: bool,
    max_context: usize,
}

/// Detect maximum context window size for a model (works across all backends)
fn detect_model_context(model_name: &str) -> usize {
    let name_lower = model_name.to_lowercase();

    // OpenAI models
    if name_lower.contains("gpt-4") {
        if name_lower.contains("gpt-4-turbo") || name_lower.contains("gpt-4o") {
            return 128000;
        }
        if name_lower.contains("32k") {
            return 32000;
        }
        return 8192;
    }

    if name_lower.contains("gpt-3.5") {
        if name_lower.contains("16k") {
            return 16000;
        }
        return 4096;
    }

    // Anthropic models
    if name_lower.contains("claude-3") {
        if name_lower.contains("sonnet") {
            return 200000;
        }
        if name_lower.contains("opus") {
            return 200000;
        }
        if name_lower.contains("haiku") {
            return 200000;
        }
    }

    if name_lower.contains("claude") {
        return 100000;
    }

    // Google models
    if name_lower.contains("gemini-1.5") {
        return 1000000;
    }

    if name_lower.contains("gemini") {
        return 1000000;
    }

    // xAI models
    if name_lower.contains("grok") {
        return 128000;
    }

    // Fallback to Ollama model detection for local models
    detect_ollama_model_context(model_name)
}

/// Detect maximum context window size for an Ollama model
fn detect_ollama_model_context(model_name: &str) -> usize {
    let name_lower = model_name.to_lowercase();

    // Qwen family (qwen, qwen2, qwen2.5, qwen3, qwen3-vl)
    if name_lower.starts_with("qwen") {
        // Qwen3 and Qwen3-VL support 32k context
        // Qwen2.5 supports 128k for some variants
        if name_lower.contains("qwen2") && name_lower.contains("128") {
            return 128000;
        } else if name_lower.contains("qwen2") {
            return 32000;
        } else {
            return 32000;
        }
    }

    // Llama 3.x family
    if name_lower.contains("llama3") || name_lower.contains("llama-3") {
        return 8000;
    }

    // DeepSeek R1
    if name_lower.contains("deepseek-r1") || name_lower.contains("deepseek-r ") {
        return 64000;
    }

    // Mistral family
    if name_lower.contains("mistral") {
        return 32000;
    }

    // Gemma family
    if name_lower.contains("gemma") {
        return 8000;
    }

    // Phi family
    if name_lower.contains("phi-3") {
        return 32000;
    }

    // Default fallback
    8192
}

/// Get default capabilities for a backend type
fn get_default_capabilities(backend_type: &LlmBackendType) -> BackendCapabilities {
    match backend_type {
        LlmBackendType::Ollama => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: true,
            supports_tools: true,
            max_context: 8192,
        },
        LlmBackendType::OpenAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 128000,
        },
        LlmBackendType::Anthropic => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 200000,
        },
        LlmBackendType::Google => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 1000000,
        },
        LlmBackendType::XAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: false,
            supports_tools: false,
            max_context: 128000,
        },
    }
}

/// Adjust capabilities based on the actual model name.
/// This refines the default backend-type capabilities with model-specific knowledge.
fn adjust_capabilities_for_model(model_name: &str, capabilities: &mut BackendCapabilities) {
    let name_lower = model_name.to_lowercase();

    // === Multimodal (vision) detection ===
    // Models with "vl", "vision", or "-mm" suffix support vision
    let explicit_vision = name_lower.contains("-vl")
        || name_lower.contains(":vl")
        || name_lower.ends_with("vl")
        || name_lower.contains("vision")
        || name_lower.contains("-mm")
        || name_lower.contains(":mm");

    // Vision models typically don't support extended thinking (they're optimized for speed)
    if explicit_vision {
        capabilities.supports_multimodal = true;
        capabilities.supports_thinking = false;
    } else {
        // For non-vision models, check if they might support vision
        // Most models don't, so default to false unless explicitly marked
        // Exception: some newer models might have vision but no explicit marker
        // For now, be conservative and assume no vision unless marked
        capabilities.supports_multimodal = false;

        // === Thinking detection ===
        // Models that support extended thinking (deepseek-r1, qwen thinking models)
        capabilities.supports_thinking = name_lower.starts_with("qwen3")
            && !name_lower.contains("-vl") && !name_lower.contains(":vl")
            || name_lower.starts_with("qwen2.5")
            && !name_lower.contains("-vl") && !name_lower.contains(":vl")
            || name_lower.contains("deepseek-r1")
            || name_lower.contains("thinking");
    }

    // === Tool support ===
    // Very small models (< 1B params) typically don't support tool calling
    if name_lower.contains(":0.5") || name_lower.contains(":0.5b") {
        capabilities.supports_tools = false;
    }

    // === Context size adjustment ===
    capabilities.max_context = detect_model_context(model_name);
}
