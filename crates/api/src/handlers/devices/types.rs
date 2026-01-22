//! Device type management.

use edge_ai_automation::device_type_generator::{DeviceTypeGenerator, GenerationConfig};
use edge_ai_automation::discovery::DeviceSample;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use edge_ai_devices::registry::DeviceTypeTemplate;
use edge_ai_core::llm::backend::LlmRuntime;
use edge_ai_storage::{LlmBackendInstance, LlmBackendType};
use edge_ai_llm::{
    instance_manager::get_instance_manager,
    OllamaConfig, OllamaRuntime,
};
use edge_ai_llm::backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

use super::models::{DeviceTypeDto, MetricDefinitionDto, CommandDefinitionDto, ParameterDefinitionDto};
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// List device types.
/// Uses new DeviceService - now includes metrics and commands
pub async fn list_device_types_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let templates = state.device_service.list_templates().await;
    let dtos: Vec<DeviceTypeDto> = templates
        .into_iter()
        .map(|t| {
            let mode_str = match t.mode {
                edge_ai_devices::registry::DeviceTypeMode::Simple => "simple",
                edge_ai_devices::registry::DeviceTypeMode::Full => "full",
            };

            // Convert metrics to DTO
            let metrics: Vec<MetricDefinitionDto> = t.metrics.iter().map(|m| {
                MetricDefinitionDto {
                    name: m.name.clone(),
                    display_name: m.display_name.clone(),
                    data_type: format!("{:?}", m.data_type),
                    unit: if m.unit.is_empty() { None } else { Some(m.unit.clone()) },
                    min: m.min,
                    max: m.max,
                }
            }).collect();

            // Convert commands to DTO
            let commands: Vec<CommandDefinitionDto> = t.commands.iter().map(|c| {
                CommandDefinitionDto {
                    name: c.name.clone(),
                    display_name: c.display_name.clone(),
                    parameters: c.parameters.iter().map(|p| {
                        ParameterDefinitionDto {
                            name: p.name.clone(),
                            display_name: p.display_name.clone(),
                            data_type: format!("{:?}", p.data_type),
                        }
                    }).collect(),
                }
            }).collect();

            DeviceTypeDto {
                device_type: t.device_type.clone(),
                name: t.name.clone(),
                description: t.description.clone(),
                categories: t.categories.clone(),
                mode: mode_str.to_string(),
                metrics,
                commands,
                metric_count: Some(t.metrics.len()),
                command_count: Some(t.commands.len()),
            }
        })
        .collect();

    ok(json!({
        "device_types": dtos,
        "count": dtos.len(),
    }))
}

/// Get device type details.
/// Uses new DeviceService - returns simplified format (direct metrics/commands)
pub async fn get_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let template = state
        .device_service
        .get_template(&device_type)
        .await
        .ok_or_else(|| ErrorResponse::not_found("DeviceType"))?;

    let mode_str = match template.mode {
        edge_ai_devices::registry::DeviceTypeMode::Simple => "simple",
        edge_ai_devices::registry::DeviceTypeMode::Full => "full",
    };

    // Return in new simplified format (direct metrics/commands arrays)
    ok(json!({
        "device_type": template.device_type,
        "name": template.name,
        "description": template.description,
        "categories": template.categories,
        "mode": mode_str,
        "metrics": template.metrics,
        "uplink_samples": template.uplink_samples,
        "commands": template.commands,
        "metric_count": template.metrics.len(),
        "command_count": template.commands.len(),
    }))
}

/// Register a new device type.
/// Uses new DeviceService - accepts simplified format (direct metrics/commands)
pub async fn register_device_type_handler(
    State(state): State<ServerState>,
    Json(template): Json<DeviceTypeTemplate>,
) -> HandlerResult<serde_json::Value> {
    // Register the template directly (already in simplified format)
    state
        .device_service
        .register_template(template)
        .await
        .map_err(|e| {
            ErrorResponse::bad_request(format!("Failed to register device type: {}", e))
        })?;

    ok(json!({
        "success": true,
        "registered": true,
    }))
}

/// Delete a device type.
/// Uses new DeviceService
pub async fn delete_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state
        .device_service
        .unregister_template(&device_type)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete device type: {}", e)))?;

    ok(json!({
        "success": true,
        "device_type": device_type,
        "deleted": true,
    }))
}

/// Validate a device type definition without registering it.
/// Accepts simplified format (direct metrics/commands)
pub async fn validate_device_type_handler(
    Json(template): Json<DeviceTypeTemplate>,
) -> HandlerResult<serde_json::Value> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate required fields
    if template.device_type.is_empty() {
        errors.push("device_type 不能为空".to_string());
    } else if !template
        .device_type
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        errors.push("device_type 只能包含字母、数字、下划线和连字符".to_string());
    }

    if template.name.is_empty() {
        errors.push("name 不能为空".to_string());
    }

    // Validate categories
    for category in &template.categories {
        if category.is_empty() {
            warnings.push("categories 中包含空字符串".to_string());
        }
    }

    // Validate metrics (simplified structure - direct array)
    for (idx, metric) in template.metrics.iter().enumerate() {
        if metric.name.is_empty() {
            errors.push(format!("metrics[{}]: name 不能为空", idx));
        }
        if metric.display_name.is_empty() {
            warnings.push(format!("metrics[{}]: display_name 为空", idx));
        }
        // Validate data type
        match metric.data_type {
            edge_ai_devices::MetricDataType::Integer
            | edge_ai_devices::MetricDataType::Float
            | edge_ai_devices::MetricDataType::String
            | edge_ai_devices::MetricDataType::Boolean
            | edge_ai_devices::MetricDataType::Binary
            | edge_ai_devices::MetricDataType::Enum { .. }
            | edge_ai_devices::MetricDataType::Array { .. } => {}
        }
        // Validate min/max for numeric types
        if matches!(
            metric.data_type,
            edge_ai_devices::MetricDataType::Integer | edge_ai_devices::MetricDataType::Float
        )
            && let (Some(min), Some(max)) = (metric.min, metric.max)
                && min > max {
                    errors.push(format!(
                        "metrics[{}]: min ({}) 不能大于 max ({})",
                        idx, min, max
                    ));
                }
    }

    // Validate commands (simplified structure - direct array)
    for (idx, command) in template.commands.iter().enumerate() {
        if command.name.is_empty() {
            errors.push(format!("commands[{}]: name 不能为空", idx));
        }
        if command.payload_template.is_empty() {
            warnings.push(format!("commands[{}]: payload_template 为空", idx));
        }
        // Validate parameters
        for (pidx, param) in command.parameters.iter().enumerate() {
            if param.name.is_empty() {
                errors.push(format!(
                    "commands[{}].parameters[{}]: name 不能为空",
                    idx, pidx
                ));
            }
        }
    }

    // Check for duplicate metric names
    let mut metric_names = std::collections::HashSet::new();
    for metric in &template.metrics {
        if !metric_names.insert(&metric.name) {
            errors.push(format!("metrics: 存在重复的指标名称 '{}'", metric.name));
        }
    }

    // Check for duplicate command names
    let mut command_names = std::collections::HashSet::new();
    for command in &template.commands {
        if !command_names.insert(&command.name) {
            errors.push(format!("commands: 存在重复的命令名称 '{}'", command.name));
        }
    }

    if errors.is_empty() {
        ok(json!({
            "valid": true,
            "warnings": warnings,
            "message": "设备类型定义有效"
        }))
    } else {
        ok(json!({
            "valid": false,
            "errors": errors,
            "warnings": warnings,
            "message": format!("发现 {} 个错误", errors.len())
        }))
    }
}

/// Request for generating device type from samples
#[derive(Debug, Deserialize)]
pub struct GenerateDeviceTypeRequest {
    /// Optional device ID
    #[serde(rename = "device_id")]
    pub device_id: Option<String>,
    /// Optional manufacturer
    #[serde(rename = "manufacturer")]
    pub manufacturer: Option<String>,
    /// Data samples from the device
    pub samples: Vec<DeviceSampleData>,
    /// Minimum coverage threshold (0.0-1.0) for including fields
    /// Fields appearing in less than this ratio of samples will be excluded
    #[serde(rename = "min_coverage", default = "default_min_coverage")]
    pub min_coverage: f32,
    /// Minimum confidence threshold (0.0-1.0) for including metrics
    /// Metrics with AI confidence below this will be excluded
    #[serde(rename = "min_confidence", default = "default_min_confidence")]
    pub min_confidence: f32,
}

fn default_min_coverage() -> f32 { 0.0 }
fn default_min_confidence() -> f32 { 0.0 }

/// A single data sample with timestamp
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceSampleData {
    /// Timestamp of the sample
    pub timestamp: i64,
    /// Data payload
    pub data: serde_json::Value,
}

/// Response from device type generation
#[derive(Debug, Serialize)]
pub struct GenerateDeviceTypeResponse {
    /// Generated device type ID
    #[serde(rename = "id")]
    pub id: String,
    /// Generated device type name
    pub name: String,
    /// Description
    pub description: String,
    /// Device category
    pub category: String,
    /// Manufacturer
    pub manufacturer: String,
    /// Discovered metrics
    pub metrics: Vec<GeneratedMetricDto>,
    /// Discovered commands
    pub commands: Vec<GeneratedCommandDto>,
    /// Confidence score (0-1)
    pub confidence: f32,
}

/// A generated metric
#[derive(Debug, Serialize)]
pub struct GeneratedMetricDto {
    /// Field name (internal)
    pub name: String,
    /// Path to the data
    pub path: String,
    /// Display name
    #[serde(rename = "display_name")]
    pub display_name: String,
    /// Description
    pub description: String,
    /// Data type
    #[serde(rename = "data_type")]
    pub data_type: String,
    /// Semantic type
    #[serde(rename = "semantic_type")]
    pub semantic_type: String,
    /// Unit (if applicable)
    pub unit: Option<String>,
    /// Whether metric is readable
    pub readable: bool,
    /// Whether metric is writable
    pub writable: bool,
    /// Confidence score
    pub confidence: f32,
}

/// A generated command
#[derive(Debug, Serialize)]
pub struct GeneratedCommandDto {
    /// Command name
    pub name: String,
    /// Display name
    #[serde(rename = "display_name")]
    pub display_name: String,
    /// Description
    pub description: String,
    /// Command parameters
    pub parameters: Vec<GeneratedParameterDto>,
    /// Confidence score
    pub confidence: f32,
}

/// A command parameter
#[derive(Debug, Serialize)]
pub struct GeneratedParameterDto {
    /// Parameter name
    pub name: String,
    /// Parameter type
    #[serde(rename = "type")]
    pub type_: String,
    /// Whether parameter is required
    pub required: bool,
}

/// Convert LlmBackendInstance to LlmRuntime
fn instance_to_runtime(instance: &LlmBackendInstance) -> Result<Arc<dyn LlmRuntime>, String> {
    match instance.backend_type {
        LlmBackendType::Ollama => {
            let config = OllamaConfig {
                endpoint: instance.endpoint.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
                model: instance.model.clone(),
                timeout_secs: 120,
            };
            OllamaRuntime::new(config)
                .map(|runtime| Arc::new(runtime) as Arc<dyn LlmRuntime>)
                .map_err(|e| format!("Failed to create Ollama runtime: {}", e))
        }
        LlmBackendType::OpenAi => {
            let provider = CloudProvider::OpenAI;
            let config = CloudConfig {
                api_key: instance.api_key.clone().unwrap_or_default(),
                provider,
                model: Some(instance.model.clone()),
                base_url: instance.endpoint.clone(),
                timeout_secs: 120,
            };
            CloudRuntime::new(config)
                .map(|runtime| Arc::new(runtime) as Arc<dyn LlmRuntime>)
                .map_err(|e| format!("Failed to create OpenAI runtime: {}", e))
        }
        LlmBackendType::Anthropic => {
            let provider = CloudProvider::Anthropic;
            let config = CloudConfig {
                api_key: instance.api_key.clone().unwrap_or_default(),
                provider,
                model: Some(instance.model.clone()),
                base_url: instance.endpoint.clone(),
                timeout_secs: 120,
            };
            CloudRuntime::new(config)
                .map(|runtime| Arc::new(runtime) as Arc<dyn LlmRuntime>)
                .map_err(|e| format!("Failed to create Anthropic runtime: {}", e))
        }
        LlmBackendType::Google => {
            let provider = CloudProvider::Google;
            let config = CloudConfig {
                api_key: instance.api_key.clone().unwrap_or_default(),
                provider,
                model: Some(instance.model.clone()),
                base_url: instance.endpoint.clone(),
                timeout_secs: 120,
            };
            CloudRuntime::new(config)
                .map(|runtime| Arc::new(runtime) as Arc<dyn LlmRuntime>)
                .map_err(|e| format!("Failed to create Google runtime: {}", e))
        }
        LlmBackendType::XAi => {
            let provider = CloudProvider::Grok;
            let config = CloudConfig {
                api_key: instance.api_key.clone().unwrap_or_default(),
                provider,
                model: Some(instance.model.clone()),
                base_url: instance.endpoint.clone(),
                timeout_secs: 120,
            };
            CloudRuntime::new(config)
                .map(|runtime| Arc::new(runtime) as Arc<dyn LlmRuntime>)
                .map_err(|e| format!("Failed to create xAI runtime: {}", e))
        }
    }
}

/// Generate a device type from data samples
pub async fn generate_device_type_from_samples_handler(
    State(_state): State<ServerState>,
    Json(request): Json<GenerateDeviceTypeRequest>,
) -> HandlerResult<GenerateDeviceTypeResponse> {
    // Get LLM instance
    let instance_manager = get_instance_manager()
        .map_err(|e| ErrorResponse::internal(format!("Failed to get LLM manager: {}", e)))?;

    let instance = instance_manager.get_active_instance()
        .ok_or_else(|| ErrorResponse::internal("No active LLM backend. Please configure an LLM backend first."))?;

    // Convert to LlmRuntime
    let llm = instance_to_runtime(&instance)
        .map_err(|e| ErrorResponse::internal(format!("Failed to create LLM runtime: {}", e)))?;

    // Create generator
    let generator = DeviceTypeGenerator::new(llm);

    // Convert request samples to DeviceSample format
    let device_id = request.device_id.as_deref().unwrap_or("unknown-device");
    let manufacturer = request.manufacturer.as_deref();

    let samples: Vec<DeviceSample> = request.samples
        .into_iter()
        .map(|s| DeviceSample::from_json(s.data, format!("sample-{}", s.timestamp)))
        .collect();

    if samples.is_empty() {
        return Err(ErrorResponse::bad_request("No valid samples provided"));
    }

    // Create generation config from request
    let config = GenerationConfig {
        min_coverage: request.min_coverage,
        min_confidence: request.min_confidence,
    };

    // Generate device type with config
    let generated = generator.generate_device_type_with_config(device_id, manufacturer, &samples, config)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to generate device type: {}", e)))?;

    // Convert to response format
    let response = GenerateDeviceTypeResponse {
        id: generated.id,
        name: generated.name,
        description: generated.description,
        category: format!("{:?}", generated.category),
        manufacturer: generated.manufacturer,
        metrics: generated.metrics.into_iter().map(|m| GeneratedMetricDto {
            name: m.name,
            path: m.path,
            display_name: m.display_name,
            description: m.description,
            data_type: format!("{:?}", m.data_type),
            semantic_type: format!("{:?}", m.semantic_type),
            unit: m.unit,
            readable: m.is_readable,
            writable: m.is_writable,
            confidence: 1.0, // Default confidence
        }).collect(),
        commands: generated.commands.into_iter().map(|c| GeneratedCommandDto {
            name: c.name,
            display_name: c.display_name,
            description: c.description,
            parameters: c.parameters.into_iter().map(|p| GeneratedParameterDto {
                name: p.name,
                type_: format!("{:?}", p.param_type),
                required: p.required,
            }).collect(),
            confidence: 1.0, // Default confidence
        }).collect(),
        confidence: 1.0, // Default confidence
    };

    ok(response)
}
