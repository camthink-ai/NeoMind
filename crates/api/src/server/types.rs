//! Server state and types.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use edge_ai_agent::SessionManager;
use edge_ai_devices::{MqttDeviceManager, MultiBrokerManager, TimeSeriesStorage};
use edge_ai_rules::{InMemoryValueProvider, RuleEngine};
use edge_ai_alerts::AlertManager;
use edge_ai_workflow::WorkflowEngine;
use edge_ai_core::EventBus;
use edge_ai_storage::business::EventLogStore;
use edge_ai_commands::{CommandManager, CommandQueue, CommandStateStore};
use edge_ai_storage::decisions::DecisionStore;

use crate::rate_limit::{RateLimiter, RateLimitConfig};
use crate::auth::AuthState;
use crate::auth_users::AuthUserState;
use crate::config::LlmSettingsRequest;

#[cfg(feature = "embedded-broker")]
use edge_ai_devices::EmbeddedBroker;

/// Maximum request body size (10 MB)
pub const MAX_REQUEST_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Device status update for WebSocket broadcast.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceStatusUpdate {
    /// Update type
    pub update_type: String,
    /// Device ID
    pub device_id: String,
    /// Device status (online/offline/etc)
    pub status: Option<String>,
    /// Last seen timestamp
    pub last_seen: Option<i64>,
}

/// Server state shared across all handlers.
#[derive(Clone)]
pub struct ServerState {
    /// Session manager.
    pub session_manager: Arc<SessionManager>,
    /// MQTT device manager (reference to internal broker).
    pub mqtt_device_manager: Arc<MqttDeviceManager>,
    /// Multi-broker manager for managing multiple MQTT connections.
    pub multi_broker_manager: Arc<MultiBrokerManager>,
    /// Time series storage for device metrics.
    pub time_series_storage: Arc<TimeSeriesStorage>,
    /// Rule engine.
    pub rule_engine: Arc<RuleEngine>,
    /// Alert manager.
    pub alert_manager: Arc<AlertManager>,
    /// Workflow engine (initialized asynchronously).
    pub workflow_engine: Arc<tokio::sync::RwLock<Option<Arc<WorkflowEngine>>>>,
    /// Embedded broker (only used in embedded mode)
    #[cfg(feature = "embedded-broker")]
    pub embedded_broker: Option<Arc<EmbeddedBroker>>,
    /// Device status broadcast sender
    pub device_update_tx: broadcast::Sender<DeviceStatusUpdate>,
    /// Event bus for system-wide event distribution.
    pub event_bus: Option<Arc<EventBus>>,
    /// Event log store for historical events.
    pub event_log: Option<Arc<EventLogStore>>,
    /// Command manager for command history and retry.
    pub command_manager: Option<Arc<CommandManager>>,
    /// Decision store for LLM decisions.
    pub decision_store: Option<Arc<DecisionStore>>,
    /// Authentication state for API key validation.
    pub auth_state: Arc<AuthState>,
    /// User authentication state for JWT token validation.
    pub auth_user_state: Arc<AuthUserState>,
    /// Response cache for API endpoints.
    pub response_cache: Arc<crate::cache::ResponseCache>,
    /// Rate limiter for API request throttling.
    pub rate_limiter: Arc<RateLimiter>,
    /// Server start timestamp.
    pub started_at: i64,
}

impl ServerState {
    /// Create a new server state.
    pub fn new() -> Self {
        let started_at = chrono::Utc::now().timestamp();
        let value_provider = Arc::new(InMemoryValueProvider::new());

        // Use in-memory SessionManager to avoid database lock conflicts
        let session_manager = SessionManager::memory();

        // Load MQTT configuration (connects to embedded broker on localhost)
        let mqtt_config = edge_ai_devices::MqttManagerConfig::default();

        // Create multi-broker manager (internal broker will be added in init_device_storage)
        let multi_broker_manager = Arc::new(
            edge_ai_devices::MultiBrokerManager::new()
                .with_storage_dir("data")
        );

        // Create a temporary mqtt_device_manager for now (will be replaced in init_device_storage)
        let mqtt_device_manager = Arc::new(
            MqttDeviceManager::new("internal-mqtt", mqtt_config.clone())
                .with_storage_dir("data")
        );

        // Use the SAME time series storage path as MqttDeviceManager (data/telemetry.redb)
        // This ensures telemetry data written by the device manager is readable by the API
        let telemetry_path = std::path::Path::new("data").join("telemetry.redb");
        let time_series_storage = Arc::new(
            match TimeSeriesStorage::open(&telemetry_path) {
                Ok(storage) => {
                    tracing::info!("Time series storage initialized at {:?}", telemetry_path);
                    storage
                }
                Err(e) => {
                    tracing::warn!(category = "storage", error = %e, "Failed to open telemetry storage at {:?}, using in-memory", telemetry_path);
                    match TimeSeriesStorage::memory() {
                        Ok(storage) => storage,
                        Err(e) => {
                            tracing::error!(category = "storage", error = %e, "Failed to create in-memory time series storage");
                            std::process::exit(1);
                        }
                    }
                }
            }
        );

        // Create in-memory workflow engine
        let workflow_engine = std::sync::Arc::new(tokio::sync::RwLock::new(None));

        // Create device status broadcast channel
        let device_update_tx = broadcast::channel(100).0;

        // Create command manager
        let command_queue = Arc::new(CommandQueue::new(1000));
        let command_state = Arc::new(CommandStateStore::new(10000));
        let command_manager = Arc::new(CommandManager::new(command_queue, command_state));

        // Create decision store
        let decision_store: Option<Arc<DecisionStore>> = match DecisionStore::open("data/decisions.redb") {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open decision store, using in-memory");
                match DecisionStore::memory() {
                    Ok(store) => Some(store),
                    Err(_) => {
                        tracing::error!(category = "storage", "Failed to create in-memory decision store");
                        None
                    }
                }
            }
        };

        // Create event log store
        let event_log: Option<Arc<EventLogStore>> = match EventLogStore::open("data/events.redb") {
            Ok(store) => {
                tracing::info!(category = "storage", "Event log store initialized: data/events.redb");
                Some(Arc::new(store))
            },
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open event log store, using in-memory");
                match EventLogStore::open(":memory:") {
                    Ok(store) => {
                        tracing::info!(category = "storage", "Event log store using in-memory storage");
                        Some(Arc::new(store))
                    },
                    Err(e) => {
                        tracing::error!(category = "storage", error = %e, "Failed to create in-memory event log");
                        None
                    }
                }
            }
        };

        // Load rate limit configuration
        let rate_limit_config = RateLimitConfig::default();
        let rate_limiter = Arc::new(RateLimiter::with_config(rate_limit_config));

        Self {
            session_manager: Arc::new(session_manager),
            mqtt_device_manager,
            multi_broker_manager,
            time_series_storage,
            rule_engine: Arc::new(RuleEngine::new(value_provider)),
            alert_manager: Arc::new(AlertManager::new()),
            workflow_engine,
            #[cfg(feature = "embedded-broker")]
            embedded_broker: None,
            device_update_tx,
            event_bus: Some(Arc::new(EventBus::new())),
            event_log,
            command_manager: Some(command_manager),
            decision_store,
            auth_state: Arc::new(AuthState::new()),
            auth_user_state: Arc::new(AuthUserState::new()),
            response_cache: Arc::new(crate::cache::ResponseCache::with_default_ttl()),
            rate_limiter,
            started_at,
        }
    }

    /// Initialize device type storage.
    pub async fn init_device_storage(&self) {
        if let Err(e) = tokio::fs::create_dir_all("data").await {
            tracing::error!(category = "storage", error = %e, "Failed to create data directory");
        }

        // Initialize the mqtt_device_manager
        if let Err(e) = self.mqtt_device_manager.initialize().await {
            tracing::warn!(category = "storage", error = %e, "Failed to initialize device storage, using in-memory");
        } else {
            tracing::info!(category = "storage", "Device type storage initialized: data/devices.redb");
        }

        // Add internal broker to multi-broker manager
        let mqtt_config = edge_ai_devices::MqttManagerConfig::default();
        if let Err(e) = self.multi_broker_manager.add_broker("internal-mqtt", mqtt_config).await {
            tracing::warn!(category = "storage", error = %e, "Failed to add internal broker to multi-broker manager");
        } else {
            tracing::info!(category = "storage", "Internal MQTT broker added to multi-broker manager");
        }
    }

    /// Initialize workflow engine with persistent storage.
    pub async fn init_workflow_engine(&self) {
        use edge_ai_workflow::WorkflowEngine;

        let engine = match WorkflowEngine::new("data/workflows").await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(category = "workflow", error = %e, "Failed to create WorkflowEngine with storage, using in-memory");
                match WorkflowEngine::new("/tmp/workflows_neotalk").await {
                    Ok(e) => e,
                    Err(_) => {
                        tracing::warn!(category = "workflow", "Failed to create in-memory workflow engine, using empty path");
                        WorkflowEngine::new("").await.expect("Failed to create workflow engine")
                    }
                }
            }
        };

        *self.workflow_engine.write().await = Some(Arc::new(engine));
    }

    /// Initialize LLM backend using the unified config loader.
    pub async fn init_llm(&self) {
        if let Some(backend) = crate::config::load_llm_config() {
            match self.session_manager.set_llm_backend(backend).await {
                Ok(_) => tracing::info!(category = "ai", "Configured LLM backend successfully"),
                Err(e) => tracing::error!(category = "ai", error = %e, "Failed to configure LLM backend"),
            }
        } else {
            tracing::warn!(category = "ai", "No LLM backend configured. Create config.toml or set environment variables");
        }
    }

    /// Initialize MQTT device manager.
    pub async fn init_mqtt(&self) {
        #[cfg(feature = "embedded-broker")]
        {
            use crate::config::get_embedded_broker_config;

            let config = get_embedded_broker_config();
            let port = config.port;
            let broker = EmbeddedBroker::new(config);
            match broker.start() {
                Ok(_) => {
                    tracing::info!("Embedded MQTT broker started on :{}", port);
                }
                Err(e) => {
                    tracing::error!("Failed to start embedded broker: {}", e);
                    tracing::warn!("Device management may not work properly");
                }
            }
        }

        match self.mqtt_device_manager.connect().await {
            Ok(_) => {
                tracing::info!("MQTT device manager connected successfully");
            }
            Err(e) => {
                tracing::warn!("MQTT broker not available, device management will be disabled: {}", e);
            }
        }
    }

    /// Initialize event log storage (no-op, kept for compatibility).
    pub async fn init_event_log(&self) {
        if self.event_log.is_some() {
            tracing::debug!("Event log already initialized during construction");
        } else {
            tracing::warn!("Event log not available - may have failed to initialize");
        }
    }

    /// Save LLM configuration to database.
    pub async fn save_llm_config(&self, request: &LlmSettingsRequest) -> std::io::Result<()> {
        let settings = request.to_llm_settings();
        crate::config::save_llm_settings(&settings).await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}
