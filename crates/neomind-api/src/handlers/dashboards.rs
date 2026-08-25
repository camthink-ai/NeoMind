//! Dashboard handlers
//!
//! Provides API endpoints for managing visual dashboards with components.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::automation::types::{AutomationMetadata, TransformAutomation};
use crate::models::ErrorResponse;
use neomind_core::event::NeoMindEvent;
use neomind_storage::dashboards::{
    default_templates, Dashboard as StoredDashboard, DashboardComponent as StoredComponent,
    DashboardLayout as StoredLayout, DashboardTemplate as StoredTemplate,
    SharePermissions as StoredSharePermissions, ShareToken as StoredShareToken,
};

/// All widget type ids the frontend can render: builtin ∪ community ∪
/// extension-bundled. Shared by the add-components and update (full-replace)
/// paths so both reject unknown types at the door.
fn collect_known_widget_types(state: &ServerState) -> std::collections::HashSet<String> {
    let mut known: std::collections::HashSet<String> =
        neomind_core::dashboard::BUILTIN_WIDGET_TYPES
            .iter()
            .map(|t| t.type_id.to_string())
            .collect();
    if let Ok(installed) = state.frontend_component_store.list_all() {
        for m in installed {
            known.insert(m.id.clone());
        }
    }
    // Extension-bundled components (from installed extensions). The
    // async runtime list is not awaitable here (sync validation block),
    // so read the store's file paths and scan frontend/ manifests.
    for rec in state.extensions.store.load_all().unwrap_or_default() {
        let dir = std::path::Path::new(&rec.file_path)
            .parent()
            .map(|p| p.to_path_buf());
        if let Some(dir) = dir {
            for c in super::extensions::load_extension_components(&rec.id, Some(&dir))
                .unwrap_or_default()
            {
                known.insert(c.component_type.clone());
            }
        }
    }
    known
}

/// Resolve a dashboard by id first, then by exact NAME. Every handler that
/// takes :id must resolve identically — LLM callers naturally address boards
/// by name (or reuse the name after a `dashboard get` that itself accepts
/// names); a get-by-name that succeeds followed by a mutation-by-name that
/// 404s traps small models into hallucinating success (observed 2026-08-23).
fn resolve_dashboard(
    state: &ServerState,
    id_or_name: &str,
) -> Result<StoredDashboard, ErrorResponse> {
    match state.dashboard_store.load(id_or_name) {
        Ok(Some(d)) => Ok(d),
        Ok(None) => {
            let all = state.dashboard_store.list_all().map_err(|e| {
                ErrorResponse::internal(format!("Failed to list dashboards: {}", e))
            })?;
            all.into_iter().find(|d| d.name == id_or_name).ok_or_else(|| {
                ErrorResponse::not_found(format!(
                    "Dashboard '{}' not found (by id or name) — run `neomind dashboard list` for valid ids",
                    id_or_name
                ))
            })
        }
        Err(e) => Err(ErrorResponse::internal(format!(
            "Failed to load dashboard: {}",
            e
        ))),
    }
}

/// Emit a DashboardUpdated event to notify frontend of changes.
fn emit_dashboard_event(state: &ServerState, dashboard_id: &str, action: &str) {
    if let Some(event_bus) = state.core.event_bus.clone() {
        let event = NeoMindEvent::DashboardUpdated {
            dashboard_id: dashboard_id.to_string(),
            action: action.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        };
        tokio::spawn(async move {
            event_bus.publish(event).await;
        });
    }
}

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

impl Default for DashboardLayout {
    fn default() -> Self {
        Self {
            columns: 12,
            rows: RowsValue::String("auto".to_string()),
            breakpoints: LayoutBreakpoints::default(),
        }
    }
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

impl Default for LayoutBreakpoints {
    fn default() -> Self {
        Self {
            lg: 1200,
            md: 996,
            sm: 768,
            xs: 480,
        }
    }
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "title",
        rename = "title"
    )]
    pub title: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "data_source",
        rename = "data_source"
    )]
    pub data_source: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "display",
        rename = "display"
    )]
    pub display: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "config",
        rename = "config"
    )]
    pub config: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "actions",
        rename = "actions"
    )]
    pub actions: Option<JsonValue>,
}

/// Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub layout: DashboardLayout,
    pub components: Vec<DashboardComponent>,
    #[serde(alias = "created_at", rename = "created_at")]
    pub created_at: i64,
    #[serde(alias = "updated_at", rename = "updated_at")]
    pub updated_at: i64,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "is_default",
        rename = "is_default"
    )]
    pub is_default: Option<bool>,
    /// Manual sort order (lower = higher in the sidebar list).
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "sort_order",
        rename = "sort_order"
    )]
    pub sort_order: Option<i32>,
}

/// Request to create a dashboard
#[derive(Debug, Deserialize)]
pub struct CreateDashboardRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub layout: DashboardLayout,
    #[serde(default)]
    pub components: Vec<CreateDashboardComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDashboardComponent {
    /// Optional client-provided ID; if absent, server generates one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(alias = "type", rename = "type")]
    pub component_type: String,
    pub position: ComponentPosition,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "title",
        rename = "title"
    )]
    pub title: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "data_source",
        rename = "data_source"
    )]
    pub data_source: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "display",
        rename = "display"
    )]
    pub display: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "config",
        rename = "config"
    )]
    pub config: Option<JsonValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "actions",
        rename = "actions"
    )]
    pub actions: Option<JsonValue>,
}

/// Request to update a dashboard - use JsonValue to accept flexible formats
#[derive(Debug, Deserialize)]
pub struct UpdateDashboardRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

/// Pagination query parameters
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

// ============================================================================
// Conversion Helpers
// ============================================================================

/// Convert stored dashboard to API dashboard
fn stored_to_api(dashboard: &StoredDashboard) -> Dashboard {
    Dashboard {
        id: dashboard.id.clone(),
        name: dashboard.name.clone(),
        description: dashboard.description.clone(),
        layout: convert_layout(&dashboard.layout),
        components: dashboard.components.iter().map(convert_component).collect(),
        created_at: dashboard.created_at,
        updated_at: dashboard.updated_at,
        is_default: dashboard.is_default,
        sort_order: dashboard.sort_order,
    }
}

/// Convert API layout to stored layout
fn api_to_stored_layout(layout: &DashboardLayout) -> StoredLayout {
    StoredLayout {
        columns: layout.columns,
        rows: match &layout.rows {
            RowsValue::String(s) => neomind_storage::dashboards::RowsValue::String(s.clone()),
            RowsValue::Number(n) => neomind_storage::dashboards::RowsValue::Number(*n),
        },
        breakpoints: neomind_storage::dashboards::LayoutBreakpoints {
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
            neomind_storage::dashboards::RowsValue::String(s) => RowsValue::String(s.clone()),
            neomind_storage::dashboards::RowsValue::Number(n) => RowsValue::Number(*n),
        },
        breakpoints: LayoutBreakpoints {
            lg: layout.breakpoints.lg,
            md: layout.breakpoints.md,
            sm: layout.breakpoints.sm,
            xs: layout.breakpoints.xs,
        },
    }
}

/// Convert API component to stored component.
/// Preserves client-provided ID when available, otherwise caller must assign one.
fn api_to_stored_component(component: &CreateDashboardComponent) -> StoredComponent {
    StoredComponent {
        id: component.id.clone().unwrap_or_default(),
        component_type: component.component_type.clone(),
        position: neomind_storage::dashboards::ComponentPosition {
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
        required_resources: template
            .required_resources
            .as_ref()
            .map(|r| RequiredResources {
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
///
/// Performance optimization: Supports pagination via limit/offset query parameters.
/// Example: GET /api/dashboards?limit=10&offset=20
pub async fn list_dashboards_handler(
    State(state): State<ServerState>,
    Query(params): Query<PaginationParams>,
) -> HandlerResult<DashboardsResponse> {
    // Enforce reasonable limits to prevent performance issues
    let limit = params.limit.unwrap_or(100).min(1000); // Default 100, max 1000
    let offset = params.offset.unwrap_or(0);

    // Get total count for pagination metadata (only when paginating)
    let total = if limit < usize::MAX || offset > 0 {
        state.dashboard_store.count().ok()
    } else {
        None
    };

    let dashboards = state
        .dashboard_store
        .list_paginated(Some(limit), Some(offset))
        .map_err(|e| ErrorResponse::internal(format!("Failed to list dashboards: {}", e)))?;

    // Sort by `sort_order` so the sidebar reflects manual reordering. Legacy
    // rows without `sort_order` fall to the bottom (i32::MAX); stable sort
    // keeps their relative order intact.
    let mut dashboards = dashboards;
    dashboards.sort_by_key(|d| d.sort_order.unwrap_or(i32::MAX));

    let api_dashboards: Vec<Dashboard> = dashboards.iter().map(stored_to_api).collect();
    let count = api_dashboards.len();

    ok(DashboardsResponse {
        dashboards: api_dashboards,
        count,
        total,
        limit: if total.is_some() { Some(limit) } else { None },
        offset: if total.is_some() { Some(offset) } else { None },
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
        let template = templates
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| ErrorResponse::not_found(format!("Template '{}' not found", id)))?;

        let now = chrono::Utc::now().timestamp();
        return ok(Dashboard {
            id: template.id.clone(),
            name: template.name.clone(),
            description: Some(template.description.clone()),
            layout: convert_layout(&template.layout),
            components: vec![],
            created_at: now,
            updated_at: now,
            is_default: Some(id == "overview"),
            sort_order: None,
        });
    }

    let dashboard = resolve_dashboard(&state, &id)?;

    ok(stored_to_api(&dashboard))
}

/// Create a new dashboard
pub async fn create_dashboard_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateDashboardRequest>,
) -> HandlerResult<Dashboard> {
    let id = format!("dashboard_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().timestamp();

    let next_order = state
        .dashboard_store
        .max_sort_order()
        .map_err(|e| ErrorResponse::internal(format!("Failed to compute sort order: {}", e)))?
        + 1;

    let stored_dashboard = StoredDashboard {
        id: id.clone(),
        name: req.name.clone(),
        description: req.description,
        layout: api_to_stored_layout(&req.layout),
        components: req
            .components
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut comp = api_to_stored_component(c);
                if comp.id.is_empty() {
                    comp.id = format!("component_{}", i);
                }
                comp
            })
            .collect(),
        created_at: now,
        updated_at: now,
        is_default: None,
        sort_order: Some(next_order),
    };

    state
        .dashboard_store
        .save(&stored_dashboard)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save dashboard: {}", e)))?;

    emit_dashboard_event(&state, &stored_dashboard.id, "create");

    ok(stored_to_api(&stored_dashboard))
}

/// Update a dashboard
pub async fn update_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDashboardRequest>,
) -> HandlerResult<Dashboard> {
    let mut dashboard = resolve_dashboard(&state, &id)?;

    // Update fields if provided
    if let Some(name) = req.name {
        dashboard.name = name;
    }
    if let Some(description) = req.description {
        dashboard.description = Some(description);
    }
    if let Some(layout) = req.layout {
        dashboard.layout = api_to_stored_layout(&layout);
    }
    if let Some(components) = req.components {
        // Parse components from JSON — fail if any component is invalid
        let parsed: Result<Vec<StoredComponent>, String> = components
            .iter()
            .enumerate()
            .map(|(i, c)| {
                serde_json::from_value::<StoredComponent>(c.clone())
                    .map_err(|e| format!("component[{}]: {}", i, e))
            })
            .collect();
        match parsed {
            Ok(parsed_components) => {
                // Same type gate as add-components — the full-replace path is
                // the one place a typo'd type could still sneak in silently.
                let known = collect_known_widget_types(&state);
                // The update path must NOT hard-fail on orphaned/legacy types:
                // a dashboard with a widget whose type was later uninstalled (or
                // shared from another instance) would become un-editable. Warn
                // and persist — the renderer shows an UnknownComponent placeholder.
                // The ADD path keeps the strict gate (typos on new widgets).
                for c in &parsed_components {
                    if !known.contains(&c.component_type) {
                        tracing::warn!(
                            dashboard_id = %id,
                            component_type = %c.component_type,
                            "update-dashboard: persisting a component with an unknown type (orphaned/legacy)"
                        );
                    }
                }
                dashboard.components = parsed_components;
            }
            Err(e) => {
                return Err(
                    ErrorResponse::bad_request(format!("Invalid component data: {}", e)).with_hint(
                        "Each component needs: id, type, title, position {x,y,w,h}.\n\
                             Use 'neomind dashboard add-components' instead of full replacement.",
                    ),
                )
            }
        }
    }
    dashboard.updated_at = chrono::Utc::now().timestamp();

    state
        .dashboard_store
        .save(&dashboard)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save dashboard: {}", e)))?;

    emit_dashboard_event(&state, &id, "update");

    ok(stored_to_api(&dashboard))
}

/// Add components to a dashboard (append mode)
pub async fn add_components_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<AddComponentsRequest>,
) -> HandlerResult<Dashboard> {
    let mut dashboard = resolve_dashboard(&state, &id)?;

    // Parse and append new components
    let new_components: Vec<StoredComponent> = req
        .components
        .iter()
        .enumerate()
        .map(|(i, c)| {
            serde_json::from_value::<StoredComponent>(c.clone()).map_err(|e| {
                format!("component[{}]: {}", i, e)
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            ErrorResponse::bad_request(format!("Invalid component data: {}", e)).with_hint(
                "Each component needs: id (string), type (value-card|line-chart|bar-chart|gauge|markdown-display), title (string), \
                 position {x, y, w, h}.\n\
                 Example: {\"id\":\"temp\",\"type\":\"value-card\",\"title\":\"Temp\",\"position\":{\"x\":0,\"y\":0,\"w\":4,\"h\":2},\
                 \"data_source\":{\"type\":\"device\",\"sourceId\":\"<device-id>\",\"property\":\"<metric>\"}}",
            )
        })?;

    // Reject unknown component TYPES up front: a typo'd type ("value_card")
    // used to persist silently and render as an UnknownComponent placeholder
    // while the agent claimed success. Known = builtin ∪ community ∪
    // extension-bundled.
    let known = collect_known_widget_types(&state);
    if let Some(bad) = new_components
        .iter()
        .find(|c| !known.contains(&c.component_type))
    {
        return Err(ErrorResponse::bad_request(format!(
            "Unknown component type '{}' — run `neomind widget list` for valid types",
            bad.component_type
        ))
        .with_hint(
            "Use the exact type id (kebab-case, e.g. value-card / line-chart / sparkline).",
        ));
    }

    // Reject component-id collisions: duplicates make `update-component`
    // (find-first) ambiguous and break the frontend's React keys.
    if let Some(dup) = new_components.iter().find(|c| {
        dashboard
            .components
            .iter()
            .any(|existing| existing.id == c.id)
    }) {
        return Err(ErrorResponse::bad_request(format!(
            "Component id '{}' already exists on dashboard '{}' — component ids must be unique. \
             Use `dashboard get {}` to list existing ids, or `update-component` to modify one.",
            dup.id, id, id
        )));
    }
    // Also reject duplicates WITHIN the same batch.
    {
        let mut seen = std::collections::HashSet::new();
        if let Some(dup) = new_components.iter().find(|c| !seen.insert(&c.id)) {
            return Err(ErrorResponse::bad_request(format!(
                "Duplicate component id '{}' within the same --components batch",
                dup.id
            )));
        }
    }

    dashboard.components.extend(new_components);
    dashboard.updated_at = chrono::Utc::now().timestamp();

    state
        .dashboard_store
        .save(&dashboard)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save dashboard: {}", e)))?;

    emit_dashboard_event(&state, &id, "add_components");

    ok(stored_to_api(&dashboard))
}

/// Remove components from a dashboard by ID
pub async fn remove_components_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<RemoveComponentsRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut dashboard = resolve_dashboard(&state, &id)?;

    let before = dashboard.components.len();
    dashboard.components.retain(|c| !req.ids.contains(&c.id));
    let removed = before - dashboard.components.len();
    dashboard.updated_at = chrono::Utc::now().timestamp();

    state
        .dashboard_store
        .save(&dashboard)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save dashboard: {}", e)))?;

    emit_dashboard_event(&state, &id, "remove_components");

    ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "remaining": dashboard.components.len(),
    }))
}

/// Request to add components
#[derive(Debug, Deserialize)]
pub struct AddComponentsRequest {
    pub components: Vec<JsonValue>,
}

/// Request to remove components by ID
#[derive(Debug, Deserialize)]
pub struct RemoveComponentsRequest {
    pub ids: Vec<String>,
}

/// Request to patch a single component (deep merge)
#[derive(Debug, Deserialize)]
pub struct UpdateComponentRequest {
    /// Partial component JSON, DEEP-MERGED into the stored component:
    /// objects merge recursively, everything else (scalars, arrays) replaces.
    /// `id` and `type` are immutable and ignored.
    pub set: Option<JsonValue>,
}

/// Recursive JSON merge: objects merge key-by-key, everything else replaces.
fn deep_merge_json(target: &mut JsonValue, patch: &JsonValue) {
    if let (JsonValue::Object(t), JsonValue::Object(p)) = (target, patch) {
        for (k, v) in p {
            match t.get_mut(k) {
                Some(JsonValue::Object(_)) if v.is_object() => {
                    if let Some(child) = t.get_mut(k) {
                        deep_merge_json(child, v);
                    }
                }
                _ => {
                    t.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// Patch ONE component of a dashboard (deep merge) — the cheap way to tweak a
/// single field (e.g. a data_source timeWindow) without round-tripping the
/// whole component array through `dashboard update --components`.
pub async fn update_component_handler(
    State(state): State<ServerState>,
    Path((id, component_id)): Path<(String, String)>,
    Json(req): Json<UpdateComponentRequest>,
) -> HandlerResult<serde_json::Value> {
    let set = req.set.unwrap_or(JsonValue::Null);
    if !set.is_object() {
        return Err(ErrorResponse::bad_request(
            "`set` must be a JSON object of partial component fields",
        ));
    }

    let mut dashboard = resolve_dashboard(&state, &id)?;

    let component = dashboard
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or_else(|| {
            ErrorResponse::not_found(format!(
                "Component '{}' not found on dashboard '{}' (use `dashboard get {}` to list component ids)",
                component_id, id, id
            ))
        })?;

    // Round-trip through JSON so the patch can touch any field generically.
    let mut value = serde_json::to_value(&*component)
        .map_err(|e| ErrorResponse::internal(format!("Failed to serialize component: {}", e)))?;
    // id and type identify the component — never let a patch move/retype it.
    if let JsonValue::Object(patch) = &set {
        let mut sanitized = patch.clone();
        sanitized.remove("id");
        sanitized.remove("type");
        deep_merge_json(&mut value, &JsonValue::Object(sanitized));
    }
    let patched: StoredComponent = serde_json::from_value(value)
        .map_err(|e| ErrorResponse::bad_request(format!("Patched component is invalid: {}", e)))?;
    let patched_id = patched.id.clone();
    *component = patched;

    dashboard.updated_at = chrono::Utc::now().timestamp();
    state
        .dashboard_store
        .save(&dashboard)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save dashboard: {}", e)))?;

    emit_dashboard_event(&state, &id, "update_component");

    ok(serde_json::json!({
        "ok": true,
        "dashboard_id": id,
        "component_id": patched_id,
        "component": serde_json::to_value(
            dashboard.components.iter().find(|c| c.id == patched_id)
        ).unwrap_or(JsonValue::Null),
    }))
}

/// Delete a dashboard
pub async fn delete_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if dashboard exists
    if !state
        .dashboard_store
        .exists(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to check dashboard: {}", e)))?
    {
        return Err(ErrorResponse::not_found(format!(
            "Dashboard '{}' not found",
            id
        )));
    }

    state
        .dashboard_store
        .delete(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete dashboard: {}", e)))?;

    emit_dashboard_event(&state, &id, "delete");

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
    if !state
        .dashboard_store
        .exists(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to check dashboard: {}", e)))?
    {
        return Err(ErrorResponse::not_found(format!(
            "Dashboard '{}' not found",
            id
        )));
    }

    state
        .dashboard_store
        .set_default(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to set default dashboard: {}", e)))?;

    ok(serde_json::json!({
        "id": id,
        "is_default": true,
    }))
}

/// Request body for `PUT /api/dashboards/reorder`.
/// `dashboard_ids` is the desired full ordering (index 0 = top).
#[derive(Debug, Deserialize)]
pub struct ReorderDashboardsRequest {
    pub dashboard_ids: Vec<String>,
}

/// Response body for the reorder endpoint.
#[derive(Debug, Serialize)]
pub struct ReorderDashboardsResponse {
    pub ok: bool,
    pub count: usize,
}

/// Batch-update `sort_order` for a list of dashboards (PUT /api/dashboards/reorder)
///
/// Frontend sends the complete desired order as a list of dashboard IDs. We
/// assign each its index as the new `sort_order` and persist them in a single
/// transaction. Emits a single `DashboardUpdated` event with action `"reorder"`
/// so other clients refetch.
pub async fn reorder_dashboards_handler(
    State(state): State<ServerState>,
    Json(req): Json<ReorderDashboardsRequest>,
) -> HandlerResult<ReorderDashboardsResponse> {
    if req.dashboard_ids.is_empty() {
        return Err(ErrorResponse::bad_request(
            "dashboard_ids must not be empty",
        ));
    }

    let items: Vec<(String, i32)> = req
        .dashboard_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i as i32))
        .collect();

    let count = items.len();
    state
        .dashboard_store
        .set_sort_orders(&items)
        .map_err(|e| ErrorResponse::internal(format!("Failed to reorder dashboards: {}", e)))?;

    // Notify other clients. Use the first id as the event target — the SSE
    // consumer refetches the full list on receipt regardless of which id.
    if let Some(first_id) = req.dashboard_ids.first() {
        emit_dashboard_event(&state, first_id, "reorder");
    }

    ok(ReorderDashboardsResponse { ok: true, count })
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
    let template = templates
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Template '{}' not found", id)))?;

    ok(stored_template_to_api(template))
}

// ============================================================================
// Share API Types
// ============================================================================

/// Share permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePermissions {
    pub allow_interactive: bool,
}

/// Request to create a share link
#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    pub permissions: SharePermissions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_hours: Option<i64>,
}

/// Share token response
#[derive(Debug, Serialize)]
pub struct ShareTokenResponse {
    pub token: String,
    pub dashboard_id: String,
    pub permissions: SharePermissions,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub share_url: String,
}

/// Shared dashboard response (public)
#[derive(Debug, Serialize)]
pub struct SharedDashboardResponse {
    pub dashboard: Dashboard,
    pub permissions: SharePermissions,
    pub expires_at: Option<i64>,
}

// ============================================================================
// Share Handlers
// ============================================================================

/// Create a share link for a dashboard
pub async fn create_share_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> HandlerResult<ShareTokenResponse> {
    // Verify dashboard exists
    state
        .dashboard_store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load dashboard: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Dashboard '{}' not found", id)))?;

    // Generate token: ds_ prefix + 22 random hex chars
    let random_bytes: [u8; 16] = rand::random();
    let token_str = format!("ds_{}", hex::encode(random_bytes));

    let now = chrono::Utc::now().timestamp();
    let expires_at = req.expires_in_hours.and_then(|h| {
        h.checked_mul(3600)
            .and_then(|seconds| now.checked_add(seconds))
    });

    let share = StoredShareToken {
        token: token_str.clone(),
        dashboard_id: id.clone(),
        permissions: StoredSharePermissions {
            allow_interactive: req.permissions.allow_interactive,
        },
        created_at: now,
        expires_at,
        created_by: None,
    };

    state
        .dashboard_store
        .save_share_token(&share)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save share token: {}", e)))?;

    ok(ShareTokenResponse {
        token: token_str.clone(),
        dashboard_id: id.clone(),
        permissions: SharePermissions {
            allow_interactive: share.permissions.allow_interactive,
        },
        created_at: now,
        expires_at,
        share_url: format!("/share/{}", token_str),
    })
}

/// List all share links for a dashboard
pub async fn list_shares_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<ShareTokenResponse>> {
    let tokens = state
        .dashboard_store
        .list_share_tokens(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to list share tokens: {}", e)))?;

    let now = chrono::Utc::now().timestamp();
    let responses: Vec<ShareTokenResponse> = tokens
        .into_iter()
        .map(|t| ShareTokenResponse {
            share_url: format!("/share/{}", t.token),
            token: t.token,
            dashboard_id: t.dashboard_id,
            permissions: SharePermissions {
                allow_interactive: t.permissions.allow_interactive,
            },
            created_at: t.created_at,
            expires_at: t.expires_at,
        })
        .filter(|r| r.expires_at.is_none_or(|exp| exp > now))
        .collect();

    ok(responses)
}

/// Revoke a share link
pub async fn revoke_share_handler(
    State(state): State<ServerState>,
    Path((id, token)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    // Verify the token belongs to this dashboard
    let share = state
        .dashboard_store
        .load_share_token(&token)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load share token: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found("Share link not found"))?;

    if share.dashboard_id != id {
        return Err(ErrorResponse::not_found("Share link not found"));
    }

    state
        .dashboard_store
        .delete_share_token(&token)
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete share token: {}", e)))?;

    ok(serde_json::json!({
        "ok": true,
        "token": token,
    }))
}

/// Validate a share token: load it, check it exists
fn validate_share_token(
    state: &ServerState,
    token: &str,
) -> Result<StoredShareToken, ErrorResponse> {
    state
        .dashboard_store
        .load_share_token(token)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load share token: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found("Share link not found"))
}

/// Get shared dashboard data (public, no auth)
pub async fn get_shared_dashboard_handler(
    State(state): State<ServerState>,
    Path(token): Path<String>,
) -> HandlerResult<SharedDashboardResponse> {
    let share = validate_share_token(&state, &token)?;

    let dashboard = state
        .dashboard_store
        .load(&share.dashboard_id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load dashboard: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found("Dashboard not found"))?;

    // Check expiration
    if let Some(exp) = share.expires_at {
        if chrono::Utc::now().timestamp() > exp {
            return Err(ErrorResponse::new(
                "GONE",
                "This share link has expired",
                StatusCode::GONE,
            ));
        }
    }

    ok(SharedDashboardResponse {
        dashboard: stored_to_api(&dashboard),
        permissions: SharePermissions {
            allow_interactive: share.permissions.allow_interactive,
        },
        expires_at: share.expires_at,
    })
}

/// Proxy data requests through share token (public, no auth)
///
/// Forwards requests via localhost loopback to the same Axum server.
/// This avoids manually matching every API path — any GET/POST that
/// the existing router handles will work. We only enforce:
/// - Token validation + expiration check
/// - Read-only mode blocks write methods
/// - Sensitive admin/config paths are blocked
pub async fn share_proxy_handler(
    State(state): State<ServerState>,
    Path((token, path)): Path<(String, String)>,
    req: Request<Body>,
) -> Response {
    let method = req.method().clone();
    let headers = req.headers().clone();
    let query = req.uri().query().unwrap_or("").to_string();
    let body = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap_or_default();

    // 1. Validate share token
    let share = match validate_share_token(&state, &token) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // 2. Check expiration
    if let Some(exp) = share.expires_at {
        if chrono::Utc::now().timestamp() > exp {
            return ErrorResponse::new("GONE", "This share link has expired", StatusCode::GONE)
                .into_response();
        }
    }

    let allow_interactive = share.permissions.allow_interactive;
    let path_str = path.as_ref();

    // 3. Enforce an ALLOWLIST of paths reachable through the share proxy.
    //    Previously this was a blocklist, which silently allowed anonymous
    //    share-token holders to read any device / telemetry / agent /
    //    extension data — far beyond what the shared dashboard needs. The
    //    semantics of a share link are "read the data this dashboard needs",
    //    not "read the entire platform".
    if !is_share_proxy_path_allowed(path_str) {
        return ErrorResponse::new(
            "FORBIDDEN",
            "This path is not accessible via share proxy",
            StatusCode::FORBIDDEN,
        )
        .into_response();
    }

    // 4. Method gate. Interactive links get the same POST whitelist as
    //    read-only ones, PLUS device control commands — previously an
    //    interactive link skipped the gate entirely, letting an anonymous
    //    share-token holder run ANY write method on any allowlisted path
    //    (install extensions, delete agents, rewrite channels): "allow this
    //    viewer to press the dashboard's buttons" must not mean "full admin
    //    write inside the allowlist prefixes". PUT/DELETE stay blocked for
    //    both modes — interactive means actuating devices, never editing
    //    configuration.
    if !is_allowed_share_method(path_str, &method, allow_interactive) {
        let reason = if allow_interactive {
            "This action is not available through a shared link"
        } else {
            "This share link is read-only"
        };
        return ErrorResponse::new("FORBIDDEN", reason, StatusCode::FORBIDDEN).into_response();
    }

    // 5. Build query string
    let qs = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query)
    };
    // Bind port is configurable (NEOMIND_PORT / config) — a hardcoded 9375
    // broke every shared-dashboard data proxy on non-default-port installs.
    let target_url = format!(
        "http://127.0.0.1:{}/api/{}{}",
        crate::server::http_bind_port(),
        path_str,
        qs
    );

    // 6. Forward via reqwest (internal loopback, skips auth middleware)
    let client = reqwest::Client::new();
    let mut req_builder = match method {
        Method::GET => client.get(&target_url),
        Method::POST => client.post(&target_url),
        Method::PUT => client.put(&target_url),
        Method::DELETE => client.delete(&target_url),
        _ => {
            return ErrorResponse::new(
                "METHOD_NOT_ALLOWED",
                "Method not supported",
                StatusCode::METHOD_NOT_ALLOWED,
            )
            .into_response();
        }
    };

    // Forward content-type header
    if let Some(ct) = headers.get("content-type") {
        req_builder = req_builder.header("content-type", ct);
    }

    // Mark as internal proxy so auth middleware bypasses JWT check.
    // MUST include the per-process secret — the auth middleware rejects
    // bypass attempts that lack it (blocks spoofed external requests).
    req_builder = req_builder.header("x-internal-proxy", "share").header(
        "x-internal-proxy-secret",
        state.internal_proxy_secret.as_str(),
    );

    if !body.is_empty() {
        req_builder = req_builder.body(body.to_vec());
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let ct = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            let body_bytes = resp.bytes().await.unwrap_or_default();

            Response::builder()
                .status(status)
                .header("content-type", ct)
                .body(Body::from(body_bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => ErrorResponse::internal(format!("Proxy request failed: {}", e)).into_response(),
    }
}

/// Paths reachable through the share proxy. This is an ALLOWLIST — anything
/// not explicitly listed is rejected by default.
///
/// A share link grants "read the data this dashboard needs" semantics. It
/// is NOT a general-purpose read-only account. Previously this used a
/// blocklist that silently allowed anonymous token holders to read every
/// device, telemetry series, agent execution, and extension output on the
/// platform.
///
/// Allowed categories:
/// - Telemetry queries (history + aggregates)
/// - Device list / detail / metrics (dashboard device widgets)
/// - Extension metric outputs (live data widgets)
/// - Agent execution history (status widgets)
/// - Data source values
/// - Messages (alerts shown on dashboard)
fn is_share_proxy_path_allowed(path: &str) -> bool {
    const ALLOWED_PREFIXES: &[&str] = &[
        "telemetry",
        "telemetry-stats",
        "devices",
        "device-types", // device type metadata for rendering device widgets; GET only, mutations blocked by method check
        "extensions", // list + metric/outputs subpaths; install/uninstall/write blocked by method check
        "agents", // list + execution history + detail; full CRUD blocked by method check in step 4
        "llm-backends", // model list for AI analyst widget; DTO exposes only api_key_configured, never the key
        "data-sources",
        "messages", // alerts shown on dashboard (list + detail); channel mutation blocked by method
        "frontend-components", // community widget manifests + bundles (GET only; install/uninstall blocked by method check)
    ];
    // First path segment (no query string).
    let first_segment = path.split('/').next().unwrap_or("");
    // Bare `messages` and `devices` (no trailing slash) also need to match.
    ALLOWED_PREFIXES
        .iter()
        .any(|p| path.starts_with(*p) || first_segment == *p)
}

/// Generate a unique output_prefix by appending the first 8 chars of a fresh UUID.
/// Two transforms with the same output_prefix would collide in the
/// extensionMetric: "<prefix>.<field>" namespace, so this MUST be unique.
fn new_output_prefix(source_prefix: &str) -> String {
    // Sanitize: replace any non-alphanumeric_underscore with underscore
    let clean: String = source_prefix
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let suffix: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
    format!("{}_{}", clean, suffix)
}

/// Rewrite all transform-ID and output-prefix references inside a single cloned
/// component. Only fires when `config._transformId` matches `old_id` — this is the
/// marker that the component "owns" the transform (vs. just referencing a user-picked
/// one). Updates:
///   - config._transformId             → new_id
///   - dataSource.transformId          → new_id
///   - dataSource.sourceId             → "transform:{new_id}"
///   - dataSource.id                   → new_id
///   - dataSource.metricId / .field    → replace old_prefix with new_prefix if the
///                                       string starts with "{old_prefix}."
fn rewrite_component_transform_refs(
    component: &mut StoredComponent,
    old_id: &str,
    new_id: &str,
    old_prefix: &str,
    new_prefix: &str,
) {
    // Gate: only rewrite if config._transformId == old_id (ownership marker)
    let is_owner = component
        .config
        .as_ref()
        .and_then(|c| c.get("_transformId"))
        .and_then(|v| v.as_str())
        .map(|s| s == old_id)
        .unwrap_or(false);
    if !is_owner {
        return;
    }

    // config._transformId
    if let Some(cfg) = component.config.as_mut().and_then(|c| c.as_object_mut()) {
        cfg.insert(
            "_transformId".to_string(),
            JsonValue::String(new_id.to_string()),
        );
    }

    // dataSource fields
    if let Some(ds) = component
        .data_source
        .as_mut()
        .and_then(|d| d.as_object_mut())
    {
        ds.insert(
            "transformId".to_string(),
            JsonValue::String(new_id.to_string()),
        );
        ds.insert(
            "sourceId".to_string(),
            JsonValue::String(format!("transform:{}", new_id)),
        );
        ds.insert("id".to_string(), JsonValue::String(new_id.to_string()));

        let prefix_replacements = ["metricId", "field"];
        let old_prefix_dot = format!("{}.", old_prefix);
        for key in prefix_replacements {
            if let Some(JsonValue::String(s)) = ds.get(key).cloned() {
                if s.starts_with(&old_prefix_dot) {
                    let suffix = &s[old_prefix_dot.len()..];
                    ds.insert(
                        key.to_string(),
                        JsonValue::String(format!("{}.{}", new_prefix, suffix)),
                    );
                }
            }
        }
    }
}

/// Result of cloning a dashboard's contents in memory.
/// The handler persists `dashboard` and each entry of `new_transforms`.
struct DuplicateBuild {
    dashboard: StoredDashboard,
    new_transforms: Vec<TransformAutomation>,
}

/// Pure clone logic — no I/O. Produces a new dashboard + new transforms
/// that the caller then persists. Tested in isolation.
fn build_duplicate_dashboard(
    source: &StoredDashboard,
    available_transforms: Vec<TransformAutomation>,
    max_sort_order: i32,
) -> DuplicateBuild {
    let now = chrono::Utc::now().timestamp();
    let new_dashboard_id = format!("dashboard_{}", uuid::Uuid::new_v4());

    // Deep clone components via JSON round-trip (StoredComponent fields are serde_json::Value)
    let mut new_components: Vec<StoredComponent> = source
        .components
        .iter()
        .map(|c| {
            serde_json::from_value(serde_json::to_value(c).unwrap_or(JsonValue::Null))
                .unwrap_or_else(|_| c.clone())
        })
        .collect();

    let mut new_transforms: Vec<TransformAutomation> = Vec::new();

    for component in &mut new_components {
        // Detect ownership marker
        let old_t_id = match component
            .config
            .as_ref()
            .and_then(|c| c.get("_transformId"))
            .and_then(|v| v.as_str())
        {
            Some(id) => id.to_string(),
            None => continue,
        };

        // Find the source transform
        let src_t = match available_transforms
            .iter()
            .find(|t| t.metadata.id == old_t_id)
        {
            Some(t) => t.clone(),
            None => continue, // Spec edge case: missing transform → leave refs as-is
        };

        // Clone transform with fresh IDs
        let new_t_id = format!("transform_{}", uuid::Uuid::new_v4());
        let new_prefix = new_output_prefix(&src_t.output_prefix);
        let new_transform = TransformAutomation {
            metadata: AutomationMetadata {
                id: new_t_id.clone(),
                name: format!("{} (copy)", src_t.metadata.name),
                description: src_t.metadata.description.clone(),
                enabled: src_t.metadata.enabled,
                execution_count: 0,
                last_executed: None,
                created_at: now,
                updated_at: now,
            },
            scope: src_t.scope.clone(),
            intent: src_t.intent.clone(),
            js_code: src_t.js_code.clone(),
            output_prefix: new_prefix.clone(),
            complexity: src_t.complexity,
            operations: src_t.operations.clone(),
        };

        rewrite_component_transform_refs(
            component,
            &old_t_id,
            &new_t_id,
            &src_t.output_prefix,
            &new_prefix,
        );

        new_transforms.push(new_transform);
    }

    let new_dashboard = StoredDashboard {
        id: new_dashboard_id,
        name: format!("{} (copy)", source.name),
        description: source.description.clone(),
        layout: source.layout.clone(),
        components: new_components,
        created_at: now,
        updated_at: now,
        is_default: None,
        sort_order: Some(max_sort_order + 1),
    };

    DuplicateBuild {
        dashboard: new_dashboard,
        new_transforms,
    }
}

/// Duplicate a dashboard. Clones the source dashboard with a new ID and
/// "(copy)" name. Component-owned transforms (marked via config._transformId)
/// are deep-cloned with fresh IDs and output_prefix; their references in the
/// cloned components are rewritten. Device/agent/extension references stay
/// shared (they are global resources).
pub async fn duplicate_dashboard_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Dashboard> {
    // Load source
    let source = state
        .dashboard_store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to load source dashboard: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Dashboard '{}' not found", id)))?;

    // Read max sort order
    let max_order = state
        .dashboard_store
        .max_sort_order()
        .map_err(|e| ErrorResponse::internal(format!("Failed to compute sort order: {}", e)))?;

    // Load all automations. `Automation` is a TYPE ALIAS for `TransformAutomation`
    // (NOT an enum), so list_automations() returns Vec<TransformAutomation> directly.
    let available_transforms: Vec<TransformAutomation> = match &state.automation.automation_store {
        Some(store) => store
            .list_automations()
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to list automations: {}", e)))?,
        None => Vec::new(),
    };

    // Pure clone logic
    let build = build_duplicate_dashboard(&source, available_transforms, max_order);

    // Persist new transforms first (orphan transforms are harmless if dashboard save fails)
    if let Some(store) = &state.automation.automation_store {
        for t in &build.new_transforms {
            if let Err(e) = store.save_automation(t).await {
                tracing::warn!(error = %e, transform_id = %t.metadata.id, "failed to save cloned transform");
            }
        }
    }

    // Persist new dashboard
    state.dashboard_store.save(&build.dashboard).map_err(|e| {
        ErrorResponse::internal(format!("Failed to save duplicated dashboard: {}", e))
    })?;

    emit_dashboard_event(&state, &build.dashboard.id, "create");

    ok(stored_to_api(&build.dashboard))
}

#[cfg(test)]
mod share_proxy_tests {
    use super::is_share_proxy_path_allowed;

    #[test]
    fn allows_bare_list_endpoints() {
        // Regression: `extensions/` and `agents/` (trailing slash) blocked
        // the bare list GETs that shared-dashboard widgets call.
        for p in [
            "devices",
            "device-types",
            "extensions",
            "agents",
            "llm-backends",
            "messages",
            "frontend-components",
            "data-sources",
            "telemetry",
        ] {
            assert!(
                is_share_proxy_path_allowed(p),
                "bare `{p}` should be allowed"
            );
        }
    }

    #[test]
    fn allows_subpaths() {
        for p in [
            "devices/abc/current",
            "device-types/foo",
            "extensions/weather/command",
            "agents/xyz/executions",
            "telemetry/foo/values",
        ] {
            assert!(is_share_proxy_path_allowed(p), "`{p}` should be allowed");
        }
    }

    #[test]
    fn rejects_unlisted_paths() {
        for p in [
            "users",
            "api-keys",
            "settings",
            "instances",
            "rules",
            "dashboards",
        ] {
            assert!(!is_share_proxy_path_allowed(p), "`{p}` should be blocked");
        }
    }
}

/// Check if a path matches a pattern where `:id` matches any single segment.
/// e.g., `extensions/abc123/command` matches `extensions/:id/command`
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let path_segs: Vec<&str> = path.split('/').collect();
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    if path_segs.len() != pat_segs.len() {
        return false;
    }
    path_segs
        .iter()
        .zip(pat_segs.iter())
        .all(|(p, pat)| pat.starts_with(':') || *p == *pat)
}

/// Method gate for the share proxy — both share modes. Uses exact endpoint
/// matching to prevent unauthorized access to side-effect operations that
/// share a prefix with read-like endpoints.
///
/// "Interactive" means the viewer may actuate what the dashboard shows
/// (send a device control command) — it does NOT open configuration writes.
///
/// Share mode permission matrix:
/// ┌──────────────────────────────────────────┬────────┬─────────┬──────────────┐
/// │ Path pattern                             │ Method │ Allowed │ Mode         │
/// ├──────────────────────────────────────────┼────────┼─────────┼──────────────┤
/// │ * (unless path-blocked)                  │ GET    │ YES     │ both         │
/// │ extensions/:id/command                   │ POST   │ YES     │ both         │
/// │ devices/current-batch                    │ POST   │ YES     │ both         │
/// │ agents/:id/executions/details            │ POST   │ YES     │ both         │
/// │ devices/:id/command/:command             │ POST   │ YES     │ interactive  │
/// │ * (all other)                            │ POST   │ NO      │ both         │
/// │ *                                        │ PUT    │ NO      │ both         │
/// │ *                                        │ DELETE │ NO      │ both         │
/// └──────────────────────────────────────────┴────────┴─────────┴──────────────┘
fn is_allowed_share_method(path: &str, method: &Method, allow_interactive: bool) -> bool {
    if method == Method::GET {
        return true;
    }
    if method != Method::POST {
        return false;
    }
    // Read-like POSTs both modes allow…
    let read_like = [
        "extensions/:id/command",        // Extension commands (incl. read-only queries)
        "devices/current-batch",         // Batch device current values
        "agents/:id/executions/details", // Batch get execution details
    ];
    if read_like.iter().any(|p| path_matches_pattern(path, p)) {
        return true;
    }
    // …plus the one write interactive mode exists for: device control buttons.
    allow_interactive && path_matches_pattern(path, "devices/:id/command/:command")
}

#[cfg(test)]
mod tests {
    use super::{
        build_duplicate_dashboard, deep_merge_json, is_allowed_share_method, new_output_prefix,
        rewrite_component_transform_refs,
    };
    use axum::http::Method;
    use neomind_storage::dashboards::{
        Dashboard as StoredDashboard, DashboardComponent as StoredComponent,
        DashboardLayout as StoredLayout,
    };

    /// Interactive links must NOT be "full admin write inside the allowlist
    /// prefixes" — the method gate whitelists POSTs for both modes, with
    /// interactive adding exactly one endpoint (device control).
    #[test]
    fn share_method_gate_interactive_is_not_blanket_write() {
        // Read-like POSTs: both modes.
        for path in [
            "devices/current-batch",
            "extensions/weather/command",
            "agents/a1/executions/details",
        ] {
            assert!(
                is_allowed_share_method(path, &Method::POST, false),
                "read-like POST `{path}` must pass in read-only mode"
            );
            assert!(
                is_allowed_share_method(path, &Method::POST, true),
                "read-like POST `{path}` must pass in interactive mode"
            );
        }

        // Device control: interactive only.
        assert!(!is_allowed_share_method(
            "devices/ne101-01/command/reboot",
            &Method::POST,
            false
        ));
        assert!(is_allowed_share_method(
            "devices/ne101-01/command/reboot",
            &Method::POST,
            true
        ));

        // Configuration writes: NEVER — this is the fix. An interactive
        // share-token holder previously reached any write method on any
        // allowlisted path (install extensions, delete agents, rewrite
        // channels).
        for (path, method) in [
            ("extensions/install", Method::POST),
            ("agents/a1", Method::DELETE),
            ("agents/a1", Method::PUT),
            ("messages/ch-1", Method::PUT),
            ("messages/ch-1", Method::DELETE),
            ("devices/ne101-01", Method::DELETE),
            ("extensions/weather/config", Method::PUT),
        ] {
            assert!(
                !is_allowed_share_method(path, &method, true),
                "interactive share must not allow {method} `{path}`"
            );
            assert!(
                !is_allowed_share_method(path, &method, false),
                "read-only share must not allow {method} `{path}`"
            );
        }

        // GET stays open for both modes (path allowlist handles scope).
        assert!(is_allowed_share_method("telemetry/latest", &Method::GET, false));
        assert!(is_allowed_share_method("telemetry/latest", &Method::GET, true));
    }

    #[test]
    fn rewrite_component_transform_refs_updates_all_three_fields() {
        let mut component = StoredComponent {
            id: "comp_1".to_string(),
            component_type: "chart".to_string(),
            position: neomind_storage::dashboards::ComponentPosition {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
                min_w: None,
                min_h: None,
                max_w: None,
                max_h: None,
            },
            title: None,
            data_source: Some(serde_json::json!({
                "type": "transform",
                "transformId": "t1",
                "sourceId": "transform:t1",
                "id": "t1",
                "metricId": "detection_count.value",
                "field": "detection_count.value",
            })),
            display: None,
            config: Some(serde_json::json!({ "_transformId": "t1" })),
            actions: None,
        };

        rewrite_component_transform_refs(
            &mut component,
            "t1",
            "t2",
            "detection_count",
            "detection_count_a1b2c3d4",
        );

        let ds = component.data_source.as_ref().unwrap();
        assert_eq!(ds["transformId"], "t2");
        assert_eq!(ds["sourceId"], "transform:t2");
        assert_eq!(ds["id"], "t2");
        assert_eq!(ds["metricId"], "detection_count_a1b2c3d4.value");
        assert_eq!(ds["field"], "detection_count_a1b2c3d4.value");
        assert_eq!(component.config.as_ref().unwrap()["_transformId"], "t2");
    }

    #[test]
    fn rewrite_component_transform_refs_skips_when_no_marker() {
        let mut component = StoredComponent {
            id: "comp_2".to_string(),
            component_type: "chart".to_string(),
            position: neomind_storage::dashboards::ComponentPosition {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
                min_w: None,
                min_h: None,
                max_w: None,
                max_h: None,
            },
            title: None,
            data_source: Some(serde_json::json!({
                "type": "transform",
                "transformId": "t1",
            })),
            display: None,
            config: None,
            actions: None,
        };

        rewrite_component_transform_refs(
            &mut component,
            "t1",
            "t2",
            "detection_count",
            "detection_count_new",
        );

        // No _transformId marker → helper should NOT touch this component
        assert_eq!(component.data_source.as_ref().unwrap()["transformId"], "t1");
    }

    #[test]
    fn rewrite_component_transform_refs_handles_missing_metric_prefix() {
        let mut component = StoredComponent {
            id: "comp_3".to_string(),
            component_type: "chart".to_string(),
            position: neomind_storage::dashboards::ComponentPosition {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
                min_w: None,
                min_h: None,
                max_w: None,
                max_h: None,
            },
            title: None,
            data_source: Some(serde_json::json!({
                "transformId": "t1",
                "sourceId": "transform:t1",
                "metricId": "custom_field",
            })),
            display: None,
            config: Some(serde_json::json!({ "_transformId": "t1" })),
            actions: None,
        };

        rewrite_component_transform_refs(
            &mut component,
            "t1",
            "t2",
            "detection_count",
            "detection_count_new",
        );

        let ds = component.data_source.as_ref().unwrap();
        assert_eq!(ds["transformId"], "t2");
        assert_eq!(ds["sourceId"], "transform:t2");
        // metricId not prefixed by old output_prefix → unchanged
        assert_eq!(ds["metricId"], "custom_field");
    }

    #[test]
    fn new_output_prefix_appends_short_uuid_suffix() {
        let p1 = new_output_prefix("detection_count");
        let p2 = new_output_prefix("detection_count");
        assert!(p1.starts_with("detection_count_"));
        assert!(p2.starts_with("detection_count_"));
        assert_ne!(p1, p2, "two calls must produce different prefixes");
    }

    #[tokio::test]
    async fn duplicate_dashboard_handler_clones_owned_transform_and_rewrites_refs() {
        use crate::automation::types::{AutomationMetadata, TransformAutomation, TransformScope};

        let src_component = StoredComponent {
            id: "comp_1".to_string(),
            component_type: "chart".to_string(),
            position: neomind_storage::dashboards::ComponentPosition {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
                min_w: None,
                min_h: None,
                max_w: None,
                max_h: None,
            },
            title: None,
            data_source: Some(serde_json::json!({
                "type": "transform",
                "transformId": "t_old",
                "sourceId": "transform:t_old",
                "id": "t_old",
                "metricId": "detection_count.fish",
            })),
            display: None,
            config: Some(serde_json::json!({ "_transformId": "t_old" })),
            actions: None,
        };

        let src_transform = TransformAutomation {
            metadata: AutomationMetadata {
                id: "t_old".to_string(),
                name: "Fish Counter".to_string(),
                description: String::new(),
                enabled: true,
                execution_count: 5,
                last_executed: Some(1000),
                created_at: 900,
                updated_at: 950,
            },
            scope: TransformScope::Global,
            intent: None,
            js_code: Some("return {}".to_string()),
            output_prefix: "detection_count".to_string(),
            complexity: 2,
            operations: None,
        };

        let src_dashboard = StoredDashboard {
            id: "dashboard_src".to_string(),
            name: "My Dashboard".to_string(),
            description: None,
            layout: StoredLayout::default_layout(),
            components: vec![src_component],
            created_at: 100,
            updated_at: 200,
            is_default: Some(true),
            sort_order: Some(3),
        };

        let result = build_duplicate_dashboard(&src_dashboard, vec![src_transform], 10);

        // Dashboard-level assertions
        assert_ne!(result.dashboard.id, "dashboard_src");
        assert!(result.dashboard.id.starts_with("dashboard_"));
        assert_eq!(result.dashboard.name, "My Dashboard (copy)");
        assert_eq!(
            result.dashboard.is_default, None,
            "is_default must NOT be inherited"
        );
        assert_eq!(result.dashboard.sort_order, Some(11), "appended to end");
        assert!(result.dashboard.created_at > 100, "fresh created_at");

        // Transform clones
        assert_eq!(
            result.new_transforms.len(),
            1,
            "exactly one transform cloned"
        );
        let new_t = &result.new_transforms[0];
        assert_ne!(new_t.metadata.id, "t_old");
        assert_eq!(new_t.metadata.name, "Fish Counter (copy)");
        assert_ne!(
            new_t.output_prefix, "detection_count",
            "output_prefix must change"
        );
        assert!(new_t.output_prefix.starts_with("detection_count_"));
        assert_eq!(new_t.metadata.execution_count, 0, "execution_count reset");
        assert_eq!(new_t.metadata.last_executed, None, "last_executed cleared");

        // Component refs rewritten to new transform ID and new output_prefix
        let new_comp = &result.dashboard.components[0];
        let ds = new_comp.data_source.as_ref().unwrap();
        assert_eq!(ds["transformId"], new_t.metadata.id);
        assert_eq!(ds["sourceId"], format!("transform:{}", new_t.metadata.id));
        assert_eq!(ds["id"], new_t.metadata.id);
        assert_eq!(ds["metricId"], format!("{}.fish", new_t.output_prefix));
        assert_eq!(
            new_comp.config.as_ref().unwrap()["_transformId"],
            new_t.metadata.id
        );
    }

    #[test]
    fn duplicate_dashboard_name_appends_copy_even_if_already_suffixed() {
        let src = StoredDashboard {
            id: "x".to_string(),
            name: "X (copy)".to_string(),
            description: None,
            layout: StoredLayout::default_layout(),
            components: vec![],
            created_at: 0,
            updated_at: 0,
            is_default: None,
            sort_order: None,
        };
        let result = build_duplicate_dashboard(&src, vec![], 0);
        assert_eq!(result.dashboard.name, "X (copy) (copy)");
    }

    #[test]
    fn deep_merge_json_merges_objects_and_replaces_scalars() {
        let mut target = serde_json::json!({
            "title": "Old",
            "position": {"x": 0, "y": 0, "w": 4, "h": 2},
            "data_source": {"id": "demo-001", "field": "temperature", "timeWindow": {"type": "last_24hours"}},
            "tags": ["a", "b"]
        });
        let patch = serde_json::json!({
            "title": "New",
            "position": {"w": 6},                       // partial — x/y/h kept
            "data_source": {"timeWindow": {"type": "last_6hours"}}, // nested merge
            "tags": ["z"]                                // arrays REPLACE
        });
        deep_merge_json(&mut target, &patch);
        assert_eq!(target["title"], "New");
        assert_eq!(
            target["position"],
            serde_json::json!({"x": 0, "y": 0, "w": 6, "h": 2})
        );
        assert_eq!(target["data_source"]["id"], "demo-001");
        assert_eq!(target["data_source"]["field"], "temperature");
        assert_eq!(target["data_source"]["timeWindow"]["type"], "last_6hours");
        assert_eq!(target["tags"], serde_json::json!(["z"]));
    }
}
