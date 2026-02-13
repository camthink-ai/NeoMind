//! Tools management handlers.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

use neomind_tools::{ToolDefinition, ToolRegistry, ToolRegistryBuilder};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// DTO for tool definition responses.
#[derive(Debug, Serialize)]
struct ToolDefinitionDto {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl From<&ToolDefinition> for ToolDefinitionDto {
    fn from(d: &ToolDefinition) -> Self {
        Self {
            name: d.name.clone(),
            description: d.description.clone(),
            parameters: d.parameters.clone(),
        }
    }
}

/// Global tool registry.
/// Initialized with basic tools for testing and API access.
fn get_tool_registry() -> Arc<ToolRegistry> {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<Arc<ToolRegistry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            // Create a registry with basic system help tool for testing
            Arc::new(ToolRegistryBuilder::new().with_system_help_tool().build())
        })
        .clone()
}

/// List all available tools.
///
/// GET /api/tools
pub async fn list_tools_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = get_tool_registry();
    let definitions = registry.definitions();
    let dtos: Vec<ToolDefinitionDto> = definitions.iter().map(ToolDefinitionDto::from).collect();

    ok(json!({
        "tools": dtos,
        "count": dtos.len(),
    }))
}

/// Get tool schema (definition) by name.
///
/// GET /api/tools/:name/schema
pub async fn get_tool_schema_handler(
    State(_state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = get_tool_registry();
    let definitions = registry.definitions();

    let tool = definitions
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| ErrorResponse::not_found("Tool"))?;

    ok(json!({
        "tool": ToolDefinitionDto::from(tool),
    }))
}

/// Get tool execution metrics.
///
/// GET /api/tools/metrics
pub async fn get_tool_metrics_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = get_tool_registry();

    // Get execution counts for each tool
    let definitions = registry.definitions();
    let mut metrics = serde_json::Map::new();

    for tool in definitions {
        // For now, just return the tool info
        // In production, the registry should track actual execution metrics
        metrics.insert(
            tool.name.clone(),
            json!({
                "name": tool.name,
                "description": tool.description,
                "executions": 0,  // TODO: track actual executions
                "errors": 0,
                "avg_duration_ms": 0,
            }),
        );
    }

    ok(json!({
        "metrics": metrics,
    }))
}

/// Execute a tool.
///
/// POST /api/tools/:name/execute
pub async fn execute_tool_handler(
    State(_state): State<ServerState>,
    Path(name): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let registry = get_tool_registry();

    let result = registry
        .execute(&name, args)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to execute tool: {}", e)))?;

    ok(json!({
        "result": result,
    }))
}

/// Format tools for LLM function calling.
///
/// GET /api/tools/format-for-llm
pub async fn format_for_llm_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = get_tool_registry();
    let definitions = registry.definitions();
    let formatted = neomind_tools::format_for_llm(&definitions);

    ok(json!({
        "formatted": formatted,
    }))
}
