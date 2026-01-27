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
use edge_ai_storage::dashboards::{
    Dashboard as StoredDashboard, DashboardStore, DashboardTemplate as StoredTemplate,
    DashboardLayout as StoredLayout, DashboardComponent as StoredComponent, default_templates,
};

// ============================================================================
// API Types (match frontend expectations)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_h: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_h: Option<u32>,
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
// Conversion Helpers
// ============================================================================

/// Convert stored dashboard to API dashboard
fn stored_to_api(dashboard: &StoredDashboard) -> Dashboard {
    Dashboard {
        id: dashboard.id.clone(),
        name: dashboard.name.clone(),
        layout: convert_layout(&dashboard.layout),
        components: dashboard.components.iter().map(convert_component).collect(),
        created_at: dashboard.created_at,
        updated_at: dashboard.updated_at,
        is_default: dashboard.is_default,
    }
}

/// Convert API layout to stored layout
fn api_to_stored_layout(layout: &DashboardLayout) -> StoredLayout {
    StoredLayout {
        columns: layout.columns,
        rows: match &layout.rows {
            RowsValue::String(s) => edge_ai_storage::dashboards::RowsValue::String(s.clone()),
            RowsValue::Number(n) => edge_ai_storage::dashboards::RowsValue::Number(*n),
        },
        breakpoints: edge_ai_storage::dashboards::LayoutBreakpoints {
            lg: layout.breakpoints.lg,
            md: layout.breakpoints.md,
            sm: layout.breakpoints.sm,
            xs: layout.breakpoints.xs,
        },
    }
}

/// Convert stored layout to API layout
fn convert_layout(layout: &StoredLayout) -> DashboardLayout {
    DashboardLayout {
        columns: layout.columns,
        rows: match &layout.rows {
            edge_ai_storage::dashboards::RowsValue::String(s) => RowsValue::String(s.clone()),
            edge_ai_storage::dashboards::RowsValue::Number(n) => RowsValue::Number(*n),
        },
        breakpoints: LayoutBreakpoints {
            lg: layout.breakpoints.lg,
            md: layout.breakpoints.md,
            sm: layout.breakpoints.sm,
            xs: layout.breakpoints.xs,
        },
    }
}

/// Convert API component to stored component
fn api_to_stored_component(component: &CreateDashboardComponent) -> StoredComponent {
    StoredComponent {
        id: String::new(), // Will be set by caller
        component_type: component.component_type.clone(),
        position: edge_ai_storage::dashboards::ComponentPosition {
            x: component.position.x,
            y: component.position.y,
            w: component.position.w,
            h: component.position.h,
            min_w: component.position.min_w,
            min_h: component.position.min_h,
            max_w: component.position.max_w,
            max_h: component.position.max_h,
        },
        title: component.title.clone(),
        data_source: component.data_source.clone(),
        display: component.display.clone(),
        config: component.config.clone(),
        actions: component.actions.clone(),
    }
}

/// Convert stored component to API component
fn convert_component(component: &StoredComponent) -> DashboardComponent {
    DashboardComponent {
        id: component.id.clone(),
        component_type: component.component_type.clone(),
        position: ComponentPosition {
            x: component.position.x,
            y: component.position.y,
            w: component.position.w,
            h: component.position.h,
            min_w: component.position.min_w,
            min_h: component.position.min_h,
            max_w: component.position.max_w,
            max_h: component.position.max_h,
        },
        title: component.title.clone(),
        data_source: component.data_source.clone(),
        display: component.display.clone(),
        config: component.config.clone(),
        actions: component.actions.clone(),
    }
}

/// Convert stored template to API template
fn stored_template_to_api(template: &StoredTemplate) -> DashboardTemplate {
    DashboardTemplate {
        id: template.id.clone(),
        name: template.name.clone(),
        description: template.description.clone(),
        category: template.category.clone(),
        icon: template.icon.clone(),
        layout: convert_layout(&template.layout),
        components: template.components.clone(),
        required_resources: template.required_resources.as_ref().map(|r| RequiredResources {
            devices: r.devices,
            agents: r.agents,
            rules: r.rules,
        }),
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// List all dashboards
pub async fn list_dashboards_handler(
    State(state): State<ServerState>,
) -> HandlerResult<DashboardsResponse> {
    let dashboards = state.dashboard_store.list_all()
        .map_err(|e| ErrorResponse::internal(&format!("Failed to list dashboards: {}", e)))?;

    let api_dashboards: Vec<Dashboard> = dashboards.iter().map(stored_to_api).collect();
    let count = api_dashboards.len();

    ok(DashboardsResponse {
        dashboards: api_dashboards,
        count,
    })
}

/// Get a dashboard by ID
pub async fn get_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Dashboard> {
    // Special handling for "overview" and "blank" template IDs
    if id == "overview" || id == "blank" {
        let templates = default_templates();
        let template = templates.into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| ErrorResponse::not_found(&format!("Template '{}' not found", id)))?;

        let now = chrono::Utc::now().timestamp();
        return ok(Dashboard {
            id: template.id,
            name: template.name,
            layout: convert_layout(&template.layout),
            components: vec![],
            created_at: now,
            updated_at: now,
            is_default: Some(id == "overview"),
        });
    }

    let dashboard = state.dashboard_store.load(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to load dashboard: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Dashboard '{}' not found", id)))?;

    ok(stored_to_api(&dashboard))
}

/// Create a new dashboard
pub async fn create_dashboard_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateDashboardRequest>,
) -> HandlerResult<Dashboard> {
    let now = chrono::Utc::now().timestamp();
    let id = format!("dashboard_{}", now);

    let stored_dashboard = StoredDashboard {
        id: id.clone(),
        name: req.name,
        layout: api_to_stored_layout(&req.layout),
        components: req.components.iter()
            .enumerate()
            .map(|(i, c)| {
                let mut comp = api_to_stored_component(c);
                comp.id = format!("component_{}", i);
                comp
            })
            .collect(),
        created_at: now,
        updated_at: now,
        is_default: None,
    };

    state.dashboard_store.save(&stored_dashboard)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to save dashboard: {}", e)))?;

    ok(stored_to_api(&stored_dashboard))
}

/// Update a dashboard
pub async fn update_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDashboardRequest>,
) -> HandlerResult<Dashboard> {
    let mut dashboard = state.dashboard_store.load(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to load dashboard: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Dashboard '{}' not found", id)))?;

    // Update fields if provided
    if let Some(name) = req.name {
        dashboard.name = name;
    }
    if let Some(layout) = req.layout {
        dashboard.layout = api_to_stored_layout(&layout);
    }
    if let Some(components) = req.components {
        // Parse components from JSON
        dashboard.components = components.iter()
            .filter_map(|c| serde_json::from_value::<StoredComponent>(c.clone()).ok())
            .collect();
    }
    dashboard.updated_at = chrono::Utc::now().timestamp();

    state.dashboard_store.save(&dashboard)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to save dashboard: {}", e)))?;

    ok(stored_to_api(&dashboard))
}

/// Delete a dashboard
pub async fn delete_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if dashboard exists
    if !state.dashboard_store.exists(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to check dashboard: {}", e)))? {
        return Err(ErrorResponse::not_found(&format!("Dashboard '{}' not found", id)));
    }

    state.dashboard_store.delete(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to delete dashboard: {}", e)))?;

    ok(serde_json::json!({
        "ok": true,
        "id": id,
    }))
}

/// Set default dashboard
pub async fn set_default_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if dashboard exists
    if !state.dashboard_store.exists(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to check dashboard: {}", e)))? {
        return Err(ErrorResponse::not_found(&format!("Dashboard '{}' not found", id)));
    }

    state.dashboard_store.set_default(&id)
        .map_err(|e| ErrorResponse::internal(&format!("Failed to set default dashboard: {}", e)))?;

    ok(serde_json::json!({
        "id": id,
        "is_default": true,
    }))
}

/// List dashboard templates
pub async fn list_templates_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<Vec<DashboardTemplate>> {
    let templates = default_templates();
    ok(templates.iter().map(stored_template_to_api).collect())
}

/// Get a template by ID
pub async fn get_template_handler(
    State(_state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<DashboardTemplate> {
    let templates = default_templates();
    let template = templates.into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| ErrorResponse::not_found(&format!("Template '{}' not found", id)))?;

    ok(stored_template_to_api(&template))
}
