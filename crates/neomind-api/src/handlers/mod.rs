//! API handlers organized by domain.

pub mod agents;
pub mod auth;
pub mod auth_users;
pub mod automations;
pub mod basic;
pub mod capabilities;
pub mod common;
pub mod config;
pub mod dashboards;
pub mod data;
pub mod data_push;
pub mod devices;
pub mod events;
pub mod extension_stream;
pub mod extensions;
pub mod frontend_components;
pub mod images;
pub mod im_bridges;
pub mod instances;
pub mod llm_backends;
pub mod logs;
pub mod memory;
pub mod message_channels;
pub mod messages;
pub mod mqtt;
pub mod onboarding;
pub mod rules;
pub mod sessions;
pub mod settings;
pub mod setup;
pub mod skills;
pub mod stats;
pub mod suggestions;
pub mod summarization;
pub mod tools;
pub mod ws;

// Re-export ServerState so handlers can use it
pub use crate::server::ServerState;

// Re-export commonly used handler functions
pub use basic::health_handler;
pub use devices::{
    add_device_handler, aggregate_metric_handler, analyze_metric_timestamps_handler,
    delete_device_handler, delete_device_type_handler, generate_mdl_handler,
    get_device_command_history_handler, get_device_handler, get_device_telemetry_handler,
    get_device_telemetry_summary_handler, get_device_type_handler,
    import_cloud_device_types_handler, list_cloud_device_types_handler,
    list_device_metrics_debug_handler, list_device_types_handler, list_devices_handler,
    query_metric_handler, read_metric_handler, register_device_type_handler, send_command_handler,
    validate_device_type_handler,
};
pub use events::{event_stream_handler, event_websocket_handler};
pub use rules::{create_rule_handler, list_rules_handler};
pub use sessions::{
    chat_handler,
    cleanup_sessions_handler,
    clear_pending_stream_handler,
    create_session_handler,
    delete_session_handler,
    // P0.3: Pending stream state handlers
    get_pending_stream_handler,
    get_session_handler,
    get_session_history_handler,
    list_sessions_handler,
    update_session_handler,
    ws_chat_handler,
};
pub use settings::llm_generate_handler;
// Stats API
pub use stats::{get_device_stats_handler, get_rule_stats_handler, get_system_stats_handler};
// Extensions API
pub use extensions::{
    clear_extension_logs_handler,
    execute_extension_command_handler,
    extension_health_handler,
    get_extension_handler,
    get_extension_logs_handler,
    // Command-based extension handlers
    list_extension_commands_handler,
    list_extension_data_sources_handler,
    list_extension_types_handler,
    list_extensions_handler,
    register_extension_handler,
    start_extension_handler,
    stop_extension_handler,
    uninstall_extension_handler,
    unregister_extension_handler,
    upload_extension_file_handler,
};
// LLM Backends API
pub use llm_backends::{
    activate_backend_handler, create_backend_handler, delete_backend_handler, get_backend_handler,
    get_backend_schema_handler, get_backend_stats_handler, list_backend_types_handler,
    list_backends_handler, test_backend_handler, update_backend_handler,
};
// Instances API
pub use instances::{
    create_instance_handler, delete_instance_handler, get_instance_handler, list_instances_handler,
    test_instance_handler, update_instance_handler,
};
// User Authentication API
pub use auth_users::{
    change_password_handler, create_user_handler, delete_user_handler, get_current_user_handler,
    list_users_handler, login_handler, logout_handler, register_handler,
};
// Suggestions API
pub use suggestions::{get_suggestions_categories_handler, get_suggestions_handler};
// Messages API
pub use messages::{
    acknowledge_message_handler, archive_message_handler, bulk_acknowledge_handler,
    bulk_delete_handler, bulk_resolve_handler, cleanup_handler, create_message_handler,
    delete_message_handler, get_message_handler, list_messages_handler, message_stats_handler,
    resolve_message_handler,
};
// Message channels API
pub use message_channels::{
    add_recipient_handler, create_channel_handler, delete_channel_handler,
    get_channel_filter_handler, get_channel_handler, get_channel_stats_handler,
    get_channel_type_schema_handler, list_channel_types_handler, list_channels_handler,
    list_recipients_handler, remove_recipient_handler, test_channel_handler,
    toggle_enabled_handler, update_channel_filter_handler, update_channel_handler,
};
// Unified Data API
pub use data::{list_all_data_sources_handler, query_telemetry_handler};
// Tools API
pub use tools::{get_tool_handler, list_tools_handler};
// System Memory API
pub use memory::{
    delete_memory_file, export_all, export_memory, get_all_memory, get_category, get_config,
    get_memory_content, get_stats, trigger_compress, update_category, update_config,
    update_memory_content,
};
