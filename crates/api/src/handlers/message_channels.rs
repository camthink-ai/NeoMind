//! Message channel API handlers.
//!
//! GET    /api/messages/channels              - List channels
//! POST   /api/messages/channels              - Create channel
//! GET    /api/messages/channels/:name        - Get channel
//! DELETE /api/messages/channels/:name        - Delete channel
//! POST   /api/messages/channels/:name/test   - Test channel
//! GET    /api/messages/channels/stats        - Channel stats
//! GET    /api/messages/channels/types        - Available channel types
//! GET    /api/messages/channels/types/:type/schema - Channel schema

use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};

use edge_ai_messages::{
    ChannelInfo, ChannelStats, MessageChannel, ChannelFactory,
};



#[cfg(feature = "webhook")]
use edge_ai_messages::WebhookChannelFactory;

#[cfg(feature = "email")]
use edge_ai_messages::EmailChannelFactory;

use super::{ServerState, common::{HandlerResult, ok}};

// Import json macro for handler responses
use serde_json::json;
use crate::models::ErrorResponse;

/// Create channel request.
#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: String,
    #[serde(flatten)]
    pub config: serde_json::Value,
}

/// Channel list response.
#[derive(Debug, Serialize)]
pub struct ChannelListResponse {
    pub channels: Vec<ChannelInfo>,
    pub count: usize,
    pub stats: ChannelStats,
}

/// List all channels.
/// GET /api/messages/channels
pub async fn list_channels_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;
    let registry_guard = registry.read().await;
    let channels = registry_guard.list_info().await;
    let stats = registry_guard.get_stats().await;

    ok(json!({
        "channels": channels,
        "count": channels.len(),
        "stats": stats,
    }))
}

/// Get a specific channel.
/// GET /api/messages/channels/:name
pub async fn get_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;
    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_info(&name)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Channel not found"))?;

    ok(json!(info))
}

/// List available channel types.
/// GET /api/messages/channels/types
pub async fn list_channel_types_handler() -> HandlerResult<serde_json::Value> {
    let types = edge_ai_messages::list_channel_types();

    ok(json!({
        "types": types,
        "count": types.len(),
    }))
}

/// Get channel type schema.
/// GET /api/messages/channels/types/:type/schema
pub async fn get_channel_type_schema_handler(
    Path(channel_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let schema = edge_ai_messages::get_channel_schema(&channel_type)
        .ok_or_else(|| ErrorResponse::not_found("Channel type not found"))?;

    let info = edge_ai_messages::list_channel_types()
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
/// POST /api/messages/channels
pub async fn create_channel_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateChannelRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;

    // Check if channel already exists
    {
        let registry_guard = registry.read().await;
        if registry_guard.get(&req.name).await.is_some() {
            return Err(ErrorResponse::bad_request("Channel already exists"));
        }
    }

    // Prepare config value for storing
    let config_value = req.config.clone();

    // Create channel based on type
    let channel: std::sync::Arc<dyn MessageChannel> = match req.channel_type.as_str() {
        "console" => {
            let factory = edge_ai_messages::ConsoleChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        "memory" => {
            let factory = edge_ai_messages::MemoryChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "webhook")]
        "webhook" => {
            let factory = WebhookChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "email")]
        "email" => {
            let factory = EmailChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Unknown channel type: {}",
                req.channel_type
            )));
        }
    };

    // Register channel
    {
        let registry_guard = registry.write().await;
        registry_guard.register_with_config(req.name.clone(), channel, config_value).await;
    }

    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_info(&req.name)
        .await
        .expect("Just created");

    ok(json!({
        "message": "Channel created successfully",
        "message_zh": "通道创建成功",
        "channel": info,
    }))
}

/// Delete a channel.
/// DELETE /api/messages/channels/:name
pub async fn delete_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;
    let registry_guard = registry.write().await;

    let removed = registry_guard.unregister(&name).await;

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
/// POST /api/messages/channels/:name/test
pub async fn test_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;
    let registry_guard = registry.read().await;

    let result = registry_guard
        .test(&name)
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!(result))
}

/// Get channel statistics.
/// GET /api/messages/channels/stats
pub async fn get_channel_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.message_manager.channels().await;
    let registry_guard = registry.read().await;
    let stats = registry_guard.get_stats().await;

    ok(json!(stats))
}

/// Router for message channel endpoints.
pub fn message_channels_router() -> axum::Router<ServerState> {
    use axum::routing::{get, post, delete};

    axum::Router::new()
        .route("/messages/channels", get(list_channels_handler).post(create_channel_handler))
        .route("/messages/channels/stats", get(get_channel_stats_handler))
        .route("/messages/channels/types", get(list_channel_types_handler))
        .route(
            "/messages/channels/types/:type/schema",
            get(get_channel_type_schema_handler),
        )
        .route("/messages/channels/:name", get(get_channel_handler))
        .route("/messages/channels/:name", delete(delete_channel_handler))
        .route("/messages/channels/:name/test", post(test_channel_handler))
}
