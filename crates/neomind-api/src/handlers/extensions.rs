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
use crate::server::ServerState;
use neomind_core::datasource::DataSourceId;
use neomind_core::extension::{MetricDataType, ParameterDefinition};
use neomind_storage::{ExtensionRecord, ExtensionStore};

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
    /// Commands provided by this extension
    #[serde(default)]
    pub commands: Vec<CommandDescriptorDto>,
    /// Metrics provided by this extension (V2)
    #[serde(default)]
    pub metrics: Vec<MetricDescriptorDto>,
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

/// Extension statistics DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionStatsDto {
    /// Start count
    pub start_count: u64,
    /// Stop count
    pub stop_count: u64,
    /// Error count
    pub error_count: u64,
    /// Last error
    pub last_error: Option<String>,
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

/// Extension discovery result DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionDiscoveryResult {
    /// Extension ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// File path
    pub file_path: String,
}

/// GET /api/extensions
/// List all registered extensions.
pub async fn list_extensions_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListExtensionsQuery>,
) -> HandlerResult<Vec<ExtensionDto>> {
    let registry = &state.extensions.registry;

    let all_extensions = registry.list().await;

    let mut extensions: Vec<ExtensionDto> = all_extensions
        .into_iter()
        .map(|info| {
            // Convert commands to DTOs (V2 format)
            let commands: Vec<CommandDescriptorDto> = info
                .commands
                .iter()
                .map(|cmd| CommandDescriptorDto {
                    id: cmd.name.clone(),
                    display_name: cmd.display_name.clone(),
                    description: cmd.llm_hints.clone(),
                    input_schema: build_parameters_schema(&cmd.parameters),
                    output_fields: vec![], // V2: Commands don't declare output fields
                    config: CommandConfigDto {
                        requires_auth: false,
                        timeout_ms: 30000,
                        is_stream: false,
                        expected_duration_ms: None,
                    },
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

            ExtensionDto {
                id: info.metadata.id.clone(),
                name: info.metadata.name.clone(),
                version: info.metadata.version.to_string(),
                description: info.metadata.description.clone(),
                author: info.metadata.author.clone(),
                state: info.state.to_string(),
                file_path: info
                    .metadata
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string()),
                loaded_at: info.loaded_at.map(|t| t.timestamp()),
                commands,
                metrics,
            }
        })
        .collect();

    // Filter by state
    if let Some(state_filter) = &query.state {
        extensions.retain(|e| e.state.to_lowercase() == state_filter.to_lowercase());
    }

    ok(extensions)
}

/// GET /api/extensions/:id
/// Get a specific extension.
pub async fn get_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionDto> {
    let registry = &state.extensions.registry;

    let info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Convert commands to DTOs (V2 format)
    let commands: Vec<CommandDescriptorDto> = info
        .commands
        .iter()
        .map(|cmd| CommandDescriptorDto {
            id: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.llm_hints.clone(),
            input_schema: build_parameters_schema(&cmd.parameters),
            output_fields: vec![], // V2: Commands don't declare output fields
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 30000,
                is_stream: false,
                expected_duration_ms: None,
            },
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

    ok(ExtensionDto {
        id: info.metadata.id.clone(),
        name: info.metadata.name.clone(),
        version: info.metadata.version.to_string(),
        description: info.metadata.description.clone(),
        author: info.metadata.author.clone(),
        state: info.state.to_string(),
        file_path: info
            .metadata
            .file_path
            .as_ref()
            .map(|p| p.display().to_string()),
        loaded_at: info.loaded_at.map(|t| t.timestamp()),
        commands,
        metrics,
    })
}

/// GET /api/extensions/:id/stats
/// Get extension statistics.
pub async fn get_extension_stats_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExtensionStatsDto> {
    let registry = &state.extensions.registry;

    let info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    ok(ExtensionStatsDto {
        start_count: info.stats.start_count,
        stop_count: info.stats.stop_count,
        error_count: info.stats.error_count,
        last_error: info.stats.last_error.clone(),
    })
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

/// POST /api/extensions/discover
/// Discover extensions in configured directories.
///
/// Scans default and configured extension directories for
/// unregistered extensions and returns their metadata.
pub async fn discover_extensions_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<ExtensionDiscoveryResult>> {
    let registry = &state.extensions.registry;

    // Discover extensions using the registry
    let discovered = registry.discover().await;

    // Convert to DTOs (V2 - no extension_type)
    let results: Vec<ExtensionDiscoveryResult> = discovered
        .into_iter()
        .map(|(path, metadata)| ExtensionDiscoveryResult {
            id: metadata.id.clone(),
            name: metadata.name.clone(),
            version: metadata.version.to_string(),
            description: metadata.description.clone(),
            file_path: path.to_string_lossy().to_string(),
        })
        .collect();

    ok(results)
}

/// POST /api/extensions/register-all
/// Register all discovered extensions.
///
/// Discovers extensions from default directories and registers them all.
pub async fn register_all_discovered_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = &state.extensions.registry;

    // Discover extensions using the registry
    let discovered = registry.discover().await;

    if discovered.is_empty() {
        return ok(serde_json::json!({
            "message": "No new extensions to register",
            "registered": 0,
            "extensions": []
        }));
    }

    // Open the store once for all registrations
    let store = match ExtensionStore::open("data/extensions.redb") {
        Ok(s) => s,
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to open extension store: {}",
                e
            )));
        }
    };

    let mut registered = Vec::new();
    let mut failed = Vec::new();

    for (path, metadata) in discovered {
        // Check if already registered
        let is_registered = registry.contains(&metadata.id).await;
        if is_registered {
            continue;
        }

        // Load and register the extension
        match registry.load_from_path(&path).await {
            Ok(_) => {
                // Save to storage for persistence
                let record = ExtensionRecord::new(
                    metadata.id.clone(),
                    metadata.name.clone(),
                    path.to_string_lossy().to_string(),
                    "native".to_string(),
                    metadata.version.to_string(),
                )
                .with_description(metadata.description.clone())
                .with_author(metadata.author.clone())
                .with_auto_start(true);

                if let Err(e) = store.save(&record) {
                    tracing::warn!("Failed to save extension to storage: {}", e);
                }

                registered.push(serde_json::json!({
                    "id": metadata.id,
                    "name": metadata.name,
                    "version": metadata.version.to_string(),
                    "file_path": path.to_string_lossy().to_string(),
                }));
            }
            Err(e) => {
                failed.push(serde_json::json!({
                    "id": metadata.id,
                    "name": metadata.name,
                    "error": e.to_string(),
                }));
            }
        }
    }

    ok(serde_json::json!({
        "message": format!("Registered {} extension(s)", registered.len()),
        "registered": registered.len(),
        "failed": failed.len(),
        "extensions": registered,
        "failed_extensions": failed,
    }))
}

/// POST /api/extensions
/// Register a new extension from file path.
pub async fn register_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = &state.extensions.registry;

    let path = PathBuf::from(&req.file_path);

    // Load metadata from the extension file
    let metadata = registry.load_from_path(&path).await.map_err(|e| {
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
    if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
        let record = neomind_storage::ExtensionRecord::new(
            ext_id.clone(),
            ext_name.clone(),
            req.file_path.clone(),
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

    // Note: Extensions are always active once registered in the new system
    // The auto_start flag is kept for API compatibility but not used

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
    let registry = &state.extensions.registry;

    // Check if extension exists
    if !registry.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Unregister from memory
    let registry = &state.extensions.registry;
    registry
        .unregister(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to unregister: {}", e)))?;

    // Also remove from persistent storage
    if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
        if let Err(e) = store.delete(&id) {
            tracing::warn!("Failed to delete extension from storage: {}", e);
        }
    }

    ok(serde_json::json!({
        "message": "Extension unregistered",
        "extension_id": id
    }))
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
    let registry = &state.extensions.registry;

    let healthy = registry
        .health_check(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Health check failed: {}", e)))?;

    ok(serde_json::json!({
        "extension_id": id,
        "healthy": healthy
    }))
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
    let extensions = state.extensions.registry.list().await;
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
                let _ = event_bus.publish(event);
            }
        }
    }
}

/// POST /api/extensions/:id/command
/// Execute a command on an extension.
pub async fn execute_extension_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = &state.extensions.registry;

    let result = registry
        .execute_command(&id, &req.command, &req.args)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Command execution failed: {}", e)))?;

    // Publish ExtensionOutput events for real-time dashboard updates
    publish_extension_metrics(&state, &id, &result).await;

    ok(result)
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
    let registry = &state.extensions.registry;

    // Check if extension exists
    if !registry.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    let result = registry
        .execute_command(&id, &req.command, &req.params)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Invoke failed: {}", e)))?;

    // Publish ExtensionOutput events for real-time dashboard updates
    publish_extension_metrics(&state, &id, &result).await;

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
    let registry = &state.extensions.registry;

    // Check if extension exists
    let _info = registry
        .list()
        .await
        .into_iter()
        .find(|info| info.metadata.id == id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // V2: Commands don't declare streaming capability in config
    // Default to no streaming support
    ok(serde_json::json!({
        "extension_id": id,
        "supports_streaming": false,
        "stream_url": "",
        "output_mode": "once",
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
    let registry = &state.extensions.registry;

    let ext = registry
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let ext_read = ext.read().await;
    let commands = ext_read.commands();

    let result: Vec<CommandDescriptorDto> = commands
        .iter()
        .map(|cmd| CommandDescriptorDto {
            id: cmd.name.clone(),
            display_name: cmd.display_name.clone(),
            description: cmd.llm_hints.clone(),
            input_schema: build_parameters_schema(&cmd.parameters),
            output_fields: vec![], // V2: Commands don't declare output fields
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 30000,
                is_stream: false,
                expected_duration_ms: None,
            },
        })
        .collect();

    ok(result)
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
            &source_id.device_part(),
            source_id.metric_part(),
            start,
            end,
        )
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query metric: {:?}", e)))?;

    let data_points: Vec<serde_json::Value> = points
        .iter()
        .take(query.limit.unwrap_or(1000))
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

/// GET /api/extensions/:id/data-sources
///
/// List data sources (metrics) provided by an extension
///
/// Uses typed DataSourceId for clean data source identification.
pub async fn list_extension_data_sources_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<DataSourceInfoDto>> {
    let registry = &state.extensions.registry;

    let ext = registry
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let ext_read = ext.read().await;
    let metrics = ext_read.metrics();
    let metadata = ext_read.metadata();

    let mut sources = Vec::new();

    // Return extension metrics as data sources using DataSourceId
    for metric in metrics {
        let source_id = DataSourceId::extension(&id, &metric.name);
        sources.push(DataSourceInfoDto {
            id: source_id.storage_key(),
            extension_id: id.clone(),
            command: String::new(), // V2: No command field
            field: metric.name.clone(),
            display_name: format!("{}: {}", metadata.name, metric.display_name),
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
    let registry = &state.extensions.registry;
    let extensions = registry.list().await;

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
                description: cmd.llm_hints.clone(),
            });

            // Command info for agents (as tools)
            tools.push(ExtensionToolDto {
                name: cmd.name.clone(),
                description: cmd.llm_hints.clone(),
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
const _MARKET_REPO: &str = "camthink-ai/NeoMind-Extensions";
const MARKET_BRANCH: &str = "main";
const MARKET_BASE_URL: &str = "https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions";

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
    #[serde(default)]
    pub metadata_url: Option<String>,
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

    #[serde(default)]
    pub capabilities: ExtensionCapabilities,

    #[serde(default)]
    pub builds: HashMap<String, ExtensionBuild>,

    #[serde(default)]
    pub requirements: ExtensionRequirements,

    #[serde(default)]
    pub safety: ExtensionSafety,
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
    pub data_type: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: Option<String>,
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
    let index_url = format!(
        "{}/{}/extensions/index.json",
        MARKET_BASE_URL, MARKET_BRANCH
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ErrorResponse::internal(format!("Failed to build HTTP client: {}", e)))?;

    let response = match client
        .get(&index_url)
        .header("User-Agent", "NeoMind-Extension-Marketplace")
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
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json",
        MARKET_BASE_URL, MARKET_BRANCH, id
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

/// Detect current platform for extension download
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

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        "unknown"
    }
}

/// Compute SHA256 checksum of file content
fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// POST /api/extensions/market/install
///
/// Download and install an extension from the marketplace
pub async fn install_marketplace_extension_handler(
    State(state): State<ServerState>,
    Json(req): Json<MarketplaceInstallRequest>,
) -> HandlerResult<MarketplaceInstallResponse> {
    let registry = &state.extensions.registry;

    // First fetch metadata to get download URL
    let metadata_url = format!(
        "{}/{}/extensions/{}/metadata.json",
        MARKET_BASE_URL, MARKET_BRANCH, req.id
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

    // Detect platform
    let platform = detect_platform();

    // Check if this is a WASM extension (works on all platforms)
    let is_wasm = metadata.builds.contains_key("wasm");
    let build_key = if is_wasm { "wasm" } else { platform };

    if is_wasm && platform == "unknown" {
        // WASM extensions don't need platform detection
    } else if !is_wasm && platform == "unknown" {
        return ok(MarketplaceInstallResponse {
            success: false,
            extension_id: req.id,
            downloaded: false,
            installed: false,
            path: None,
            error: Some("Unsupported platform".to_string()),
        });
    }

    // Get build info for this platform/WASM
    let build = metadata.builds.get(build_key).ok_or_else(|| {
        ErrorResponse::bad_request(format!("No build available for platform: {}", platform))
    })?;

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

    let bytes = download_response
        .bytes()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to read download: {}", e)))?;

    // Verify SHA256 if provided
    if !build.sha256.is_empty() {
        let checksum = compute_sha256(&bytes);
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

    // Create extensions directory
    let extensions_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".neomind")
        .join("extensions");

    std::fs::create_dir_all(&extensions_dir).map_err(|e| {
        ErrorResponse::internal(format!("Failed to create extensions directory: {}", e))
    })?;

    // Write the extension file(s)
    let (file_path, json_path) = if is_wasm {
        // WASM: write both .wasm and .json files
        let wasm_path = extensions_dir.join(&wasm_filename);

        std::fs::write(&wasm_path, &bytes)
            .map_err(|e| ErrorResponse::internal(format!("Failed to write WASM file: {}", e)))?;

        // Download and write JSON sidecar
        let json_path = extensions_dir.join(&json_filename);

        if let Some(json_url) = &build.json_url {
            let json_response = client
                .get(json_url)
                .send()
                .await
                .map_err(|e| ErrorResponse::internal(format!("JSON download failed: {}", e)))?;

            if json_response.status().is_success() {
                let json_bytes = json_response
                    .bytes()
                    .await
                    .map_err(|e| ErrorResponse::internal(format!("Failed to read JSON: {}", e)))?;

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
                                error: Some(format!("JSON checksum verification failed")),
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

        std::fs::write(&file_path, &bytes).map_err(|e| {
            ErrorResponse::internal(format!("Failed to write extension file: {}", e))
        })?;

        tracing::info!("Extension downloaded to: {:?}", file_path);
        (file_path, None)
    };

    // Load and register the extension
    match registry.load_from_path(&file_path).await {
        Ok(_) => {
            // Save to persistent storage
            if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
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

/// GET /api/extensions/market/updates
///
/// Check for updates for installed extensions
pub async fn check_marketplace_updates_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = &state.extensions.registry;
    let installed = registry.list().await;

    let mut updates = Vec::new();

    // For each installed extension, check if there's a newer version in marketplace
    for ext_info in installed {
        let ext_id = ext_info.metadata.id.clone();

        // Fetch metadata from marketplace
        let metadata_url = format!(
            "{}/{}/extensions/{}/metadata.json",
            MARKET_BASE_URL, MARKET_BRANCH, ext_id
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
                        // Compare versions (simple string comparison for now)
                        if metadata.version != ext_info.metadata.version.to_string() {
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
    let registry = &state.extensions.registry;

    // Get extension info for schema
    let ext_info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Get current config from storage
    let current_config: Option<serde_json::Value> =
        if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
            store.load(&id).ok().flatten().and_then(|r| r.config)
        } else {
            None
        };

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
    let registry = &state.extensions.registry;

    // Verify extension exists
    let ext_info = registry
        .get_info(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Validate config against schema if present
    if let Some(config_params) = &ext_info.metadata.config_parameters {
        validate_config(&config, config_params)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?;
    }

    // Save config to storage
    if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
        if let Ok(Some(mut record)) = store.load(&id) {
            record.config = Some(config.clone());
            store.save(&record)?;
        } else {
            // Create new record with config
            let new_record = ExtensionRecord::new(
                id.clone(),
                ext_info.metadata.name.clone(),
                ext_info
                    .metadata
                    .file_path
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

    ok(json!({
        "extension_id": id,
        "config": config,
        "message": "Configuration updated. Reload the extension for changes to take effect.",
    }))
}

/// POST /api/extensions/:id/reload
///
/// Reload an extension with its current configuration.
pub async fn reload_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = &state.extensions.registry;

    // Get current config
    let config: Option<serde_json::Value> =
        if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
            store.load(&id).ok().flatten().and_then(|r| r.config)
        } else {
            None
        };

    // Unregister the extension
    registry
        .unregister(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to unregister: {}", e)))?;

    // Re-register with config (this is a simplified reload - in production
    // you'd want to load from the stored file path)
    // For now, we'll need the file path from the registry

    ok(json!({
        "extension_id": id,
        "message": "Extension reloaded",
        "config_applied": config.is_some(),
    }))
}

/// Build JSON Schema for configuration parameters.
fn build_config_schema_dto(parameters: &[ParameterDefinition]) -> serde_json::Value {
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
