//! Server state and types.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use edge_ai_agent::SessionManager;
use edge_ai_alerts::AlertManager;
use edge_ai_commands::{CommandManager, CommandQueue, CommandStateStore};
use edge_ai_core::{EventBus, plugin::UnifiedPluginRegistry};
use edge_ai_devices::adapter::AdapterResult;
use edge_ai_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use edge_ai_rules::{InMemoryValueProvider, RuleEngine};
use edge_ai_storage::business::EventLogStore;
use edge_ai_storage::decisions::DecisionStore;
use edge_ai_workflow::WorkflowEngine;

use crate::auth::AuthState;
use crate::auth_users::AuthUserState;
use crate::config::LlmSettingsRequest;
use crate::rate_limit::{RateLimitConfig, RateLimiter};

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
    /// Plugin registry for managing all plugins.
    pub plugin_registry: Arc<UnifiedPluginRegistry>,
    /// Device registry for templates and configurations (new architecture)
    pub device_registry: Arc<DeviceRegistry>,
    /// Device service for unified device operations (new architecture)
    pub device_service: Arc<DeviceService>,
    /// Server start timestamp.
    pub started_at: i64,
}

impl ServerState {
    /// Create a new server state.
    /// This is now async to support persistent device registry initialization.
    pub async fn new() -> Self {
        let started_at = chrono::Utc::now().timestamp();
        let value_provider = Arc::new(InMemoryValueProvider::new());

        // Use persistent SessionManager for session recovery after restart
        // Sessions are stored in data/sessions.redb and restored on startup
        let session_manager = SessionManager::new().unwrap_or_else(|e| {
            tracing::warn!(category = "storage", error = %e, "Failed to create persistent SessionManager, using in-memory");
            SessionManager::memory()
        });

        // Create event bus FIRST (needed for adapters to publish events)
        let event_bus = Arc::new(EventBus::new());

        // Ensure data directory exists
        if let Err(e) = std::fs::create_dir_all("data") {
            tracing::warn!(category = "storage", error = %e, "Failed to create data directory");
        }

        // Create device registry with persistent storage
        // Device types and configurations are stored in data/devices.redb
        let device_registry = match DeviceRegistry::with_persistence("data/devices.redb").await {
            Ok(registry) => {
                tracing::info!("Device registry initialized with persistent storage at data/devices.redb");
                Arc::new(registry)
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open persistent device registry, using in-memory");
                Arc::new(DeviceRegistry::new())
            }
        };

        // Use the SAME time series storage path (data/telemetry.redb)
        // This ensures telemetry data written by adapters is readable by the API
        let telemetry_path = std::path::Path::new("data").join("telemetry.redb");
        let time_series_storage = Arc::new(match TimeSeriesStorage::open(&telemetry_path) {
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
        });

        // Create in-memory workflow engine
        let workflow_engine = std::sync::Arc::new(tokio::sync::RwLock::new(None));

        // Create device status broadcast channel
        let device_update_tx = broadcast::channel(100).0;

        // Create command manager
        let command_queue = Arc::new(CommandQueue::new(1000));
        let command_state = Arc::new(CommandStateStore::new(10000));
        let command_manager = Arc::new(CommandManager::new(command_queue, command_state));

        // Create decision store
        let decision_store: Option<Arc<DecisionStore>> = match DecisionStore::open(
            "data/decisions.redb",
        ) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open decision store, using in-memory");
                match DecisionStore::memory() {
                    Ok(store) => Some(store),
                    Err(_) => {
                        tracing::error!(
                            category = "storage",
                            "Failed to create in-memory decision store"
                        );
                        None
                    }
                }
            }
        };

        // Create event log store
        let event_log: Option<Arc<EventLogStore>> = match EventLogStore::open("data/events.redb") {
            Ok(store) => {
                tracing::info!(
                    category = "storage",
                    "Event log store initialized: data/events.redb"
                );
                Some(Arc::new(store))
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open event log store, using in-memory");
                match EventLogStore::open(":memory:") {
                    Ok(store) => {
                        tracing::info!(
                            category = "storage",
                            "Event log store using in-memory storage"
                        );
                        Some(Arc::new(store))
                    }
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

        // Create plugin registry with NeoTalk version
        let plugin_registry = Arc::new(UnifiedPluginRegistry::new(env!("CARGO_PKG_VERSION")));

        // Create device service (new architecture)
        let device_service = Arc::new(DeviceService::new(
            device_registry.clone(),
            (*event_bus).clone(),
        ));

        // Set telemetry storage for device service (synchronously)
        device_service
            .set_telemetry_storage(time_series_storage.clone())
            .await;

        Self {
            session_manager: Arc::new(session_manager),
            time_series_storage,
            rule_engine: Arc::new(RuleEngine::new(value_provider)),
            alert_manager: Arc::new(AlertManager::new()),
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
            response_cache: Arc::new(crate::cache::ResponseCache::with_default_ttl()),
            rate_limiter,
            plugin_registry,
            device_registry,
            device_service,
            started_at,
        }
    }

    /// Initialize device type storage.
    pub async fn init_device_storage(&self) {
        if let Err(e) = tokio::fs::create_dir_all("data").await {
            tracing::error!(category = "storage", error = %e, "Failed to create data directory");
        }

        // Device registry storage is initialized automatically on first use
        tracing::info!(category = "storage", "Data directory created/verified");
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
                        tracing::warn!(
                            category = "workflow",
                            "Failed to create in-memory workflow engine, using empty path"
                        );
                        WorkflowEngine::new("")
                            .await
                            .expect("Failed to create workflow engine")
                    }
                }
            }
        };

        *self.workflow_engine.write().await = Some(Arc::new(engine));
    }

    /// Initialize LLM backend using the unified config loader.
    /// Falls back to LlmBackendInstanceManager if no config file is found.
    pub async fn init_llm(&self) {
        // First try to load from config file
        if let Some(backend) = crate::config::load_llm_config() {
            match self.session_manager.set_llm_backend(backend).await {
                Ok(_) => {
                    tracing::info!(
                        category = "ai",
                        "Configured LLM backend successfully from config file"
                    );
                    return;
                }
                Err(e) => {
                    tracing::error!(category = "ai", error = %e, "Failed to configure LLM backend from config file")
                }
            }
        }

        // Fallback: try to load from LlmBackendInstanceManager (database-stored backends)
        match self
            .session_manager
            .configure_llm_from_instance_manager()
            .await
        {
            Ok(_) => {
                tracing::info!(
                    category = "ai",
                    "Configured LLM backend successfully from instance manager"
                );
            }
            Err(e) => {
                tracing::warn!(category = "ai", error = %e, "No LLM backend configured. Set up via Web UI or create config.toml");
            }
        }
    }

    /// Initialize MQTT adapter for device communication.
    /// Creates and starts a real MQTT client that connects to the embedded broker.
    pub async fn init_mqtt(&self) {
        use edge_ai_devices::adapter::DeviceAdapter;
        use edge_ai_devices::adapters::{create_adapter, mqtt::MqttAdapterConfig};

        // Start device service to listen for EventBus events
        self.device_service.start().await;

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

        // Create and register the internal MQTT adapter
        let mqtt_config = MqttAdapterConfig {
            name: "internal-mqtt".to_string(),
            mqtt: edge_ai_devices::mqtt::MqttConfig {
                broker: "localhost".to_string(),
                port: 1883,
                client_id: Some("neotalk-internal".to_string()),
                username: None,
                password: None,
                keep_alive: 60,
                clean_session: true,
                qos: 1,
                topic_prefix: "device".to_string(),
                command_topic: "downlink".to_string(),
            },
            subscribe_topics: vec!["device/+/+/uplink".to_string()],
            discovery_topic: Some("device/+/+/uplink".to_string()),
            discovery_prefix: "device".to_string(),
            auto_discovery: true,
            storage_dir: Some("data".to_string()),
        };

        // Create the MQTT adapter
        let mqtt_adapter_result: AdapterResult<Arc<dyn DeviceAdapter>> = {
            create_adapter(
                "mqtt",
                &serde_json::to_value(mqtt_config).unwrap(),
                self.event_bus.as_ref().unwrap(),
            )
        };

        match mqtt_adapter_result {
            Ok(mqtt_adapter) => {
                // Register adapter with device service
                self.device_service
                    .register_adapter("internal-mqtt".to_string(), mqtt_adapter.clone())
                    .await;

                // Start the adapter
                if let Err(e) = mqtt_adapter.start().await {
                    tracing::warn!("Failed to start MQTT adapter: {}", e);
                } else {
                    tracing::info!("MQTT adapter started successfully");
                }
            }
            Err(e) => {
                tracing::error!("Failed to create MQTT adapter: {}", e);
            }
        }

        // Initialize the global device adapter plugin registry
        if let Some(event_bus) = &self.event_bus {
            use edge_ai_devices::DeviceAdapterPluginRegistry;
            let registry = DeviceAdapterPluginRegistry::get_or_init((**event_bus).clone());
            // Connect DeviceService with plugin registry for automatic adapter registration
            registry
                .set_device_service(self.device_service.clone())
                .await;
            tracing::info!(
                "Device adapter plugin registry initialized and connected to DeviceService"
            );
        }
    }

    /// Initialize tool registry with real service connections.
    pub async fn init_tools(&self) {
        use edge_ai_tools::{ToolRegistryBuilder, real};
        use std::sync::Arc;

        // Check if workflow engine is initialized first
        let workflow_engine_read = self.workflow_engine.read().await;
        let _has_workflow = workflow_engine_read.as_ref().is_some();
        let workflow_engine_clone = workflow_engine_read.as_ref().cloned();
        drop(workflow_engine_read);

        // Build tool registry with real implementations
        let mut builder = ToolRegistryBuilder::new()
            // Query time series data (requires metric name)
            .with_real_query_data_tool(self.time_series_storage.clone())
            // Get device data (simplified - returns all metrics)
            .with_real_get_device_data_tool(self.device_service.clone(), self.time_series_storage.clone())
            // Control devices via DeviceService
            .with_real_control_device_tool(self.device_service.clone())
            // List devices
            .with_real_list_devices_tool(self.device_service.clone())
            // Create rules
            .with_real_create_rule_tool(self.rule_engine.clone())
            // List rules
            .with_real_list_rules_tool(self.rule_engine.clone());

        // Add trigger workflow tool if workflow engine is initialized
        if let Some(engine) = workflow_engine_clone {
            builder = builder.with_tool(Arc::new(real::TriggerWorkflowTool::new(engine)));
            let tool_registry = Arc::new(builder.build());
            self.session_manager
                .set_tool_registry(tool_registry.clone())
                .await;
            tracing::info!(
                category = "ai",
                "Tool registry initialized with {} tools (including workflow)",
                tool_registry.len()
            );
        } else {
            let tool_registry = Arc::new(builder.build());
            self.session_manager
                .set_tool_registry(tool_registry.clone())
                .await;
            tracing::info!(
                category = "ai",
                "Tool registry initialized with {} tools (workflow engine not available)",
                tool_registry.len()
            );
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
        crate::config::save_llm_settings(&settings)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))
    }
}

// Note: Default implementation removed because ServerState::new() is now async
// to support persistent device registry initialization.
