//! Dashboard handlers
//!
//! Provides API endpoints for managing visual dashboards with components.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::Arc;

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

// ============================================================================
// Types
// ============================================================================

/// Dashboard layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardLayout {
    pub columns: u32,
    #[serde(alias = "rows", rename = "rows")]
    pub rows: RowsValue,
    pub breakpoints: LayoutBreakpoints,
}

/// Rows value - can be "auto" string or a number
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowsValue {
    String(String),
    Number(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutBreakpoints {
    pub lg: u32,
    pub md: u32,
    pub sm: u32,
    pub xs: u32,
}

/// Component position on the grid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPosition {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Dashboard component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponent {
    pub id: String,
    #[serde(alias = "type", rename = "type")]
    pub component_type: String,
    pub position: ComponentPosition,
    #[serde(skip_serializing_if = "Option::is_none", alias = "title", rename = "title")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "data_source", rename = "data_source")]
    pub data_source: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "display", rename = "display")]
    pub display: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "config", rename = "config")]
    pub config: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "actions", rename = "actions")]
    pub actions: Option<JsonValue>,
}

/// Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub layout: DashboardLayout,
    pub components: Vec<DashboardComponent>,
    #[serde(alias = "created_at", rename = "created_at")]
    pub created_at: i64,
    #[serde(alias = "updated_at", rename = "updated_at")]
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none", alias = "is_default", rename = "is_default")]
    pub is_default: Option<bool>,
}

/// Request to create a dashboard
#[derive(Debug, Deserialize)]
pub struct CreateDashboardRequest {
    pub name: String,
    pub layout: DashboardLayout,
    pub components: Vec<CreateDashboardComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDashboardComponent {
    #[serde(alias = "type", rename = "type")]
    pub component_type: String,
    pub position: ComponentPosition,
    #[serde(skip_serializing_if = "Option::is_none", alias = "title", rename = "title")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "data_source", rename = "data_source")]
    pub data_source: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "display", rename = "display")]
    pub display: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "config", rename = "config")]
    pub config: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "actions", rename = "actions")]
    pub actions: Option<JsonValue>,
}

/// Request to update a dashboard - use JsonValue to accept flexible formats
#[derive(Debug, Deserialize)]
pub struct UpdateDashboardRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<DashboardLayout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<JsonValue>>,
}

/// Dashboard template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub layout: DashboardLayout,
    pub components: Vec<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_resources: Option<RequiredResources>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<u32>,
}

/// Response with dashboards list
#[derive(Serialize)]
pub struct DashboardsResponse {
    pub dashboards: Vec<Dashboard>,
    pub count: usize,
}

// ============================================================================
// Default templates
// ============================================================================

fn default_layout() -> DashboardLayout {
    DashboardLayout {
        columns: 12,
        rows: RowsValue::String("auto".to_string()),
        breakpoints: LayoutBreakpoints {
            lg: 1200,
            md: 996,
            sm: 768,
            xs: 480,
        },
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// List all dashboards
pub async fn list_dashboards_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<DashboardsResponse> {
    // For now, return empty list - dashboards are stored in frontend local storage
    // TODO: Implement persistent dashboard storage in redb
    ok(DashboardsResponse {
        dashboards: vec![],
        count: 0,
    })
}

/// Get a dashboard by ID
pub async fn get_dashboard_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Dashboard> {
    // TODO: Implement persistent dashboard storage
    // For now, return a default dashboard if requested
    if id == "overview" || id == "blank" {
        ok(Dashboard {
            id: id.to_string(),
            name: if id == "overview" { "Overview" } else { "Blank Canvas" }.to_string(),
            layout: default_layout(),
            components: vec![],
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            is_default: Some(id == "overview"),
        })
    } else {
        Err(ErrorResponse::not_found(&format!("Dashboard '{}' not found", id)))
    }
}

/// Create a new dashboard
pub async fn create_dashboard_handler(
    State(_state): State<ServerState>,
    Json(req): Json<CreateDashboardRequest>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Implement persistent dashboard storage
    let id = format!("dashboard_{}", chrono::Utc::now().timestamp_millis());
    ok(serde_json::json!({
        "id": id,
        "name": req.name,
    }))
}

/// Update a dashboard
pub async fn update_dashboard_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
    Json(_req): Json<UpdateDashboardRequest>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Implement persistent dashboard storage
    // Accept any JSON format for now since frontend uses local storage fallback
    ok(serde_json::json!({
        "id": id,
        "updated": true,
    }))
}

/// Delete a dashboard
pub async fn delete_dashboard_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Implement persistent dashboard storage
    ok(serde_json::json!({
        "ok": true,
        "id": id,
    }))
}

/// Set default dashboard
pub async fn set_default_dashboard_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // TODO: Implement persistent dashboard storage
    ok(serde_json::json!({
        "id": id,
        "is_default": true,
    }))
}

/// List dashboard templates
pub async fn list_templates_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<Vec<DashboardTemplate>> {
    let templates = vec![
        DashboardTemplate {
            id: "overview".to_string(),
            name: "Overview".to_string(),
            description: "System overview with devices, agents, and events".to_string(),
            category: "overview".to_string(),
            icon: Some("LayoutDashboard".to_string()),
            layout: default_layout(),
            components: vec![],
            required_resources: Some(RequiredResources {
                devices: Some(1),
                agents: Some(1),
                rules: Some(0),
            }),
        },
        DashboardTemplate {
            id: "blank".to_string(),
            name: "Blank Canvas".to_string(),
            description: "Start from scratch with an empty dashboard".to_string(),
            category: "custom".to_string(),
            icon: Some("Square".to_string()),
            layout: default_layout(),
            components: vec![],
            required_resources: Some(RequiredResources {
                devices: Some(0),
                agents: Some(0),
                rules: Some(0),
            }),
        },
    ];

    ok(templates)
}

/// Get a template by ID
pub async fn get_template_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<DashboardTemplate> {
    let templates = vec![
        ("overview", "Overview", "System overview with devices, agents, and events", "overview", Some("LayoutDashboard")),
        ("blank", "Blank Canvas", "Start from scratch with an empty dashboard", "custom", Some("Square")),
    ];

    let template = templates.iter().find(|(tid, _, _, _, _)| tid == &id);

    if let Some((id, name, desc, category, icon)) = template {
        ok(DashboardTemplate {
            id: id.to_string(),
            name: name.to_string(),
            description: desc.to_string(),
            category: category.to_string(),
            icon: icon.map(|s| s.to_string()),
            layout: default_layout(),
            components: vec![],
            required_resources: Some(RequiredResources {
                devices: Some(1),
                agents: Some(1),
                rules: Some(0),
            }),
        })
    } else {
        Err(ErrorResponse::not_found(&format!("Template '{}' not found", id)))
    }
}
