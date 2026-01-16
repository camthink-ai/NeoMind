//! Rules engine handlers.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};
use std::collections::HashMap;

use edge_ai_rules::{CompiledRule, RuleId, RuleStatus};

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
    condition: RuleConditionDto,
    actions: Vec<RuleActionDto>,
}

/// Rule condition for API responses.
#[derive(Debug, serde::Serialize)]
struct RuleConditionDto {
    device_id: String,
    metric: String,
    operator: String,
    threshold: f64,
}

/// Rule action for API responses.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
enum RuleActionDto {
    Notify {
        message: String,
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
        Self {
            id: rule.id.to_string(),
            name: rule.name.clone(),
            description: None, // TODO: add description field to CompiledRule
            enabled: matches!(rule.status, RuleStatus::Active),
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered.map(|dt| dt.to_rfc3339()),
            created_at: rule.created_at.to_rfc3339(),
            condition: RuleConditionDto {
                device_id: rule.condition.device_id.clone(),
                metric: rule.condition.metric.clone(),
                operator: format!("{:?}", rule.condition.operator),
                threshold: rule.condition.threshold,
            },
            actions: rule
                .actions
                .iter()
                .map(|a| match a {
                    edge_ai_rules::RuleAction::Notify { message } => RuleActionDto::Notify {
                        message: message.clone(),
                    },
                    edge_ai_rules::RuleAction::Execute {
                        device_id,
                        command,
                        params,
                    } => RuleActionDto::Execute {
                        device_id: device_id.clone(),
                        command: command.clone(),
                        params: params.clone(),
                    },
                    edge_ai_rules::RuleAction::Log { level, message, .. } => RuleActionDto::Log {
                        level: level.to_string(),
                        message: message.clone(),
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

    // Get current value for the condition
    let current_value = state
        .rule_engine
        .get_value(&rule.condition.device_id, &rule.condition.metric);

    let condition_met = if let Some(val) = current_value {
        rule.condition
            .operator
            .evaluate(val, rule.condition.threshold)
    } else {
        return Err(ErrorResponse::internal(format!(
            "Cannot get value for {}:{}",
            rule.condition.device_id, rule.condition.metric,
        )));
    };

    ok(json!({
        "rule_id": id,
        "rule_name": rule.name,
        "condition_met": condition_met,
        "current_value": current_value,
        "threshold": rule.condition.threshold,
        "operator": format!("{:?}", rule.condition.operator),
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
///
/// GET /api/rules/resources
pub async fn get_resources_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_rules::{DeviceInfo, MetricInfo, MetricDataType, CommandInfo};

    let mut devices = Vec::new();

    // Get devices from device service
    let all_devices = state.device_service.list_devices().await;
    for device in all_devices {
        // Convert device to DeviceInfo format
        let mut metrics = Vec::new();

        // Add common metrics based on device type
        match device.device_type.as_str() {
            "sensor" | "temperature_sensor" => {
                metrics.push(MetricInfo {
                    name: "temperature".to_string(),
                    data_type: MetricDataType::Number,
                    unit: Some("°C".to_string()),
                    min_value: Some(-50.0),
                    max_value: Some(150.0),
                });
                metrics.push(MetricInfo {
                    name: "humidity".to_string(),
                    data_type: MetricDataType::Number,
                    unit: Some("%".to_string()),
                    min_value: Some(0.0),
                    max_value: Some(100.0),
                });
            }
            "switch" | "light" => {
                metrics.push(MetricInfo {
                    name: "state".to_string(),
                    data_type: MetricDataType::Boolean,
                    unit: None,
                    min_value: None,
                    max_value: None,
                });
                metrics.push(MetricInfo {
                    name: "power".to_string(),
                    data_type: MetricDataType::Number,
                    unit: Some("W".to_string()),
                    min_value: Some(0.0),
                    max_value: Some(5000.0),
                });
            }
            _ => {
                // Generic metrics
                metrics.push(MetricInfo {
                    name: "value".to_string(),
                    data_type: MetricDataType::Number,
                    unit: None,
                    min_value: None,
                    max_value: None,
                });
            }
        }

        let commands = vec![
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
        ];

        devices.push(DeviceInfo {
            id: device.device_id.clone(),
            name: device.name.clone(),
            device_type: device.device_type.clone(),
            metrics,
            commands,
            online: true, // Device is considered online if in the registry
        });
    }

    // Get alert channels
    let mut alert_channels = Vec::new();
    // Get alert channel names from alert manager
    let channel_names = state.alert_manager.list_channels().await;
    for name in channel_names {
        alert_channels.push(edge_ai_rules::AlertChannelInfo {
            id: name.clone(),
            name: name.clone(),
            channel_type: "unknown".to_string(), // Type info not directly available
            enabled: true, // Assume enabled if in the list
        });
    }

    ok(json!({
        "devices": devices,
        "alert_channels": alert_channels,
    }))
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

    let condition = RuleCondition {
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
                            actions.push(edge_ai_rules::RuleAction::Notify {
                                message: message.to_string(),
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

/// Generate rule from natural language description.
///
/// POST /api/rules/generate
pub async fn generate_rule_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_rules::{DeviceInfo, MetricInfo, MetricDataType, RuleGenerator, ValidationContext};

    let description = req
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'description'"))?;

    // Build validation context
    let mut context = ValidationContext::new();

    // Add devices
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
                metrics.push(MetricInfo {
                    name: "humidity".to_string(),
                    data_type: MetricDataType::Number,
                    unit: Some("%".to_string()),
                    min_value: Some(0.0),
                    max_value: Some(100.0),
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
            online: true,
        });
    }

    // Generate rule
    let generated = RuleGenerator::generate(description, &context, None)
        .map_err(|e| ErrorResponse::bad_request(format!("Generation failed: {}", e)))?;

    // Convert to DSL
    let rule = &generated.rule;
    let dsl = format!(
        "RULE \"{}\"\n  WHEN {}.{} {} {}\n  DO {}\nEND",
        rule.name,
        rule.condition.device_id,
        rule.condition.metric,
        rule.condition.operator.as_str(),
        rule.condition.threshold,
        if !rule.actions.is_empty() {
            match &rule.actions[0] {
                edge_ai_rules::RuleAction::Notify { message } => {
                    format!("NOTIFY \"{}\"", message)
                }
                edge_ai_rules::RuleAction::Execute { command, .. } => {
                    format!("EXECUTE {}", command)
                }
                edge_ai_rules::RuleAction::Log { .. } => "LOG".to_string(),
            }
        } else {
            "LOG".to_string()
        }
    );

    ok(json!({
        "rule": rule,
        "dsl": dsl,
        "explanation": generated.explanation,
        "confidence": generated.confidence,
        "warnings": generated.warnings,
        "suggested_edits": generated.suggested_edits,
    }))
}

/// Get available rule templates.
///
/// GET /api/rules/templates
pub async fn get_templates_handler() -> HandlerResult<serde_json::Value> {
    use edge_ai_rules::RuleTemplates;

    let templates = RuleTemplates::all();

    ok(json!(templates))
}

/// Fill a template with parameters.
///
/// POST /api/rules/templates/fill
pub async fn fill_template_handler(
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use edge_ai_rules::RuleTemplates;

    let template_id = req
        .get("template_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'template_id'"))?;

    let template = RuleTemplates::get(template_id)
        .ok_or_else(|| ErrorResponse::not_found("Template not found"))?;

    let params_map: std::collections::HashMap<String, String> = req
        .get("parameters")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Fill template with parameters
    let filled = template
        .fill(&params_map)
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to fill template: {}", e)))?;

    ok(json!({
        "template_id": filled.template_id,
        "dsl": filled.dsl,
        "parameters": filled.parameters,
    }))
}
