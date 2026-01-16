//! Alert channel management handlers.
//!
//! This module provides API endpoints for managing alert notification channels.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use edge_ai_alerts::{
    ChannelInfo, ChannelPluginRegistry, ChannelStats, ChannelTypeInfo,
    ConsoleChannel, DynAlertChannel, MemoryChannel, TestResult,
};
use edge_ai_core::alerts::ChannelFactory;

#[cfg(feature = "webhook")]
use edge_ai_alerts::WebhookChannelFactory;

#[cfg(feature = "email")]
use edge_ai_alerts::EmailChannelFactory;

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

/// ============================================================================
// Request/Response Types
/// ============================================================================

/// Create channel request.
#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    /// Unique channel name
    pub name: String,
    /// Channel type (console, memory, webhook, email)
    pub channel_type: String,
    /// Channel configuration
    #[serde(flatten)]
    pub config: serde_json::Value,
}

/// Update channel request.
#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    /// New configuration
    pub config: serde_json::Value,
}

/// Channel list response.
#[derive(Debug, Serialize)]
pub struct ChannelListResponse {
    /// List of channels
    pub channels: Vec<ChannelInfo>,
    /// Total count
    pub count: usize,
    /// Statistics
    pub stats: ChannelStats,
}

/// Channel types list response.
#[derive(Debug, Serialize)]
pub struct ChannelTypesResponse {
    /// Available channel types
    pub types: Vec<ChannelTypeInfo>,
    /// Count
    pub count: usize,
}

/// Channel type with schema response.
#[derive(Debug, Serialize)]
pub struct ChannelTypeSchemaResponse {
    /// Channel type info
    #[serde(flatten)]
    pub info: ChannelTypeInfo,
    /// Configuration schema
    pub config_schema: serde_json::Value,
}

/// ============================================================================
// Handlers
/// ============================================================================

/// List all alert channels.
/// GET /api/alert-channels
pub async fn list_channels_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;
    let registry_guard = registry.read().await;
    let channels = registry_guard.list_channels_info().await;
    let stats = registry_guard.get_stats().await;

    ok(json!({
        "channels": channels,
        "count": channels.len(),
        "stats": stats,
    }))
}

/// Get a specific channel by name.
/// GET /api/alert-channels/:name
pub async fn get_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;
    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_channel_info(&name)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Channel not found"))?;

    ok(json!(info))
}

/// List available channel types.
/// GET /api/alert-channels/types
pub async fn list_channel_types_handler() -> HandlerResult<serde_json::Value> {
    let types = ChannelPluginRegistry::list_available_types();

    ok(json!({
        "types": types,
        "count": types.len(),
    }))
}

/// Get channel type configuration schema.
/// GET /api/alert-channels/types/:type/schema
pub async fn get_channel_type_schema_handler(
    Path(channel_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let schema = ChannelPluginRegistry::get_type_schema(&channel_type)
        .ok_or_else(|| ErrorResponse::not_found("Channel type not found"))?;

    let info = ChannelPluginRegistry::list_available_types()
        .into_iter()
        .find(|t| t.id == channel_type)
        .ok_or_else(|| ErrorResponse::not_found("Channel type not found"))?;

    ok(json!({
        "id": info.id,
        "name": info.name,
        "name_zh": info.name_zh,
        "description": info.description,
        "description_zh": info.description_zh,
        "icon": info.icon,
        "category": info.category,
        "config_schema": schema,
    }))
}

/// Create a new channel.
/// POST /api/alert-channels
pub async fn create_channel_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateChannelRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;

    // Check if channel already exists
    {
        let registry_guard = registry.read().await;
        if registry_guard.get_channel(&req.name).await.is_some() {
            return Err(ErrorResponse::bad_request("Channel already exists"));
        }
    }

    // Create channel based on type
    let channel: DynAlertChannel = match req.channel_type.as_str() {
        "console" => {
            let factory = edge_ai_alerts::ConsoleChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(&format!("Invalid config: {}", e)))?
        }
        "memory" => {
            let factory = edge_ai_alerts::MemoryChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(&format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "webhook")]
        "webhook" => {
            let factory = WebhookChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(&format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "email")]
        "email" => {
            let factory = EmailChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(&format!("Invalid config: {}", e)))?
        }
        _ => {
            return Err(ErrorResponse::bad_request(&format!(
                "Unknown channel type: {}",
                req.channel_type
            )));
        }
    };

    {
        let mut registry_guard = registry.write().await;
        registry_guard.register_channel(req.name.clone(), channel).await;
    }

    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_channel_info(&req.name)
        .await
        .expect("Just created");

    ok(json!({
        "message": "Channel created successfully",
        "message_zh": "通道创建成功",
        "channel": info,
    }))
}

/// Delete a channel.
/// DELETE /api/alert-channels/:name
pub async fn delete_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;
    let mut registry_guard = registry.write().await;

    let removed = registry_guard
        .unregister_channel(&name)
        .await;

    if !removed {
        return Err(ErrorResponse::not_found("Channel not found"));
    }

    ok(json!({
        "message": "Channel deleted successfully",
        "message_zh": "通道删除成功",
        "name": name,
    }))
}

/// Test a channel.
/// POST /api/alert-channels/:name/test
pub async fn test_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;
    let registry_guard = registry.read().await;

    let result = registry_guard
        .test_channel(&name)
        .await
        .map_err(|e: edge_ai_alerts::Error| ErrorResponse::internal(&e.to_string()))?;

    ok(json!(result))
}

/// Get channel statistics.
/// GET /api/alert-channels/stats
pub async fn get_channel_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.alert_manager.channel_registry().await;
    let registry_guard = registry.read().await;
    let stats = registry_guard.get_stats().await;

    ok(json!(stats))
}

/// ============================================================================
// Router
/// ============================================================================

/// Returns the alert channels router.
pub fn alert_channels_router() -> axum::Router<ServerState> {
    use axum::routing::{get, post, delete};

    axum::Router::new()
        .route("/alert-channels", get(list_channels_handler).post(create_channel_handler))
        .route("/alert-channels/stats", get(get_channel_stats_handler))
        .route("/alert-channels/types", get(list_channel_types_handler))
        .route(
            "/alert-channels/types/:type/schema",
            get(get_channel_type_schema_handler),
        )
        .route("/alert-channels/:name", get(get_channel_handler))
        .route("/alert-channels/:name", delete(delete_channel_handler))
        .route("/alert-channels/:name/test", post(test_channel_handler))
}
