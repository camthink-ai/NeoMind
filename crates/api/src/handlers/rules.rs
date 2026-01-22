//! Rules engine handlers.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};
use std::collections::HashMap;

use edge_ai_rules::{CompiledRule, RuleId, RuleStatus, RuleCondition, RuleAction, ComparisonOperator, MetricDataType as RulesMetricDataType};
use edge_ai_devices::MetricDataType as DeviceMetricDataType;

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// Detailed rule info for API responses.
#[derive(Debug, serde::Serialize)]
struct RuleDetailDto {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    enabled: bool,
    trigger_count: u64,
    last_triggered: Option<String>,
    created_at: String,
    condition: Value,  // Changed to Value to handle different condition types
    actions: Vec<RuleActionDto>,
}

/// Rule action for API responses.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
enum RuleActionDto {
    Notify {
        message: String,
        channels: Option<Vec<String>>,
    },
    Execute {
        device_id: String,
        command: String,
        params: HashMap<String, Value>,
    },
    Log {
        level: String,
        message: String,
    },
}

/// Simple rule info for list responses.
#[derive(Debug, serde::Serialize)]
struct RuleDto {
    id: String,
    name: String,
    enabled: bool,
    trigger_count: u64,
}

/// Request body for updating a rule.
#[derive(Debug, serde::Deserialize)]
pub struct UpdateRuleRequest {
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Request body for enabling/disabling a rule.
#[derive(Debug, serde::Deserialize)]
pub struct SetRuleStatusRequest {
    pub enabled: bool,
}

impl From<&CompiledRule> for RuleDetailDto {
    fn from(rule: &CompiledRule) -> Self {
        // Convert RuleCondition enum to JSON Value
        let condition_json = match &rule.condition {
            RuleCondition::Simple { device_id, metric, operator, threshold } => {
                json!({
                    "type": "simple",
                    "device_id": device_id,
                    "metric": metric,
                    "operator": format!("{:?}", operator),
                    "threshold": threshold,
                })
            }
            RuleCondition::Range { device_id, metric, min, max } => {
                json!({
                    "type": "range",
                    "device_id": device_id,
                    "metric": metric,
                    "min": min,
                    "max": max,
                })
            }
            RuleCondition::And(conditions) => {
                json!({
                    "type": "and",
                    "conditions": conditions,
                })
            }
            RuleCondition::Or(conditions) => {
                json!({
                    "type": "or",
                    "conditions": conditions,
                })
            }
            RuleCondition::Not(condition) => {
                json!({
                    "type": "not",
                    "condition": condition,
                })
            }
        };

        Self {
            id: rule.id.to_string(),
            name: rule.name.clone(),
            description: None, // TODO: add description field to CompiledRule
            enabled: matches!(rule.status, RuleStatus::Active),
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered.map(|dt| dt.to_rfc3339()),
            created_at: rule.created_at.to_rfc3339(),
            condition: condition_json,
            actions: rule
                .actions
                .iter()
                .map(|a| match a {
                    RuleAction::Notify { message, channels } => RuleActionDto::Notify {
                        message: message.clone(),
                        channels: channels.clone(),
                    },
                    RuleAction::Execute {
                        device_id,
                        command,
                        params,
                    } => RuleActionDto::Execute {
                        device_id: device_id.clone(),
                        command: command.clone(),
                        params: params.clone(),
                    },
                    RuleAction::Log { level, message, severity: _ } => RuleActionDto::Log {
                        level: level.to_string(),
                        message: message.clone(),
                    },
                    // For other action types, create a placeholder
                    _ => RuleActionDto::Log {
                        level: "info".to_string(),
                        message: format!("{:?}", a),
                    },
                })
                .collect(),
        }
    }
}

/// List rules.
///
/// GET /api/rules
pub async fn list_rules_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let rules = state.rule_engine.list_rules().await;
    let dtos: Vec<RuleDto> = rules
        .into_iter()
        .map(|r| RuleDto {
            id: r.id.to_string(),
            name: r.name,
            enabled: matches!(r.status, RuleStatus::Active),
            trigger_count: r.state.trigger_count,
        })
        .collect();

    ok(json!({
        "rules": dtos,
        "count": dtos.len(),
    }))
}

/// Get a rule by ID.
///
/// GET /api/rules/:id
pub async fn get_rule_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    let rule = state
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    let dto = RuleDetailDto::from(&rule);
    let history = state.rule_engine.get_rule_history(&rule_id).await;

    ok(json!({
        "rule": dto,
        "history": history,
    }))
}

/// Update a rule.
///
/// PUT /api/rules/:id
pub async fn update_rule_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRuleRequest>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Get the current rule
    let mut rule = state
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Update fields
    if let Some(name) = req.name {
        rule.name = name;
    }

    // Handle enable/disable
    let enabled = if let Some(enabled) = req.enabled {
        enabled
    } else {
        matches!(rule.status, RuleStatus::Active)
    };

    rule.status = if enabled {
        RuleStatus::Active
    } else {
        RuleStatus::Paused
    };

    // Re-add the rule (this updates it)
    state
        .rule_engine
        .add_rule(rule)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update rule: {}", e)))?;

    ok(json!({
        "rule_id": id,
        "updated": true,
    }))
}

/// Delete a rule.
///
/// DELETE /api/rules/:id
pub async fn delete_rule_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    let removed = state
        .rule_engine
        .remove_rule(&rule_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete rule: {}", e)))?;

    if !removed {
        return Err(ErrorResponse::not_found("Rule"));
    }

    ok(json!({
        "rule_id": id,
        "deleted": true,
    }))
}

/// Enable or disable a rule.
///
/// POST /api/rules/:id/enable
pub async fn set_rule_status_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<SetRuleStatusRequest>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    if req.enabled {
        state
            .rule_engine
            .resume_rule(&rule_id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to enable rule: {}", e)))?;
    } else {
        state
            .rule_engine
            .pause_rule(&rule_id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to disable rule: {}", e)))?;
    }

    ok(json!({
        "rule_id": id,
        "enabled": req.enabled,
    }))
}

/// Test a rule.
///
/// POST /api/rules/:id/test
pub async fn test_rule_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Get the rule
    let rule = state
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Extract condition fields using pattern matching
    let (device_id, metric, operator, threshold) = match &rule.condition {
        RuleCondition::Simple { device_id, metric, operator, threshold } => {
            (device_id.clone(), metric.clone(), operator.clone(), *threshold)
        }
        RuleCondition::Range { device_id, metric, min, max } => {
            // For range conditions, use the max as threshold for testing
            (device_id.clone(), metric.clone(), ComparisonOperator::GreaterThan, *max)
        }
        _ => {
            return Err(ErrorResponse::bad_request("Cannot test complex conditions"));
        }
    };

    // Get current value for the condition
    let current_value = state.rule_engine.get_value(&device_id, &metric);

    let condition_met = if let Some(val) = current_value {
        operator.evaluate(val, threshold)
    } else {
        // Try to get device telemetry as fallback
        let telemetry_value = state.time_series_storage
            .latest(&device_id, &metric)
            .await
            .ok()
            .flatten()
            .and_then(|point| point.value.as_f64());

        if let Some(val) = telemetry_value {
            operator.evaluate(val, threshold)
        } else {
            return Err(ErrorResponse::internal(format!(
                "Device '{}' has no current value for metric '{}'. Please ensure the device is online and transmitting data.",
                device_id, metric,
            )));
        }
    };

    ok(json!({
        "rule_id": id,
        "rule_name": rule.name,
        "condition_met": condition_met,
        "current_value": current_value,
        "threshold": threshold,
        "operator": format!("{:?}", operator),
    }))
}

/// Create rule.
///
/// POST /api/rules
pub async fn create_rule_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let dsl = req
        .get("dsl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'dsl' field"))?;

    // Add rule from DSL
    let rule_id = state
        .rule_engine
        .add_rule_from_dsl(dsl)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create rule: {}", e)))?;

    ok(json!({
        "rule_id": rule_id.to_string(),
    }))
}

/// Get rule execution history.
///
/// GET /api/rules/:id/history
pub async fn get_rule_history_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Check if rule exists
    let _rule = state
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    let history = state.rule_engine.get_rule_history(&rule_id).await;

    ok(json!({
        "rule_id": id,
        "executions": history,
    }))
}

/// Export all rules as JSON.
///
/// GET /api/rules/export
pub async fn export_rules_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Get all rules
    let rules = state.rule_engine.list_rules().await;

    // Build export structure
    let export = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "count": rules.len(),
        "rules": rules,
    });

    ok(export)
}

/// Import rules from JSON.
///
/// POST /api/rules/import
pub async fn import_rules_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let rules_data = req
        .get("rules")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'rules' array"))?;

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for rule_value in rules_data {
        // Try to serialize to DSL format
        let dsl = serde_json::to_string_pretty(rule_value)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid rule format: {}", e)))?;

        match state.rule_engine.add_rule_from_dsl(&dsl).await {
            Ok(_) => imported += 1,
            Err(e) => {
                let name = rule_value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                errors.push(format!("Rule {}: {}", name, e));
                skipped += 1;
            }
        }
    }

    ok(json!({
        "imported": imported,
        "skipped": skipped,
        "errors": errors,
    }))
}

/// Get available resources for rule validation.
/// Now uses DeviceTypeTemplate for actual device capabilities instead of hardcoded mappings.
///
/// GET /api/rules/resources
pub async fn get_resources_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    
    use edge_ai_rules::{DeviceInfo, MetricInfo, CommandInfo, ParameterInfo};
    use edge_ai_devices::ConnectionStatus;

    let mut devices = Vec::new();

    // Get all devices with their templates
    let all_devices = state.device_service.list_devices().await;
    for device in all_devices {
        // Try to get the template for this device type
        let template = state.device_service.get_template(&device.device_type).await;

        let (metrics, commands) = if let Some(tpl) = template {
            // Convert from DeviceTypeTemplate (MDL definition)
            let metrics: Vec<MetricInfo> = tpl.metrics.into_iter().map(|m| MetricInfo {
                name: m.name,
                data_type: convert_metric_data_type(m.data_type),
                unit: if m.unit.is_empty() { None } else { Some(m.unit) },
                min_value: m.min,
                max_value: m.max,
            }).collect();

            let commands: Vec<CommandInfo> = tpl.commands.into_iter().map(|c| CommandInfo {
                name: c.name,
                description: if c.display_name.is_empty() {
                    c.llm_hints.clone()
                } else {
                    c.display_name
                },
                parameters: c.parameters.into_iter().map(|p| ParameterInfo {
                    name: p.name,
                    param_type: format!("{:?}", p.data_type),
                    required: true, // MDL doesn't have required flag, assume required
                    default_value: p.default_value.and_then(|v| serde_json::to_value(v).ok()),
                }).collect(),
            }).collect();

            (metrics, commands)
        } else {
            // Fallback to generic metrics if no template found
            (vec![MetricInfo {
                name: "value".to_string(),
                data_type: RulesMetricDataType::Number,
                unit: None,
                min_value: None,
                max_value: None,
            }], vec![
                CommandInfo {
                    name: "on".to_string(),
                    description: "Turn on".to_string(),
                    parameters: vec![],
                },
                CommandInfo {
                    name: "off".to_string(),
                    description: "Turn off".to_string(),
                    parameters: vec![],
                },
            ])
        };

        // Check device online status
        let status = state.device_service.get_device_connection_status(&device.device_id).await;
        let online = matches!(status, ConnectionStatus::Connected);

        devices.push(DeviceInfo {
            id: device.device_id.clone(),
            name: device.name.clone(),
            device_type: device.device_type.clone(),
            metrics,
            commands,
            properties: vec![],
            online,
        });
    }

    // Get alert channels from alert manager
    let mut alert_channels = Vec::new();
    let channel_names = state.alert_manager.list_channels().await;
    for name in channel_names {
        alert_channels.push(edge_ai_rules::AlertChannelInfo {
            id: name.clone(),
            name: name.clone(),
            channel_type: "notification".to_string(),
            enabled: true,
        });
    }

    ok(json!({
        "devices": devices,
        "alert_channels": alert_channels,
    }))
}

/// Convert DeviceTypeTemplate MetricDataType to Rules Engine MetricDataType
fn convert_metric_data_type(dt: DeviceMetricDataType) -> RulesMetricDataType {
    match dt {
        DeviceMetricDataType::Integer => RulesMetricDataType::Number,
        DeviceMetricDataType::Float => RulesMetricDataType::Number,
        DeviceMetricDataType::String => RulesMetricDataType::String,
        DeviceMetricDataType::Boolean => RulesMetricDataType::Boolean,
        DeviceMetricDataType::Binary => RulesMetricDataType::String, // Binary as base64 string
        DeviceMetricDataType::Enum { .. } => RulesMetricDataType::Enum(vec![]),
        DeviceMetricDataType::Array { .. } => RulesMetricDataType::String, // Arrays as JSON string
    }
}

/// Validate a rule against available resources.
///
/// POST /api/rules/validate
pub async fn validate_rule_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_rules::{RuleCondition, RuleValidator, ValidationContext};

    // Parse condition from request
    let condition_obj = req
        .get("condition")
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'condition'"))?;

    let device_id = condition_obj
        .get("device_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'condition.device_id'"))?;

    let metric = condition_obj
        .get("metric")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'condition.metric'"))?;

    let operator_str = condition_obj
        .get("operator")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'condition.operator'"))?;

    let operator = match operator_str {
        ">" => edge_ai_rules::ComparisonOperator::GreaterThan,
        "<" => edge_ai_rules::ComparisonOperator::LessThan,
        ">=" => edge_ai_rules::ComparisonOperator::GreaterEqual,
        "<=" => edge_ai_rules::ComparisonOperator::LessEqual,
        "==" => edge_ai_rules::ComparisonOperator::Equal,
        "!=" => edge_ai_rules::ComparisonOperator::NotEqual,
        _ => return Err(ErrorResponse::bad_request(format!("Invalid operator: {}", operator_str))),
    };

    let threshold = condition_obj
        .get("threshold")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| ErrorResponse::bad_request("Missing or invalid 'condition.threshold'"))?;

    let condition = RuleCondition::Simple {
        device_id: device_id.to_string(),
        metric: metric.to_string(),
        operator,
        threshold,
    };

    // Build validation context
    let mut context = ValidationContext::new();

    // Add devices
    use edge_ai_rules::{DeviceInfo, MetricInfo, MetricDataType};

    let all_devices = state.device_service.list_devices().await;
    for device in all_devices {
        let mut metrics = Vec::new();
        match device.device_type.as_str() {
            "sensor" | "temperature_sensor" => {
                metrics.push(MetricInfo {
                    name: "temperature".to_string(),
                    data_type: MetricDataType::Number,
                    unit: Some("°C".to_string()),
                    min_value: Some(-50.0),
                    max_value: Some(150.0),
                });
            }
            _ => {
                metrics.push(MetricInfo {
                    name: "value".to_string(),
                    data_type: MetricDataType::Number,
                    unit: None,
                    min_value: None,
                    max_value: None,
                });
            }
        }

        context.add_device(DeviceInfo {
            id: device.device_id.clone(),
            name: device.name.clone(),
            device_type: device.device_type.clone(),
            metrics,
            commands: vec![],
            properties: vec![],
            online: true,
        });
    }

    // Parse actions if present
    let mut actions = Vec::new();
    if let Some(actions_arr) = req.get("actions").and_then(|v| v.as_array()) {
        for action_value in actions_arr {
            if let Some(action_type) = action_value.get("type").and_then(|v| v.as_str()) {
                match action_type {
                    "notify" => {
                        if let Some(message) = action_value.get("message").and_then(|v| v.as_str()) {
                            // Get channels if specified
                            let channels = action_value.get("channels")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

                            actions.push(edge_ai_rules::RuleAction::Notify {
                                message: message.to_string(),
                                channels,
                            });
                        }
                    }
                    "log" => {
                        actions.push(edge_ai_rules::RuleAction::Log {
                            level: edge_ai_rules::LogLevel::Info,
                            message: "Rule triggered".to_string(),
                            severity: None,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // Validate
    let result = RuleValidator::validate_rule(&condition, &actions, &context);

    ok(json!({
        "valid": result.is_valid,
        "errors": result.errors,
        "warnings": result.warnings,
        "resources": result.available_resources,
    }))
}

