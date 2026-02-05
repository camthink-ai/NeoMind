//! Unified automations API handlers.
//!
//! This module provides a unified API for transforms and rules,
//! allowing them to be managed through a single interface.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use edge_ai_automation::{
    Automation, AutomationConverter, AutomationType, IntentResult,
};

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// Query parameters for filtering automations.
#[derive(Debug, Deserialize)]
pub struct AutomationFilter {
    /// Filter by automation type
    pub r#type: Option<AutomationTypeFilter>,
    /// Filter by enabled status
    pub enabled: Option<bool>,
    /// Search in name/description
    pub search: Option<String>,
}

/// Automation type filter.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationTypeFilter {
    Transform,
    Rule,
    All,
}

impl From<AutomationTypeFilter> for Option<AutomationType> {
    fn from(filter: AutomationTypeFilter) -> Self {
        match filter {
            AutomationTypeFilter::Transform => Some(AutomationType::Transform),
            AutomationTypeFilter::Rule => Some(AutomationType::Rule),
            AutomationTypeFilter::All => None,
        }
    }
}

/// Automation list response.
#[derive(Debug, Serialize)]
pub struct AutomationListResponse {
    /// List of automations
    pub automations: Vec<AutomationDto>,
    /// Total count
    pub count: usize,
}

/// Automation DTO for API responses.
#[derive(Debug, Serialize)]
pub struct AutomationDto {
    /// Automation ID
    pub id: String,
    /// Automation name
    pub name: String,
    /// Description
    pub description: String,
    /// Automation type
    #[serde(rename = "type")]
    pub automation_type: AutomationType,
    /// Whether enabled
    pub enabled: bool,
    /// Execution count
    pub execution_count: u64,
    /// Last executed timestamp
    pub last_executed: Option<i64>,
    /// Complexity score (1-5)
    pub complexity: u8,
    /// Created at timestamp
    pub created_at: i64,
    /// Updated at timestamp
    pub updated_at: i64,
}

/// Request body for analyzing intent.
#[derive(Debug, Deserialize)]
pub struct AnalyzeIntentRequest {
    /// Natural language description
    pub description: String,
}

/// Request body for creating an automation.
#[derive(Debug, Deserialize)]
pub struct CreateAutomationRequest {
    /// Automation name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Automation type (if not specified, will be inferred from description)
    pub r#type: Option<AutomationType>,
    /// Whether to enable immediately
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Automation definition (transform or rule)
    pub definition: Value,
}

fn default_enabled() -> bool {
    true
}

/// Request body for updating an automation.
#[derive(Debug, Deserialize)]
pub struct UpdateAutomationRequest {
    /// New name
    pub name: Option<String>,
    /// New description
    pub description: Option<String>,
    /// New definition
    pub definition: Option<Value>,
    /// Enabled status
    pub enabled: Option<bool>,
}

/// Request body for enabling/disabling an automation.
#[derive(Debug, Deserialize)]
pub struct SetAutomationStatusRequest {
    /// Whether to enable the automation
    pub enabled: bool,
}

/// Request body for converting automation type.
#[derive(Debug, Deserialize)]
pub struct ConvertAutomationRequest {
    /// Target type
    pub r#type: AutomationType,
}

/// Convert automation to DTO.
impl From<Automation> for AutomationDto {
    fn from(automation: Automation) -> Self {
        match automation {
            Automation::Transform(transform) => Self {
                id: transform.metadata.id.clone(),
                name: transform.metadata.name.clone(),
                description: transform.metadata.description.clone(),
                automation_type: AutomationType::Transform,
                enabled: transform.metadata.enabled,
                execution_count: transform.metadata.execution_count,
                last_executed: transform.metadata.last_executed,
                complexity: transform.complexity_score(),
                created_at: transform.metadata.created_at,
                updated_at: transform.metadata.updated_at,
            },
            Automation::Rule(rule) => Self {
                id: rule.metadata.id.clone(),
                name: rule.metadata.name.clone(),
                description: rule.metadata.description.clone(),
                automation_type: AutomationType::Rule,
                enabled: rule.metadata.enabled,
                execution_count: rule.metadata.execution_count,
                last_executed: rule.metadata.last_executed,
                complexity: rule.complexity_score(),
                created_at: rule.metadata.created_at,
                updated_at: rule.metadata.updated_at,
            },
        }
    }
}

/// List all automations.
///
/// GET /api/automations?type=transform|rule|all&enabled=true|false&search=query
pub async fn list_automations_handler(
    Query(filter): Query<AutomationFilter>,
    State(state): State<ServerState>,
) -> HandlerResult<AutomationListResponse> {
    let Some(store) = &state.automation_store else {
        return ok(AutomationListResponse {
            automations: Vec::new(),
            count: 0,
        });
    };

    let automations = store.list_automations().await.unwrap_or_default();

    let mut filtered: Vec<_> = automations
        .into_iter()
        .filter(|a| {
            // Filter by type
            if let Some(type_filter) = &filter.r#type {
                let filter_type = match type_filter {
                    AutomationTypeFilter::Transform => AutomationType::Transform,
                    AutomationTypeFilter::Rule => AutomationType::Rule,
                    AutomationTypeFilter::All => return true,
                };
                if a.automation_type() != filter_type {
                    return false;
                }
            }

            // Filter by enabled status
            if let Some(enabled) = filter.enabled
                && a.is_enabled() != enabled {
                    return false;
                }

            // Search in name/description
            if let Some(search) = &filter.search {
                let search_lower = search.to_lowercase();
                let name_matches = a.name().to_lowercase().contains(&search_lower);
                let desc_matches = match a {
                    Automation::Transform(t) => t.metadata.description.to_lowercase().contains(&search_lower),
                    Automation::Rule(r) => r.metadata.description.to_lowercase().contains(&search_lower),
                };
                if !name_matches && !desc_matches {
                    return false;
                }
            }

            true
        })
        .map(AutomationDto::from)
        .collect();

    // Sort by updated_at descending
    filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    ok(AutomationListResponse {
        count: filtered.len(),
        automations: filtered,
    })
}

/// Get a specific automation by ID.
///
/// GET /api/automations/:id
pub async fn get_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    match store.get_automation(&id).await {
        Ok(Some(automation)) => {
            let dto = AutomationDto::from(automation.clone());
            ok(json!({
                "automation": dto,
                "definition": automation,
            }))
        }
        Ok(None) => {
            Err(ErrorResponse::not_found("Automation not found"))
        }
        Err(e) => {
            tracing::error!("Error getting automation: {}", e);
            Err(ErrorResponse::internal(format!("Failed to get automation: {}", e)))
        }
    }
}

/// Create a new automation.
///
/// POST /api/automations
pub async fn create_automation_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateAutomationRequest>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Determine automation type
    let automation_type = req.r#type.unwrap_or(AutomationType::Rule);

    // Create the automation based on type
    let automation = match automation_type {
        AutomationType::Transform => {
            // Parse transform from definition
            match serde_json::from_value(req.definition) {
                Ok(transform) => Automation::Transform(transform),
                Err(e) => {
                    return Err(ErrorResponse::bad_request(format!("Invalid transform definition: {}", e)));
                }
            }
        }
        AutomationType::Rule => {
            // Parse rule from definition
            match serde_json::from_value(req.definition) {
                Ok(rule) => Automation::Rule(rule),
                Err(e) => {
                    return Err(ErrorResponse::bad_request(format!("Invalid rule definition: {}", e)));
                }
            }
        }
    };

    // Save the automation
    match store.save_automation(&automation).await {
        Ok(_) => {
            let dto = AutomationDto::from(automation.clone());
            ok(json!({
                "automation": dto,
                "message": "Automation created successfully",
            }))
        }
        Err(e) => {
            tracing::error!("Error creating automation: {}", e);
            Err(ErrorResponse::internal(format!("Failed to create automation: {}", e)))
        }
    }
}

/// Update an automation.
///
/// PUT /api/automations/:id
pub async fn update_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
    Json(req): Json<UpdateAutomationRequest>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Get existing automation
    let existing = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Automation not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!("Failed to get automation: {}", e)));
        }
    };

    // Update the automation
    let updated = match existing {
        Automation::Transform(mut transform) => {
            if let Some(name) = req.name {
                transform.metadata.name = name;
            }
            if let Some(description) = req.description {
                transform.metadata.description = description;
            }
            if let Some(enabled) = req.enabled {
                transform.metadata.enabled = enabled;
            }
            Automation::Transform(transform)
        }
        Automation::Rule(mut rule) => {
            if let Some(name) = req.name {
                rule.metadata.name = name;
            }
            if let Some(description) = req.description {
                rule.metadata.description = description;
            }
            if let Some(enabled) = req.enabled {
                rule.metadata.enabled = enabled;
            }
            // Definition updates would require parsing the new definition
            Automation::Rule(rule)
        }
    };

    // Save the updated automation
    match store.save_automation(&updated).await {
        Ok(_) => {
            let dto = AutomationDto::from(updated.clone());
            ok(json!({
                "automation": dto,
                "message": "Automation updated successfully",
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to update automation: {}", e)))
        }
    }
}

/// Delete an automation.
///
/// DELETE /api/automations/:id
pub async fn delete_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    match store.delete_automation(&id).await {
        Ok(true) => {
            ok(json!({
                "message": "Automation deleted successfully",
            }))
        }
        Ok(false) => {
            Err(ErrorResponse::not_found("Automation not found"))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to delete automation: {}", e)))
        }
    }
}

/// Set automation enabled status.
///
/// POST /api/automations/:id/enable
pub async fn set_automation_status_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
    Json(req): Json<SetAutomationStatusRequest>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Get existing automation
    let mut existing = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Automation not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!("Failed to get automation: {}", e)));
        }
    };

    // Update enabled status
    match &mut existing {
        Automation::Transform(transform) => {
            transform.metadata.enabled = req.enabled;
        }
        Automation::Rule(rule) => {
            rule.metadata.enabled = req.enabled;
        }
    }

    // Save the updated automation
    match store.save_automation(&existing).await {
        Ok(_) => {
            ok(json!({
                "message": format!("Automation {}", if req.enabled { "enabled" } else { "disabled" }),
                "enabled": req.enabled,
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to update automation: {}", e)))
        }
    }
}

/// Analyze intent to recommend automation type.
///
/// POST /api/automations/analyze-intent
pub async fn analyze_intent_handler(
    State(state): State<ServerState>,
    Json(req): Json<AnalyzeIntentRequest>,
) -> HandlerResult<IntentResult> {
    // Use intent analyzer if available, otherwise use heuristic analysis
    let result = if let Some(analyzer) = &state.intent_analyzer {
        match analyzer.analyze(&req.description).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Error analyzing intent: {}", e);
                // Fallback to heuristic analysis
                heuristic_analysis(&req.description)
            }
        }
    } else {
        // Intent analyzer not initialized, use heuristic analysis
        heuristic_analysis(&req.description)
    };

    ok(result)
}

/// Quick heuristic analysis for intent classification (used when LLM is not available)
fn heuristic_analysis(description: &str) -> IntentResult {
    use edge_ai_automation::AutomationType;

    let desc_lower = description.to_lowercase();

    // Transform indicators (data processing keywords)
    let transform_keywords = [
        "calculate", "compute", "aggregate", "average", "sum", "count",
        "extract", "parse", "transform", "convert", "process",
        "statistics", "metric", "virtual", "derived",
        "array", "group by", "filter", "map",
    ];

    // Rule indicators (simple if-then)
    let rule_keywords = [
        "when", "if", "then", "trigger", "activates",
        "exceeds", "below", "above", "equals", "threshold",
        "sensor", "detects", "monitors", "alert",
        "send", "notify", "execute", "command",
    ];

    let mut transform_score = 0i32;
    let mut rule_score = 0i32;

    // Check for transform indicators
    for keyword in &transform_keywords {
        if desc_lower.contains(keyword) {
            transform_score += 5;
        }
    }

    // Check for rule indicators
    for keyword in &rule_keywords {
        if desc_lower.contains(keyword) {
            rule_score += 5;
        }
    }

    // Check for device/action patterns (suggests rule)
    if desc_lower.contains("device") || desc_lower.contains("send") {
        rule_score += 10;
    }

    // Check for data processing patterns (suggests transform)
    if desc_lower.contains("data") || desc_lower.contains("value") {
        transform_score += 5;
    }

    // Determine result
    let (recommended_type, confidence, reasoning, warnings) =
        if transform_score > rule_score + 10 {
            (
                AutomationType::Transform,
                (transform_score - rule_score).min(100) as u8,
                "This appears to be a data processing automation".to_string(),
                vec![]
            )
        } else if rule_score > transform_score + 10 {
            (
                AutomationType::Rule,
                (rule_score - transform_score).min(100) as u8,
                "This appears to be a conditional automation".to_string(),
                vec![]
            )
        } else {
            // Close call - default to rule for automation use cases
            (
                AutomationType::Rule,
                55,
                "This could be either a transform or rule - defaulting to rule".to_string(),
                vec!["Moderate confidence - consider specifying the type explicitly".to_string()]
            )
        };

    IntentResult {
        recommended_type,
        confidence,
        reasoning,
        suggested_automation: None,
        warnings,
    }
}

/// Get conversion recommendation for an automation.
///
/// GET /api/automations/:id/conversion-info
pub async fn get_conversion_info_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    match store.get_automation(&id).await {
        Ok(Some(automation)) => {
            let recommendation = AutomationConverter::get_conversion_recommendation(&automation);
            ok(json!({
                "automation_id": id,
                "current_type": automation.automation_type(),
                "can_convert": recommendation.can_convert,
                "target_type": recommendation.target_type,
                "reason": recommendation.reason,
                "estimated_complexity": recommendation.estimated_complexity,
            }))
        }
        Ok(None) => {
            Err(ErrorResponse::not_found("Automation not found"))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to get automation: {}", e)))
        }
    }
}

/// Convert an automation between types.
///
/// POST /api/automations/:id/convert
pub async fn convert_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
    Json(req): Json<ConvertAutomationRequest>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Get existing automation
    let existing = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Automation not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!("Failed to get automation: {}", e)));
        }
    };

    // Perform conversion - only Transform <-> Rule conversion is supported
    let converted = match (existing, req.r#type) {
        (Automation::Transform(_), AutomationType::Transform) => {
            return Err(ErrorResponse::bad_request("Cannot convert to the same type"));
        }
        (Automation::Rule(_), AutomationType::Rule) => {
            return Err(ErrorResponse::bad_request("Cannot convert to the same type"));
        }
        (Automation::Transform(_), AutomationType::Rule) => {
            return Err(ErrorResponse::bad_request("Transform to Rule conversion is not directly supported. Create a new Rule based on the Transform's output metrics."));
        }
        (Automation::Rule(_), AutomationType::Transform) => {
            return Err(ErrorResponse::bad_request("Rule to Transform conversion is not supported. Transforms are for data processing, not reactive automation."));
        }
    };

    // Save the converted automation (with a new ID to preserve the original)
    let new_id = format!("{}-converted", id);
    let converted_with_new_id = match converted {
        Automation::Transform(mut transform) => {
            transform.metadata.id = new_id.clone();
            transform.metadata.name = format!("{} (converted)", transform.metadata.name);
            Automation::Transform(transform)
        }
        Automation::Rule(mut rule) => {
            rule.metadata.id = new_id.clone();
            rule.metadata.name = format!("{} (converted)", rule.metadata.name);
            Automation::Rule(rule)
        }
    };

    match store.save_automation(&converted_with_new_id).await {
        Ok(_) => {
            let dto = AutomationDto::from(converted_with_new_id.clone());
            ok(json!({
                "automation": dto,
                "message": "Automation converted successfully",
                "original_id": id,
                "new_id": new_id,
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to save converted automation: {}", e)))
        }
    }
}

/// Get execution history for an automation.
///
/// GET /api/automations/:id/executions?limit=10
pub async fn get_automations_executions_handler(
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10);

    match store.get_executions(&id, limit).await {
        Ok(executions) => {
            ok(json!({
                "automation_id": id,
                "executions": executions,
                "count": executions.len(),
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to get executions: {}", e)))
        }
    }
}

/// List all automation templates.
///
/// GET /api/automations/templates
pub async fn list_templates_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return ok(json!({
            "templates": [],
            "count": 0,
        }));
    };

    match store.list_templates() {
        Ok(templates) => {
            ok(json!({
                "templates": templates,
                "count": templates.len(),
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to list templates: {}", e)))
        }
    }
}

/// Export all automations.
///
/// GET /api/automations/export
pub async fn export_automations_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return ok(json!({
            "automations": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(automations) => {
            ok(json!({
                "automations": automations,
                "count": automations.len(),
                "exported_at": chrono::Utc::now().to_rfc3339(),
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to export automations: {}", e)))
        }
    }
}

/// Import automations.
///
/// POST /api/automations/import
pub async fn import_automations_handler(
    State(state): State<ServerState>,
    Json(data): Json<Value>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    let automations: Vec<Automation> = match serde_json::from_value(data["automations"].clone()) {
        Ok(a) => a,
        Err(e) => {
            return Err(ErrorResponse::bad_request(format!("Invalid automations data: {}", e)));
        }
    };

    let mut imported = 0;
    let mut failed = 0;

    for automation in automations {
        match store.save_automation(&automation).await {
            Ok(_) => imported += 1,
            Err(_) => failed += 1,
        }
    }

    ok(json!({
        "message": "Import completed",
        "imported": imported,
        "failed": failed,
    }))
}

// ========== Transform-Specific Handlers ==========

/// Request body for processing data through transforms.
#[derive(Debug, Deserialize)]
pub struct ProcessDataRequest {
    /// Device ID that produced the data
    pub device_id: String,
    /// Device type (optional, for better transform matching)
    pub device_type: Option<String>,
    /// Raw data from the device (JSON)
    pub data: Value,
    /// Timestamp of the data (defaults to now)
    #[serde(default = "default_timestamp")]
    pub timestamp: i64,
}

fn default_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Process device data through all applicable transforms.
///
/// POST /api/automations/transforms/process
pub async fn process_data_handler(
    State(state): State<ServerState>,
    Json(req): Json<ProcessDataRequest>,
) -> HandlerResult<Value> {
    let Some(transform_engine) = &state.transform_engine else {
        return Err(ErrorResponse::service_unavailable("Transform engine not available"));
    };

    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Load all transforms
    let transforms = match store.list_automations().await {
        Ok(automations) => automations
            .into_iter()
            .filter_map(|a| match a {
                Automation::Transform(t) => Some(t),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::error!("Failed to load transforms: {}", e);
            Vec::new()
        }
    };

    // Process data through transforms
    let result = transform_engine
        .process_device_data(
            &transforms,
            &req.device_id,
            req.device_type.as_deref(),
            &req.data,
        )
        .await;

    match result {
        Ok(transform_result) => {
            tracing::debug!(
                "Processed data for device {}: {} metrics produced",
                req.device_id,
                transform_result.metrics.len()
            );

            // Publish transformed metrics to event bus
            if let Some(event_bus) = &state.event_bus {
                for metric in &transform_result.metrics {
                    // Publish as a device metric event
                    use edge_ai_core::NeoTalkEvent;
                    if let Ok(_event_json) = serde_json::to_value(metric) {
                        let _ = event_bus.publish(NeoTalkEvent::DeviceMetric {
                            device_id: metric.device_id.clone(),
                            metric: metric.metric.clone(),
                            value: edge_ai_core::MetricValue::Float(metric.value),
                            timestamp: metric.timestamp,
                            quality: metric.quality,
                        });
                    }
                }
            }

            ok(json!({
                "success": true,
                "metrics": transform_result.metrics,
                "count": transform_result.metrics.len(),
                "warnings": transform_result.warnings,
            }))
        }
        Err(e) => {
            tracing::error!("Transform processing error: {}", e);
            Err(ErrorResponse::internal(format!("Transform processing failed: {}", e)))
        }
    }
}

/// Test a transform with sample data.
///
/// POST /api/automations/transforms/:id/test
pub async fn test_transform_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
    Json(req): Json<ProcessDataRequest>,
) -> HandlerResult<Value> {
    let Some(transform_engine) = &state.transform_engine else {
        return Err(ErrorResponse::service_unavailable("Transform engine not available"));
    };

    let Some(store) = &state.automation_store else {
        return Err(ErrorResponse::service_unavailable("Automation store not available"));
    };

    // Load the specific transform
    let automation = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Transform not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!("Failed to load transform: {}", e)));
        }
    };

    let transform = match automation {
        Automation::Transform(t) => t,
        _ => {
            return Err(ErrorResponse::bad_request("Automation is not a Transform"));
        }
    };

    // Process data through this specific transform
    let result = transform_engine
        .process_device_data(
            &[transform],
            &req.device_id,
            req.device_type.as_deref(),
            &req.data,
        )
        .await;

    match result {
        Ok(transform_result) => {
            ok(json!({
                "transform_id": id,
                "metrics": transform_result.metrics,
                "count": transform_result.metrics.len(),
                "warnings": transform_result.warnings,
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Transform test failed: {}", e)))
        }
    }
}

/// Get list of all transforms (filtering by type).
///
/// GET /api/automations/transforms
pub async fn list_transforms_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return ok(json!({
            "transforms": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(automations) => {
            let transforms: Vec<_> = automations
                .into_iter()
                .filter_map(|a| match a {
                    Automation::Transform(t) => Some(t),
                    _ => None,
                })
                .collect();

            ok(json!({
                "transforms": transforms,
                "count": transforms.len(),
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to list transforms: {}", e)))
        }
    }
}

/// Get virtual metrics generated by transforms.
///
/// GET /api/automations/transforms/metrics
pub async fn list_virtual_metrics_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation_store else {
        return ok(json!({
            "metrics": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(automations) => {
            use std::collections::HashMap;

            let mut metrics_map: HashMap<String, Vec<String>> = HashMap::new();

            for automation in automations {
                if let Automation::Transform(transform) = automation {
                    // Check for JS-based transforms
                    if transform.js_code.is_some() && !transform.output_prefix.is_empty() {
                        metrics_map
                            .entry(transform.output_prefix.clone())
                            .or_default()
                            .push(format!("{}:{}", transform.metadata.id, transform.metadata.name));
                    }
                    // Check for legacy operation-based transforms
                    if let Some(ref operations) = transform.operations {
                        for operation in operations {
                            let output_metrics = operation.output_metrics();
                            for metric in output_metrics {
                                metrics_map
                                    .entry(metric.clone())
                                    .or_default()
                                    .push(format!("{}:{}", transform.metadata.id, transform.metadata.name));
                            }
                        }
                    }
                }
            }

            ok(json!({
                "metrics": metrics_map,
                "count": metrics_map.len(),
            }))
        }
        Err(e) => {
            Err(ErrorResponse::internal(format!("Failed to list virtual metrics: {}", e)))
        }
    }
}
