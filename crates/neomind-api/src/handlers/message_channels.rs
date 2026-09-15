//! Message channel API handlers.
//!
//! GET    /api/messages/channels              - List channels
//! POST   /api/messages/channels              - Create channel
//! GET    /api/messages/channels/:name        - Get channel
//! DELETE /api/messages/channels/:name        - Delete channel
//! POST   /api/messages/channels/:name/test   - Test channel
//! PUT    /api/messages/channels/:name/enabled - Toggle channel enabled state
//! GET    /api/messages/channels/:name/filter - Get channel filter
//! PUT    /api/messages/channels/:name/filter - Update channel filter
//! GET    /api/messages/channels/stats        - Channel stats
//! GET    /api/messages/channels/types        - Available channel types
//! GET    /api/messages/channels/types/:type/schema - Channel schema

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use neomind_messages::channels::ChannelFilter;
use neomind_messages::{ChannelFactory, ChannelInfo, ChannelStats, MessageChannel};

#[cfg(feature = "webhook")]
use neomind_messages::WebhookChannelFactory;

#[cfg(feature = "email")]
use neomind_messages::EmailChannelFactory;

#[cfg(feature = "telegram")]
use neomind_messages::TelegramChannelFactory;

#[cfg(feature = "wecom")]
use neomind_messages::WeComChannelFactory;

#[cfg(feature = "dingtalk")]
use neomind_messages::DingTalkChannelFactory;

#[cfg(feature = "slack")]
use neomind_messages::SlackChannelFactory;

#[cfg(feature = "feishu")]
use neomind_messages::FeishuChannelFactory;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};

// Import json macro for handler responses
use crate::models::ErrorResponse;
use serde_json::json;

/// Create channel request.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: String,
    /// Channel-specific configuration. NOTE the serde `flatten`: these
    /// fields are flattened into the request TOP LEVEL — a webhook channel
    /// is `{"name","channel_type","url"}` with `url` at the root, NOT
    /// nested under a `config` object.
    #[serde(flatten)]
    #[schema(value_type = Object)]
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
#[utoipa::path(
    get,
    path = "/api/messages/channels",
    tag = "channels",
    responses(
        (status = 200, description = "Configured notification channels"),
    )
)]
pub async fn list_channels_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
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
#[utoipa::path(
    get,
    path = "/api/messages/channels/{name}",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    responses(
        (status = 200, description = "One channel with config (secrets redacted)"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_info(&name)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Channel not found"))?;

    ok(json!(info))
}

/// List available channel types.
/// GET /api/messages/channels/types
#[utoipa::path(
    get,
    path = "/api/messages/channels/types",
    tag = "channels",
    responses(
        (status = 200, description = "Static metadata for every channel type"),
    )
)]
pub async fn list_channel_types_handler() -> HandlerResult<serde_json::Value> {
    let types = neomind_messages::list_channel_types();

    ok(json!({
        "types": types,
        "count": types.len(),
    }))
}

/// Get channel type schema.
/// GET /api/messages/channels/types/:type/schema
#[utoipa::path(
    get,
    path = "/api/messages/channels/types/{type}/schema",
    tag = "channels",
    params(
        ("type" = String, Path, description = "Channel type key"),
    ),
    responses(
        (status = 200, description = "JSON-schema of the config object for a channel type"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_channel_type_schema_handler(
    Path(channel_type): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let schema = neomind_messages::get_channel_schema(&channel_type)
        .ok_or_else(|| ErrorResponse::not_found("Channel type not found"))?;

    let info = neomind_messages::list_channel_types()
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
#[utoipa::path(
    post,
    path = "/api/messages/channels",
    tag = "channels",
    request_body = CreateChannelRequest,
    responses(
        (status = 200, description = "Channel created"),
    )
)]
pub async fn create_channel_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateChannelRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;

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
        #[cfg(feature = "telegram")]
        "telegram" => {
            let factory = TelegramChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "wecom")]
        "wecom" => {
            let factory = WeComChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "dingtalk")]
        "dingtalk" => {
            let factory = DingTalkChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "slack")]
        "slack" => {
            let factory = SlackChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "feishu")]
        "feishu" => {
            let factory = FeishuChannelFactory;
            factory
                .create(&req.config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Unknown channel type: {}. Supported types: webhook, email, telegram, wecom, dingtalk, slack, feishu",
                req.channel_type
            )));
        }
    };

    // Register channel
    {
        let registry_guard = registry.write().await;
        registry_guard
            .register_with_config(req.name.clone(), channel, config_value)
            .await;
    }

    let registry_guard = registry.read().await;
    let info = registry_guard.get_info(&req.name).await.ok_or_else(|| {
        ErrorResponse::internal("Channel was registered but could not be retrieved")
    })?;

    ok(json!({
        "message": "Channel created successfully",
        "message_zh": "通道创建成功",
        "channel": info,
    }))
}

/// Delete a channel.
/// DELETE /api/messages/channels/:name
#[utoipa::path(
    delete,
    path = "/api/messages/channels/{name}",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    responses(
        (status = 200, description = "Channel deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
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
#[utoipa::path(
    post,
    path = "/api/messages/channels/{name}/test",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    responses(
        (status = 200, description = "Test notification sent through the channel"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn test_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    let result = registry_guard.test(&name).await.map_err(|e| match e {
        neomind_messages::Error::NotFound(msg) => ErrorResponse::not_found(msg),
        other => ErrorResponse::internal(other.to_string()),
    })?;

    ok(json!(result))
}

/// Request to update a channel.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub config: serde_json::Value,
}

/// Update a channel's configuration.
/// PUT /api/messages/channels/:name
#[utoipa::path(
    put,
    path = "/api/messages/channels/{name}",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    request_body = UpdateChannelRequest,
    responses(
        (status = 200, description = "Channel config updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_channel_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateChannelRequest>,
) -> HandlerResult<serde_json::Value> {
    use std::sync::Arc;

    let registry = state.core.message_manager.channels().await;

    // Get existing channel info first
    let (channel_type, enabled, recipients) = {
        let registry_guard = registry.read().await;
        let info = registry_guard
            .get_info(&name)
            .await
            .ok_or_else(|| ErrorResponse::not_found("Channel not found"))?;
        (
            info.channel_type.clone(),
            info.enabled,
            info.recipients.clone().unwrap_or_default(),
        )
    };

    // Include recipients in config for email channels
    let mut config = req.config.clone();
    if channel_type == "email" && !recipients.is_empty() {
        if let Some(obj) = config.as_object_mut() {
            obj.insert("recipients".to_string(), serde_json::json!(recipients));
        }
    }

    // Create new channel with updated config
    let channel: Arc<dyn MessageChannel> = match channel_type.as_str() {
        #[cfg(feature = "webhook")]
        "webhook" => {
            let factory = WebhookChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "email")]
        "email" => {
            let factory = EmailChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "telegram")]
        "telegram" => {
            let factory = TelegramChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "wecom")]
        "wecom" => {
            let factory = WeComChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "dingtalk")]
        "dingtalk" => {
            let factory = DingTalkChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "slack")]
        "slack" => {
            let factory = SlackChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        #[cfg(feature = "feishu")]
        "feishu" => {
            let factory = FeishuChannelFactory;
            factory
                .create(&config)
                .map_err(|e| ErrorResponse::bad_request(format!("Invalid config: {}", e)))?
        }
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Unknown channel type: {}",
                channel_type
            )));
        }
    };

    // Register the updated channel (this replaces the old one)
    {
        let registry_guard = registry.write().await;
        registry_guard
            .register_with_config(name.clone(), channel, config)
            .await;
    }

    // Restore enabled state
    {
        let registry_guard = registry.read().await;
        if let Err(e) = registry_guard.set_enabled(&name, enabled).await {
            tracing::warn!("Failed to restore enabled state: {}", e);
        }
    }

    // Note: recipients remain in the registry's memory map after update
    // and are already included in the channel config for email channels

    // Get updated info
    let registry_guard = registry.read().await;
    let info = registry_guard
        .get_info(&name)
        .await
        .ok_or_else(|| ErrorResponse::internal("Channel was updated but could not be retrieved"))?;

    ok(json!({
        "message": "Channel updated successfully",
        "message_zh": "通道更新成功",
        "channel": info,
    }))
}

/// Request to toggle channel enabled state.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct ToggleEnabledRequest {
    pub enabled: bool,
}

/// Toggle channel enabled state.
/// PUT /api/messages/channels/:name/enabled
#[utoipa::path(
    put,
    path = "/api/messages/channels/{name}/enabled",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    request_body = ToggleEnabledRequest,
    responses(
        (status = 200, description = "Channel enabled/disabled"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn toggle_enabled_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<ToggleEnabledRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    registry_guard
        .set_enabled(&name, req.enabled)
        .await
        .map_err(|e| match e {
            neomind_messages::Error::NotFound(msg) => ErrorResponse::not_found(msg),
            other => ErrorResponse::internal(other.to_string()),
        })?;

    // Get updated info
    let info = registry_guard
        .get_info(&name)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Channel not found"))?;

    ok(json!({
        "message": if req.enabled { "Channel enabled successfully" } else { "Channel disabled successfully" },
        "message_zh": if req.enabled { "通道已启用" } else { "通道已禁用" },
        "channel": info,
    }))
}

/// Get channel statistics.
/// GET /api/messages/channels/stats
#[utoipa::path(
    get,
    path = "/api/messages/channels/stats",
    tag = "channels",
    responses(
        (status = 200, description = "Delivery counters per channel"),
    )
)]
pub async fn get_channel_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;
    let stats = registry_guard.get_stats().await;

    ok(json!(stats))
}

// ========== Recipient Management ==========

/// Add recipient request.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct AddRecipientRequest {
    pub email: String,
}

/// Get recipients for a channel.
/// GET /api/messages/channels/:name/recipients
#[utoipa::path(
    get,
    path = "/api/messages/channels/{name}/recipients",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    responses(
        (status = 200, description = "Recipients of an email-type channel"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn list_recipients_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    // Check if channel exists
    if registry_guard.get(&name).await.is_none() {
        return Err(ErrorResponse::not_found("Channel not found"));
    }

    let recipients = registry_guard.get_recipients(&name).await;

    ok(json!({
        "channel": name,
        "recipients": recipients,
        "count": recipients.len(),
    }))
}

/// Add a recipient to a channel.
/// POST /api/messages/channels/:name/recipients
#[utoipa::path(
    post,
    path = "/api/messages/channels/{name}/recipients",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    request_body = AddRecipientRequest,
    responses(
        (status = 200, description = "Recipient added"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn add_recipient_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<AddRecipientRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    registry_guard
        .add_recipient(&name, &req.email)
        .await
        .map_err(|e| ErrorResponse::bad_request(e.to_string()))?;

    let recipients = registry_guard.get_recipients(&name).await;

    ok(json!({
        "message": "Recipient added successfully",
        "message_zh": "收件人添加成功",
        "channel": name,
        "recipients": recipients,
    }))
}

/// Remove a recipient from a channel.
/// DELETE /api/messages/channels/:name/recipients/:email
#[utoipa::path(
    delete,
    path = "/api/messages/channels/{name}/recipients/{email}",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
        ("email" = String, Path, description = "Recipient email"),
    ),
    responses(
        (status = 200, description = "Recipient removed"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn remove_recipient_handler(
    State(state): State<ServerState>,
    Path((name, email)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    // URL decode the email
    let email_decoded = urlencoding::decode(&email)
        .map(|s| s.to_string())
        .unwrap_or(email);

    registry_guard
        .remove_recipient(&name, &email_decoded)
        .await
        .map_err(|e| match e {
            neomind_messages::Error::NotFound(msg) => ErrorResponse::not_found(&msg),
            _ => ErrorResponse::bad_request(e.to_string()),
        })?;

    let recipients = registry_guard.get_recipients(&name).await;

    ok(json!({
        "message": "Recipient removed successfully",
        "message_zh": "收件人删除成功",
        "channel": name,
        "recipients": recipients,
    }))
}

// ========== Filter Management ==========

/// Get channel filter configuration.
/// GET /api/messages/channels/:name/filter
#[utoipa::path(
    get,
    path = "/api/messages/channels/{name}/filter",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    responses(
        (status = 200, description = "Severity/category filter of a channel"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_channel_filter_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    // Check if channel exists
    if registry_guard.get(&name).await.is_none() {
        return Err(ErrorResponse::not_found("Channel not found"));
    }

    let filter = registry_guard.get_filter(&name).await;

    ok(json!(filter))
}

/// Request to update channel filter.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct UpdateFilterRequest {
    pub source_types: Vec<String>,
    pub categories: Vec<String>,
    pub min_severity: Option<String>,
}

/// Update channel filter configuration.
/// PUT /api/messages/channels/:name/filter
#[utoipa::path(
    put,
    path = "/api/messages/channels/{name}/filter",
    tag = "channels",
    params(
        ("name" = String, Path, description = "Channel name"),
    ),
    request_body = UpdateFilterRequest,
    responses(
        (status = 200, description = "Filter updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_channel_filter_handler(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateFilterRequest>,
) -> HandlerResult<serde_json::Value> {
    let registry = state.core.message_manager.channels().await;
    let registry_guard = registry.read().await;

    // Check if channel exists
    if registry_guard.get(&name).await.is_none() {
        return Err(ErrorResponse::not_found(format!(
            "Channel not found: {}",
            name
        )));
    }

    // Build ChannelFilter. If the user explicitly passes a min_severity
    // string, validate it — silently coercing an unrecognized value to None
    // would disable severity filtering without any error, surprising
    // operators who think they set a threshold.
    let min_severity = match req.min_severity.as_ref() {
        None => None,
        Some(sev) => {
            let parsed = neomind_messages::MessageSeverity::from_string(sev);
            if parsed.is_none() {
                return Err(ErrorResponse::bad_request(format!(
                    "Invalid min_severity '{}'. Valid values: info, warning, error, critical",
                    sev
                )));
            }
            parsed
        }
    };
    let filter = ChannelFilter {
        source_types: req.source_types,
        categories: req.categories,
        min_severity,
    };

    // Save filter
    registry_guard
        .set_filter(&name, filter.clone())
        .await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "message": "Filter updated successfully",
        "message_zh": "过滤器更新成功",
        "channel": name,
        "filter": filter
    }))
}

/// Router for message channel endpoints.
pub fn message_channels_router() -> axum::Router<ServerState> {
    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        .route(
            "/messages/channels",
            get(list_channels_handler).post(create_channel_handler),
        )
        .route("/messages/channels/stats", get(get_channel_stats_handler))
        .route("/messages/channels/types", get(list_channel_types_handler))
        .route(
            "/messages/channels/types/:type/schema",
            get(get_channel_type_schema_handler),
        )
        .route("/messages/channels/:name", get(get_channel_handler))
        .route("/messages/channels/:name", delete(delete_channel_handler))
        .route("/messages/channels/:name/test", post(test_channel_handler))
        .route(
            "/messages/channels/:name/enabled",
            put(toggle_enabled_handler),
        )
        // Recipient management (for email channels)
        .route(
            "/messages/channels/:name/recipients",
            get(list_recipients_handler),
        )
        .route(
            "/messages/channels/:name/recipients",
            post(add_recipient_handler),
        )
        .route(
            "/messages/channels/:name/recipients/:email",
            delete(remove_recipient_handler),
        )
        // Filter management
        .route(
            "/messages/channels/:name/filter",
            get(get_channel_filter_handler),
        )
        .route(
            "/messages/channels/:name/filter",
            put(update_channel_filter_handler),
        )
}
