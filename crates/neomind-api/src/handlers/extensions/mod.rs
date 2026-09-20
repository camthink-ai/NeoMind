//! Extension API handlers.
//!
//! Handlers for managing dynamically loaded extensions (.so/.dylib/.dll/.wasm).
//! Extensions are distinct from user configurations like LLM backends or device connections.
//!
//! V2 Extension System:
//! - Extensions use device-standard types (MetricDefinition, ExtensionCommand)
//! - Commands no longer declare output_fields or config
//! - extension_type field removed from metadata

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use base64::Engine as _;

use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::extension::{MetricDataType, ParameterDefinition};
use neomind_storage::{ExtensionRecord, ExtensionStore};

// Domain submodules; `pub use` keeps the historical
// `handlers::extensions::*` paths (router + openapi + re-exports) stable.
mod capabilities;
mod commands;
mod components;
mod config;
mod lifecycle;
mod logs;
mod marketplace;
mod metrics;
mod packages;

pub use capabilities::*;
pub use commands::*;
pub use components::*;
pub use config::*;
pub use lifecycle::*;
pub use logs::*;
pub use marketplace::*;
pub use metrics::*;
pub use packages::*;

/// Validate an extension ID to prevent path traversal in filesystem operations.
/// Extension IDs are kebab-case identifiers (e.g. "weather-forecast").
/// Rejects empty, too-long, or characters outside [a-zA-Z0-9-_].
pub(crate) fn validate_extension_id(id: &str) -> Result<(), ErrorResponse> {
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
pub(crate) fn build_parameters_schema(parameters: &[ParameterDefinition]) -> serde_json::Value {
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

/// Helper function to convert ExtensionInfo to ExtensionDto
pub(crate) fn extension_info_to_dto(
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
