//! API handlers organized by domain.

pub mod common;
pub mod basic;
pub mod sessions;
pub mod devices;
pub mod rules;
pub mod alerts;
pub mod settings;
pub mod workflows;
pub mod memory;
pub mod scenarios;
pub mod tools;
pub mod mqtt;
pub mod hass;
pub mod events;
pub mod commands;
pub mod decisions;
pub mod stats;
pub mod auth;
pub mod auth_users;
pub mod bulk;
pub mod config;
pub mod plugins;
pub mod search;
pub mod llm_backends;

// Re-export ServerState so handlers can use it
pub use crate::server::ServerState;

// Re-export commonly used handler functions
pub use basic::health_handler;
pub use sessions::{
    create_session_handler,
    delete_session_handler,
    get_session_handler,
    get_session_history_handler,
    list_sessions_handler,
    update_session_handler,
    cleanup_sessions_handler,
    chat_handler,
    ws_chat_handler,
};
pub use devices::{
    list_devices_handler, get_device_handler, add_device_handler, delete_device_handler,
    send_command_handler, read_metric_handler, query_metric_handler, aggregate_metric_handler,
    list_device_types_handler, get_device_type_handler, register_device_type_handler,
    validate_device_type_handler, delete_device_type_handler,
    discover_devices_handler, discovery_info_handler,
    generate_mdl_handler,
    discover_hass_devices_handler, stop_hass_discovery_handler, process_hass_discovery_handler, register_aggregated_hass_device_handler,
    hass_discovery_status_handler, get_hass_discovered_devices_handler, clear_hass_discovered_devices_handler,
    get_device_telemetry_handler, get_device_telemetry_summary_handler, get_device_command_history_handler,
};
pub use rules::{create_rule_handler, list_rules_handler};
pub use alerts::{
    acknowledge_alert_handler,
    create_alert_handler,
    get_alert_handler,
    list_alerts_handler,
};
pub use settings::{
    get_llm_settings_handler,
    list_ollama_models_handler,
    set_llm_handler,
    test_llm_handler,
};
pub use events::{
    event_stream_handler, event_websocket_handler, event_history_handler,
    events_query_handler, event_stats_handler, subscribe_events_handler,
    unsubscribe_events_handler,
};
// Commands API
pub use commands::{
    list_commands_handler, get_command_handler, retry_command_handler,
    cancel_command_handler, get_command_stats_handler, cleanup_commands_handler,
};
// Decisions API
pub use decisions::{
    list_decisions_handler, get_decision_handler, execute_decision_handler,
    approve_decision_handler, reject_decision_handler, delete_decision_handler,
    get_decision_stats_handler, cleanup_decisions_handler,
};
// Stats API
pub use stats::{
    get_system_stats_handler, get_device_stats_handler, get_rule_stats_handler,
};
// Plugins API
pub use plugins::{
    list_plugins_handler, get_plugin_handler, register_plugin_handler, unregister_plugin_handler,
    enable_plugin_handler, disable_plugin_handler, start_plugin_handler, stop_plugin_handler,
    plugin_health_handler, get_plugin_config_handler, update_plugin_config_handler,
    execute_plugin_command_handler, get_plugin_stats_handler, discover_plugins_handler,
    list_plugins_by_type_handler, get_plugin_types_handler,
};
// LLM Backends API
pub use llm_backends::{
    list_backends_handler, get_backend_handler, create_backend_handler,
    update_backend_handler, delete_backend_handler, activate_backend_handler,
    test_backend_handler, list_backend_types_handler, get_backend_schema_handler,
    get_backend_stats_handler,
};
// User Authentication API
pub use auth_users::{
    login_handler, logout_handler, register_handler, get_current_user_handler,
    change_password_handler, list_users_handler, create_user_handler, delete_user_handler,
};
