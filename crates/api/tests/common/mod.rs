//! Common test utilities for API tests.

use std::sync::Arc;
use tokio::sync::broadcast;

use edge_ai_core::EventBus;
use edge_ai_core::plugin::UnifiedPluginRegistry;
use edge_ai_agent::SessionManager;
use edge_ai_rules::InMemoryValueProvider;
use edge_ai_commands::{CommandManager, CommandQueue, CommandStateStore};
use edge_ai_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use edge_ai_messages::MessageManager;

use edge_ai_api::auth::AuthState;
use edge_ai_api::auth_users::AuthUserState;
use edge_ai_api::cache::ResponseCache;
use edge_ai_api::rate_limit::{RateLimitConfig, RateLimiter};

/// Create a mock server state for testing.
///
/// Note: This creates a minimal server state for testing.
/// For integration tests that need the full server state, prefer using
/// `ServerState::new()` which initializes all components properly.
pub async fn create_test_server_state() -> edge_ai_api::handlers::ServerState {
    let started_at = chrono::Utc::now().timestamp();
    let value_provider = Arc::new(InMemoryValueProvider::new());
    let event_bus = Arc::new(EventBus::new());

    let session_manager = Arc::new(SessionManager::memory());
    let time_series_storage = Arc::new(TimeSeriesStorage::memory().unwrap());
    let rule_engine = Arc::new(edge_ai_rules::RuleEngine::new(value_provider));
    let message_manager = Arc::new(MessageManager::new());
    let device_update_tx = broadcast::channel(100).0;

    let command_queue = Arc::new(CommandQueue::new(1000));
    let command_state = Arc::new(CommandStateStore::new(10000));
    let command_manager = Arc::new(CommandManager::new(command_queue, command_state));

    let plugin_registry = Arc::new(UnifiedPluginRegistry::new("1.0.0"));
    let device_registry = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(
        device_registry.clone(),
        (*event_bus).clone(),
    ));

    edge_ai_api::handlers::ServerState {
        session_manager,
        time_series_storage,
        rule_engine,
        rule_store: None,
        message_manager,
        automation_store: None,
        intent_analyzer: None,
        transform_engine: None,
        #[cfg(feature = "embedded-broker")]
        embedded_broker: None,
        device_update_tx,
        event_bus: Some(event_bus),
        command_manager: Some(command_manager),
        auth_state: Arc::new(AuthState::new()),
        auth_user_state: Arc::new(AuthUserState::new()),
        response_cache: Arc::new(ResponseCache::with_default_ttl()),
        rate_limiter: Arc::new(RateLimiter::with_config(RateLimitConfig::default())),
        extension_registry: Arc::new(tokio::sync::RwLock::new(
            edge_ai_core::plugin::ExtensionRegistry::new(),
        )),
        device_registry,
        device_service,
        auto_onboard_manager: Arc::new(tokio::sync::RwLock::new(None)),
        rule_history_store: None,
        memory: Arc::new(tokio::sync::RwLock::new(
            edge_ai_memory::TieredMemory::with_default_config(),
        )),
        agent_store: edge_ai_storage::AgentStore::memory().unwrap(),
        agent_manager: Arc::new(tokio::sync::RwLock::new(None)),
        dashboard_store: edge_ai_storage::DashboardStore::memory().unwrap(),
        started_at,
        agent_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rule_engine_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rule_engine_event_service: Arc::new(tokio::sync::Mutex::new(None)),
    }
}
