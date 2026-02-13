//! Rules engine handlers.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono;
use serde_json::{json, Value};

use neomind_devices::MetricDataType as DeviceMetricDataType;
use neomind_rules::dsl::{AlertSeverity, HttpMethod};
use neomind_rules::{
    ComparisonOperator, CompiledRule, MetricDataType as RulesMetricDataType, RuleAction,
    RuleCondition, RuleId, RuleStatus,
};

use super::{
    common::{ok, HandlerResult},
    ServerState,
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
    condition: Value,    // Changed to Value to handle different condition types
    actions: Vec<Value>, // Changed to Value for frontend-compatible format
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<Value>, // Frontend UI state for proper restoration on edit
}

/// Simple rule info for list responses.
#[derive(Debug, serde::Serialize)]
struct RuleDto {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    enabled: bool,
    trigger_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_triggered: Option<String>,
    dsl: String,
}

/// Request body for enabling/disabling a rule.
#[derive(Debug, serde::Deserialize)]
pub struct SetRuleStatusRequest {
    pub enabled: bool,
}

/// Convert ComparisonOperator to symbol string for frontend
fn operator_to_symbol(op: &ComparisonOperator) -> &'static str {
    match op {
        ComparisonOperator::GreaterThan => ">",
        ComparisonOperator::LessThan => "<",
        ComparisonOperator::GreaterEqual => ">=",
        ComparisonOperator::LessEqual => "<=",
        ComparisonOperator::Equal => "==",
        ComparisonOperator::NotEqual => "!=",
    }
}

/// Convert LogLevel to frontend-compatible string
fn log_level_to_string(level: &neomind_rules::LogLevel) -> &'static str {
    match level {
        neomind_rules::LogLevel::Alert => "debug",
        neomind_rules::LogLevel::Info => "info",
        neomind_rules::LogLevel::Warning => "warn",
        neomind_rules::LogLevel::Error => "error",
    }
}

/// Convert AlertSeverity to frontend-compatible string
fn alert_severity_to_string(severity: &AlertSeverity) -> &'static str {
    match severity {
        AlertSeverity::Info => "info",
        AlertSeverity::Warning => "warning",
        AlertSeverity::Error => "error",
        AlertSeverity::Critical => "critical",
    }
}

/// Convert HttpMethod to frontend-compatible string
fn http_method_to_string(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Patch => "PATCH",
    }
}

/// Convert RuleCondition to frontend-compatible JSON Value
fn condition_to_json(cond: &RuleCondition) -> Value {
    match cond {
        RuleCondition::Device {
            device_id,
            metric,
            operator,
            threshold,
        } => {
            json!({
                "device_id": device_id,
                "metric": metric,
                "operator": operator_to_symbol(operator),
                "threshold": threshold,
            })
        }
        RuleCondition::Extension {
            extension_id,
            metric,
            operator,
            threshold,
        } => {
            json!({
                "extension_id": extension_id,
                "metric": metric,
                "operator": operator_to_symbol(operator),
                "threshold": threshold,
            })
        }
        RuleCondition::DeviceRange {
            device_id,
            metric,
            min,
            max,
        } => {
            json!({
                "device_id": device_id,
                "metric": metric,
                "operator": "between",
                "range_min": min,
                "threshold": max,
            })
        }
        RuleCondition::ExtensionRange {
            extension_id,
            metric,
            min,
            max,
        } => {
            json!({
                "extension_id": extension_id,
                "metric": metric,
                "operator": "between",
                "range_min": min,
                "threshold": max,
            })
        }
        RuleCondition::And(conditions) => {
            json!({
                "operator": "and",
                "conditions": conditions.iter().map(condition_to_json).collect::<Vec<_>>(),
            })
        }
        RuleCondition::Or(conditions) => {
            json!({
                "operator": "or",
                "conditions": conditions.iter().map(condition_to_json).collect::<Vec<_>>(),
            })
        }
        RuleCondition::Not(condition) => {
            json!({
                "operator": "not",
                "conditions": [condition_to_json(condition)],
            })
        }
    }
}

/// Convert RuleAction to frontend-compatible JSON Value
fn action_to_json(action: &RuleAction) -> Value {
    match action {
        RuleAction::Notify {
            message,
            channels: _,
        } => {
            json!({
                "type": "Notify",
                "message": message,
            })
        }
        RuleAction::Execute {
            device_id,
            command,
            params,
        } => {
            json!({
                "type": "Execute",
                "device_id": device_id,
                "command": command,
                "params": params,
            })
        }
        RuleAction::Log {
            level,
            message,
            severity: _,
        } => {
            json!({
                "type": "Log",
                "level": log_level_to_string(level),
                "message": message,
            })
        }
        RuleAction::Set {
            device_id,
            property,
            value,
        } => {
            json!({
                "type": "Set",
                "device_id": device_id,
                "property": property,
                "value": value,
            })
        }
        RuleAction::Delay { duration } => {
            json!({
                "type": "Delay",
                "duration": duration.as_millis(),
            })
        }
        RuleAction::CreateAlert {
            title,
            message,
            severity,
        } => {
            json!({
                "type": "CreateAlert",
                "title": title,
                "message": message,
                "severity": alert_severity_to_string(severity),
            })
        }
        RuleAction::HttpRequest {
            method,
            url,
            headers: _,
            body: _,
        } => {
            json!({
                "type": "HttpRequest",
                "method": http_method_to_string(method),
                "url": url,
            })
        }
    }
}

impl From<&CompiledRule> for RuleDetailDto {
    fn from(rule: &CompiledRule) -> Self {
        // Convert RuleCondition to frontend-compatible JSON format
        let condition_json = condition_to_json(&rule.condition);

        // Convert actions to frontend-compatible format
        let actions_json: Vec<Value> = rule.actions.iter().map(action_to_json).collect();

        Self {
            id: rule.id.to_string(),
            name: rule.name.clone(),
            description: rule.description.clone(),
            enabled: matches!(rule.status, RuleStatus::Active),
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered.map(|dt| dt.to_rfc3339()),
            created_at: rule.created_at.to_rfc3339(),
            condition: condition_json,
            actions: actions_json,
            source: rule.source.clone(),
        }
    }
}

/// List rules.
///
/// GET /api/rules
pub async fn list_rules_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let rules = state.automation.rule_engine.list_rules().await;
    let dtos: Vec<RuleDto> = rules
        .into_iter()
        .map(|r| {
            // Format last_triggered as ISO string if available
            let last_triggered = r.state.last_triggered.as_ref().map(|dt| dt.to_rfc3339());

            RuleDto {
                id: r.id.to_string(),
                name: r.name,
                description: r.description.clone(),
                enabled: matches!(r.status, RuleStatus::Active),
                trigger_count: r.state.trigger_count,
                last_triggered,
                dsl: r.dsl.clone(),
            }
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
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    let dto = RuleDetailDto::from(&rule);
    let history = state
        .automation
        .rule_engine
        .get_rule_history(&rule_id)
        .await;

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
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Extract fields from request
    let dsl = req.get("dsl").and_then(|v| v.as_str());
    let name = req.get("name").and_then(|v| v.as_str());
    let description = req.get("description").and_then(|v| v.as_str());
    let enabled = req.get("enabled").and_then(|v| v.as_bool());
    let source = req.get("source").cloned();

    // If DSL is provided, re-parse and replace the entire rule
    if let Some(dsl) = dsl {
        // Use the inner engine to parse DSL and get a CompiledRule
        let parsed = neomind_rules::dsl::RuleDslParser::parse(dsl)
            .map_err(|e| ErrorResponse::internal(format!("Failed to parse DSL: {}", e)))?;

        // Create a compiled rule from the parsed DSL, then override with original ID
        let mut rule = CompiledRule::from_parsed_with_dsl(parsed, dsl.to_string());
        rule.id = rule_id.clone();

        // Override name if provided
        if let Some(name) = name {
            rule.name = name.to_string();
        }

        // Override description if provided
        if let Some(description) = description {
            rule.description = Some(description.to_string());
        }

        // Set source from frontend if provided
        rule.source = source;

        // Handle enable/disable
        rule.status = if enabled.unwrap_or(matches!(rule.status, RuleStatus::Active)) {
            RuleStatus::Active
        } else {
            RuleStatus::Paused
        };

        // Add the rule (this replaces the old one with same ID)
        state
            .automation
            .rule_engine
            .add_rule(rule.clone())
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to update rule: {}", e)))?;

        // Persist to store
        if let Some(ref store) = state.automation.rule_store {
            if let Err(e) = store.save(&rule) {
                tracing::warn!("Failed to save rule to store: {}", e);
            }
        }

        return ok(json!({
            "rule": RuleDetailDto::from(&rule),
            "updated": true,
        }));
    }

    // Get the current rule for simple updates (no DSL provided)
    let mut rule = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Update fields
    if let Some(name) = name {
        rule.name = name.to_string();
    }

    if let Some(description) = description {
        rule.description = Some(description.to_string());
    }

    // Update source if provided
    if let Some(source) = source {
        rule.source = Some(source);
    }

    // Handle enable/disable
    rule.status = if enabled.unwrap_or(matches!(rule.status, RuleStatus::Active)) {
        RuleStatus::Active
    } else {
        RuleStatus::Paused
    };

    // Re-add the rule (this updates it)
    state
        .automation
        .rule_engine
        .add_rule(rule.clone())
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update rule: {}", e)))?;

    // Also update in persistent store
    if let Some(ref store) = state.automation.rule_store {
        if let Err(e) = store.save(&rule) {
            tracing::warn!("Failed to update rule in store: {}", e);
            // Don't fail the request if persistence fails
        } else {
            tracing::debug!("Updated rule {} in persistent store", rule_id);
        }
    }

    ok(json!({
        "rule": RuleDetailDto::from(&rule),
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
        .automation
        .rule_engine
        .remove_rule(&rule_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete rule: {}", e)))?;

    if !removed {
        return Err(ErrorResponse::not_found("Rule"));
    }

    // Also remove from persistent store
    if let Some(ref store) = state.automation.rule_store {
        if let Err(e) = store.delete(&rule_id) {
            tracing::warn!("Failed to delete rule from store: {}", e);
            // Don't fail the request if persistence fails
        } else {
            tracing::debug!("Deleted rule {} from persistent store", rule_id);
        }
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
            .automation
            .rule_engine
            .resume_rule(&rule_id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to enable rule: {}", e)))?;
    } else {
        state
            .automation
            .rule_engine
            .pause_rule(&rule_id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to disable rule: {}", e)))?;
    }

    // Also update in persistent store
    if let Some(ref store) = state.automation.rule_store {
        if let Some(rule) = state.automation.rule_engine.get_rule(&rule_id).await {
            if let Err(e) = store.save(&rule) {
                tracing::warn!("Failed to update rule status in store: {}", e);
                // Don't fail the request if persistence fails
            } else {
                tracing::debug!("Updated rule {} status in persistent store", rule_id);
            }
        }
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
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Check if we should also execute actions (not just test condition)
    let execute_actions = params
        .get("execute")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Get the rule
    let rule = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Debug: Log the source field to understand the data structure
    tracing::debug!(
        rule_id = %id,
        rule_source = ?rule.source,
        "Rule source field for testing"
    );

    // Extract condition fields using pattern matching
    let (dsl_device_id, metric, operator, threshold) = match &rule.condition {
        RuleCondition::Device {
            device_id,
            metric,
            operator,
            threshold,
        } => (device_id.clone(), metric.clone(), *operator, *threshold),
        RuleCondition::DeviceRange {
            device_id,
            metric,
            min: _,
            max,
        } => {
            // For range conditions, use the max as threshold for testing
            (
                device_id.clone(),
                metric.clone(),
                ComparisonOperator::GreaterThan,
                *max,
            )
        }
        _ => {
            return Err(ErrorResponse::bad_request("Cannot test complex conditions"));
        }
    };

    tracing::debug!(
        dsl_device_id = %dsl_device_id,
        metric = %metric,
        "Extracted DSL device_id and metric"
    );

    // Use device_id from source.uiCondition if available (contains actual device ID)
    // The parsed DSL contains device names which won't work for lookups
    let resolved_device_id = if let Some(source) = &rule.source {
        source
            .get("uiCondition")
            .and_then(|ui| ui.get("device_id"))
            .and_then(|id| id.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    } else {
        None
    };

    // Fallback: try to resolve device name to device ID using the device registry
    let device_id = if let Some(ref resolved) = resolved_device_id {
        resolved.clone()
    } else {
        // Try to find device by name in the device registry
        match state
            .devices
            .service
            .get_device_by_name(&dsl_device_id)
            .await
        {
            Some(device) => {
                tracing::debug!(
                    dsl_device_name = %dsl_device_id,
                    resolved_device_id = %device.device_id,
                    "Resolved device name to device ID"
                );
                device.device_id
            }
            None => {
                // Try exact match as device ID (for backwards compatibility)
                tracing::debug!(
                    dsl_device_id = %dsl_device_id,
                    "Could not resolve device name, using as-is"
                );
                dsl_device_id.clone()
            }
        }
    };

    tracing::debug!(
        resolved_device_id = ?resolved_device_id,
        final_device_id = %device_id,
        "Resolved device_id for testing"
    );

    // Get current value for the rule engine
    let current_value = state.automation.rule_engine.get_value(&device_id, &metric);

    // Try to get historical data from time series storage as fallback
    // The metric in the rule might be "battery" but the storage key could be "values.battery"
    // Try multiple common prefixes if the direct lookup fails
    let metric_variants = vec![
        metric.clone(),
        format!("values.{}", metric),
        format!("value.{}", metric),
        format!("data.{}", metric),
        format!("telemetry.{}", metric),
    ];

    let mut telemetry_value = None;
    let mut value_source = "none";

    for metric_variant in &metric_variants {
        tracing::debug!(
            "Trying to query time series for {}/{}",
            device_id,
            metric_variant
        );
        let result = state
            .devices
            .telemetry
            .latest(&device_id, metric_variant)
            .await;

        match result {
            Ok(Some(point)) => {
                telemetry_value = point.value.as_f64();
                value_source = "historical";
                tracing::debug!(
                    "Found data for {}/{} with value {:?}",
                    device_id,
                    metric_variant,
                    point.value
                );
                break;
            }
            Ok(None) => {
                // No data for this variant, try next
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query time series for {}/{}: {}",
                    device_id,
                    metric_variant,
                    e
                );
                continue;
            }
        }
    }

    // Use current value if available, otherwise use historical value
    let used_value = current_value.or(telemetry_value);

    let condition_met = if let Some(val) = used_value {
        operator.evaluate(val, threshold)
    } else {
        return Err(ErrorResponse::internal(format!(
            "Device '{}' has no data for metric '{}'. Tried variants: {}. Current value unavailable and no historical data found. \
            Please ensure the device has transmitted data at least once.",
            device_id,
            metric,
            metric_variants.join(", "),
        )));
    };

    // If execute=true and condition is met, actually execute the rule actions
    let execution_result = if execute_actions && condition_met {
        tracing::info!(
            rule_id = %id,
            rule_name = %rule.name,
            "Executing rule actions via test endpoint"
        );
        Some(state.automation.rule_engine.execute_rule(&rule_id).await)
    } else {
        None
    };

    let mut response = json!({
        "rule_id": id,
        "rule_name": rule.name,
        "condition_met": condition_met,
        "value_used": used_value,
        "value_source": if current_value.is_some() { "current" } else { value_source },
        "threshold": threshold,
        "operator": format!("{:?}", operator),
    });

    // Add execution result if actions were executed
    if let Some(ref result) = execution_result {
        response["executed"] = json!(true);
        response["execution_result"] = json!({
            "success": result.success,
            "actions_executed": result.actions_executed,
            "error": result.error,
            "duration_ms": result.duration_ms
        });
    }

    ok(response)
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

    // Extract source from frontend for UI state preservation
    let source = req.get("source").cloned();

    // Parse the DSL and create rule with source
    let parsed = neomind_rules::dsl::RuleDslParser::parse(dsl)
        .map_err(|e| ErrorResponse::internal(format!("Failed to parse DSL: {}", e)))?;

    // Create a compiled rule from the parsed DSL with source
    let mut rule = CompiledRule::from_parsed_with_dsl(parsed, dsl.to_string());
    rule.source = source;

    // Add the rule to the engine
    state
        .automation
        .rule_engine
        .add_rule(rule.clone())
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create rule: {}", e)))?;
    let rule_id = rule.id.clone();

    // Persist rule to store if available
    if let Some(ref store) = state.automation.rule_store {
        match store.save(&rule) {
            Ok(()) => tracing::debug!("Saved rule {} to persistent store", rule_id),
            Err(e) => tracing::warn!("Failed to save rule to store: {}", e),
        }
    }

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
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    let history = state
        .automation
        .rule_engine
        .get_rule_history(&rule_id)
        .await;

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
    let rules = state.automation.rule_engine.list_rules().await;

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

        match state.automation.rule_engine.add_rule_from_dsl(&dsl).await {
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
    use neomind_devices::ConnectionStatus;
    use neomind_rules::{CommandInfo, DeviceInfo, MetricInfo, ParameterInfo};

    let mut devices = Vec::new();

    // Get all devices with their templates
    let all_devices = state.devices.service.list_devices().await;
    for device in all_devices {
        // Try to get the template for this device type
        let template = state
            .devices
            .service
            .get_template(&device.device_type)
            .await;

        let (metrics, commands) = if let Some(tpl) = template {
            // Convert from DeviceTypeTemplate (MDL definition)
            let metrics: Vec<MetricInfo> = tpl
                .metrics
                .into_iter()
                .map(|m| MetricInfo {
                    name: m.name,
                    data_type: convert_metric_data_type(m.data_type),
                    unit: if m.unit.is_empty() {
                        None
                    } else {
                        Some(m.unit)
                    },
                    min_value: m.min,
                    max_value: m.max,
                })
                .collect();

            let commands: Vec<CommandInfo> = tpl
                .commands
                .into_iter()
                .map(|c| CommandInfo {
                    name: c.name,
                    description: if c.display_name.is_empty() {
                        c.llm_hints.clone()
                    } else {
                        c.display_name
                    },
                    parameters: c
                        .parameters
                        .into_iter()
                        .map(|p| ParameterInfo {
                            name: p.name,
                            param_type: format!("{:?}", p.data_type),
                            required: true, // MDL doesn't have required flag, assume required
                            default_value: p
                                .default_value
                                .and_then(|v| serde_json::to_value(v).ok()),
                        })
                        .collect(),
                })
                .collect();

            (metrics, commands)
        } else {
            // Fallback to generic metrics if no template found
            (
                vec![MetricInfo {
                    name: "value".to_string(),
                    data_type: RulesMetricDataType::Number,
                    unit: None,
                    min_value: None,
                    max_value: None,
                }],
                vec![
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
                ],
            )
        };

        // Check device online status
        let status = state
            .devices
            .service
            .get_device_connection_status(&device.device_id)
            .await;
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

    // Message categories available for rule actions
    let message_categories = vec![
        neomind_rules::AlertChannelInfo {
            id: "alert".to_string(),
            name: "Alert".to_string(),
            channel_type: "alert".to_string(),
            enabled: true,
        },
        neomind_rules::AlertChannelInfo {
            id: "system".to_string(),
            name: "System".to_string(),
            channel_type: "system".to_string(),
            enabled: true,
        },
        neomind_rules::AlertChannelInfo {
            id: "business".to_string(),
            name: "Business".to_string(),
            channel_type: "business".to_string(),
            enabled: true,
        },
    ];

    ok(json!({
        "devices": devices,
        "message_categories": message_categories,
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
    use neomind_rules::{RuleCondition, RuleValidator, ValidationContext};

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
        ">" => neomind_rules::ComparisonOperator::GreaterThan,
        "<" => neomind_rules::ComparisonOperator::LessThan,
        ">=" => neomind_rules::ComparisonOperator::GreaterEqual,
        "<=" => neomind_rules::ComparisonOperator::LessEqual,
        "==" => neomind_rules::ComparisonOperator::Equal,
        "!=" => neomind_rules::ComparisonOperator::NotEqual,
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid operator: {}",
                operator_str
            )));
        }
    };

    let threshold = condition_obj
        .get("threshold")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| ErrorResponse::bad_request("Missing or invalid 'condition.threshold'"))?;

    let condition = RuleCondition::Device {
        device_id: device_id.to_string(),
        metric: metric.to_string(),
        operator,
        threshold,
    };

    // Build validation context
    let mut context = ValidationContext::new();

    // Add devices
    use neomind_rules::{DeviceInfo, MetricDataType, MetricInfo};

    let all_devices = state.devices.service.list_devices().await;
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
                        if let Some(message) = action_value.get("message").and_then(|v| v.as_str())
                        {
                            // Get channels if specified
                            let channels = action_value
                                .get("channels")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                });

                            actions.push(neomind_rules::RuleAction::Notify {
                                message: message.to_string(),
                                channels,
                            });
                        }
                    }
                    "log" => {
                        actions.push(neomind_rules::RuleAction::Log {
                            level: neomind_rules::LogLevel::Info,
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
