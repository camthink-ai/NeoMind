//! `config` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::extension::{MetricDataType, ParameterDefinition};
use neomind_storage::ExtensionRecord;

use super::*;

/// PATCH /api/extensions/:id/enabled
///
/// Master tool-toggle for an extension. `enabled=false` removes ALL of this
/// extension's tools from the LLM-facing list; `enabled=true` restores them
/// (subject to per-command disables). Storage is the source of truth; the
/// live ToolRegistry is refreshed from storage after the write.
#[utoipa::path(
    patch,
    path = "/api/extensions/{id}/enabled",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = SetToolEnabledRequest,
    responses(
        (status = 200, description = "Master enable flag updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn set_extension_enabled_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<SetToolEnabledRequest>,
) -> HandlerResult<serde_json::Value> {
    validate_extension_id(&id)?;

    // Shared pre-opened store from ServerState (was: per-request
    // ExtensionStore::open, whose failure swallowed into a 500).
    let store = state.extensions.store.clone();
    let mut record = store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Load extension: {e}")))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {id}")))?;

    record.enabled = req.enabled;
    record.touch();
    store
        .save(&record)
        .map_err(|e| ErrorResponse::internal(format!("Save extension: {e}")))?;

    refresh_tool_registry_disabled(&state).await;

    tracing::info!(
        extension = %id,
        enabled = req.enabled,
        "extension tool-toggle updated"
    );
    ok(json!({ "id": id, "enabled": req.enabled }))
}

/// GET /api/extensions/:id/config
///
/// Get the configuration schema and current values for an extension.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/config",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension config panel values"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_extension_config_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Get extension info using unified service
    let ext_info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Get current config from storage
    let current_config: Option<serde_json::Value> = state
        .extensions
        .store
        .load(&id)
        .ok()
        .flatten()
        .and_then(|r| r.config);

    // Build config schema from extension metadata
    let config_schema = if let Some(config_params) = &ext_info.metadata.config_parameters {
        build_config_schema_dto(config_params)
    } else {
        // No config parameters defined
        json!({"type": "object", "properties": {}})
    };

    ok(json!({
        "extension_id": id,
        "extension_name": ext_info.metadata.name,
        "config_schema": config_schema,
        "current_config": current_config.unwrap_or_else(|| json!({})),
    }))
}

/// PUT /api/extensions/:id/config
///
/// Update the configuration for an extension.
///
/// Note: This updates the stored configuration. The extension will need to be
/// reloaded for the new configuration to take effect.
#[utoipa::path(
    put,
    path = "/api/extensions/{id}/config",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Config panel values saved"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_extension_config_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    // Verify extension exists using unified service
    let ext_info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Validate config against schema if present
    if let Some(config_params) = &ext_info.metadata.config_parameters {
        validate_config(&config, config_params)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?;
    }

    // Save config to storage
    let store = state.extensions.store.clone();
    {
        if let Ok(Some(mut record)) = store.load(&id) {
            record.config = Some(config.clone());
            store.save(&record)?;
        } else {
            // Create new record with config
            let new_record = ExtensionRecord::new(
                id.clone(),
                ext_info.metadata.name.clone(),
                ext_info
                    .path
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "native".to_string(),
                ext_info.metadata.version.to_string(),
            )
            .with_config(config.clone());
            store.save(&new_record)?;
        }
    }

    // Attempt hot-reload: notify running extension without restart
    let hot_reload_result = state
        .extensions
        .runtime
        .send_config_update(&id, &config)
        .await;

    let message = match &hot_reload_result {
        Ok(()) => "Configuration updated and hot-reloaded to running extension.".to_string(),
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                error = %e,
                "Config hot-reload failed (config saved for next restart)"
            );
            "Configuration saved. Hot-reload failed — reload the extension for changes to take effect.".to_string()
        }
    };

    ok(json!({
        "extension_id": id,
        "config": config,
        "hot_reloaded": hot_reload_result.is_ok(),
        "message": message,
    }))
}

/// POST /api/extensions/:id/reload
///
/// Reload an extension with its current configuration.
/// Uses the unified extension service which handles both native and WASM extensions
/// via process isolation.
#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/reload",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Extension reloaded from disk"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn reload_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = state.extensions.runtime.clone();

    // Get extension info before reloading
    let ext_info = runtime
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let file_path = ext_info.path.clone();

    // Get current config
    let config: Option<serde_json::Value> = state
        .extensions
        .store
        .load(&id)
        .ok()
        .flatten()
        .and_then(|r| r.config);

    // Unload the extension
    runtime
        .unload(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to unload: {}", e)))?;

    // Re-load from file if we have the path
    let mut config_applied = false;
    if let Some(ref path) = file_path {
        // Load via unified service (handles both native and WASM)
        match runtime.load(path).await {
            Ok(metadata) => {
                // Clear error status on successful reload
                let store = state.extensions.store.clone();
                {
                    if let Ok(Some(mut record)) = store.load(&id) {
                        record.health_status = "ok".to_string();
                        record.last_error = None;
                        record.last_error_at = None;
                        let _ = store.save(&record);
                    }
                }

                // Apply saved config via the lifecycle `configure()` method.
                // NOTE: must NOT use `execute_command(id, "configure", cfg)` —
                // `configure` is a lifecycle method, not a dispatched command,
                // and is not in any extension's commands list. Doing so fails
                // with "Command not found: configure" on every reload (see
                // the analogous hot-reload path at send_config_update above).
                // The runtime routes send_config_update through the proper
                // IPC channel (ConfigUpdate) that the runner turns into a
                // call to `neomind_extension_configure_json`.
                if let Some(ref cfg) = config {
                    if let Err(e) = runtime.send_config_update(&metadata.id, cfg).await {
                        tracing::warn!(
                            extension_id = %id,
                            error = %e,
                            "Failed to apply config to extension during reload"
                        );
                    } else {
                        config_applied = true;
                        tracing::info!(
                            extension_id = %id,
                            "Applied saved config to extension during reload"
                        );
                    }
                }

                let is_isolated = runtime.is_isolated(&id).await;
                tracing::info!(
                    extension_id = %id,
                    is_isolated = is_isolated,
                    "Extension reloaded"
                );
            }
            Err(e) => {
                // Record the reload failure in storage so the UI shows Error state
                let store = state.extensions.store.clone();
                {
                    let _ = store.update_error_status(&id, &format!("Reload failed: {}", e));
                }
                return Err(ErrorResponse::internal(format!(
                    "Failed to reload extension: {}",
                    e
                )));
            }
        }
    }

    // Rebuild tool registry: reload may change the command set
    state.refresh_extension_tools().await;

    ok(json!({
        "extension_id": id,
        "message": "Extension reloaded from file",
        "config_applied": config_applied,
        "file_path": file_path,
    }))
}

/// Build JSON Schema for configuration parameters.
fn build_config_schema_dto(parameters: &[ParameterDefinition]) -> serde_json::Value {
    use neomind_core::extension::system::ParamMetricValue;

    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    let mut property_order = Vec::new();

    for param in parameters {
        let param_type = match param.param_type {
            MetricDataType::Float => "number",
            MetricDataType::Integer => "integer",
            MetricDataType::Boolean => "boolean",
            MetricDataType::String | MetricDataType::Enum { .. } => "string",
            MetricDataType::Binary => "string",
        };

        let mut param_schema = serde_json::json!({
            "type": param_type,
            "title": param.display_name.as_str(),
            "description": param.description.as_str(),
        });

        // Add enum options if present
        if let MetricDataType::Enum { options } = &param.param_type {
            param_schema["enum"] = serde_json::json!(options);
        }

        // Add default value if present - unwrap the ParamMetricValue to get actual JSON value
        if let Some(default_val) = &param.default_value {
            param_schema["default"] = match default_val {
                ParamMetricValue::Float(f) => serde_json::json!(f),
                ParamMetricValue::Integer(i) => serde_json::json!(i),
                ParamMetricValue::Boolean(b) => serde_json::json!(b),
                ParamMetricValue::String(s) => serde_json::json!(s),
                ParamMetricValue::Binary(_) => serde_json::json!(null),
                ParamMetricValue::Null => serde_json::json!(null),
            };
        }

        // Add min/max for numeric types
        if let Some(min) = param.min {
            param_schema["minimum"] = serde_json::json!(min);
        }
        if let Some(max) = param.max {
            param_schema["maximum"] = serde_json::json!(max);
        }

        property_order.push(param.name.clone());
        properties.insert(param.name.clone(), param_schema);

        if param.required {
            required.push(param.name.clone());
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "propertyOrder": property_order,
    })
}

/// Validate configuration against parameter definitions.
fn validate_config(
    config: &serde_json::Value,
    parameters: &[ParameterDefinition],
) -> std::result::Result<(), String> {
    let obj = config
        .as_object()
        .ok_or_else(|| "Config must be an object".to_string())?;

    for param in parameters {
        let value = obj.get(&param.name);

        // Check required parameters
        if param.required && value.is_none() {
            return Err(format!("Missing required parameter: {}", param.name));
        }

        if let Some(v) = value {
            // Validate type
            match &param.param_type {
                MetricDataType::Float => {
                    if !v.is_f64() && !v.is_i64() {
                        return Err(format!("Parameter '{}' must be a number", param.name));
                    }
                }
                MetricDataType::Integer => {
                    if !v.is_i64() {
                        return Err(format!("Parameter '{}' must be an integer", param.name));
                    }
                }
                MetricDataType::Boolean => {
                    if !v.is_boolean() {
                        return Err(format!("Parameter '{}' must be a boolean", param.name));
                    }
                }
                MetricDataType::String => {
                    if !v.is_string() {
                        return Err(format!("Parameter '{}' must be a string", param.name));
                    }
                }
                MetricDataType::Enum { options } => {
                    if let Some(s) = v.as_str() {
                        if !options.contains(&s.to_string()) {
                            return Err(format!(
                                "Parameter '{}' must be one of: {:?}",
                                param.name, options
                            ));
                        }
                    }
                }
                MetricDataType::Binary => {
                    // Binary configs are typically base64 strings
                    if !v.is_string() {
                        return Err(format!(
                            "Parameter '{}' must be a string (base64)",
                            param.name
                        ));
                    }
                }
            }

            // Validate min/max for numeric types
            if let Some(n) = v.as_f64() {
                if let Some(min) = param.min {
                    if n < min {
                        return Err(format!(
                            "Parameter '{}' must be at least {}",
                            param.name, min
                        ));
                    }
                }
                if let Some(max) = param.max {
                    if n > max {
                        return Err(format!(
                            "Parameter '{}' must be at most {}",
                            param.name, max
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Dashboard Components API
// ============================================================================

/// Component category for dashboard components.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentCategory {
    Chart,
    Metric,
    Table,
    Control,
    Media,
    Custom,
    Other,
}
