//! Server state and types.

use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;

use neomind_agent::SessionManager;
use neomind_commands::{CommandManager, CommandQueue, CommandStateStore};
use neomind_core::{extension::ExtensionRegistry, EventBus};
use neomind_devices::adapter::AdapterResult;
use neomind_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use neomind_rules::{
    device_integration::DeviceActionExecutor, extension_integration::ExtensionActionExecutor,
    store::RuleStore, RuleEngine, UnifiedValueProvider,
};
use neomind_storage::dashboards::DashboardStore;
use neomind_storage::llm_backends::LlmBackendStore;

use neomind_automation::{
    intent::IntentAnalyzer, store::SharedAutomationStore, transform::TransformEngine,
    AutoOnboardManager,
};
use neomind_memory::TieredMemory;
use neomind_messages::MessageManager;

use crate::auth::AuthState as ApiKeyAuthState;
use crate::auth_users::AuthUserState;
use crate::config::LlmSettingsRequest;
use crate::rate_limit::{RateLimitConfig, RateLimiter};
use crate::server::state::{
    AgentManager, AgentState, AuthState, AutomationState, CoreState, DeviceState,
    ExtensionMetricsStorage, ExtensionRegistryAdapter, ExtensionState,
};

#[cfg(feature = "embedded-broker")]
use neomind_devices::EmbeddedBroker;

/// Maximum request body size (10 MB)
pub const MAX_REQUEST_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Server state shared across all handlers.
///
/// Organized into logical sub-states for better maintainability.
#[derive(Clone)]
pub struct ServerState {
    /// Core system services (EventBus, CommandManager, MessageManager)
    pub core: CoreState,

    /// Device management (Registry, Service, Telemetry, Broker)
    pub devices: DeviceState,

    /// Extension management (Registry, Metrics Storage) - Decoupled from devices
    pub extensions: ExtensionState,

    /// Automation and rules (RuleEngine, Stores, IntentAnalyzer, TransformEngine)
    pub automation: AutomationState,

    /// AI Agents and sessions (SessionManager, Memory, Store, Manager)
    pub agents: AgentState,

    /// Authentication (API keys and JWT)
    pub auth: AuthState,

    /// Response cache for API endpoints.
    pub response_cache: Arc<crate::cache::ResponseCache>,

    /// Rate limiter for API request throttling.
    pub rate_limiter: Arc<RateLimiter>,

    /// Auto-onboarding manager for zero-config device discovery (lazy-initialized).
    pub auto_onboard_manager: Arc<tokio::sync::RwLock<Option<Arc<AutoOnboardManager>>>>,

    /// Dashboard store for visual dashboard persistence.
    pub dashboard_store: Arc<DashboardStore>,

    /// Server start timestamp.
    pub started_at: i64,

    /// Flag to track if agent events have been initialized (prevents duplicate subscribers).
    agent_events_initialized: Arc<std::sync::atomic::AtomicBool>,

    /// Flag to track if rule engine events have been initialized (prevents duplicate subscribers).
    rule_engine_events_initialized: Arc<std::sync::atomic::AtomicBool>,

    /// Cached rule engine event service instance (prevents duplicate instances).
    rule_engine_event_service:
        Arc<tokio::sync::Mutex<Option<crate::event_services::RuleEngineEventService>>>,
}

// Backward compatibility: Provide direct field access as before
impl ServerState {
    /// Get session manager (backward compatibility).
    pub fn session_manager(&self) -> Arc<SessionManager> {
        self.agents.session_manager.clone()
    }

    /// Get time series storage (backward compatibility).
    pub fn time_series_storage(&self) -> Arc<TimeSeriesStorage> {
        self.devices.telemetry.clone()
    }

    /// Get rule engine (backward compatibility).
    pub fn rule_engine(&self) -> Arc<RuleEngine> {
        self.automation.rule_engine.clone()
    }

    /// Get rule store (backward compatibility).
    pub fn rule_store(&self) -> Option<Arc<RuleStore>> {
        self.automation.rule_store.clone()
    }

    /// Get message manager (backward compatibility).
    pub fn message_manager(&self) -> Arc<MessageManager> {
        self.core.message_manager.clone()
    }

    /// Get automation store (backward compatibility).
    pub fn automation_store(&self) -> Option<Arc<SharedAutomationStore>> {
        self.automation.automation_store.clone()
    }

    /// Get intent analyzer (backward compatibility).
    pub fn intent_analyzer(&self) -> Option<Arc<IntentAnalyzer>> {
        self.automation.intent_analyzer.clone()
    }

    /// Get transform engine (backward compatibility).
    pub fn transform_engine(&self) -> Option<Arc<TransformEngine>> {
        self.automation.transform_engine.clone()
    }

    /// Get embedded broker (backward compatibility).
    #[cfg(feature = "embedded-broker")]
    pub fn embedded_broker(&self) -> Option<Arc<EmbeddedBroker>> {
        self.devices.embedded_broker.clone()
    }

    /// Get event bus (backward compatibility).
    pub fn event_bus(&self) -> Option<Arc<EventBus>> {
        self.core.event_bus.clone()
    }

    /// Get command manager (backward compatibility).
    pub fn command_manager(&self) -> Option<Arc<CommandManager>> {
        self.core.command_manager.clone()
    }

    /// Get API key auth state (backward compatibility).
    pub fn auth_state(&self) -> Arc<ApiKeyAuthState> {
        self.auth.api_key_state.clone()
    }

    /// Get user auth state (backward compatibility).
    pub fn auth_user_state(&self) -> Arc<AuthUserState> {
        self.auth.user_state.clone()
    }

    /// Get extension registry (backward compatibility).
    pub fn extension_registry(&self) -> Arc<ExtensionRegistry> {
        self.extensions.registry.clone()
    }

    /// Get device registry (backward compatibility).
    pub fn device_registry(&self) -> Arc<DeviceRegistry> {
        self.devices.registry.clone()
    }

    /// Get device service (backward compatibility).
    pub fn device_service(&self) -> Arc<DeviceService> {
        self.devices.service.clone()
    }

    /// Get rule history store (backward compatibility).
    pub fn rule_history_store(&self) -> Option<Arc<neomind_storage::business::RuleHistoryStore>> {
        self.automation.rule_history_store.clone()
    }

    /// Get memory (backward compatibility).
    pub fn memory(&self) -> Arc<tokio::sync::RwLock<TieredMemory>> {
        self.agents.memory.clone()
    }

    /// Get agent store (backward compatibility).
    pub fn agent_store(&self) -> Arc<neomind_storage::AgentStore> {
        self.agents.agent_store.clone()
    }

    /// Get agent manager (backward compatibility).
    pub fn agent_manager(&self) -> Arc<tokio::sync::RwLock<Option<AgentManager>>> {
        self.agents.agent_manager.clone()
    }
}

impl ServerState {
    /// Create a new server state.
    /// This is now async to support persistent device registry initialization.
    pub async fn new() -> Self {
        let started_at = chrono::Utc::now().timestamp();

        // ========== Create Unified Value Provider ==========
        // This will be wired up with device and extension storage later
        let value_provider = Arc::new(UnifiedValueProvider::new().with_ttl(5000));

        // Ensure data directory exists
        if let Err(e) = std::fs::create_dir_all("data") {
            tracing::warn!(category = "storage", error = %e, "Failed to create data directory");
        }

        // ========== Build CORE STATE ==========
        // Create event bus FIRST (needed for adapters to publish events)
        let event_bus = Some(Arc::new(EventBus::new()));

        // Create command manager
        let command_queue = Arc::new(CommandQueue::new(1000));
        let command_state = Arc::new(CommandStateStore::new(10000));
        let command_manager = Some(Arc::new(CommandManager::new(command_queue, command_state)));

        // Create message manager with persistent storage
        let message_manager = match MessageManager::with_storage("data/messages.redb") {
            Ok(manager) => {
                tracing::info!("Message store initialized at data/messages.redb");
                Arc::new(manager)
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open message store, using in-memory");
                Arc::new(MessageManager::new())
            }
        };
        message_manager.register_default_channels().await;

        let core = CoreState::new(event_bus.clone(), command_manager, message_manager.clone());

        // ========== Build DEVICE STATE ==========
        // Create device registry with persistent storage
        let device_registry = match DeviceRegistry::with_persistence("data/devices.redb").await {
            Ok(registry) => {
                tracing::info!(
                    "Device registry initialized with persistent storage at data/devices.redb"
                );
                Arc::new(registry)
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open persistent device registry, using in-memory");
                Arc::new(DeviceRegistry::new())
            }
        };

        // Create time series storage
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

        // Create device service
        let event_bus_for_service = (**event_bus.as_ref().unwrap()).clone();
        let device_service = Arc::new(DeviceService::new(
            device_registry.clone(),
            event_bus_for_service,
        ));
        device_service
            .set_telemetry_storage(time_series_storage.clone())
            .await;

        // Create device status broadcast channel
        let device_update_tx: tokio::sync::broadcast::Sender<super::state::DeviceStatusUpdate> =
            tokio::sync::broadcast::channel(100).0;

        let devices = DeviceState::new(
            device_registry,
            device_service,
            time_series_storage.clone(),
            device_update_tx,
        );

        // ========== Build EXTENSION STATE ==========
        // Create extension registry with default directories
        let default_ext_dirs = vec![
            std::path::PathBuf::from("extensions"), // Local extensions dir
            std::path::PathBuf::from("data/extensions"), // Data directory extensions
        ];

        let mut registry_builder = ExtensionRegistry::new();
        for ext_dir in &default_ext_dirs {
            if ext_dir.exists() {
                registry_builder.add_extension_dir(ext_dir.clone());
                tracing::info!("Added extension discovery directory: {:?}", ext_dir);
            }
        }
        let extension_registry = Arc::new(registry_builder);

        // Create extension metrics storage (shares device telemetry.redb)
        let extension_metrics_storage = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            time_series_storage.clone(),
        ));

        // Create the extension state with registry and storage
        let extensions = ExtensionState::new(extension_registry, extension_metrics_storage);

        tracing::info!("Extension state initialized");

        // ========== Build AUTOMATION STATE ==========
        let rule_engine = Arc::new(RuleEngine::new(value_provider.clone()));

        // Wire rule engine to message manager
        rule_engine
            .set_message_manager(core.message_manager.clone())
            .await;

        // Wire rule engine to device service
        let event_bus_for_action = (**event_bus.as_ref().unwrap()).clone();
        let device_service_for_action = devices.service.clone();
        let device_action_executor = Arc::new(DeviceActionExecutor::with_device_service(
            event_bus_for_action,
            device_service_for_action,
        ));
        rule_engine
            .set_device_action_executor(device_action_executor)
            .await;

        // Wire rule engine to extension registry for extension command execution
        let extension_registry_adapter =
            Arc::new(ExtensionRegistryAdapter::new(extensions.registry.clone()));
        let extension_action_executor =
            Arc::new(ExtensionActionExecutor::new(extension_registry_adapter));
        rule_engine
            .set_extension_action_executor(extension_action_executor)
            .await;

        // Wire event bus to message manager
        if let Some(ref bus) = event_bus {
            core.message_manager.set_event_bus(bus.clone()).await;
        }

        // Create rule store
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
                        if let Err(e) = rule_engine.add_rule(rule.clone()).await {
                            tracing::warn!("Failed to load rule {}: {}", rule.id, e);
                        } else {
                            tracing::debug!("Loaded rule: {} ({})", rule.name, rule.id);
                        }
                    }
                    tracing::info!(
                        "Successfully loaded {} rules from persistent store",
                        rule_count
                    );
                }
                Err(e) => {
                    tracing::warn!(category = "storage", error = %e, "Failed to load rules from store");
                }
            }
        }

        // Create automation store
        let automation_store = match SharedAutomationStore::open("data/automations.redb").await {
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

        // Create transform engine with extension registry support
        let transform_engine = Some(Arc::new(TransformEngine::with_extension_registry(
            extensions.registry.clone(),
        )));
        tracing::info!("Transform engine initialized with extension registry");

        // Create rule history store
        let rule_history_store = match neomind_storage::business::RuleHistoryStore::open(
            "data/rule_history.redb",
        ) {
            Ok(store) => {
                tracing::info!("Rule history store initialized at data/rule_history.redb");
                Some(Arc::new(store))
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open rule history store, statistics will be limited");
                None
            }
        };

        let automation = AutomationState::new(
            rule_engine,
            rule_store,
            automation_store,
            None, // intent_analyzer - TODO: Initialize with LLM backend
            transform_engine,
            rule_history_store,
        );

        // ========== Build AGENT STATE ==========
        // Create session manager
        let session_manager = SessionManager::new().unwrap_or_else(|e| {
            tracing::warn!(category = "storage", error = %e, "Failed to create persistent SessionManager, using in-memory");
            SessionManager::memory()
        });

        // Create tiered memory
        let memory_config = crate::config::get_memory_config();
        let memory = Arc::new(tokio::sync::RwLock::new(TieredMemory::with_config(
            memory_config,
        )));

        // Create agent store
        let agent_store = match neomind_storage::AgentStore::open("data/agents.redb") {
            Ok(store) => {
                tracing::info!("AI Agent store initialized at data/agents.redb");
                store
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open agent store, using in-memory");
                neomind_storage::AgentStore::memory().unwrap_or_else(|e| {
                    tracing::error!(category = "storage", error = %e, "Failed to create in-memory agent store");
                    std::process::exit(1);
                })
            }
        };

        let agents = AgentState::new(
            Arc::new(session_manager),
            memory,
            agent_store,
            Arc::new(tokio::sync::RwLock::new(None)),
        );

        // ========== Build AUTH STATE ==========
        let auth = AuthState {
            api_key_state: Arc::new(ApiKeyAuthState::new()),
            user_state: Arc::new(AuthUserState::new()),
        };

        // ========== Cross-cutting services ==========
        let rate_limit_config = RateLimitConfig::default();
        let rate_limiter = Arc::new(RateLimiter::with_config(rate_limit_config));
        let response_cache = Arc::new(crate::cache::ResponseCache::with_default_ttl());

        let auto_onboard_manager = Arc::new(tokio::sync::RwLock::new(None));

        let dashboard_store = match DashboardStore::open("data/dashboards.redb") {
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
        };

        Self {
            core,
            devices,
            extensions,
            automation,
            agents,
            auth,
            response_cache,
            rate_limiter,
            auto_onboard_manager,
            dashboard_store,
            started_at,
            agent_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_event_service: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Create a new server state for testing.
    ///
    /// This creates a minimal ServerState with all in-memory storage,
    /// suitable for parallel test execution without shared state.
    ///
    /// # Test Isolation
    /// Each call creates a completely isolated instance with:
    /// - In-memory user storage (no database)
    /// - In-memory device registry
    /// - In-memory time-series storage
    /// - In-memory session manager
    /// - Fresh event bus and message manager
    /// - No API key generation
    #[cfg(any(test, feature = "testing"))]
    pub async fn new_for_testing() -> Self {
        let started_at = chrono::Utc::now().timestamp();

        // Create unified value provider
        let value_provider = Arc::new(UnifiedValueProvider::new().with_ttl(5000));

        // ========== Build CORE STATE ==========
        let event_bus = Some(Arc::new(EventBus::new()));
        let command_queue = Arc::new(CommandQueue::new(1000));
        let command_state = Arc::new(CommandStateStore::new(10000));
        let command_manager = Some(Arc::new(CommandManager::new(command_queue, command_state)));

        // In-memory message manager
        let message_manager = Arc::new(MessageManager::new());
        message_manager.register_default_channels().await;

        let core = CoreState::new(event_bus.clone(), command_manager, message_manager.clone());

        // ========== Build DEVICE STATE ==========
        // In-memory device registry
        let device_registry = Arc::new(DeviceRegistry::new());
        let time_series_storage = Arc::new(TimeSeriesStorage::memory().unwrap());

        let event_bus_for_service = (**event_bus.as_ref().unwrap()).clone();
        let device_service = Arc::new(DeviceService::new(
            device_registry.clone(),
            event_bus_for_service,
        ));
        device_service
            .set_telemetry_storage(time_series_storage.clone())
            .await;

        let device_update_tx: tokio::sync::broadcast::Sender<super::state::DeviceStatusUpdate> =
            tokio::sync::broadcast::channel(100).0;

        let devices = DeviceState::new(
            device_registry,
            device_service,
            time_series_storage.clone(),
            device_update_tx,
        );

        // ========== Build EXTENSION STATE ==========
        let extension_registry = Arc::new(ExtensionRegistry::new());
        let extension_metrics_storage = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            time_series_storage.clone(),
        ));
        let extensions = ExtensionState::new(extension_registry, extension_metrics_storage);

        // ========== Build AUTOMATION STATE ==========
        let rule_engine = Arc::new(RuleEngine::new(value_provider.clone()));
        rule_engine
            .set_message_manager(core.message_manager.clone())
            .await;

        let event_bus_for_action = (**event_bus.as_ref().unwrap()).clone();
        let device_service_for_action = devices.service.clone();
        let device_action_executor = Arc::new(DeviceActionExecutor::with_device_service(
            event_bus_for_action,
            device_service_for_action,
        ));
        rule_engine
            .set_device_action_executor(device_action_executor)
            .await;

        let extension_registry_adapter =
            Arc::new(ExtensionRegistryAdapter::new(extensions.registry.clone()));
        let extension_action_executor =
            Arc::new(ExtensionActionExecutor::new(extension_registry_adapter));
        rule_engine
            .set_extension_action_executor(extension_action_executor)
            .await;

        if let Some(ref bus) = event_bus {
            core.message_manager.set_event_bus(bus.clone()).await;
        }

        // In-memory stores
        let automation_store = Some(Arc::new(SharedAutomationStore::memory().unwrap()));
        let transform_engine = Some(Arc::new(TransformEngine::with_extension_registry(
            extensions.registry.clone(),
        )));
        let rule_history_store = None; // Skip for tests

        let automation = AutomationState::new(
            rule_engine,
            None, // rule_store - skip for tests
            automation_store,
            None, // intent_analyzer
            transform_engine,
            rule_history_store,
        );

        // ========== Build AGENT STATE ==========
        let session_manager = SessionManager::memory();
        let memory_config = crate::config::get_memory_config();
        let memory = Arc::new(tokio::sync::RwLock::new(TieredMemory::with_config(
            memory_config,
        )));
        let agent_store = neomind_storage::AgentStore::memory().unwrap();

        let agents = AgentState::new(
            Arc::new(session_manager),
            memory,
            agent_store,
            Arc::new(tokio::sync::RwLock::new(None)),
        );

        // ========== Build AUTH STATE ==========
        // Use in-memory storage for tests - no API key generation
        let auth = AuthState {
            api_key_state: Arc::new(crate::auth::AuthState::new_for_testing()),
            user_state: Arc::new(AuthUserState::new_with_memory_store()),
        };

        // ========== Cross-cutting services ==========
        let rate_limiter = Arc::new(RateLimiter::with_config(RateLimitConfig::default()));
        let response_cache = Arc::new(crate::cache::ResponseCache::with_default_ttl());
        let auto_onboard_manager = Arc::new(tokio::sync::RwLock::new(None));
        let dashboard_store = DashboardStore::memory().unwrap();

        Self {
            core,
            devices,
            extensions,
            automation,
            agents,
            auth,
            response_cache,
            rate_limiter,
            auto_onboard_manager,
            dashboard_store,
            started_at,
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

    /// Initialize extensions from persistent storage.
    ///
    /// This loads all extensions marked with `auto_start=true` from the extension store.
    /// Must be called after the server is fully initialized.
    pub async fn init_extensions(&self) {
        match self.extensions.load_from_storage().await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        category = "extensions",
                        loaded = count,
                        "Loaded extensions from storage"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    category = "extensions",
                    error = %e,
                    "Failed to load extensions from storage (continuing without stored extensions)"
                );
            }
        }
    }

    /// Initialize LLM backend using the unified config loader.
    /// Falls back to LlmBackendInstanceManager if no config file is found.
    pub async fn init_llm(&self) {
        // First try to load from config file
        if let Some(backend) = crate::config::load_llm_config() {
            match self.agents.session_manager.set_llm_backend(backend).await {
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
            .agents
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
        use neomind_devices::adapter::DeviceAdapter;
        use neomind_devices::adapters::{create_adapter, mqtt::MqttAdapterConfig};

        // Start device service to listen for EventBus events
        self.devices.service.start().await;

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
            mqtt: neomind_devices::mqtt::MqttConfig {
                broker: "localhost".to_string(),
                port: 1883,
                client_id: Some("neomind-internal".to_string()),
                username: None,
                password: None,
                keep_alive: 60,
                clean_session: true,
                qos: 1,
                topic_prefix: "device".to_string(),
                command_topic: "downlink".to_string(),
            },
            subscribe_topics: vec!["#".to_string()], // Subscribe to ALL topics for auto-discovery
            discovery_topic: Some("device/+/+/uplink".to_string()),
            discovery_prefix: "device".to_string(),
            auto_discovery: true,
            storage_dir: Some("data".to_string()),
        };

        // Create the MQTT adapter
        let Some(event_bus) = self.core.event_bus.as_ref() else {
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

        let mqtt_adapter_result: AdapterResult<Arc<dyn DeviceAdapter>> =
            { create_adapter("mqtt", &mqtt_config_value, event_bus) };

        match mqtt_adapter_result {
            Ok(mqtt_adapter) => {
                // Set telemetry storage for the MQTT adapter so it can write metrics
                mqtt_adapter.set_telemetry_storage(self.devices.telemetry.clone());

                // Try to set the shared device registry on the MQTT adapter
                // This allows the adapter to look up devices by custom telemetry topics
                if let Some(mqtt) = mqtt_adapter
                    .as_any()
                    .downcast_ref::<neomind_devices::adapters::mqtt::MqttAdapter>()
                {
                    mqtt.set_shared_device_registry(self.devices.service.get_registry().await)
                        .await;
                }

                // Register adapter with device service
                self.devices
                    .service
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
                tracing::warn!(
                    "Failed to open settings store for external broker reconnection: {}",
                    e
                );
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

        tracing::info!(
            "Found {} external MQTT broker(s), attempting to reconnect...",
            brokers.len()
        );

        let Some(event_bus) = self.core.event_bus.as_ref() else {
            tracing::warn!("EventBus not initialized, cannot reconnect external brokers");
            return;
        };

        let context = ExternalBrokerContext {
            device_service: self.devices.service.clone(),
            event_bus: event_bus.clone(),
        };

        for broker in brokers {
            // Skip disabled brokers
            if !broker.enabled {
                tracing::info!("Skipping disabled external broker: {}", broker.id);
                continue;
            }

            tracing::info!(
                "Reconnecting to external broker: {} ({})",
                broker.id,
                broker.name
            );

            // Use the broker connection logic
            match create_and_connect_broker(&broker, &context).await {
                Ok(connected) => {
                    if connected {
                        tracing::info!(
                            "Successfully reconnected to external broker: {}",
                            broker.id
                        );
                    } else {
                        tracing::warn!(
                            "External broker reconnection attempted but failed: {}",
                            broker.id
                        );
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
        use neomind_tools::ToolRegistryBuilder;
        use std::sync::Arc;

        // Build tool registry with real implementations that connect to actual services
        let builder = ToolRegistryBuilder::new()
            // Real implementations
            .with_query_data_tool(
                self.devices.telemetry.clone(),
                Some(self.devices.service.clone()),
            )
            .with_get_device_data_tool(self.devices.service.clone(), self.devices.telemetry.clone())
            .with_control_device_tool(self.devices.service.clone())
            .with_list_devices_tool(self.devices.service.clone())
            .with_device_analyze_tool(self.devices.service.clone(), self.devices.telemetry.clone())
            .with_create_rule_tool(self.automation.rule_engine.clone())
            .with_list_rules_tool(self.automation.rule_engine.clone())
            .with_delete_rule_tool(self.automation.rule_engine.clone())
            // AI Agent tools for Chat integration
            .with_agent_tools(self.agents.agent_store.clone())
            // System help tool for onboarding
            .with_system_help_tool_named("NeoMind");

        let tool_registry = Arc::new(builder.build());
        self.agents
            .session_manager
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
        if self
            .rule_engine_events_initialized
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            tracing::debug!("Rule engine event service already initialized, skipping");
            return;
        }

        tracing::info!("Initializing rule engine event service...");
        tracing::info!(
            "event_bus available: {}, rule_engine available: true",
            self.core.event_bus.is_some()
        );

        let (event_bus, rule_engine) = match (&self.core.event_bus, &self.automation.rule_engine) {
            (Some(bus), engine) => (bus, engine),
            _ => {
                tracing::warn!(
                    "Rule engine events not started: event_bus or rule_engine not available"
                );
                return;
            }
        };

        use crate::event_services::RuleEngineEventService;

        // Get or create the service instance (cached in ServerState)
        {
            let mut cached_service = self.rule_engine_event_service.lock().await;
            if cached_service.is_none() {
                let service =
                    RuleEngineEventService::new((*event_bus).clone(), rule_engine.clone());
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

        // Start a task to update the UnifiedValueProvider when device metrics arrive
        // This is needed for rule evaluation to work with current values
        let mut rx = event_bus.filter().device_events();
        let value_provider = rule_engine.get_value_provider();
        let rule_engine_for_update = rule_engine.clone();

        tokio::spawn(async move {
            use neomind_core::{MetricValue, NeoMindEvent};

            tracing::info!("Starting value provider update task for rule engine");

            while let Some((event, _metadata)) = rx.recv().await {
                if let NeoMindEvent::DeviceMetric {
                    device_id,
                    metric,
                    value,
                    timestamp: _,
                    quality: _,
                } = event
                {
                    tracing::debug!(
                        "Received device metric: {} {} = {:?}",
                        device_id,
                        metric,
                        value
                    );

                    // Extract numeric value for rule evaluation
                    let numeric_value = match &value {
                        MetricValue::Float(v) => Some(*v),
                        MetricValue::Integer(v) => Some(*v as f64),
                        MetricValue::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
                        _ => None,
                    };

                    if let Some(num_value) = numeric_value {
                        // Update the UnifiedValueProvider with the new value
                        if let Some(provider) = value_provider
                            .as_any()
                            .downcast_ref::<UnifiedValueProvider>()
                        {
                            // Store with original metric key
                            provider
                                .update_value("device", &device_id, &metric, num_value)
                                .await;

                            // Also store with common prefixes stripped for rule matching
                            // Rules might reference "battery" while events use "values.battery"
                            let common_prefixes = [
                                "values.",
                                "value.",
                                "data.",
                                "telemetry.",
                                "metrics.",
                                "state.",
                            ];
                            for prefix in &common_prefixes {
                                if let Some(stripped_metric) = metric.strip_prefix(prefix) {
                                    provider
                                        .update_value(
                                            "device",
                                            &device_id,
                                            stripped_metric,
                                            num_value,
                                        )
                                        .await;
                                    break;
                                }
                            }
                        }

                        // Update rule states (for FOR clauses)
                        rule_engine_for_update.update_states().await;

                        // Evaluate and execute any rules that should trigger
                        let results = rule_engine_for_update.execute_triggered().await;
                        if !results.is_empty() {
                            tracing::info!(
                                "Executed {} triggered rule(s) from device event: {} {} = {:?}",
                                results.len(),
                                device_id,
                                metric,
                                num_value
                            );
                            for result in &results {
                                if result.success {
                                    tracing::info!(
                                        "  Rule '{}' executed: actions={:?}",
                                        result.rule_name,
                                        result.actions_executed
                                    );
                                } else {
                                    tracing::warn!(
                                        "  Rule '{}' failed: {:?}",
                                        result.rule_name,
                                        result.error
                                    );
                                }
                            }
                        }
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
        let event_bus = match &self.core.event_bus {
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
                    use neomind_core::llm::backend::LlmRuntime;
                    use neomind_llm::backends::{OllamaConfig, OllamaRuntime};

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
                                fn backend_id(&self) -> neomind_core::llm::backend::BackendId {
                                    neomind_core::llm::backend::BackendId::new("dummy")
                                }
                                fn model_name(&self) -> &str {
                                    "dummy"
                                }
                                fn capabilities(
                                    &self,
                                ) -> neomind_core::llm::backend::BackendCapabilities
                                {
                                    neomind_core::llm::backend::BackendCapabilities::default()
                                }
                                async fn generate(
                                    &self,
                                    _input: neomind_core::llm::backend::LlmInput,
                                ) -> Result<
                                    neomind_core::llm::backend::LlmOutput,
                                    neomind_core::llm::backend::LlmError,
                                > {
                                    Ok(neomind_core::llm::backend::LlmOutput {
                                        text: String::new(),
                                        thinking: None,
                                        finish_reason:
                                            neomind_core::llm::backend::FinishReason::Stop,
                                        usage: Some(neomind_core::llm::backend::TokenUsage::new(
                                            0, 0,
                                        )),
                                    })
                                }
                                async fn generate_stream(
                                    &self,
                                    _input: neomind_core::llm::backend::LlmInput,
                                ) -> Result<
                                    Pin<
                                        Box<
                                            dyn Stream<
                                                    Item = Result<
                                                        (String, bool),
                                                        neomind_core::llm::backend::LlmError,
                                                    >,
                                                > + Send,
                                        >,
                                    >,
                                    neomind_core::llm::backend::LlmError,
                                > {
                                    Ok(Box::pin(futures::stream::empty()))
                                }
                                fn max_context_length(&self) -> usize {
                                    4096
                                }
                            }
                            Arc::new(DummyRuntime) as Arc<dyn LlmRuntime>
                        }
                    };

                    let manager = Arc::new(neomind_automation::AutoOnboardManager::new(
                        llm,
                        event_bus.clone(),
                    ));
                    *mgr_guard = Some(manager.clone());
                    tracing::info!("AutoOnboardManager initialized at startup");
                    manager
                }
            }
        };

        let manager = auto_onboard_manager.clone();
        let event_bus_clone = event_bus.clone();
        let device_service_clone = self.devices.service.clone();

        tokio::spawn(async move {
            let mut rx = event_bus_clone.subscribe();
            tracing::info!("Auto-onboarding event listener started");

            while let Some((event, _metadata)) = rx.recv().await {
                // Check if this is an unknown device data event
                if let neomind_core::NeoMindEvent::Custom { event_type, data } = event {
                    if event_type == "unknown_device_data" {
                        // Extract device_id and sample from the event data
                        if let Some(device_id) = data.get("device_id").and_then(|v| v.as_str()) {
                            if let Some(sample) = data.get("sample") {
                                // Extract the actual payload data from sample
                                let payload_data = sample.get("data").unwrap_or(sample);

                                let is_binary = data
                                    .get("is_binary")
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
                                    device_id,
                                    is_binary
                                );

                                let source = data
                                    .get("source")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("mqtt")
                                    .to_string();

                                // Extract original_topic if available (for MQTT devices)
                                let original_topic = data
                                    .get("original_topic")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                // Extract adapter_id if available (for external brokers)
                                let adapter_id = data
                                    .get("adapter_id")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                // Process the payload data through auto-onboarding
                                match manager
                                    .process_unknown_device_with_topic(
                                        device_id,
                                        &source,
                                        payload_data,
                                        is_binary,
                                        original_topic,
                                        adapter_id,
                                    )
                                    .await
                                {
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
                                            device_id,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        tracing::info!(
            "Auto-onboarding event listener initialized - MQTT unknown devices will trigger auto-onboarding"
        );
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
            &self.core.event_bus,
            &self.automation.transform_engine,
            &self.automation.automation_store,
        ) {
            (Some(bus), Some(engine), Some(store)) => (bus.clone(), engine.clone(), store.clone()),
            _ => {
                tracing::warn!(
                    "Transform event service not started: required components not available"
                );
                return;
            }
        };

        let service = TransformEventService::new(
            event_bus,
            transform_engine,
            automation_store,
            self.devices.telemetry.clone(),
            self.devices.registry.clone(),
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
    pub async fn get_or_init_agent_manager(
        &self,
    ) -> Result<AgentManager, crate::models::ErrorResponse> {
        let mgr_guard = self.agents.agent_manager.read().await;
        if let Some(mgr) = mgr_guard.as_ref() {
            return Ok(mgr.clone());
        }
        drop(mgr_guard);

        // Initialize the manager
        let mut mgr_guard = self.agents.agent_manager.write().await;
        if let Some(mgr) = mgr_guard.as_ref() {
            return Ok(mgr.clone());
        }

        // Create or open TimeSeriesStore for agent data collection
        // Use the same telemetry.redb as DeviceService to access device telemetry data
        let time_series_store = match neomind_storage::TimeSeriesStore::open("data/telemetry.redb")
        {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open TimeSeriesStore, agents will not collect data");
                None
            }
        };

        // Get LLM runtime from SessionManager if available
        let llm_runtime = if let Ok(Some(backend)) =
            self.agents.session_manager.get_llm_backend().await
        {
            use neomind_agent::LlmBackend;
            use neomind_core::llm::backend::LlmRuntime;
            use neomind_llm::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};

            match backend {
                LlmBackend::Ollama { endpoint, model } => {
                    let timeout = std::env::var("OLLAMA_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(120);
                    match OllamaRuntime::new(
                        OllamaConfig::new(&model)
                            .with_endpoint(&endpoint)
                            .with_timeout_secs(timeout),
                    ) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create Ollama runtime for agents");
                            None
                        }
                    }
                }
                LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model,
                } => {
                    // Use CloudRuntime for OpenAI-compatible APIs
                    let timeout = std::env::var("OPENAI_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(60);
                    match CloudRuntime::new(
                        CloudConfig::custom(&api_key, &endpoint)
                            .with_model(&model)
                            .with_timeout_secs(timeout),
                    ) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create OpenAI runtime for agents");
                            None
                        }
                    }
                }
                // Other cloud backends (Anthropic, Google, XAi, Qwen, DeepSeek, GLM, MiniMax)
                _backend => {
                    let (api_key, endpoint, model) = match &_backend {
                        LlmBackend::Anthropic {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::Google {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::XAi {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::Qwen {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::DeepSeek {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::GLM {
                            api_key,
                            endpoint,
                            model,
                        }
                        | LlmBackend::MiniMax {
                            api_key,
                            endpoint,
                            model,
                        } => (api_key.clone(), endpoint.clone(), model.clone()),
                        // This is unreachable since we've excluded Ollama and OpenAi above
                        _ => unreachable!("Unexpected LLM backend type"),
                    };
                    let timeout = std::env::var("OPENAI_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(60);
                    match CloudRuntime::new(
                        CloudConfig::custom(&api_key, &endpoint)
                            .with_model(&model)
                            .with_timeout_secs(timeout),
                    ) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create cloud runtime for agents");
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

        let executor_config = neomind_agent::ai_agent::AgentExecutorConfig {
            store: self.agents.agent_store.clone(),
            time_series_storage: time_series_store,
            device_service: Some(self.devices.service.clone()),
            extension_registry: Some(self.extensions.registry.clone()),
            event_bus: self.core.event_bus.clone(),
            message_manager: Some(self.core.message_manager.clone()),
            llm_runtime,
            llm_backend_store,
        };

        let manager = neomind_agent::ai_agent::AiAgentManager::new(executor_config)
            .await
            .map_err(|e| {
                crate::models::ErrorResponse::internal(format!(
                    "Failed to create agent manager: {}",
                    e
                ))
            })?;

        *mgr_guard = Some(manager.clone());

        tracing::info!(has_llm, has_time_series, "AI Agent manager initialized");
        Ok(manager)
    }

    /// Start the AI Agent manager scheduler.
    pub async fn start_agent_manager(&self) -> Result<(), crate::models::ErrorResponse> {
        let manager = self.get_or_init_agent_manager().await?;
        manager.start().await.map_err(|e| {
            crate::models::ErrorResponse::internal(format!("Failed to start agent manager: {}", e))
        })?;
        tracing::info!("AI Agent manager scheduler started");
        Ok(())
    }

    /// Initialize AI Agent event listener.
    ///
    /// Starts a background task that listens for device events and triggers
    /// event-scheduled agents.
    pub async fn init_agent_events(&self) {
        // Prevent duplicate initialization
        if self
            .agent_events_initialized
            .fetch_or(true, std::sync::atomic::Ordering::Relaxed)
        {
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

        let event_bus = match &self.core.event_bus {
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
            if let Ok(agents) = store
                .query_agents(neomind_storage::AgentFilter {
                    status: Some(neomind_storage::AgentStatus::Executing),
                    ..Default::default()
                })
                .await
            {
                let now = chrono::Utc::now().timestamp();
                for agent in agents {
                    // Check if agent has been executing for more than 10 minutes
                    if let Some(last_exec) = agent.last_execution_at {
                        let exec_duration_secs = now - last_exec;
                        if exec_duration_secs > 600 {
                            // 10 minutes
                            tracing::warn!(
                                agent_id = %agent.id,
                                agent_name = %agent.name,
                                exec_duration_secs = exec_duration_secs,
                                "Agent stuck in Executing status, resetting to Active"
                            );
                            let _ = store
                                .update_agent_status(
                                    &agent.id,
                                    neomind_storage::AgentStatus::Active,
                                    Some("Execution timeout - status reset".to_string()),
                                )
                                .await;
                        }
                    } else {
                        // No last execution time but status is Executing - reset it
                        tracing::warn!(
                            agent_id = %agent.id,
                            agent_name = %agent.name,
                            "Agent in Executing status with no execution time, resetting to Active"
                        );
                        let _ = store
                            .update_agent_status(
                                &agent.id,
                                neomind_storage::AgentStatus::Active,
                                Some("Invalid executing state - status reset".to_string()),
                            )
                            .await;
                    }
                }
            }
        });

        tokio::spawn(async move {
            let mut rx = event_bus.subscribe();
            tracing::info!("Agent event listener started - monitoring for event-triggered agents");

            while let Some((event, _metadata)) = rx.recv().await {
                // Check if any agent should be triggered by this event
                if let neomind_core::NeoMindEvent::DeviceMetric {
                    device_id,
                    metric,
                    value,
                    timestamp: _,
                    quality: _,
                } = event
                {
                    // Trigger agents that have this device/metric in their event filter
                    if let Err(e) = executor
                        .check_and_trigger_event(device_id, &metric, &value)
                        .await
                    {
                        tracing::debug!("No agent triggered for event: {}", e);
                    }
                }
            }
        });
    }
}

// Note: Default implementation removed because ServerState::new() is now async
// to support persistent device registry initialization.
