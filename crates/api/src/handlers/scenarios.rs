//! Scenario management handlers.

use axum::{extract::{Path, Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use edge_ai_scenarios::{
    ScenarioManager, Scenario, ScenarioId, ScenarioCategory, Environment, ScenarioTemplates,
};

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;

/// DTO for scenario responses.
#[derive(Debug, Serialize)]
struct ScenarioDto {
    id: String,
    name: String,
    description: String,
    devices: Vec<String>,
    rules: Vec<String>,
    category: String,
    environment: String,
    business_context: String,
    tags: Vec<String>,
    priority: u8,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

impl From<&Scenario> for ScenarioDto {
    fn from(s: &Scenario) -> Self {
        Self {
            id: s.id.to_string(),
            name: s.name.clone(),
            description: s.description.clone(),
            devices: s.devices.clone(),
            rules: s.rules.clone(),
            category: format!("{:?}", s.metadata.category),
            environment: format!("{:?}", s.metadata.environment),
            business_context: s.metadata.business_context.clone(),
            tags: s.metadata.tags.clone(),
            priority: s.metadata.priority,
            is_active: s.is_active,
            created_at: s.created_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// Request body for creating a scenario.
#[derive(Debug, Deserialize)]
pub struct CreateScenarioRequest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub business_context: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_priority() -> u8 { 5 }

/// Request body for updating a scenario.
#[derive(Debug, Deserialize)]
pub struct UpdateScenarioRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub environment: Option<String>,
    pub business_context: Option<String>,
    pub tags: Option<Vec<String>>,
    pub priority: Option<u8>,
    pub is_active: Option<bool>,
}

/// Request body for adding a device to a scenario.
#[derive(Debug, Deserialize)]
pub struct AddDeviceRequest {
    pub device_id: String,
}

/// Request body for adding a rule to a scenario.
#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    pub rule_id: String,
}

/// Global scenario manager.
fn get_scenario_manager() -> Arc<ScenarioManager> {
    use std::sync::OnceLock;
    static MANAGER: OnceLock<Arc<ScenarioManager>> = OnceLock::new();
    MANAGER.get_or_init(|| Arc::new(ScenarioManager::new())).clone()
}

/// Parse category from string.
fn parse_category(s: &str) -> ScenarioCategory {
    match s.to_lowercase().as_str() {
        "monitoring" | "监控" => ScenarioCategory::Monitoring,
        "alert" | "告警" => ScenarioCategory::Alert,
        "automation" | "自动化" => ScenarioCategory::Automation,
        "reporting" | "报表" => ScenarioCategory::Reporting,
        "control" | "控制" => ScenarioCategory::Control,
        "optimization" | "优化" => ScenarioCategory::Optimization,
        _ => ScenarioCategory::Monitoring,
    }
}

/// Parse environment from string.
fn parse_environment(s: &str) -> Environment {
    match s.to_lowercase().as_str() {
        "office" | "办公楼" => Environment::Office,
        "factory" | "工厂" => Environment::Factory,
        "datacenter" | "数据中心" => Environment::DataCenter,
        "smarthome" | "智能家居" => Environment::SmartHome,
        "outdoor" | "户外" => Environment::Outdoor,
        "laboratory" | "实验室" => Environment::Laboratory,
        _ => Environment::Other(s.to_string()),
    }
}

/// List all scenarios.
///
/// GET /api/scenarios
pub async fn list_scenarios_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenarios = manager.list_scenarios().await;
    let dtos: Vec<ScenarioDto> = scenarios.iter().map(ScenarioDto::from).collect();

    ok(json!({
        "scenarios": dtos,
        "count": dtos.len(),
    }))
}

/// List active scenarios.
///
/// GET /api/scenarios/active
pub async fn list_active_scenarios_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenarios = manager.list_active().await;
    let dtos: Vec<ScenarioDto> = scenarios.iter().map(ScenarioDto::from).collect();

    ok(json!({
        "scenarios": dtos,
        "count": dtos.len(),
    }))
}

/// Get scenario statistics.
///
/// GET /api/scenarios/stats
pub async fn get_scenario_stats_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let stats = manager.get_stats().await;

    ok(json!({
        "stats": {
            "total": stats.total,
            "active": stats.active,
            "inactive": stats.inactive,
            "by_category": stats.by_category,
            "by_environment": stats.by_environment,
        }
    }))
}

/// Get a scenario by ID.
///
/// GET /api/scenarios/:id
pub async fn get_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    let scenario = manager.get_scenario(&scenario_id).await
        .ok_or_else(|| ErrorResponse::not_found(format!("Scenario {}", id)))?;

    ok(json!({
        "scenario": ScenarioDto::from(&scenario),
    }))
}

/// Create a new scenario.
///
/// POST /api/scenarios
pub async fn create_scenario_handler(
    State(_state): State<ServerState>,
    Json(req): Json<CreateScenarioRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();

    let scenario = manager
        .create_scenario(req.name.clone(), req.description.clone())
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario created",
        "scenario": ScenarioDto::from(&scenario),
    }))
}

/// Update a scenario.
///
/// PUT /api/scenarios/:id
pub async fn update_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateScenarioRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    let mut scenario = manager.get_scenario(&scenario_id).await
        .ok_or_else(|| ErrorResponse::not_found(format!("Scenario {}", id)))?;

    // Update fields
    if let Some(name) = req.name {
        scenario.name = name;
    }
    if let Some(description) = req.description {
        scenario.description = description;
    }
    if let Some(category) = req.category {
        scenario.metadata.category = parse_category(&category);
    }
    if let Some(environment) = req.environment {
        scenario.metadata.environment = parse_environment(&environment);
    }
    if let Some(business_context) = req.business_context {
        scenario.metadata.business_context = business_context;
    }
    if let Some(tags) = req.tags {
        scenario.metadata.tags = tags;
    }
    if let Some(priority) = req.priority {
        scenario.metadata.priority = priority;
    }
    if let Some(is_active) = req.is_active {
        scenario.is_active = is_active;
    }

    manager.add_scenario(scenario.clone()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario updated",
        "scenario": ScenarioDto::from(&scenario),
    }))
}

/// Delete a scenario.
///
/// DELETE /api/scenarios/:id
pub async fn delete_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.remove_scenario(&scenario_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario deleted",
    }))
}

/// Activate a scenario.
///
/// POST /api/scenarios/:id/activate
pub async fn activate_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.activate(&scenario_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to activate scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario activated",
    }))
}

/// Deactivate a scenario.
///
/// POST /api/scenarios/:id/deactivate
pub async fn deactivate_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.deactivate(&scenario_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to deactivate scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario deactivated",
    }))
}

/// Add a device to a scenario.
///
/// POST /api/scenarios/:id/devices
pub async fn add_device_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<AddDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.add_device_to_scenario(&scenario_id, req.device_id.clone()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add device: {}", e)))?;

    ok(json!({
        "message": "Device added to scenario",
    }))
}

/// Remove a device from a scenario.
///
/// DELETE /api/scenarios/:id/devices/:device_id
pub async fn remove_device_handler(
    State(_state): State<ServerState>,
    Path((id, device_id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.remove_device_from_scenario(&scenario_id, &device_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to remove device: {}", e)))?;

    ok(json!({
        "message": "Device removed from scenario",
    }))
}

/// Add a rule to a scenario.
///
/// POST /api/scenarios/:id/rules
pub async fn add_rule_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<AddRuleRequest>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.add_rule_to_scenario(&scenario_id, req.rule_id.clone()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add rule: {}", e)))?;

    ok(json!({
        "message": "Rule added to scenario",
    }))
}

/// Remove a rule from a scenario.
///
/// DELETE /api/scenarios/:id/rules/:rule_id
pub async fn remove_rule_handler(
    State(_state): State<ServerState>,
    Path((id, rule_id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    manager.remove_rule_from_scenario(&scenario_id, &rule_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to remove rule: {}", e)))?;

    ok(json!({
        "message": "Rule removed from scenario",
    }))
}

/// List scenario templates.
///
/// GET /api/scenario-templates
pub async fn list_templates_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let templates = vec![
        ("datacenter_temperature", "Datacenter Temperature Monitoring"),
        ("production_quality", "Factory Production Line"),
        ("smart_home_comfort", "Smart Home Automation"),
        ("office_energy_saving", "Office Energy Management"),
    ];

    ok(json!({
        "templates": templates,
    }))
}

/// Create scenario from template.
///
/// POST /api/scenarios/from-template/:template_id
pub async fn create_from_template_handler(
    State(_state): State<ServerState>,
    Path(template_id): Path<String>,
    Query(params): Query<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let custom_name = params.get("name").and_then(|v| v.as_str());

    let template = match template_id.as_str() {
        "datacenter_temperature" => ScenarioTemplates::datacenter_temperature(),
        "production_quality" => ScenarioTemplates::production_quality(),
        "smart_home_comfort" => ScenarioTemplates::smart_home_comfort(),
        "office_energy_saving" => ScenarioTemplates::office_energy_saving(),
        _ => return Err(ErrorResponse::bad_request("Unknown template")),
    };

    let scenario = manager
        .create_from_template(template, custom_name.map(|s| s.to_string()))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create scenario: {}", e)))?;

    ok(json!({
        "message": "Scenario created from template",
        "scenario": ScenarioDto::from(&scenario),
    }))
}

/// Get LLM prompt for a scenario.
///
/// GET /api/scenarios/:id/prompt
pub async fn get_scenario_prompt_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    let prompt = manager.get_llm_prompt(&scenario_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to generate prompt: {}", e)))?;

    ok(json!({
        "prompt": prompt,
    }))
}

/// Execute a scenario.
///
/// POST /api/scenarios/:id/execute
pub async fn execute_scenario_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let manager = get_scenario_manager();
    let scenario_id = ScenarioId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid scenario ID"))?;

    // Check if scenario exists
    let _scenario = manager.get_scenario(&scenario_id).await
        .ok_or_else(|| ErrorResponse::not_found(format!("Scenario {}", id)))?;

    // For now, execution is just a placeholder
    // In production, this would trigger the scenario's rules and workflows
    ok(json!({
        "message": "Scenario execution triggered",
        "scenario_id": id,
        "status": "executing",
    }))
}
