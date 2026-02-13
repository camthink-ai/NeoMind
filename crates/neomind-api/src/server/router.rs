//! Application router configuration.

use axum::{
    routing::{delete, get, post, put},
    Router,
};

use super::assets;
use super::middleware::rate_limit_middleware;
use super::types::ServerState;
use super::types::MAX_REQUEST_BODY_SIZE;
use crate::auth::hybrid_auth_middleware;
use crate::auth_users::jwt_auth_middleware;

/// Create the application router.
pub async fn create_router() -> Router {
    create_router_with_state(ServerState::new().await)
}

/// Create the application router with a specific state.
pub fn create_router_with_state(state: ServerState) -> Router {
    use crate::handlers::{
        agents, auth as auth_handlers, auth_users, automations, basic, bulk, commands, config,
        dashboards, devices, events, extensions, llm_backends, memory, message_channels, messages,
        mqtt, rules, search, sessions, settings, setup, stats, suggestions, test_data, tools,
    };

    // Public routes (no authentication required)
    let public_routes = Router::new()
        // Health check endpoints
        .route("/api/health", get(basic::health_handler))
        .route("/api/health/status", get(basic::health_status_handler))
        .route("/api/health/live", get(basic::liveness_handler))
        .route("/api/health/ready", get(basic::readiness_handler))
        // Auth status (public - shows if auth is enabled)
        .route("/api/auth/status", get(auth_handlers::auth_status_handler))
        // User authentication (public - login and register)
        .route("/api/auth/login", post(auth_users::login_handler))
        .route("/api/auth/register", post(auth_users::register_handler))
        // Setup endpoints (public - only available when no users exist)
        .route("/api/setup/status", get(setup::setup_status_handler))
        .route(
            "/api/setup/initialize",
            post(setup::initialize_admin_handler),
        )
        .route("/api/setup/complete", post(setup::complete_setup_handler))
        .route(
            "/api/setup/llm-config",
            post(setup::save_llm_config_handler),
        )
        // LLM Backends Types API (public - read-only metadata)
        .route(
            "/api/llm-backends/types",
            get(llm_backends::list_backend_types_handler),
        )
        .route(
            "/api/llm-backends/types/:type/schema",
            get(llm_backends::get_backend_schema_handler),
        )
        // LLM Backends (public - read-only for viewing)
        .route(
            "/api/llm-backends",
            get(llm_backends::list_backends_handler),
        )
        .route(
            "/api/llm-backends/:id",
            get(llm_backends::get_backend_handler),
        )
        .route(
            "/api/llm-backends/stats",
            get(llm_backends::get_backend_stats_handler),
        )
        // Ollama models API (public - fetch available models with capabilities)
        .route(
            "/api/llm-backends/ollama/models",
            get(llm_backends::list_ollama_models_handler),
        )
        // Messages Channel Types API (public - read-only metadata)
        .route(
            "/api/messages/channels/types",
            get(message_channels::list_channel_types_handler),
        )
        .route(
            "/api/messages/channels/types/:type/schema",
            get(message_channels::get_channel_type_schema_handler),
        )
        // Messages Channels (public - read-only for viewing)
        .route(
            "/api/messages/channels",
            get(message_channels::list_channels_handler),
        )
        .route(
            "/api/messages/channels/:name",
            get(message_channels::get_channel_handler),
        )
        .route(
            "/api/messages/channels/stats",
            get(message_channels::get_channel_stats_handler),
        )
        // Extensions API (public - read-only endpoints for viewing dynamic extensions)
        .route("/api/extensions", get(extensions::list_extensions_handler))
        .route(
            "/api/extensions/types",
            get(extensions::list_extension_types_handler),
        )
        .route(
            "/api/extensions/:id",
            get(extensions::get_extension_handler),
        )
        .route(
            "/api/extensions/:id/health",
            get(extensions::extension_health_handler),
        )
        .route(
            "/api/extensions/:id/stats",
            get(extensions::get_extension_stats_handler),
        )
        .route(
            "/api/extensions/:id/commands",
            get(extensions::list_extension_commands_handler),
        )
        .route(
            "/api/extensions/:id/data-sources",
            get(extensions::list_extension_data_sources_handler),
        )
        .route(
            "/api/extensions/:id/metrics/:metric/data",
            get(extensions::query_extension_metric_data_handler),
        )
        .route(
            "/api/extensions/capabilities",
            get(extensions::list_extension_capabilities_handler),
        )
        // Discover extensions (public - scans filesystem for available extensions)
        .route(
            "/api/extensions/discover",
            post(extensions::discover_extensions_handler),
        )
        // Extension streaming API (public)
        .route(
            "/api/extensions/:id/stream",
            get(extensions::stream_extension_handler),
        )
        // Test data generation (public - for development)
        .route(
            "/api/test-data/alerts",
            post(test_data::generate_test_alerts_handler),
        )
        .route(
            "/api/test-data/all",
            post(test_data::generate_test_data_handler),
        )
        // Stats API (public - system stats for dashboard components)
        .route("/api/stats/system", get(stats::get_system_stats_handler))
        // Suggestions API (public - provides intelligent input suggestions)
        .route(
            "/api/suggestions",
            get(suggestions::get_suggestions_handler),
        )
        .route(
            "/api/suggestions/categories",
            get(suggestions::get_suggestions_categories_handler),
        )
        // Device Types Cloud API (public - read-only for browsing cloud repository)
        .route(
            "/api/device-types/cloud/list",
            get(devices::list_cloud_device_types_handler),
        )
        // Extension Marketplace API (public - read-only for browsing marketplace)
        .route(
            "/api/extensions/market/list",
            get(extensions::list_marketplace_extensions_handler),
        )
        .route(
            "/api/extensions/market/:id",
            get(extensions::get_marketplace_extension_handler),
        )
        .route(
            "/api/extensions/market/updates",
            get(extensions::check_marketplace_updates_handler),
        )
        // API documentation (public)
        .merge(crate::openapi::swagger_ui());

    // JWT protected routes (require JWT token authentication)
    let jwt_routes = Router::new()
        // User info and session management
        .route("/api/auth/me", get(auth_users::get_current_user_handler))
        .route("/api/auth/logout", post(auth_users::logout_handler))
        .route(
            "/api/auth/change-password",
            post(auth_users::change_password_handler),
        )
        // Apply JWT authentication middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            jwt_auth_middleware,
        ));

    // WebSocket routes - authentication handled in handler
    // Event WebSocket uses message-based auth (more secure than URL parameter)
    // Chat WebSocket and SSE use ?token= parameter for compatibility
    let websocket_routes = Router::new()
        // Event streaming WebSocket/SSE
        .route("/api/events/ws", get(events::event_websocket_handler))
        .route("/api/events/stream", get(events::event_stream_handler))
        // Chat WebSocket (JWT via ?token= parameter)
        .route("/api/chat", get(sessions::ws_chat_handler))
        // Apply only rate limiting (no auth middleware - handled in handlers)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ));

    // Protected routes (require API key or JWT via Authorization header)
    let protected_routes = Router::new()
        // Session management
        .route("/api/sessions", post(sessions::create_session_handler))
        .route("/api/sessions", get(sessions::list_sessions_handler))
        .route(
            "/api/sessions/cleanup",
            post(sessions::cleanup_sessions_handler),
        )
        .route("/api/sessions/:id", get(sessions::get_session_handler))
        .route(
            "/api/sessions/:id/history",
            get(sessions::get_session_history_handler),
        )
        .route("/api/sessions/:id", put(sessions::update_session_handler))
        .route(
            "/api/sessions/:id",
            delete(sessions::delete_session_handler),
        )
        .route("/api/sessions/:id/chat", post(sessions::chat_handler))
        // P0.3: Pending stream state management (for recovery after disconnection)
        .route(
            "/api/sessions/:id/pending",
            get(sessions::get_pending_stream_handler),
        )
        .route(
            "/api/sessions/:id/pending",
            delete(sessions::clear_pending_stream_handler),
        )
        // Devices API
        .route("/api/devices", get(devices::list_devices_handler))
        .route("/api/devices", post(devices::add_device_handler))
        .route("/api/devices/:id", get(devices::get_device_handler))
        .route("/api/devices/:id", put(devices::update_device_handler))
        .route("/api/devices/:id", delete(devices::delete_device_handler))
        .route(
            "/api/devices/:id/current",
            get(devices::get_device_current_handler),
        )
        .route(
            "/api/devices/current-batch",
            post(devices::get_devices_current_batch_handler),
        )
        .route(
            "/api/devices/:id/state",
            get(devices::get_device_state_handler),
        )
        .route(
            "/api/devices/:id/health",
            get(devices::get_device_health_handler),
        )
        .route(
            "/api/devices/:id/refresh",
            post(devices::refresh_device_handler),
        )
        .route(
            "/api/devices/:id/command/:command",
            post(devices::send_command_handler),
        )
        .route(
            "/api/devices/:id/metrics/:metric",
            get(devices::read_metric_handler),
        )
        .route(
            "/api/devices/:id/metrics/:metric/data",
            get(devices::query_metric_handler),
        )
        .route(
            "/api/devices/:id/metrics/:metric/aggregate",
            get(devices::aggregate_metric_handler),
        )
        .route(
            "/api/devices/:id/telemetry",
            get(devices::get_device_telemetry_handler),
        )
        .route(
            "/api/devices/:id/telemetry/summary",
            get(devices::get_device_telemetry_summary_handler),
        )
        .route(
            "/api/devices/:id/commands",
            get(devices::get_device_command_history_handler),
        )
        // Device Types API
        .route("/api/device-types", get(devices::list_device_types_handler))
        .route(
            "/api/device-types/:id",
            get(devices::get_device_type_handler),
        )
        .route(
            "/api/device-types",
            post(devices::register_device_type_handler),
        )
        .route(
            "/api/device-types",
            put(devices::validate_device_type_handler),
        )
        .route(
            "/api/device-types/:id",
            delete(devices::delete_device_type_handler),
        )
        .route(
            "/api/device-types/generate-from-samples",
            post(devices::generate_device_type_from_samples_handler),
        )
        // Device Type Import from Cloud API
        .route(
            "/api/device-types/cloud/import",
            post(devices::import_cloud_device_types_handler),
        )
        // Device Discovery API
        .route(
            "/api/devices/discover",
            post(devices::discover_devices_handler),
        )
        .route(
            "/api/devices/discover/info",
            get(devices::discovery_info_handler),
        )
        // MDL Generation API
        .route(
            "/api/devices/generate-mdl",
            post(devices::generate_mdl_handler),
        )
        // Webhook API - devices can POST data to these endpoints
        .route(
            "/api/devices/webhook/:device_id",
            post(devices::webhook_handler),
        )
        .route(
            "/api/devices/webhook",
            post(devices::webhook_generic_handler),
        )
        .route(
            "/api/devices/:id/webhook-url",
            get(devices::get_webhook_url_handler),
        )
        // Draft Devices API - auto-onboarding
        .route("/api/devices/drafts", get(devices::list_draft_devices))
        .route(
            "/api/devices/drafts/:device_id",
            get(devices::get_draft_device),
        )
        .route(
            "/api/devices/drafts/:device_id",
            put(devices::update_draft_device),
        )
        .route(
            "/api/devices/drafts/:device_id/approve",
            post(devices::approve_draft_device),
        )
        .route(
            "/api/devices/drafts/:device_id/reject",
            post(devices::reject_draft_device),
        )
        .route(
            "/api/devices/drafts/:device_id/analyze",
            post(devices::trigger_draft_analysis),
        )
        .route(
            "/api/devices/drafts/:device_id/enhance",
            post(devices::enhance_draft_with_llm),
        )
        .route(
            "/api/devices/drafts/:device_id/suggest-types",
            get(devices::suggest_device_types),
        )
        .route(
            "/api/devices/drafts/cleanup",
            post(devices::cleanup_draft_devices),
        )
        .route(
            "/api/devices/drafts/type-signatures",
            get(devices::get_type_signatures),
        )
        .route(
            "/api/devices/drafts/config",
            get(devices::get_onboard_config),
        )
        .route(
            "/api/devices/drafts/config",
            put(devices::update_onboard_config),
        )
        .route(
            "/api/devices/drafts/upload",
            post(devices::upload_device_data),
        )
        // Rules API - specific routes first, then parameterized routes
        .route("/api/rules", get(rules::list_rules_handler))
        .route("/api/rules", post(rules::create_rule_handler))
        .route("/api/rules/export", get(rules::export_rules_handler))
        .route("/api/rules/import", post(rules::import_rules_handler))
        .route("/api/rules/resources", get(rules::get_resources_handler))
        .route("/api/rules/validate", post(rules::validate_rule_handler))
        .route("/api/rules/:id", get(rules::get_rule_handler))
        .route("/api/rules/:id", put(rules::update_rule_handler))
        .route("/api/rules/:id", delete(rules::delete_rule_handler))
        .route(
            "/api/rules/:id/enable",
            post(rules::set_rule_status_handler),
        )
        .route("/api/rules/:id/test", post(rules::test_rule_handler))
        .route(
            "/api/rules/:id/history",
            get(rules::get_rule_history_handler),
        )
        // Messages API
        .route("/api/messages", get(messages::list_messages_handler))
        .route("/api/messages", post(messages::create_message_handler))
        .route("/api/messages/stats", get(messages::message_stats_handler))
        .route("/api/messages/cleanup", post(messages::cleanup_handler))
        .route(
            "/api/messages/acknowledge",
            post(messages::bulk_acknowledge_handler),
        )
        .route(
            "/api/messages/resolve",
            post(messages::bulk_resolve_handler),
        )
        .route("/api/messages/delete", post(messages::bulk_delete_handler))
        .route("/api/messages/:id", get(messages::get_message_handler))
        .route(
            "/api/messages/:id",
            delete(messages::delete_message_handler),
        )
        .route(
            "/api/messages/:id/acknowledge",
            post(messages::acknowledge_message_handler),
        )
        .route(
            "/api/messages/:id/resolve",
            post(messages::resolve_message_handler),
        )
        .route(
            "/api/messages/:id/archive",
            post(messages::archive_message_handler),
        )
        // Messages Channels API (write operations - protected)
        .route(
            "/api/messages/channels",
            post(message_channels::create_channel_handler),
        )
        .route(
            "/api/messages/channels/:name",
            delete(message_channels::delete_channel_handler),
        )
        .route(
            "/api/messages/channels/:name/test",
            post(message_channels::test_channel_handler),
        )
        // LLM Generation API (one-shot, no session)
        .route("/api/llm/generate", post(settings::llm_generate_handler))
        // Global Timezone Settings API
        .route("/api/settings/timezone", get(settings::get_timezone))
        .route("/api/settings/timezone", put(settings::update_timezone))
        .route("/api/settings/timezones", get(settings::list_timezones))
        // Unified Automations API
        .route(
            "/api/automations",
            get(automations::list_automations_handler),
        )
        .route(
            "/api/automations",
            post(automations::create_automation_handler),
        )
        .route(
            "/api/automations/export",
            get(automations::export_automations_handler),
        )
        .route(
            "/api/automations/import",
            post(automations::import_automations_handler),
        )
        .route(
            "/api/automations/analyze-intent",
            post(automations::analyze_intent_handler),
        )
        .route(
            "/api/automations/templates",
            get(automations::list_templates_handler),
        )
        .route(
            "/api/automations/:id",
            get(automations::get_automation_handler),
        )
        .route(
            "/api/automations/:id",
            put(automations::update_automation_handler),
        )
        .route(
            "/api/automations/:id",
            delete(automations::delete_automation_handler),
        )
        .route(
            "/api/automations/:id/enable",
            post(automations::set_automation_status_handler),
        )
        .route(
            "/api/automations/:id/convert",
            post(automations::convert_automation_handler),
        )
        .route(
            "/api/automations/:id/conversion-info",
            get(automations::get_conversion_info_handler),
        )
        .route(
            "/api/automations/:id/executions",
            get(automations::get_automations_executions_handler),
        )
        // Transform API (data processing)
        .route(
            "/api/automations/transforms/process",
            post(automations::process_data_handler),
        )
        .route(
            "/api/automations/transforms/:id/test",
            post(automations::test_transform_handler),
        )
        .route(
            "/api/automations/transforms/test-code",
            post(automations::test_transform_code_handler),
        )
        .route(
            "/api/automations/transforms",
            get(automations::list_transforms_handler),
        )
        .route(
            "/api/automations/transforms/metrics",
            get(automations::list_virtual_metrics_handler),
        )
        // Transform Output Data Source API (auto-registered outputs)
        .route(
            "/api/automations/transforms/data-sources",
            get(automations::list_transform_data_sources_handler),
        )
        .route(
            "/api/automations/transforms/:id/data-sources",
            get(automations::get_transform_data_sources_handler),
        )
        .route(
            "/api/automations/transforms/data-sources/:data_source_id",
            get(automations::get_transform_data_source_handler),
        )
        // AI Agents API - User-defined automation agents
        .route("/api/agents", get(agents::list_agents))
        .route("/api/agents", post(agents::create_agent))
        .route("/api/agents/:id", get(agents::get_agent))
        .route("/api/agents/:id", put(agents::update_agent))
        .route("/api/agents/:id", delete(agents::delete_agent))
        .route("/api/agents/:id/execute", post(agents::execute_agent))
        .route("/api/agents/:id/status", post(agents::set_agent_status))
        .route(
            "/api/agents/:id/executions",
            get(agents::get_agent_executions),
        )
        .route(
            "/api/agents/:id/executions/:execution_id",
            get(agents::get_execution),
        )
        .route("/api/agents/:id/memory", get(agents::get_agent_memory))
        .route("/api/agents/:id/memory", delete(agents::clear_agent_memory))
        .route("/api/agents/:id/stats", get(agents::get_agent_stats))
        .route(
            "/api/agents/validate-cron",
            post(agents::validate_cron_expression),
        )
        .route(
            "/api/agents/validate-llm",
            post(agents::validate_llm_backend),
        )
        // User messages API
        .route("/api/agents/:id/messages", get(agents::get_user_messages))
        .route("/api/agents/:id/messages", post(agents::add_user_message))
        .route(
            "/api/agents/:id/messages",
            delete(agents::clear_user_messages),
        )
        .route(
            "/api/agents/:id/messages/:message_id",
            delete(agents::delete_user_message),
        )
        // Memory API
        .route("/api/memory/stats", get(memory::get_memory_stats_handler))
        .route("/api/memory/query", get(memory::query_memory_handler))
        .route(
            "/api/memory/consolidate/:session_id",
            post(memory::consolidate_memory_handler),
        )
        .route(
            "/api/memory/short-term",
            get(memory::get_short_term_handler),
        )
        .route(
            "/api/memory/short-term",
            post(memory::add_short_term_handler),
        )
        .route(
            "/api/memory/short-term",
            delete(memory::clear_short_term_handler),
        )
        .route(
            "/api/memory/mid-term/:session_id",
            get(memory::get_session_history_handler),
        )
        .route("/api/memory/mid-term", post(memory::add_mid_term_handler))
        .route(
            "/api/memory/mid-term/search",
            get(memory::search_mid_term_handler),
        )
        .route(
            "/api/memory/mid-term",
            delete(memory::clear_mid_term_handler),
        )
        .route(
            "/api/memory/long-term/search",
            get(memory::search_knowledge_handler),
        )
        .route(
            "/api/memory/long-term/category/:category",
            get(memory::get_knowledge_by_category_handler),
        )
        .route(
            "/api/memory/long-term/device/:device_id",
            get(memory::get_device_knowledge_handler),
        )
        .route(
            "/api/memory/long-term/popular",
            get(memory::get_popular_knowledge_handler),
        )
        .route("/api/memory/long-term", post(memory::add_knowledge_handler))
        .route(
            "/api/memory/long-term",
            delete(memory::clear_long_term_handler),
        )
        // Tools API
        .route("/api/tools", get(tools::list_tools_handler))
        .route(
            "/api/tools/:name/schema",
            get(tools::get_tool_schema_handler),
        )
        .route("/api/tools/metrics", get(tools::get_tool_metrics_handler))
        .route(
            "/api/tools/:name/execute",
            post(tools::execute_tool_handler),
        )
        .route(
            "/api/tools/format-for-llm",
            get(tools::format_for_llm_handler),
        )
        // MQTT Management API
        .route("/api/mqtt/status", get(mqtt::get_mqtt_status_handler))
        .route(
            "/api/mqtt/subscriptions",
            get(mqtt::list_subscriptions_handler),
        )
        .route("/api/mqtt/subscribe", post(mqtt::subscribe_handler))
        .route("/api/mqtt/unsubscribe", post(mqtt::unsubscribe_handler))
        .route(
            "/api/mqtt/subscribe/:device_id",
            post(mqtt::subscribe_device_handler),
        )
        .route(
            "/api/mqtt/unsubscribe/:device_id",
            post(mqtt::unsubscribe_device_handler),
        )
        // External Brokers API
        .route("/api/brokers", get(mqtt::list_brokers_handler))
        .route("/api/brokers", post(mqtt::create_broker_handler))
        .route("/api/brokers/:id", get(mqtt::get_broker_handler))
        .route("/api/brokers/:id", put(mqtt::update_broker_handler))
        .route("/api/brokers/:id", delete(mqtt::delete_broker_handler))
        .route("/api/brokers/:id/test", post(mqtt::test_broker_handler))
        // Commands API
        .route("/api/commands", get(commands::list_commands_handler))
        .route("/api/commands/:id", get(commands::get_command_handler))
        .route(
            "/api/commands/:id/retry",
            post(commands::retry_command_handler),
        )
        .route(
            "/api/commands/:id/cancel",
            post(commands::cancel_command_handler),
        )
        .route(
            "/api/commands/stats",
            get(commands::get_command_stats_handler),
        )
        .route(
            "/api/commands/cleanup",
            post(commands::cleanup_commands_handler),
        )
        // Stats API (devices and rules require auth, system info is public)
        .route("/api/stats/devices", get(stats::get_device_stats_handler))
        .route("/api/stats/rules", get(stats::get_rule_stats_handler))
        // Bulk Operations API
        .route("/api/bulk/alerts", post(bulk::bulk_create_alerts_handler))
        .route(
            "/api/bulk/alerts/resolve",
            post(bulk::bulk_resolve_alerts_handler),
        )
        .route(
            "/api/bulk/alerts/acknowledge",
            post(bulk::bulk_acknowledge_alerts_handler),
        )
        .route(
            "/api/bulk/alerts/delete",
            post(bulk::bulk_delete_alerts_handler),
        )
        .route(
            "/api/bulk/sessions/delete",
            post(bulk::bulk_delete_sessions_handler),
        )
        .route(
            "/api/bulk/devices/delete",
            post(bulk::bulk_delete_devices_handler),
        )
        .route(
            "/api/bulk/devices/command",
            post(bulk::bulk_device_command_handler),
        )
        .route(
            "/api/bulk/device-types/delete",
            post(bulk::bulk_delete_device_types_handler),
        )
        // Config Import/Export API
        .route("/api/config/export", get(config::export_config_handler))
        .route("/api/config/import", post(config::import_config_handler))
        .route(
            "/api/config/validate",
            post(config::validate_config_handler),
        )
        // Global Search API
        .route("/api/search", get(search::global_search_handler))
        .route(
            "/api/search/suggestions",
            get(search::search_suggestions_handler),
        )
        // Dashboards API
        .route("/api/dashboards", get(dashboards::list_dashboards_handler))
        .route(
            "/api/dashboards",
            post(dashboards::create_dashboard_handler),
        )
        .route(
            "/api/dashboards/:id",
            get(dashboards::get_dashboard_handler),
        )
        .route(
            "/api/dashboards/:id",
            put(dashboards::update_dashboard_handler),
        )
        .route(
            "/api/dashboards/:id",
            delete(dashboards::delete_dashboard_handler),
        )
        .route(
            "/api/dashboards/:id/default",
            post(dashboards::set_default_dashboard_handler),
        )
        .route(
            "/api/dashboards/templates",
            get(dashboards::list_templates_handler),
        )
        .route(
            "/api/dashboards/templates/:id",
            get(dashboards::get_template_handler),
        )
        // Auth management API (also protected)
        .route("/api/auth/keys", get(auth_handlers::list_keys_handler))
        .route("/api/auth/keys", post(auth_handlers::create_key_handler))
        .route(
            "/api/auth/keys/:id",
            delete(auth_handlers::delete_key_handler),
        )
        // Extensions API (write operations - protected)
        .route(
            "/api/extensions",
            post(extensions::register_extension_handler),
        )
        .route(
            "/api/extensions/register-all",
            post(extensions::register_all_discovered_handler),
        )
        .route(
            "/api/extensions/:id",
            delete(extensions::unregister_extension_handler),
        )
        .route(
            "/api/extensions/:id/start",
            post(extensions::start_extension_handler),
        )
        .route(
            "/api/extensions/:id/stop",
            post(extensions::stop_extension_handler),
        )
        .route(
            "/api/extensions/:id/command",
            post(extensions::execute_extension_command_handler),
        )
        .route(
            "/api/extensions/:id/invoke",
            post(extensions::invoke_extension_handler),
        )
        // Extension Configuration (protected)
        .route(
            "/api/extensions/:id/config",
            get(extensions::get_extension_config_handler),
        )
        .route(
            "/api/extensions/:id/config",
            put(extensions::update_extension_config_handler),
        )
        .route(
            "/api/extensions/:id/reload",
            post(extensions::reload_extension_handler),
        )
        // Extension Marketplace (install endpoint - protected)
        .route(
            "/api/extensions/market/install",
            post(extensions::install_marketplace_extension_handler),
        )
        // LLM Backends API (write operations - protected)
        .route(
            "/api/llm-backends",
            post(llm_backends::create_backend_handler),
        )
        .route(
            "/api/llm-backends/:id",
            put(llm_backends::update_backend_handler),
        )
        .route(
            "/api/llm-backends/:id",
            delete(llm_backends::delete_backend_handler),
        )
        .route(
            "/api/llm-backends/:id/activate",
            post(llm_backends::activate_backend_handler),
        )
        .route(
            "/api/llm-backends/:id/test",
            post(llm_backends::test_backend_handler),
        )
        // Apply rate limiting middleware to all protected routes
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        // Apply hybrid authentication middleware (supports both JWT tokens and API keys)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            hybrid_auth_middleware,
        ));

    // Admin routes (require JWT + Admin role)
    let admin_routes = Router::new()
        // User management (admin only)
        .route("/api/users", get(auth_users::list_users_handler))
        .route("/api/users", post(auth_users::create_user_handler))
        .route(
            "/api/users/:username",
            delete(auth_users::delete_user_handler),
        )
        // Apply JWT authentication middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            jwt_auth_middleware,
        ));

    // Combine all routes
    // IMPORTANT: More specific routes must come before catch-all routes.
    // Also, routes with their own middleware must be merged BEFORE routes
    // with wildcard middleware to avoid route masking.

    // Add debug-only routes
    #[cfg(debug_assertions)]
    let debug_routes = Router::new()
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            MAX_REQUEST_BODY_SIZE,
        ))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        );

    let router = public_routes.merge(websocket_routes); // WebSocket routes with custom auth

    #[cfg(debug_assertions)]
    let router = router.merge(debug_routes);

    let router = router
        .merge(jwt_routes)
        .merge(admin_routes)
        .merge(protected_routes);

    // Static file routes - serve embedded frontend assets
    let static_routes = Router::new()
        .route("/assets/*path", get(assets::serve_asset))
        .route("/*path", get(assets::serve_asset))
        .route("/", get(assets::serve_index));

    router
        // Merge static routes before fallback
        .merge(static_routes)
        // Apply middleware layers
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            MAX_REQUEST_BODY_SIZE,
        ))
        // CORS layer
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state)
}
