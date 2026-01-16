//! API handlers organized by domain.

pub mod alerts;
pub mod alert_channels;
pub mod auth;
pub mod auth_users;
pub mod basic;
pub mod bulk;
pub mod commands;
pub mod common;
pub mod config;
pub mod decisions;
pub mod devices;
pub mod events;
pub mod hass;
pub mod llm_backends;
pub mod memory;
pub mod mqtt;
pub mod plugins;
pub mod rules;
pub mod scenarios;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod stats;
pub mod tools;
pub mod workflows;

// Re-export ServerState so handlers can use it
pub use crate::server::ServerState;

// Re-export commonly used handler functions
pub use alerts::{
    acknowledge_alert_handler, create_alert_handler, get_alert_handler, list_alerts_handler,
};
pub use alert_channels::{
    create_channel_handler, delete_channel_handler, get_channel_handler,
    get_channel_stats_handler, get_channel_type_schema_handler, list_channel_types_handler,
    list_channels_handler, test_channel_handler,
};
pub use basic::health_handler;
pub use devices::{
    add_device_handler, aggregate_metric_handler, clear_hass_discovered_devices_handler,
    delete_device_handler, delete_device_type_handler, discover_devices_handler,
    discover_hass_devices_handler, discovery_info_handler, generate_mdl_handler,
    get_device_command_history_handler, get_device_handler, get_device_telemetry_handler,
    get_device_telemetry_summary_handler, get_device_type_handler,
    get_hass_discovered_devices_handler, hass_discovery_status_handler, list_device_metrics_debug_handler,
    list_device_types_handler, list_devices_handler, process_hass_discovery_handler, query_metric_handler,
    read_metric_handler, register_aggregated_hass_device_handler, register_device_type_handler,
    send_command_handler, stop_hass_discovery_handler, validate_device_type_handler,
};
pub use events::{
    event_history_handler, event_stats_handler, event_stream_handler, event_websocket_handler,
    events_query_handler, subscribe_events_handler, unsubscribe_events_handler,
};
pub use rules::{create_rule_handler, list_rules_handler};
pub use sessions::{
    chat_handler, cleanup_sessions_handler, create_session_handler, delete_session_handler,
    get_session_handler, get_session_history_handler, list_sessions_handler,
    update_session_handler, ws_chat_handler,
};
pub use settings::{
    get_llm_settings_handler, list_ollama_models_handler, set_llm_handler, test_llm_handler,
};
// Commands API
pub use commands::{
    cancel_command_handler, cleanup_commands_handler, get_command_handler,
    get_command_stats_handler, list_commands_handler, retry_command_handler,
};
// Decisions API
pub use decisions::{
    approve_decision_handler, cleanup_decisions_handler, delete_decision_handler,
    execute_decision_handler, get_decision_handler, get_decision_stats_handler,
    list_decisions_handler, reject_decision_handler,
};
// Stats API
pub use stats::{get_device_stats_handler, get_rule_stats_handler, get_system_stats_handler};
// Plugins API
pub use plugins::{
    disable_plugin_handler, discover_plugins_handler, enable_plugin_handler,
    execute_plugin_command_handler, get_plugin_config_handler, get_plugin_handler,
    get_plugin_stats_handler, get_plugin_types_handler, list_plugins_by_type_handler,
    list_plugins_handler, plugin_health_handler, register_plugin_handler, start_plugin_handler,
    stop_plugin_handler, unregister_plugin_handler, update_plugin_config_handler,
};
// LLM Backends API
pub use llm_backends::{
    activate_backend_handler, create_backend_handler, delete_backend_handler, get_backend_handler,
    get_backend_schema_handler, get_backend_stats_handler, list_backend_types_handler,
    list_backends_handler, test_backend_handler, update_backend_handler,
};
// User Authentication API
pub use auth_users::{
    change_password_handler, create_user_handler, delete_user_handler, get_current_user_handler,
    list_users_handler, login_handler, logout_handler, register_handler,
};
