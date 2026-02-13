//! LLM generation handler for one-shot LLM requests.
//! Used for features like AI-assisted MDL generation.

use axum::{extract::State, Json};
use serde_json::json;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

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
    use neomind_agent::LlmBackend;
    use neomind_core::{
        llm::backend::{GenerationParams, LlmInput, LlmRuntime},
        Message,
    };

    // Load current LLM backend configuration
    let backend_config = crate::config::load_llm_config().ok_or_else(|| {
        ErrorResponse::bad_request("LLM not configured. Please configure LLM settings first.")
    })?;

    // Convert LlmBackend to a Box<dyn LlmRuntime>
    let (llm_runtime, model_name): (Box<dyn LlmRuntime>, String) = match backend_config {
        LlmBackend::Ollama { endpoint, model } => {
            use neomind_llm::{OllamaConfig, OllamaRuntime};
            let config = OllamaConfig::new(&model).with_endpoint(&endpoint);
            let runtime = OllamaRuntime::new(config).map_err(|e| {
                ErrorResponse::internal(format!("Failed to create Ollama runtime: {}", e))
            })?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
        LlmBackend::OpenAi {
            api_key,
            endpoint,
            model,
        } => {
            use neomind_llm::{CloudConfig, CloudRuntime};
            let config = if endpoint.is_empty() || endpoint.contains("api.openai.com") {
                CloudConfig::openai(&api_key).with_model(&model)
            } else {
                CloudConfig::custom(&api_key, &endpoint).with_model(&model)
            };
            let runtime = CloudRuntime::new(config).map_err(|e| {
                ErrorResponse::internal(format!("Failed to create Cloud runtime: {}", e))
            })?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
        // Other backends (Anthropic, Google, XAi, Qwen, DeepSeek, GLM, MiniMax)
        // use CloudConfig with custom endpoint
        _backend => {
            use neomind_llm::{CloudConfig, CloudRuntime};
            let (api_key, endpoint, model) = match &_backend {
                LlmBackend::Anthropic {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::Google {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::XAi {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::Qwen {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::DeepSeek {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::GLM {
                    api_key,
                    endpoint,
                    model,
                }
                | LlmBackend::MiniMax {
                    api_key,
                    endpoint,
                    model,
                } => (api_key.clone(), endpoint.clone(), model.clone()),
                _ => return Err(ErrorResponse::bad_request("Unsupported LLM backend")),
            };
            let config = if endpoint.is_empty() {
                CloudConfig::custom(&api_key, &endpoint).with_model(&model)
            } else {
                CloudConfig::custom(&api_key, &endpoint).with_model(&model)
            };
            let runtime = CloudRuntime::new(config).map_err(|e| {
                ErrorResponse::internal(format!("Failed to create Cloud runtime: {}", e))
            })?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
    };

    // Build the input with system prompt
    let system_prompt = "You are a helpful assistant.";
    let input = LlmInput {
        messages: vec![Message::system(system_prompt), Message::user(&req.prompt)],
        params: GenerationParams {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: None,
            max_tokens: Some(usize::MAX),
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            thinking_enabled: None,
            max_context: None,
        },
        model: Some(model_name),
        stream: false,
        tools: None,
    };

    let start = std::time::Instant::now();

    // Call LLM directly (bypassing agent's tool calling)
    let output = llm_runtime
        .generate(input)
        .await
        .map_err(|e| ErrorResponse::internal(format!("LLM generation failed: {}", e)))?;

    let latency_ms = start.elapsed().as_millis();

    ok(json!({
        "response": output.text,
        "thinking": null,
        "tools_used": [],
        "processing_time_ms": latency_ms,
    }))
}

// ============================================================================
// Global Timezone Settings Handlers
// ============================================================================

/// Request body for updating timezone.
#[derive(serde::Deserialize)]
pub struct TimezoneRequest {
    pub timezone: String,
}

/// Response for timezone requests.
#[derive(serde::Serialize)]
pub struct TimezoneResponse {
    pub timezone: String,
    pub is_default: bool,
}

/// Get the current global timezone setting.
pub async fn get_timezone(State(_state): State<ServerState>) -> HandlerResult<TimezoneResponse> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let timezone = settings_store.get_global_timezone();
    let is_default = timezone == neomind_storage::DEFAULT_GLOBAL_TIMEZONE;

    ok(TimezoneResponse {
        timezone,
        is_default,
    })
}

/// Update the global timezone setting.
pub async fn update_timezone(
    State(_state): State<ServerState>,
    Json(req): Json<TimezoneRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    // Validate timezone using chrono-tz
    if req.timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err(ErrorResponse::bad_request(format!(
            "Invalid timezone: '{}'. Expected IANA format like 'Asia/Shanghai'",
            req.timezone
        )));
    }

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    settings_store
        .save_global_timezone(&req.timezone)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save timezone: {}", e)))?;

    tracing::info!("Global timezone updated to: {}", req.timezone);

    ok(json!({
        "success": true,
        "timezone": req.timezone,
    }))
}

/// Get available timezone options.
pub async fn list_timezones() -> HandlerResult<serde_json::Value> {
    // Common IANA timezones with display names
    let timezones = vec![
        ("Asia/Shanghai", "中国 (UTC+8)"),
        ("Asia/Tokyo", "日本 (UTC+9)"),
        ("Asia/Seoul", "韩国 (UTC+9)"),
        ("Asia/Singapore", "新加坡 (UTC+8)"),
        ("Asia/Dubai", "迪拜 (UTC+4)"),
        ("Europe/London", "伦敦 (UTC+0/+1)"),
        ("Europe/Paris", "巴黎 (UTC+1/+2)"),
        ("Europe/Berlin", "柏林 (UTC+1/+2)"),
        ("Europe/Moscow", "莫斯科 (UTC+3)"),
        ("America/New_York", "纽约 (UTC-5/-4)"),
        ("America/Los_Angeles", "洛杉矶 (UTC-8/-7)"),
        ("America/Chicago", "芝加哥 (UTC-6/-5)"),
        ("America/Toronto", "多伦多 (UTC-5/-4)"),
        ("America/Sao_Paulo", "圣保罗 (UTC-3/-2)"),
        ("Australia/Sydney", "悉尼 (UTC+10/+11)"),
        ("Pacific/Auckland", "奥克兰 (UTC+12/+13)"),
        ("UTC", "UTC (UTC+0)"),
    ];

    ok(json!({
        "timezones": timezones.iter().map(|(id, name)| {
            json!({
                "id": id,
                "name": name,
            })
        }).collect::<Vec<_>>()
    }))
}
