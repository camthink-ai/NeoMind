//! Server state and types.

use std::pin::Pin;
use std::sync::Arc;
use futures::Stream;
use tokio::sync::broadcast;

use edge_ai_agent::SessionManager;
use edge_ai_alerts::AlertManager;
use edge_ai_commands::{CommandManager, CommandQueue, CommandStateStore};
use edge_ai_core::{EventBus, extension::ExtensionRegistry};
use edge_ai_devices::adapter::AdapterResult;
use edge_ai_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use edge_ai_rules::{InMemoryValueProvider, RuleEngine, store::RuleStore};
use edge_ai_storage::dashboards::DashboardStore;
use edge_ai_storage::decisions::DecisionStore;
use edge_ai_storage::llm_backends::LlmBackendStore;

use edge_ai_automation::{AutoOnboardManager, store::SharedAutomationStore, intent::IntentAnalyzer, transform::TransformEngine};
use edge_ai_memory::TieredMemory;

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

/// AI Agent manager for user-defined automation agents.
pub type AgentManager = Arc<edge_ai_agent::ai_agent::AiAgentManager>;

/// Server state shared across all handlers.
#[derive(Clone)]
pub struct ServerState {
    /// Session manager.
    pub session_manager: Arc<SessionManager>,
    /// Time series storage for device metrics.
    pub time_series_storage: Arc<TimeSeriesStorage>,
    /// Rule engine.
    pub rule_engine: Arc<RuleEngine>,
    /// Rule store for persistent rule storage.
    pub rule_store: Option<Arc<RuleStore>>,
    /// Alert manager.
    pub alert_manager: Arc<AlertManager>,
    /// Automation store for unified automations.
    pub automation_store: Option<Arc<SharedAutomationStore>>,
    /// Intent analyzer for automation type recommendations.
    pub intent_analyzer: Option<Arc<IntentAnalyzer>>,
    /// Transform engine for data processing.
    pub transform_engine: Option<Arc<TransformEngine>>,
    /// Embedded broker (only used in embedded mode)
    #[cfg(feature = "embedded-broker")]
    pub embedded_broker: Option<Arc<EmbeddedBroker>>,
    /// Device status broadcast sender
    pub device_update_tx: broadcast::Sender<DeviceStatusUpdate>,
    /// Event bus for system-wide event distribution.
    pub event_bus: Option<Arc<EventBus>>,
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
    /// Extension registry for managing dynamically loaded extensions (.so/.wasm).
    pub extension_registry: Arc<tokio::sync::RwLock<ExtensionRegistry>>,
    /// Device registry for templates and configurations (new architecture)
    pub device_registry: Arc<DeviceRegistry>,
    /// Device service for unified device operations (new architecture)
    pub device_service: Arc<DeviceService>,
    /// Auto-onboarding manager for zero-config device discovery (lazy-initialized)
    pub auto_onboard_manager: Arc<tokio::sync::RwLock<Option<Arc<AutoOnboardManager>>>>,
    /// Rule history store for statistics.
    pub rule_history_store: Option<Arc<edge_ai_storage::business::RuleHistoryStore>>,
    /// Alert store for statistics.
    pub alert_store: Option<Arc<edge_ai_storage::business::AlertStore>>,
    /// Tiered memory system for conversation history and knowledge.
    pub memory: Arc<tokio::sync::RwLock<TieredMemory>>,
    /// AI Agent store for user-defined automation agents.
    pub agent_store: Arc<edge_ai_storage::AgentStore>,
    /// AI Agent manager for executing user-defined agents (lazy-initialized).
    pub agent_manager: Arc<tokio::sync::RwLock<Option<AgentManager>>>,
    /// Server start timestamp.
    pub started_at: i64,
    /// Dashboard store for visual dashboard persistence.
    pub dashboard_store: Arc<DashboardStore>,
    /// Flag to track if agent events have been initialized (prevents duplicate subscribers).
    agent_events_initialized: Arc<std::sync::atomic::AtomicBool>,
    /// Flag to track if rule engine events have been initialized (prevents duplicate subscribers).
    rule_engine_events_initialized: Arc<std::sync::atomic::AtomicBool>,
    /// Cached rule engine event service instance (prevents duplicate instances).
    rule_engine_event_service: Arc<tokio::sync::Mutex<Option<crate::event_services::RuleEngineEventService>>>,
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

        // Create device status broadcast channel
        let _device_update_tx: broadcast::Sender<DeviceStatusUpdate> = broadcast::channel(100).0;

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


        // Create device status broadcast channel
        let device_update_tx: broadcast::Sender<DeviceStatusUpdate> = broadcast::channel(100).0;

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

        // Load rate limit configuration
        let rate_limit_config = RateLimitConfig::default();
        let rate_limiter = Arc::new(RateLimiter::with_config(rate_limit_config));

        // Create extension registry for dynamically loaded extensions (.so/.wasm)
        let extension_registry = Arc::new(tokio::sync::RwLock::new(
            ExtensionRegistry::new(),
        ));

        // Create device service (new architecture)
        let device_service = Arc::new(DeviceService::new(
            device_registry.clone(),
            (*event_bus).clone(),
        ));

        // Set telemetry storage for device service (synchronously)
        device_service
            .set_telemetry_storage(time_series_storage.clone())
            .await;

        // Create automation store
        let automation_store: Option<Arc<SharedAutomationStore>> = match SharedAutomationStore::open("data/automations.redb").await {
            Ok(store) => {
                tracing::info!("Automation store initialized at data/automations.redb");
                Some(Arc::new(store))
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open automation store, using in-memory");
                match SharedAutomationStore::memory() {
                    Ok(store) => {
                        tracing::info!("Automation store using in-memory storage");
                        Some(Arc::new(store))
                    }
                    Err(e) => {
                        tracing::error!(category = "storage", error = %e, "Failed to create in-memory automation store");
                        None
                    }
                }
            }
        };

        // Create intent analyzer (will use LLM backend when available)
        let intent_analyzer: Option<Arc<IntentAnalyzer>> = None; // TODO: Initialize with LLM backend

        // Create transform engine for data processing
        let transform_engine: Option<Arc<TransformEngine>> = Some(Arc::new(TransformEngine::new()));
        tracing::info!("Transform engine initialized");

        // Create auto-onboarding manager for zero-config device discovery
        // Note: Will be lazy-initialized when first accessed
        let auto_onboard_manager: Arc<tokio::sync::RwLock<Option<Arc<AutoOnboardManager>>>> =
            Arc::new(tokio::sync::RwLock::new(None));

        // Create AI Agent store for user-defined automation agents
        let agent_store = match edge_ai_storage::AgentStore::open("data/agents.redb") {
            Ok(store) => {
                tracing::info!("AI Agent store initialized at data/agents.redb");
                store
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open agent store, using in-memory");
                edge_ai_storage::AgentStore::memory()
                    .unwrap_or_else(|e| {
                        tracing::error!(category = "storage", error = %e, "Failed to create in-memory agent store");
                        std::process::exit(1);
                    })
            }
        };

        // Create alert manager first (needed by rule engine)
        let alert_manager = Arc::new(AlertManager::new());
        let rule_engine = Arc::new(RuleEngine::new(value_provider));
        // Wire rule engine to alert manager for CreateAlert actions
        rule_engine.set_alert_manager(alert_manager.clone()).await;
        // Wire event bus to alert manager for AlertCreated events
        alert_manager.set_event_bus(event_bus.clone()).await;

        // Create rule store for persistent rule storage
        let rule_store = match RuleStore::open("data/rules.redb") {
            Ok(store) => {
                tracing::info!("Rule store initialized at data/rules.redb");
                Some(store)
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open rule store, rules will not be persisted");
                None
            }
        };

        // Load rules from store into rule engine
        if let Some(ref store) = rule_store {
            match store.list_all() {
                Ok(rules) => {
                    let rule_count = rules.len();
                    tracing::info!("Loading {} rules from persistent store", rule_count);
                    for rule in rules {
                        // Re-add rule to engine
                        if let Err(e) = rule_engine.add_rule(rule.clone()).await {
                            tracing::warn!("Failed to load rule {}: {}", rule.id, e);
                        } else {
                            tracing::debug!("Loaded rule: {} ({})", rule.name, rule.id);
                        }
                    }
                    tracing::info!("Successfully loaded {} rules from persistent store", rule_count);
                }
                Err(e) => {
                    tracing::warn!(category = "storage", error = %e, "Failed to load rules from store");
                }
            }
        }

        Self {
            session_manager: Arc::new(session_manager),
            time_series_storage,
            rule_engine,
            rule_store,
            alert_manager,
            automation_store,
            intent_analyzer,
            transform_engine,
            #[cfg(feature = "embedded-broker")]
            embedded_broker: None,
            device_update_tx,
            event_bus: Some(event_bus),
            command_manager: Some(command_manager),
            decision_store,
            auth_state: Arc::new(AuthState::new()),
            auth_user_state: Arc::new(AuthUserState::new()),
            response_cache: Arc::new(crate::cache::ResponseCache::with_default_ttl()),
            rate_limiter,
            extension_registry,
            device_registry,
            device_service,
            auto_onboard_manager,
            rule_history_store: {
                use edge_ai_storage::business::RuleHistoryStore;
                match RuleHistoryStore::open("data/rule_history.redb") {
                    Ok(store) => {
                        tracing::info!("Rule history store initialized at data/rule_history.redb");
                        Some(Arc::new(store))
                    }
                    Err(e) => {
                        tracing::warn!(category = "storage", error = %e, "Failed to open rule history store, statistics will be limited");
                        None
                    }
                }
            },
            alert_store: {
                use edge_ai_storage::business::AlertStore;
                match AlertStore::open("data/alerts.redb") {
                    Ok(store) => {
                        tracing::info!("Alert store initialized at data/alerts.redb");
                        Some(Arc::new(store))
                    }
                    Err(e) => {
                        tracing::warn!(category = "storage", error = %e, "Failed to open alert store, statistics will be limited");
                        None
                    }
                }
            },
            // Initialize tiered memory with configuration from config.toml
            memory: {
                let memory_config = crate::config::get_memory_config();
                Arc::new(tokio::sync::RwLock::new(TieredMemory::with_config(memory_config)))
            },
            agent_store,
            // AI Agent manager - lazy initialized, will be started when server is fully ready
            agent_manager: Arc::new(tokio::sync::RwLock::new(None)),
            // Dashboard store for visual dashboards
            dashboard_store: {
                match DashboardStore::open("data/dashboards.redb") {
                    Ok(store) => {
                        tracing::info!("Dashboard store initialized at data/dashboards.redb");
                        store
                    }
                    Err(e) => {
                        tracing::warn!(category = "storage", error = %e, "Failed to open dashboard store, using in-memory");
                        DashboardStore::memory().unwrap_or_else(|e| {
                            tracing::error!(category = "storage", error = %e, "Failed to create in-memory dashboard store");
                            std::process::exit(1);
                        })
                    }
                }
            },
            started_at,
            // Initialization flags to prevent duplicate event subscribers
            agent_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_event_service: Arc::new(tokio::sync::Mutex::new(None)),
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
            subscribe_topics: vec!["#".to_string()],  // Subscribe to ALL topics for auto-discovery
            discovery_topic: Some("device/+/+/uplink".to_string()),
            discovery_prefix: "device".to_string(),
            auto_discovery: true,
            storage_dir: Some("data".to_string()),
        };

        // Create the MQTT adapter
        let Some(event_bus) = self.event_bus.as_ref() else {
            tracing::error!("EventBus not initialized, cannot create MQTT adapter");
            return;
        };

        let mqtt_config_value = match serde_json::to_value(mqtt_config) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Failed to serialize MQTT config: {}", e);
                return;
            }
        };

        let mqtt_adapter_result: AdapterResult<Arc<dyn DeviceAdapter>> = {
            create_adapter("mqtt", &mqtt_config_value, event_bus)
        };

        match mqtt_adapter_result {
            Ok(mqtt_adapter) => {
                // Set telemetry storage for the MQTT adapter so it can write metrics
                mqtt_adapter.set_telemetry_storage(self.time_series_storage.clone());

                // Try to set the shared device registry on the MQTT adapter
                // This allows the adapter to look up devices by custom telemetry topics
                if let Some(mqtt) = mqtt_adapter.as_any().downcast_ref::<edge_ai_devices::adapters::mqtt::MqttAdapter>() {
                    mqtt.set_shared_device_registry(self.device_service.get_registry().await).await;
                }

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

        tracing::info!("Device adapters managed directly via DeviceService");

        // Load and reconnect external MQTT brokers
        self.reconnect_external_mqtt_brokers().await;
    }

    /// Reconnect to all enabled external MQTT brokers on startup
    async fn reconnect_external_mqtt_brokers(&self) {
        use crate::handlers::mqtt::brokers::{create_and_connect_broker, ExternalBrokerContext};

        let store = match crate::config::open_settings_store() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to open settings store for external broker reconnection: {}", e);
                return;
            }
        };

        let brokers = match store.load_all_external_brokers() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("Failed to load external brokers: {}", e);
                return;
            }
        };

        if brokers.is_empty() {
            tracing::info!("No external MQTT brokers configured");
            return;
        }

        tracing::info!("Found {} external MQTT broker(s), attempting to reconnect...", brokers.len());

        let Some(event_bus) = self.event_bus.as_ref() else {
            tracing::warn!("EventBus not initialized, cannot reconnect external brokers");
            return;
        };

        let context = ExternalBrokerContext {
            device_service: self.device_service.clone(),
            event_bus: event_bus.clone(),
        };

        for broker in brokers {
            // Skip disabled brokers
            if !broker.enabled {
                tracing::info!("Skipping disabled external broker: {}", broker.id);
                continue;
            }

            tracing::info!("Reconnecting to external broker: {} ({})", broker.id, broker.name);

            // Use the broker connection logic
            match create_and_connect_broker(&broker, &context).await {
                Ok(connected) => {
                    if connected {
                        tracing::info!("Successfully reconnected to external broker: {}", broker.id);
                    } else {
                        tracing::warn!("External broker reconnection attempted but failed: {}", broker.id);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to reconnect external broker {}: {}", broker.id, e);
                }
            }
        }
    }

    /// Initialize tool registry with real service connections.
    pub async fn init_tools(&self) {
        use edge_ai_tools::ToolRegistryBuilder;
        use std::sync::Arc;

        // Build tool registry with real implementations that connect to actual services
        let builder = ToolRegistryBuilder::new()
            // Real implementations
            .with_real_query_data_tool(self.time_series_storage.clone())
            .with_real_get_device_data_tool(self.device_service.clone(), self.time_series_storage.clone())
            .with_real_control_device_tool(self.device_service.clone())
            .with_real_list_devices_tool(self.device_service.clone())
            .with_real_device_analyze_tool(self.device_service.clone(), self.time_series_storage.clone())
            .with_real_create_rule_tool(self.rule_engine.clone())
            .with_real_list_rules_tool(self.rule_engine.clone())
            .with_real_delete_rule_tool(self.rule_engine.clone())
            // AI Agent tools for Chat integration
            .with_real_agent_tools(self.agent_store.clone())
            // System help tool for onboarding
            .with_system_help_tool_named("NeoTalk");

        let tool_registry = Arc::new(builder.build());
        self.session_manager
            .set_tool_registry(tool_registry.clone())
            .await;
        tracing::info!(
            category = "ai",
            "Tool registry initialized with {} tools",
            tool_registry.len()
        );
    }

    /// Initialize rule engine event service.
    ///
    /// Starts a background task that subscribes to device metric events
    /// and automatically evaluates rules when relevant data is received.
    pub async fn init_rule_engine_events(&self) {
        // Prevent duplicate initialization - use compare_exchange for atomic check-and-set
        if self.rule_engine_events_initialized.compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst
        ).is_err() {
            tracing::debug!("Rule engine event service already initialized, skipping");
            return;
        }

        tracing::info!("Initializing rule engine event service...");
        tracing::info!(
            "event_bus available: {}, rule_engine available: true",
            self.event_bus.is_some()
        );

        let (event_bus, rule_engine) = match (&self.event_bus, &self.rule_engine) {
            (Some(bus), engine) => (bus, engine),
            _ => {
                tracing::warn!("Rule engine events not started: event_bus or rule_engine not available");
                return;
            }
        };

        use crate::event_services::RuleEngineEventService;

        // Get or create the service instance (cached in ServerState)
        {
            let mut cached_service = self.rule_engine_event_service.lock().await;
            if cached_service.is_none() {
                let service = RuleEngineEventService::new(
                    (*event_bus).clone(),
                    rule_engine.clone(),
                );
                *cached_service = Some(service);
            }
        }

        // Start the service (compare_exchange inside prevents duplicate tasks)
        let running = {
            let cached_service = self.rule_engine_event_service.lock().await;
            cached_service.as_ref().unwrap().start()
        };

        if running.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                category = "rule_engine",
                "Rule engine event service started - rules will auto-evaluate on device metrics"
            );
        } else {
            tracing::warn!("Rule engine event service failed to start");
        }

        // Start a task to update the InMemoryValueProvider when device metrics arrive
        // This is needed for rule evaluation to work with current values
        let mut rx = event_bus.filter().device_events();
        let value_provider = rule_engine.get_value_provider();
        let rule_engine_for_update = rule_engine.clone();

        tokio::spawn(async move {
            use edge_ai_core::{MetricValue, NeoTalkEvent};

            tracing::info!("Starting value provider update task for rule engine");

            while let Some((event, _metadata)) = rx.recv().await {
                if let NeoTalkEvent::DeviceMetric {
                    device_id,
                    metric,
                    value,
                    timestamp: _,
                    quality: _,
                } = event
                {
                    tracing::debug!(
                        "Received device metric: {} {} = {:?}",
                        device_id, metric, value
                    );

                    // Extract numeric value for rule evaluation
                    let numeric_value = match &value {
                        MetricValue::Float(v) => Some(*v),
                        MetricValue::Integer(v) => Some(*v as f64),
                        MetricValue::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
                        _ => None,
                    };

                    if let Some(num_value) = numeric_value {
                        // Update the InMemoryValueProvider with the new value
                        if let Some(provider) = value_provider.as_any().downcast_ref::<InMemoryValueProvider>() {
                            // Store with original metric key
                            provider.set_value(&device_id, &metric, num_value);

                            // Also store with common prefixes stripped for rule matching
                            // Rules might reference "battery" while events use "values.battery"
                            let common_prefixes = ["values.", "value.", "data.", "telemetry.", "metrics.", "state."];
                            for prefix in &common_prefixes {
                                if let Some(stripped_metric) = metric.strip_prefix(prefix) {
                                    provider.set_value(&device_id, stripped_metric, num_value);
                                    break;
                                }
                            }
                        }

                        // Update rule states (for FOR clauses)
                        rule_engine_for_update.update_states().await;
                    }
                }
            }

            tracing::warn!("Value provider update task ended");
        });
    }

    /// Initialize auto-onboarding event listener.
    ///
    /// Starts a background task that listens for unknown device data events
    /// and routes them to the auto-onboarding manager for processing.
    pub async fn init_auto_onboarding_events(&self) {
        // Ensure we have event_bus
        let event_bus = match &self.event_bus {
            Some(bus) => bus.clone(),
            _ => {
                tracing::warn!("Auto-onboarding events not started: event_bus not available");
                return;
            }
        };

        // Get or create auto-onboard manager
        let auto_onboard_manager = {
            let mgr_guard = self.auto_onboard_manager.read().await;
            if let Some(mgr) = mgr_guard.as_ref() {
                mgr.clone()
            } else {
                drop(mgr_guard); // Release read lock before acquiring write lock
                // Create manager if it doesn't exist
                let mut mgr_guard = self.auto_onboard_manager.write().await;
                if let Some(mgr) = mgr_guard.as_ref() {
                    mgr.clone()
                } else {
                    // Create default LLM runtime
                    use edge_ai_core::llm::backend::LlmRuntime;
                    use edge_ai_llm::backends::{OllamaConfig, OllamaRuntime};

                    let config = OllamaConfig::new("qwen2.5:3b")
                        .with_endpoint("http://localhost:11434")
                        .with_timeout_secs(120);

                    let llm = match OllamaRuntime::new(config) {
                        Ok(runtime) => Arc::new(runtime) as Arc<dyn LlmRuntime>,
                        Err(_) => {
                            // Fallback: create a dummy runtime
                            struct DummyRuntime;
                            #[async_trait::async_trait]
                            impl LlmRuntime for DummyRuntime {
                                fn backend_id(&self) -> edge_ai_core::llm::backend::BackendId {
                                    edge_ai_core::llm::backend::BackendId::new("dummy")
                                }
                                fn model_name(&self) -> &str { "dummy" }
                                fn capabilities(&self) -> edge_ai_core::llm::backend::BackendCapabilities {
                                    edge_ai_core::llm::backend::BackendCapabilities::default()
                                }
                                async fn generate(&self, _input: edge_ai_core::llm::backend::LlmInput) -> Result<edge_ai_core::llm::backend::LlmOutput, edge_ai_core::llm::backend::LlmError> {
                                    Ok(edge_ai_core::llm::backend::LlmOutput {
                                        text: String::new(),
                                        thinking: None,
                                        finish_reason: edge_ai_core::llm::backend::FinishReason::Stop,
                                        usage: Some(edge_ai_core::llm::backend::TokenUsage::new(0, 0)),
                                    })
                                }
                                async fn generate_stream(
                                    &self,
                                    _input: edge_ai_core::llm::backend::LlmInput,
                                ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), edge_ai_core::llm::backend::LlmError>> + Send>>, edge_ai_core::llm::backend::LlmError> {
                                    Ok(Box::pin(futures::stream::empty()))
                                }
                                fn max_context_length(&self) -> usize { 4096 }
                            }
                            Arc::new(DummyRuntime) as Arc<dyn LlmRuntime>
                        }
                    };

                    let manager = Arc::new(edge_ai_automation::AutoOnboardManager::new(llm, event_bus.clone()));
                    *mgr_guard = Some(manager.clone());
                    tracing::info!("AutoOnboardManager initialized at startup");
                    manager
                }
            }
        };

        let manager = auto_onboard_manager.clone();
        let event_bus_clone = event_bus.clone();
        let device_service_clone = self.device_service.clone();

        tokio::spawn(async move {
            let mut rx = event_bus_clone.subscribe();
            tracing::info!("Auto-onboarding event listener started");

            while let Some((event, _metadata)) = rx.recv().await {
                // Check if this is an unknown device data event
                if let edge_ai_core::NeoTalkEvent::Custom { event_type, data } = event
                    && event_type == "unknown_device_data" {
                        // Extract device_id and sample from the event data
                        if let Some(device_id) = data.get("device_id").and_then(|v| v.as_str())
                            && let Some(sample) = data.get("sample") {
                                // Extract the actual payload data from sample
                                let payload_data = sample.get("data").unwrap_or(sample);

                                let is_binary = data.get("is_binary")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                // Check if device is already registered - skip auto-onboarding if it is
                                if device_service_clone.get_device(device_id).await.is_some() {
                                    tracing::debug!(
                                        "Device {} already registered, skipping auto-onboarding",
                                        device_id
                                    );
                                    continue;
                                }

                                tracing::info!(
                                    "Processing unknown device data from MQTT: device_id={}, is_binary={}",
                                    device_id, is_binary
                                );

                                let source = data.get("source")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("mqtt")
                                    .to_string();

                                // Extract original_topic if available (for MQTT devices)
                                let original_topic = data.get("original_topic")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                // Extract adapter_id if available (for external brokers)
                                let adapter_id = data.get("adapter_id")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                // Process the payload data through auto-onboarding
                                match manager.process_unknown_device_with_topic(
                                    device_id,
                                    &source,
                                    payload_data,
                                    is_binary,
                                    original_topic,
                                    adapter_id,
                                ).await {
                                    Ok(true) => {
                                        tracing::info!(
                                            "Successfully processed unknown device data: {}",
                                            device_id
                                        );
                                    }
                                    Ok(false) => {
                                        tracing::debug!(
                                            "Unknown device data not accepted (disabled or at capacity): {}",
                                            device_id
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to process unknown device data for {}: {}",
                                            device_id, e
                                        );
                                    }
                                }
                            }
                    }
            }
        });

        tracing::info!("Auto-onboarding event listener initialized - MQTT unknown devices will trigger auto-onboarding");
    }

    /// Save LLM configuration to database.
    pub async fn save_llm_config(&self, request: &LlmSettingsRequest) -> std::io::Result<()> {
        let settings = request.to_llm_settings();
        crate::config::save_llm_settings(&settings)
            .await
            .map_err(|e| std::io::Error::other(format!("{}", e)))
    }

    /// Initialize transform event service.
    ///
    /// Starts a background task that subscribes to DeviceMetric events on the EventBus
    /// and processes transforms to generate virtual metrics.
    pub async fn init_transform_event_service(&self) {
        use crate::event_services::TransformEventService;

        let (event_bus, transform_engine, automation_store) = match (
            &self.event_bus,
            &self.transform_engine,
            &self.automation_store,
        ) {
            (Some(bus), Some(engine), Some(store)) => (bus.clone(), engine.clone(), store.clone()),
            _ => {
                tracing::warn!("Transform event service not started: required components not available");
                return;
            }
        };

        let service = TransformEventService::new(
            event_bus,
            transform_engine,
            automation_store,
            self.time_series_storage.clone(),
            self.device_registry.clone(),
        );

        let running = service.start();
        if running.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                category = "transform",
                "Transform event service started - virtual metrics will be generated from transforms"
            );
        } else {
            tracing::warn!("Transform event service failed to start");
        }
    }

    /// Get or initialize the AI Agent manager.
    pub async fn get_or_init_agent_manager(&self) -> Result<AgentManager, crate::models::ErrorResponse> {
        let mgr_guard = self.agent_manager.read().await;
        if let Some(mgr) = mgr_guard.as_ref() {
            return Ok(mgr.clone());
        }
        drop(mgr_guard);

        // Initialize the manager
        let mut mgr_guard = self.agent_manager.write().await;
        if let Some(mgr) = mgr_guard.as_ref() {
            return Ok(mgr.clone());
        }

        // Create or open TimeSeriesStore for agent data collection
        let time_series_store = match edge_ai_storage::TimeSeriesStore::open("data/timeseries_agents.redb") {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open TimeSeriesStore, agents will not collect data");
                None
            }
        };

        // Get LLM runtime from SessionManager if available
        let llm_runtime = if let Ok(Some(backend)) = self.session_manager.get_llm_backend().await {
            use edge_ai_agent::LlmBackend;
            use edge_ai_llm::{OllamaConfig, OllamaRuntime, CloudConfig, CloudRuntime};
            use edge_ai_core::llm::backend::LlmRuntime;

            match backend {
                LlmBackend::Ollama { endpoint, model } => {
                    let timeout = std::env::var("OLLAMA_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(120);
                    match OllamaRuntime::new(OllamaConfig::new(&model).with_endpoint(&endpoint).with_timeout_secs(timeout)) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create Ollama runtime for agents");
                            None
                        }
                    }
                }
                LlmBackend::OpenAi { api_key, endpoint, model } => {
                    // Use CloudRuntime for OpenAI-compatible APIs
                    let timeout = std::env::var("OPENAI_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(60);
                    match CloudRuntime::new(
                        CloudConfig::custom(&api_key, &endpoint)
                            .with_model(&model)
                            .with_timeout_secs(timeout)
                    ) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create OpenAI runtime for agents");
                            None
                        }
                    }
                }
            }
        } else {
            tracing::info!(category = "ai", "No LLM backend configured for agents");
            None
        };

        let has_llm = llm_runtime.is_some();
        let has_time_series = time_series_store.is_some();

        // Open LLM backend store for per-agent backend lookup
        let llm_backend_store = match LlmBackendStore::open("data/llm_backends.redb") {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open LlmBackendStore");
                None
            }
        };

        let executor_config = edge_ai_agent::ai_agent::AgentExecutorConfig {
            store: self.agent_store.clone(),
            time_series_storage: time_series_store,
            device_service: Some(self.device_service.clone()),
            event_bus: self.event_bus.clone(),
            alert_manager: Some(self.alert_manager.clone()),
            llm_runtime,
            llm_backend_store,
        };

        let manager = edge_ai_agent::ai_agent::AiAgentManager::new(executor_config)
            .await
            .map_err(|e| crate::models::ErrorResponse::internal(format!("Failed to create agent manager: {}", e)))?;

        *mgr_guard = Some(manager.clone());

        tracing::info!(
            has_llm,
            has_time_series,
            "AI Agent manager initialized"
        );
        Ok(manager)
    }

    /// Start the AI Agent manager scheduler.
    pub async fn start_agent_manager(&self) -> Result<(), crate::models::ErrorResponse> {
        let manager = self.get_or_init_agent_manager().await?;
        manager.start().await
            .map_err(|e| crate::models::ErrorResponse::internal(format!("Failed to start agent manager: {}", e)))?;
        tracing::info!("AI Agent manager scheduler started");
        Ok(())
    }

    /// Initialize AI Agent event listener.
    ///
    /// Starts a background task that listens for device events and triggers
    /// event-scheduled agents.
    pub async fn init_agent_events(&self) {
        // Prevent duplicate initialization
        if self.agent_events_initialized.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::debug!("Agent event listener already initialized, skipping");
            return;
        }

        let manager = match self.get_or_init_agent_manager().await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Agent event listener not started: {}", e);
                return;
            }
        };

        let event_bus = match &self.event_bus {
            Some(bus) => bus.clone(),
            _ => {
                tracing::warn!("Agent event listener not started: event_bus not available");
                return;
            }
        };

        let executor = manager.executor().clone();
        let store = executor.store().clone();

        // Cleanup agents stuck in Executing status on startup
        tokio::spawn(async move {
            if let Ok(agents) = store.query_agents(edge_ai_storage::AgentFilter {
                status: Some(edge_ai_storage::AgentStatus::Executing),
                ..Default::default()
            }).await {
                let now = chrono::Utc::now().timestamp();
                for agent in agents {
                    // Check if agent has been executing for more than 10 minutes
                    if let Some(last_exec) = agent.last_execution_at {
                        let exec_duration_secs = now - last_exec;
                        if exec_duration_secs > 600 { // 10 minutes
                            tracing::warn!(
                                agent_id = %agent.id,
                                agent_name = %agent.name,
                                exec_duration_secs = exec_duration_secs,
                                "Agent stuck in Executing status, resetting to Active"
                            );
                            let _ = store.update_agent_status(&agent.id, edge_ai_storage::AgentStatus::Active, Some(
                                "Execution timeout - status reset".to_string()
                            )).await;
                        }
                    } else {
                        // No last execution time but status is Executing - reset it
                        tracing::warn!(
                            agent_id = %agent.id,
                            agent_name = %agent.name,
                            "Agent in Executing status with no execution time, resetting to Active"
                        );
                        let _ = store.update_agent_status(&agent.id, edge_ai_storage::AgentStatus::Active, Some(
                            "Invalid executing state - status reset".to_string()
                        )).await;
                    }
                }
            }
        });

        tokio::spawn(async move {
            let mut rx = event_bus.subscribe();
            tracing::info!("Agent event listener started - monitoring for event-triggered agents");

            while let Some((event, _metadata)) = rx.recv().await {
                // Check if any agent should be triggered by this event
                if let edge_ai_core::NeoTalkEvent::DeviceMetric { device_id, metric, value, timestamp: _, quality: _ } = event {
                    // Trigger agents that have this device/metric in their event filter
                    if let Err(e) = executor.check_and_trigger_event(device_id, &metric, &value).await {
                        tracing::debug!("No agent triggered for event: {}", e);
                    }
                }
            }
        });
    }
}

// Note: Default implementation removed because ServerState::new() is now async
// to support persistent device registry initialization.
