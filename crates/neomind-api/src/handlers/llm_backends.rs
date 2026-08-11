//! LLM Backend Management API Handlers
//!
//! This module provides REST API endpoints for managing multiple LLM backend instances.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

use neomind_agent::llm_backends::{
    get_instance_manager, BackendTypeDefinition, LlmBackendInstanceManager,
};
use neomind_core::llm::detect_vision_capability;
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

    /// Unified thinking/reasoning effort (preferred over `thinking_enabled`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<neomind_core::ThinkingEffort>,

    /// Model capabilities (optional, from Ollama model detection)
    #[serde(default)]
    pub capabilities: Option<BackendCapabilities>,
}

fn default_temperature() -> f32 {
    0.6 // Lowered for faster responses
}
fn default_top_p() -> f32 {
    0.85 // Lowered to reduce thinking time
}
fn default_top_k() -> Option<usize> {
    Some(20) // Lowered for faster sampling
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

    /// Unified thinking/reasoning effort (preferred over `thinking_enabled`).
    pub thinking_effort: Option<neomind_core::ThinkingEffort>,

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
    pub thinking_effort: Option<neomind_core::ThinkingEffort>,
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
            thinking_effort: instance.thinking_effort,
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
        "llamacpp" => LlmBackendType::LlamaCpp,
        "openai" => LlmBackendType::OpenAi,
        "anthropic" => LlmBackendType::Anthropic,
        "google" => LlmBackendType::Google,
        "xai" => LlmBackendType::XAi,
        "qwen" => LlmBackendType::Qwen,
        "deepseek" => LlmBackendType::DeepSeek,
        "glm" => LlmBackendType::GLM,
        "minimax" => LlmBackendType::MiniMax,
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Unknown backend type: {}",
                req.backend_type
            )));
        }
    };

    // Generate unique ID
    let id = LlmBackendStore::generate_id(&req.backend_type);

    // Get capabilities: prefer API detection for Ollama, fallback to name-based
    let capabilities = if matches!(backend_type, LlmBackendType::Ollama) {
        // For Ollama, try to get capabilities from /api/show endpoint
        let endpoint = req.endpoint.as_deref().unwrap_or("http://localhost:11434");
        let show_url = format!("{}/api/show", endpoint);
        match get_model_capabilities_from_show(&reqwest::Client::new(), &show_url, &req.model).await
        {
            Ok(caps) => {
                tracing::info!(
                    model = %req.model,
                    multimodal = caps.supports_multimodal,
                    thinking = caps.supports_thinking,
                    tools = caps.supports_tools,
                    "Detected Ollama model capabilities via API"
                );
                caps
            }
            Err(e) => {
                tracing::warn!(
                    model = %req.model,
                    error = %e,
                    "Failed to get Ollama capabilities via API, using name-based detection"
                );
                // Fallback to name-based detection
                let mut caps = req
                    .capabilities
                    .unwrap_or_else(|| get_default_capabilities(&backend_type));
                adjust_capabilities_for_model(&req.model, &mut caps);
                caps
            }
        }
    } else {
        // For non-Ollama backends, use provided capabilities or defaults with name-based adjustment
        let mut caps = req
            .capabilities
            .unwrap_or_else(|| get_default_capabilities(&backend_type));
        adjust_capabilities_for_model(&req.model, &mut caps);
        caps
    };

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
        top_k: req.top_k.unwrap_or(20), // Default to 20 for faster responses
        thinking_enabled: req.thinking_enabled,
        thinking_effort: req.thinking_effort,
        capabilities,
        updated_at: chrono::Utc::now().timestamp(),
    };

    // Validate
    instance.validate().map_err(ErrorResponse::bad_request)?;

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
        // For Ollama, try API detection first
        if matches!(instance.backend_type, LlmBackendType::Ollama) {
            let endpoint = instance
                .endpoint
                .as_deref()
                .unwrap_or("http://localhost:11434");
            let show_url = format!("{}/api/show", endpoint);
            match get_model_capabilities_from_show(&reqwest::Client::new(), &show_url, &model).await
            {
                Ok(caps) => {
                    tracing::info!(
                        model = %model,
                        multimodal = caps.supports_multimodal,
                        thinking = caps.supports_thinking,
                        tools = caps.supports_tools,
                        "Updated Ollama model capabilities via API"
                    );
                    instance.capabilities = caps;
                }
                Err(e) => {
                    tracing::warn!(
                        model = %model,
                        error = %e,
                        "Failed to get Ollama capabilities, using name-based detection"
                    );
                    adjust_capabilities_for_model(&model, &mut instance.capabilities);
                }
            }
        } else {
            adjust_capabilities_for_model(&model, &mut instance.capabilities);
        }
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
        let prev = instance.thinking_enabled;
        instance.thinking_enabled = thinking_enabled;
        // Dedicated log so users can verify the toggle took effect.
        // The "Updated Ollama model capabilities" log above reports
        // caps.supports_thinking (model capability from /api/show), NOT
        // the user's enable/disable choice — these are different concepts
        // and conflating them causes "I turned it off but logs say true"
        // confusion.
        tracing::info!(
            backend_id = %id,
            model = %instance.model,
            prev_thinking_enabled = prev,
            new_thinking_enabled = thinking_enabled,
            "User thinking_enabled setting updated"
        );
    }
    if let Some(thinking_effort) = req.thinking_effort {
        let prev = instance.thinking_effort;
        instance.thinking_effort = Some(thinking_effort);
        tracing::info!(
            backend_id = %id,
            model = %instance.model,
            prev_thinking_effort = ?prev,
            new_thinking_effort = ?thinking_effort,
            "User thinking_effort setting updated"
        );
    }
    if let Some(mut capabilities) = req.capabilities {
        // Preserve fields that the frontend doesn't render.
        // The capabilities object sent by the frontend typically doesn't include
        // `multimodal_user_override` or `multimodal_source` (those are managed via
        // the dedicated PATCH /capabilities endpoint). Without this preservation,
        // any user override or runtime_api detection would be silently wiped every
        // time the user saved an unrelated field change like temperature.
        let saved_user_override = instance.capabilities.multimodal_user_override;
        let saved_source = instance.capabilities.multimodal_source.clone();
        let saved_multimodal = instance.capabilities.supports_multimodal;

        // Adjust capabilities based on model name
        adjust_capabilities_for_model(&instance.model, &mut capabilities);

        // Restore preserved fields unless the caller explicitly cleared them
        // (sending `null` for the override). Since the request deserializer
        // can't distinguish "absent" from "explicit null" for an Option<bool>,
        // we always restore — the dedicated PATCH endpoint is the only way to
        // clear an override.
        //
        // Priority: user_override > runtime_api > name-based detection.
        // The frontend doesn't send multimodal_source, so adjust_capabilities_for_model
        // would overwrite a correct runtime_api detection with a less-accurate
        // name-based guess (e.g. qwen3.5 correctly detected by Ollama /api/show as
        // multimodal, but missed by both registry and heuristic). We must restore the
        // runtime_api source and its associated multimodal value.
        if capabilities.multimodal_user_override.is_none() {
            capabilities.multimodal_user_override = saved_user_override;
            if let Some(override_val) = saved_user_override {
                // Override restored → all three fields must reflect it:
                // supports_multimodal (effective value), source (provenance),
                // and multimodal_user_override (already set above). Without
                // syncing supports_multimodal, the runtime would read the
                // auto-detected value and ignore the user's override.
                capabilities.supports_multimodal = override_val;
                capabilities.multimodal_source = Some("user_override".to_string());
            } else if saved_source.as_deref() == Some("runtime_api") {
                // Runtime API detection (Ollama /api/show) is authoritative —
                // restore both the value and source so the correct detection
                // from the third-party backend isn't lost.
                capabilities.supports_multimodal = saved_multimodal;
                capabilities.multimodal_source = saved_source;
            }
            // else: leave source alone — adjust_capabilities_for_model set it.
        }

        instance.capabilities = capabilities;
    }
    instance.updated_at = chrono::Utc::now().timestamp();

    // Validate
    instance.validate().map_err(ErrorResponse::bad_request)?;

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

/// Request body for the capabilities-override PATCH endpoint.
///
/// All fields are optional. `multimodal: null` clears the override (returns
/// to auto-detection); `multimodal: true/false` pins the value.
#[derive(Debug, Deserialize)]
pub struct UpdateCapabilitiesOverrideRequest {
    /// Override the multimodal/vision capability.
    ///
    /// - `Some(true)`/`Some(false)` — pin the value, skip auto-detection.
    /// - `None` — clear the override, resume auto-detection.
    ///
    /// JSON `null` and field-absence both deserialize to `None` (serde's
    /// default behavior for `Option<T>`).
    #[serde(default)]
    pub multimodal: Option<bool>,
}

/// Set or clear a user override on backend capabilities.
///
/// `PATCH /api/llm-backends/:id/capabilities`
///
/// Currently only the `multimodal` (vision) capability is overridable — this
/// is the main source of false positives in auto-detection, and is the field
/// users most commonly need to correct manually.
pub async fn update_capabilities_override_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateCapabilitiesOverrideRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_manager()?;

    let mut instance = manager
        .get_instance(&id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Backend instance {}", id)))?;

    // Apply override.
    instance.capabilities.multimodal_user_override = req.multimodal;
    if let Some(v) = req.multimodal {
        instance.capabilities.supports_multimodal = v;
        instance.capabilities.multimodal_source = Some("user_override".to_string());
        tracing::info!(
            backend_id = %id,
            model = %instance.model,
            override_value = v,
            "User set multimodal override"
        );
    } else {
        // Override cleared — re-run layered detection immediately so the
        // stored value reflects the auto-detected truth.
        let detected = neomind_core::llm::detect_vision_capability(&instance.model);
        // Source attribution: 3-tier (registry / heuristic / unknown).
        // Use the same logic as `adjust_capabilities_for_model` so the source
        // is consistent across creation, update, and override-clear paths.
        let source: Option<&str> =
            if neomind_core::llm::registry::lookup_vision(&instance.model).is_some() {
                Some("registry")
            } else if neomind_core::llm::registry::heuristic_vision_match(&instance.model) {
                Some("heuristic")
            } else {
                None
            };
        tracing::info!(
            backend_id = %id,
            model = %instance.model,
            detected = detected,
            source = ?source,
            "User cleared multimodal override, re-detected"
        );
        instance.capabilities.supports_multimodal = detected;
        instance.capabilities.multimodal_source = source.map(str::to_string);
    }
    instance.updated_at = chrono::Utc::now().timestamp();

    manager
        .upsert_instance(instance.clone())
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    manager.clear_cache();

    ok(json!({
        "id": id,
        "supports_multimodal": instance.capabilities.supports_multimodal,
        "multimodal_user_override": instance.capabilities.multimodal_user_override,
        "multimodal_source": instance.capabilities.multimodal_source,
        "message": "Capabilities override applied",
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
    /// Convert storage BackendCapabilities to core BackendCapabilities
    fn convert_capabilities(
        storage_caps: &neomind_storage::BackendCapabilities,
    ) -> neomind_core::BackendCapabilities {
        neomind_core::BackendCapabilities {
            streaming: storage_caps.supports_streaming,
            multimodal: storage_caps.supports_multimodal,
            function_calling: storage_caps.supports_tools,
            thinking_display: storage_caps.supports_thinking,
            max_context: Some(storage_caps.max_context),
            multiple_models: false,
            modalities: Vec::new(),
            supports_images: storage_caps.supports_multimodal,
            reasoning: neomind_core::ReasoningCapabilities::default(),
        }
    }

    // Convert capabilities from storage type to core type
    let capabilities = Some(convert_capabilities(&instance.capabilities));

    let backend = match instance.backend_type {
        LlmBackendType::Ollama => {
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = instance.model.clone();
            LlmBackend::Ollama {
                endpoint,
                model,
                capabilities,
            }
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
                capabilities,
            }
        }
        LlmBackendType::Anthropic => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::Anthropic {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::Google => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::Google {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::XAi => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.x.ai/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::XAi {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::Qwen => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::Qwen {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::DeepSeek => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com".to_string());
            let model = instance.model.clone();
            LlmBackend::DeepSeek {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::GLM => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".to_string());
            let model = instance.model.clone();
            LlmBackend::GLM {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::MiniMax => {
            let api_key = instance.api_key.clone().unwrap_or_default();
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "https://api.minimax.chat/v1".to_string());
            let model = instance.model.clone();
            LlmBackend::MiniMax {
                api_key,
                endpoint,
                model,
                capabilities,
            }
        }
        LlmBackendType::LlamaCpp => {
            let endpoint = instance
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
            let model = instance.model.clone();
            LlmBackend::LlamaCpp {
                endpoint,
                model,
                capabilities,
            }
        }
    };

    // Update existing chat sessions and set as default for new sessions.
    // Note: SessionManager.sessions contains chat Agent instances (not AI agents).
    // AI agents use their own locked llm_backend_id via get_llm_runtime_for_agent.
    state
        .agents
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
        LlmBackendType::Qwen => crate::config::LlmSettingsRequest {
            backend: "qwen".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::DeepSeek => crate::config::LlmSettingsRequest {
            backend: "deepseek".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::GLM => crate::config::LlmSettingsRequest {
            backend: "glm".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::MiniMax => crate::config::LlmSettingsRequest {
            backend: "minimax".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: instance.api_key.clone(),
        },
        LlmBackendType::LlamaCpp => crate::config::LlmSettingsRequest {
            backend: "llamacpp".to_string(),
            model: instance.model.clone(),
            endpoint: instance.endpoint.clone(),
            api_key: None,
        },
    };

    if let Err(e) = state.save_llm_config(&settings_request).await {
        tracing::warn!(category = "settings", error = %e, "Failed to save LLM config to SettingsStore after backend activation");
    }

    // Start the memory scheduler (temp file cleanup)
    if let Err(e) = state.agents.start_memory_scheduler().await {
        tracing::warn!(category = "memory", error = %e, "Failed to start memory scheduler");
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

    let endpoint = params
        .endpoint
        .unwrap_or_else(|| "http://localhost:11434".to_string());
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
                tracing::warn!(
                    "Failed to get capabilities for {}: {}, using fallback detection",
                    model.name,
                    e
                );
                // Fallback to name-based detection if /api/show fails
                detect_ollama_model_capabilities_from_name(&model.name)
            }
        };

        models_with_caps.push(OllamaModelWithCapabilities {
            name: model.name.clone(),
            size: model.size,
            modified_at: model.modified_at,
            digest: model.digest,
            details: model.details,
            supports_multimodal: caps.supports_multimodal,
            supports_thinking: caps.supports_thinking,
            supports_tools: caps.supports_tools,
            max_context: caps.max_context,
            reasoning: reasoning_for_ollama_model(&model.name, caps.supports_thinking),
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

    // Tool calling — registry (supports_function_calling) with a conservative
    // name fallback for models absent from the registry.
    let supports_tools = neomind_core::llm::detect_tools_capability(model_name);

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
        // Track that this value came from a live runtime probe, so that
        // `ensure_instance_capabilities` doesn't second-guess it with the
        // less-accurate name-based heuristic on next load.
        multimodal_source: Some("runtime_api".to_string()),
        supports_thinking,
        supports_tools,
        max_context,
        ..Default::default()
    })
}

/// Detect thinking capability from model name only
///
/// Ollama's API doesn't provide thinking capability, so we infer it from model
/// naming patterns. Delegates to neomind-core's single `detect_thinking` to
/// stay consistent with the Ollama/llama.cpp runtimes (historically this
/// duplicated the rule and disagreed with them — e.g. it excluded `-vl` while
/// the runtime allowed modern multimodal+thinking models).
fn detect_thinking_from_name(model_name: &str) -> bool {
    neomind_core::llm::detect_thinking(model_name)
}

/// Fallback: Detect capabilities from model name when /api/show is not available.
/// Uses neomind-core's unified detect_vision_capability for consistency.
fn detect_ollama_model_capabilities_from_name(model_name: &str) -> BackendCapabilities {
    // Use unified vision detection from neomind-core
    let supports_multimodal = detect_vision_capability(model_name);

    let supports_thinking = detect_thinking_from_name(model_name);

    let supports_tools = neomind_core::llm::detect_tools_capability(model_name);

    let max_context = detect_ollama_model_context(model_name);

    // Source attribution matches `adjust_capabilities_for_model`.
    let multimodal_source = if neomind_core::llm::registry::lookup_vision(model_name).is_some() {
        Some("registry".to_string())
    } else if neomind_core::llm::registry::heuristic_vision_match(model_name) {
        Some("heuristic".to_string())
    } else {
        None
    };

    BackendCapabilities {
        supports_streaming: true,
        supports_multimodal,
        multimodal_source,
        supports_thinking,
        supports_tools,
        max_context,
        ..Default::default()
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
    /// Declared reasoning control for this model — drives the frontend's
    /// thinking-effort widget (readonly / boolean / level / effort).
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningCapabilitiesDto>,
}

/// Reasoning capabilities surfaced per-model in the Ollama list (and the
/// llama.cpp server-info probe). Mirrors the fields the frontend reads.
#[derive(Debug, Clone, Serialize)]
pub struct ReasoningCapabilitiesDto {
    supported_efforts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_effort: Option<String>,
    mandatory: bool,
    control: String,
}

/// Reasoning capabilities for an Ollama model in the list API.
/// Ollama controls thinking via `think: true/false/low/medium/high`, so a
/// thinking-capable model gets a Level control with the full effort set.
fn reasoning_for_ollama_model(
    model_name: &str,
    supports_thinking: bool,
) -> Option<ReasoningCapabilitiesDto> {
    // Use the unified thinking detector as an additional check — /api/show
    // reports `supports_thinking` from attention heads, but the name check
    // covers models the API probe might miss (e.g. qwq-* naming).
    if !supports_thinking && !neomind_core::llm::detect_thinking(model_name) {
        return None;
    }
    Some(ReasoningCapabilitiesDto {
        supported_efforts: ["none", "low", "medium", "high"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        default_effort: Some("high".to_string()),
        mandatory: false,
        control: "level".to_string(),
    })
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

    // GLM models (智谱)
    if name_lower.contains("glm") {
        return 128000;
    }

    // MiniMax models
    if name_lower.contains("abab") || name_lower.contains("minimax") {
        return 512000;
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
    128000
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
            ..Default::default()
        },
        LlmBackendType::OpenAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 128000,
            ..Default::default()
        },
        LlmBackendType::Anthropic => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 200000,
            ..Default::default()
        },
        LlmBackendType::Google => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 1000000,
            ..Default::default()
        },
        LlmBackendType::XAi => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: false,
            supports_tools: false,
            max_context: 128000,
            ..Default::default()
        },
        LlmBackendType::Qwen => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 32768,
            ..Default::default()
        },
        LlmBackendType::DeepSeek => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: false,
            supports_tools: true,
            max_context: 65536,
            ..Default::default()
        },
        LlmBackendType::GLM => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: false,
            supports_tools: true,
            max_context: 131072,
            ..Default::default()
        },
        LlmBackendType::MiniMax => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 512000,
            ..Default::default()
        },
        LlmBackendType::LlamaCpp => BackendCapabilities {
            supports_streaming: true,
            supports_multimodal: false,
            supports_thinking: true,
            supports_tools: true,
            max_context: 4096,
            ..Default::default()
        },
    }
}

/// Adjust capabilities based on the actual model name.
/// This refines the default backend-type capabilities with model-specific knowledge.
fn adjust_capabilities_for_model(model_name: &str, capabilities: &mut BackendCapabilities) {
    let name_lower = model_name.to_lowercase();

    // === Multimodal (vision) detection ===
    // Use the same logic as neomind-core capability detection.
    // Also record provenance so the frontend and diagnostics can show WHERE
    // the value came from. We do NOT touch `multimodal_user_override` here —
    // that field is sacred and only set by the explicit override endpoint.
    capabilities.supports_multimodal = detect_vision_capability(&name_lower);
    if capabilities.multimodal_user_override.is_none() {
        // Determine source: registry hit vs heuristic vs unknown.
        // We can't tell "registry" from "heuristic" from this call site without
        // re-running the layered lookup, so we use the more specific helper.
        if neomind_core::llm::registry::lookup_vision(&name_lower).is_some() {
            capabilities.multimodal_source = Some("registry".to_string());
        } else if neomind_core::llm::registry::heuristic_vision_match(&name_lower) {
            capabilities.multimodal_source = Some("heuristic".to_string());
        } else {
            capabilities.multimodal_source = None;
        }
    }

    // === Thinking detection ===
    // Models that support extended thinking (deepseek-r1, qwen thinking models).
    // NOTE: do NOT couple this to `supports_multimodal` — modern multimodal
    // models (gpt-4o, qwen3.5-vl, gemini-2.0-flash-thinking, claude-opus-4)
    // support both vision and thinking. The old "vision blocks thinking" rule
    // was correct for 2024-era llava-class models but is now stale; users can
    // always override via PATCH /capabilities.
    capabilities.supports_thinking = neomind_core::llm::detect_thinking(&name_lower);

    // === Tool support ===
    // registry (supports_function_calling) with name fallback, plus an
    // explicit guard for sub-1B models whose names don't contain the
    // fallback's "1b"/"tiny" keywords (e.g. `:0.5`).
    capabilities.supports_tools = neomind_core::llm::detect_tools_capability(model_name)
        && !name_lower.contains(":0.5")
        && !name_lower.contains(":0.5b");

    // Detect max context from model name
    capabilities.max_context = detect_model_context(model_name);
}

// ---------------------------------------------------------------------------
// llama.cpp server info endpoint
// ---------------------------------------------------------------------------

/// Lightweight /props response — only fields we need, ignores the huge chat_template.
#[derive(Debug, Deserialize)]
struct LlamaCppPropsLight {
    #[serde(default)]
    model_alias: Option<String>,
    #[serde(default)]
    model_path: Option<String>,
    #[serde(default)]
    default_generation_settings: Option<LlamaCppPropsGenSettings>,
    #[serde(default)]
    total_slots: Option<usize>,
    #[serde(default)]
    modalities: Option<LlamaCppModalities>,
    #[serde(default)]
    build_info: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlamaCppPropsGenSettings {
    #[serde(default)]
    n_ctx: Option<usize>,
    #[serde(default)]
    params: Option<LlamaCppPropsParams>,
}

#[derive(Debug, Deserialize)]
struct LlamaCppPropsParams {
    #[serde(default)]
    n_ctx: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct LlamaCppModalities {
    #[serde(default)]
    vision: Option<bool>,
}

impl LlamaCppPropsLight {
    fn n_ctx(&self) -> Option<usize> {
        self.default_generation_settings
            .as_ref()
            .and_then(|s| s.n_ctx.or_else(|| s.params.as_ref().and_then(|p| p.n_ctx)))
    }
}

/// Query parameters for fetching llama.cpp server info
#[derive(Debug, Deserialize)]
pub struct LlamaCppServerInfoQuery {
    /// llama.cpp server endpoint (default: http://127.0.0.1:8080)
    pub endpoint: Option<String>,
    /// Optional API key for --api-key authentication
    pub api_key: Option<String>,
}

/// llama.cpp server health response
#[derive(Debug, Serialize)]
struct LlamaCppHealthInfo {
    status: String,
    latency_ms: u64,
}

/// llama.cpp server properties info
#[derive(Debug, Serialize)]
struct LlamaCppServerDetails {
    /// Loaded model file name (extracted from full path)
    model_name: Option<String>,
    /// Context window size
    n_ctx: Option<usize>,
    /// Number of total slots
    total_slots: Option<usize>,
    /// Server version
    version: Option<String>,
}

/// Combined llama.cpp server info response
#[derive(Debug, Serialize)]
struct LlamaCppServerInfoResponse {
    status: String,
    health: LlamaCppHealthInfo,
    server: LlamaCppServerDetails,
    capabilities: BackendCapabilities,
}

/// GET /api/llm-backends/llamacpp/server-info?endpoint=http://127.0.0.1:8080
///
/// Fetches health check and server properties from a llama.cpp server,
/// returns combined info with auto-detected capabilities.
pub async fn list_llamacpp_server_info_handler(
    Query(params): Query<LlamaCppServerInfoQuery>,
) -> HandlerResult<serde_json::Value> {
    use reqwest::Client;

    let endpoint = params
        .endpoint
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    let base_url = endpoint.trim_end_matches('/');

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to create HTTP client: {}", e)))?;

    // 1. Health check
    let health_url = format!("{}/health", base_url);
    let mut req = client.get(&health_url);
    if let Some(ref key) = params.api_key {
        req = req.bearer_auth(key);
    }

    let start = std::time::Instant::now();
    let health_resp = req.send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let health_info = match health_resp {
        Ok(resp) if resp.status().is_success() => LlamaCppHealthInfo {
            status: "ok".to_string(),
            latency_ms,
        },
        Ok(resp) => LlamaCppHealthInfo {
            status: format!("error: HTTP {}", resp.status()),
            latency_ms,
        },
        Err(e) => LlamaCppHealthInfo {
            status: format!("unreachable: {}", e),
            latency_ms,
        },
    };

    let is_healthy = health_info.status == "ok";

    // 2. Fetch server props (only if healthy)
    let mut server_details = LlamaCppServerDetails {
        model_name: None,
        n_ctx: None,
        total_slots: None,
        version: None,
    };

    let mut capabilities = BackendCapabilities {
        supports_streaming: true,
        supports_multimodal: false,
        supports_thinking: true,
        supports_tools: true,
        max_context: 4096,
        ..Default::default()
    };

    if is_healthy {
        // 1. Fetch /v1/models FIRST — small, clean response for model name and n_ctx_train
        let models_url = format!("{}/v1/models", base_url);
        let mut req = client.get(&models_url);
        if let Some(ref key) = params.api_key {
            req = req.bearer_auth(key);
        }

        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.text().await {
                    if let Ok(models_resp) = serde_json::from_str::<serde_json::Value>(&body) {
                        // Model name from data[0].id
                        if let Some(model_id) = models_resp
                            .get("data")
                            .and_then(|d| d.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|m| m.get("id"))
                            .and_then(|v| v.as_str())
                        {
                            if !model_id.is_empty() {
                                server_details.model_name = Some(model_id.to_string());
                                adjust_capabilities_for_model(model_id, &mut capabilities);
                            }
                        }

                        // n_ctx_train from /v1/models is the model's trained context (more accurate)
                        if let Some(n_ctx_train) = models_resp
                            .get("data")
                            .and_then(|d| d.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|m| m.get("meta"))
                            .and_then(|m| m.get("n_ctx_train"))
                            .and_then(|v| v.as_u64())
                        {
                            capabilities.max_context = n_ctx_train as usize;
                        }
                    }
                }
            }
        }

        // 2. Fetch /props for runtime n_ctx, total_slots, modalities, and version
        // Use lightweight struct deserialization to avoid issues with the very large
        // chat_template field that can cause serde_json::Value parsing to fail.
        let props_url = format!("{}/props", base_url);
        let mut req = client.get(&props_url);
        if let Some(ref key) = params.api_key {
            req = req.bearer_auth(key);
        }

        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.text().await {
                    // Try typed deserialization first (ignores chat_template)
                    if let Ok(props) = serde_json::from_str::<LlamaCppPropsLight>(&body) {
                        // Fallback model name from model_alias or model_path
                        if server_details.model_name.is_none() {
                            if let Some(alias) = &props.model_alias {
                                if !alias.is_empty() {
                                    server_details.model_name = Some(alias.clone());
                                    adjust_capabilities_for_model(alias, &mut capabilities);
                                }
                            }
                        }
                        if server_details.model_name.is_none() {
                            if let Some(path) = &props.model_path {
                                if !path.is_empty() {
                                    let name = path.rsplit('/').next().unwrap_or(path).to_string();
                                    server_details.model_name = Some(name.clone());
                                    adjust_capabilities_for_model(&name, &mut capabilities);
                                }
                            }
                        }

                        // n_ctx from runtime settings
                        server_details.n_ctx = props.n_ctx();

                        // Use runtime n_ctx as fallback if no n_ctx_train
                        if capabilities.max_context == 4096 {
                            if let Some(n_ctx) = server_details.n_ctx {
                                capabilities.max_context = n_ctx;
                            }
                        }

                        server_details.total_slots = props.total_slots;

                        // Multimodal from modalities field
                        if let Some(vision) = props.modalities.as_ref().and_then(|m| m.vision) {
                            capabilities.supports_multimodal = vision;
                        }

                        // Version from build_info
                        server_details.version = props.build_info.clone();
                    }
                }
            }
        }
    }

    let response = LlamaCppServerInfoResponse {
        status: if is_healthy { "ok" } else { "error" }.to_string(),
        health: health_info,
        server: server_details,
        capabilities,
    };

    ok(json!(response))
}
