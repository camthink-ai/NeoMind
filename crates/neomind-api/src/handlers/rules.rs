//! Rules engine handlers.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono;
use serde_json::{json, Value};

use neomind_devices::MetricDataType as DeviceMetricDataType;
use neomind_rules::{
    ComparisonOperator, CompiledRule, LogicalOperator, NotifySeverity, RuleAction, RuleCondition,
    RuleId, RuleTrigger,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    last_triggered: Option<String>,
    created_at: String,
    updated_at: String,
    trigger: Value,
    condition: Value,    // Changed to Value to handle different condition types
    actions: Vec<Value>, // Changed to Value for frontend-compatible format
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cooldown: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    for_duration: Option<u64>,
    dsl_preview: String,
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
    created_at: String,
    updated_at: String,
    trigger: Value,
    condition: Value,
    actions: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cooldown: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    for_duration: Option<u64>,
    dsl_preview: String,
}

/// Request body for enabling/disabling a rule.
#[derive(Debug, serde::Deserialize)]
pub struct SetRuleStatusRequest {
    pub enabled: bool,
}

/// Extract a numeric test value from the `rule test --input` JSON body.
///
/// Accepted shapes (in priority order):
/// 1. `{"value": <number>}` — canonical CLI shape
/// 2. `{"input": <number>}` — common alternative key
/// 3. `<number>` — bare number body
///
/// Returns `None` for non-numeric bodies so the handler can fall back to
/// live/historical data.
fn extract_input_number(body: &serde_json::Value) -> Option<f64> {
    if let Some(obj) = body.as_object() {
        for key in ["value", "input"] {
            if let Some(v) = obj.get(key) {
                if let Some(n) = v.as_f64() {
                    return Some(n);
                }
            }
        }
    }
    body.as_f64()
}

/// Extract a text test value from the `rule test --input` JSON body.
///
/// Accepted shapes: `{"value": "<string>"}`, `{"input": "<string>"}`,
/// or a bare JSON string. Returns `None` for non-string bodies so the
/// handler can fall back to numeric extraction / live data.
fn extract_input_text(body: &serde_json::Value) -> Option<String> {
    if let Some(obj) = body.as_object() {
        for key in ["value", "input"] {
            if let Some(v) = obj.get(key) {
                if let Some(s) = v.as_str() {
                    return Some(s.to_string());
                }
            }
        }
    }
    body.as_str().map(|s| s.to_string())
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
        ComparisonOperator::Contains => "contains",
        ComparisonOperator::StartsWith => "starts_with",
        ComparisonOperator::EndsWith => "ends_with",
        ComparisonOperator::Regex => "regex",
    }
}

/// Convert NotifySeverity to frontend-compatible string
fn notify_severity_to_string(severity: &NotifySeverity) -> &'static str {
    match severity {
        NotifySeverity::Info => "info",
        NotifySeverity::Warning => "warning",
        NotifySeverity::Critical => "critical",
        NotifySeverity::Emergency => "emergency",
    }
}

/// Convert RuleCondition to frontend-compatible JSON Value
fn condition_to_json(cond: &RuleCondition) -> Value {
    match cond {
        RuleCondition::Comparison {
            source,
            operator,
            threshold,
            threshold_value,
        } => {
            let mut json = json!({
                "condition_type": "comparison",
                "source": source.storage_key(),
                "source_type": match source.source_type {
                    neomind_core::datasource::DataSourceType::Device => "device",
                    neomind_core::datasource::DataSourceType::Extension => "extension",
                    neomind_core::datasource::DataSourceType::Transform => "transform",
                },
                "source_id": source.source_id,
                "metric": source.field_path,
                "operator": operator_to_symbol(operator),
                "threshold": threshold,
            });
            if let Some(tv) = threshold_value {
                json["threshold_value"] = json!(tv);
            }
            json
        }
        RuleCondition::Range { source, min, max } => {
            json!({
                "condition_type": "range",
                "source": source.storage_key(),
                "source_type": match source.source_type {
                    neomind_core::datasource::DataSourceType::Device => "device",
                    neomind_core::datasource::DataSourceType::Extension => "extension",
                    neomind_core::datasource::DataSourceType::Transform => "transform",
                },
                "source_id": source.source_id,
                "metric": source.field_path,
                "operator": "between",
                "min": min,
                "max": max,
            })
        }
        RuleCondition::Logical {
            operator,
            conditions,
        } => {
            json!({
                "condition_type": "logical",
                "operator": match operator {
                    LogicalOperator::And => "and",
                    LogicalOperator::Or => "or",
                    LogicalOperator::Not => "not",
                },
                "conditions": conditions.iter().map(condition_to_json).collect::<Vec<_>>(),
            })
        }
    }
}

/// Convert RuleAction to frontend-compatible JSON Value
fn action_to_json(action: &RuleAction) -> Value {
    match action {
        RuleAction::Notify { message, severity } => {
            json!({
                "type": "notify",
                "message": message,
                "severity": notify_severity_to_string(severity),
            })
        }
        RuleAction::Execute {
            target,
            target_type,
            command,
            params,
        } => {
            json!({
                "type": "execute",
                "target": target,
                "target_type": match target_type {
                    neomind_rules::ExecuteTarget::Device => "device",
                    neomind_rules::ExecuteTarget::Extension => "extension",
                },
                "command": command,
                "params": params,
            })
        }
        RuleAction::TriggerAgent {
            agent_id,
            input,
            data,
        } => {
            json!({
                "type": "trigger_agent",
                "agent_id": agent_id,
                "input": input,
                "data": data,
            })
        }
    }
}

impl From<&CompiledRule> for RuleDetailDto {
    fn from(rule: &CompiledRule) -> Self {
        // Convert RuleCondition to frontend-compatible JSON format
        let condition_json = rule
            .condition
            .as_ref()
            .map(condition_to_json)
            .unwrap_or_else(|| json!({"condition_type": "none"}));

        // Convert actions to frontend-compatible format
        let actions_json: Vec<Value> = rule.actions.iter().map(action_to_json).collect();

        // Convert trigger
        let trigger_json = match &rule.trigger {
            RuleTrigger::DataChange { sources } => json!({
                "trigger_type": "data_change",
                "sources": sources.iter().map(|s: &neomind_core::datasource::DataSourceId| s.storage_key()).collect::<Vec<_>>(),
            }),
            RuleTrigger::Schedule { cron } => json!({
                "trigger_type": "schedule",
                "cron": cron,
            }),
            RuleTrigger::Manual => json!({
                "trigger_type": "manual",
            }),
        };

        Self {
            id: rule.id.to_string(),
            name: rule.name.clone(),
            description: rule.description.clone(),
            enabled: rule.enabled,
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered.map(|dt| dt.to_rfc3339()),
            created_at: rule.created_at.to_rfc3339(),
            updated_at: rule.updated_at.to_rfc3339(),
            trigger: trigger_json,
            condition: condition_json,
            actions: actions_json,
            tags: if rule.tags.is_empty() {
                None
            } else {
                Some(rule.tags.clone())
            },
            cooldown: Some(rule.cooldown.as_millis() as u64),
            for_duration: rule.for_duration.map(|d| d.as_millis() as u64),
            dsl_preview: rule.dsl_preview.clone(),
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
            let actions_json: Vec<Value> = r.actions.iter().map(action_to_json).collect();
            let condition_json = r
                .condition
                .as_ref()
                .map(condition_to_json)
                .unwrap_or_else(|| json!({"condition_type": "none"}));
            let trigger_json = match &r.trigger {
                RuleTrigger::DataChange { sources } => json!({
                    "trigger_type": "data_change",
                    "sources": sources.iter().map(|s| s.storage_key()).collect::<Vec<_>>(),
                }),
                RuleTrigger::Schedule { cron } => json!({
                    "trigger_type": "schedule",
                    "cron": cron,
                }),
                RuleTrigger::Manual => json!({
                    "trigger_type": "manual",
                }),
            };

            RuleDto {
                id: r.id.to_string(),
                name: r.name,
                description: r.description.clone(),
                enabled: r.enabled,
                trigger_count: r.state.trigger_count,
                last_triggered: r.state.last_triggered.map(|dt| dt.to_rfc3339()),
                created_at: r.created_at.to_rfc3339(),
                updated_at: r.updated_at.to_rfc3339(),
                trigger: trigger_json,
                condition: condition_json,
                actions: actions_json,
                tags: if r.tags.is_empty() {
                    None
                } else {
                    Some(r.tags.clone())
                },
                cooldown: Some(r.cooldown.as_millis() as u64),
                for_duration: r.for_duration.map(|d| d.as_millis() as u64),
                dsl_preview: r.dsl_preview.clone(),
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
    use crate::validator::{validate_required_string, validate_string_length};

    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    // Extract fields from request (clone before potential move)
    let name = req.get("name").and_then(|v| v.as_str()).map(String::from);
    let description = req
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);
    let enabled = req.get("enabled").and_then(|v| v.as_bool());
    let source = req.get("source").cloned();

    // Validate name if provided
    if let Some(ref name_value) = name {
        validate_required_string(name_value, "name")?;
        validate_string_length(name_value, "name", 1, 100)?;
    }

    // Validate description if provided
    if let Some(ref desc_value) = description {
        validate_string_length(desc_value, "description", 0, 500)?;
    }

    // Check if the request contains full rule definition (condition, trigger, actions)
    let has_full_definition = req.get("condition").is_some()
        || req.get("trigger").is_some()
        || req.get("actions").is_some();

    if has_full_definition {
        // Full update: deserialize the entire JSON body as a CompiledRule
        let mut rule: CompiledRule = serde_json::from_value(req)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid rule data: {}", e)))?;

        // Preserve the original ID
        rule.id = rule_id.clone();

        // Override name if provided at top level
        if let Some(name) = name {
            rule.name = name;
        }

        // Override description if provided
        if let Some(description) = description {
            rule.description = Some(description);
        }

        // Set source from frontend if provided
        rule.source = source;

        // Preserve runtime state, created_at, and enabled from existing rule
        let existing = state.automation.rule_engine.get_rule(&rule_id).await;
        if let Some(ref old) = existing {
            rule.state = old.state.clone();
            rule.created_at = old.created_at;
        }
        rule.enabled =
            enabled.unwrap_or_else(|| existing.as_ref().map(|r| r.enabled).unwrap_or(true));
        rule.updated_at = chrono::Utc::now();

        // Finalize: auto-generate dsl_preview, extract trigger sources
        rule.finalize();

        // Validate cron expression for Schedule triggers
        validate_rule_cron(&rule)?;

        // Validate rule against available resources
        {
            let context = build_validation_context(&state);
            let result = neomind_rules::RuleValidator::validate_rule(
                &rule.condition,
                &rule.actions,
                &context,
            );
            if !result.is_valid {
                let detail = result
                    .errors
                    .iter()
                    .map(|e| {
                        format!(
                            "- {}{}",
                            e.message,
                            e.field
                                .as_ref()
                                .map(|f| format!(" ({})", f))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(ErrorResponse::bad_request(format!(
                    "Rule references unavailable resources:\n{}",
                    detail
                )));
            }
        }

        // Enforce minimum cooldown for virtual-metric rules (e.g. __last_seen_age_secs).
        // Prevents 60s-tick alert spam.
        validate_virtual_metric_policy(&rule)?;

        // Update the rule in the engine
        state
            .automation
            .rule_engine
            .update_rule(rule.clone())
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

    // Partial update: get the current rule and update only provided fields
    let mut rule = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Rule"))?;

    // Update fields
    if let Some(name) = name {
        rule.name = name;
    }

    if let Some(description) = description {
        rule.description = Some(description);
    }

    // Update source if provided
    if let Some(source) = source {
        rule.source = Some(source);
    }

    // Handle enable/disable
    if let Some(enabled) = enabled {
        rule.enabled = enabled;
    }

    // Finalize to update timestamps etc.
    rule.finalize();

    // Update the rule
    state
        .automation
        .rule_engine
        .update_rule(rule.clone())
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

    state
        .automation
        .rule_engine
        .set_enabled(&rule_id, req.enabled)
        .await
        .map_err(|e| {
            ErrorResponse::internal(format!(
                "Failed to {} rule: {}",
                if req.enabled { "enable" } else { "disable" },
                e
            ))
        })?;

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
    body: Option<axum::Json<serde_json::Value>>,
) -> HandlerResult<serde_json::Value> {
    // Check if we should also execute actions (not just test condition)
    let execute_actions = params
        .get("execute")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Parse user-provided test value from request body. The CLI sends
    // `--input '{"value": <num>}'` (or a raw number) as JSON body. Without
    // this, the handler silently dropped the body and always fell back to
    // historical telemetry — making `rule test` useless for what-if testing.
    let input_value: Option<f64> = body.as_ref().and_then(|b| extract_input_number(&b.0));

    let input_text: Option<String> = body.as_ref().and_then(|b| extract_input_text(&b.0));

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

    // Extract condition info — v2 model uses Comparison/Range/Logical variants
    let condition = rule
        .condition
        .as_ref()
        .ok_or_else(|| ErrorResponse::bad_request("Cannot test rule: no condition defined"))?;

    let (source, operator, threshold, threshold_value) = match condition {
        RuleCondition::Comparison {
            source,
            operator,
            threshold,
            threshold_value,
        } => (
            source.clone(),
            *operator,
            *threshold,
            threshold_value.clone(),
        ),
        RuleCondition::Range {
            source,
            min: _,
            max,
        } => (source.clone(), ComparisonOperator::GreaterThan, *max, None),
        RuleCondition::Logical { .. } => {
            return Err(ErrorResponse::bad_request("Cannot test logical (compound) conditions directly. Test individual sub-conditions instead."));
        }
    };

    let metric = source.field_path.clone();

    tracing::debug!(
        source = %source.storage_key(),
        source_type = ?source.source_type,
        metric = %metric,
        "Extracted source from condition"
    );

    // For Device sources, resolve device name → device ID.
    // Extension/Transform sources use the ID as-is.
    let query_source = if source.source_type == neomind_core::datasource::DataSourceType::Device {
        let device_id = match state
            .devices
            .service
            .get_device_by_name(&source.source_id)
            .await
        {
            Some(device) => {
                tracing::debug!(
                    source_name = %source.source_id,
                    resolved_device_id = %device.device_id,
                    "Resolved device name to device ID"
                );
                device.device_id
            }
            None => source.source_id.clone(),
        };
        neomind_core::datasource::DataSourceId::device(&device_id, &source.field_path)
    } else {
        source.clone()
    };

    // Get current value from the value provider
    let mut current_value = state
        .automation
        .rule_engine
        .get_value_provider()
        .get_by_source(&query_source);

    // User-supplied test input takes priority over live/historical data.
    // This is the whole point of `rule test --input`: what-if evaluation
    // without needing to publish real telemetry.
    let mut used_input_value = false;
    if let Some(n) = input_value {
        current_value = Some(neomind_rules::RuleValue::Number(n));
        used_input_value = true;
    } else if let Some(s) = input_text.clone() {
        current_value = Some(neomind_rules::RuleValue::Text(s));
        used_input_value = true;
    }

    // Virtual-metric shortcut: when testing a rule that references a virtual
    // metric like `__last_seen_age_secs`, the emitter may not have ticked yet
    // (it runs on a 60s schedule) and the metric is never persisted to
    // telemetry storage. Compute the value on-demand so users can test rules
    // immediately after creation without waiting for the next emitter tick.
    //
    // Semantic mirrors the emitter: metric = 0 while the device is considered
    // online (age < effective_offline_timeout), actual age once offline.
    if current_value.is_none()
        && query_source.source_type == neomind_core::datasource::DataSourceType::Device
        && neomind_rules::VIRTUAL_METRICS.contains(&query_source.field_path.as_str())
        && query_source.field_path == "__last_seen_age_secs"
    {
        let last_seen = state
            .devices
            .service
            .get_device_last_seen(&query_source.source_id)
            .await;
        if last_seen > 0 {
            let age = (chrono::Utc::now().timestamp() - last_seen).max(0) as f64;
            let offline_timeout = state
                .devices
                .service
                .effective_offline_timeout(&query_source.source_id)
                as f64;
            let metric_value = if age >= offline_timeout { age } else { 0.0 };
            tracing::debug!(
                device_id = %query_source.source_id,
                age,
                offline_timeout,
                metric_value,
                "Virtual metric computed on-demand for rule test"
            );
            current_value = Some(neomind_rules::RuleValue::Number(metric_value));
        } else {
            return Err(ErrorResponse::bad_request(format!(
                "Cannot test rule: device '{}' has never reported data (last_seen=0), \
                 so `__last_seen_age_secs` is undefined. Wait for the device to publish \
                 at least one telemetry point, then retry.",
                query_source.source_id
            )).with_hint(
                "1. Verify the device is online and publishing: neomind device get <ID>\n\
                 2. Send a test data point: neomind device write-metric <ID> --metric <METRIC> --value <VALUE>\n\
                 3. Then retry the rule test."
            ));
        }
    }

    // Fallback: query historical data from time series storage.
    // Try multiple common metric prefixes in case the field path differs from storage key.
    let telemetry_source_id = match query_source.source_type {
        neomind_core::datasource::DataSourceType::Device => {
            format!("device:{}", query_source.source_id)
        }
        neomind_core::datasource::DataSourceType::Extension => {
            format!("extension:{}", query_source.source_id)
        }
        neomind_core::datasource::DataSourceType::Transform => {
            format!("transform:{}", query_source.source_id)
        }
    };
    let metric_variants = vec![
        metric.clone(),
        format!("values.{}", metric),
        format!("value.{}", metric),
        format!("data.{}", metric),
        format!("telemetry.{}", metric),
    ];

    let mut telemetry_value: Option<neomind_rules::RuleValue> = None;
    let mut value_source = "none";

    for metric_variant in &metric_variants {
        tracing::debug!(
            "Trying to query time series for {}/{}",
            telemetry_source_id,
            metric_variant
        );
        let result = state
            .devices
            .telemetry
            .latest(&telemetry_source_id, metric_variant)
            .await;

        match result {
            Ok(Some(point)) => {
                telemetry_value = match &point.value {
                    neomind_devices::MetricValue::Float(v) => {
                        Some(neomind_rules::RuleValue::Number(*v))
                    }
                    neomind_devices::MetricValue::Integer(v) => {
                        Some(neomind_rules::RuleValue::Number(*v as f64))
                    }
                    neomind_devices::MetricValue::Boolean(v) => {
                        Some(neomind_rules::RuleValue::Number(if *v { 1.0 } else { 0.0 }))
                    }
                    neomind_devices::MetricValue::String(s) => {
                        Some(neomind_rules::RuleValue::Text(s.clone()))
                    }
                    // Non-scalar types: skip (can't be used in rule conditions)
                    _ => None,
                };
                value_source = "historical";
                tracing::debug!(
                    "Found data for {}/{} with value {:?}",
                    telemetry_source_id,
                    metric_variant,
                    point.value
                );
                break;
            }
            Ok(None) => {
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query time series for {}/{}: {}",
                    telemetry_source_id,
                    metric_variant,
                    e
                );
                continue;
            }
        }
    }

    // Determine value source before moving current_value
    let is_current = current_value.is_some();

    // Use current value if available, otherwise use historical value
    let used_value = current_value.or(telemetry_value);

    let condition_met = if let Some(rv) = used_value.as_ref() {
        match condition {
            RuleCondition::Comparison { .. } => match rv {
                neomind_rules::RuleValue::Number(v) => operator.evaluate(*v, threshold),
                neomind_rules::RuleValue::Text(s) => {
                    let fallback = threshold.to_string();
                    let t = threshold_value.as_deref().unwrap_or(&fallback);
                    operator.evaluate_str(s, t)
                }
            },
            RuleCondition::Range { min, max, .. } => rv
                .as_number()
                .map(|v| v >= *min && v <= *max)
                .unwrap_or(false),
            RuleCondition::Logical { .. } => unreachable!(), // Returned error above
        }
    } else {
        return Err(ErrorResponse::bad_request(
            format!("Cannot test rule: source '{}' has no data for metric '{}'.", query_source.storage_key(), metric)
        ).with_hint(
            "The data source must have produced data at least once before testing rules.\n\
             1. Check source status: neomind device get <ID> / neomind extension get <ID>\n\
             2. Send test data: neomind device write-metric <ID> --metric <METRIC> --value <VALUE>\n\
             3. Then retry: neomind rule test <RULE_ID>"
        ));
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
        "value_source": if used_input_value { "input" } else if is_current { "current" } else { value_source },
        "threshold": match condition {
            RuleCondition::Range { .. } => serde_json::Value::Null,
            _ => json!(threshold),
        },
        "operator": match condition {
            RuleCondition::Range { .. } => "between",
            _ => operator.symbol(),
        },
    });

    // Add range bounds for Range conditions
    if let RuleCondition::Range { min, max, .. } = condition {
        response["range_min"] = json!(min);
        response["range_max"] = json!(max);
    }

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

/// Manually trigger a rule.
///
/// POST /api/rules/:id/trigger — evaluates the rule condition and executes actions
/// if the condition is met. This is the entry point for `RuleTrigger::Manual` rules.
pub async fn trigger_rule_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rule_id = RuleId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request(format!("Invalid rule ID: {}", id)))?;

    let result = state.automation.rule_engine.execute_rule(&rule_id).await;
    if !result.success && result.error.as_deref() == Some("Rule not found") {
        return Err(ErrorResponse::not_found(format!("Rule '{}' not found", id)));
    }
    ok(json!({
        "rule_id": id,
        "success": result.success,
        "actions_executed": result.actions_executed,
        "error": result.error,
        "duration_ms": result.duration_ms,
    }))
}

/// Create rule.
///
/// POST /api/rules — accepts a JSON body representing a CompiledRule.
pub async fn create_rule_handler(
    State(state): State<ServerState>,
    Json(mut req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use crate::validator::{validate_required_string, validate_string_length};

    // Validate name
    let name = req
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'name' field"))?;
    validate_required_string(name, "name")?;
    validate_string_length(name, "name", 1, 100)?;

    // Validate description if provided
    if let Some(desc) = req.get("description").and_then(|v| v.as_str()) {
        validate_string_length(desc, "description", 0, 500)?;
    }

    // Default the trigger to DataChange when absent. Condition-based rules
    // (the overwhelmingly common case) all want DataChange, and the engine
    // auto-extracts sources via `RuleTrigger::from_condition` during
    // `finalize()`. Forcing every API consumer to spell out
    // `{"trigger":{"trigger_type":"data_change"}}` is a footgun — without
    // this default the canonical skill example (which omits `trigger`)
    // fails with "missing field `trigger`", and agents get trapped
    // cycling through wrong enum shapes (string / externally-tagged /
    // flat) even when the 400-error hint shows the correct one.
    if req.get("trigger").is_none() {
        if let Some(obj) = req.as_object_mut() {
            obj.insert(
                "trigger".to_string(),
                serde_json::json!({"trigger_type": "data_change"}),
            );
        }
    }

    // Deserialize JSON body into a CompiledRule
    let mut rule: CompiledRule = serde_json::from_value(req)
        .map_err(|e| {
            let err_msg = format!("{}", e);
            ErrorResponse::bad_request(format!("Invalid rule data: {}", err_msg)).with_hint(
                format!(
                    "Provide a valid JSON rule object. Required fields: 'name'. \
                     `trigger` defaults to {{\"trigger_type\":\"data_change\"}} when omitted. \
                     Condition types: 'comparison', 'range', 'logical'. \
                     Action types: 'notify', 'execute', 'trigger_agent'. \
                     Trigger shape (internally tagged, field is 'trigger_type'): \
                     {{\"trigger\":{{\"trigger_type\":\"data_change\"}}}} | \
                     {{\"trigger\":{{\"trigger_type\":\"schedule\",\"cron\":\"* * * * *\"}}}} | \
                     {{\"trigger\":{{\"trigger_type\":\"manual\"}}}}. \
                     Minimal working example: \
                     {{\"name\":\"x\",\"trigger\":{{\"trigger_type\":\"data_change\"}},\
                      \"condition\":{{\"condition_type\":\"comparison\",\"source\":\"device:sensor-001:values.battery\",\
                      \"operator\":\"less_than\",\"threshold\":20.0}},\
                      \"actions\":[{{\"type\":\"notify\",\"message\":\"low\",\"severity\":\"warning\"}}]}}. \
                     Error detail: {}", err_msg
                )
            )
        })?;

    // Ensure fresh timestamps for new rules (serde default = epoch)
    let now = chrono::Utc::now();
    if rule.created_at.timestamp() == 0 {
        rule.created_at = now;
    }
    rule.updated_at = now;

    // Finalize: auto-generate dsl_preview, extract trigger sources
    rule.finalize();

    // Validate cron expression for Schedule triggers
    validate_rule_cron(&rule)?;

    // Validate rule against available resources (reject rules referencing
    // non-existent devices/metrics/agents at creation time)
    {
        let context = build_validation_context(&state);
        let result =
            neomind_rules::RuleValidator::validate_rule(&rule.condition, &rule.actions, &context);
        if !result.is_valid {
            let detail = result
                .errors
                .iter()
                .map(|e| {
                    format!(
                        "- {}{}",
                        e.message,
                        e.field
                            .as_ref()
                            .map(|f| format!(" ({})", f))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(ErrorResponse::bad_request(format!(
                "Rule references unavailable resources:\n{}",
                detail
            )));
        }
    }

    // Enforce minimum cooldown for virtual-metric rules (e.g. __last_seen_age_secs).
    // Prevents 60s-tick alert spam.
    validate_virtual_metric_policy(&rule)?;

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
        "rule": RuleDetailDto::from(&rule),
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

    // Try persistent store first, fall back to in-memory
    let history = if let Some(ref store) = state.automation.rule_store {
        match store.load_history(&rule_id) {
            Ok(h) if !h.is_empty() => h,
            _ => {
                state
                    .automation
                    .rule_engine
                    .get_rule_history(&rule_id)
                    .await
            }
        }
    } else {
        state
            .automation
            .rule_engine
            .get_rule_history(&rule_id)
            .await
    };

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
        // Deserialize each rule from JSON
        match serde_json::from_value::<CompiledRule>(rule_value.clone()) {
            Ok(mut rule) => {
                // Finalize the rule
                rule.finalize();

                match state.automation.rule_engine.add_rule(rule).await {
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
            Err(e) => {
                let name = rule_value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                errors.push(format!("Rule {}: invalid format - {}", name, e));
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
    use neomind_rules::{CommandInfo, DeviceInfo, MetricDataType, MetricInfo, ParameterInfo};

    let mut devices = Vec::new();

    // Get all devices with their templates
    let all_devices = state.devices.service.list_devices();

    // Optimize: collect unique device_types, batch get and pre-convert templates
    use std::collections::{HashMap, HashSet};

    let unique_types: HashSet<_> = all_devices.iter().map(|d| &d.device_type).collect();

    // Pre-convert all templates to (metrics, commands) pairs
    let mut template_data_map: HashMap<String, (Vec<MetricInfo>, Vec<CommandInfo>)> =
        HashMap::new();

    for device_type in unique_types {
        if let Some(tpl) = state.devices.service.get_template(device_type) {
            // Pre-convert metrics
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

            // Pre-convert commands
            let commands: Vec<CommandInfo> = tpl
                .commands
                .into_iter()
                .map(|c| CommandInfo {
                    name: c.name,
                    description: if c.display_name.is_empty() {
                        c.description.clone()
                    } else {
                        c.display_name
                    },
                    parameters: c
                        .parameters
                        .into_iter()
                        .map(|p| ParameterInfo {
                            name: p.name,
                            param_type: format!("{:?}", p.data_type),
                            required: true,
                            default_value: p
                                .default_value
                                .and_then(|v| serde_json::to_value(v).ok()),
                        })
                        .collect(),
                })
                .collect();

            template_data_map.insert(device_type.clone(), (metrics, commands));
        }
    }

    // Now loop and look up pre-converted data
    for device in all_devices {
        let (metrics, commands) = template_data_map
            .get(&device.device_type)
            .cloned()
            .unwrap_or_else(|| {
                // Fallback to generic metrics if no template found
                (
                    vec![MetricInfo {
                        name: "value".to_string(),
                        data_type: MetricDataType::Number,
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
            });

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
fn convert_metric_data_type(dt: DeviceMetricDataType) -> neomind_rules::MetricDataType {
    match dt {
        DeviceMetricDataType::Integer => neomind_rules::MetricDataType::Number,
        DeviceMetricDataType::Float => neomind_rules::MetricDataType::Number,
        DeviceMetricDataType::String => neomind_rules::MetricDataType::String,
        DeviceMetricDataType::Boolean => neomind_rules::MetricDataType::Boolean,
        DeviceMetricDataType::Binary => neomind_rules::MetricDataType::String, // Binary as base64 string
        DeviceMetricDataType::Enum { .. } => neomind_rules::MetricDataType::Enum(vec![]),
        DeviceMetricDataType::Array { .. } => neomind_rules::MetricDataType::String, // Arrays as JSON string
    }
}

/// Build a [`ValidationContext`] populated with the currently registered devices.
///
/// This is shared between `validate_rule_handler` and `create_rule_handler` so that
/// rule creation rejects rules that reference non-existent resources.
fn build_validation_context(state: &ServerState) -> neomind_rules::ValidationContext {
    use neomind_rules::{DeviceInfo, ValidationContext};

    let mut context = ValidationContext::new();

    let all_devices = state.devices.service.list_devices();
    for device in all_devices {
        let metrics = build_device_metrics_for_validation(&state.devices.service, &device);

        context.add_device(DeviceInfo {
            id: device.device_id.clone(),
            name: device.name.clone(),
            device_type: device.device_type.clone(),
            metrics,
            commands: build_device_commands_for_validation(
                &state.devices.service,
                &device,
            ),
            online: true,
        });
    }

    context
}

/// Build the command list for rule validation from the device-type template.
/// Mirrors `build_device_metrics_for_validation` — commands were previously
/// hardcoded to `vec![]` (the 2026-06-29 metrics fix missed commands),
/// causing ALL Execute actions on device targets to fail validation with
/// "Command not supported". Surfaced by the runtime rule→Execute eval case.
fn build_device_commands_for_validation(
    service: &neomind_devices::DeviceService,
    device: &neomind_devices::DeviceConfig,
) -> Vec<neomind_rules::CommandInfo> {
    if let Some(template) = service.get_template(&device.device_type) {
        return template
            .commands
            .iter()
            .map(|c| neomind_rules::CommandInfo {
                name: c.name.clone(),
                description: c.description.clone(),
                parameters: vec![],
            })
            .collect();
    }
    Vec::new()
}

/// Build the metric list for rule validation.
///
/// Source of truth is the device-type template (so rules can reference real
/// template metrics like `values.battery` on `ne101_camera`). Falls back to
/// legacy hardcoded behavior ONLY when no template is registered, to preserve
/// existing behavior for ad-hoc device types without a registered template.
fn build_device_metrics_for_validation(
    service: &neomind_devices::DeviceService,
    device: &neomind_devices::DeviceConfig,
) -> Vec<neomind_rules::MetricInfo> {
    use neomind_devices::MetricDataType as DevicesMetricDataType;
    use neomind_rules::{MetricDataType, MetricInfo};

    // Preferred: pull real metrics from the registered device-type template.
    if let Some(template) = service.get_template(&device.device_type) {
        if !template.metrics.is_empty() {
            return template
                .metrics
                .iter()
                .map(|m| MetricInfo {
                    name: m.name.clone(),
                    data_type: match m.data_type {
                        DevicesMetricDataType::Float | DevicesMetricDataType::Integer => {
                            MetricDataType::Number
                        }
                        DevicesMetricDataType::Boolean => MetricDataType::Boolean,
                        // String / Array / Binary / Enum all map to String for
                        // rule-validation purposes (rules compare against the
                        // string representation of the value).
                        _ => MetricDataType::String,
                    },
                    unit: if m.unit.is_empty() {
                        None
                    } else {
                        Some(m.unit.clone())
                    },
                    min_value: m.min,
                    max_value: m.max,
                })
                .collect();
        }
    }

    // Legacy fallback: hardcoded metric shapes for ad-hoc device types that
    // have no registered template. Preserves pre-fix behavior so we don't
    // break existing rule validations on minimal test installs.
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
    metrics
}

/// Enforce minimum cooldown for virtual-metric rules.
///
/// Virtual metrics like `__last_seen_age_secs` update every 60 seconds.
/// Without a minimum cooldown, this creates alert spam.
fn validate_virtual_metric_policy(rule: &CompiledRule) -> Result<(), ErrorResponse> {
    if let Err(msg) = neomind_rules::RuleValidator::validate_virtual_metric_cooldown(rule) {
        return Err(ErrorResponse::bad_request(msg).with_hint(
            "Set 'cooldown' to at least 60000 ms (60 seconds) when using \
             __last_seen_age_secs in the condition. Production deployments typically \
             use 5 min – 1 h to avoid alert fatigue."
                .to_string(),
        ));
    }
    Ok(())
}

/// Validate the cron expression of a Schedule-type rule.
///
/// Returns `Err` with a user-friendly message if the cron syntax is invalid.
fn validate_rule_cron(rule: &CompiledRule) -> Result<(), ErrorResponse> {
    if let neomind_rules::RuleTrigger::Schedule { ref cron } = rule.trigger {
        cron.parse::<cron::Schedule>().map_err(|e| {
            ErrorResponse::bad_request(format!(
                "Invalid cron expression '{}': {}. \
                 Use standard 5-field cron (min hour day month weekday), e.g. '0 */5 * * *'.",
                cron, e
            ))
        })?;
    }
    Ok(())
}

/// Validate a rule against available resources.
///
/// POST /api/rules/validate
pub async fn validate_rule_handler(
    State(state): State<ServerState>,
    Json(req): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use neomind_rules::{RuleCondition, RuleValidator};

    // Parse condition from request — v2 uses tagged enum format
    let condition: Option<RuleCondition> = if let Some(cond_value) = req.get("condition") {
        Some(
            serde_json::from_value(cond_value.clone())
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid condition: {}", e)))?,
        )
    } else {
        None
    };

    // Build validation context
    let context = build_validation_context(&state);

    // Parse actions if present — v2 uses tagged enum format
    let actions: Vec<RuleAction> =
        if let Some(actions_arr) = req.get("actions").and_then(|v| v.as_array()) {
            actions_arr
                .iter()
                .filter_map(|av| serde_json::from_value(av.clone()).ok())
                .collect()
        } else {
            vec![]
        };

    // Validate
    let result = RuleValidator::validate_rule(&condition, &actions, &context);

    ok(json!({
        "valid": result.is_valid,
        "errors": result.errors,
        "warnings": result.warnings,
    }))
}
