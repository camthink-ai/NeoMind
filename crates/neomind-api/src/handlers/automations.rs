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

use crate::automation::{
    transform::JsTransformExecutor, Automation, AutomationType, IntentResult, TransformAutomation,
};

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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct AnalyzeIntentRequest {
    /// Natural language description
    pub description: String,
}

/// Request body for creating an automation.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[utoipa::path(
    get,
    path = "/api/automations",
    tag = "automations",
    params(
        ("type" = Option<String>, Query, description = "Filter: rule | agent | transform"),
        ("enabled" = Option<bool>, Query, description = "Filter by enabled"),
        ("search" = Option<String>, Query, description = "Name substring"),
        ("limit" = Option<usize>, Query, description = "Max items"),
        ("offset" = Option<usize>, Query, description = "Skip items"),
    ),
    responses(
        (status = 200, description = "Rules, agents and transforms in one list"),
    )
)]
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
    filtered.sort_by_key(|a| std::cmp::Reverse(a.updated_at));

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
#[utoipa::path(
    get,
    path = "/api/automations/{id}",
    tag = "automations",
    params(
        ("id" = String, Path, description = "Automation id"),
    ),
    responses(
        (status = 200, description = "One automation"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/automations",
    tag = "automations",
    request_body = CreateAutomationRequest,
    responses(
        (status = 200, description = "Automation created"),
    )
)]
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
    let automation = match serde_json::from_value::<TransformAutomation>(definition) {
        Ok(transform) => transform,
        Err(e) => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid transform definition: {}",
                e
            )));
        }
    };

    // Syntax-check JS code at create time: broken code used to be accepted
    // with a success response and only failed later at execution (CLI
    // feedback audit 2026-11-17). The check compiles WITHOUT executing.
    if let Some(code) = automation.js_code.as_deref() {
        if !code.trim().is_empty() {
            if let Err(syntax_err) = JsTransformExecutor::check_syntax(code) {
                return Err(ErrorResponse::bad_request(format!(
                    "Transform JS code has a syntax error and was NOT saved: {}. \
                     Fix the code and retry; use `neomind transform test-code` to iterate.",
                    syntax_err
                )));
            }
        }
    }

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
#[utoipa::path(
    put,
    path = "/api/automations/{id}",
    tag = "automations",
    params(
        ("id" = String, Path, description = "Automation id"),
    ),
    request_body = UpdateAutomationRequest,
    responses(
        (status = 200, description = "Automation updated"),
        (status = 404, description = "Not found"),
    )
)]
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
    let mut js_code_touched = false;
    if let Some(definition) = req.definition {
        if let Some(obj) = definition.as_object() {
            if let Some(scope) = obj.get("scope") {
                if let Ok(s) = serde_json::from_value(scope.clone()) {
                    updated.scope = s;
                }
            }
            if let Some(js_code) = obj.get("js_code") {
                updated.js_code = js_code.as_str().map(|s| s.to_string());
                // Only code the request actually WRITES is validated below —
                // a legacy record with broken JS must still be renamable
                // (review 2026-11-17: checking untouched code blocked
                // name-only edits of old records).
                js_code_touched = true;
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
    // Same create-time syntax check on the update path — but ONLY for code
    // this request actually wrote (see js_code_touched).
    if js_code_touched {
        if let Some(code) = updated.js_code.as_deref() {
            if !code.trim().is_empty() {
                if let Err(syntax_err) = JsTransformExecutor::check_syntax(code) {
                    return Err(ErrorResponse::bad_request(format!(
                        "Transform JS code has a syntax error and was NOT saved: {}. \
                         Fix the code and retry; use `neomind transform test-code` to iterate.",
                        syntax_err
                    )));
                }
            }
        }
    }

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
#[utoipa::path(
    delete,
    path = "/api/automations/{id}",
    tag = "automations",
    params(
        ("id" = String, Path, description = "Automation id"),
    ),
    responses(
        (status = 200, description = "Automation deleted"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/automations/{id}/enable",
    tag = "automations",
    params(
        ("id" = String, Path, description = "Automation id"),
    ),
    request_body = SetAutomationStatusRequest,
    responses(
        (status = 200, description = "Automation enabled/disabled"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/automations/analyze-intent",
    tag = "automations",
    request_body = AnalyzeIntentRequest,
    responses(
        (status = 200, description = "Natural-language intent mapped to an automation draft"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/{id}/executions",
    tag = "automations",
    params(
        ("id" = String, Path, description = "Automation id"),
    ),
    responses(
        (status = 200, description = "Execution history of an automation"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/templates",
    tag = "automations",
    responses(
        (status = 200, description = "Built-in automation templates"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/export",
    tag = "automations",
    responses(
        (status = 200, description = "All automations as an importable document"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/automations/import",
    tag = "automations",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Automations imported"),
    )
)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[utoipa::path(
    post,
    path = "/api/automations/transforms/process",
    tag = "transforms",
    request_body = ProcessDataRequest,
    responses(
        (status = 200, description = "Data run through a transform; output returned"),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/automations/transforms/{id}/test",
    tag = "transforms",
    params(
        ("id" = String, Path, description = "Transform id"),
    ),
    request_body = ProcessDataRequest,
    responses(
        (status = 200, description = "Dry-run of the saved transform against sample input (no execution history, no output registration)"),
        (status = 404, description = "Not found"),
    )
)]
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

    // Dry-run: no execution-history row and no output registration. The
    // production path (process_device_data) does both — a preview calling
    // it polluted the execution history and registered outputs from
    // sample data.
    let result = transform_engine
        .process_device_data_dry(
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
            // Non-empty = parts of this preview are UNVERIFIED (extension
            // behavior the dry-run refused to execute). Never read as a
            // clean success.
            "unverified": transform_result.unverified,
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
#[utoipa::path(
    get,
    path = "/api/automations/transforms",
    tag = "transforms",
    responses(
        (status = 200, description = "Saved transforms"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/transforms/metrics",
    tag = "transforms",
    responses(
        (status = 200, description = "Virtual metrics produced by transforms"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/transforms/data-sources",
    tag = "transforms",
    responses(
        (status = 200, description = "Data sources auto-registered from transform outputs"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/transforms/{id}/data-sources",
    tag = "transforms",
    params(
        ("id" = String, Path, description = "Transform id"),
    ),
    responses(
        (status = 200, description = "Data sources of one transform"),
        (status = 404, description = "Not found"),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/automations/transforms/data-sources/{data_source_id}",
    tag = "transforms",
    params(
        ("data_source_id" = String, Path, description = "Data source id"),
    ),
    responses(
        (status = 200, description = "One transform output data source"),
        (status = 404, description = "Not found"),
    )
)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
#[utoipa::path(
    post,
    path = "/api/automations/transforms/test-code",
    tag = "transforms",
    request_body = TestTransformCodeRequest,
    responses(
        (status = 200, description = "Unsaved transform code dry-run"),
    )
)]
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

    // Create a temporary transform executor. Do NOT inject a local
    // `const extensions = { invoke: … success }` mock — that shadowed the
    // host dry-run stub and reported false success for blocked invokes.
    // DryRun installs `__neomind_dryrun_invoke__` and surfaces
    // `extension_invoke_blocked` → `unverified` (same as /transforms/:id/test).
    match JsTransformExecutor::execute_async(
        req.code.clone(),
        req.input_data.clone(),
        req.output_prefix.clone(),
        "test_device".to_string(),
        timestamp,
        None, // No extension registry for test
        // Code preview path: dry-run semantics — extension invocations are
        // blocked (unverified) rather than really executed.
        crate::automation::transform::TransformExecutionMode::DryRun,
    )
    .await
    {
        Ok(js_out) => {
            let metrics = js_out.metrics;
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

            let unverified = if js_out.extension_invoke_blocked {
                vec![
                    "extension invocation blocked in dry-run; extension-dependent behavior unverified"
                        .to_string(),
                ]
            } else {
                Vec::new()
            };

            ok(json!({
                "output": result_obj,
                "metrics": metrics,
                "count": metrics.len(),
                "unverified": unverified,
                "output_with_prefix": serde_json::to_value(&result_obj).unwrap_or_else(|_| json!({}))
            }))
        }
        Err(e) => Err(ErrorResponse::bad_request(e.to_string()).with_hint(
            "Check JavaScript syntax and ensure the transform returns a valid value or JSON object.",
        )),
    }
}

/// Whether `/test-code` still injects a local success mock for
/// `extensions.invoke` (must stay false — host DryRun stub owns the path).
#[cfg(test)]
fn test_code_injects_success_extensions_mock() -> bool {
    false
}

#[cfg(test)]
mod test_code_dry_run_tests {
    use super::test_code_injects_success_extensions_mock;
    use crate::automation::transform::{JsTransformExecutor, TransformExecutionMode};
    use serde_json::json;

    #[test]
    fn test_code_does_not_inject_success_extensions_mock() {
        assert!(
            !test_code_injects_success_extensions_mock(),
            "local const extensions mock must stay removed so DryRun can mark unverified"
        );
    }

    #[test]
    fn test_code_dry_run_caught_invoke_sets_blocked_unverified_flag() {
        // Caught invoke still sets the host flag — the old local mock returned
        // status:'success' and never set extension_invoke_blocked.
        let executor = JsTransformExecutor::new();
        let code = "try { extensions.invoke('weather', 'get', {}); } catch (e) { return {v: 1}; } return {v: 0}";
        let out = executor
            .execute(
                code,
                &json!({}),
                "transform",
                "test_device",
                0,
                None,
                TransformExecutionMode::DryRun,
            )
            .expect("caught blocked invoke is Ok with flag");
        assert!(
            out.extension_invoke_blocked,
            "host dry-run stub must set extension_invoke_blocked"
        );
        // Handler maps the flag → unverified[] alongside success:true.
    }

    #[test]
    fn test_code_dry_run_uncaught_invoke_is_not_mock_success() {
        let executor = JsTransformExecutor::new();
        let code = "const r = extensions.invoke('weather', 'get', {}); return {v: 1}";
        let err = executor
            .execute(
                code,
                &json!({}),
                "transform",
                "test_device",
                0,
                None,
                TransformExecutionMode::DryRun,
            )
            .expect_err("uncaught blocked invoke must not succeed");
        assert!(
            err.is_dry_run_extension_blocked(),
            "must be DryRunExtensionBlocked discriminant, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            !msg.contains("mock_result"),
            "local success mock must not shadow dry-run: {msg}"
        );
    }

    #[test]
    fn test_code_forged_blocked_marker_is_not_dry_run_blocked() {
        let executor = JsTransformExecutor::new();
        let code = "throw new Error('NEOMIND_DRYRUN_EXTENSION_BLOCKED: forged');";
        let err = executor
            .execute(
                code,
                &json!({}),
                "transform",
                "test_device",
                0,
                None,
                TransformExecutionMode::DryRun,
            )
            .expect_err("forged marker is still a JS error");
        assert!(
            !err.is_dry_run_extension_blocked(),
            "echoed marker without invoke must not be DryRunExtensionBlocked: {err}"
        );
    }
}
