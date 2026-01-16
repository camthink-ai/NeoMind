//! Common test utilities for API tests.

use std::sync::Arc;
use tokio::sync::broadcast;

use edge_ai_core::EventBus;
use edge_ai_core::plugin::UnifiedPluginRegistry;
use edge_ai_agent::SessionManager;
use edge_ai_rules::InMemoryValueProvider;
use edge_ai_commands::{CommandManager, CommandQueue, CommandStateStore};
use edge_ai_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use edge_ai_storage::business::EventLogStore;
use edge_ai_storage::decisions::DecisionStore;
use edge_ai_alerts::AlertManager;

use edge_ai_api::auth::AuthState;
use edge_ai_api::auth_users::AuthUserState;
use edge_ai_api::cache::ResponseCache;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::rate_limit::{RateLimitConfig, RateLimiter};

/// Create a mock server state for testing.
pub async fn create_test_server_state() -> ServerState {
    let started_at = chrono::Utc::now().timestamp();
    let value_provider = Arc::new(InMemoryValueProvider::new());
    let event_bus = Arc::new(EventBus::new());

    let session_manager = Arc::new(SessionManager::memory());
    let time_series_storage = Arc::new(TimeSeriesStorage::memory().unwrap());
    let rule_engine = Arc::new(edge_ai_rules::RuleEngine::new(value_provider));
    let alert_manager = Arc::new(AlertManager::new());
    let workflow_engine = Arc::new(tokio::sync::RwLock::new(None));
    let device_update_tx = broadcast::channel(100).0;

    let command_queue = Arc::new(CommandQueue::new(1000));
    let command_state = Arc::new(CommandStateStore::new(10000));
    let command_manager = Arc::new(CommandManager::new(command_queue, command_state));

    let decision_store = DecisionStore::memory().ok();
    let event_log = EventLogStore::open(":memory:").ok().map(Arc::new);

    let plugin_registry = Arc::new(UnifiedPluginRegistry::new("1.0.0"));
    let device_registry = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(
        device_registry.clone(),
        (*event_bus).clone(),
    ));

    ServerState {
        session_manager,
        time_series_storage,
        rule_engine,
        alert_manager,
        workflow_engine,
        #[cfg(feature = "embedded-broker")]
        embedded_broker: None,
        device_update_tx,
        event_bus: Some(event_bus),
        event_log,
        command_manager: Some(command_manager),
        decision_store,
        auth_state: Arc::new(AuthState::new()),
        auth_user_state: Arc::new(AuthUserState::new()),
        response_cache: Arc::new(ResponseCache::with_default_ttl()),
        rate_limiter: Arc::new(RateLimiter::with_config(RateLimitConfig::default())),
        plugin_registry,
        device_registry,
        device_service,
        started_at,
    }
}

/// Create a test server state with workflow engine initialized.
pub async fn create_test_server_state_with_workflow() -> ServerState {
    use std::path::PathBuf;
    use edge_ai_workflow::WorkflowEngine;

    let started_at = chrono::Utc::now().timestamp();
    let value_provider = Arc::new(InMemoryValueProvider::new());
    let event_bus = Arc::new(EventBus::new());

    let session_manager = Arc::new(SessionManager::memory());
    let time_series_storage = Arc::new(TimeSeriesStorage::memory().unwrap());
    let rule_engine = Arc::new(edge_ai_rules::RuleEngine::new(value_provider));
    let alert_manager = Arc::new(AlertManager::new());
    let device_update_tx = broadcast::channel(100).0;

    let temp_dir = PathBuf::from(format!("/tmp/neotalk_workflow_test_{}_{}", std::process::id(), uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);
    let workflow_engine = Arc::new(WorkflowEngine::new(&temp_dir).await.unwrap());

    let command_queue = Arc::new(CommandQueue::new(1000));
    let command_state = Arc::new(CommandStateStore::new(10000));
    let command_manager = Arc::new(CommandManager::new(command_queue, command_state));

    let decision_store = DecisionStore::memory().ok();
    let event_log = EventLogStore::open(":memory:").ok().map(Arc::new);
    let plugin_registry = Arc::new(UnifiedPluginRegistry::new("1.0.0"));
    let device_registry = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(
        device_registry.clone(),
        (*event_bus).clone(),
    ));

    ServerState {
        session_manager,
        time_series_storage,
        rule_engine,
        alert_manager,
        workflow_engine: Arc::new(tokio::sync::RwLock::new(Some(workflow_engine))),
        #[cfg(feature = "embedded-broker")]
        embedded_broker: None,
        device_update_tx,
        event_bus: Some(event_bus),
        event_log,
        command_manager: Some(command_manager),
        decision_store,
        auth_state: Arc::new(AuthState::new()),
        auth_user_state: Arc::new(AuthUserState::new()),
        response_cache: Arc::new(ResponseCache::with_default_ttl()),
        rate_limiter: Arc::new(RateLimiter::with_config(RateLimitConfig::default())),
        plugin_registry,
        device_registry,
        device_service,
        started_at,
    }
}
