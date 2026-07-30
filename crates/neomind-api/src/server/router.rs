//! Application router configuration.

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    middleware,
    routing::{any, delete, get, post, put},
    Router,
};

use super::assets;
use super::middleware::rate_limit_middleware;
use super::types::ServerState;
use super::types::MAX_EXTENSION_UPLOAD_SIZE;
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
        agents, auth as auth_handlers, auth_users, automations, basic, capabilities, config,
        dashboards, data, data_push, devices, events, extension_stream, extensions,
        frontend_components, im_bridges, images, instances, llm_backends, logs, memory,
        message_channels, messages, mqtt, onboarding, rules, sessions, settings, setup, skills,
        stats, suggestions, tools,
    };

    // Public routes (no authentication required)
    // SECURITY: Only truly safe endpoints belong here.
    // - Health checks, auth, setup, and static metadata are safe.
    // - Device telemetry, data sources, and write operations MUST be in protected_routes.
    let public_routes = Router::new()
        // Health check endpoints
        .route("/api/health", get(basic::health_handler))
        .route("/api/health/status", get(basic::health_status_handler))
        .route("/api/health/live", get(basic::liveness_handler))
        .route("/api/health/ready", get(basic::readiness_handler))
        .route("/api/system/network-info", get(basic::network_info_handler))
        // Auth status (public - shows if auth is enabled)
        .route("/api/auth/status", get(auth_handlers::auth_status_handler))
        // Auth verification (public route - validates credentials internally)
        .route("/api/auth/verify", get(auth_users::get_auth_status_handler))
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
        // LLM Backend Types API (public - static metadata schemas only)
        .route(
            "/api/llm-backends/types",
            get(llm_backends::list_backend_types_handler),
        )
        .route(
            "/api/llm-backends/types/:type/schema",
            get(llm_backends::get_backend_schema_handler),
        )
        // Messages Channel Types API (public - static metadata schemas only)
        .route(
            "/api/messages/channels/types",
            get(message_channels::list_channel_types_handler),
        )
        .route(
            "/api/messages/channels/types/:type/schema",
            get(message_channels::get_channel_type_schema_handler),
        )
        // Extensions API (public - static metadata only)
        .route("/api/extensions", get(extensions::list_extensions_handler))
        .route(
            "/api/extensions/types",
            get(extensions::list_extension_types_handler),
        )
        // Dashboard Components API (must come before :id routes to avoid route conflicts)
        .route(
            "/api/extensions/dashboard-components",
            get(extensions::get_all_dashboard_components_handler),
        )
        .route(
            "/api/extensions/capabilities",
            get(extensions::list_extension_capabilities_handler),
        )
        // Capability API (public - static metadata)
        .route(
            "/api/capabilities",
            get(capabilities::list_capabilities_handler),
        )
        .route(
            "/api/capabilities/:name",
            get(capabilities::get_capability_handler),
        )
        // Image static files (public — URLs contain device_id + timestamp, effectively
        // unguessable; dashboard itself requires login; <img> can't authenticate cross-origin)
        .route("/api/images/*path", get(images::get_image_handler))
        // Tools API (public - static metadata)
        .route("/api/tools", get(tools::list_tools_handler))
        .route("/api/tools/:name", get(tools::get_tool_handler))
        // Extension read-only routes (metadata, health, assets)
        .route(
            "/api/extensions/:id",
            get(extensions::get_extension_handler),
        )
        .route(
            "/api/extensions/:id/health",
            get(extensions::extension_health_handler),
        )
        .route(
            "/api/extensions/:id/commands",
            get(extensions::list_extension_commands_handler),
        )
        .route(
            "/api/extensions/:id/components",
            get(extensions::get_extension_components_handler),
        )
        .route(
            "/api/extensions/:id/assets/*asset_path",
            get(extensions::serve_extension_asset_handler),
        )
        .route(
            "/api/extensions/:id/event-subscriptions",
            get(extensions::get_event_subscriptions_handler),
        )
        .route(
            "/api/extensions/:id/stream/capability",
            get(extension_stream::get_stream_capability_handler),
        )
        .route(
            "/api/extensions/:id/stream/sessions",
            get(extension_stream::list_stream_sessions_handler),
        )
        // Suggestions API (public - input hints for UI)
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
            "/api/extensions/market/:id/readme",
            get(extensions::get_marketplace_extension_readme_handler),
        )
        .route(
            "/api/extensions/market/updates",
            get(extensions::check_marketplace_updates_handler),
        )
        // Share API (public - access shared dashboards without auth)
        .route(
            "/api/share/:token",
            get(dashboards::get_shared_dashboard_handler),
        )
        .route(
            "/api/share/:token/proxy/*path",
            any(dashboards::share_proxy_handler),
        )
        // Frontend Component Marketplace API (public - read-only for browsing marketplace)
        .route(
            "/api/frontend-components/market/list",
            get(frontend_components::market_list_handler),
        )
        // Frontend Component Bundle Serving (public - allows unauthenticated loading)
        .route(
            "/api/frontend-components/:id/bundle",
            get(frontend_components::get_bundle_handler),
        )
        // Webhook POST endpoints — moved to a rate-limited block below (out of
        // public_routes). Reason: these accept untrusted payloads from arbitrary
        // devices (JWT isn't viable for MCUs), so they MUST sit behind
        // `rate_limit_middleware` to prevent flooding. Per-device tokens, adapter
        // API keys, and IP allowlists are enforced inside the handler/adapter.
        // The read-only `webhook-url` GET lives in protected_routes (admin lookup).
        // Onboarding API (public - system setup status)
        .route(
            "/api/onboarding/status",
            get(onboarding::get_onboarding_status_handler),
        )
        .route(
            "/api/onboarding/dismiss",
            post(onboarding::dismiss_onboarding_handler),
        )
        .route(
            "/api/onboarding/reset",
            post(onboarding::reset_onboarding_handler),
        );

    // JWT protected routes (require JWT token authentication)
    let jwt_routes = Router::new()
        // User info and session management
        .route("/api/auth/me", get(auth_users::get_current_user_handler))
        .route("/api/auth/logout", post(auth_users::logout_handler))
        .route(
            "/api/auth/change-password",
            post(auth_users::change_password_handler),
        )
        // Apply hybrid authentication middleware (supports both JWT tokens and API keys)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            hybrid_auth_middleware,
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
        // Extension streaming WebSocket (generic streaming support)
        .route(
            "/api/extensions/:id/stream",
            get(extension_stream::extension_stream_ws),
        )
        // Apply only rate limiting (no auth middleware - handled in handlers)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ));

    // Webhook POST routes — devices can't carry JWT, so these stay unauthenticated
    // at the middleware level, but MUST sit behind `rate_limit_middleware` to
    // prevent flooding. Stronger controls (per-device token, adapter API key, IP
    // allowlist, per-IP discovery throttle) are enforced inside the handler.
    // Auth model: when user auth is disabled (closed LAN edge deployments), this
    // rate limit is the only top-level defense — tune `RateLimiter` accordingly.
    let webhook_routes = Router::new()
        .route("/api/devices/:id/webhook", post(devices::webhook_handler))
        .route(
            "/api/devices/webhook",
            post(devices::webhook_generic_handler),
        )
        // Webhook body limit: 8MB accommodates a 1080p JPEG frame (typical
        // 300KB-1MB) plus JSON envelope and multipart framing overhead. 4K
        // single-frame raw uploads exceed this — devices shooting 4K should
        // downscale first or use chunked upload (not yet implemented). The
        // previous 16MB limit caused excessive memory amplification under
        // concurrent uploads (~80MB peak per request, see webhook handler).
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::server::middleware::webhook_rate_limit_middleware,
        ));

    // Protected routes (require API key or JWT via Authorization header)
    // SECURITY: Sensitive data and all write operations require authentication.
    let protected_routes = Router::new()
        // === Telemetry & Data (moved from public for security) ===
        .route("/api/telemetry", get(data::query_telemetry_handler))
        .route(
            "/api/telemetry/stats",
            get(devices::get_telemetry_stats_handler),
        )
        .route(
            "/api/data/sources",
            get(data::list_all_data_sources_handler),
        )
        .route("/api/stats/system", get(stats::get_system_stats_handler))
        // Diagnostic log archive download (admin/auth-only) — bundles
        // data/logs/*.log.* into a zip for support flows.
        .route("/api/logs/download", get(logs::download_logs_handler))
        // Webhook URL lookup — admin/UI helper, not for devices. Moved here from
        // public_routes to stop leaking device existence (404 vs 200) and the
        // configured NEOMIND_SERVER_URL to unauthenticated callers.
        .route(
            "/api/devices/:id/webhook-url",
            get(devices::get_webhook_url_handler),
        )
        // === LLM Backends (moved from public - expose config/keys) ===
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
        .route(
            "/api/llm-backends/ollama/models",
            get(llm_backends::list_ollama_models_handler),
        )
        .route(
            "/api/llm-backends/llamacpp/server-info",
            get(llm_backends::list_llamacpp_server_info_handler),
        )
        // === Messages Channels (moved from public - expose config) ===
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
        // === IM Bridges (read - expose running bridge platforms) ===
        .route("/api/im-bridges", get(im_bridges::list_bridges_handler))
        .route(
            "/api/im-bridges/:id/invites",
            get(im_bridges::list_invites_handler),
        )
        .route(
            "/api/im-bridges/:id/allowlist",
            get(im_bridges::list_allowlist_handler),
        )
        .route(
            "/api/im-bridges/:id/sessions",
            get(im_bridges::list_sessions_handler),
        )
        // === Skills (moved from public - expose skill configs) ===
        .route("/api/skills", get(skills::list_skills_handler))
        .route("/api/skills/match", post(skills::match_skills_handler))
        .route("/api/skills/:id", get(skills::get_skill_handler))
        // === Extension write operations (moved from public) ===
        .route(
            "/api/extensions/:id",
            delete(extensions::unregister_extension_handler),
        )
        // Tool enable/disable (master + per-command). Distinct paths so they
        // don't get shadowed by the `:id` catch-all above.
        .route(
            "/api/extensions/:id/enabled",
            axum::routing::patch(extensions::set_extension_enabled_handler),
        )
        .route(
            "/api/extensions/:id/commands/:cmd/enabled",
            axum::routing::patch(extensions::set_extension_command_enabled_handler),
        )
        .route(
            "/api/extensions/:id/logs",
            get(extensions::get_extension_logs_handler)
                .delete(extensions::clear_extension_logs_handler),
        )
        .route(
            "/api/extensions/:id/descriptor",
            get(extensions::get_extension_descriptor_handler),
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
            "/api/extensions/:id/push-metrics",
            post(extensions::push_extension_metrics_handler),
        )
        .route(
            "/api/extensions/:id/command",
            post(extensions::execute_extension_command_handler),
        )
        .route(
            "/api/extensions/:id/invoke",
            post(extensions::invoke_extension_handler),
        )
        .route(
            "/api/extensions/:id/reload",
            post(extensions::reload_extension_handler),
        )
        // === Event publishing (requires auth) ===
        .route("/api/events", post(events::publish_event_handler))
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
            "/api/sessions/:id/memory-toggle",
            put(sessions::toggle_memory_handler),
        )
        .route(
            "/api/sessions/:id",
            delete(sessions::delete_session_handler),
        )
        .route("/api/sessions/:id/chat", post(sessions::chat_handler))
        // Skills API (protected - write operations)
        .route("/api/skills", post(skills::create_skill_handler))
        .route("/api/skills/reload", post(skills::reload_skills_handler))
        .route(
            "/api/skills/:id",
            put(skills::update_skill_handler).delete(skills::delete_skill_handler),
        )
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
        .route(
            "/api/devices/ble-provision",
            post(devices::ble_provision_handler),
        )
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
            "/api/devices/:id/command/:command",
            post(devices::send_command_handler),
        )
        .route(
            "/api/devices/:id/telemetry",
            get(devices::get_device_telemetry_handler),
        )
        .route(
            "/api/devices/:id/metrics",
            post(devices::write_metric_handler),
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
        // MDL Generation API
        .route(
            "/api/devices/generate-mdl",
            post(devices::generate_mdl_handler),
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
        .route("/api/rules/:id/trigger", post(rules::trigger_rule_handler))
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
            "/api/messages/channels/:name",
            put(message_channels::update_channel_handler),
        )
        .route(
            "/api/messages/channels/:name/test",
            post(message_channels::test_channel_handler),
        )
        // Data Push API
        .route("/api/data-push", get(data_push::list_push_targets_handler))
        .route(
            "/api/data-push",
            post(data_push::create_push_target_handler),
        )
        .route(
            "/api/data-push/stats",
            get(data_push::get_push_stats_handler),
        )
        .route(
            "/api/data-push/:id",
            get(data_push::get_push_target_handler),
        )
        .route(
            "/api/data-push/:id",
            put(data_push::update_push_target_handler),
        )
        .route(
            "/api/data-push/:id",
            delete(data_push::delete_push_target_handler),
        )
        .route(
            "/api/data-push/:id/test",
            post(data_push::test_push_target_handler),
        )
        .route(
            "/api/data-push/:id/start",
            post(data_push::start_push_target_handler),
        )
        .route(
            "/api/data-push/:id/stop",
            post(data_push::stop_push_target_handler),
        )
        .route(
            "/api/data-push/:id/logs",
            get(data_push::list_delivery_logs_handler),
        )
        // Message Channel Recipients API
        .route(
            "/api/messages/channels/:name/recipients",
            get(message_channels::list_recipients_handler),
        )
        .route(
            "/api/messages/channels/:name/recipients",
            post(message_channels::add_recipient_handler),
        )
        .route(
            "/api/messages/channels/:name/recipients/:email",
            delete(message_channels::remove_recipient_handler),
        )
        // Message Channel Filter API
        .route(
            "/api/messages/channels/:name/filter",
            get(message_channels::get_channel_filter_handler),
        )
        .route(
            "/api/messages/channels/:name/filter",
            put(message_channels::update_channel_filter_handler),
        )
        // Message Channel Toggle Enabled
        .route(
            "/api/messages/channels/:name/enabled",
            put(message_channels::toggle_enabled_handler),
        )
        // IM Bridges API (write operations - create + delete bridges)
        .route("/api/im-bridges", post(im_bridges::create_bridge_handler))
        .route(
            "/api/im-bridges/:id",
            delete(im_bridges::delete_bridge_handler),
        )
        .route(
            "/api/im-bridges/:id/invites",
            post(im_bridges::create_invite_handler),
        )
        .route(
            "/api/im-bridges/:id/invites/:token",
            delete(im_bridges::revoke_invite_handler),
        )
        .route(
            "/api/im-bridges/:id/allowlist/:chat_id",
            delete(im_bridges::remove_allowlist_handler),
        )
        .route(
            "/api/im-bridges/:id/sessions/:chat_id/reset",
            post(im_bridges::reset_session_handler),
        )
        // LLM Generation API (one-shot, no session)
        .route("/api/llm/generate", post(settings::llm_generate_handler))
        // Global Timezone Settings API
        .route("/api/settings/timezone", get(settings::get_timezone))
        .route("/api/settings/timezone", put(settings::update_timezone))
        .route("/api/settings/timezones", get(settings::list_timezones))
        // Retention Configuration API
        .route(
            "/api/settings/retention",
            get(settings::get_retention_config),
        )
        .route(
            "/api/settings/retention",
            put(settings::update_retention_config),
        )
        .route(
            "/api/settings/retention/cleanup",
            post(settings::trigger_retention_cleanup),
        )
        .route(
            "/api/settings/agent",
            get(settings::get_agent_defaults).put(settings::update_agent_defaults),
        )
        .route(
            "/api/settings/device",
            get(settings::get_device_defaults).put(settings::update_device_defaults),
        )
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
        // IMPORTANT: `/api/agents/tools` MUST be declared before `/api/agents/:id`
        // or Axum's router will route `GET /api/agents/tools` to `get_agent` with
        // id="tools". Read-only catalog of the server's ToolRegistry.
        .route("/api/agents/tools", get(agents::list_agent_tools))
        .route("/api/agents/:id", get(agents::get_agent))
        .route("/api/agents/:id", put(agents::update_agent))
        .route("/api/agents/:id", delete(agents::delete_agent))
        .route("/api/agents/:id/execute", post(agents::execute_agent))
        .route("/api/agents/:id/invoke", post(agents::invoke_agent))
        .route("/api/agents/:id/status", post(agents::set_agent_status))
        .route(
            "/api/agents/:id/executions",
            get(agents::get_agent_executions),
        )
        .route(
            "/api/agents/:id/executions/:execution_id",
            get(agents::get_execution),
        )
        .route(
            "/api/agents/:id/executions/details",
            post(agents::batch_get_executions),
        )
        .route("/api/agents/:id/memory", get(agents::get_agent_memory))
        .route("/api/agents/:id/memory", delete(agents::clear_agent_memory))
        .route("/api/agents/:id/stats", get(agents::get_agent_stats))
        .route(
            "/api/agents/:id/available-resources",
            get(agents::get_available_resources),
        )
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
        // System Memory API (Markdown-based)
        .route("/api/memory", get(memory::get_all_memory))
        .route("/api/memory/export", get(memory::export_all))
        .route("/api/memory/stats", get(memory::get_stats))
        .route(
            "/api/memory/config",
            get(memory::get_config).put(memory::update_config),
        )
        .route("/api/memory/compress", post(memory::trigger_compress))
        .route(
            "/api/memory/category/:category",
            get(memory::get_category).put(memory::update_category),
        )
        .route(
            "/api/memory/:source_type/:id",
            get(memory::get_memory_content)
                .put(memory::update_memory_content)
                .delete(memory::delete_memory_file),
        )
        // New file-based memory API (2-file layout)
        .route(
            "/api/memory/file/:target",
            get(memory::get_memory_file).put(memory::update_memory_file),
        )
        // Custom memory files API
        .route("/api/memory/custom", get(memory::list_custom_files))
        .route(
            "/api/memory/custom/:name",
            get(memory::get_custom_file)
                .put(memory::update_custom_file)
                .delete(memory::delete_custom_file),
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
        // Embedded Broker Config API
        .route(
            "/api/mqtt/broker-config",
            get(mqtt::get_broker_config_handler),
        )
        .route(
            "/api/mqtt/broker-config",
            put(mqtt::update_broker_config_handler),
        )
        .route(
            "/api/mqtt/broker-config/credentials",
            post(mqtt::add_credential_handler),
        )
        .route(
            "/api/mqtt/broker-config/credentials/delete",
            post(mqtt::delete_credential_handler),
        )
        .route("/api/mqtt/broker-config/tls", put(mqtt::upload_tls_handler))
        .route(
            "/api/mqtt/broker-config/tls/generate",
            post(mqtt::generate_tls_handler),
        )
        .route(
            "/api/mqtt/broker-config/tls/ca-cert",
            get(mqtt::download_ca_cert_handler),
        )
        // Stats API (devices and rules require auth, system info is public)
        .route("/api/stats/devices", get(stats::get_device_stats_handler))
        .route("/api/stats/rules", get(stats::get_rule_stats_handler))
        // Config Import/Export API
        .route("/api/config/export", get(config::export_config_handler))
        .route("/api/config/import", post(config::import_config_handler))
        .route(
            "/api/config/validate",
            post(config::validate_config_handler),
        )
        // Dashboards API
        .route("/api/dashboards", get(dashboards::list_dashboards_handler))
        .route(
            "/api/dashboards",
            post(dashboards::create_dashboard_handler),
        )
        // Reorder MUST precede /:id to avoid shadowing (literal segments win
        // over dynamic, but explicit ordering is safer & self-documenting).
        .route(
            "/api/dashboards/reorder",
            put(dashboards::reorder_dashboards_handler),
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
            "/api/dashboards/:id/components",
            post(dashboards::add_components_handler),
        )
        .route(
            "/api/dashboards/:id/components",
            delete(dashboards::remove_components_handler),
        )
        .route(
            "/api/dashboards/:id/default",
            post(dashboards::set_default_dashboard_handler),
        )
        .route(
            "/api/dashboards/:id/duplicate",
            post(dashboards::duplicate_dashboard_handler),
        )
        .route(
            "/api/dashboards/templates",
            get(dashboards::list_templates_handler),
        )
        .route(
            "/api/dashboards/templates/:id",
            get(dashboards::get_template_handler),
        )
        // Dashboard share management (protected - require auth)
        .route(
            "/api/dashboards/:id/share",
            post(dashboards::create_share_handler),
        )
        .route(
            "/api/dashboards/:id/share",
            get(dashboards::list_shares_handler),
        )
        .route(
            "/api/dashboards/:id/share/:token",
            delete(dashboards::revoke_share_handler),
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
            "/api/extensions/:id/uninstall",
            delete(extensions::uninstall_extension_handler),
        )
        .route(
            "/api/extensions/:id/start",
            post(extensions::start_extension_handler),
        )
        .route(
            "/api/extensions/:id/stop",
            post(extensions::stop_extension_handler),
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
        // Extension Marketplace (install endpoint - protected)
        .route(
            "/api/extensions/market/install",
            post(extensions::install_marketplace_extension_handler),
        )
        // Extension sync (protected - manual sync from /extensions/ directory)
        .route(
            "/api/extensions/sync",
            post(extensions::sync_extensions_handler),
        )
        .route(
            "/api/extensions/sync-status",
            get(extensions::get_sync_status_handler),
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
        .route(
            "/api/llm-backends/:id/capabilities",
            axum::routing::patch(llm_backends::update_capabilities_override_handler),
        )
        // Instances API (remote backend management)
        .route("/api/instances", get(instances::list_instances_handler))
        .route("/api/instances", post(instances::create_instance_handler))
        .route(
            "/api/instances/:id",
            get(instances::get_instance_handler)
                .put(instances::update_instance_handler)
                .delete(instances::delete_instance_handler),
        )
        .route(
            "/api/instances/:id/test",
            post(instances::test_instance_handler),
        )
        // Frontend Component API (protected - install/uninstall/list)
        .route(
            "/api/frontend-components/market/install",
            post(frontend_components::market_install_handler),
        )
        .route(
            "/api/frontend-components/updates",
            get(frontend_components::check_updates_handler),
        )
        .route(
            "/api/frontend-components",
            get(frontend_components::list_components_handler),
        )
        .route(
            "/api/frontend-components/:id",
            get(frontend_components::get_component_handler)
                .delete(frontend_components::uninstall_component_handler),
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

    // Extension upload routes with larger body limit (100MB for large extension packages)
    // This needs to be a separate router to apply a different body limit
    // Use DefaultBodyLimit::max() for the route-specific limit
    let extension_upload_routes = Router::new()
        .route(
            "/api/extensions/upload/file",
            post(extensions::upload_extension_file_handler)
                .layer(DefaultBodyLimit::max(MAX_EXTENSION_UPLOAD_SIZE)),
        )
        // Apply hybrid authentication middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            hybrid_auth_middleware,
        ))
        // Apply rate limiting middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ));

    // Frontend component upload routes with 5MB body limit
    // Separate router for multipart upload with custom body limit
    let component_upload_routes = Router::new()
        .route(
            "/api/frontend-components",
            post(frontend_components::install_component_handler)
                .layer(DefaultBodyLimit::max(5 * 1024 * 1024)),
        )
        // Apply hybrid authentication middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            hybrid_auth_middleware,
        ))
        // Apply rate limiting middleware
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ));

    // Combine all routes
    // IMPORTANT: More specific routes must come before catch-all routes.
    // Also, routes with their own middleware must be merged BEFORE routes
    // with wildcard middleware to avoid route masking.

    // Add debug-only routes (no body limit here - limits are applied per-router)
    #[cfg(debug_assertions)]
    let debug_routes = Router::new()
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        );

    let router = public_routes
        .merge(websocket_routes) // WebSocket routes with custom auth
        .merge(webhook_routes); // Device webhook POSTs (rate-limited, no auth)

    #[cfg(debug_assertions)]
    let router = router.merge(debug_routes);

    // Apply global body limit to routes that need it (NOT extension upload)
    let limited_routes = Router::new()
        .merge(jwt_routes)
        .merge(admin_routes)
        .merge(protected_routes)
        // Apply global body limit to these routes
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            MAX_REQUEST_BODY_SIZE,
        ))
        // Match DefaultBodyLimit to RequestBodyLimitLayer so the Json/Bytes
        // extractors accept the same payload size. Without this, axum's
        // default 2MB DefaultBodyLimit silently rejects large POST bodies
        // (e.g. base64 images sent to extension command endpoints) with 413,
        // even though RequestBodyLimitLayer allows 10MB.
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_SIZE));

    // Combine all routes - extension_upload_routes has its own larger limit
    let router = router
        .merge(limited_routes)
        .merge(extension_upload_routes)
        .merge(component_upload_routes);

    // Static file routes
    let router = assets::configure_static_file_serving(router);

    router
        // Cache-Control for static assets: immutable for hashed files, no-cache for HTML.
        // Only adds headers when not already set (API responses keep their own headers).
        .layer(middleware::from_fn(cache_headers_middleware))
        // Apply middleware layers (compression, CORS) but NOT body limit - that's applied per-router
        .layer(tower_http::compression::CompressionLayer::new().br(true))
        // CORS layer
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state)
}

/// Middleware that adds Cache-Control headers to static file responses.
///
/// - `index.html` → `no-cache` (users get latest version after upgrades)
/// - `/assets/*`  → `immutable, 1 year` (Vite outputs content-hashed filenames)
/// - Other files  → `1 hour` (reasonable default)
/// - API responses → unchanged (already have their own headers or don't need caching)
async fn cache_headers_middleware(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;

    // Only set Cache-Control on success responses that don't already have it
    if response.status().is_success() && !response.headers().contains_key(header::CACHE_CONTROL) {
        let cc = if path == "/" || path == "/index.html" || path.ends_with(".html") {
            "no-cache, no-store, must-revalidate"
        } else if path.starts_with("/assets/") {
            "public, max-age=31536000, immutable"
        } else if path.starts_with("/api/") {
            // API responses — don't add caching headers
            return response;
        } else {
            "public, max-age=3600"
        };
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(cc));
    }

    response
}
