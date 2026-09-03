//! Extension API handlers.
//!
//! Handlers for managing dynamically loaded extensions (.so/.dylib/.dll/.wasm).
//! Extensions are distinct from user configurations like LLM backends or device connections.
//!
//! V2 Extension System:
//! - Extensions use device-standard types (MetricDefinition, ExtensionCommand)
//! - Commands no longer declare output_fields or config
//! - extension_type field removed from metadata

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::handlers::devices::models::TimeRangeQuery;
use crate::models::error::ErrorResponse;
use crate::server::types::MAX_EXTENSION_DOWNLOAD_SIZE;
use crate::server::ServerState;
use futures::StreamExt;
use neomind_core::datasource::DataSourceId;
use neomind_core::extension::{MetricDataType, ParameterDefinition};
use neomind_storage::{ExtensionRecord, ExtensionStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Validate an extension ID to prevent path traversal in filesystem operations.
/// Extension IDs are kebab-case identifiers (e.g. "weather-forecast").
/// Rejects empty, too-long, or characters outside [a-zA-Z0-9-_].
fn validate_extension_id(id: &str) -> Result<(), ErrorResponse> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ErrorResponse::bad_request(format!(
            "Invalid extension ID: '{}'",
            id
        )));
    }
    Ok(())
}

/// Extension DTO for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionDto {
    /// Extension ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// Author
    pub author: Option<String>,
    /// Current state
    pub state: String,
    /// File path
    pub file_path: Option<String>,
    /// Loaded at timestamp
    pub loaded_at: Option<i64>,
    /// Health status: "ok", "warning", "error", "unknown"
    #[serde(default = "default_health_status")]
    pub health_status: String,
    /// Last error message if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Last error timestamp if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<i64>,
    /// Consecutive crashes (0 = healthy / stopped on purpose). Non-zero
    /// while stopped means the extension crashed.
    #[serde(default)]
    pub consecutive_crashes: u32,
    /// Reason for the most recent crash, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_crash_reason: Option<String>,
    /// Commands provided by this extension
    #[serde(default)]
    pub commands: Vec<CommandDescriptorDto>,
    /// Metrics provided by this extension (V2)
    #[serde(default)]
    pub metrics: Vec<MetricDescriptorDto>,
    /// Configuration parameters for this extension
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_parameters: Option<Vec<ConfigParamDto>>,
    /// Master tool-toggle: when false, none of this extension's tools are
    /// exposed to the LLM. Persisted in ExtensionRecord.enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Per-command disable list (command names without extension-id prefix).
    /// Source of truth for per-command toggles; UI flips membership.
    #[serde(default)]
    pub disabled_commands: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_health_status() -> String {
    "unknown".to_string()
}

/// Configuration parameter DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigParamDto {
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

/// Metric descriptor DTO (V2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDescriptorDto {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    pub unit: String,
    pub description: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub required: bool,
}

/// Build JSON Schema for parameters from V2 ParameterDefinition list.
fn build_parameters_schema(parameters: &[ParameterDefinition]) -> serde_json::Value {
    use neomind_core::extension::system::ParamMetricValue;

    let mut properties = HashMap::new();
    let mut required = Vec::new();

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
            "description": param.description,
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

        properties.insert(param.name.clone(), param_schema);

        if param.required {
            required.push(param.name.clone());
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// Extension type DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionTypeDto {
    /// Type identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
}

/// Query parameters for listing extensions.
#[derive(Debug, Deserialize)]
pub struct ListExtensionsQuery {
    /// Filter by state
    pub state: Option<String>,
}

/// Request to register an extension.
#[derive(Debug, Deserialize)]
pub struct RegisterExtensionRequest {
    /// Path to the extension file
    pub file_path: String,
    /// Whether to auto-start the extension
    #[serde(default)]
    pub auto_start: bool,
}

/// Request to execute an extension command.
#[derive(Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    /// Command name
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: serde_json::Value,
}

/// GET /api/extensions
/// List all registered extensions (including failed to load).
pub async fn list_extensions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListExtensionsQuery>,
) -> HandlerResult<Vec<ExtensionDto>> {
    // Use unified service to get successfully loaded extensions
    let loaded_extensions = state.extensions.runtime.list().await;

    // Also get all extension records from storage (including failed ones)
    let stored_records = state.extensions.store.load_all().unwrap_or_default();

    // Build a set of loaded extension IDs for quick lookup
    let loaded_ids: std::collections::HashSet<String> = loaded_extensions
        .iter()
        .map(|e| e.metadata.id.clone())
        .collect();

    let mut extensions: Vec<ExtensionDto> = Vec::new();

    // First, add all successfully loaded extensions
    for info in loaded_extensions {
        extensions.push(extension_info_to_dto(&info, &state.extensions.store));
    }

    // Then, add extensions from storage that failed to load
    for record in stored_records {
        // Skip if already in loaded extensions
        if loaded_ids.contains(&record.id) {
            continue;
        }

        // Skip uninstalled extensions
        if record.uninstalled {
            continue;
        }

        // Create DTO for failed extension
        extensions.push(ExtensionDto {
            id: record.id.clone(),
            name: record.name,
            version: record.version,
            description: record.description,
            author: record.author,
            state: "Failed".to_string(),
            file_path: Some(record.file_path),
            loaded_at: None,
            health_status: record.health_status,
            last_error: record.last_error,
            last_error_at: record.last_error_at,
            consecutive_crashes: 0,
            last_crash_reason: None,
            commands: Vec::new(),
            metrics: Vec::new(),
            config_parameters: None,
            enabled: record.enabled,
            disabled_commands: record.disabled_commands,
        });
    }

    // Filter by state
    if let Some(state_filter) = &query.state {
        extensions.retain(|e| e.state.to_lowercase() == state_filter.to_lowercase());
    }

    ok(extensions)
}

/// Helper function to convert ExtensionInfo to ExtensionDto
fn extension_info_to_dto(
    info: &neomind_core::extension::ExtensionRuntimeInfo,
    store: &ExtensionStore,
) -> ExtensionDto {
    use neomind_core::extension::system::ParamMetricValue;

    // Load persisted record (if any) to get enabled flag + disabled_commands.
    // Defaults: enabled=true, no disabled commands. Same lookup is reused for
    // health_status below, so we cache it once.
    let record_opt: Option<ExtensionRecord> = store.load(&info.metadata.id).ok().flatten();
    let (enabled, disabled_commands): (bool, Vec<String>) = record_opt
        .as_ref()
        .map(|r| (r.enabled, r.disabled_commands.clone()))
        .unwrap_or((true, Vec::new()));
    let disabled_set: std::collections::HashSet<String> =
        disabled_commands.iter().cloned().collect();

    // Convert commands to DTOs (V2 format)
    let commands: Vec<CommandDescriptorDto> = info
        .commands
        .iter()
        .map(|cmd| CommandDescriptorDto {
            id: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.description.clone(),
            input_schema: build_parameters_schema(&cmd.parameters),
            output_fields: vec![], // V2: Commands don't declare output fields
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 300000,
                is_stream: false,
                expected_duration_ms: None,
            },
            disabled: !enabled || disabled_set.contains(&cmd.name),
        })
        .collect();

    // Convert metrics to DTOs (V2)
    let metrics: Vec<MetricDescriptorDto> = info
        .metrics
        .iter()
        .map(|m| MetricDescriptorDto {
            name: m.name.clone(),
            display_name: m.display_name.clone(),
            data_type: format!("{:?}", m.data_type),
            unit: m.unit.clone(),
            description: None, // V2: MetricDefinition doesn't have description
            min: m.min,
            max: m.max,
            required: m.required,
        })
        .collect();

    // Convert config parameters to DTOs
    let config_parameters = info.metadata.config_parameters.as_ref().map(|params| {
        params
            .iter()
            .map(|p| ConfigParamDto {
                name: p.name.clone(),
                display_name: p.display_name.clone(),
                description: p.description.clone(),
                param_type: format!("{:?}", p.param_type).to_lowercase(),
                required: p.required,
                default: p.default_value.as_ref().map(|v| match v {
                    ParamMetricValue::Float(f) => serde_json::json!(f),
                    ParamMetricValue::Integer(i) => serde_json::json!(i),
                    ParamMetricValue::Boolean(b) => serde_json::json!(b),
                    ParamMetricValue::String(s) => serde_json::json!(s),
                    ParamMetricValue::Binary(_) => serde_json::json!(null),
                    ParamMetricValue::Null => serde_json::json!(null),
                }),
                min: p.min,
                max: p.max,
                options: match &p.param_type {
                    MetricDataType::Enum { options } => options.clone(),
                    _ => Vec::new(),
                },
            })
            .collect()
    });

    // Try to get health status from storage (reuse the record loaded above)
    let (health_status, last_error, last_error_at) = match &record_opt {
        Some(record) => (
            record.health_status.clone(),
            record.last_error.clone(),
            record.last_error_at,
        ),
        None => ("unknown".to_string(), None, None),
    };

    // Determine state based on is_running and health_status. A stopped
    // extension with crash-loop counters did not stop on purpose — it
    // crashed; the old derivation hid that behind "Stopped".
    let state_str = if !info.is_running && info.consecutive_crashes > 0 {
        "Crashed"
    } else if !info.is_running {
        "Stopped"
    } else if health_status == "error" {
        "Error"
    } else if health_status == "warning" {
        "Warning"
    } else if info.is_isolated {
        "Running (Isolated)"
    } else {
        "Running"
    };

    ExtensionDto {
        id: info.metadata.id.clone(),
        name: info.metadata.name.clone(),
        version: info.metadata.version.to_string(),
        description: info.metadata.description.clone(),
        author: info.metadata.author.clone(),
        state: state_str.to_string(),
        file_path: info.path.as_ref().map(|p| p.display().to_string()),
        loaded_at: None, // Not available in ExtensionRuntimeInfo
        health_status,
        last_error,
        last_error_at,
        consecutive_crashes: info.consecutive_crashes,
        last_crash_reason: info.last_crash_reason.clone(),
        commands,
        metrics,
        config_parameters,
        enabled,
        disabled_commands,
    }
}

/// GET /api/extensions/:id
/// Get a specific extension.
pub async fn get_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionDto> {
    // Use unified service to get both in-process and isolated extensions
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    ok(extension_info_to_dto(&info, &state.extensions.store))
}

/// GET /api/extensions/types
/// List available extension types.
pub async fn list_extension_types_handler() -> HandlerResult<Vec<ExtensionTypeDto>> {
    let types = vec![
        ExtensionTypeDto {
            id: "llm_provider".to_string(),
            name: "LLM Provider".to_string(),
            description: "Provides a new LLM backend implementation".to_string(),
        },
        ExtensionTypeDto {
            id: "device_protocol".to_string(),
            name: "Device Protocol".to_string(),
            description: "Implements a device communication protocol".to_string(),
        },
        ExtensionTypeDto {
            id: "alert_channel_type".to_string(),
            name: "Alert Channel Type".to_string(),
            description: "Provides a new alert notification channel type".to_string(),
        },
        ExtensionTypeDto {
            id: "tool".to_string(),
            name: "Tool".to_string(),
            description: "Provides AI function calling tools".to_string(),
        },
        ExtensionTypeDto {
            id: "generic".to_string(),
            name: "Generic".to_string(),
            description: "Generic extension".to_string(),
        },
    ];

    ok(types)
}

/// Fall back to the release-level `checksums.txt` for a package sha256.
///
/// The marketplace index does not carry per-build sha256; the Extensions CI
/// uploads a `checksums.txt` (sha256sum format) to the SAME release as the
/// .nep assets, so integrity data is at least as fresh as the package
/// itself. Derives the checksum URL from the package URL and looks up the
/// matching filename.
async fn fetch_release_checksum(
    client: &reqwest::Client,
    package_url: &str,
    nep_filename: &str,
) -> Option<String> {
    let base = package_url.rsplit_once('/')?.0;
    let url = format!("{base}/checksums.txt");
    let text = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name == nep_filename && sha.len() == 64 {
            return Some(sha.to_string());
        }
    }
    None
}

/// Resolve a client-provided package/library path, confining it to the
/// server's data directory.
///
/// `file_path` used to accept any host path, which made the register/upload/
/// validate endpoints a read-and-try-load primitive for arbitrary files on
/// the machine for anyone holding credentials. Relative paths resolve
/// against the data dir; absolute paths must land inside it (after
/// canonicalization, so `..` and symlinks can't escape).
pub(crate) fn resolve_confined_package_path(raw: &str) -> Result<PathBuf, ErrorResponse> {
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let data_root = std::path::PathBuf::from(&data_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&data_dir));

    let candidate = PathBuf::from(raw);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        data_root.join(candidate)
    };
    let canonical = joined
        .canonicalize()
        .map_err(|_| ErrorResponse::not_found(format!("Package file not found: {}", raw)))?;
    if !canonical.starts_with(&data_root) {
        return Err(ErrorResponse::bad_request(
            "file_path must point inside the server data directory",
        ));
    }
    Ok(canonical)
}

/// POST /api/extensions
/// Register a new extension from file path.
pub async fn register_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Store the resolved canonical path: load_from_storage replays
    // record.file_path verbatim on every boot, so persisting the raw
    // request string would keep an outside-data-dir path loadable forever
    // (the confinement would only ever check the write side).
    let path = resolve_confined_package_path(&req.file_path)?;

    let metadata = runtime.load(&path).await.map_err(|e| {
        // Check for specific error types to return appropriate HTTP status codes
        let error_msg = e.to_string();
        if error_msg.contains("already registered") || error_msg.contains("Already registered") {
            ErrorResponse::conflict(format!("Extension already registered: {}", error_msg))
        } else if error_msg.contains("not found") || error_msg.contains("NotFound") {
            ErrorResponse::not_found(format!("Extension file not found: {}", error_msg))
        } else if error_msg.contains("incompatible") || error_msg.contains("Incompatible") {
            ErrorResponse::validation(format!("Incompatible extension: {}", error_msg))
        } else {
            ErrorResponse::bad_request(format!("Failed to load extension: {}", error_msg))
        }
    })?;

    let ext_id = metadata.id.clone();
    let ext_name = metadata.name.clone();
    let ext_version = metadata.version.to_string();

    // Save to persistent storage for auto-load on server restart
    // V2: Use empty string for extension_type (storage API still requires it)
    let store = state.extensions.store.clone();
    {
        let record = neomind_storage::ExtensionRecord::new(
            ext_id.clone(),
            ext_name.clone(),
            // Canonical, data-dir-confined path (NOT the raw request value) —
            // replayed verbatim by load_from_storage on every boot.
            path.display().to_string(),
            String::new(), // V2: No extension_type, use empty string
            ext_version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_auto_start(req.auto_start);

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
            // Don't fail the request if storage fails, extension is already loaded in memory
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    ok(serde_json::json!({
        "message": "Extension registered successfully",
        "extension_id": ext_id,
        "name": ext_name,
        "version": ext_version,
        "auto_start": req.auto_start
    }))
}

/// DELETE /api/extensions/:id
/// Unregister an extension.
pub async fn unregister_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists in memory or storage
    let in_memory = runtime.contains(&id).await;
    let in_storage = state.extensions.store.load(&id).ok().flatten().is_some();

    // Extension must exist somewhere to unregister
    if !in_memory && !in_storage {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Unregister from memory if present (handles both in-process and isolated)
    if in_memory {
        if let Err(e) = runtime.unregister(&id).await {
            tracing::warn!(
                extension_id = %id,
                error = %e,
                "Failed to unregister extension from memory (continuing with storage cleanup)"
            );
        }
    }

    // Mark as uninstalled in storage (instead of deleting) to prevent auto-discovery
    // from re-registering it on server restart
    let store = state.extensions.store.clone();
    {
        if let Err(e) = store.mark_uninstalled(&id) {
            tracing::warn!("Failed to mark extension as uninstalled: {}", e);
        }
    }

    // Clean up extension metrics data from telemetry.redb
    // Extension metrics are stored with source_part = "extension:{extension_id}"
    cleanup_extension_metrics(&state, &id).await;

    ok(serde_json::json!({
        "message": "Extension unregistered",
        "extension_id": id
    }))
}

/// Clean up extension metrics data from time-series storage.
async fn cleanup_extension_metrics(state: &ServerState, extension_id: &str) {
    // Get the metrics storage from ExtensionState
    let metrics_storage = &state.extensions.metrics_storage;

    // Extension metrics are stored with source_part = "extension:{extension_id}"
    let source_part = format!("extension:{}", extension_id);

    // List all metrics for this extension
    match metrics_storage.list_metrics(&source_part).await {
        Ok(metrics) => {
            if !metrics.is_empty() {
                tracing::info!(
                    extension_id = %extension_id,
                    metrics_count = metrics.len(),
                    metrics = ?metrics,
                    "Extension unregistered, {} metric data series will be cleaned up by retention policy",
                    metrics.len()
                );
                // Note: TimeSeriesStorage doesn't have a bulk delete API
                // The data will eventually be cleaned up by retention policies
                // For immediate cleanup, we would need to add a delete method to TimeSeriesStorage
            }
        }
        Err(e) => {
            tracing::warn!(
                extension_id = %extension_id,
                error = %e,
                "Failed to list extension metrics for cleanup"
            );
        }
    }
}

/// POST /api/extensions/:id/start
/// Start an extension.
///
/// Note: In the new extension system, extensions are always active once registered.
/// This endpoint exists for API compatibility only.
pub async fn start_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Extensions are always active in the new system
    ok(serde_json::json!({
        "message": "Extension is active",
        "extension_id": id,
        "note": "Extensions are always active once registered"
    }))
}

/// POST /api/extensions/:id/stop
/// Stop an extension.
///
/// Note: In the new extension system, extensions cannot be stopped.
/// They remain active until unregistered. This endpoint exists for API compatibility only.
pub async fn stop_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Extensions cannot be stopped in the new system
    ok(serde_json::json!({
        "message": "Extensions cannot be stopped",
        "extension_id": id,
        "note": "To deactivate an extension, unregister it instead"
    }))
}

/// GET /api/extensions/:id/health
/// Check extension health.
pub async fn extension_health_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let healthy = runtime
        .health_check(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Health check failed: {}", e)))?;

    ok(serde_json::json!({
        "extension_id": id,
        "healthy": healthy
    }))
}

/// Extension log entry DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionLogEntryDto {
    /// Timestamp (milliseconds since epoch)
    pub timestamp: i64,
    /// Log level (trace/debug/info/warn/error)
    pub level: String,
    /// Log message
    pub message: String,
}

/// GET /api/extensions/:id/logs
/// Get extension log entries.
pub async fn get_extension_logs_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<ExtensionLogEntryDto>> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let logs = runtime
        .get_logs(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get logs: {}", e)))?;

    let dtos: Vec<ExtensionLogEntryDto> = logs
        .into_iter()
        .map(|entry| ExtensionLogEntryDto {
            timestamp: entry.timestamp,
            level: entry.level,
            message: entry.message,
        })
        .collect();

    ok(dtos)
}

/// DELETE /api/extensions/:id/logs
/// Clear extension log entries.
pub async fn clear_extension_logs_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check extension exists before attempting IPC
    runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    runtime
        .clear_logs(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to clear logs: {}", e)))?;

    ok(serde_json::json!({ "message": "Logs cleared" }))
}

/// Publish ExtensionOutput events for extension command results.
///
/// This enables real-time dashboard updates when extension commands are executed.
/// Extracts metric values from the command result and publishes events for each metric.
async fn publish_extension_metrics(
    state: &ServerState,
    extension_id: &str,
    result: &serde_json::Value,
) {
    use neomind_core::{event::NeoMindEvent, MetricValue as CoreMetricValue};

    // Get event bus if available
    let event_bus = match &state.core.event_bus {
        Some(bus) => bus,
        None => return, // No event bus, skip publishing
    };

    // Get extension info to know which metrics to extract
    // Use unified service to get info from both isolated and in-process extensions
    let extensions = state.extensions.runtime.list().await;
    let ext_info = match extensions.iter().find(|e| e.metadata.id == extension_id) {
        Some(info) => info,
        None => return, // Extension not found, skip
    };

    // Skip if extension has no metrics
    if ext_info.metrics.is_empty() {
        return;
    }

    // Extract result as object if possible
    let result_obj = match result.as_object() {
        Some(obj) => obj,
        None => return, // Result is not an object, can't extract metrics
    };

    let timestamp = chrono::Utc::now().timestamp();

    // Publish event for each metric found in result
    for metric in &ext_info.metrics {
        // Look for metric value in result (support multiple formats)
        let metric_value = result_obj
            .get(&metric.name)
            .or_else(|| result_obj.get("data").and_then(|d| d.get(&metric.name)))
            .or_else(|| {
                result_obj.get("data").and_then(|d| {
                    if let Some(name) = d.get("name").and_then(|n| n.as_str()) {
                        if name == metric.name {
                            d.get("value")
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            });

        if let Some(value) = metric_value {
            // Convert JSON value to Core MetricValue
            let core_value = match metric.data_type {
                neomind_core::extension::MetricDataType::Float => {
                    value.as_f64().map(CoreMetricValue::Float)
                }
                neomind_core::extension::MetricDataType::Integer => {
                    value.as_i64().map(CoreMetricValue::Integer)
                }
                neomind_core::extension::MetricDataType::Boolean => {
                    value.as_bool().map(CoreMetricValue::Boolean)
                }
                neomind_core::extension::MetricDataType::String => value
                    .as_str()
                    .map(|s| CoreMetricValue::String(s.to_string())),
                _ => None,
            };

            if let Some(v) = core_value {
                let event = NeoMindEvent::ExtensionOutput {
                    extension_id: extension_id.to_string(),
                    output_name: format!("{}:{}", extension_id, metric.name), // 修改：添加扩展ID前缀
                    value: v,
                    timestamp,
                    labels: None,
                    quality: None,
                };
                if !event_bus.publish(event).await {
                    tracing::debug!("No subscribers for extension output event");
                }
            }
        }
    }
}

/// POST /api/extensions/:id/command
/// Execute a command on an extension.
///
/// Includes panic protection to prevent server crashes from buggy extensions.
pub async fn execute_extension_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists first
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!(
            "Extension '{}' not found",
            id
        )));
    }

    // Validate command name (non-empty, max 256 chars, no control characters)
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate args payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.args).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Command arguments too large (max 1 MB)",
            ));
        }
    }

    // Execute command with panic protection
    let result = match runtime.execute_command(&id, &req.command, &req.args).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                error = %e,
                "Extension command execution failed"
            );
            return Err(ErrorResponse::internal(format!(
                "Command execution failed: {}",
                e
            )));
        }
    };

    // Publish ExtensionOutput events for agent triggers and dashboard subscriptions
    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(result)
}

/// Safe version of publish_extension_metrics that handles errors gracefully
async fn publish_extension_metrics_safe(
    state: &ServerState,
    extension_id: &str,
    result: &serde_json::Value,
) {
    // Use a timeout to prevent hanging on slow operations
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        publish_extension_metrics(state, extension_id, result),
    )
    .await
    {
        Ok(()) => {
            tracing::debug!(extension_id = %extension_id, "Extension metrics published successfully");
        }
        Err(_) => {
            tracing::warn!(extension_id = %extension_id, "Timeout while publishing extension metrics");
        }
    }
}

// ============================================================================
// Extension Invoke/Stream APIs
// ============================================================================

/// Request to invoke an extension.
#[derive(Debug, Deserialize)]
pub struct InvokeExtensionRequest {
    /// Command/function to invoke
    pub command: String,
    /// Input parameters
    #[serde(default)]
    pub params: serde_json::Value,
}

/// POST /api/extensions/:id/invoke
/// Invoke an extension and get JSON response.
///
/// This is a simplified version of execute_extension_command_handler
/// that returns results in a more JSON-friendly format for AI agents.
pub async fn invoke_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<InvokeExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Validate command name
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate params payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.params).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Invoke parameters too large (max 1 MB)",
            ));
        }
    }

    // Execute command with proper error logging
    let result = match runtime
        .execute_command(&id, &req.command, &req.params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                error = %e,
                "Extension invoke failed"
            );
            return Err(ErrorResponse::internal(format!("Invoke failed: {}", e)));
        }
    };

    // Publish ExtensionOutput events with timeout protection
    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(serde_json::json!({
        "extension_id": id,
        "command": req.command,
        "result": result,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// GET /api/extensions/:id/stream
/// Get streaming output from an extension.
///
/// Returns information about the extension's stream capability.
/// For actual streaming, clients should use WebSocket or SSE endpoints.
///
/// V2: Streaming capability not declared in command config.
/// This endpoint always returns false for streaming support.
pub async fn stream_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if extension exists and get its info
    let info = state.extensions.runtime.get_info(&id).await;
    let Some(info) = info else {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    };

    // Isolated extensions support streaming via WebSocket
    let has_streaming = info.is_isolated && info.is_running;
    let stream_url = if has_streaming {
        format!("/api/extensions/{}/stream", id)
    } else {
        String::new()
    };

    ok(serde_json::json!({
        "extension_id": id,
        "supports_streaming": has_streaming,
        "stream_url": stream_url,
        "output_mode": if has_streaming { "stream" } else { "once" },
    }))
}

// ============================================================================
// Command-Based Extension DTOs and Handlers
// ============================================================================

/// Command descriptor DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptorDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_fields: Vec<OutputFieldDto>,
    pub config: CommandConfigDto,
    /// True when this command is excluded from the LLM tool registry — either
    /// named in the parent extension's `disabled_commands` list, or the parent
    /// extension's master `enabled` flag is off.
    #[serde(default)]
    pub disabled: bool,
}

/// Output field DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFieldDto {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub is_primary: bool,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// Command configuration DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfigDto {
    pub requires_auth: bool,
    pub timeout_ms: u64,
    pub is_stream: bool,
    pub expected_duration_ms: Option<u64>,
}

/// Data source info DTO
/// Matches frontend ExtensionDataSourceInfo interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceInfoDto {
    pub id: String, // Format: "extension:{extension_id}:{command}:{field}"
    pub extension_id: String,
    pub command: String,
    pub field: String,
    pub display_name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// GET /api/extensions/:id/commands
///
/// List all commands for an extension (V2 format)
pub async fn list_extension_commands_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<CommandDescriptorDto>> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Load persisted disable state so `disabled` flag reflects current setting.
    let (ext_enabled, disabled_cmds): (bool, std::collections::HashSet<String>) =
        match state.extensions.store.load(&id).ok().flatten() {
            Some(r) => (r.enabled, r.disabled_commands.iter().cloned().collect()),
            None => (true, Default::default()),
        };

    let result: Vec<CommandDescriptorDto> = info
        .commands
        .iter()
        .map(|cmd| CommandDescriptorDto {
            id: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.description.clone(),
            input_schema: build_parameters_schema(&cmd.parameters),
            output_fields: vec![], // V2: Commands don't declare output fields
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 300000,
                is_stream: false,
                expected_duration_ms: None,
            },
            disabled: !ext_enabled || disabled_cmds.contains(&cmd.name),
        })
        .collect();

    ok(result)
}

/// Request body for tool-enable toggles.
#[derive(Debug, Deserialize)]
pub struct SetToolEnabledRequest {
    pub enabled: bool,
}

/// Rebuild the ToolRegistry disabled set from the on-disk state of all
/// extensions and push it live. Called from both PATCH handlers below so the
/// LLM picks up the change on the next tool-calling round without a server
/// restart. Cheap: O(n_extensions × n_commands) string formatting.
async fn refresh_tool_registry_disabled(state: &ServerState) {
    let Some(registry) = state.agents.session_manager.get_tool_registry().await else {
        return;
    };

    let mut disabled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let store = state.extensions.store.clone();
    {
        if let Ok(records) = store.load_all() {
            for r in records {
                if !r.enabled {
                    // Master off: every command of this extension is hidden.
                    // We don't know all command names here cheaply, so we mark
                    // by prefix-lookup at the registry level using the disabled
                    // set as exact names; the registry's `definitions_for_llm`
                    // also needs a prefix mode. Simpler: walk the loaded
                    // extension runtime to enumerate command names.
                    if let Some(info) = state.extensions.runtime.get(&r.id).await {
                        for cmd in &info.commands {
                            disabled.insert(format!("{}:{}", r.id, cmd.name));
                        }
                    }
                    continue;
                }
                for cmd_name in &r.disabled_commands {
                    disabled.insert(format!("{}:{}", r.id, cmd_name));
                }
            }
        }
    }
    registry.set_disabled(disabled);
}

/// PATCH /api/extensions/:id/enabled
///
/// Master tool-toggle for an extension. `enabled=false` removes ALL of this
/// extension's tools from the LLM-facing list; `enabled=true` restores them
/// (subject to per-command disables). Storage is the source of truth; the
/// live ToolRegistry is refreshed from storage after the write.
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

/// PATCH /api/extensions/:id/commands/:cmd/enabled
///
/// Per-command tool-toggle. `enabled=false` adds the command to
/// `disabled_commands`; `enabled=true` removes it. Master `enabled` flag is
/// untouched. Storage is the source of truth; the live ToolRegistry is
/// refreshed from storage after the write.
pub async fn set_extension_command_enabled_handler(
    State(state): State<ServerState>,
    Path((id, cmd)): Path<(String, String)>,
    Json(req): Json<SetToolEnabledRequest>,
) -> HandlerResult<serde_json::Value> {
    validate_extension_id(&id)?;
    if cmd.is_empty() {
        return Err(ErrorResponse::bad_request(
            "Command name required".to_string(),
        ));
    }

    // Shared pre-opened store from ServerState (was: per-request
    // ExtensionStore::open, whose failure swallowed into a 500).
    let store = state.extensions.store.clone();
    let mut record = store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Load extension: {e}")))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {id}")))?;

    if req.enabled {
        record.disabled_commands.retain(|c| c != &cmd);
    } else if !record.disabled_commands.iter().any(|c| c == &cmd) {
        record.disabled_commands.push(cmd.clone());
    }
    record.touch();
    store
        .save(&record)
        .map_err(|e| ErrorResponse::internal(format!("Save extension: {e}")))?;

    refresh_tool_registry_disabled(&state).await;

    tracing::info!(
        extension = %id,
        command = %cmd,
        enabled = req.enabled,
        "extension command tool-toggle updated"
    );
    ok(json!({ "id": id, "command": cmd, "enabled": req.enabled }))
}

/// GET /api/extensions/:id/event-subscriptions
///
/// Get event subscriptions for an extension.
pub async fn get_event_subscriptions_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    if !state.extensions.runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    let subscriptions = state.extensions.runtime.get_event_subscriptions(&id).await;

    ok(serde_json::json!({
        "extension_id": id,
        "subscriptions": subscriptions,
    }))
}

/// GET /api/extensions/:id/descriptor
///
/// Get the full extension descriptor (metadata, commands, metrics, capabilities).
pub async fn get_extension_descriptor_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Build descriptor response from ExtensionRuntimeInfo
    ok(serde_json::json!({
        "metadata": {
            "id": info.metadata.id,
            "name": info.metadata.name,
            "version": info.metadata.version,
            "description": info.metadata.description,
            "author": info.metadata.author,
        },
        "commands": info.commands,
        "metrics": info.metrics,
        "is_isolated": info.is_isolated,
        "is_running": info.is_running,
    }))
}

/// GET /api/extensions/:id/metrics/:metric/data
///
/// Query historical data for an extension metric
///
/// Uses typed DataSourceId for data source identification.
pub async fn query_extension_metric_data_handler(
    State(state): State<ServerState>,
    Path((extension_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    use neomind_devices::mdl::MetricValue;

    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let start = query.start.unwrap_or(end - 86400); // Default 24 hours

    // Use typed DataSourceId
    let source_id = DataSourceId::extension(&extension_id, &metric);

    // Query from extension metrics storage using DataSourceId parts
    let points = state
        .extensions
        .metrics_storage
        .query(
            &source_id.source_part(),
            source_id.metric_part(),
            start,
            end,
        )
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query metric: {:?}", e)))?;

    // Cap the limit to prevent memory exhaustion
    const MAX_METRIC_QUERY_LIMIT: usize = 10000;
    let effective_limit = query.limit.unwrap_or(1000).min(MAX_METRIC_QUERY_LIMIT);

    let data_points: Vec<serde_json::Value> = points
        .iter()
        .take(effective_limit)
        .map(|point| {
            let value_json = match &point.value {
                MetricValue::Integer(n) => serde_json::json!(n),
                MetricValue::Float(f) => serde_json::json!(f),
                MetricValue::String(s) => serde_json::json!(s),
                MetricValue::Boolean(b) => serde_json::json!(b),
                MetricValue::Binary(data) => {
                    serde_json::json!(STANDARD.encode(data))
                }
                MetricValue::Array(arr) => serde_json::json!(arr),
                MetricValue::Null => serde_json::json!(null),
            };
            json!({
                "timestamp": point.timestamp,
                "value": value_json,
                "quality": point.quality,
            })
        })
        .collect();

    ok(json!({
        "source_id": source_id.storage_key(),
        "extension_id": extension_id,
        "metric": metric,
        "start": start,
        "end": end,
        "count": data_points.len(),
        "data": data_points,
    }))
}

/// POST /api/extensions/:id/push-metrics
///
/// Push metric values from an external source (device, gateway, etc.)
/// Stores metrics and publishes ExtensionOutput events immediately,
/// enabling real-time data updates without waiting for the next poll cycle.
///
/// Body: `{ "metrics": { "temperature_c": 23.2, "humidity": 80 } }`
pub async fn push_extension_metrics_handler(
    State(state): State<ServerState>,
    Path(extension_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use neomind_devices::mdl::MetricValue;

    let metrics_map = body
        .get("metrics")
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'metrics' field".to_string()))?;

    let obj = metrics_map
        .as_object()
        .ok_or_else(|| ErrorResponse::bad_request("'metrics' must be an object".to_string()))?;

    if obj.is_empty() {
        return Err(ErrorResponse::bad_request("'metrics' is empty".to_string()));
    }

    // Limit the number of metrics per request to prevent abuse
    const MAX_METRICS_PER_REQUEST: usize = 100;
    if obj.len() > MAX_METRICS_PER_REQUEST {
        return Err(ErrorResponse::bad_request(format!(
            "Too many metrics: {} (max {})",
            obj.len(),
            MAX_METRICS_PER_REQUEST
        )));
    }

    // Convert JSON values to MetricValues
    let mut metrics: Vec<(&str, MetricValue)> = Vec::new();
    for (key, val) in obj {
        let mv = match val {
            serde_json::Value::Number(n) if n.is_f64() => {
                MetricValue::Float(n.as_f64().ok_or_else(|| {
                    ErrorResponse::bad_request(format!("Invalid float value for metric '{}'", key))
                })?)
            }
            serde_json::Value::Number(n) if n.is_i64() => {
                MetricValue::Integer(n.as_i64().ok_or_else(|| {
                    ErrorResponse::bad_request(format!(
                        "Invalid integer value for metric '{}'",
                        key
                    ))
                })?)
            }
            serde_json::Value::String(s) => MetricValue::String(s.clone()),
            serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
            other => MetricValue::String(other.to_string()),
        };
        metrics.push((key.as_str(), mv));
    }

    // Store and publish events
    let source_id = DataSourceId::extension(&extension_id, "_");
    let source_part = source_id.source_part();
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let timestamp_secs = timestamp_ms / 1000;
    let mut stored = 0;

    for (name, value) in &metrics {
        // [unit fix] the extension-metrics store is SECONDS (the collection
        // path divides by 1000 at extension_metrics.rs); this wrote millis,
        // corrupting range queries and the metrics-data endpoint.
        let dp = neomind_devices::telemetry::DataPoint::new(timestamp_secs, value.clone());
        if let Err(e) = state
            .extensions
            .metrics_storage
            .write(&source_part, name, dp)
            .await
        {
            tracing::warn!(extension_id = %extension_id, metric = %name, error = %e, "[PUSH] Failed to store");
            continue;
        }
        stored += 1;

        // Publish ExtensionOutput event
        if let Some(bus) = state.core.event_bus.as_ref() {
            let core_val = match value {
                MetricValue::Integer(n) => neomind_core::MetricValue::Integer(*n),
                MetricValue::Float(f) => neomind_core::MetricValue::Float(*f),
                MetricValue::String(s) => neomind_core::MetricValue::String(s.clone()),
                MetricValue::Boolean(b) => neomind_core::MetricValue::Boolean(*b),
                other => neomind_core::MetricValue::String(format!("{:?}", other)),
            };
            bus.publish_sync(neomind_core::NeoMindEvent::ExtensionOutput {
                extension_id: extension_id.clone(),
                output_name: name.to_string(),
                value: core_val,
                timestamp: timestamp_secs,
                labels: None,
                quality: None,
            });
        }
    }

    tracing::info!(
        extension_id = %extension_id,
        stored,
        total = metrics.len(),
        "[PUSH] Metrics pushed"
    );

    ok(json!({
        "ok": true,
        "stored": stored,
        "total": metrics.len(),
    }))
}

/// GET /api/extensions/:id/data-sources
///
/// List data sources (metrics) provided by an extension
///
/// Uses typed DataSourceId for clean data source identification.
pub async fn list_extension_data_sources_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<DataSourceInfoDto>> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let mut sources = Vec::new();

    // Return extension metrics as data sources using DataSourceId
    for metric in &info.metrics {
        let source_id = DataSourceId::extension(&id, &metric.name);
        sources.push(DataSourceInfoDto {
            id: source_id.storage_key(),
            extension_id: id.clone(),
            command: String::new(), // V2: No command field
            field: metric.name.clone(),
            display_name: format!("{}: {}", info.metadata.name, metric.display_name),
            data_type: format!("{:?}", metric.data_type),
            unit: if metric.unit.is_empty() {
                None
            } else {
                Some(metric.unit.clone())
            },
            description: metric.display_name.clone(), // V2: Use display_name as description
            aggregatable: true,                       // V2: Metrics are generally aggregatable
            default_agg_func: "last".to_string(),
        });
    }

    ok(sources)
}

/// Extension capability for dashboard/automation integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCapabilityDto {
    pub extension_id: String,
    pub extension_name: String,
    #[serde(rename = "type")]
    pub cap_type: String, // "provider", "processor", "hybrid"
    pub metrics: Vec<ExtensionMetricDto>,
    pub commands: Option<Vec<ExtensionCommandDto>>,
    pub tools: Option<Vec<ExtensionToolDto>>,
}

/// Extension metric for dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetricDto {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
}

/// Extension command for transform operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCommandDto {
    pub name: String,
    pub description: String,
}

/// Extension tool for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionToolDto {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
}

/// GET /api/extensions/capabilities
///
/// Get all extension capabilities for dashboard/automation integration
///
/// V2: Uses extension metrics and command parameters schema
pub async fn list_extension_capabilities_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<ExtensionCapabilityDto>> {
    let extensions = state.extensions.runtime.list().await;

    let mut capabilities = Vec::new();

    for ext_info in extensions {
        // Skip extensions with no metrics or commands
        if ext_info.metrics.is_empty() && ext_info.commands.is_empty() {
            continue;
        }

        let mut metrics = Vec::new();
        let mut commands = Vec::new();
        let mut tools = Vec::new();

        // V2: Extract metrics from extension metrics (not command output fields)
        for metric in &ext_info.metrics {
            metrics.push(ExtensionMetricDto {
                name: metric.name.clone(),
                data_type: format!("{:?}", metric.data_type),
                unit: if metric.unit.is_empty() {
                    None
                } else {
                    Some(metric.unit.clone())
                },
            });
        }

        // Extract command info for transforms and agents
        for cmd in &ext_info.commands {
            // Command info for transforms
            commands.push(ExtensionCommandDto {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
            });

            // Command info for agents (as tools)
            tools.push(ExtensionToolDto {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
                parameters: Some(build_parameters_schema(&cmd.parameters)),
            });
        }

        // Create provider capability for dashboard (if has metrics)
        if !metrics.is_empty() {
            capabilities.push(ExtensionCapabilityDto {
                extension_id: ext_info.metadata.id.clone(),
                extension_name: ext_info.metadata.name.clone(),
                cap_type: "provider".to_string(),
                metrics: metrics.clone(),
                commands: Some(commands.clone()),
                tools: Some(tools.clone()),
            });
        }

        // Create processor capability for transforms (if has commands)
        if !commands.is_empty() {
            capabilities.push(ExtensionCapabilityDto {
                extension_id: ext_info.metadata.id.clone(),
                extension_name: ext_info.metadata.name.clone(),
                cap_type: "processor".to_string(),
                metrics: vec![], // Not used for processor type
                commands: Some(commands.clone()),
                tools: Some(tools.clone()),
            });
        }
    }

    ok(capabilities)
}

// ============================================================================
// EXTENSION MARKETPLACE API
// ============================================================================

/// Configuration for cloud extension marketplace
const MARKET_BRANCH: &str = "main";
const DEFAULT_EXTENSION_MARKET_BASE_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions";

/// Effective extension-marketplace base URL.
///
/// Precedence: saved value (Settings → Preferences, admin) >
/// `NEOMIND_EXTENSION_MARKET_URL` env > built-in default. The default host
/// (`raw.githubusercontent.com`) is often unreachable from CN networks —
/// before this existed the source was hardcoded with NO override at all,
/// while the component market and LLM catalog both had env overrides.
/// Mirrors follow the component-market shape:
/// `https://ghfast.top/https://raw.githubusercontent.com/camthink-ai/...`
pub(crate) fn extension_market_base_url() -> String {
    let saved = neomind_storage::SettingsStore::open_default()
        .ok()
        .and_then(|s| s.load("extension_market_url").ok().flatten());
    if let Some(url) = saved {
        if let Some(clean) = normalize_market_url(&url) {
            return clean;
        }
    }
    if let Ok(url) = std::env::var("NEOMIND_EXTENSION_MARKET_URL") {
        if let Some(clean) = normalize_market_url(&url) {
            return clean;
        }
    }
    DEFAULT_EXTENSION_MARKET_BASE_URL.to_string()
}

/// Accept an http(s) URL, trimmed of trailing slashes. Empty or non-URL
/// values fall back to `None` (→ the built-in default).
fn normalize_market_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && trimmed.len() > "https://".len()
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Cloud extension metadata from index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudExtension {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// URL to full metadata (can also be specified as metadata_path)
    #[serde(default, alias = "metadata_path")]
    pub metadata_url: Option<String>,
    /// Frontend component info from index
    #[serde(default)]
    pub frontend: Option<FrontendInfo>,
    /// Available builds by platform
    #[serde(default)]
    pub builds: HashMap<String, ExtensionBuild>,
}

/// Frontend info from marketplace index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendInfo {
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
}

/// Full extension metadata from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceExtensionMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub readme_url: Option<String>,

    /// Extension type: native, wasm, frontend-only
    #[serde(default = "default_extension_type")]
    #[serde(rename = "type")]
    pub extension_type: String,

    /// ABI version for native extensions
    #[serde(default)]
    pub abi_version: u32,

    /// SDK version used to build the extension
    #[serde(default)]
    pub sdk_version: String,

    /// Keywords for search
    #[serde(default)]
    pub keywords: Vec<String>,

    /// .nep package URL (if available as a package instead of individual binaries)
    #[serde(default)]
    pub package_url: Option<String>,

    /// Package SHA256 checksum (for .nep packages)
    #[serde(default)]
    pub package_sha256: Option<String>,

    #[serde(default)]
    pub capabilities: ExtensionCapabilities,

    /// Commands at top level (for backward compatibility, merged into capabilities)
    #[serde(default)]
    pub commands: Vec<CommandInfo>,

    /// Metrics at top level (for backward compatibility, merged into capabilities)
    #[serde(default)]
    pub metrics: Vec<MetricInfo>,

    #[serde(default)]
    pub builds: HashMap<String, ExtensionBuild>,

    #[serde(default)]
    pub requirements: ExtensionRequirements,

    /// Safety/isolation settings (can also be specified as isolation)
    #[serde(default, alias = "isolation")]
    pub safety: ExtensionSafety,

    /// Configuration parameters for the extension
    #[serde(default)]
    pub config_parameters: Vec<ConfigParameterInfo>,

    /// Dashboard components provided by this extension
    #[serde(default)]
    pub dashboard_components: Vec<DashboardComponentInfo>,

    /// Frontend components (for backward compatibility)
    #[serde(default)]
    pub frontend: Option<FrontendInfo>,

    /// Permissions required by the extension
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_extension_type() -> String {
    "native".to_string()
}

/// Extension capabilities from metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionCapabilities {
    #[serde(default)]
    pub tools: Vec<ToolDescriptor>,
    #[serde(default)]
    pub metrics: Vec<MetricInfo>,
    #[serde(default)]
    pub commands: Vec<CommandInfo>,
    /// Streaming capability (for video/image processing extensions)
    #[serde(default)]
    pub streaming: Option<StreamingCapability>,
}

/// Tool descriptor from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub returns: Option<String>,
}

/// Metric info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    /// Data type (can also be specified as "type")
    #[serde(default, alias = "type")]
    pub data_type: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

/// Command info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// Streaming capability definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingCapability {
    pub mode: String,
    pub direction: String,
    #[serde(default)]
    pub supported_data_types: Vec<String>,
    #[serde(default)]
    pub max_chunk_size: usize,
    #[serde(default)]
    pub preferred_chunk_size: usize,
    #[serde(default)]
    pub max_concurrent_sessions: usize,
}

/// Configuration parameter info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigParameterInfo {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type", default)]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
}

/// Dashboard component info from marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentInfo {
    #[serde(rename = "type")]
    pub component_type: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default)]
    pub export_name: Option<String>,
}

/// Extension build info for different platforms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionBuild {
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: usize,
    /// For WASM: URL to the JSON metadata sidecar file
    #[serde(default)]
    pub json_url: Option<String>,
    /// For WASM: SHA256 of the JSON file
    #[serde(default)]
    pub json_sha256: Option<String>,
}

/// Extension requirements
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionRequirements {
    #[serde(default)]
    pub min_neomind_version: String,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

/// Extension safety limits
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionSafety {
    #[serde(default)]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub max_memory_mb: usize,
}

/// Response for listing marketplace extensions
#[derive(Debug, Serialize)]
pub struct MarketplaceListResponse {
    pub extensions: Vec<CloudExtension>,
    pub total: usize,
}

/// Request to install an extension from marketplace
#[derive(Debug, Deserialize)]
pub struct MarketplaceInstallRequest {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// Response for install operation
#[derive(Debug, Serialize)]
pub struct MarketplaceInstallResponse {
    pub success: bool,
    pub extension_id: String,
    pub downloaded: bool,
    pub installed: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// GET /api/extensions/market/list
///
/// List available extensions from the marketplace
pub async fn list_marketplace_extensions_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Use timestamp-only cache-busting to avoid CDN caching issues
    // without requiring version sync between repos
    let cache_buster = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let index_url = format!(
        "{}/{}/extensions/index.json?t={}",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        cache_buster
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = match client
        .get(&index_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to connect to marketplace: {}", e);
            return ok(json!({
                "extensions": [],
                "total": 0,
                "error": "network_error",
                "message": "Unable to connect to extension marketplace. Please check your internet connection."
            }));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        tracing::error!("Marketplace returned status {}", status);
        return ok(json!({
            "extensions": [],
            "total": 0,
            "error": format!("http_error_{}", status.as_u16()),
        }));
    }

    #[derive(Deserialize)]
    struct MarketIndex {
        version: String,
        extensions: Vec<CloudExtension>,
    }

    let index: MarketIndex = match response.json().await {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Failed to parse marketplace index: {}", e);
            return ok(json!({
                "extensions": [],
                "total": 0,
                "error": "parse_error",
            }));
        }
    };

    ok(json!({
        "extensions": index.extensions,
        "total": index.extensions.len(),
        "market_version": index.version,
    }))
}

/// GET /api/extensions/market/:id
///
/// Get detailed metadata for a specific extension from marketplace
pub async fn get_marketplace_extension_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<MarketplaceExtensionMetadata> {
    validate_extension_id(&id)?;
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = client
        .get(&metadata_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to fetch metadata: {}", e)))?;

    if !response.status().is_success() {
        return Err(ErrorResponse::not_found(format!(
            "Extension {} not found in marketplace",
            id
        )));
    }

    let metadata: MarketplaceExtensionMetadata = response
        .json()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to parse metadata: {}", e)))?;

    ok(metadata)
}

/// Response for the README fetch — `content` is `null` when the README
/// doesn't exist (README is optional, so a missing one is not an error).
#[derive(Debug, serde::Serialize)]
pub struct ExtensionReadmeResponse {
    pub content: Option<String>,
}

/// GET /api/extensions/market/:id/readme
///
/// Fetch the README.md content for a specific extension from the marketplace.
/// Returns `{ content: null }` when the README does not exist or the fetch
/// fails — README is optional, so this best-effort endpoint never reports a
/// hard error (the frontend just hides the README section).
pub async fn get_marketplace_extension_readme_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionReadmeResponse> {
    validate_extension_id(&id)?;
    let readme_url = format!(
        "{}/{}/extensions/{}/README.md",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = client
        .get(&readme_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await;

    // README missing (or transient fetch failure) is normal → content: null.
    let content = match response {
        Ok(r) if r.status().is_success() => r.text().await.ok(),
        _ => None,
    };

    ok(ExtensionReadmeResponse { content })
}

/// Detect current platform for extension download
/// Returns platform string in hyphen format (e.g., "darwin-aarch64")
/// This matches the format used in marketplace metadata `builds` keys
fn detect_platform() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-aarch64"
    }

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64")
    )))]
    {
        "unknown"
    }
}

/// Select the best marketplace `builds` key for the current platform + variant.
///
/// Candidate order: variant-specific key(s) → base platform → `wasm` (universal).
/// Returns the first key present in `available_keys`, or `None` if nothing matches.
///
/// Pure function (no I/O) so it can be unit-tested without network/state.
/// Caller (marketplace install) is responsible for the "unknown platform" guard
/// and for producing a user-facing error when this returns `None`.
fn select_build_key(
    available_keys: &std::collections::HashSet<&str>,
    base_platform: &str,
    variant: neomind_core::extension::accel::Variant,
) -> Option<String> {
    let mut candidates = neomind_core::extension::accel::fallback_keys(base_platform, variant);
    candidates.push("wasm".to_string());
    candidates
        .into_iter()
        .find(|k| available_keys.contains(k.as_str()))
}

/// Compute SHA256 checksum of file content
/// RAII guard that removes a path when dropped (best-effort). Used to make
/// sure a downloaded temp file is cleaned up on every exit path (success,
/// error, panic) without a manual `remove_file` at each return.
struct AutoRemove(std::path::PathBuf);
impl Drop for AutoRemove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Stream a reqwest response body to a temp file, enforcing a max size.
/// Returns the temp file path. Size is enforced via Content-Length (when
/// reported) AND a running byte counter during streaming (defends against
/// wrong/missing Content-Length headers). Never buffers the whole body in
/// memory — each chunk is written and dropped. Caller owns the returned
/// path and is responsible for removing it (typically via [`AutoRemove`]).
async fn download_to_temp_file(
    response: reqwest::Response,
    max_size: u64,
    label: &str,
) -> Result<std::path::PathBuf, String> {
    if let Some(len) = response.content_length() {
        if len > max_size {
            return Err(format!(
                "Package too large: {} bytes reported (max {} bytes)",
                len, max_size
            ));
        }
    }
    let tmp_path = std::env::temp_dir().join(format!(
        "neomind-{}-{}-{}.tmp",
        label,
        std::process::id(),
        tmp_counter()
    ));
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            // Clean up the partial temp file on stream error.
            let _ = std::fs::remove_file(&tmp_path);
            format!("Download stream error: {}", e)
        })?;
        total += chunk.len() as u64;
        if total > max_size {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(format!(
                "Package exceeded max size of {} bytes during download (got {})",
                max_size, total
            ));
        }
        file.write_all(&chunk).await.map_err(|e| {
            // Clean up the partial temp file on write failure (disk full,
            // quota, perms) — the chunk-error and oversize paths already
            // do this; without it a failed install leaks up to max_size bytes.
            let _ = std::fs::remove_file(&tmp_path);
            format!("Failed to write temp file: {}", e)
        })?;
    }
    file.flush().await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Failed to flush temp file: {}", e)
    })?;
    tracing::info!("Downloaded {} bytes to {}", total, tmp_path.display());
    Ok(tmp_path)
}

/// Monotonic counter for unique temp file names across concurrent installs.
fn tmp_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    C.fetch_add(1, Ordering::SeqCst)
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Stream-hash a file's contents for SHA256 verification without buffering
/// the whole file in memory. Used by the download path to verify large
/// binaries (ORT libs, model blobs) that were streamed to a temp file.
fn compute_sha256_of_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// POST /api/extensions/market/install
///
/// Download and install an extension from the marketplace
pub async fn install_marketplace_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<MarketplaceInstallRequest>,
) -> HandlerResult<MarketplaceInstallResponse> {
    let install_start = std::time::Instant::now();
    let runtime = &state.extensions.runtime;

    // Same id grammar as the local endpoints: req.id is interpolated into
    // a URL path — `../..` would turn the marketplace client into a
    // limited arbitrary-GET gadget against whatever host the (admin-set)
    // market base points at.
    validate_extension_id(&req.id)?;
    tracing::info!(extension_id = %req.id, "Starting marketplace extension install");

    // First fetch metadata to get download URL
    // Add cache-busting to avoid GitHub CDN serving stale content
    let cache_buster = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json?t={}",
        extension_market_base_url().as_str(),
        MARKET_BRANCH,
        req.id,
        cache_buster
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    // Fetch metadata
    let metadata: MarketplaceExtensionMetadata = match client
        .get(&metadata_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(m) => m,
            Err(e) => {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to parse metadata: {}", e)),
                });
            }
        },
        Ok(r) => {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Extension not found: {}", r.status())),
            });
        }
        Err(e) => {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Network error: {}", e)),
            });
        }
    };

    // Detect platform + hardware variant (jetson/cuda/cpu) so jetson/cuda
    // installs download the accelerated .nep build, not the plain CPU one.
    // Mirrors the legacy binary branch below (which already uses select_build_key);
    // the .nep branch previously skipped variant selection entirely.
    let platform = detect_platform();
    let variant = neomind_core::extension::accel::detect_variant();

    // Build platform-specific .nep package URL from builds metadata
    // The package_url field in metadata is hardcoded to darwin_aarch64, so we ignore it
    // and use the correct URL from builds for the current platform
    let (package_url, expected_sha256): (Option<String>, Option<String>) = if platform != "unknown"
    {
        let available_keys: std::collections::HashSet<&str> =
            metadata.builds.keys().map(|s| s.as_str()).collect();
        select_build_key(&available_keys, platform, variant)
            .and_then(|key| {
                metadata.builds.get(key.as_str()).map(|b| {
                    (
                        Some(b.url.clone()),
                        if b.sha256.is_empty() {
                            None
                        } else {
                            Some(b.sha256.clone())
                        },
                    )
                })
            })
            .unwrap_or((None, None))
    } else {
        (
            metadata.package_url.clone(),
            metadata.package_sha256.clone(),
        )
    };

    // Check if .nep package is available (preferred method)
    if let Some(ref package_url) = package_url {
        tracing::info!(
            "Downloading .nep package for extension {} from {}",
            req.id,
            package_url
        );

        // Download the .nep package
        let package_response = match client
            .get(package_url)
            .header("User-Agent", "NeoMind-Extension-Marketplace")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id.clone(),
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to download package: {}", e)),
                });
            }
        };

        if !package_response.status().is_success() {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!(
                    "Package download failed: {}",
                    package_response.status()
                )),
            });
        }

        // Stream the package body to a temp file (never buffer the whole
        // .nep in memory — large ML model bundles would OOM). The temp file
        // is cleaned up via _guard on every exit path.
        let tmp_path =
            match download_to_temp_file(package_response, MAX_EXTENSION_DOWNLOAD_SIZE, &req.id)
                .await
            {
                Ok(p) => p,
                Err(msg) => {
                    return ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: false,
                        installed: false,
                        path: None,
                        error: Some(msg),
                    });
                }
            };
        let _guard = AutoRemove(tmp_path.clone());

        // Verify it's a valid ZIP file by reading only the magic bytes from
        // the temp file (don't load the whole package to check 4 bytes).
        let mut magic_buf = [0u8; 4];
        if let Ok(mut magic_file) = tokio::fs::File::open(&tmp_path).await {
            // read_exact errors (file < 4 bytes) → magic_buf stays partial/zero → not a zip.
            let _ = magic_file.read_exact(&mut magic_buf).await;
        }
        let is_zip = magic_buf == [0x50, 0x4B, 0x03, 0x04]
            || magic_buf == [0x50, 0x4B, 0x05, 0x06]
            || magic_buf == [0x50, 0x4B, 0x07, 0x08];

        if !is_zip {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: false,
                installed: false,
                path: None,
                error: Some("Downloaded file is not a valid .nep package (ZIP format)".to_string()),
            });
        }

        // The marketplace index does not carry sha256 yet (the release
        // pipeline computes checksums but never publishes them), so the
        // verify branch below is effectively dead for today's metadata.
        // Surface that loudly instead of installing silently unverified, and
        // give strict deployments a fail-closed switch.
        let mut expected_sha256 = expected_sha256;
        if expected_sha256.is_none() {
            let filename = package_url.rsplit('/').next().unwrap_or("");
            if let Some(sha) = fetch_release_checksum(&client, package_url, filename).await {
                tracing::info!(
                    extension_id = %req.id,
                    "Package SHA256 sourced from the release checksums.txt"
                );
                expected_sha256 = Some(sha);
            }
        }
        if expected_sha256.is_none() {
            let strict = std::env::var("NEOMIND_STRICT_PACKAGE_SHA256")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if strict {
                let _ = std::fs::remove_file(&tmp_path);
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id.clone(),
                    downloaded: true,
                    installed: false,
                    path: None,
                    error: Some(
                        "Marketplace metadata carries no sha256 and NEOMIND_STRICT_PACKAGE_SHA256 is enabled — refusing to install an unverified package"
                            .to_string(),
                    ),
                });
            }
            tracing::warn!(
                extension_id = %req.id,
                url = %package_url,
                "Marketplace metadata carries no sha256 — integrity check skipped (set NEOMIND_STRICT_PACKAGE_SHA256=1 to refuse unverified packages)"
            );
        }

        // Verify SHA256 when the marketplace metadata provides one. Defends
        // against CDN/transport corruption or a swapped package being loaded
        // into the process via dlopen. The legacy binary branch already does
        // this; the .nep branch previously only checked the 4-byte ZIP magic.
        if let Some(ref expected) = expected_sha256 {
            match compute_sha256_of_file(&tmp_path) {
                Ok(actual) if actual == *expected => {
                    tracing::debug!(extension_id = %req.id, "Package SHA256 verified");
                }
                Ok(actual) => {
                    return ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: true,
                        installed: false,
                        path: None,
                        error: Some(format!(
                            "Package SHA256 mismatch: expected {}, got {} — refusing to install",
                            expected, actual
                        )),
                    });
                }
                Err(e) => {
                    // Fail CLOSED: the whole point of the pinned sha is that
                    // an unverifiable artifact never reaches dlopen. A read
                    // error means we cannot prove integrity — refuse.
                    tracing::error!(
                        extension_id = %req.id,
                        error = %e,
                        "Failed to compute package SHA256 — refusing to install"
                    );
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(ErrorResponse::internal(format!(
                        "Failed to verify package integrity (sha256 read error: {e}) — install aborted"
                    )));
                }
            }
        }

        // Prepare target directory
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        let target_dir = PathBuf::from(data_dir).join("extensions");

        // Install from the temp file — streams the ZIP from disk, not memory.
        // (install_from_file reads manifest + validates internally, so the old
        // from_bytes() pre-check is redundant and dropped.)
        let tmp_path_clone = tmp_path.clone();
        let target_dir_clone = target_dir.clone();
        let install_result = tokio::task::spawn_blocking(move || {
            use neomind_core::extension::package::ExtensionPackage;
            ExtensionPackage::install_from_file(&tmp_path_clone, &target_dir_clone)
        })
        .await;
        // _guard drops here on early return / end of scope → removes tmp_path.

        match install_result {
            Ok(Ok(result)) => {
                let ext_id = result.extension_id.clone();
                let version = result.version.clone();

                tracing::info!(
                    extension_id = %ext_id,
                    version = %version,
                    binary_path = %result.binary_path.display(),
                    elapsed_ms = install_start.elapsed().as_millis() as u64,
                    "Package downloaded and extracted, starting extension load"
                );

                // Check if already registered and unregister if needed
                let is_registered = runtime.contains(&ext_id).await;

                if is_registered {
                    tracing::info!("Extension {} already registered, will replace", ext_id);
                    if let Err(e) = runtime.unregister(&ext_id).await {
                        return ok(MarketplaceInstallResponse {
                            success: false,
                            extension_id: req.id.clone(),
                            downloaded: true,
                            installed: false,
                            path: Some(result.binary_path.to_string_lossy().to_string()),
                            error: Some(format!("Failed to unregister existing extension: {}", e)),
                        });
                    }
                }

                // Load and register the extension binary (unified handles isolated/in-process)
                match runtime.load(&result.binary_path).await {
                    Ok(ext_metadata) => {
                        // Determine extension type from binary path
                        let extension_type = result
                            .binary_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| if e == "wasm" { "wasm" } else { "native" })
                            .unwrap_or("native")
                            .to_string();

                        // Save to storage. An upgrade/reinstall overwrites the
                        // whole row — carry the previous user config (YOLO
                        // thresholds, bindings, …) into the new record so it
                        // survives, instead of resetting to defaults.
                        let store = state.extensions.store.clone();
                        let preserved_config = store
                            .load(&ext_id)
                            .ok()
                            .flatten()
                            .and_then(|old| old.config.clone());
                        {
                            let mut record = ExtensionRecord::new(
                                ext_id.clone(),
                                ext_metadata.name.clone(),
                                result.binary_path.to_string_lossy().to_string(),
                                extension_type,
                                version.clone(),
                            )
                            .with_description(ext_metadata.description.clone())
                            .with_author(ext_metadata.author.clone())
                            .with_checksum(Some(result.checksum.clone()))
                            .with_auto_start(true)
                            .with_frontend_path(
                                result
                                    .frontend_dir
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string()),
                            );
                            if let Some(cfg) = preserved_config.clone() {
                                record = record.with_config(cfg);
                            }

                            if let Err(e) = store.save(&record) {
                                tracing::warn!("Failed to save extension to storage: {}", e);
                            }
                        }

                        // Rebuild tool registry so the new extension's tools are visible to the LLM
                        state.refresh_extension_tools().await;

                        // Push the preserved config to the freshly-started
                        // runtime — without this the running process stays on
                        // defaults until the next restart (same gap the reload
                        // path closes with its send_config_update).
                        if let Some(cfg) = preserved_config {
                            if let Err(e) = state
                                .extensions
                                .runtime
                                .send_config_update(&ext_id, &cfg)
                                .await
                            {
                                tracing::warn!(
                                    extension_id = %ext_id,
                                    error = %e,
                                    "Failed to apply preserved config after market upgrade"
                                );
                            }
                        }

                        ok(MarketplaceInstallResponse {
                            success: true,
                            extension_id: ext_id,
                            downloaded: true,
                            installed: true,
                            path: Some(result.binary_path.to_string_lossy().to_string()),
                            error: None,
                        })
                    }
                    Err(e) => ok(MarketplaceInstallResponse {
                        success: false,
                        extension_id: req.id.clone(),
                        downloaded: true,
                        installed: false,
                        path: Some(result.binary_path.to_string_lossy().to_string()),
                        error: Some(format!("Failed to load extension binary: {}", e)),
                    }),
                }
            }
            Ok(Err(e)) => ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: true,
                installed: false,
                path: None,
                error: Some(format!("Package installation failed: {}", e)),
            }),
            Err(e) => ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id.clone(),
                downloaded: true,
                installed: false,
                path: None,
                error: Some(format!("Task join error: {}", e)),
            }),
        }
    } else {
        // Fall back to platform-specific binary download with variant fallback.
        // Variant chain (via accel): e.g. Jetson → linux-aarch64-jetson → linux-aarch64 → wasm.
        let variant = neomind_core::extension::accel::detect_variant();

        // "unknown" platform is only acceptable for pure-WASM extensions.
        if platform == "unknown" && !metadata.builds.contains_key("wasm") {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id,
                downloaded: false,
                installed: false,
                path: None,
                error: Some("Unsupported platform".to_string()),
            });
        }

        let available_keys: std::collections::HashSet<&str> =
            metadata.builds.keys().map(|s| s.as_str()).collect();
        let build_key = select_build_key(&available_keys, platform, variant).ok_or_else(|| {
            ErrorResponse::bad_request(format!(
                "No compatible build for {} (variant={:?}); available builds: {:?}",
                platform,
                variant,
                metadata.builds.keys().collect::<Vec<_>>()
            ))
        })?;

        // Get build info for the selected platform/variant/WASM.
        // SAFE: select_build_key only returns a key present in `available_keys`,
        // which we built from metadata.builds.keys() — so .get() is guaranteed Some.
        // (If select_build_key is ever refactored to emit keys NOT derived from
        // available_keys, this invariant breaks — keep it subset-bound.)
        let build = metadata
            .builds
            .get(build_key.as_str())
            .expect("select_build_key invariant: returned key must exist in metadata.builds");

        // Determine if this is a WASM build for file extension logic (used later)
        let is_wasm = build_key == "wasm";

        // Download the extension binary
        tracing::info!("Downloading extension {} from {}", req.id, build.url);

        let download_response = client
            .get(&build.url)
            .send()
            .await
            .map_err(|e| ErrorResponse::internal(format!("Download failed: {}", e)))?;

        if !download_response.status().is_success() {
            return ok(MarketplaceInstallResponse {
                success: false,
                extension_id: req.id,
                downloaded: false,
                installed: false,
                path: None,
                error: Some(format!("Download failed: {}", download_response.status())),
            });
        }

        // Stream the binary to a temp file (don't buffer large ORT/model
        // blobs in memory). _guard removes the temp file on every exit path.
        let tmp_path =
            download_to_temp_file(download_response, MAX_EXTENSION_DOWNLOAD_SIZE, &req.id)
                .await
                .map_err(ErrorResponse::internal)?;
        let _guard = AutoRemove(tmp_path.clone());

        // Verify SHA256 if provided — hash the temp file in a streaming fashion.
        if !build.sha256.is_empty() {
            let checksum = compute_sha256_of_file(&tmp_path).map_err(|e| {
                ErrorResponse::internal(format!("Failed to hash downloaded file: {}", e))
            })?;
            if checksum != build.sha256 {
                return ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: false,
                    installed: false,
                    path: None,
                    error: Some(format!(
                        "Checksum verification failed: expected {}, got {}",
                        build.sha256, checksum
                    )),
                });
            }
        }

        // Determine file extension and naming based on type
        let (ext, wasm_filename, json_filename) = if is_wasm {
            (
                ".wasm",
                format!("{}.wasm", req.id.replace("-", "_")),
                format!("{}.json", req.id.replace("-", "_")),
            )
        } else if platform.starts_with("darwin") {
            (".dylib", String::new(), String::new())
        } else if platform.starts_with("linux") {
            (".so", String::new(), String::new())
        } else if platform.starts_with("windows") {
            (".dll", String::new(), String::new())
        } else {
            ("", String::new(), String::new())
        };

        // Create extensions directory using NEOMIND_DATA_DIR for consistency
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        let extensions_dir = PathBuf::from(data_dir).join("extensions");

        std::fs::create_dir_all(&extensions_dir).map_err(|e| {
            ErrorResponse::internal(format!("Failed to create extensions directory: {}", e))
        })?;

        tracing::info!(
            extensions_dir = %extensions_dir.display(),
            "Installing extension from legacy binary format"
        );

        // Write the extension file(s)
        let (file_path, json_path) = if is_wasm {
            // WASM: write both .wasm and .json files
            let wasm_path = extensions_dir.join(&wasm_filename);

            std::fs::copy(&tmp_path, &wasm_path)
                .map_err(|e| ErrorResponse::internal(format!("Failed to copy WASM file: {}", e)))?;

            // Download and write JSON sidecar
            let json_path = extensions_dir.join(&json_filename);

            if let Some(json_url) = &build.json_url {
                let json_response =
                    client.get(json_url).send().await.map_err(|e| {
                        ErrorResponse::internal(format!("JSON download failed: {}", e))
                    })?;

                if json_response.status().is_success() {
                    let json_bytes = json_response.bytes().await.map_err(|e| {
                        ErrorResponse::internal(format!("Failed to read JSON: {}", e))
                    })?;

                    // Verify JSON SHA256 if provided
                    if let Some(ref expected_sha) = build.json_sha256 {
                        if !expected_sha.is_empty() {
                            let json_checksum = compute_sha256(&json_bytes);
                            if json_checksum != *expected_sha {
                                // Clean up on verification failure
                                let _ = std::fs::remove_file(&wasm_path);
                                return ok(MarketplaceInstallResponse {
                                    success: false,
                                    extension_id: req.id,
                                    downloaded: true,
                                    installed: false,
                                    path: None,
                                    error: Some("JSON checksum verification failed".to_string()),
                                });
                            }
                        }
                    }

                    std::fs::write(&json_path, &json_bytes).map_err(|e| {
                        ErrorResponse::internal(format!("Failed to write JSON file: {}", e))
                    })?;
                } else {
                    // Copy local JSON if download fails (fallback)
                    let local_json = format!(
                        "extensions/{}/{}.json",
                        req.id.replace("-", "_"),
                        req.id.replace("-", "_")
                    );
                    if PathBuf::from(&local_json).exists() {
                        let _ = std::fs::copy(&local_json, &json_path);
                    }
                }
            }

            tracing::info!(
                "WASM extension downloaded to: {:?} + {:?}",
                wasm_path,
                json_path
            );
            (wasm_path, Some(json_path))
        } else {
            // Native: write single binary file
            let filename = format!("libneomind_extension_{}{}", req.id, ext);
            let file_path = extensions_dir.join(&filename);

            std::fs::copy(&tmp_path, &file_path).map_err(|e| {
                ErrorResponse::internal(format!("Failed to copy extension file: {}", e))
            })?;

            tracing::info!("Extension downloaded to: {:?}", file_path);
            (file_path, None)
        };

        match runtime.load(&file_path).await {
            Ok(_) => {
                // Save to persistent storage
                let store = state.extensions.store.clone();
                {
                    let record = ExtensionRecord::new(
                        metadata.id.clone(),
                        metadata.name.clone(),
                        file_path.to_string_lossy().to_string(),
                        String::new(),
                        metadata.version.clone(),
                    )
                    .with_description(Some(metadata.description.clone()))
                    .with_author(Some(metadata.author.clone()))
                    .with_auto_start(true);

                    if let Err(e) = store.save(&record) {
                        tracing::warn!("Failed to save extension to storage: {}", e);
                    }
                }

                tracing::info!("Extension {} installed successfully", req.id);

                // Rebuild tool registry so the new extension's tools are visible to the LLM
                state.refresh_extension_tools().await;

                ok(MarketplaceInstallResponse {
                    success: true,
                    extension_id: req.id,
                    downloaded: true,
                    installed: true,
                    path: Some(file_path.to_string_lossy().to_string()),
                    error: None,
                })
            }
            Err(e) => {
                // Clean up the downloaded file(s) on failure
                let _ = std::fs::remove_file(&file_path);
                if let Some(ref jp) = json_path {
                    let _ = std::fs::remove_file(jp);
                }

                ok(MarketplaceInstallResponse {
                    success: false,
                    extension_id: req.id,
                    downloaded: true,
                    installed: false,
                    path: None,
                    error: Some(format!("Failed to load extension: {}", e)),
                })
            }
        }
    }
}

/// GET /api/extensions/market/updates
///
/// Check for updates for installed extensions
pub async fn check_marketplace_updates_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let installed = state.extensions.runtime.list().await;

    let mut updates = Vec::new();

    // For each installed extension, check if there's a newer version in marketplace
    for ext_info in installed {
        let ext_id = ext_info.metadata.id.clone();

        // Fetch metadata from marketplace
        let metadata_url = format!(
            "{}/{}/extensions/{}/metadata.json",
            extension_market_base_url().as_str(),
            MARKET_BRANCH,
            ext_id
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();

        if let Ok(client) = client {
            if let Ok(response) = client
                .get(&metadata_url)
                .header("User-Agent", "NeoMind-Extension-Marketplace")
                .send()
                .await
            {
                if response.status().is_success() {
                    if let Ok(metadata) = response.json::<MarketplaceExtensionMetadata>().await {
                        // Semver comparison, and only upgrades count. The old
                        // string `!=` flagged downgrades and build-suffix
                        // mismatches as "updates" — combined with extensions
                        // that hardcoded their version, some entries showed a
                        // permanent false "update available".
                        let newer = matches!(
                            (
                                metadata.version.parse::<semver::Version>(),
                                ext_info.metadata.version.parse::<semver::Version>(),
                            ),
                            (Ok(new), Ok(cur)) if new > cur
                        );
                        if newer {
                            updates.push(serde_json::json!({
                                "id": ext_id,
                                "name": ext_info.metadata.name,
                                "current_version": ext_info.metadata.version.to_string(),
                                "latest_version": metadata.version,
                                "categories": metadata.categories,
                            }));
                        }
                    }
                }
            }
        }
    }

    ok(json!({
        "updates_available": updates,
        "count": updates.len(),
    }))
}

// ============================================================================
// Extension Configuration API (V2)
// ============================================================================

/// GET /api/extensions/:id/config
///
/// Get the configuration schema and current values for an extension.
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

/// Size constraints for dashboard components.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SizeConstraints {
    pub min_w: u32,
    pub min_h: u32,
    pub default_w: u32,
    pub default_h: u32,
    pub max_w: u32,
    pub max_h: u32,
    pub preserve_aspect: Option<bool>,
}

/// Data binding configuration for dashboard components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBindingConfig {
    pub extension_metric: Option<String>,
    pub extension_command: Option<String>,
    pub required_fields: Vec<String>,
}

/// Dashboard component definition from manifest.
/// Uses String for category to be compatible with neomind-core's definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentDef {
    #[serde(rename = "type")]
    pub component_type: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: Option<String>,
    pub bundle_path: String,
    pub export_name: String,
    /// Global variable name for the bundle (used for script tag loading)
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub size_constraints: SizeConstraints,
    #[serde(default)]
    pub has_data_source: bool,
    #[serde(default)]
    pub has_display_config: bool,
    #[serde(default)]
    pub has_actions: bool,
    #[serde(default)]
    pub max_data_sources: u8,
    /// Whether this component supports device binding (receives deviceContext)
    #[serde(default)]
    pub has_device_binding: bool,
    pub config_schema: Option<serde_json::Value>,
    pub data_source_schema: Option<serde_json::Value>,
    pub default_config: Option<serde_json::Value>,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub data_binding: Option<DataBindingConfig>,
    /// Other fields that we don't parse (e.g., examples, etc.)
    #[serde(flatten)]
    pub _other: serde_json::Value,
}

/// Dashboard component DTO for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentDto {
    /// Component type identifier
    #[serde(rename = "type")]
    pub component_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Component category
    pub category: String,
    /// Icon name (lucide-react)
    pub icon: Option<String>,
    /// Bundle URL (resolved)
    pub bundle_url: String,
    /// Export name in bundle
    pub export_name: String,
    /// Global variable name for the bundle (used for script tag loading)
    pub global_name: Option<String>,
    /// Size constraints
    pub size_constraints: SizeConstraintsDto,
    /// Whether this component accepts a data source
    pub has_data_source: bool,
    /// Whether this component has display configuration
    pub has_display_config: bool,
    /// Whether this component has actions
    pub has_actions: bool,
    /// Maximum number of data sources
    pub max_data_sources: u8,
    /// JSON Schema for component configuration
    pub config_schema: Option<serde_json::Value>,
    /// JSON Schema for data source binding
    pub data_source_schema: Option<serde_json::Value>,
    /// Default configuration values
    pub default_config: Option<serde_json::Value>,
    /// Component variants
    pub variants: Vec<String>,
    /// Data binding configuration
    pub data_binding: DataBindingDto,
    /// Data source allowed types (e.g., ["device"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_allowed_types: Option<Vec<String>>,
    /// Whether this component supports device binding (receives deviceContext)
    #[serde(default)]
    pub has_device_binding: bool,
    /// Extension ID
    pub extension_id: String,
}

/// Size constraints DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeConstraintsDto {
    pub min_w: u32,
    pub min_h: u32,
    pub default_w: u32,
    pub default_h: u32,
    pub max_w: u32,
    pub max_h: u32,
    pub preserve_aspect: Option<bool>,
}

impl From<SizeConstraints> for SizeConstraintsDto {
    fn from(c: SizeConstraints) -> Self {
        Self {
            min_w: c.min_w,
            min_h: c.min_h,
            default_w: c.default_w,
            default_h: c.default_h,
            max_w: c.max_w,
            max_h: c.max_h,
            preserve_aspect: c.preserve_aspect,
        }
    }
}

/// Data binding DTO.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataBindingDto {
    pub extension_metric: Option<String>,
    pub extension_command: Option<String>,
    pub required_fields: Vec<String>,
}

impl From<DataBindingConfig> for DataBindingDto {
    fn from(c: DataBindingConfig) -> Self {
        Self {
            extension_metric: c.extension_metric,
            extension_command: c.extension_command,
            required_fields: c.required_fields,
        }
    }
}

/// Response for dashboard components list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentsResponse {
    /// Extension ID
    pub extension_id: String,
    /// Extension name
    pub extension_name: String,
    /// Dashboard components provided by this extension
    pub components: Vec<DashboardComponentDto>,
}

/// Extension manifest JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Frontend configuration with components
    #[serde(default)]
    pub frontend: Option<FrontendConfigDef>,
    /// Other fields that we don't parse
    #[serde(flatten)]
    pub _other: serde_json::Value,
}

/// Frontend configuration in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfigDef {
    /// Dashboard components provided by this extension
    #[serde(default)]
    pub components: Vec<DashboardComponentDef>,
}

/// GET /api/extensions/:id/components
/// Get dashboard components provided by an extension.
pub async fn get_extension_components_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<DashboardComponentsResponse> {
    // Check if extension exists using unified service
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Try to load manifest from extension directory
    let components = load_extension_components(&id, info.path.as_ref()).unwrap_or_default();

    let extension_name = info.metadata.name.clone();

    ok(DashboardComponentsResponse {
        extension_id: id,
        extension_name,
        components,
    })
}

/// Load dashboard components from extension manifest.
pub(crate) fn load_extension_components(
    // Log path configuration for debugging
    extension_id: &str,
    file_path: Option<&std::path::PathBuf>,
) -> Option<Vec<DashboardComponentDto>> {
    // Log path configuration for debugging
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    tracing::debug!(
        extension_id = %extension_id,
        data_dir = %data_dir,
        "Loading extension components (NEOMIND_DATA_DIR set)"
    );

    // If no file_path provided, try to find extension in data directory
    let file_path = if let Some(fp) = file_path {
        fp.clone()
    } else {
        // Try to find extension in data/extensions directory
        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();
        std::path::PathBuf::from(data_dir)
            .join("extensions")
            .join(extension_id)
    };

    tracing::debug!(
        extension_id = %extension_id,
        file_path = %file_path.display(),
        "Loading dashboard components for extension"
    );

    // Get the extension directory
    // For legacy format: file_path = extensions/xxx.wasm -> ext_dir = extensions/
    // For .nep format: file_path = extensions/xxx/binaries/wasm/extension.wasm -> ext_dir should be extensions/xxx/
    // Determine the extension directory
    // If file_path is a directory, use it directly
    // If file_path is a file, use its parent directory
    let ext_dir = if file_path.is_dir() {
        file_path.clone()
    } else {
        file_path.parent()?.to_path_buf()
    };

    tracing::debug!(ext_dir = %ext_dir.display(), "Extension directory");

    // Determine the extension root directory
    // Check if we're in a .nep format (contains "binaries" directory)
    let extension_root = {
        let components: Vec<_> = ext_dir.components().collect();
        let binaries_idx = components.iter().position(|c| {
            if let std::path::Component::Normal(os_str) = c {
                os_str.to_str().map(|s| s == "binaries").unwrap_or(false)
            } else {
                false
            }
        });

        if let Some(idx) = binaries_idx {
            // .nep format: go up to the directory containing "binaries"
            let root: std::path::PathBuf = components[..idx].iter().collect();
            tracing::debug!(root = %root.display(), "Detected .nep format");
            root
        } else {
            // Legacy format: use parent as-is
            tracing::debug!(root = %ext_dir.display(), "Detected legacy format");
            ext_dir.to_path_buf()
        }
    };

    tracing::debug!(extension_root = %extension_root.display(), "Extension root directory");

    // Try multiple manifest locations in order:
    // 1. extension_root/manifest.json (.nep format)
    // 2. ext_dir/manifest.json (legacy format, same dir as library)
    // 3. ext_dir/{extension_id}/manifest.json
    // 4. ext_dir/{extension_name}/manifest.json
    let extension_name = extension_id
        .strip_prefix("neomind.")
        .unwrap_or(extension_id)
        .replace('.', "-");

    let manifest_paths = vec![
        extension_root.join("manifest.json"),
        ext_dir.join("manifest.json"),
        ext_dir.join(extension_id).join("manifest.json"),
        ext_dir.join(&extension_name).join("manifest.json"),
    ];

    tracing::debug!(
        paths = ?manifest_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "Trying manifest paths"
    );

    // Try each manifest path
    let mut manifest_content = None;
    for manifest_path in &manifest_paths {
        if manifest_path.exists() {
            match std::fs::read_to_string(manifest_path) {
                Ok(content) => {
                    manifest_content = Some(content);
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        manifest_path = %manifest_path.display(),
                        error = %e,
                        "Failed to read manifest.json"
                    );
                }
            }
        }
    }

    let manifest_content = manifest_content?;

    tracing::debug!(
        extension_id = %extension_id,
        content_len = manifest_content.len(),
        "Found manifest.json"
    );

    // Parse manifest
    let manifest: ExtensionManifest = match serde_json::from_str(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                extension_id = %extension_id,
                error = %e,
                "Failed to parse manifest.json"
            );
            return None;
        }
    };

    tracing::debug!(
        extension_id = %extension_id,
        components_count = manifest.frontend.as_ref().map(|f| f.components.len()).unwrap_or(0),
        "Parsed manifest.json"
    );

    // Convert component definitions to DTOs
    let base_url = format!("/api/extensions/{}/assets", extension_id);

    // Get components from frontend.components
    let components: Vec<DashboardComponentDef> =
        manifest.frontend.map(|f| f.components).unwrap_or_default();

    let components: Vec<DashboardComponentDto> = components
        .into_iter()
        .map(|def| DashboardComponentDto {
            component_type: def.component_type,
            name: def.name,
            description: def.description,
            category: def.category,
            icon: def.icon,
            bundle_url: format!("{}/{}", base_url, def.bundle_path.trim_start_matches('/')),
            export_name: def.export_name,
            global_name: def.global_name,
            size_constraints: SizeConstraintsDto::from(def.size_constraints),
            has_data_source: def.has_data_source,
            has_display_config: def.has_display_config,
            has_actions: def.has_actions,
            max_data_sources: def.max_data_sources,
            config_schema: def.config_schema,
            data_source_schema: def.data_source_schema,
            default_config: def.default_config,
            variants: def.variants,
            data_binding: def
                .data_binding
                .map(DataBindingDto::from)
                .unwrap_or_default(),
            data_source_allowed_types: def
                ._other
                .get("dataSourceAllowedTypes")
                .or_else(|| def._other.get("data_source_allowed_types"))
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            has_device_binding: def.has_device_binding,
            extension_id: extension_id.to_string(),
        })
        .collect();

    Some(components)
}

/// GET /api/extensions/:id/assets/*
/// Serve static assets from extension directory.
pub async fn serve_extension_asset_handler(
    Path((id, asset_path)): Path<(String, String)>,
) -> Result<axum::response::Response, ErrorResponse> {
    use axum::body::Body;
    use axum::http::{header, StatusCode};

    // Prevent directory traversal AND absolute-path escape via asset_path.
    // `PathBuf::join` REPLACES the base when the argument is absolute — an
    // asset_path of "/etc/passwd" (no ".." anywhere) sailed through the old
    // check and served the file. Reject absolute paths, dot segments, and
    // verify the resolved file stays inside the extension dir.
    validate_extension_id(&id)?;
    if asset_path.contains("..")
        || asset_path.starts_with('/')
        || asset_path.starts_with('\\')
        || asset_path.split('/').any(|seg| seg == ".")
    {
        return Err(ErrorResponse::bad_request("Invalid asset path"));
    }

    // Extension directory is always data/extensions/{id}
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let ext_dir = std::path::PathBuf::from(data_dir)
        .join("extensions")
        .join(&id);

    let asset_file = ext_dir.join(&asset_path);

    // Check if file exists
    if !asset_file.exists() {
        return Err(ErrorResponse::not_found("Asset not found"));
    }
    // Belt-and-suspenders: the resolved file MUST live under the extension
    // dir (canonicalized comparison defeats every remaining join trick).
    if let (Ok(real_file), Ok(real_dir)) = (
        std::fs::canonicalize(&asset_file),
        std::fs::canonicalize(&ext_dir),
    ) {
        if !real_file.starts_with(&real_dir) {
            return Err(ErrorResponse::bad_request("Invalid asset path"));
        }
    }

    // Read file content
    let content = match std::fs::read(&asset_file) {
        Ok(c) => c,
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to read asset: {}",
                e
            )))
        }
    };

    // Determine content type based on file extension
    let mime_type = asset_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext {
            "js" | "cjs" | "mjs" => "application/javascript",
            "json" => "application/json",
            "css" => "text/css",
            "html" => "text/html",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            _ => "application/octet-stream",
        })
        .unwrap_or("application/octet-stream");

    // Build response. Use no-cache so the client always revalidates:
    // extension bundles change on every rebuild and a stale max-age cache
    // (the previous "public, max-age=3600") made iterating in dev painful —
    // Tauri WKWebView would serve a 1-hour-stale bundle even after we just
    // dropped a new one in place. Bundle is small (tens of KB); re-fetch on
    // each navigation is negligible.
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(content))
        .map_err(|e| ErrorResponse::internal(format!("Failed to build response: {}", e)))
}

/// GET /api/extensions/dashboard-components
/// Get all dashboard components from all registered extensions.
///
/// This endpoint only returns components from extensions that are currently registered.
/// When an extension is unregistered, its components will no longer appear.
pub async fn get_all_dashboard_components_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<DashboardComponentDto>> {
    let mut all_components = Vec::new();

    // Load components only from registered extensions
    let all_extensions = state.extensions.runtime.list().await;
    for info in all_extensions {
        if let Some(components) = load_extension_components(&info.metadata.id, info.path.as_ref()) {
            all_components.extend(components);
        }
    }

    ok(all_components)
}

/// POST /api/extensions/upload
/// Upload and install an extension package (.nep file).
/// Note: This endpoint requires the .nep file to be manually uploaded to the data directory first.
/// POST body: { "file_path": "/path/to/package.nep" }
pub async fn upload_extension_package_handler(
    State(state): State<ServerState>,
    Json(req): Json<UploadPackageRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let file_path = resolve_confined_package_path(&req.file_path)?;

    // Load the package
    let package = ExtensionPackage::load(&file_path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid package: {}", e)))?;

    let ext_id = package.manifest.id.clone();
    let version = package.manifest.version.clone();
    let name = package.manifest.name.clone();

    tracing::info!(
        extension_id = %ext_id,
        version = %version,
        name = %name,
        checksum = %package.checksum,
        size = package.size,
        "Processing extension package upload"
    );

    // Check if extension is already registered
    let runtime = &state.extensions.runtime;
    let is_registered = runtime.contains(&ext_id).await;

    if is_registered {
        // Unregister existing version first
        tracing::info!("Extension {} already registered, will replace", ext_id);
        runtime.unregister(&ext_id).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to unregister existing: {}", e))
        })?;
    }

    // Install the package
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let target_dir = PathBuf::from(data_dir).join("extensions");

    let install_result = package
        .install(&target_dir)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Installation failed: {}", e)))?;

    tracing::info!(
        extension_id = %install_result.extension_id,
        binary_path = %install_result.binary_path.display(),
        manifest_path = %install_result.manifest_path.display(),
        frontend_dir = ?install_result.frontend_dir,
        components_count = install_result.components.len(),
        "Package installed successfully"
    );

    // Load and register the extension binary (handles both isolated and in-process)
    let _metadata = runtime
        .load(&install_result.binary_path)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to load extension binary: {}", e)))?;

    // Save to storage
    let store = state.extensions.store.clone();
    {
        let record = ExtensionRecord::new(
            ext_id.clone(),
            name.clone(),
            install_result.binary_path.to_string_lossy().to_string(),
            package.manifest.extension_type.clone(),
            version.clone(),
        )
        .with_description(package.manifest.description.clone())
        .with_author(package.manifest.author.clone())
        .with_checksum(Some(install_result.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            install_result
                .frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    // Build response
    ok(serde_json::json!({
        "message": "Extension package installed successfully",
        "extension_id": ext_id,
        "name": name,
        "version": version,
        "description": package.manifest.description,
        "author": package.manifest.author,
        "checksum": install_result.checksum,
        "binary_path": install_result.binary_path.to_string_lossy(),
        "manifest_path": install_result.manifest_path.to_string_lossy(),
        "frontend_dir": install_result.frontend_dir.as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        "components_count": install_result.components.len(),
        "components": install_result.components.iter().map(|c| json!({
            "type": c.component_type,
            "name": c.name,
            "description": c.description,
            "category": c.category
        })).collect::<Vec<_>>(),
        "replaced": is_registered
    }))
}

/// POST /api/extensions/package/validate
/// Validate an extension package without installing.
pub async fn validate_extension_package_handler(
    Json(req): Json<ValidatePackageRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let file_path = resolve_confined_package_path(&req.file_path)?;

    let package = ExtensionPackage::load(&file_path)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid package: {}", e)))?;

    let platform = detect_platform();
    let has_binary = package.get_binary_path().is_some();
    let has_frontend = package.manifest.frontend.is_some();
    let components_count = package
        .manifest
        .frontend
        .as_ref()
        .map(|f| f.components.len())
        .unwrap_or(0);

    ok(serde_json::json!({
        "valid": true,
        "format": package.manifest.format,
        "format_version": package.manifest.format_version,
        "extension_id": package.manifest.id,
        "name": package.manifest.name,
        "version": package.manifest.version,
        "description": package.manifest.description,
        "author": package.manifest.author,
        "license": package.manifest.license,
        "current_platform": platform,
        "has_binary_for_platform": has_binary,
        "has_frontend": has_frontend,
        "components_count": components_count,
        "capabilities": package.manifest.capabilities,
        "permissions": package.manifest.permissions,
        "checksum": package.checksum,
        "size": package.size
    }))
}

/// Upload package request
#[derive(Debug, Deserialize)]
pub struct UploadPackageRequest {
    pub file_path: String,
}

/// Validate package request
#[derive(Debug, Deserialize)]
pub struct ValidatePackageRequest {
    pub file_path: String,
}

/// DELETE /api/extensions/:id/uninstall
/// Completely uninstall an extension (remove all files).
pub async fn uninstall_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Prevent path traversal via id (used in remove_dir_all below)
    validate_extension_id(&id)?;

    let runtime = &state.extensions.runtime;

    // Check if extension exists
    let exists = runtime.contains(&id).await;
    let ext_info = if exists { runtime.get(&id).await } else { None };

    // Unregister from memory
    if exists {
        runtime
            .unregister(&id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to unregister: {}", e)))?;
    }

    // Mark as uninstalled in storage
    let store = state.extensions.store.clone();
    {
        if let Err(e) = store.mark_uninstalled(&id) {
            tracing::warn!("Failed to mark extension as uninstalled: {}", e);
        }
    }

    // Clean up extension directory
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let extensions_dir = PathBuf::from(data_dir).join("extensions");
    let ext_dir = extensions_dir.join(&id);

    let mut removed_files = Vec::new();
    if ext_dir.exists() {
        tracing::info!(
            "Removing extension directory (data/ preserved): {}",
            ext_dir.display()
        );
        // Preserve the platform-guaranteed private `data/` subdir — it holds
        // user state (pipelines, face libraries, licenses) that must survive
        // uninstall; everything else (package files) goes.
        let preserved_data_dir = ext_dir.join("data");
        let mut entries = tokio::fs::read_dir(&ext_dir).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to read extension directory: {}", e))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to read extension directory: {}", e))
        })? {
            let path = entry.path();
            if path == preserved_data_dir {
                continue;
            }
            if path.is_dir() {
                tokio::fs::remove_dir_all(&path).await.map_err(|e| {
                    ErrorResponse::internal(format!("Failed to remove extension directory: {}", e))
                })?;
            } else {
                tokio::fs::remove_file(&path).await.map_err(|e| {
                    ErrorResponse::internal(format!("Failed to remove extension file: {}", e))
                })?;
            }
        }
        removed_files.push(ext_dir.to_string_lossy().to_string());
    }

    // Clean up extension metrics
    cleanup_extension_metrics(&state, &id).await;

    // Rebuild tool registry to remove the uninstalled extension's tools
    state.refresh_extension_tools().await;

    ok(serde_json::json!({
        "message": "Extension uninstalled completely",
        "extension_id": id,
        "name": ext_info.map(|info| info.metadata.name),
        "removed_files": removed_files,
        "note": "All extension files, including frontend components, have been removed"
    }))
}

/// POST /api/extensions/upload/file
/// Upload an extension package file directly (.nep format).
///
/// This endpoint accepts a JSON body with base64-encoded file data.
///
/// Request body:
/// ```json
/// {
///   "data": "<base64-encoded .nep file>",
///   "filename": "extension.nep"
/// }
/// ```
///
/// Example with curl:
/// ```bash
/// # First encode the file to base64
/// BASE64_DATA=$(base64 -w 0 extension.nep)
/// curl -X POST http://localhost:9375/api/extensions/upload/file \
///   -H "Content-Type: application/json" \
///   -d "{\"data\": \"$BASE64_DATA\"}"
/// ```
#[derive(Debug, serde::Deserialize)]
pub struct UploadExtensionFileRequest {
    /// Base64-encoded .nep file data
    pub data: String,
    /// Optional filename
    pub filename: Option<String>,
}

#[axum::debug_handler]
pub async fn upload_extension_file_handler(
    State(state): State<ServerState>,
    Json(req): Json<UploadExtensionFileRequest>,
) -> HandlerResult<serde_json::Value> {
    // Log upload request details
    let data_len = req.data.len();
    let filename = req.filename.as_deref().unwrap_or("unknown");
    tracing::info!(
        "Extension upload request received: filename={}, base64_size={}MB",
        filename,
        data_len / 1_000_000
    );

    // Decode base64 data
    let body_bytes = STANDARD
        .decode(&req.data)
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid base64 data: {}", e)))?;

    tracing::info!(
        "Base64 decoded successfully: binary_size={}MB",
        body_bytes.len() / 1_000_000
    );

    // Check if this looks like a ZIP file
    if body_bytes.len() < 4 {
        return Err(ErrorResponse::bad_request(
            "File too small to be a valid package",
        ));
    }

    let zip_magic: &[u8] = &[0x50, 0x4B, 0x03, 0x04];
    let zip_empty: &[u8] = &[0x50, 0x4B, 0x05, 0x06];
    let zip_spanned: &[u8] = &[0x50, 0x4B, 0x07, 0x08];

    let is_zip = body_bytes.starts_with(zip_magic)
        || body_bytes.starts_with(zip_empty)
        || body_bytes.starts_with(zip_spanned);

    if !is_zip {
        return Err(ErrorResponse::bad_request(
            "File is not a valid ZIP archive",
        ));
    }

    // Prepare target directory
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let target_dir = PathBuf::from(data_dir).join("extensions");

    // Step 1: Parse the package to get extension ID (validation only, no install yet)
    // This allows us to check if the extension is registered BEFORE overwriting files
    let body_bytes_for_validate = body_bytes.clone();
    let (ext_id, version) = tokio::task::spawn_blocking(move || {
        use neomind_core::extension::package::ExtensionPackage;
        let package = ExtensionPackage::from_bytes(body_bytes_for_validate)
            .map_err(|e| format!("Package parse error: {}", e))?;
        Ok::<_, String>((
            package.manifest.id.clone(),
            package.manifest.version.clone(),
        ))
    })
    .await
    .map_err(|e| ErrorResponse::internal(format!("Task join error: {}", e)))?
    .map_err(|e| ErrorResponse::internal(format!("Package validation failed: {}", e)))?;

    tracing::info!(
        extension_id = %ext_id,
        version = %version,
        "Package validated successfully"
    );

    // Step 2: Check if already registered and unload FIRST (before overwriting files)
    // This is critical on macOS where overwriting a dylib that's in use can cause issues
    let runtime = state.extensions.runtime.clone();
    let is_registered = runtime.contains(&ext_id).await;

    if is_registered {
        tracing::info!(
            "Extension {} already registered, unloading before update",
            ext_id
        );
        runtime.unload(&ext_id).await.map_err(|e| {
            ErrorResponse::internal(format!("Failed to unload existing extension: {}", e))
        })?;
        // Wait a moment for the process to fully terminate and release file handles
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Step 3: Now install the package (safe to overwrite files)
    // body_bytes is no longer needed after this point, move it directly
    let target_dir_clone = target_dir.clone();
    let install_result = tokio::task::spawn_blocking(move || {
        use neomind_core::extension::package::ExtensionPackage;
        ExtensionPackage::install_sync(&body_bytes, &target_dir_clone)
    })
    .await
    .map_err(|e| {
        tracing::error!("install_sync task join error for {}: {}", ext_id, e);
        ErrorResponse::internal(format!("Task join error: {}", e))
    })?
    .map_err(|e| {
        tracing::error!("install_sync failed for {}: {} (kind={:?})", ext_id, e, e);
        ErrorResponse::internal(format!("Installation failed: {}", e))
    })?;

    tracing::info!(
        extension_id = %install_result.extension_id,
        binary_path = %install_result.binary_path.display(),
        frontend_dir = ?install_result.frontend_dir,
        components_count = install_result.components.len(),
        "Package installed successfully"
    );

    // Step 4: Load and register the extension binary with process isolation
    let metadata = runtime
        .load(&install_result.binary_path)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to load extension binary: {}", e)))?;

    // Determine extension type from binary path
    let extension_type = install_result
        .binary_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| if e == "wasm" { "wasm" } else { "native" })
        .unwrap_or("native")
        .to_string();

    // Save to storage
    let store = state.extensions.store.clone();
    {
        let record = ExtensionRecord::new(
            ext_id.clone(),
            metadata.name.clone(),
            install_result.binary_path.to_string_lossy().to_string(),
            extension_type,
            version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_checksum(Some(install_result.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            install_result
                .frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );

        if let Err(e) = store.save(&record) {
            tracing::warn!("Failed to save extension to storage: {}", e);
        }
    }

    // Rebuild tool registry so the new extension's tools are visible to the LLM
    state.refresh_extension_tools().await;

    // Build response
    ok(serde_json::json!({
        "message": "Extension package installed successfully",
        "extension_id": ext_id,
        "name": metadata.name,
        "version": version,
        "description": metadata.description,
        "author": metadata.author,
        "checksum": install_result.checksum,
        "binary_path": install_result.binary_path.to_string_lossy(),
        "manifest_path": install_result.manifest_path.to_string_lossy(),
        "frontend_dir": install_result.frontend_dir.as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        "components_count": install_result.components.len(),
        "components": install_result.components.iter().map(|c| json!({
            "type": c.component_type,
            "name": c.name,
            "description": c.description,
            "category": c.category
        })).collect::<Vec<_>>(),
        "replaced": is_registered
    }))
}

/// POST /api/extensions/sync
///
/// Manually trigger extension synchronization from /extensions/ directory.
/// Register an on-disk-installed extension with the runtime: unregister the
/// old instance if any, load the binary, and upsert the ExtensionRecord
/// carrying the previous user config forward (same contract as the
/// marketplace install path). Shared by the sync handler and the startup
/// cache scan so they can't drift apart again.
pub(crate) async fn register_installed_package(
    state: &ServerState,
    pkg: &crate::server::install_service::InstalledPackage,
) -> Result<(), String> {
    let runtime = state.extensions.runtime.clone();

    // Replace a registered instance (mirrors the marketplace flow). An
    // unregister failure is fatal there; here we propagate the error too —
    // a half-replaced extension is worse than a reported failure.
    if runtime.contains(&pkg.extension_id).await {
        runtime
            .unregister(&pkg.extension_id)
            .await
            .map_err(|e| format!("failed to unregister old instance: {e}"))?;
    }

    let metadata = runtime
        .load(&pkg.binary_path)
        .await
        .map_err(|e| format!("failed to load extension: {e}"))?;

    let extension_type = pkg
        .binary_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| if e == "wasm" { "wasm" } else { "native" })
        .unwrap_or("native")
        .to_string();

    let store = state.extensions.store.clone();
    let preserved_config = store
        .load(&pkg.extension_id)
        .ok()
        .flatten()
        .and_then(|old| old.config.clone());
    {
        let mut record = ExtensionRecord::new(
            pkg.extension_id.clone(),
            metadata.name.clone(),
            pkg.binary_path.to_string_lossy().to_string(),
            extension_type,
            pkg.version.clone(),
        )
        .with_description(metadata.description.clone())
        .with_author(metadata.author.clone())
        .with_checksum(Some(pkg.checksum.clone()))
        .with_auto_start(true)
        .with_frontend_path(
            pkg.frontend_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        );
        if let Some(cfg) = preserved_config.clone() {
            record = record.with_config(cfg);
        }
        if let Err(e) = store.save(&record) {
            tracing::warn!(
                extension_id = %pkg.extension_id,
                error = %e,
                "Failed to save extension record after sync install"
            );
        }
    }

    state.refresh_extension_tools().await;

    if let Some(cfg) = preserved_config {
        if let Err(e) = runtime.send_config_update(&pkg.extension_id, &cfg).await {
            tracing::warn!(
                extension_id = %pkg.extension_id,
                error = %e,
                "Failed to apply preserved config after sync install"
            );
        }
    }

    Ok(())
}

pub async fn sync_extensions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use crate::server::ExtensionInstallService;

    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let extensions_dir = std::path::PathBuf::from(&data_dir).join("extensions");
    // The cache IS the extensions dir: the marketplace download path already
    // drops .nep files there. (This used to scan a CWD-relative "extensions/"
    // that had nothing to do with the data dir.)
    let install_service = ExtensionInstallService::new(&extensions_dir, &extensions_dir);

    let report = install_service
        .sync_nep_cache()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Sync failed: {}", e)))?;

    // Disk install is only half the job — register what changed. This sync
    // used to report "installed: N" while doing neither half.
    let mut registered = 0usize;
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for pkg in &report.installed_packages {
        match register_installed_package(&state, pkg).await {
            Ok(()) => registered += 1,
            Err(e) => errors.push(serde_json::json!({
                "extension_id": pkg.extension_id,
                "error": e,
            })),
        }
    }

    ok(serde_json::json!({
        "message": "Extensions synchronized",
        "scanned": report.scanned,
        "installed": report.installed,
        "upgraded": report.upgraded,
        "skipped": report.skipped,
        "failed": report.failed,
        "registered": registered,
        "errors": errors,
    }))
}

/// GET /api/extensions/sync-status
pub async fn get_sync_status_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use neomind_core::extension::package::ExtensionPackage;

    let nep_cache_dir = std::path::PathBuf::from("extensions");
    let data_dir = neomind_core::paths::data_dir()
        .to_string_lossy()
        .to_string();
    let install_dir = std::path::PathBuf::from(data_dir).join("extensions");

    let mut nep_packages = Vec::new();

    if nep_cache_dir.exists() {
        if let Ok(mut entries) = std::fs::read_dir(&nep_cache_dir) {
            while let Some(Ok(entry)) = entries.next() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("nep") {
                    continue;
                }

                let package_info = match ExtensionPackage::load(&path).await {
                    Ok(package) => {
                        let ext_id = package.manifest.id.clone();
                        serde_json::json!({
                            "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                            "extension_id": ext_id,
                            "version": package.manifest.version,
                            "name": package.manifest.name,
                            "installed": install_dir.join(&ext_id).exists(),
                            "size": path.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                    }
                    Err(_) => {
                        serde_json::json!({
                            "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                            "error": "Failed to load package",
                        })
                    }
                };
                nep_packages.push(package_info);
            }
        }
    }

    ok(serde_json::json!({
        "nep_packages": nep_packages,
        "nep_cache_dir": nep_cache_dir.to_string_lossy().to_string(),
        "install_dir": install_dir.to_string_lossy().to_string(),
    }))
}

#[cfg(test)]
mod select_build_key_tests {
    use super::select_build_key;
    use neomind_core::extension::accel::Variant;
    use std::collections::HashSet;

    fn keys(v: &[&'static str]) -> HashSet<&'static str> {
        v.iter().copied().collect()
    }

    #[test]
    fn jetson_picks_jetson_build_when_present() {
        let avail = keys(&["linux-aarch64-jetson", "linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64-jetson".to_string())
        );
    }

    #[test]
    fn jetson_falls_back_to_plain_when_no_jetson_build() {
        let avail = keys(&["linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn cpu_picks_plain_build() {
        let avail = keys(&["linux-aarch64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Cpu),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn native_preferred_over_wasm_when_both_present() {
        let avail = keys(&["linux-aarch64", "wasm"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            Some("linux-aarch64".to_string())
        );
    }

    #[test]
    fn wasm_picked_for_pure_wasm_extension() {
        let avail = keys(&["wasm"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Cpu),
            Some("wasm".to_string())
        );
    }

    #[test]
    fn no_match_returns_none() {
        let avail = keys(&["windows-x86_64"]);
        assert_eq!(
            select_build_key(&avail, "linux-aarch64", Variant::Jetson),
            None
        );
    }
}
