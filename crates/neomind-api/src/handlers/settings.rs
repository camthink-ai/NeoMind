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
        LlmBackend::Ollama {
            endpoint,
            model,
            capabilities: _,
        } => {
            use neomind_agent::llm_backends::{OllamaConfig, OllamaRuntime};
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
            capabilities: _,
        } => {
            use neomind_agent::llm_backends::{CloudConfig, CloudRuntime};
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
            use neomind_agent::llm_backends::{CloudConfig, CloudRuntime};
            let (api_key, endpoint, model) = match &_backend {
                LlmBackend::Anthropic {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::Google {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::XAi {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::Qwen {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::DeepSeek {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::GLM {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                }
                | LlmBackend::MiniMax {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
                } => (api_key.clone(), endpoint.clone(), model.clone()),
                _ => return Err(ErrorResponse::bad_request("Unsupported LLM backend")),
            };
            let config = CloudConfig::custom(&api_key, &endpoint).with_model(&model);
            let runtime = CloudRuntime::new(config).map_err(|e| {
                ErrorResponse::internal(format!("Failed to create Cloud runtime: {}", e))
            })?;
            (Box::new(runtime) as Box<dyn LlmRuntime>, model)
        }
    };

    // Build the input with system prompt (includes language policy to respond in user's language).
    // Canonical copy lives in `crates/neomind-agent/src/prompts/system_prompt.md` (Language Policy
    // section) — keep this in sync if that policy text changes.
    const LANGUAGE_POLICY: &str = "## Language Policy (Highest Priority)\n\nYou MUST respond in the EXACT SAME language as the user's message.\n- User writes in English → respond in English\n- User writes in Chinese → respond in Chinese\n- Never mix languages in a single response\n- When uncertain, default to English";
    let system_prompt = format!("You are a helpful assistant.\n\n{}", LANGUAGE_POLICY);
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
            thinking_effort: None,
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

// ============================================================================
// Retention Configuration Handlers
// ============================================================================

/// Get the current retention configuration.
pub async fn get_retention_config(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let config = settings_store.get_retention_config();

    ok(json!({
        "enabled": config.enabled,
        "interval_hours": config.interval_hours,
        "default_retention": config.default_retention,
        "image_retention": config.image_retention,
    }))
}

/// Update the retention configuration.
pub async fn update_retention_config(
    State(_state): State<ServerState>,
    Json(req): Json<RetentionConfigRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    // Validate interval
    if req.interval_hours == 0 {
        return Err(ErrorResponse::bad_request(
            "interval_hours must be greater than 0",
        ));
    }

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let config = neomind_storage::settings::RetentionConfig {
        enabled: req.enabled,
        interval_hours: req.interval_hours,
        default_retention: req.default_retention,
        image_retention: req.image_retention,
    };

    settings_store
        .save_retention_config(&config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save retention config: {}", e)))?;

    tracing::info!(
        enabled = config.enabled,
        interval_h = config.interval_hours,
        default_h = ?config.default_retention,
        image_h = ?config.image_retention,
        "Retention configuration updated"
    );

    ok(json!({
        "success": true,
        "enabled": config.enabled,
        "interval_hours": config.interval_hours,
        "default_retention": config.default_retention,
        "image_retention": config.image_retention,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct AgentDefaultsRequest {
    #[serde(default)]
    pub max_rounds: u32,
    #[serde(default)]
    pub execution_timeout_secs: u64,
    #[serde(default)]
    pub tool_concurrency: usize,
    #[serde(default)]
    pub default_temperature: f32,
    #[serde(default)]
    pub default_top_p: f32,
    #[serde(default)]
    pub default_thinking_enabled: Option<bool>,
}

/// Get agent execution defaults (max_rounds, timeout, concurrency, sampling).
pub async fn get_agent_defaults(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";
    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    let config = settings_store.get_agent_defaults();

    ok(json!({
        "max_rounds": config.max_rounds,
        "execution_timeout_secs": config.execution_timeout_secs,
        "tool_concurrency": config.tool_concurrency,
        "default_temperature": config.default_temperature,
        "default_top_p": config.default_top_p,
        "default_thinking_enabled": config.default_thinking_enabled,
    }))
}

/// Update agent execution defaults. Values are clamped to sane ranges.
/// Applies to the NEXT agent execution (not mid-flight).
pub async fn update_agent_defaults(
    State(_state): State<ServerState>,
    Json(req): Json<AgentDefaultsRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    let config = neomind_storage::AgentDefaults {
        max_rounds: req.max_rounds.clamp(1, 50),
        execution_timeout_secs: req.execution_timeout_secs.clamp(30, 1800),
        tool_concurrency: req.tool_concurrency.clamp(1, 16),
        default_temperature: req.default_temperature.clamp(0.0, 2.0),
        default_top_p: req.default_top_p.clamp(0.0, 1.0),
        default_thinking_enabled: req.default_thinking_enabled,
    };

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    settings_store
        .save_agent_defaults(&config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save agent defaults: {}", e)))?;

    tracing::info!(
        max_rounds = config.max_rounds,
        timeout_secs = config.execution_timeout_secs,
        tool_conc = config.tool_concurrency,
        temp = config.default_temperature,
        top_p = config.default_top_p,
        thinking = ?config.default_thinking_enabled,
        "Agent defaults updated"
    );

    ok(json!({
        "success": true,
        "max_rounds": config.max_rounds,
        "execution_timeout_secs": config.execution_timeout_secs,
        "tool_concurrency": config.tool_concurrency,
        "default_temperature": config.default_temperature,
        "default_top_p": config.default_top_p,
        "default_thinking_enabled": config.default_thinking_enabled,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct DeviceDefaultsRequest {
    #[serde(default)]
    pub default_offline_timeout_secs: u64,
    #[serde(default)]
    pub auto_onboard_enabled: bool,
}

/// Get device defaults (offline timeout, auto-onboarding).
pub async fn get_device_defaults(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";
    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    let config = settings_store.get_device_defaults();

    ok(json!({
        "default_offline_timeout_secs": config.default_offline_timeout_secs,
        "auto_onboard_enabled": config.auto_onboard_enabled,
    }))
}

/// Update device defaults. offline_timeout is live; auto_onboard applies on next restart.
pub async fn update_device_defaults(
    State(_state): State<ServerState>,
    Json(req): Json<DeviceDefaultsRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::SettingsStore;

    const SETTINGS_DB_PATH: &str = "data/settings.redb";

    let config = neomind_storage::DeviceDefaults {
        default_offline_timeout_secs: req.default_offline_timeout_secs.max(10),
        auto_onboard_enabled: req.auto_onboard_enabled,
    };

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    settings_store
        .save_device_defaults(&config)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save device defaults: {}", e)))?;

    tracing::info!(
        offline_timeout = config.default_offline_timeout_secs,
        auto_onboard = config.auto_onboard_enabled,
        "Device defaults updated"
    );

    ok(json!({
        "success": true,
        "default_offline_timeout_secs": config.default_offline_timeout_secs,
        "auto_onboard_enabled": config.auto_onboard_enabled,
    }))
}

/// Manually trigger a retention cleanup.
pub async fn trigger_retention_cleanup(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_storage::{SettingsStore, TimeSeriesStore};

    const SETTINGS_DB_PATH: &str = "data/settings.redb";
    const TELEMETRY_DB_PATH: &str = "data/telemetry.redb";

    let settings_store = SettingsStore::open(SETTINGS_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let config = settings_store.get_retention_config();
    let policy = config.to_retention_policy();

    let ts_store = TimeSeriesStore::open(TELEMETRY_DB_PATH)
        .map_err(|e| ErrorResponse::internal(format!("Failed to open telemetry store: {}", e)))?;

    // Apply the policy synchronously (cheap) so the config is live before
    // we trigger cleanup.
    ts_store.set_retention_policy(policy).await;

    // Spawn the actual cleanup in the background so the HTTP request
    // doesn't block on what could be minutes of deletion work (large
    // backlogs with millions of expired points). The in-progress flag
    // on TimeSeriesStore dedupes against the hourly background task.
    let ts_store_clone = ts_store.clone();
    tokio::spawn(async move {
        match ts_store_clone.apply_retention().await {
            Ok(result) => {
                if result.points_removed > 0 {
                    tracing::info!(
                        points_removed = result.points_removed,
                        metrics_cleaned = result.metrics_cleaned.len(),
                        "Manual retention cleanup completed (background)"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Manual retention cleanup failed (background)");
            }
        }
    });

    ok(json!({
        "success": true,
        "triggered": true,
        "message": "Retention cleanup scheduled in background",
    }))
}

/// Request body for updating retention configuration.
#[derive(serde::Deserialize)]
pub struct RetentionConfigRequest {
    pub enabled: bool,
    pub interval_hours: u64,
    pub default_retention: Option<u64>,
    pub image_retention: Option<u64>,
}

/// Trigger an immediate data-directory backup (admin only).
///
/// Copies every `*.redb` + secret file into `data/backups/backup-<ts>/`,
/// verifies each copied database opens (redb crash-recovery check), and
/// prunes old backups down to the retention limit. The periodic scheduler
/// (`NEOMIND_BACKUP_INTERVAL_SECS`, default 24h) calls the same path.
pub async fn create_backup_handler(
    State(state): State<ServerState>,
    axum::extract::Extension(admin): axum::extract::Extension<crate::auth_users::SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }

    let data_dir = state.data_dir.clone();
    let keep: usize = std::env::var("NEOMIND_BACKUP_KEEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);

    let result = tokio::task::spawn_blocking(move || {
        let manifest =
            neomind_storage::backup::create_backup(&data_dir, env!("CARGO_PKG_VERSION"))?;
        let pruned = neomind_storage::backup::prune_backups(&data_dir, keep);
        Ok::<_, neomind_storage::backup::BackupError>((manifest, pruned))
    })
    .await
    .map_err(|e| ErrorResponse::internal(format!("Backup task failed: {}", e)))?
    .map_err(|e| ErrorResponse::internal(format!("Backup failed: {}", e)))?;

    let (manifest, pruned) = result;
    tracing::info!(
        admin = %admin.username,
        id = %manifest.id,
        pruned,
        "Manual backup triggered"
    );

    ok(json!({
        "id": manifest.id,
        "created_at": manifest.created_at,
        "total_bytes": manifest.total_bytes,
        "files": manifest.files,
        "pruned_old_backups": pruned,
    }))
}

/// List existing backups (admin only), newest first.
///
/// Restoring is deliberately manual: stop the server, copy the files from
/// `data/backups/<id>/` back into the data dir, start the server.
pub async fn list_backups_handler(
    State(state): State<ServerState>,
    axum::extract::Extension(admin): axum::extract::Extension<crate::auth_users::SessionInfo>,
) -> HandlerResult<serde_json::Value> {
    if admin.role != crate::auth_users::UserRole::Admin {
        return Err(ErrorResponse::bad_request("Admin access required"));
    }

    let backups = neomind_storage::backup::list_backups(&state.data_dir);
    ok(json!({ "backups": backups }))
}
