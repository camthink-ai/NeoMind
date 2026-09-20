//! `capabilities` handlers — split from the former extensions.rs monolith.

use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use super::*;

/// GET /api/extensions/:id/event-subscriptions
///
/// Get event subscriptions for an extension.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/event-subscriptions",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Server events an extension subscribes to"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_event_subscriptions_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    if !state.extensions.runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    let subscriptions = state.extensions.runtime.get_event_subscriptions(&id).await;

    ok(serde_json::json!({
        "extension_id": id,
        "subscriptions": subscriptions,
    }))
}

/// GET /api/extensions/:id/descriptor
///
/// Get the full extension descriptor (metadata, commands, metrics, capabilities).
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/descriptor",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Raw extension descriptor (manifest)"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_extension_descriptor_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Build descriptor response from ExtensionRuntimeInfo
    ok(serde_json::json!({
        "metadata": {
            "id": info.metadata.id,
            "name": info.metadata.name,
            "version": info.metadata.version,
            "description": info.metadata.description,
            "author": info.metadata.author,
        },
        "commands": info.commands,
        "metrics": info.metrics,
        "is_isolated": info.is_isolated,
        "is_running": info.is_running,
    }))
}

/// Extension capability for dashboard/automation integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCapabilityDto {
    pub extension_id: String,
    pub extension_name: String,
    #[serde(rename = "type")]
    pub cap_type: String, // "provider", "processor", "hybrid"
    pub metrics: Vec<ExtensionMetricDto>,
    pub commands: Option<Vec<ExtensionCommandDto>>,
    pub tools: Option<Vec<ExtensionToolDto>>,
}

/// Extension command for transform operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCommandDto {
    pub name: String,
    pub description: String,
}

/// Extension tool for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionToolDto {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
}

/// GET /api/extensions/capabilities
///
/// Get all extension capabilities for dashboard/automation integration
///
/// V2: Uses extension metrics and command parameters schema
#[utoipa::path(
    get,
    path = "/api/extensions/capabilities",
    tag = "extensions",
    responses(
        (status = 200, description = "Capabilities contributed by every extension"),
    )
)]
pub async fn list_extension_capabilities_handler(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<ExtensionCapabilityDto>> {
    let extensions = state.extensions.runtime.list().await;

    let mut capabilities = Vec::new();

    for ext_info in extensions {
        // Skip extensions with no metrics or commands
        if ext_info.metrics.is_empty() && ext_info.commands.is_empty() {
            continue;
        }

        let mut metrics = Vec::new();
        let mut commands = Vec::new();
        let mut tools = Vec::new();

        // V2: Extract metrics from extension metrics (not command output fields)
        for metric in &ext_info.metrics {
            metrics.push(ExtensionMetricDto {
                name: metric.name.clone(),
                data_type: format!("{:?}", metric.data_type),
                unit: if metric.unit.is_empty() {
                    None
                } else {
                    Some(metric.unit.clone())
                },
            });
        }

        // Extract command info for transforms and agents
        for cmd in &ext_info.commands {
            // Command info for transforms
            commands.push(ExtensionCommandDto {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
            });

            // Command info for agents (as tools)
            tools.push(ExtensionToolDto {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
                parameters: Some(build_parameters_schema(&cmd.parameters)),
            });
        }

        // Create provider capability for dashboard (if has metrics)
        if !metrics.is_empty() {
            capabilities.push(ExtensionCapabilityDto {
                extension_id: ext_info.metadata.id.clone(),
                extension_name: ext_info.metadata.name.clone(),
                cap_type: "provider".to_string(),
                metrics: metrics.clone(),
                commands: Some(commands.clone()),
                tools: Some(tools.clone()),
            });
        }

        // Create processor capability for transforms (if has commands)
        if !commands.is_empty() {
            capabilities.push(ExtensionCapabilityDto {
                extension_id: ext_info.metadata.id.clone(),
                extension_name: ext_info.metadata.name.clone(),
                cap_type: "processor".to_string(),
                metrics: vec![], // Not used for processor type
                commands: Some(commands.clone()),
                tools: Some(tools.clone()),
            });
        }
    }

    ok(capabilities)
}

// ============================================================================
// EXTENSION MARKETPLACE API
