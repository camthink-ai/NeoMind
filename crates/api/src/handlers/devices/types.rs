//! Device type management.

use axum::{extract::{Path, State}, Json};
use serde_json::json;

use edge_ai_devices::DeviceTypeDefinition;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::DeviceTypeDto;

/// List device types.
pub async fn list_device_types_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let types = state.mqtt_device_manager.list_device_types().await;
    let dtos: Vec<DeviceTypeDto> = types.into_iter().map(|dt| DeviceTypeDto {
        device_type: dt.device_type.clone(),
        name: dt.name.clone(),
        description: dt.description.clone(),
        categories: dt.categories.clone(),
        metric_count: dt.uplink.metrics.len(),
        command_count: dt.downlink.commands.len(),
    }).collect();

    ok(json!({
        "device_types": dtos,
        "count": dtos.len(),
    }))
}

/// Get device type details.
pub async fn get_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let dt = state.mqtt_device_manager.get_device_type(&device_type).await
        .ok_or_else(|| ErrorResponse::not_found("DeviceType"))?;

    ok(json!({
        "device_type": dt.device_type,
        "name": dt.name,
        "description": dt.description,
        "categories": dt.categories,
        "uplink": {
            "metrics": dt.uplink.metrics,
        },
        "downlink": {
            "commands": dt.downlink.commands,
        },
    }))
}

/// Register a new device type.
pub async fn register_device_type_handler(
    State(state): State<ServerState>,
    Json(def): Json<DeviceTypeDefinition>,
) -> HandlerResult<serde_json::Value> {
    state.mqtt_device_manager.register_device_type(def).await?;
    ok(json!({
        "success": true,
        "registered": true,
    }))
}

/// Delete a device type.
pub async fn delete_device_type_handler(
    State(state): State<ServerState>,
    Path(device_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state.mqtt_device_manager.mdl_registry().unregister(&device_type).await?;
    ok(json!({
        "success": true,
        "device_type": device_type,
        "deleted": true,
    }))
}

/// Validate a device type definition without registering it.
pub async fn validate_device_type_handler(
    Json(def): Json<DeviceTypeDefinition>,
) -> HandlerResult<serde_json::Value> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate required fields
    if def.device_type.is_empty() {
        errors.push("device_type 不能为空".to_string());
    } else if !def.device_type.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        errors.push("device_type 只能包含字母、数字、下划线和连字符".to_string());
    }

    if def.name.is_empty() {
        errors.push("name 不能为空".to_string());
    }

    // Validate categories
    for category in &def.categories {
        if category.is_empty() {
            warnings.push("categories 中包含空字符串".to_string());
        }
    }

    // Validate uplink metrics
    for (idx, metric) in def.uplink.metrics.iter().enumerate() {
        if metric.name.is_empty() {
            errors.push(format!("uplink.metrics[{}]: name 不能为空", idx));
        }
        if metric.display_name.is_empty() {
            warnings.push(format!("uplink.metrics[{}]: display_name 为空", idx));
        }
        // Validate data type
        match metric.data_type {
            edge_ai_devices::mdl::MetricDataType::Integer |
            edge_ai_devices::mdl::MetricDataType::Float |
            edge_ai_devices::mdl::MetricDataType::String |
            edge_ai_devices::mdl::MetricDataType::Boolean |
            edge_ai_devices::mdl::MetricDataType::Binary => {},
        }
        // Validate min/max for numeric types
        if matches!(metric.data_type, edge_ai_devices::mdl::MetricDataType::Integer | edge_ai_devices::mdl::MetricDataType::Float) {
            if let (Some(min), Some(max)) = (metric.min, metric.max) {
                if min > max {
                    errors.push(format!("uplink.metrics[{}]: min ({}) 不能大于 max ({})", idx, min, max));
                }
            }
        }
    }

    // Validate downlink commands
    for (idx, command) in def.downlink.commands.iter().enumerate() {
        if command.name.is_empty() {
            errors.push(format!("downlink.commands[{}]: name 不能为空", idx));
        }
        if command.payload_template.is_empty() {
            warnings.push(format!("downlink.commands[{}]: payload_template 为空", idx));
        }
        // Validate parameters
        for (pidx, param) in command.parameters.iter().enumerate() {
            if param.name.is_empty() {
                errors.push(format!("downlink.commands[{}].parameters[{}]: name 不能为空", idx, pidx));
            }
        }
    }

    // Check for duplicate metric names
    let mut metric_names = std::collections::HashSet::new();
    for metric in &def.uplink.metrics {
        if !metric_names.insert(&metric.name) {
            errors.push(format!("uplink.metrics: 存在重复的指标名称 '{}'", metric.name));
        }
    }

    // Check for duplicate command names
    let mut command_names = std::collections::HashSet::new();
    for command in &def.downlink.commands {
        if !command_names.insert(&command.name) {
            errors.push(format!("downlink.commands: 存在重复的命令名称 '{}'", command.name));
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
