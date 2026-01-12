//! Rules engine handlers.

use axum::{extract::{Path, State}, Json};
use serde_json::{json, Value};
use std::collections::HashMap;

use edge_ai_rules::{RuleId, RuleStatus, CompiledRule};

use super::{ServerState, common::{HandlerResult, ok}};
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
    Notify { message: String },
    Execute { device_id: String, command: String, params: HashMap<String, Value> },
    Log { level: String, message: String },
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
            actions: rule.actions.iter().map(|a| match a {
                edge_ai_rules::RuleAction::Notify { message } => {
                    RuleActionDto::Notify { message: message.clone() }
                }
                edge_ai_rules::RuleAction::Execute { device_id, command, params } => {
                    RuleActionDto::Execute {
                        device_id: device_id.clone(),
                        command: command.clone(),
                        params: params.clone(),
                    }
                }
                edge_ai_rules::RuleAction::Log { level, message, .. } => {
                    RuleActionDto::Log {
                        level: level.to_string(),
                        message: message.clone(),
                    }
                }
            }).collect(),
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
    let dtos: Vec<RuleDto> = rules.into_iter().map(|r| RuleDto {
        id: r.id.to_string(),
        name: r.name,
        enabled: matches!(r.status, RuleStatus::Active),
        trigger_count: r.state.trigger_count,
    }).collect();

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

    let rule = state.rule_engine.get_rule(&rule_id).await
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
    let mut rule = state.rule_engine.get_rule(&rule_id).await
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
    state.rule_engine.add_rule(rule).await
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

    let removed = state.rule_engine.remove_rule(&rule_id).await
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
        state.rule_engine.resume_rule(&rule_id).await
            .map_err(|e| ErrorResponse::internal(format!("Failed to enable rule: {}", e)))?;
    } else {
        state.rule_engine.pause_rule(&rule_id).await
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
    let rule = state.rule_engine.get_rule(&rule_id).await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Get current value for the condition
    let current_value = state.rule_engine
        .get_value(&rule.condition.device_id, &rule.condition.metric);

    let condition_met = if let Some(val) = current_value {
        rule.condition.operator.evaluate(val, rule.condition.threshold)
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
    let dsl = req.get("dsl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'dsl' field"))?;

    // Add rule from DSL
    let rule_id = state.rule_engine.add_rule_from_dsl(dsl).await
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
    let _rule = state.rule_engine.get_rule(&rule_id).await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    let history = state.rule_engine.get_rule_history(&rule_id).await;

    ok(json!({
        "rule_id": id,
        "executions": history,
    }))
}
