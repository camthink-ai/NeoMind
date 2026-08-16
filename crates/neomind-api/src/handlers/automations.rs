//! Unified automations API handlers.
//!
//! This module provides a unified API for transforms and rules,
//! allowing them to be managed through a single interface.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::automation::{transform::JsTransformExecutor, Automation, AutomationType, IntentResult};

use super::{
    common::{ok, HandlerResult},
    ServerState,
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
    /// Pagination: limit number of results (default: 50, max: 1000)
    pub limit: Option<usize>,
    /// Pagination: skip N results (default: 0)
    pub offset: Option<usize>,
}

/// Automation type filter.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationTypeFilter {
    Transform,
    All,
}

impl From<AutomationTypeFilter> for Option<AutomationType> {
    fn from(filter: AutomationTypeFilter) -> Self {
        match filter {
            AutomationTypeFilter::Transform => Some(AutomationType::Transform),
            AutomationTypeFilter::All => None,
        }
    }
}

/// Automation list response.
#[derive(Debug, Serialize)]
pub struct AutomationListResponse {
    /// List of automations
    pub automations: Vec<AutomationDto>,
    /// Number of items in this page
    pub count: usize,
    /// Total number of items (before pagination)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    /// Pagination metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<PaginationMeta>,
}

/// Pagination metadata.
#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
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

/// Convert automation to DTO.
impl From<Automation> for AutomationDto {
    fn from(transform: Automation) -> Self {
        Self {
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
        }
    }
}

/// List all automations.
///
/// GET /api/automations?type=transform|rule|all&enabled=true|false&search=query&limit=50&offset=0
///
/// Performance optimization: Supports pagination to avoid loading all automations into memory.
pub async fn list_automations_handler(
    Query(filter): Query<AutomationFilter>,
    State(state): State<ServerState>,
) -> HandlerResult<AutomationListResponse> {
    let Some(store) = &state.automation.automation_store else {
        return ok(AutomationListResponse {
            automations: Vec::new(),
            count: 0,
            total: None,
            pagination: None,
        });
    };

    // Enforce reasonable pagination limits
    let limit = filter.limit.unwrap_or(50).min(1000); // Default 50, max 1000
    let offset = filter.offset.unwrap_or(0);

    let automations = store.list_automations().await.unwrap_or_default();

    let mut filtered: Vec<_> = automations
        .into_iter()
        .filter(|a| {
            // Filter by enabled status
            if let Some(enabled) = filter.enabled {
                if a.is_enabled() != enabled {
                    return false;
                }
            }

            // Search in name/description
            if let Some(search) = &filter.search {
                let search_lower = search.to_lowercase();
                let name_matches = a.name().to_lowercase().contains(&search_lower);
                let desc_matches = a
                    .metadata
                    .description
                    .to_lowercase()
                    .contains(&search_lower);
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

    let total = filtered.len();
    let has_more = offset + limit < total;

    // Apply pagination
    let paginated: Vec<_> = filtered.into_iter().skip(offset).take(limit).collect();

    let count = paginated.len();

    // Include pagination metadata only when pagination is explicitly requested
    let (total_res, pagination_res) = if filter.limit.is_some() || filter.offset.is_some() {
        (
            Some(total),
            Some(PaginationMeta {
                limit,
                offset,
                has_more,
            }),
        )
    } else {
        (None, None)
    };

    ok(AutomationListResponse {
        automations: paginated,
        count,
        total: total_res,
        pagination: pagination_res,
    })
}

/// Get a specific automation by ID.
///
/// GET /api/automations/:id
pub async fn get_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    match store.get_automation(&id).await {
        Ok(Some(automation)) => {
            let dto = AutomationDto::from(automation.clone());
            ok(json!({
                "automation": dto,
                "definition": automation,
            }))
        }
        Ok(None) => Err(ErrorResponse::not_found("Automation not found")),
        Err(e) => {
            tracing::error!("Error getting automation: {}", e);
            Err(ErrorResponse::internal(format!(
                "Failed to get automation: {}",
                e
            )))
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
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    // Generate a unique ID for new automations
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    // Start with base definition and add required fields
    let mut definition = req.definition.clone();

    // Ensure id and timestamps are set
    if let Some(obj) = definition.as_object_mut() {
        obj.insert("id".to_string(), json!(id));
        obj.insert("name".to_string(), json!(req.name));
        obj.insert("description".to_string(), json!(req.description));
        obj.insert("enabled".to_string(), json!(req.enabled));
        obj.insert("created_at".to_string(), json!(now));
        obj.insert("updated_at".to_string(), json!(now));
    }

    // Parse transform from definition
    let automation = match serde_json::from_value(definition) {
        Ok(transform) => transform,
        Err(e) => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid transform definition: {}",
                e
            )));
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
            Err(ErrorResponse::internal(format!(
                "Failed to create automation: {}",
                e
            )))
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
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    // Get existing automation
    let existing = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Automation not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to get automation: {}",
                e
            )));
        }
    };

    // Update the automation
    let mut updated = existing;
    if let Some(name) = req.name {
        updated.metadata.name = name;
    }
    if let Some(description) = req.description {
        updated.metadata.description = description;
    }
    if let Some(enabled) = req.enabled {
        updated.metadata.enabled = enabled;
    }

    // Merge definition fields if provided
    if let Some(definition) = req.definition {
        if let Some(obj) = definition.as_object() {
            if let Some(scope) = obj.get("scope") {
                if let Ok(s) = serde_json::from_value(scope.clone()) {
                    updated.scope = s;
                }
            }
            if let Some(js_code) = obj.get("js_code") {
                updated.js_code = js_code.as_str().map(|s| s.to_string());
            }
            if let Some(output_prefix) = obj.get("output_prefix") {
                if let Some(s) = output_prefix.as_str() {
                    updated.output_prefix = s.to_string();
                }
            }
            if let Some(operations) = obj.get("operations") {
                if let Ok(ops) = serde_json::from_value(operations.clone()) {
                    updated.operations = Some(ops);
                }
            }
            if let Some(intent) = obj.get("intent") {
                updated.intent = intent.as_str().map(|s| s.to_string());
            }
        }
    }

    updated.metadata.updated_at = chrono::Utc::now().timestamp();

    // Save the updated automation
    match store.save_automation(&updated).await {
        Ok(_) => {
            let dto = AutomationDto::from(updated.clone());
            ok(json!({
                "automation": dto,
                "message": "Automation updated successfully",
            }))
        }
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to update automation: {}",
            e
        ))),
    }
}

/// Delete an automation.
///
/// DELETE /api/automations/:id
pub async fn delete_automation_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    match store.delete_automation(&id).await {
        Ok(true) => ok(json!({
            "message": "Automation deleted successfully",
        })),
        Ok(false) => Err(ErrorResponse::not_found("Automation not found")),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to delete automation: {}",
            e
        ))),
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
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    // Get existing automation
    let mut existing = match store.get_automation(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Automation not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to get automation: {}",
                e
            )));
        }
    };

    // Update enabled status
    existing.metadata.enabled = req.enabled;

    // Save the updated automation
    match store.save_automation(&existing).await {
        Ok(_) => ok(json!({
            "message": format!("Automation {}", if req.enabled { "enabled" } else { "disabled" }),
            "enabled": req.enabled,
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to update automation: {}",
            e
        ))),
    }
}

/// Analyze intent to recommend automation type.
///
/// POST /api/automations/analyze-intent
pub async fn analyze_intent_handler(
    State(_state): State<ServerState>,
    Json(req): Json<AnalyzeIntentRequest>,
) -> HandlerResult<IntentResult> {
    // Use heuristic analysis (LLM-based intent analysis removed for simplification)
    let result = heuristic_analysis(&req.description);
    ok(result)
}

/// Quick heuristic analysis for intent classification
fn heuristic_analysis(description: &str) -> IntentResult {
    use crate::automation::AutomationType;

    let desc_lower = description.to_lowercase();

    // Transform indicators (data processing keywords)
    let transform_keywords = [
        "calculate",
        "compute",
        "aggregate",
        "average",
        "sum",
        "count",
        "extract",
        "parse",
        "transform",
        "convert",
        "process",
        "statistics",
        "metric",
        "virtual",
        "derived",
        "array",
        "group by",
        "filter",
        "map",
    ];

    let mut transform_score = 0i32;

    for keyword in &transform_keywords {
        if desc_lower.contains(keyword) {
            transform_score += 5;
        }
    }

    if desc_lower.contains("data") || desc_lower.contains("value") {
        transform_score += 5;
    }

    // Always recommend Transform (rules are managed via neomind-rules)
    let confidence = (50 + transform_score).min(100) as u8;

    IntentResult {
        recommended_type: AutomationType::Transform,
        confidence,
        reasoning: "Recommended as a data transform automation".to_string(),
        suggested_automation: None,
        warnings: vec![],
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
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10);

    match store.get_executions(&id, limit).await {
        Ok(executions) => ok(json!({
            "automation_id": id,
            "executions": executions,
            "count": executions.len(),
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to get executions: {}",
            e
        ))),
    }
}

/// List all automation templates.
///
/// GET /api/automations/templates
pub async fn list_templates_handler(State(state): State<ServerState>) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return ok(json!({
            "templates": [],
            "count": 0,
        }));
    };

    match store.list_templates() {
        Ok(templates) => ok(json!({
            "templates": templates,
            "count": templates.len(),
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to list templates: {}",
            e
        ))),
    }
}

/// Export all automations.
///
/// GET /api/automations/export
pub async fn export_automations_handler(State(state): State<ServerState>) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return ok(json!({
            "automations": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(automations) => ok(json!({
            "automations": automations,
            "count": automations.len(),
            "exported_at": chrono::Utc::now().to_rfc3339(),
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to export automations: {}",
            e
        ))),
    }
}

/// Import automations.
///
/// POST /api/automations/import
pub async fn import_automations_handler(
    State(state): State<ServerState>,
    Json(data): Json<Value>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    let automations: Vec<Automation> = match serde_json::from_value(data["automations"].clone()) {
        Ok(a) => a,
        Err(e) => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid automations data: {}",
                e
            )));
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
    let Some(transform_engine) = &state.automation.transform_engine else {
        return Err(ErrorResponse::service_unavailable(
            "Transform engine not available",
        ));
    };

    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    // Load all transforms
    let transforms = match store.list_automations().await {
        Ok(automations) => automations,
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

            // [no side effects] This endpoint used to publish the produced
            // metrics to the LIVE event bus as is_virtual device metrics —
            // but the rule value-provider listener does not skip virtual
            // metrics, so a manual "process" call overwrote live rule
            // values and could fire REAL rules (notifications, device
            // commands) from test data. test_transform_handler already
            // returns output without publishing; this now matches it.
            // Use the /test endpoint to preview, or let the normal
            // event-driven path produce virtual metrics for real.

            ok(json!({
                "success": true,
                "metrics": transform_result.metrics,
                "count": transform_result.metrics.len(),
                "warnings": transform_result.warnings,
            }))
        }
        Err(e) => {
            tracing::error!("Transform processing error: {}", e);
            Err(ErrorResponse::internal(format!(
                "Transform processing failed: {}",
                e
            )))
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
    let Some(transform_engine) = &state.automation.transform_engine else {
        return Err(ErrorResponse::service_unavailable(
            "Transform engine not available",
        ));
    };

    let Some(store) = &state.automation.automation_store else {
        return Err(ErrorResponse::service_unavailable(
            "Automation store not available",
        ));
    };

    // Load the specific transform
    let transform = match store.get_automation(&id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(ErrorResponse::not_found("Transform not found"));
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "Failed to load transform: {}",
                e
            )));
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
        Ok(transform_result) => ok(json!({
            "transform_id": id,
            "metrics": transform_result.metrics,
            "count": transform_result.metrics.len(),
            "warnings": transform_result.warnings,
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Transform test failed: {}",
            e
        ))),
    }
}

/// Get list of all transforms (filtering by type).
///
/// GET /api/automations/transforms
pub async fn list_transforms_handler(State(state): State<ServerState>) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return ok(json!({
            "transforms": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(transforms) => ok(json!({
            "transforms": transforms,
            "count": transforms.len(),
        })),
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to list transforms: {}",
            e
        ))),
    }
}

/// Get virtual metrics generated by transforms.
///
/// GET /api/automations/transforms/metrics
pub async fn list_virtual_metrics_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(store) = &state.automation.automation_store else {
        return ok(json!({
            "metrics": [],
            "count": 0,
        }));
    };

    match store.list_automations().await {
        Ok(transforms) => {
            use std::collections::HashMap;

            let mut metrics_map: HashMap<String, Vec<String>> = HashMap::new();

            for transform in transforms {
                // Check for JS-based transforms
                if transform.js_code.is_some() && !transform.output_prefix.is_empty() {
                    metrics_map
                        .entry(transform.output_prefix.clone())
                        .or_default()
                        .push(format!(
                            "{}:{}",
                            transform.metadata.id, transform.metadata.name
                        ));
                }
                // Check for legacy operation-based transforms
                if let Some(ref operations) = transform.operations {
                    for operation in operations {
                        let output_metrics = operation.output_metrics();
                        for metric in output_metrics {
                            metrics_map.entry(metric.clone()).or_default().push(format!(
                                "{}:{}",
                                transform.metadata.id, transform.metadata.name
                            ));
                        }
                    }
                }
            }

            ok(json!({
                "metrics": metrics_map,
                "count": metrics_map.len(),
            }))
        }
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to list virtual metrics: {}",
            e
        ))),
    }
}

// ============================================================================
// Transform Output Data Source API
// ============================================================================

/// Get Transform outputs as data sources.
///
/// This endpoint returns Transform outputs in a format compatible with
/// Extension data sources, allowing the frontend to use them interchangeably.
///
/// GET /api/automations/transforms/data-sources
pub async fn list_transform_data_sources_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(transform_engine) = &state.automation.transform_engine else {
        return ok(json!({
            "data_sources": [],
            "count": 0,
        }));
    };

    let registry = transform_engine.output_registry();
    let data_sources = registry.list_as_data_sources().await;

    ok(json!({
        "data_sources": data_sources,
        "count": data_sources.len(),
    }))
}

/// Get data sources for a specific Transform.
///
/// GET /api/automations/transforms/:id/data-sources
pub async fn get_transform_data_sources_handler(
    Path(id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(transform_engine) = &state.automation.transform_engine else {
        return ok(json!({
            "data_sources": [],
            "count": 0,
        }));
    };

    let registry = transform_engine.output_registry();
    let data_sources = registry.get_transform_outputs(&id).await;

    ok(json!({
        "transform_id": id,
        "data_sources": data_sources,
        "count": data_sources.len(),
    }))
}

/// Get a specific Transform output data source.
///
/// GET /api/automations/transforms/data-sources/:data_source_id
pub async fn get_transform_data_source_handler(
    Path(data_source_id): Path<String>,
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let Some(transform_engine) = &state.automation.transform_engine else {
        return Err(ErrorResponse::service_unavailable(
            "Transform engine not available",
        ));
    };

    let registry = transform_engine.output_registry();
    match registry.get_output(&data_source_id).await {
        Some(output) => ok(json!(output)),
        None => Err(ErrorResponse::not_found("Transform data source not found")),
    }
}

// ============================================================================
// Transform Code Testing API
// ============================================================================

/// Request body for testing transform code directly.
#[derive(Debug, Deserialize)]
pub struct TestTransformCodeRequest {
    /// JavaScript code to test
    pub code: String,
    /// Input data to test with (JSON)
    pub input_data: Value,
    /// Output prefix for metrics
    #[serde(default = "default_output_prefix")]
    pub output_prefix: String,
}

fn default_output_prefix() -> String {
    "transform".to_string()
}

/// Test transform code directly without saving.
///
/// This endpoint allows testing JavaScript code before creating a transform.
/// It executes the code in a sandboxed environment and returns the result.
///
/// POST /api/automations/transforms/test-code
pub async fn test_transform_code_handler(
    State(state): State<ServerState>,
    Json(req): Json<TestTransformCodeRequest>,
) -> HandlerResult<Value> {
    let Some(_transform_engine) = &state.automation.transform_engine else {
        return Err(ErrorResponse::service_unavailable(
            "Transform engine not available",
        ));
    };

    // Use the current time for the test
    let timestamp = Utc::now().timestamp();

    // Preprocess code to handle extensions.invoke calls
    let processed_code = preprocess_extensions_invoke(&req.code, &req.input_data, &state).await?;

    // Create a temporary transform executor
    let executor = JsTransformExecutor::new();

    // Execute the code
    match executor.execute(
        &processed_code,
        &req.input_data,
        &req.output_prefix,
        "test_device",
        timestamp,
        None, // No extension registry for test
    ) {
        Ok(metrics) => {
            // Convert metrics to a more readable format
            let result_obj: serde_json::Map<String, Value> = metrics
                .iter()
                .map(|m| {
                    // Remove prefix from key for cleaner output
                    let key = m
                        .metric
                        .strip_prefix(&format!("{}.", req.output_prefix))
                        .unwrap_or(&m.metric)
                        .to_string();
                    (key, serde_json::json!(m.value))
                })
                .collect();

            ok(json!({
                "success": true,
                "output": result_obj,
                "metrics": metrics,
                "count": metrics.len(),
                "output_with_prefix": serde_json::to_value(&result_obj).unwrap_or_else(|_| json!({}))
            }))
        }
        Err(e) => {
            let error_msg: String = e.to_string();
            ok(json!({
                "success": false,
                "error": error_msg
            }))
        }
    }
}

/// Preprocess JavaScript code to handle extensions.invoke calls.
///
/// This replaces extensions.invoke calls with mock results for testing purposes.
/// In production, the actual extension would be called.
async fn preprocess_extensions_invoke(
    code: &str,
    _input_data: &Value,
    _state: &ServerState,
) -> Result<String, ErrorResponse> {
    // Check if code contains extensions.invoke
    if !code.contains("extensions.invoke") {
        return Ok(code.to_string());
    }

    // Define a mock extensions object at the beginning of the code
    let mock_extensions = r#"
// Mock extensions.invoke for testing
const extensions = {
  invoke: (extId, command, args) => ({
    _mock: true,
    _extension_id: extId,
    _command: command,
    _args: args,
    result: 'mock_result_from_' + extId,
    input: args?.data || input,
    status: 'success'
  })
};
"#;

    // Inject the mock before the user code
    let processed = format!("{}\n{}", mock_extensions, code);

    Ok(processed)
}
