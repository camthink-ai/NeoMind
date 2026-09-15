//! Tools API handlers for listing available AI agent tools.

use axum::extract::{Path, State};
use serde_json::{json, Value};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// GET /api/tools - List all available tools.
///
/// Returns a JSON array of tool definitions including name, description,
/// category, and parameters for each registered tool.
#[utoipa::path(
    get,
    path = "/api/tools",
    tag = "tools",
    responses(
        (status = 200, description = "Static tool catalog"),
    )
)]
pub async fn list_tools_handler(State(state): State<ServerState>) -> HandlerResult<Value> {
    let registry = state
        .session_manager()
        .get_tool_registry()
        .await
        .ok_or_else(|| ErrorResponse::not_found("Tool registry"))?;

    let disabled_names = registry.disabled_names();
    let definitions = registry.definitions();

    let tools: Vec<Value> = definitions
        .into_iter()
        .map(|def| {
            json!({
                "name": def.name,
                "description": def.description,
                "category": def.category.as_str(),
                "parameters": def.parameters,
                "deprecated": def.deprecated,
                "version": def.version,
                "disabled": disabled_names.contains(&def.name),
            })
        })
        .collect();

    ok(json!(tools))
}

/// GET /api/tools/:name - Get details for a specific tool.
///
/// Returns the full tool definition for the named tool.
#[utoipa::path(
    get,
    path = "/api/tools/{name}",
    tag = "tools",
    params(
        ("name" = String, Path, description = "Tool name"),
    ),
    responses(
        (status = 200, description = "One tool descriptor"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_tool_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<Value> {
    let registry = state
        .session_manager()
        .get_tool_registry()
        .await
        .ok_or_else(|| ErrorResponse::not_found("Tool registry"))?;

    let tool = registry
        .get(&name)
        .ok_or_else(|| ErrorResponse::not_found(format!("Tool '{}'", name)))?;

    let def = tool.definition();

    ok(json!({
        "name": def.name,
        "description": def.description,
        "category": def.category.as_str(),
        "parameters": def.parameters,
        "deprecated": def.deprecated,
        "replaced_by": def.replaced_by,
        "version": def.version,
        "namespace": def.namespace,
        "scenarios": def.scenarios,
        "relationships": def.relationships,
    }))
}
