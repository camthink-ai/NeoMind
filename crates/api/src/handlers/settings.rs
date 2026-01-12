//! LLM settings handlers.

use axum::{extract::{Query, State}, Json};
use serde_json::json;

use super::{ServerState, common::{HandlerResult, ok}};
use crate::config::LlmSettingsRequest;
use crate::models::{ErrorResponse, OllamaModelsResponse};

/// Get current LLM settings.
pub async fn get_llm_settings_handler() -> HandlerResult<serde_json::Value> {
    // Try to load from database first
    match crate::config::load_llm_settings_from_db().await {
        Ok(Some(settings)) => {
            ok(json!({
                "backend": settings.backend_name(),
                "endpoint": settings.endpoint,
                "model": settings.model,
                "api_key": settings.api_key.as_ref().map(|k| if k.is_empty() { None } else { Some(k) }).flatten(),
                "temperature": settings.temperature,
                "top_p": settings.top_p,
                "max_tokens": settings.max_tokens,
                "updated_at": settings.updated_at,
            }))
        }
        Ok(None) => {
            // No settings in database, return empty
            ok(json!({
                "backend": serde_json::Value::Null,
                "endpoint": serde_json::Value::Null,
                "model": serde_json::Value::Null,
            }))
        }
        Err(e) => {
            tracing::warn!(category = "settings", error = %e, "Failed to load LLM settings");
            Err(ErrorResponse::internal(format!("Failed to load settings: {}", e)))
        }
    }
}

/// Set LLM configuration.
pub async fn set_llm_handler(
    State(state): State<ServerState>,
    Json(settings): Json<LlmSettingsRequest>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_agent::LlmBackend;

    let backend = match settings.backend.as_str() {
        "ollama" => {
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = settings.model.clone();
            LlmBackend::Ollama { endpoint, model }
        }
        "openai" => {
            let api_key = settings.api_key.clone()
                .unwrap_or_else(|| "".to_string());
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = settings.model.clone();
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
        _ => return Err(ErrorResponse::bad_request("Invalid backend")),
    };

    // Update the session manager's LLM backend
    state.session_manager.set_llm_backend(backend.clone()).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Save configuration to database for persistence
    if let Err(e) = state.save_llm_config(&settings).await {
        tracing::warn!(category = "settings", error = %e, "Failed to save LLM config");
    }

    ok(json!({
        "backend": settings.backend,
        "model": settings.model,
    }))
}

/// Test LLM connection with a real request.
pub async fn test_llm_handler(
    State(state): State<ServerState>,
    Json(settings): Json<LlmSettingsRequest>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_agent::LlmBackend;

    if settings.backend.is_empty() {
        return ok(json!({
            "connected": false,
            "error": "Backend is required",
        }));
    }

    // Construct backend from settings
    let backend = match settings.backend.as_str() {
        "ollama" => {
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = if settings.model.is_empty() {
                "qwen3-vl:2b".to_string()
            } else {
                settings.model.clone()
            };
            LlmBackend::Ollama { endpoint, model }
        }
        "openai" => {
            let api_key = settings.api_key.clone()
                .unwrap_or_else(|| "".to_string());
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = if settings.model.is_empty() {
                "gpt-4o-mini".to_string()
            } else {
                settings.model.clone()
            };
            LlmBackend::OpenAi { api_key, endpoint, model }
        }
        _ => return ok(json!({
            "connected": false,
            "error": format!("Invalid backend: {}", settings.backend),
        })),
    };

    // Create a temporary session for testing
    let test_session_id = state.session_manager.create_session().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create test session: {}", e)))?;

    // Set the backend for the test
    state.session_manager.set_llm_backend(backend.clone()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to set LLM backend: {}", e)))?;

    // Time the request
    let start = std::time::Instant::now();

    // Send a simple test message
    let result = state.session_manager.process_message(
        &test_session_id,
        "Reply with just 'OK' if you receive this."
    ).await;

    let latency_ms = start.elapsed().as_millis();

    // Clean up test session
    let _ = state.session_manager.remove_session(&test_session_id).await;

    match result {
        Ok(response) => {
            ok(json!({
                "connected": true,
                "backend": settings.backend,
                "model": settings.model,
                "latency_ms": latency_ms,
                "response": response.message.content,
                "processing_time_ms": response.processing_time_ms,
            }))
        }
        Err(e) => {
            ok(json!({
                "connected": false,
                "error": format!("LLM request failed: {}", e),
                "backend": settings.backend,
                "latency_ms": latency_ms,
            }))
        }
    }
}

/// Get available models from Ollama.
pub async fn list_ollama_models_handler(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Get endpoint from query params or use default
    let endpoint = params.get("endpoint")
        .cloned()
        .unwrap_or_else(|| "http://localhost:11434".to_string());

    // Build the API URL for listing models
    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));

    // Make request to Ollama
    let client = reqwest::Client::new();
    let response = client.get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to connect to Ollama: {}", e)))?;

    if !response.status().is_success() {
        return ok(json!({
            "models": [],
            "error": format!("Ollama returned status: {}", response.status()),
        }));
    }

    let ollama_response: OllamaModelsResponse = response
        .json::<OllamaModelsResponse>()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to parse response: {}", e)))?;

    // Extract model names (keep the full name with tag)
    let models: Vec<String> = ollama_response.models
        .iter()
        .map(|m| m.name.clone())
        .collect();

    ok(json!({
        "models": models,
        "endpoint": endpoint,
    }))
}

/// Request body for LLM generation.
#[derive(serde::Deserialize)]
pub struct LlmGenerateRequest {
    pub prompt: String,
}

/// Generate LLM response (one-shot, no session required).
/// This bypasses the agent's tool calling pipeline and calls LLM directly.
/// Useful for features like AI-assisted MDL generation.
pub async fn llm_generate_handler(
    State(_state): State<ServerState>,
    Json(req): Json<LlmGenerateRequest>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_agent::LlmBackend;
    use edge_ai_core::{Message, llm::backend::{LlmRuntime, LlmInput, GenerationParams}};

    // Load current LLM backend configuration
    let backend_config = crate::config::load_llm_config()
        .ok_or_else(|| ErrorResponse::bad_request("LLM not configured. Please configure LLM settings first."))?;

    // Convert LlmBackend to a Box<dyn LlmRuntime>
    let (llm_runtime, model_name): (Box<dyn LlmRuntime>, String) = match backend_config {
        LlmBackend::Ollama { endpoint, model } => {
            use edge_ai_llm::{OllamaRuntime, OllamaConfig};
            let config = OllamaConfig::new(&model).with_endpoint(&endpoint);
            let runtime = OllamaRuntime::new(config)
                .map_err(|e| ErrorResponse::internal(format!("Failed to create Ollama runtime: {}", e)))?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
        LlmBackend::OpenAi { api_key, endpoint, model } => {
            use edge_ai_llm::{CloudRuntime, CloudConfig};
            let config = if endpoint.is_empty() || endpoint.contains("api.openai.com") {
                CloudConfig::openai(&api_key).with_model(&model)
            } else {
                CloudConfig::custom(&api_key, &endpoint).with_model(&model)
            };
            let runtime = CloudRuntime::new(config)
                .map_err(|e| ErrorResponse::internal(format!("Failed to create Cloud runtime: {}", e)))?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
    };

    // Build the input with system prompt
    let system_prompt = "You are a helpful assistant.";
    let input = LlmInput {
        messages: vec![
            Message::system(system_prompt),
            Message::user(&req.prompt),
        ],
        params: GenerationParams {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            max_tokens: Some(2048),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
        },
        model: Some(model_name),
        stream: false,
    };

    let start = std::time::Instant::now();

    // Call LLM directly (bypassing agent's tool calling)
    let output = llm_runtime.generate(input).await
        .map_err(|e| ErrorResponse::internal(format!("LLM generation failed: {}", e)))?;

    let latency_ms = start.elapsed().as_millis();

    ok(json!({
        "response": output.text,
        "thinking": null,
        "tools_used": [],
        "processing_time_ms": latency_ms,
    }))
}
