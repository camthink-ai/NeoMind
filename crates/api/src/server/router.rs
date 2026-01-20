//! Application router configuration.

use axum::{
    Router,
    routing::{delete, get, post, put},
};

use super::middleware::rate_limit_middleware;
use super::types::MAX_REQUEST_BODY_SIZE;
use super::types::ServerState;
use crate::auth::hybrid_auth_middleware;
use crate::auth_users::jwt_auth_middleware;

/// Create the application router.
pub async fn create_router() -> Router {
    create_router_with_state(ServerState::new().await)
}

/// Create the application router with a specific state.
pub fn create_router_with_state(state: ServerState) -> Router {
    use crate::handlers::{
        alert_channels, alerts, automations, auth as auth_handlers, auth_users, basic, bulk, commands, config,
        decisions, devices, events, extensions, llm_backends, memory, mqtt, plugins, rules,
        search, sessions, settings, stats, test_data, tools, workflows,
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
        // LLM Backends Types API (public - read-only metadata)
        .route("/api/llm-backends/types", get(llm_backends::list_backend_types_handler))
        .route("/api/llm-backends/types/:type/schema", get(llm_backends::get_backend_schema_handler))
        // LLM Backends (public - read-only for viewing)
        .route("/api/llm-backends", get(llm_backends::list_backends_handler))
        .route("/api/llm-backends/:id", get(llm_backends::get_backend_handler))
        .route("/api/llm-backends/stats", get(llm_backends::get_backend_stats_handler))
        // Device Adapter Types (public - read-only metadata)
        .route("/api/device-adapters/types", get(plugins::list_adapter_types_handler))
        // Alert Channels Types API (public - read-only metadata)
        .route("/api/alert-channels/types", get(alert_channels::list_channel_types_handler))
        .route("/api/alert-channels/types/:type/schema", get(alert_channels::get_channel_type_schema_handler))
        // Alert Channels (public - read-only for viewing)
        .route("/api/alert-channels", get(alert_channels::list_channels_handler))
        .route("/api/alert-channels/:name", get(alert_channels::get_channel_handler))
        .route("/api/alert-channels/stats", get(alert_channels::get_channel_stats_handler))
        // Extensions API (public - read-only endpoints for viewing dynamic extensions)
        .route("/api/extensions", get(extensions::list_extensions_handler))
        .route("/api/extensions/types", get(extensions::list_extension_types_handler))
        .route("/api/extensions/:id", get(extensions::get_extension_handler))
        .route("/api/extensions/:id/health", get(extensions::extension_health_handler))
        .route("/api/extensions/:id/stats", get(extensions::get_extension_stats_handler))
        // Plugins API (deprecated - use Extensions API for dynamic extensions)
        .route("/api/plugins", get(plugins::list_plugins_handler))
        .route("/api/plugins/:id", get(plugins::get_plugin_handler))
        .route("/api/plugins/:id/config", get(plugins::get_plugin_config_handler))
        .route("/api/plugins/:id/health", get(plugins::plugin_health_handler))
        .route("/api/plugins/:id/stats", get(plugins::get_plugin_stats_handler))
        .route("/api/plugins/types", get(plugins::get_plugin_types_handler))
        .route("/api/plugins/type/:type", get(plugins::list_plugins_by_type_handler))
        // Device Adapter Plugins (public - read-only)
        .route("/api/plugins/device-adapters", get(plugins::list_device_adapter_plugins_handler))
        .route("/api/plugins/device-adapters/stats", get(plugins::get_device_adapter_stats_handler))
        .route("/api/plugins/:id/devices", get(plugins::get_adapter_devices_handler))
        // Test data generation (public - for development)
        .route("/api/test-data/alerts", post(test_data::generate_test_alerts_handler))
        .route("/api/test-data/events", post(test_data::generate_test_events_handler))
        .route("/api/test-data/all", post(test_data::generate_test_data_handler))
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

    // WebSocket routes - authentication handled in handler via query parameter
    // These routes bypass middleware because WebSocket doesn't support custom headers reliably
    let websocket_routes = Router::new()
        // Event streaming WebSocket/SSE (JWT via ?token= parameter)
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
        // Events API (REST endpoints)
        .route("/api/events/history", get(events::event_history_handler))
        .route("/api/events", get(events::events_query_handler))
        .route("/api/events/stats", get(events::event_stats_handler))
        .route(
            "/api/events/subscribe",
            post(events::subscribe_events_handler),
        )
        .route(
            "/api/events/subscribe/:id",
            delete(events::unsubscribe_events_handler),
        )
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
        // Devices API
        .route("/api/devices", get(devices::list_devices_handler))
        .route("/api/devices", post(devices::add_device_handler))
        .route("/api/devices/:id", get(devices::get_device_handler))
        .route("/api/devices/:id", put(devices::update_device_handler))
        .route("/api/devices/:id", delete(devices::delete_device_handler))
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
        .route(
            "/api/devices/:id/metrics/list",
            get(devices::list_device_metrics_debug_handler),
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
        .route("/api/devices/drafts/:device_id", get(devices::get_draft_device))
        .route("/api/devices/drafts/:device_id", put(devices::update_draft_device))
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
        .route("/api/devices/drafts/cleanup", post(devices::cleanup_draft_devices))
        .route(
            "/api/devices/drafts/type-signatures",
            get(devices::get_type_signatures),
        )
        .route("/api/devices/drafts/config", get(devices::get_onboard_config))
        .route("/api/devices/drafts/config", put(devices::update_onboard_config))
        .route("/api/devices/drafts/upload", post(devices::upload_device_data))
        // Rules API - specific routes first, then parameterized routes
        .route("/api/rules", get(rules::list_rules_handler))
        .route("/api/rules", post(rules::create_rule_handler))
        .route("/api/rules/export", get(rules::export_rules_handler))
        .route("/api/rules/import", post(rules::import_rules_handler))
        .route("/api/rules/resources", get(rules::get_resources_handler))
        .route("/api/rules/validate", post(rules::validate_rule_handler))
        .route("/api/rules/templates", get(rules::get_templates_handler))
        .route("/api/rules/templates/fill", post(rules::fill_template_handler))
        .route("/api/rules/generate", post(rules::generate_rule_handler))
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
        // Alerts API
        .route("/api/alerts", get(alerts::list_alerts_handler))
        .route("/api/alerts", post(alerts::create_alert_handler))
        .route("/api/alerts/:id", get(alerts::get_alert_handler))
        .route(
            "/api/alerts/:id/acknowledge",
            post(alerts::acknowledge_alert_handler),
        )
        // Alert Channels API (write operations - protected)
        .route("/api/alert-channels", post(alert_channels::create_channel_handler))
        .route("/api/alert-channels/:name", delete(alert_channels::delete_channel_handler))
        .route("/api/alert-channels/:name/test", post(alert_channels::test_channel_handler))
        // LLM Generation API (one-shot, no session)
        .route("/api/llm/generate", post(settings::llm_generate_handler))
        // Workflows API
        .route("/api/workflows", get(workflows::list_workflows_handler))
        .route("/api/workflows", post(workflows::create_workflow_handler))
        .route("/api/workflows/:id", get(workflows::get_workflow_handler))
        .route(
            "/api/workflows/:id",
            put(workflows::update_workflow_handler),
        )
        .route(
            "/api/workflows/:id",
            delete(workflows::delete_workflow_handler),
        )
        .route(
            "/api/workflows/:id/enable",
            post(workflows::set_workflow_status_handler),
        )
        .route(
            "/api/workflows/:id/execute",
            post(workflows::execute_workflow_handler),
        )
        .route(
            "/api/workflows/:id/executions",
            get(workflows::get_workflow_executions_handler),
        )
        .route(
            "/api/workflows/:id/executions/:exec_id",
            get(workflows::get_execution_handler),
        )
        // Unified Automations API
        .route("/api/automations", get(automations::list_automations_handler))
        .route("/api/automations", post(automations::create_automation_handler))
        .route("/api/automations/export", get(automations::export_automations_handler))
        .route("/api/automations/import", post(automations::import_automations_handler))
        .route("/api/automations/analyze-intent", post(automations::analyze_intent_handler))
        .route("/api/automations/generate-code", post(automations::generate_transform_code_handler))
        .route("/api/automations/templates", get(automations::list_templates_handler))
        .route("/api/automations/:id", get(automations::get_automation_handler))
        .route("/api/automations/:id", put(automations::update_automation_handler))
        .route("/api/automations/:id", delete(automations::delete_automation_handler))
        .route("/api/automations/:id/enable", post(automations::set_automation_status_handler))
        .route("/api/automations/:id/convert", post(automations::convert_automation_handler))
        .route("/api/automations/:id/conversion-info", get(automations::get_conversion_info_handler))
        .route("/api/automations/:id/executions", get(automations::get_automations_executions_handler))
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
            "/api/automations/transforms",
            get(automations::list_transforms_handler),
        )
        .route(
            "/api/automations/transforms/metrics",
            get(automations::list_virtual_metrics_handler),
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
        // Decisions API
        .route("/api/decisions", get(decisions::list_decisions_handler))
        .route("/api/decisions/:id", get(decisions::get_decision_handler))
        .route(
            "/api/decisions/:id/execute",
            post(decisions::execute_decision_handler),
        )
        .route(
            "/api/decisions/:id/approve",
            post(decisions::approve_decision_handler),
        )
        .route(
            "/api/decisions/:id/reject",
            post(decisions::reject_decision_handler),
        )
        .route(
            "/api/decisions/:id",
            delete(decisions::delete_decision_handler),
        )
        .route(
            "/api/decisions/stats",
            get(decisions::get_decision_stats_handler),
        )
        .route(
            "/api/decisions/cleanup",
            post(decisions::cleanup_decisions_handler),
        )
        // Stats API
        .route("/api/stats/system", get(stats::get_system_stats_handler))
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
        // Auth management API (also protected)
        .route("/api/auth/keys", get(auth_handlers::list_keys_handler))
        .route("/api/auth/keys", post(auth_handlers::create_key_handler))
        .route(
            "/api/auth/keys/:id",
            delete(auth_handlers::delete_key_handler),
        )
        // Extensions API (write operations - protected)
        .route("/api/extensions", post(extensions::register_extension_handler))
        .route("/api/extensions/discover", post(extensions::discover_extensions_handler))
        .route("/api/extensions/:id", delete(extensions::unregister_extension_handler))
        .route("/api/extensions/:id/start", post(extensions::start_extension_handler))
        .route("/api/extensions/:id/stop", post(extensions::stop_extension_handler))
        .route("/api/extensions/:id/command", post(extensions::execute_extension_command_handler))
        // Plugins API (write operations - deprecated, use Extensions API)
        .route("/api/plugins", post(plugins::register_plugin_handler))
        .route("/api/plugins/:id", delete(plugins::unregister_plugin_handler))
        .route("/api/plugins/:id/enable", post(plugins::enable_plugin_handler))
        .route("/api/plugins/:id/disable", post(plugins::disable_plugin_handler))
        .route("/api/plugins/:id/start", post(plugins::start_plugin_handler))
        .route("/api/plugins/:id/stop", post(plugins::stop_plugin_handler))
        .route("/api/plugins/:id/config", put(plugins::update_plugin_config_handler))
        .route("/api/plugins/:id/command", post(plugins::execute_plugin_command_handler))
        .route("/api/plugins/discover", post(plugins::discover_plugins_handler))
        // Device Adapter Plugin Endpoints (write operations - protected)
        .route("/api/plugins/device-adapters", post(plugins::register_device_adapter_handler))
        // LLM Backends API (write operations - protected)
        .route("/api/llm-backends", post(llm_backends::create_backend_handler))
        .route("/api/llm-backends/:id", put(llm_backends::update_backend_handler))
        .route("/api/llm-backends/:id", delete(llm_backends::delete_backend_handler))
        .route("/api/llm-backends/:id/activate", post(llm_backends::activate_backend_handler))
        .route("/api/llm-backends/:id/test", post(llm_backends::test_backend_handler))
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
        .route("/api/events/test/generate", post(events::generate_test_events_handler))
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

    let router = public_routes
        .merge(websocket_routes); // WebSocket routes with custom auth

    #[cfg(debug_assertions)]
    let router = router.merge(debug_routes);

    let router = router
        .merge(jwt_routes)
        .merge(admin_routes)
        .merge(protected_routes);

    router
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
        // Serve static files
        .fallback_service(tower_http::services::ServeDir::new("static"))
        .with_state(state)
}
