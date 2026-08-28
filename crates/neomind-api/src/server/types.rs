//! Server state and types.

use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;

pub type CredentialValidator = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// In-memory cache for MQTT credentials (I5: avoid redb hit on every CONNECT).
///
/// Stores user credentials (username -> bcrypt hash) and the system password
/// so the auth hook can validate without touching redb.
#[cfg(feature = "embedded-broker")]
#[derive(Default)]
pub struct CredentialCache {
    /// User credentials: username -> bcrypt password hash.
    pub users: std::collections::HashMap<String, String>,
    /// System password for `__neomind_internal__` (plaintext, never logged).
    pub system_password: Option<String>,
}

#[cfg(feature = "embedded-broker")]
impl std::fmt::Debug for CredentialCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialCache")
            .field("users", &format!("{} entries", self.users.len()))
            .field(
                "system_password",
                &self.system_password.as_ref().map(|_| "***REDACTED***"),
            )
            .finish()
    }
}

#[cfg(feature = "embedded-broker")]
impl CredentialCache {
    /// Load all credentials from redb into the cache.
    pub fn load_from_store(store: &neomind_storage::SettingsStore) -> Result<Self, String> {
        let mut cache = Self::default();

        // Load system credential
        match store.get_system_mqtt_credential() {
            Ok(Some(pass)) => cache.system_password = Some(pass),
            Ok(None) => tracing::warn!("Credential cache: no system credential found"),
            Err(e) => tracing::error!("Credential cache: failed to load system credential: {}", e),
        }

        // Load user credentials
        match store.list_mqtt_credentials() {
            Ok(creds) => {
                for cred in &creds {
                    cache
                        .users
                        .insert(cred.username.clone(), cred.password_hash.clone());
                }
                tracing::info!(
                    "Credential cache: loaded {} user credentials",
                    cache.users.len()
                );
            }
            Err(e) => {
                return Err(format!("Failed to load user credentials: {}", e));
            }
        }

        Ok(cache)
    }
}

use neomind_agent::SessionManager;
use neomind_core::{extension::ExtensionRegistry, EventBus};
use neomind_devices::adapter::AdapterResult;
use neomind_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};
use neomind_rules::{
    device_integration::DeviceActionExecutor, device_status_emitter::DeviceStatusEmitter,
    extension_integration::ExtensionActionExecutor, store::RuleStore, RuleEngine,
    UnifiedValueProvider,
};
use neomind_storage::dashboards::DashboardStore;
use neomind_storage::frontend_components::FrontendComponentStore;
use neomind_storage::instances::InstanceStore;
use neomind_storage::llm_backends::LlmBackendStore;

use crate::automation::{
    store::SharedAutomationStore, transform::TransformEngine, AutoOnboardManager,
};

use neomind_data_push::PushManager;
use neomind_messages::MessageManager;

use crate::auth::AuthState as ApiKeyAuthState;
use crate::auth_users::AuthUserState;
use crate::config::LlmSettingsRequest;
use crate::rate_limit::{RateLimitConfig, RateLimiter};
use crate::server::state::{
    AgentManager, AgentState, AuthState, AutomationState, CoreState, DeviceState,
    ExtensionMetricsStorage, ExtensionRegistryAdapter, ExtensionState, ExtensionStore,
};

#[cfg(feature = "embedded-broker")]
use neomind_devices::{EmbeddedBroker, TopicResolverFn};

/// Maximum request body size (10 MB)
pub const MAX_REQUEST_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Generate a fresh per-process random secret used to authenticate the
/// internal share-proxy bypass. Returns a 43-char base64-encoded string
/// (32 bytes of entropy). Regenerated on every server startup, so leaked
/// secrets from one process do not transfer to another.
fn generate_internal_proxy_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    // Hex encoding avoids extra base64 dependencies.
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Maximum request body size for extension uploads (512 MB - accommodates large
/// ML model bundles, e.g. paddle-ocr-v6 with CUDA ORT libs + multi-tier ONNX models)
pub const MAX_EXTENSION_UPLOAD_SIZE: usize = 512 * 1024 * 1024;

/// Maximum size of an extension package downloaded from the marketplace
/// (1 GB). Higher than the upload limit because the download path streams
/// the body to a temp file (no in-memory buffering), and marketplace
/// packages may bundle large ML models. Enforced via Content-Length (when
/// reported) plus a running byte counter during streaming.
pub const MAX_EXTENSION_DOWNLOAD_SIZE: u64 = 1024 * 1024 * 1024;

/// Server state shared across all handlers.
///
/// Organized into logical sub-states for better maintainability.
#[derive(Clone)]
pub struct ServerState {
    /// Core system services (EventBus, MessageManager)
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

    /// Data directory for persistent storage (e.g. skills, extensions).
    pub data_dir: std::path::PathBuf,

    /// Data push manager (lazy-initialized).
    pub data_push: Arc<tokio::sync::RwLock<Option<PushManager>>>,

    /// Auto-onboarding manager for zero-config device discovery (lazy-initialized).
    pub auto_onboard_manager: Arc<tokio::sync::RwLock<Option<Arc<AutoOnboardManager>>>>,

    /// Dashboard store for visual dashboard persistence.
    pub dashboard_store: Arc<DashboardStore>,

    /// Instance store for remote backend instance management.
    pub instance_store: Arc<InstanceStore>,

    /// Frontend component store for community dashboard components.
    pub frontend_component_store: FrontendComponentStore,

    /// Server start timestamp.
    pub started_at: i64,

    /// Cached GPU information (detected once at startup).
    pub gpu_info: Arc<std::sync::OnceLock<Vec<crate::handlers::stats::GpuInfo>>>,

    /// Flag to track if agent events have been initialized (prevents duplicate subscribers).
    agent_events_initialized: Arc<std::sync::atomic::AtomicBool>,

    /// Flag to track if rule engine events have been initialized (prevents duplicate subscribers).
    rule_engine_events_initialized: Arc<std::sync::atomic::AtomicBool>,

    /// Flag to track if extension event subscription has been initialized (prevents duplicate subscribers).
    extension_event_subscription_initialized: Arc<std::sync::atomic::AtomicBool>,

    /// Cached extension event subscription service instance (prevents duplicate instances).
    extension_event_subscription_service:
        Arc<tokio::sync::Mutex<Option<neomind_core::extension::ExtensionEventSubscriptionService>>>,

    /// Semaphore to limit concurrent telemetry DB queries (max 16).
    pub telemetry_query_semaphore: Arc<tokio::sync::Semaphore>,

    /// Mutex to prevent concurrent broker restarts (C2 race condition fix).
    #[cfg(feature = "embedded-broker")]
    broker_restart_lock: Arc<tokio::sync::Mutex<()>>,

    /// In-memory credential cache for fast auth validation (I5: avoids redb on every CONNECT).
    #[cfg(feature = "embedded-broker")]
    pub credential_cache: Arc<std::sync::RwLock<CredentialCache>>,

    /// Random secret generated at startup, required to use the
    /// `x-internal-proxy: share` auth bypass. Without this check, any network
    /// client (server binds 0.0.0.0 by default) could spoof the header and
    /// bypass authentication as `share-proxy` User. The share dashboard proxy
    /// sets BOTH headers when forwarding via loopback reqwest; external
    /// attackers cannot know this per-process random secret.
    pub internal_proxy_secret: Arc<String>,

    /// IM bridge router (lazy-initialized by `start_im_router` at startup).
    /// Mirrors `agents.agent_manager`'s `Arc<RwLock<Option<…>>>` lazy-init
    /// pattern, but lives on `ServerState` directly because the IM bridge is a
    /// cross-cutting concern (events + agents + messages), not agent-only.
    pub im_router:
        Arc<tokio::sync::RwLock<Option<Arc<neomind_messages::im_bridge::router::ImRouter>>>>,
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

    /// Get transform engine (backward compatibility).
    pub fn transform_engine(&self) -> Option<Arc<TransformEngine>> {
        self.automation.transform_engine.clone()
    }

    /// Get embedded broker.
    #[cfg(feature = "embedded-broker")]
    pub fn embedded_broker(&self) -> Option<Arc<EmbeddedBroker>> {
        self.devices.embedded_broker.read().unwrap().clone()
    }

    /// Restart the embedded MQTT broker and internal adapter with updated config.
    ///
    /// Called by the broker config API after saving new settings to redb.
    /// Stops the running broker (abort), restarts it with the new config, then
    /// recreates the internal-mqtt adapter so it picks up new TLS/auth settings.
    #[cfg(feature = "embedded-broker")]
    pub async fn restart_embedded_broker(&self) -> Result<(), String> {
        use crate::config::{get_embedded_broker_config, open_settings_store};
        use neomind_devices::adapter::DeviceAdapter;
        use neomind_devices::adapters::{create_adapter, mqtt::MqttAdapterConfig};

        // Acquire restart lock to prevent concurrent restarts (C2 race condition fix).
        // If another restart is in progress, this waits until it completes.
        let _guard = self.broker_restart_lock.lock().await;

        let broker_config = get_embedded_broker_config();

        // 1. Stop existing broker — rmqtt abort is instant, port released immediately
        let old_broker_config = {
            let old_broker = self.devices.embedded_broker.read().unwrap().clone();
            if let Some(ref broker) = old_broker {
                if broker.is_running() {
                    tracing::info!("Stopping embedded broker for config change...");
                    broker
                        .stop()
                        .await
                        .map_err(|e| format!("Broker stop failed: {}", e))?;
                }
            }
            let config = old_broker.as_ref().map(|b| b.config());
            *self.devices.embedded_broker.write().unwrap() = None;
            config
        };

        // 2. Stop existing internal-mqtt adapter
        if let Some(adapter) = self.devices.service.get_adapter("internal-mqtt").await {
            tracing::info!("Stopping internal-mqtt adapter for config change...");
            if let Err(e) = adapter.stop().await {
                tracing::warn!("Adapter stop error: {}", e);
            }
            self.devices
                .service
                .unregister_adapter("internal-mqtt")
                .await;
        }

        // 3. Refresh credential cache and create new broker with cache-backed validator
        if let Ok(store) = crate::config::open_settings_store() {
            match CredentialCache::load_from_store(&store) {
                Ok(cache) => {
                    *self.credential_cache.write().unwrap() = cache;
                }
                Err(e) => {
                    tracing::error!("Failed to refresh credential cache during restart: {}", e)
                }
            }
        }

        let cache = self.credential_cache.clone();
        let credential_validator: CredentialValidator =
            std::sync::Arc::new(move |username: &str, password: &str| {
                let cache = cache.read().unwrap();

                if username == "__neomind_internal__" {
                    if let Some(ref system_pass) = cache.system_password {
                        return password == system_pass;
                    }
                    tracing::warn!("Credential validator: no system credential in cache");
                    return false;
                }

                if let Some(hash) = cache.users.get(username) {
                    return bcrypt::verify(password, hash).unwrap_or(false);
                }

                tracing::debug!(
                    "Credential validator: user '{}' not found in cache",
                    username
                );
                false
            });

        let broker = EmbeddedBroker::new(broker_config.clone(), credential_validator);
        if let Some(bus) = self.core.event_bus.as_ref() {
            broker.set_event_bus(bus.clone());
        }
        if let Some(resolver) = self.build_topic_resolver() {
            broker.set_topic_resolver(resolver);
        }

        if let Err(e) = broker.start().await {
            // Attempt rollback: restart with the old config and rebuild adapter
            if let Some(ref old_cfg) = old_broker_config {
                tracing::warn!(
                    "Broker restart failed ({}), attempting rollback with previous config...",
                    e
                );
                let rollback_cache = self.credential_cache.clone();
                let rollback_validator: CredentialValidator =
                    std::sync::Arc::new(move |username: &str, password: &str| {
                        let cache = rollback_cache.read().unwrap();
                        if username == "__neomind_internal__" {
                            return cache.system_password.as_deref() == Some(password);
                        }
                        cache
                            .users
                            .get(username)
                            .is_some_and(|hash| bcrypt::verify(password, hash).unwrap_or(false))
                    });
                let rollback_broker = EmbeddedBroker::new(old_cfg.clone(), rollback_validator);
                if let Some(resolver) = self.build_topic_resolver() {
                    rollback_broker.set_topic_resolver(resolver);
                }
                if let Some(bus) = self.core.event_bus.as_ref() {
                    rollback_broker.set_event_bus(bus.clone());
                }
                if let Ok(()) = rollback_broker.start().await {
                    *self.devices.embedded_broker.write().unwrap() =
                        Some(Arc::new(rollback_broker));
                    tracing::info!("Rollback successful: broker restarted with previous config");

                    // Also rebuild the internal-mqtt adapter with old config
                    // Extract adapter password before any async operations (store is not Send)
                    let rollback_adapter_pass = crate::config::open_settings_store()
                        .ok()
                        .and_then(|s| s.get_system_mqtt_credential().ok().flatten());
                    {
                        let adapter_pass = rollback_adapter_pass;
                        let rollback_mqtt_config = MqttAdapterConfig {
                            name: "internal-mqtt".to_string(),
                            mqtt: neomind_devices::mqtt::MqttConfig {
                                broker: "127.0.0.1".to_string(),
                                port: old_cfg.port,
                                client_id: Some("neomind-internal".to_string()),
                                username: adapter_pass
                                    .as_ref()
                                    .map(|_| "__neomind_internal__".to_string()),
                                password: adapter_pass,
                                tls: old_cfg.tls_enabled,
                                ca_cert: old_cfg.tls_ca_path.clone(),
                                client_cert: None,
                                client_key: None,
                                keep_alive: 60,
                                clean_session: true,
                                qos: 1,
                                topic_prefix: "device".to_string(),
                                command_topic: "downlink".to_string(),
                            },
                            subscribe_topics: vec!["#".to_string()],
                            discovery_topic: Some("device/+/+/uplink".to_string()),
                            discovery_prefix: "device".to_string(),
                            auto_discovery: true,
                            device_id_field: old_cfg.device_id_field.clone(),
                            storage_dir: Some("data".to_string()),
                        };
                        if let Some(event_bus) = self.core.event_bus.as_ref() {
                            if let Ok(val) = serde_json::to_value(&rollback_mqtt_config) {
                                if let Ok(adapter) = create_adapter("mqtt", &val, event_bus) {
                                    adapter.set_telemetry_storage(self.devices.telemetry.clone());
                                    self.devices
                                        .service
                                        .register_adapter(
                                            "internal-mqtt".to_string(),
                                            adapter.clone(),
                                        )
                                        .await;
                                    if let Err(e) = adapter.start().await {
                                        tracing::warn!("Rollback adapter start failed: {}", e);
                                    } else {
                                        tracing::info!("Rollback: internal-mqtt adapter restarted");
                                    }
                                }
                            }
                        }
                    }
                } else {
                    tracing::error!("Rollback also failed — system has no running broker");
                }
            }
            return Err(format!("Failed to start broker: {}", e));
        }
        tracing::info!(
            "Embedded broker restarted: listen={}, port={}, auth={}, tls={}",
            broker_config.listen,
            broker_config.port,
            broker_config.auth_enabled,
            broker_config.tls_enabled
        );

        *self.devices.embedded_broker.write().unwrap() = Some(Arc::new(broker));

        // 4. Create new internal-mqtt adapter with updated config
        let (adapter_username, adapter_password) = {
            if let Ok(store) = open_settings_store() {
                match store.get_system_mqtt_credential() {
                    Ok(Some(pass)) => (Some("__neomind_internal__".to_string()), Some(pass)),
                    _ => {
                        tracing::warn!(
                            "No system credential found for adapter, connecting without auth"
                        );
                        (None, None)
                    }
                }
            } else {
                (None, None)
            }
        };

        let mqtt_config = MqttAdapterConfig {
            name: "internal-mqtt".to_string(),
            mqtt: neomind_devices::mqtt::MqttConfig {
                broker: "127.0.0.1".to_string(),
                port: broker_config.port,
                client_id: Some("neomind-internal".to_string()),
                username: adapter_username,
                password: adapter_password,
                tls: broker_config.tls_enabled,
                ca_cert: broker_config.tls_ca_path.clone(),
                // One-way TLS: client only needs CA to verify server, no client cert/key
                client_cert: None,
                client_key: None,
                keep_alive: 60,
                clean_session: true,
                qos: 1,
                topic_prefix: "device".to_string(),
                command_topic: "downlink".to_string(),
            },
            subscribe_topics: vec!["#".to_string()],
            discovery_topic: Some("device/+/+/uplink".to_string()),
            discovery_prefix: "device".to_string(),
            auto_discovery: true,
            device_id_field: broker_config.device_id_field.clone(),
            storage_dir: Some("data".to_string()),
        };

        let Some(event_bus) = self.core.event_bus.as_ref() else {
            return Err("EventBus not initialized".to_string());
        };

        let mqtt_config_value = serde_json::to_value(&mqtt_config)
            .map_err(|e| format!("Failed to serialize MQTT config: {}", e))?;

        let mqtt_adapter: Arc<dyn DeviceAdapter> = {
            create_adapter("mqtt", &mqtt_config_value, event_bus)
                .map_err(|e| format!("Failed to create MQTT adapter: {}", e))?
        };

        mqtt_adapter.set_telemetry_storage(self.devices.telemetry.clone());

        if let Some(mqtt) = mqtt_adapter
            .as_any()
            .downcast_ref::<neomind_devices::adapters::mqtt::MqttAdapter>()
        {
            mqtt.set_shared_device_registry(self.devices.service.get_registry())
                .await;
            mqtt.set_data_dir(self.data_dir.clone());
        }

        self.devices
            .service
            .register_adapter("internal-mqtt".to_string(), mqtt_adapter.clone())
            .await;

        if let Err(e) = mqtt_adapter.start().await {
            tracing::warn!("Failed to start internal-mqtt adapter: {}", e);
        } else {
            tracing::info!("Internal-mqtt adapter restarted successfully");
        }

        Ok(())
    }

    /// Get event bus (backward compatibility).
    pub fn event_bus(&self) -> Option<Arc<EventBus>> {
        self.core.event_bus.clone()
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

    /// Build a topic-to-device-id resolver suitable for
    /// `EmbeddedBroker::set_topic_resolver`. The resolver does an exact-match
    /// lookup against `DeviceRegistry::find_device_by_telemetry_topic`.
    /// Returns `None` if the registry is the empty in-memory fallback (so
    /// the broker skips the `MessagePublish` hook entirely).
    fn build_topic_resolver(&self) -> Option<TopicResolverFn> {
        let registry = self.devices.registry.clone();
        // Optimization: skip resolver when registry is empty (e.g. tests /
        // first-run before any device is registered) to avoid registering
        // a no-op publish hook.
        if registry.list_devices().is_empty() {
            return None;
        }
        Some(Arc::new(move |topic: &str| {
            registry
                .find_device_by_telemetry_topic(topic)
                .map(|(id, _)| id)
        }))
    }

    /// Get device service (backward compatibility).
    pub fn device_service(&self) -> Arc<DeviceService> {
        self.devices.service.clone()
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
        let value_provider = Arc::new(UnifiedValueProvider::new());

        // Ensure data directory exists
        if let Err(e) = std::fs::create_dir_all("data") {
            tracing::warn!(category = "storage", error = %e, "Failed to create data directory");
        }

        // ========== Parallel store opens (spawn_blocking for concurrent I/O) ==========
        // Open independent stores concurrently while we build in-memory state.
        // Handles are awaited later when results are needed.
        let t_stores = std::time::Instant::now();

        let rule_store_h = tokio::task::spawn_blocking(|| {
            match RuleStore::open("data/rules.redb") {
                Ok(store) => {
                    tracing::info!("Rule store initialized at data/rules.redb");
                    Some(store)
                }
                Err(e) => {
                    tracing::warn!(category = "storage", error = %e, "Failed to open rule store, rules will not be persisted");
                    None
                }
            }
        });

        let agent_store_h = tokio::task::spawn_blocking(
            || match neomind_storage::AgentStore::open("data/agents.redb") {
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
            },
        );

        let dashboard_store_h = tokio::task::spawn_blocking(|| {
            match DashboardStore::open("data/dashboards.redb") {
                Ok(store) => store,
                Err(_e) => {
                    DashboardStore::memory().unwrap_or_else(|e| {
                        tracing::error!(category = "storage", error = %e, "Failed to create in-memory dashboard store");
                        std::process::exit(1);
                    })
                }
            }
        });

        let instance_store_h = tokio::task::spawn_blocking(|| {
            match InstanceStore::open("data/instances.redb") {
                Ok(store) => store,
                Err(e) => {
                    tracing::error!(category = "storage", error = %e, "Failed to open instance store");
                    InstanceStore::memory().unwrap_or_else(|e| {
                        tracing::error!(category = "storage", error = %e, "Failed to create in-memory instance store");
                        std::process::exit(1);
                    })
                }
            }
        });

        let session_manager_h = tokio::task::spawn_blocking(|| {
            SessionManager::new().unwrap_or_else(|e| {
                tracing::warn!(category = "storage", error = %e, "Failed to create persistent SessionManager, using in-memory");
                SessionManager::memory()
            })
        });

        let data_dir = std::path::PathBuf::from(
            std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string()),
        );
        let frontend_component_store_h = tokio::task::spawn_blocking({
            let dir = data_dir.join("frontend-components");
            move || {
                FrontendComponentStore::open(dir).expect("Failed to init frontend component store")
            }
        });

        // ========== Build CORE STATE ==========
        // Create event bus FIRST (needed for adapters to publish events)
        let event_bus = Some(Arc::new(EventBus::new()));

        // Create message manager with persistent storage
        let message_manager = match MessageManager::with_storage("data") {
            Ok(manager) => {
                tracing::info!("Message store initialized at data/messages.redb");
                Arc::new(manager)
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to open message store, using in-memory");
                Arc::new(MessageManager::new())
            }
        };
        // Load persisted channel configurations
        message_manager.load_persisted_channels().await;
        message_manager.register_default_channels().await;

        let core = CoreState::new(event_bus.clone(), message_manager.clone());

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

        // Create time series storage — start with an in-memory placeholder and
        // load the persistent database in the background so that a large
        // telemetry.redb does not block server startup.
        let time_series_storage =
            Arc::new(TimeSeriesStorage::memory().expect("in-memory telemetry storage"));
        let telemetry_for_bg = time_series_storage.clone();
        let telemetry_path = std::path::Path::new("data").join("telemetry.redb");
        tokio::spawn(async move {
            let t = tokio::task::spawn_blocking(move || {
                let start = std::time::Instant::now();
                match TimeSeriesStorage::open(&telemetry_path) {
                    Ok(s) => {
                        tracing::info!(
                            "Time series storage initialized at {:?} in {:.1}s",
                            telemetry_path,
                            start.elapsed().as_secs_f64()
                        );
                        Some(s)
                    }
                    Err(e) => {
                        tracing::warn!(
                            category = "storage",
                            error = %e,
                            "Failed to open telemetry storage at {:?}, keeping in-memory",
                            telemetry_path
                        );
                        None
                    }
                }
            })
            .await
            .expect("telemetry open task panicked");

            if let Some(persistent) = t {
                let inner = persistent.inner_store();

                // Migrate legacy bare device_id keys to unified "device:" prefix format.
                // Must run here — after the DB is opened, before swap_store makes it live.
                match inner.migrate_device_prefix() {
                    Ok(count) => {
                        tracing::info!(
                            "Time-series key migration completed: {} keys migrated",
                            count
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Time-series key migration failed: {}", e);
                    }
                }

                telemetry_for_bg.swap_store(inner);
                tracing::info!("Persistent telemetry storage swapped in");
            }
        });

        // Create device service
        let event_bus_for_service = (**event_bus
            .as_ref()
            .expect("event_bus initialized during startup"))
        .clone();
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
        // Use NEOMIND_DATA_DIR if set, otherwise use relative path
        // NOTE: Only use data/extensions for consistent behavior
        // Extensions should be installed via frontend upload (.nep packages)
        // Development builds should also output to data/extensions/
        let extensions_dir = if let Ok(data_dir) = std::env::var("NEOMIND_DATA_DIR") {
            std::path::PathBuf::from(data_dir).join("extensions")
        } else {
            std::path::PathBuf::from("data/extensions")
        };

        let default_ext_dirs = vec![extensions_dir];

        let mut registry_builder = ExtensionRegistry::new();
        for ext_dir in &default_ext_dirs {
            if ext_dir.exists() {
                registry_builder.add_extension_dir(ext_dir.clone());
                tracing::info!("Added extension discovery directory: {:?}", ext_dir);
            }
        }
        // Set event bus on registry for lifecycle events
        if let Some(ref bus) = event_bus {
            registry_builder.set_event_bus(bus.clone());
        }
        let extension_registry = Arc::new(registry_builder);

        // Create extension metrics storage (shares device telemetry.redb)
        let extension_metrics_storage = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            time_series_storage.clone(),
        ));

        // Open extension store (singleton-cached internally)
        let extension_store = ExtensionStore::open("data/extensions.redb")
            .expect("Failed to open extension store — ensure data/ directory exists");

        // Create the extension state with registry, storage, and persistent store
        let extensions = ExtensionState::new(
            extension_registry.clone(),
            extension_metrics_storage,
            extension_store,
        );

        tracing::info!("Extension state initialized");

        // Set up extension command router so DeviceService can route commands to extensions
        {
            let ext_registry = extension_registry.clone();
            let router: neomind_devices::ExtensionCommandRouterFn = Arc::new(
                move |extension_id: String,
                      device_id: String,
                      command_name: String,
                      params: std::collections::HashMap<String, serde_json::Value>| {
                    let ext_registry = ext_registry.clone();
                    Box::pin(async move {
                        // Flatten params into top-level args so extension handlers can find them directly
                        let mut args_map = serde_json::Map::new();
                        args_map.insert("device_id".into(), serde_json::json!(device_id));
                        for (k, v) in params {
                            args_map.insert(k, v);
                        }
                        let args = serde_json::Value::Object(args_map);
                        ext_registry
                            .execute_command(&extension_id, &command_name, &args)
                            .await
                            .map_err(|e| format!("Extension command failed: {}", e))?;
                        Ok(())
                    })
                },
            );
            devices.service.set_extension_command_router(router).await;
        }

        // ========== Build AUTOMATION STATE ==========
        let rule_engine =
            Arc::new(RuleEngine::new(value_provider.clone()).with_event_bus(event_bus.clone()));

        // Set up capability provider for isolated extensions
        // This allows isolated extensions to invoke capabilities on the host process
        //
        // The SessionManager isn't built yet at this point in `new()`, so we
        // create a late-binding holder that gets populated after `agents`
        // construction below. ChatStreamCapabilityProvider reads from this
        // holder at invocation time.
        let session_manager_holder: crate::capability_providers::SessionManagerHolder =
            Arc::new(tokio::sync::RwLock::new(None));
        {
            use crate::capability_providers::CompositeCapabilityProvider;
            use neomind_core::extension::CapabilityServices;

            let services = CapabilityServices::new()
                .with_service(
                    neomind_core::extension::keys::DEVICE_SERVICE,
                    devices.service.clone(),
                )
                .with_service(
                    neomind_core::extension::keys::TELEMETRY_STORAGE,
                    devices.telemetry.clone(),
                )
                .with_service(
                    neomind_core::extension::keys::RULE_ENGINE,
                    rule_engine.clone(),
                )
                .with_service(
                    neomind_core::extension::keys::EXTENSION_REGISTRY,
                    extensions.registry.clone(),
                )
                .with_service(
                    neomind_core::extension::keys::EVENT_BUS,
                    event_bus
                        .clone()
                        .unwrap_or_else(|| Arc::new(neomind_core::EventBus::new())),
                );

            let event_dispatcher = extensions.get_event_dispatcher();
            let composite_provider = Arc::new(CompositeCapabilityProvider::with_all_providers(
                services,
                event_bus
                    .clone()
                    .unwrap_or_else(|| Arc::new(neomind_core::EventBus::new())),
                event_dispatcher,
                session_manager_holder.clone(),
            ));

            extensions.set_capability_provider(composite_provider).await;
            tracing::info!("Capability provider set for isolated extensions");
        }

        // Wire rule engine to message manager
        rule_engine
            .set_message_manager(core.message_manager.clone())
            .await;

        // Wire rule engine to device service
        let device_service_for_action = devices.service.clone();
        let device_action_executor = Arc::new(DeviceActionExecutor::with_device_service(
            device_service_for_action,
        ));
        rule_engine
            .set_device_action_executor(device_action_executor)
            .await;

        // Wire rule engine to extension registry for extension command execution
        let extension_registry_adapter =
            Arc::new(ExtensionRegistryAdapter::new(extensions.runtime.clone()));
        let extension_action_executor =
            Arc::new(ExtensionActionExecutor::new(extension_registry_adapter));
        rule_engine
            .set_extension_action_executor(extension_action_executor)
            .await;

        // Wire event bus to message manager
        if let Some(ref bus) = event_bus {
            core.message_manager.set_event_bus(bus.clone()).await;
        }

        // Await parallel-opened rule store
        let rule_store = rule_store_h.await.expect("rule_store task panicked");

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

            // Clean up execution history older than 30 days to prevent unbounded growth
            match store.cleanup_history(30) {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "Cleaned up old rule execution history (>30 days)");
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(category = "storage", error = %e, "Failed to clean up rule history");
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

        // Create transform engine with extension registry and automation store support
        let transform_engine = {
            let mut engine = TransformEngine::with_extension_registry(extensions.registry.clone());
            // Persistent telemetry history — enables TimeSeriesAggregation
            // over real data (the old in-RAM cache was never populated).
            engine = engine.with_time_series_storage(time_series_storage.clone());
            if let Some(ref store) = automation_store {
                engine = engine.with_automation_store(store.clone());
            }
            Some(Arc::new(engine))
        };
        tracing::info!("Transform engine initialized with extension registry");

        // Spawn DeviceStatusEmitter — 60s tick that refreshes the
        // __last_seen_age_secs virtual metric for devices referenced by rules.
        let device_status_emitter = std::sync::Arc::new(DeviceStatusEmitter::new(
            rule_engine.clone(),
            value_provider.clone(),
            devices.service.clone(),
        ));
        let _device_status_emitter_handle = device_status_emitter.start();
        tracing::info!("DeviceStatusEmitter spawned (60s tick for __last_seen_age_secs)");

        let automation = AutomationState::new(
            value_provider,
            rule_engine,
            rule_store,
            automation_store,
            transform_engine,
        );

        // ========== Build AGENT STATE ==========
        // Await parallel-opened session manager
        let session_manager = session_manager_h
            .await
            .expect("session_manager task panicked");

        // Await parallel-opened agent store
        let agent_store = agent_store_h.await.expect("agent_store task panicked");

        // Initialize system memory store (Markdown-based persistent memory)
        let system_memory_store =
            Arc::new(neomind_storage::MarkdownMemoryStore::new("data/memory"));
        if let Err(e) = system_memory_store.init() {
            tracing::warn!(category = "storage", error = %e, "Failed to initialize system memory store");
        }

        let agents = AgentState::new(
            Arc::new(session_manager),
            agent_store,
            Arc::new(tokio::sync::RwLock::new(None)),
            system_memory_store,
        );

        // Late-bind SessionManager into the ChatStream capability holder so
        // ChatStream invocations from isolated extensions can resolve it.
        *session_manager_holder.write().await = Some(agents.session_manager.clone());

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

        // ========== GPU info: lazy — populated on first /api/stats request ==========
        let gpu_info: Arc<std::sync::OnceLock<Vec<crate::handlers::stats::GpuInfo>>> =
            Arc::new(std::sync::OnceLock::new());

        // Await parallel-opened stores
        let dashboard_store = dashboard_store_h
            .await
            .expect("dashboard_store task panicked");
        let instance_store = instance_store_h
            .await
            .expect("instance_store task panicked");
        let frontend_component_store = frontend_component_store_h
            .await
            .expect("frontend_component_store task panicked");

        tracing::info!(
            elapsed_ms = t_stores.elapsed().as_millis() as u64,
            "All parallel store opens completed"
        );

        let data_push_telemetry = devices.telemetry.clone();

        // Spawn periodic old message cleanup (every 6 hours)
        {
            let mm = core.message_manager.clone();
            tokio::spawn(async move {
                let mut cleanup_interval =
                    tokio::time::interval(tokio::time::Duration::from_secs(6 * 60 * 60));
                // Consume the immediate first tick: tokio intervals fire at
                // once, which used to trigger a full messages scan during
                // startup (the agent-execution task below does the same).
                cleanup_interval.tick().await;
                loop {
                    cleanup_interval.tick().await;
                    if let Ok(cleaned_msgs) = mm.cleanup_old(30).await {
                        if cleaned_msgs > 0 {
                            tracing::info!(
                                "Periodic cleanup: removed {} messages older than 30 days",
                                cleaned_msgs
                            );
                        }
                    }
                }
            });
        }

        // Spawn periodic old agent-execution cleanup (every 6 hours, >30 days).
        // cleanup_executions exists on the store but was never wired to a
        // scheduler, so execution history (input/output/journal per run) grew
        // agents.redb without bound.
        {
            let agent_store = agents.agent_store.clone();
            tokio::spawn(async move {
                let mut cleanup_interval =
                    tokio::time::interval(tokio::time::Duration::from_secs(6 * 60 * 60));
                // `interval`'s first tick fires immediately — consume it so the
                // server doesn't do a full AGENT_EXECUTIONS_TABLE scan
                // (deserializing every record) during startup, contending with
                // the just-booted server against the very backlog this cleanup
                // exists to clear. First real cleanup is delayed one interval.
                cleanup_interval.tick().await;
                loop {
                    cleanup_interval.tick().await;
                    let cutoff = chrono::Utc::now().timestamp() - 30 * 24 * 60 * 60;
                    match agent_store.cleanup_executions(cutoff).await {
                        Ok(cleaned) => {
                            if cleaned > 0 {
                                tracing::info!(
                                    "Periodic cleanup: removed {} agent executions older than 30 days",
                                    cleaned
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Periodic agent-execution cleanup failed — execution history may grow unbounded"
                            );
                        }
                    }
                }
            });
        }

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
            instance_store,
            frontend_component_store,
            started_at,
            gpu_info,
            agent_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            extension_event_subscription_initialized: Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            extension_event_subscription_service: Arc::new(tokio::sync::Mutex::new(None)),
            telemetry_query_semaphore: Arc::new(tokio::sync::Semaphore::new(16)),
            data_dir,
            data_push: {
                let push_manager = match PushManager::new_with_telemetry(
                    std::path::Path::new("data"),
                    event_bus.clone(),
                    data_push_telemetry,
                ) {
                    Ok(m) => {
                        tracing::info!("Data push manager initialized");
                        Some(m)
                    }
                    Err(e) => {
                        tracing::warn!(category = "storage", error = %e, "Failed to initialize data push manager");
                        None
                    }
                };
                Arc::new(tokio::sync::RwLock::new(push_manager))
            },
            #[cfg(feature = "embedded-broker")]
            broker_restart_lock: Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(feature = "embedded-broker")]
            credential_cache: Arc::new(std::sync::RwLock::new(CredentialCache::default())),
            internal_proxy_secret: Arc::new(generate_internal_proxy_secret()),
            im_router: Arc::new(tokio::sync::RwLock::new(None)),
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
        let value_provider = Arc::new(UnifiedValueProvider::new());

        // ========== Build CORE STATE ==========
        let event_bus = Some(Arc::new(EventBus::new()));

        // In-memory message manager
        let message_manager = Arc::new(MessageManager::new());
        message_manager.register_default_channels().await;

        let core = CoreState::new(event_bus.clone(), message_manager.clone());

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
        let mut registry = ExtensionRegistry::new();
        if let Some(ref bus) = event_bus {
            registry.set_event_bus(bus.clone());
        }
        let extension_registry = Arc::new(registry);
        let extension_metrics_storage = Arc::new(ExtensionMetricsStorage::with_shared_storage(
            time_series_storage.clone(),
        ));
        let extension_store = ExtensionStore::open(":memory:")
            .expect("Failed to open in-memory extension store for testing");
        let extensions = ExtensionState::new(
            extension_registry,
            extension_metrics_storage,
            extension_store,
        );

        // ========== Build AUTOMATION STATE ==========
        let rule_engine =
            Arc::new(RuleEngine::new(value_provider.clone()).with_event_bus(event_bus.clone()));
        rule_engine
            .set_message_manager(core.message_manager.clone())
            .await;

        let device_service_for_action = devices.service.clone();
        let device_action_executor = Arc::new(DeviceActionExecutor::with_device_service(
            device_service_for_action,
        ));
        rule_engine
            .set_device_action_executor(device_action_executor)
            .await;

        let extension_registry_adapter =
            Arc::new(ExtensionRegistryAdapter::new(extensions.runtime.clone()));
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
        let automation = AutomationState::new(
            value_provider,
            rule_engine,
            None, // rule_store - skip for tests
            automation_store,
            transform_engine,
        );

        // ========== Build AGENT STATE ==========
        let session_manager = SessionManager::memory();
        let agent_store = neomind_storage::AgentStore::memory().unwrap();
        let system_memory_store = Arc::new(neomind_storage::MarkdownMemoryStore::new(
            std::env::temp_dir().join("neomind-test-memory"),
        ));

        let agents = AgentState::new(
            Arc::new(session_manager),
            agent_store,
            Arc::new(tokio::sync::RwLock::new(None)),
            system_memory_store,
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
        let instance_store = InstanceStore::memory().unwrap();
        let frontend_component_store = FrontendComponentStore::open(
            std::env::temp_dir().join(format!("neomind-test-fc-{}", uuid::Uuid::new_v4())),
        )
        .expect("Failed to create test frontend component store");

        // Empty GPU info for testing
        let gpu_info = Arc::new(std::sync::OnceLock::from(vec![]));
        let data_push_telemetry = time_series_storage.clone();

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
            instance_store,
            frontend_component_store,
            started_at,
            gpu_info,
            agent_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rule_engine_events_initialized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            extension_event_subscription_initialized: Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            extension_event_subscription_service: Arc::new(tokio::sync::Mutex::new(None)),
            telemetry_query_semaphore: Arc::new(tokio::sync::Semaphore::new(16)),
            data_dir: std::path::PathBuf::from("data"),
            data_push: {
                let push_manager = PushManager::memory_with_telemetry(data_push_telemetry).ok();
                Arc::new(tokio::sync::RwLock::new(push_manager))
            },
            #[cfg(feature = "embedded-broker")]
            broker_restart_lock: Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(feature = "embedded-broker")]
            credential_cache: Arc::new(std::sync::RwLock::new(CredentialCache::default())),
            internal_proxy_secret: Arc::new(generate_internal_proxy_secret()),
            im_router: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Initialize device type storage.
    pub async fn init_device_storage(&self) {
        if let Err(e) = tokio::fs::create_dir_all("data").await {
            tracing::error!(category = "storage", error = %e, "Failed to create data directory");
        }

        // Device registry storage is initialized automatically on first use
        tracing::info!(category = "storage", "Data directory created/verified");

        // Seed built-in device type templates via the registry's OWN storage
        // handle (same store the cache reads from), so the subsequent reload
        // is guaranteed to see the seed — no cross-handle visibility race.
        match self.devices.registry.seed_builtin_templates().await {
            Ok(seeded) => {
                if seeded > 0 {
                    tracing::info!(
                        category = "storage",
                        seeded,
                        "Seeded built-in device type templates"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(category = "storage", error = %e, "Failed to seed built-in templates");
            }
        }

        // Reload the in-memory registry cache so freshly-seeded builtin
        // templates (NE101/NE301) become visible. `with_persistence()` loaded
        // the cache at `ServerState::new` before this seed ran; on a fresh
        // boot that cache is empty and without a reload, storage has the
        // templates but the cache stays empty — device registration then
        // fails with "Device type template '...' not found".
        if let Err(e) = self.devices.registry.load_from_storage().await {
            tracing::warn!(
                category = "storage",
                error = %e,
                "Failed to reload device registry cache after builtin seed"
            );
        }
    }

    /// Start enabled data push targets from persistent storage.
    ///
    /// Must be called after the event bus is initialized.
    pub async fn init_data_push_targets(&self) {
        let push_manager = self.data_push.read().await;
        if let Some(manager) = push_manager.as_ref() {
            match manager.start_enabled_targets().await {
                Ok(()) => {
                    tracing::info!(category = "data_push", "Enabled push targets started");
                }
                Err(e) => {
                    tracing::warn!(category = "data_push", error = %e, "Failed to start enabled push targets");
                }
            }
        }
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

        // Spawn background task to sync extension packages
        // This doesn't block server startup
        //
        // Path strategy:
        // - install_dir: $NEOMIND_DATA_DIR/extensions/ (where extensions are unpacked)
        // - nep_cache_dir: $NEOMIND_DATA_DIR/extensions/packages/ (where .nep files are cached)
        //
        // This ensures all extension data is in the app data directory, avoiding
        // path inconsistencies between development and production modes.
        let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
        let install_dir = std::path::PathBuf::from(data_dir.clone()).join("extensions");
        let nep_cache_dir = std::path::PathBuf::from(data_dir)
            .join("extensions")
            .join("packages");

        tracing::info!(
            install_dir = %install_dir.display(),
            nep_cache_dir = %nep_cache_dir.display(),
            "Extension sync paths configured"
        );

        let state_for_sync = self.clone();
        tokio::spawn(async move {
            use crate::server::ExtensionInstallService;

            let install_service = ExtensionInstallService::new(install_dir, nep_cache_dir);

            match install_service.sync_nep_cache().await {
                Ok(report) => {
                    if report.scanned > 0 {
                        tracing::info!(
                            category = "extensions",
                            scanned = report.scanned,
                            installed = report.installed,
                            upgraded = report.upgraded,
                            skipped = report.skipped,
                            "Extension cache sync completed"
                        );
                    }
                    // Register whatever actually landed on disk (the sync used
                    // to stop here, reporting counts while loading nothing).
                    for pkg in &report.installed_packages {
                        if let Err(e) = crate::handlers::extensions::register_installed_package(
                            &state_for_sync,
                            pkg,
                        )
                        .await
                        {
                            tracing::warn!(
                                category = "extensions",
                                extension_id = %pkg.extension_id,
                                error = %e,
                                "Failed to register synced extension (installed on disk only)"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        category = "extensions",
                        error = %e,
                        "Failed to sync extension packages from cache directory"
                    );
                }
            }
        });
    }

    /// Initialize LLM backend using the unified config loader.
    /// Falls back to LlmBackendInstanceManager if no config file is found.
    /// Only sets the default backend for NEW sessions.
    pub async fn init_llm(&self) {
        // Priority: DB active instance FIRST, config file as fallback. The
        // instance manager reflects the user's latest explicit activation
        // (incl. the builtin model); a config-file entry is a static
        // deployment default. File-over-DB used to resurrect a stale
        // config.toml backend after restart on upgraded installs — the user
        // activates builtin, restarts, and every new session silently routes
        // back to the dead endpoint the TOML names (observed on a real
        // 0.9.4→0.9.19 Jetson upgrade).
        match self
            .agents
            .session_manager
            .configure_llm_from_instance_manager()
            .await
        {
            Ok(_) => {
                tracing::info!(
                    category = "ai",
                    "Configured LLM backend successfully from instance manager (active instance)"
                );
            }
            Err(e) => {
                // No active instance in the DB — fall back to the config file.
                if let Some(backend) = crate::config::load_llm_config() {
                    self.agents
                        .session_manager
                        .set_default_llm_backend(backend)
                        .await;
                    tracing::info!(
                        category = "ai",
                        prior_error = %e,
                        "Configured default LLM backend from config file (no active DB instance)"
                    );
                    return;
                }
                tracing::warn!(category = "ai", error = %e, "No LLM backend configured. Set up via Web UI or create config.toml");
            }
        }
    }

    /// Start the embedded MQTT broker early so it's ready for connections.
    /// Called before extension loading so devices can connect sooner.
    /// The internal MQTT adapter is created later in `init_device_adapters`.
    #[cfg(feature = "embedded-broker")]
    pub async fn start_embedded_broker(&self) {
        use crate::config::{get_embedded_broker_config, open_settings_store};
        use std::sync::Arc;

        let broker_config = get_embedded_broker_config();
        let port = broker_config.port;

        // Always ensure system credential exists.
        if let Ok(store) = open_settings_store() {
            if let Ok(None) = store.get_system_mqtt_credential() {
                let system_password = uuid::Uuid::new_v4().to_string().replace("-", "");

                if let Err(e) = store.set_system_mqtt_credential(&system_password) {
                    tracing::error!("Failed to set system MQTT credential: {}", e);
                } else {
                    tracing::info!("Generated system MQTT credential for internal broker");
                }
            }
        }

        // Populate credential cache from redb (one-time load)
        if let Ok(store) = open_settings_store() {
            match CredentialCache::load_from_store(&store) {
                Ok(cache) => {
                    *self.credential_cache.write().unwrap() = cache;
                }
                Err(e) => tracing::error!("Failed to load credential cache: {}", e),
            }
        }

        // Credential validator closure: validates username/password against in-memory cache.
        // No redb access on the hot path — only bcrypt verification.
        let cache = self.credential_cache.clone();
        let credential_validator: CredentialValidator =
            Arc::new(move |username: &str, password: &str| {
                let cache = cache.read().unwrap();

                // System credential check (plaintext comparison)
                if username == "__neomind_internal__" {
                    if let Some(ref system_pass) = cache.system_password {
                        return password == system_pass;
                    }
                    tracing::warn!("Credential validator: no system credential in cache");
                    return false;
                }

                // User credential check (bcrypt, no redb)
                if let Some(hash) = cache.users.get(username) {
                    let result = bcrypt::verify(password, hash).unwrap_or(false);
                    return result;
                }

                tracing::debug!(
                    "Credential validator: user '{}' not found in cache",
                    username
                );
                false
            });

        let broker = EmbeddedBroker::new(broker_config.clone(), credential_validator);
        // Wire up transport-level presence events (DeviceTransportOnline/Offline)
        // so connected-but-idle devices no longer display as "Never Connected".
        if let Some(bus) = self.core.event_bus.as_ref() {
            broker.set_event_bus(bus.clone());
        }
        // Wire up topic resolver so the broker can learn the mapping from
        // MQTT client_id to NeoMind device_id by observing publish topics.
        if let Some(resolver) = self.build_topic_resolver() {
            broker.set_topic_resolver(resolver);
        }

        match broker.start().await {
            Ok(_) => {
                tracing::info!(
                    "Embedded MQTT broker started on :{} (auth_enabled read dynamically)",
                    port,
                );
            }
            Err(e) => {
                tracing::error!("Failed to start embedded broker: {}", e);
                tracing::warn!("Device management may not work properly");
            }
        }

        *self.devices.embedded_broker.write().unwrap() = Some(Arc::new(broker));
    }

    /// Initialize MQTT adapter for device communication.
    /// Creates and starts a real MQTT client that connects to the embedded broker.
    /// Initialize all built-in device adapters (MQTT, Webhook).
    ///
    /// Starts the device service, creates adapter instances, and registers
    /// them with the DeviceService.
    pub async fn init_device_adapters(&self) {
        use crate::config::get_embedded_broker_config;
        use neomind_devices::adapter::DeviceAdapter;
        use neomind_devices::adapters::{create_adapter, mqtt::MqttAdapterConfig};

        // Start device service to listen for EventBus events
        self.devices.service.start().await;

        // Create and register the internal MQTT adapter.
        // When auth is enabled, system credentials are needed for the handler.
        // When auth is disabled, providing credentials is harmless since
        // rumqttd accepts connections with or without login.
        let broker_config = get_embedded_broker_config();
        let (adapter_username, adapter_password) = {
            #[cfg(feature = "embedded-broker")]
            {
                let cache = self.credential_cache.read().unwrap();
                if let Some(ref pass) = cache.system_password {
                    tracing::debug!("Internal MQTT adapter: using system credential from cache");
                    (Some("__neomind_internal__".to_string()), Some(pass.clone()))
                } else {
                    tracing::warn!("Internal MQTT adapter: no system credential in cache, connecting without auth");
                    (None, None)
                }
            }
            #[cfg(not(feature = "embedded-broker"))]
            {
                (None, None)
            }
        };

        let mqtt_config = MqttAdapterConfig {
            name: "internal-mqtt".to_string(),
            mqtt: neomind_devices::mqtt::MqttConfig {
                broker: "127.0.0.1".to_string(), // Use IPv4 literal to avoid IPv6 resolution on Windows
                port: broker_config.port,        // Dynamic port from config
                client_id: Some("neomind-internal".to_string()),
                username: adapter_username,
                password: adapter_password,
                tls: broker_config.tls_enabled, // Use TLS setting from config
                ca_cert: broker_config.tls_ca_path.clone(),
                // One-way TLS: client only needs CA to verify server, no client cert/key
                client_cert: None,
                client_key: None,
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
            device_id_field: broker_config.device_id_field.clone(),
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
                    mqtt.set_shared_device_registry(self.devices.service.get_registry())
                        .await;
                    mqtt.set_data_dir(self.data_dir.clone());
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

        // Create and register the webhook adapter
        {
            use neomind_devices::adapters::{create_adapter, webhook::WebhookAdapterConfig};

            let webhook_config = WebhookAdapterConfig::new("internal-webhook");
            let webhook_config_value = serde_json::to_value(&webhook_config)
                .unwrap_or_else(|_| serde_json::json!({ "name": "internal-webhook" }));

            let event_bus = match self.core.event_bus.as_ref() {
                Some(bus) => bus,
                None => {
                    tracing::error!("EventBus not initialized, cannot create webhook adapter");
                    return;
                }
            };

            match create_adapter("webhook", &webhook_config_value, event_bus) {
                Ok(adapter) => {
                    adapter.set_telemetry_storage(self.devices.telemetry.clone());

                    // Set shared device registry so token verification and device lookup work
                    if let Some(whk) = adapter
                        .as_any()
                        .downcast_ref::<neomind_devices::adapters::webhook::WebhookAdapter>(
                    ) {
                        whk.set_shared_device_registry(self.devices.service.get_registry())
                            .await;
                        whk.set_data_dir(self.data_dir.clone());
                    }

                    self.devices
                        .service
                        .register_adapter("internal-webhook".to_string(), adapter.clone())
                        .await;
                    if let Err(e) = adapter.start().await {
                        tracing::warn!("Failed to start webhook adapter: {}", e);
                    } else {
                        tracing::info!("Webhook adapter started successfully");
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to create webhook adapter: {}", e);
                }
            }
        }

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

    /// Initialize tool registry with CLI-first design.
    /// Domain tools (device, agent, rule, message, etc.) are handled via `neomind` CLI
    /// commands through the shell tool. Built-in skills guide the LLM on CLI usage.
    pub async fn init_tools(&self) {
        use neomind_agent::toolkit::ToolRegistryBuilder;
        use std::sync::Arc;

        let mut registry = ToolRegistryBuilder::new()
            // Extension registry for scanning extension-provided tools
            .with_extension_registry(self.extensions.registry.clone())
            // Shell tool — CLI-first: domain operations via `neomind` commands
            .with_shell_tool(Some(neomind_agent::toolkit::ShellConfig {
                enabled: true,
                timeout_secs: 30,
                max_output_chars: 10000,
            }))
            // Scan extensions and register their tools (dynamic, keep)
            .with_extensions_scanned()
            .await
            .build();

        // Register standalone tools that don't map to CLI commands
        // Skill tool — manages skill CRUD (not CLI-replaceable)
        let skill_registry = self.agents.session_manager.skill_registry();
        {
            let tool = neomind_agent::toolkit::skill_tool::SkillTool::with_data_dir(
                skill_registry.clone(),
                self.data_dir.clone(),
            );
            registry.register(Arc::new(tool));
        }

        // Web fetch tool — retrieves URL content
        registry.register(Arc::new(neomind_agent::toolkit::WebFetchTool::new()));
        // File write tool — creates/overwrites files in data/
        registry.register(Arc::new(neomind_agent::toolkit::FileWriteTool::new(
            self.data_dir.clone(),
        )));
        // File edit tool — precise string replacement in files
        registry.register(Arc::new(neomind_agent::toolkit::FileEditTool::new(
            self.data_dir.clone(),
        )));

        // Vision tool — auto-detect VLM backend and register if available
        if let Ok(llm_manager) = neomind_agent::llm_backends::get_instance_manager() {
            let has_vlm = llm_manager
                .list_instances()
                .iter()
                .any(|inst| inst.capabilities.supports_multimodal);
            if has_vlm {
                registry.register(Arc::new(neomind_agent::toolkit::VisionTool::new(
                    neomind_agent::toolkit::VisionConfig::default(),
                    llm_manager,
                )));
                tracing::info!(
                    category = "ai",
                    "Vision tool registered (VLM backend detected)"
                );
            }
        } else {
            tracing::debug!(
                category = "ai",
                "Vision tool skipped: LLM instance manager not available"
            );
        }

        // Image edit tool — crop, draw, blur. No VLM capability gate:
        // works on any deployment (pure image manipulation, no model needed).
        registry.register(Arc::new(neomind_agent::toolkit::ImageEditTool::new(
            self.data_dir.clone(),
        )));
        tracing::info!(category = "ai", "Image edit tool registered");

        // Memory tool — persistent memory across sessions
        // Uses per-execution handle swapping for concurrency safety
        {
            let memory_store = tokio::sync::RwLock::new((*self.agents.system_memory_store).clone());
            let memory_tool = neomind_agent::toolkit::MemoryTool::with_session_handle(
                std::sync::Arc::new(memory_store),
                self.agents.memory_session_handle.clone(),
            );
            registry.register(Arc::new(memory_tool));
        }

        let tool_registry = Arc::new(registry);
        self.agents
            .session_manager
            .set_tool_registry(tool_registry.clone())
            .await;

        tracing::info!(
            category = "ai",
            "Tool registry initialized with {} tools (CLI-first + extensions + meta)",
            tool_registry.len()
        );
    }

    /// Refresh extension tools in the tool registry.
    ///
    /// Should be called after extensions are loaded (`init_extensions`) to ensure
    /// extension-provided tools are available to chat sessions and agents.
    /// The initial `init_tools()` runs before extensions are loaded, so this
    /// rescans the extension registry and updates the cached tool registry.
    pub async fn refresh_extension_tools(&self) {
        use neomind_agent::toolkit::ToolRegistryBuilder;
        use std::sync::Arc;

        // Rebuild the registry with extensions now loaded
        let mut registry = ToolRegistryBuilder::new()
            .with_extension_registry(self.extensions.registry.clone())
            .with_shell_tool(Some(neomind_agent::toolkit::ShellConfig {
                enabled: true,
                timeout_secs: 30,
                max_output_chars: 10000,
            }))
            .with_extensions_scanned()
            .await
            .build();

        // Re-register standalone tools
        let skill_registry = self.agents.session_manager.skill_registry();
        {
            let tool = neomind_agent::toolkit::skill_tool::SkillTool::with_data_dir(
                skill_registry.clone(),
                self.data_dir.clone(),
            );
            registry.register(Arc::new(tool));
        }

        // Re-register web/file tools
        registry.register(Arc::new(neomind_agent::toolkit::WebFetchTool::new()));
        registry.register(Arc::new(neomind_agent::toolkit::FileWriteTool::new(
            self.data_dir.clone(),
        )));
        registry.register(Arc::new(neomind_agent::toolkit::FileEditTool::new(
            self.data_dir.clone(),
        )));

        // Vision tool — auto-detect VLM backend and register if available
        if let Ok(llm_manager) = neomind_agent::llm_backends::get_instance_manager() {
            let has_vlm = llm_manager
                .list_instances()
                .iter()
                .any(|inst| inst.capabilities.supports_multimodal);
            if has_vlm {
                registry.register(Arc::new(neomind_agent::toolkit::VisionTool::new(
                    neomind_agent::toolkit::VisionConfig::default(),
                    llm_manager,
                )));
                tracing::info!(
                    category = "ai",
                    "Vision tool registered (VLM backend detected)"
                );
            }
        } else {
            tracing::debug!(
                category = "ai",
                "Vision tool skipped: LLM instance manager not available"
            );
        }

        // Image edit tool — crop, draw, blur. No VLM capability gate.
        registry.register(Arc::new(neomind_agent::toolkit::ImageEditTool::new(
            self.data_dir.clone(),
        )));

        // Re-register memory tool (per-execution handle swapping for concurrency safety)
        {
            let memory_store = tokio::sync::RwLock::new((*self.agents.system_memory_store).clone());
            let memory_tool = neomind_agent::toolkit::MemoryTool::with_session_handle(
                std::sync::Arc::new(memory_store),
                self.agents.memory_session_handle.clone(),
            );
            registry.register(Arc::new(memory_tool));
        }

        let tool_registry = Arc::new(registry);
        let tool_count = tool_registry.len();
        self.agents
            .session_manager
            .set_tool_registry(tool_registry.clone())
            .await;

        // Apply persisted tool-disable state so previously toggled-off tools
        // stay hidden from the LLM after a server restart. Built-ins are never
        // disabled; only extension tools (and only those listed in storage).
        apply_persisted_tool_disabled_state(&self.extensions.runtime, &tool_registry).await;

        tracing::info!(
            category = "ai",
            "Tool registry refreshed with {} tools (extensions now loaded)",
            tool_count
        );
    }

    /// Initialize extension event subscription.
    ///
    /// Starts a background task that subscribes to EventBus events
    /// and forwards them to extensions that have subscribed via EventCapabilityProvider.
    ///
    /// This uses the EventBus as the single source of truth for all events,
    /// eliminating the need for a separate event dispatcher.
    pub async fn init_extension_event_subscription(&self) {
        // Prevent duplicate initialization
        if self
            .extension_event_subscription_initialized
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            tracing::debug!("Extension event subscription already initialized, skipping");
            return;
        }

        tracing::info!("Initializing extension event subscription...");

        let event_bus = match &self.core.event_bus {
            Some(bus) => bus,
            None => {
                tracing::warn!("Extension event subscription not started: event_bus not available");
                return;
            }
        };

        // Get the event dispatcher from the extension state
        let event_dispatcher = match self.extensions.get_event_dispatcher() {
            Some(dispatcher) => dispatcher,
            None => {
                tracing::warn!(
                    "Extension event subscription not started: event_dispatcher not available"
                );
                return;
            }
        };

        use neomind_core::extension::ExtensionEventSubscriptionService;

        // Get or create the service instance (cached in ServerState)
        {
            let mut cached_service = self.extension_event_subscription_service.lock().await;
            if cached_service.is_none() {
                let service =
                    ExtensionEventSubscriptionService::new((*event_bus).clone(), event_dispatcher);
                *cached_service = Some(service);
            }
        }

        // Start the service
        let running = {
            let cached_service = self.extension_event_subscription_service.lock().await;
            cached_service
                .as_ref()
                .expect("extension event subscription service should be initialized")
                .start()
        };

        if running.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                category = "extensions",
                "Extension event subscription started - events will be forwarded to subscribed extensions"
            );
        } else {
            tracing::warn!("Extension event subscription failed to start");
        }
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

        // The rule engine reacts to device metrics via the value-provider update
        // task spawned below (plus the extension-output task). There is no
        // separate "rule engine event service" anymore — it was a dead shell
        // whose start() only flipped an AtomicBool that nothing ever read.
        tracing::info!(
            category = "rule_engine",
            "Rule engine event listener starting - rules will auto-evaluate on device metrics"
        );

        // Start a task to update the UnifiedValueProvider when device metrics arrive
        // This is needed for rule evaluation to work with current values
        let mut rx = event_bus.filter().device_events();
        let value_provider = self.automation.value_provider.clone();
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
                    ..
                } = event
                {
                    tracing::debug!(
                        "Received device metric: {} {} = {:?}",
                        device_id,
                        metric,
                        value
                    );

                    // Extract value for rule evaluation (numeric or string)
                    let rule_value = match &value {
                        MetricValue::Float(v) => Some(neomind_rules::RuleValue::Number(*v)),
                        MetricValue::Integer(v) => {
                            Some(neomind_rules::RuleValue::Number(*v as f64))
                        }
                        MetricValue::Boolean(v) => {
                            Some(neomind_rules::RuleValue::Number(if *v { 1.0 } else { 0.0 }))
                        }
                        MetricValue::String(s) => Some(neomind_rules::RuleValue::Text(s.clone())),
                        MetricValue::Json(v) => Some(neomind_rules::RuleValue::Text(v.to_string())),
                    };

                    if let Some(rv) = rule_value {
                        // Update the UnifiedValueProvider with the new value
                        {
                            match &rv {
                                neomind_rules::RuleValue::Number(num_value) => {
                                    // Store with original metric key
                                    value_provider
                                        .update_value("device", &device_id, &metric, *num_value)
                                        .await;

                                    // Also store with common prefixes stripped for rule matching
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
                                            value_provider
                                                .update_value(
                                                    "device",
                                                    &device_id,
                                                    stripped_metric,
                                                    *num_value,
                                                )
                                                .await;
                                            break;
                                        }
                                    }
                                }
                                neomind_rules::RuleValue::Text(text_value) => {
                                    // Store with original metric key
                                    value_provider
                                        .update_string_value(
                                            "device", &device_id, &metric, text_value,
                                        )
                                        .await;

                                    // Also store with common prefixes stripped for rule matching
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
                                            value_provider
                                                .update_string_value(
                                                    "device",
                                                    &device_id,
                                                    stripped_metric,
                                                    text_value,
                                                )
                                                .await;
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        // Event-driven rule evaluation: notify engine of data change.
                        // Mirror the prefix-stripping logic used for value_provider
                        // above: rules authored against the stripped field name
                        // (e.g. "temperature") must still trigger when the device
                        // publishes with a common prefix (e.g. "values.temperature"),
                        // otherwise the subscription index never matches.
                        let data_source =
                            neomind_core::datasource::DataSourceId::device(&device_id, &metric);
                        rule_engine_for_update
                            .on_data_update(&data_source, rv.clone())
                            .await;

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
                                let stripped_source =
                                    neomind_core::datasource::DataSourceId::device(
                                        &device_id,
                                        stripped_metric,
                                    );
                                rule_engine_for_update
                                    .on_data_update(&stripped_source, rv.clone())
                                    .await;
                                break;
                            }
                        }
                    }
                }
            }

            tracing::warn!("Value provider update task ended");
        });

        // Extension events → rule engine
        // Extension metrics are published as ExtensionOutput events, not DeviceMetric,
        // so they need a separate subscription to reach the value provider + rule engine.
        let mut ext_rx = event_bus.filter().extension_output();
        let value_provider_ext = self.automation.value_provider.clone();
        let rule_engine_ext = rule_engine.clone();

        tokio::spawn(async move {
            use neomind_core::{MetricValue, NeoMindEvent};

            tracing::info!("Starting extension output listener for rule engine");

            while let Some((event, _)) = ext_rx.recv().await {
                if let NeoMindEvent::ExtensionOutput {
                    extension_id,
                    output_name,
                    value,
                    ..
                } = event
                {
                    // output_name has two formats:
                    //   "extension_id:metric_name" (from publish_extension_metrics)
                    //   "metric_name" (from ExtensionMetricsCollector)
                    let metric_name = output_name
                        .strip_prefix(&format!("{}:", extension_id))
                        .unwrap_or(&output_name);

                    let rv = match &value {
                        MetricValue::Float(v) => neomind_rules::RuleValue::Number(*v),
                        MetricValue::Integer(v) => neomind_rules::RuleValue::Number(*v as f64),
                        MetricValue::Boolean(v) => {
                            neomind_rules::RuleValue::Number(if *v { 1.0 } else { 0.0 })
                        }
                        MetricValue::String(s) => neomind_rules::RuleValue::Text(s.clone()),
                        MetricValue::Json(v) => neomind_rules::RuleValue::Text(v.to_string()),
                    };

                    // Update value provider
                    value_provider_ext
                        .update_rule_value("extension", &extension_id, metric_name, rv.clone())
                        .await;

                    // Notify rule engine
                    let data_source = neomind_core::datasource::DataSourceId::extension(
                        &extension_id,
                        metric_name,
                    );
                    rule_engine_ext.on_data_update(&data_source, rv).await;
                }
            }

            tracing::warn!("Extension output listener task ended");
        });

        // Wire agent trigger callback for rule TriggerAgent actions
        {
            let agent_manager = self.agents.agent_manager.clone();
            let rule_engine = self.automation.rule_engine.clone();
            let callback: neomind_rules::AgentTriggerCallback = Arc::new(
                move |agent_id: String, input: Option<String>, data: Option<serde_json::Value>| {
                    let agent_manager = agent_manager.clone();
                    Box::pin(async move {
                        let mgr_guard = agent_manager.read().await;
                        if let Some(mgr) = mgr_guard.as_ref() {
                            let agent_input = if input.is_some() || data.is_some() {
                                Some(neomind_agent::AgentInput {
                                    content: input,
                                    data,
                                    source: Some("rule_engine".to_string()),
                                })
                            } else {
                                None
                            };
                            match mgr.execute_agent_now(&agent_id, agent_input).await {
                                Ok(_) => Ok(()),
                                Err(e) => Err(format!("Failed to trigger agent: {}", e)),
                            }
                        } else {
                            Err("Agent manager not initialized".to_string())
                        }
                    })
                },
            );
            rule_engine.set_agent_trigger_callback(callback).await;
        }

        // Start cron scheduler tick for Schedule-type rules
        {
            let rule_engine = self.automation.rule_engine.clone();
            tokio::spawn(async move {
                use chrono::Utc;
                use cron::Schedule;

                tracing::info!("Starting rule cron scheduler (30s tick)");

                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
                ticker.tick().await; // First tick completes immediately

                loop {
                    ticker.tick().await;
                    let now = Utc::now();

                    let schedule_rules = rule_engine.list_schedule_rules().await;
                    for (rule_id, cron_expr) in schedule_rules {
                        // Parse cron and check if it should fire now (within the last 30s window)
                        let parsed: Result<Schedule, _> = cron_expr.parse();
                        let parsed = match parsed {
                            Ok(s) => s,
                            Err(_) => continue,
                        };

                        // Get the next scheduled time after (now - 30s).
                        // If that time is <= now, the rule should fire.
                        let window_start = now - chrono::Duration::seconds(30);
                        let next = parsed.after(&window_start.fixed_offset()).next();
                        if let Some(next_time) = next {
                            if next_time.timestamp() <= now.timestamp() {
                                tracing::debug!(
                                    rule_id = %rule_id,
                                    cron = %cron_expr,
                                    "Executing scheduled rule"
                                );
                                let result = rule_engine.execute_rule(&rule_id).await;
                                if result.success {
                                    tracing::info!(
                                        rule_id = %rule_id,
                                        rule_name = %result.rule_name,
                                        duration_ms = result.duration_ms,
                                        "Scheduled rule executed successfully"
                                    );
                                } else {
                                    tracing::warn!(
                                        rule_id = %rule_id,
                                        rule_name = %result.rule_name,
                                        error = ?result.error,
                                        "Scheduled rule execution failed"
                                    );
                                }
                            }
                        }
                    }
                }
            });
        }
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
                    use neomind_agent::llm_backends::backends::{OllamaConfig, OllamaRuntime};
                    use neomind_core::llm::backend::LlmRuntime;

                    let config = OllamaConfig::new(neomind_core::config::models::OLLAMA_DEFAULT)
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
                                        tool_calls: None,
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

                    let manager = Arc::new(crate::automation::AutoOnboardManager::new(
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
                // Check if this is a device discovered event
                if let neomind_core::NeoMindEvent::DeviceDiscovered {
                    device_id,
                    source,
                    adapter_id,
                    metadata,
                    sample,
                    is_binary,
                    timestamp: _,
                } = event
                {
                    // Extract the actual payload data from sample
                    let payload_data = sample.get("data").unwrap_or(&sample);

                    // Check if device is already registered - skip auto-onboarding if it is
                    if device_service_clone.get_device(&device_id).is_some() {
                        tracing::debug!(
                            "Device {} already registered, skipping auto-onboarding",
                            device_id
                        );
                        continue;
                    }

                    tracing::info!(
                        "Processing discovered device from {}: device_id={}, is_binary={}",
                        source,
                        device_id,
                        is_binary
                    );

                    // Extract original_topic if available (for MQTT devices)
                    let original_topic = metadata
                        .get("original_topic")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    // Process the payload data through auto-onboarding
                    match manager
                        .process_unknown_device_with_topic(
                            &device_id,
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
                                "Successfully processed discovered device: {}",
                                device_id
                            );
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "Discovered device not accepted (disabled or at capacity): {}",
                                device_id
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to process discovered device {}: {}",
                                device_id,
                                e
                            );
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
            self.automation.value_provider.clone(),
            self.automation.rule_engine.clone(),
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

        // Reuse the TimeSeriesStore that's already opened by DeviceService
        // We can't reopen telemetry.redb because redb doesn't support opening the same
        // file multiple times in the same process
        let time_series_store = Some(self.devices.telemetry.inner_store());

        // Get LLM runtime from SessionManager if available
        let llm_runtime = if let Ok(Some(backend)) =
            self.agents.session_manager.get_llm_backend().await
        {
            use neomind_agent::llm_backends::{
                CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime,
            };
            use neomind_agent::LlmBackend;
            use neomind_core::llm::backend::LlmRuntime;

            match backend {
                LlmBackend::Ollama {
                    endpoint,
                    model,
                    capabilities: _,
                } => {
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
                LlmBackend::LlamaCpp {
                    endpoint,
                    model,
                    capabilities: _,
                } => {
                    use neomind_agent::llm_backends::backends::llamacpp::{
                        LlamaCppConfig, LlamaCppRuntime,
                    };
                    let timeout = std::env::var("LLAMACPP_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(180);
                    match LlamaCppRuntime::new(
                        LlamaCppConfig::new(&model)
                            .with_endpoint(&endpoint)
                            .with_timeout_secs(timeout),
                    ) {
                        Ok(runtime) => Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>),
                        Err(e) => {
                            tracing::warn!(category = "ai", error = %e, "Failed to create llama.cpp runtime for agents");
                            None
                        }
                    }
                }
                LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model,
                    capabilities: _,
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
                            capabilities: _,
                        }
                        | LlmBackend::Google {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
                        }
                        | LlmBackend::XAi {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
                        }
                        | LlmBackend::Qwen {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
                        }
                        | LlmBackend::DeepSeek {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
                        }
                        | LlmBackend::GLM {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
                        }
                        | LlmBackend::MiniMax {
                            api_key,
                            endpoint,
                            model,
                            capabilities: _,
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
            tool_registry: self.agents.session_manager.get_tool_registry().await,
            memory_store: Some(self.agents.system_memory_store.clone()),
            backend_semaphores: None,
            skill_registry: Some(self.agents.session_manager.skill_registry()),
            // Populated inside AiAgentManager::new from the scheduler.
            execution_semaphore: None,
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

        // Inject the latest tool registry into the executor.
        // init_tools() may have triggered get_or_init_agent_manager() before the registry was built,
        // so the executor's tool_registry was None. Now the registry is ready, update it.
        if let Some(registry) = self.agents.session_manager.get_tool_registry().await {
            manager.update_tool_registry(registry);
        }

        manager.start().await.map_err(|e| {
            crate::models::ErrorResponse::internal(format!("Failed to start agent manager: {}", e))
        })?;
        tracing::info!("AI Agent manager scheduler started");
        Ok(())
    }

    /// Start the IM bridge router.
    ///
    /// Subscribes to `ImMessageReceived` on the EventBus and dispatches each
    /// event to `ImRouter::handle_inbound`. The router owns per-chat
    /// serialization, msg_id dedup, allowlist gating, and chat↔session reuse.
    /// Outbound replies are delivered through bridges registered on the
    /// router's `ImBridgeRegistry` (Task 10+ wires the Telegram bridge).
    ///
    /// Mirrors `start_agent_manager` (above): a failure here means all inbound
    /// IM messages are silently dropped, so callers in server/mod.rs startup
    /// surface errors via `tracing::error!` — do not swallow with `let _ =`.
    pub async fn start_im_router(&self) -> Result<(), crate::models::ErrorResponse> {
        use neomind_messages::im_bridge::{router::InboundMessage, ImPlatform};
        use std::collections::HashSet;

        // Open the IM session store under data/. A failure here means the
        // bridge cannot persist chat↔session mappings — surface as 500 rather
        // than panicking (the plan's `.unwrap()` would crash the server).
        let store = Arc::new(
            neomind_messages::im_bridge::session_store::ImSessionStore::open(&self.data_dir)
                .map_err(|e| {
                    crate::models::ErrorResponse::internal(format!(
                        "Failed to open IM session store: {}",
                        e
                    ))
                })?,
        );

        // SessionManagerAgentRunner forwards to the real SessionManager (Task 8).
        // Annotate the trait-object type so the unsizing coercion to
        // `Arc<dyn AgentRunner>` (expected by ImRouter::new) is unambiguous.
        let runner: Arc<dyn neomind_messages::im_bridge::AgentRunner> =
            Arc::new(SessionManagerAgentRunner::new(self.session_manager()));

        // Resolve the default IM agent lazily, per inbound message. With no
        // per-channel override mechanism yet (M1), sessions bind to the
        // first Active agent at creation time. Resolving lazily (instead of
        // hard-failing at boot) keeps a fresh system — which by definition
        // has zero agents — from logging a scary "IM router not started"
        // error and 503-ing every /im-bridges call before the user has done
        // anything. An agent created later works without a server restart.
        let agent_store = self.agent_store();
        let default_agent_resolver: neomind_messages::im_bridge::router::DefaultAgentResolver =
            Arc::new(move || {
                let store = agent_store.clone();
                Box::pin(async move {
                    store
                        .query_agents(neomind_storage::AgentFilter {
                            status: Some(neomind_storage::AgentStatus::Active),
                            ..Default::default()
                        })
                        .await
                        .ok()
                        .and_then(|agents| agents.into_iter().next().map(|a| a.id))
                })
            });

        // Clone a handle for the periodic expiry cleanup task (the original
        // `store` is moved into ImRouter below). Same Arc<ImSessionStore>,
        // so the cleanup task evicts from the same backing redb the router
        // reads/writes — no divergence.
        let store_for_cleanup = store.clone();

        // Load the persisted allowlist BEFORE `store` moves into the router.
        // Across restarts this repopulates the runtime gate from `/start`
        // binds the operator already approved — without it the router would
        // boot in allow-all mode (None) and the invite system would be
        // cosmetic: any chat could talk to the agent. A fresh deploy with no
        // approved chats yields an empty set, so Some(empty) rejects all
        // inbound until an operator mints + a user `/start`-binds an invite.
        // That is the intended M2a posture (invite-gated from boot).
        let initial_allowlist: HashSet<String> = store
            .allow_list()
            .map_err(|e| {
                crate::models::ErrorResponse::internal(format!(
                    "Failed to read IM allowlist: {}",
                    e
                ))
            })?
            .into_iter()
            .collect();

        let router = Arc::new(neomind_messages::im_bridge::router::ImRouter::new(
            store,
            runner,
            default_agent_resolver,
            Some(initial_allowlist),
        ));

        // Subscribe to ImMessageReceived events and forward each to the router.
        // The task is intentionally detached: it lives for the process lifetime
        // (same pattern as init_agent_events at types.rs:2999). Holding the
        // `FilteredReceiver` in `rx` keeps the broadcast subscription alive;
        // dropping it on bus close terminates the `while let` cleanly.
        if let Some(bus) = self.core.event_bus.clone() {
            let r = router.clone();
            let mut rx = bus.subscribe_filtered(move |e| {
                matches!(e, neomind_core::NeoMindEvent::ImMessageReceived { .. })
            });
            tokio::spawn(async move {
                tracing::info!(category = "im", "IM router event listener started");
                while let Some((ev, _meta)) = rx.recv().await {
                    if let neomind_core::NeoMindEvent::ImMessageReceived {
                        platform,
                        im_chat_id,
                        sender_id,
                        text,
                        msg_id,
                        timestamp,
                    } = ev
                    {
                        // Unknown platform strings fall back to Telegram so a
                        // typo in a bridge's published event doesn't blackhole
                        // the message; this mirrors the bridge's own inbound
                        // publish path.
                        let p = ImPlatform::parse(&platform).unwrap_or(ImPlatform::Telegram);
                        r.handle_inbound(InboundMessage {
                            platform: p,
                            chat_id: im_chat_id,
                            sender_id,
                            text,
                            msg_id,
                            timestamp,
                        })
                        .await;
                    }
                }
                tracing::warn!(
                    category = "im",
                    "IM router event listener exited (EventBus closed)"
                );
            });
        } else {
            tracing::warn!(
                category = "im",
                "EventBus not available — IM router will not receive inbound messages"
            );
        }

        // Reload persisted bridges: each platform's credential set was saved by
        // `create_bridge_handler` so the bridge auto-starts after a server
        // restart. Reload happens AFTER the router is built + subscribed (so the
        // registry exists to receive the rebuilt bridges) and BEFORE the router
        // is published into `im_router` (so handlers can't observe a half-loaded
        // registry). Single-bridge failure is warned + skipped inside
        // reload_persisted_bridges — it does not block server startup. No bus
        // means no spawn target; the persisted rows remain on disk for next boot.
        if let Some(bus) = self.core.event_bus.clone() {
            crate::handlers::im_bridges::reload_persisted_bridges(&router, router.store(), &bus)
                .await;
        } else {
            tracing::warn!(
                category = "im",
                "EventBus not available — persisted IM bridges skipped this boot (rows remain on disk)"
            );
        }

        *self.im_router.write().await = Some(router);
        tracing::info!(category = "im", "IM router started");

        // Periodic IM session expiry cleanup — mirrors the agent-execution
        // cleanup pattern above (line ~1132). TTL = 7 days: a chat inactive
        // this long is unlikely to recall its prior NeoMind session, and
        // re-prompting from a clean slate (new session via get_or_create) is
        // cheaper than carrying stale context indefinitely. First tick is
        // immediate (`tokio::time::interval` semantics) — consumed so the
        // server doesn't do a full-table scan during startup, matching the
        // agent-execution cleanup's rationale.
        {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                // Skip the immediate first tick to avoid contending with
                // startup I/O (same rationale as the agent-execution cleanup).
                interval.tick().await;
                let ttl_secs: i64 = 7 * 86400;
                loop {
                    interval.tick().await;
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let cutoff = now_secs - ttl_secs;
                    match store_for_cleanup.evict_expired(cutoff) {
                        Ok(n) if n > 0 => {
                            tracing::info!(
                                category = "im",
                                removed = n,
                                "IM session expiry cleanup removed stale records"
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(
                                category = "im",
                                error = %e,
                                "IM session expiry cleanup failed — stale sessions may accumulate"
                            );
                        }
                    }
                }
            });
        }

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

        // Cleanup agents stuck in Executing status on startup.
        // On a fresh startup, NO agent can be executing — so unconditionally
        // reset all Executing agents to Active to prevent permanent stuck states.
        tokio::spawn(async move {
            if let Ok(agents) = store
                .query_agents(neomind_storage::AgentFilter {
                    status: Some(neomind_storage::AgentStatus::Executing),
                    ..Default::default()
                })
                .await
            {
                for agent in agents {
                    tracing::warn!(
                        agent_id = %agent.id,
                        agent_name = %agent.name,
                        "Agent stuck in Executing status on startup, resetting to Active"
                    );
                    let _ = store
                        .update_agent_status(
                            &agent.id,
                            neomind_storage::AgentStatus::Active,
                            Some("Server restarted - status reset".to_string()),
                        )
                        .await;
                }
            }
        });

        tokio::spawn(async move {
            let mut rx = event_bus.subscribe();
            tracing::info!("Agent event listener started - monitoring for event-triggered agents");

            while let Some((event, _metadata)) = rx.recv().await {
                // Unified data source event handling for agent triggers
                match event {
                    neomind_core::NeoMindEvent::DeviceMetric {
                        device_id,
                        metric,
                        value,
                        ..
                    } => {
                        if let Err(e) = executor
                            .check_and_trigger_data_event("device", device_id, metric, &value)
                            .await
                        {
                            tracing::debug!("No agent triggered for device event: {}", e);
                        }
                    }
                    neomind_core::NeoMindEvent::ExtensionOutput {
                        extension_id,
                        output_name,
                        value,
                        ..
                    } => {
                        if let Err(e) = executor
                            .check_and_trigger_data_event(
                                "extension",
                                extension_id,
                                output_name,
                                &value,
                            )
                            .await
                        {
                            tracing::debug!("No agent triggered for extension event: {}", e);
                        }
                    }
                    _ => {} // Ignore other events
                }
            }
        });
    }
}

/// Rebuild the ToolRegistry disabled set from the persisted ExtensionRecord
/// state and push it live. Built-in tools are never disabled; only extension
/// tools whose parent extension is `enabled=false` (master off) or whose
/// command name appears in `disabled_commands` (per-command off) are hidden.
///
/// Called at startup (after extensions load + tool registry finalized) and
/// after every toggle API call. Cheap: O(n_extensions × n_commands).
async fn apply_persisted_tool_disabled_state(
    runtime: &neomind_core::extension::ExtensionRuntime,
    tool_registry: &std::sync::Arc<neomind_agent::toolkit::ToolRegistry>,
) {
    use std::collections::HashSet;

    let records = match neomind_storage::ExtensionStore::open("data/extensions.redb") {
        Ok(store) => store.load_all().unwrap_or_default(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to open extension store for tool-disable bootstrap; all tools enabled"
            );
            return;
        }
    };

    let mut disabled: HashSet<String> = HashSet::new();
    for r in records {
        if r.uninstalled {
            continue;
        }
        if !r.enabled {
            // Master off: enumerate live commands to materialize tool names.
            if let Some(info) = runtime.get(&r.id).await {
                for cmd in &info.commands {
                    disabled.insert(format!("{}:{}", r.id, cmd.name));
                }
            }
            continue;
        }
        for cmd_name in &r.disabled_commands {
            disabled.insert(format!("{}:{}", r.id, cmd_name));
        }
    }

    if disabled.is_empty() {
        tracing::debug!(
            category = "ai",
            "Tool-disable bootstrap: all extension tools enabled"
        );
    } else {
        tracing::info!(
            category = "ai",
            count = disabled.len(),
            "Tool-disable bootstrap: hiding disabled extension tools from LLM"
        );
    }
    tool_registry.set_disabled(disabled);
}

// Note: Default implementation removed because ServerState::new() is now async
// to support persistent device registry initialization.

/// Production `AgentRunner` adapter that forwards to `SessionManager`.
///
/// Used by `start_im_router` (Task 9) to bind the IM bridge to the real agent
/// backend. The aggregation pattern mirrors the canonical HTTP chat handler in
/// `handlers/sessions.rs:757-781`: append `AgentEvent::Content` chunks, break
/// on `AgentEvent::End`, ignore everything else (Thinking/ToolCall*/Warning).
///
/// Deliberately no unit test — `SessionManager` cannot have its LLM backend
/// injected in-process (by design), so the adapter is exercised end-to-end in
/// Task 11 via an `EchoRunner` mock instead.
pub struct SessionManagerAgentRunner {
    sm: Arc<SessionManager>,
}

impl SessionManagerAgentRunner {
    pub fn new(sm: Arc<SessionManager>) -> Self {
        Self { sm }
    }
}

#[async_trait::async_trait]
impl neomind_messages::im_bridge::AgentRunner for SessionManagerAgentRunner {
    async fn create_session(&self) -> anyhow::Result<String> {
        // CreateSessionOptions::default() leaves every override as None, so the
        // new session inherits the manager's default_config (model / system
        // prompt / tools). The configured "default IM agent" binding happens
        // in Task 9's start_im_router, not here.
        let opts = neomind_agent::CreateSessionOptions::default();
        Ok(self.sm.create_session_with_options(opts).await?)
    }

    async fn run(&self, session_id: &str, text: &str) -> anyhow::Result<String> {
        use futures::StreamExt as _;
        // backend_id=None → use the session's currently-active backend.
        // selected_skills=&[] → no pinned skills for this turn.
        let stream = self
            .sm
            .process_message_events_with_backend_and_skills(session_id, text, None, None)
            .await?;
        let mut s = stream;
        let mut out = String::new();
        let mut last_error: Option<String> = None;
        while let Some(ev) = s.next().await {
            match ev {
                neomind_agent::AgentEvent::Content { content } => out.push_str(&content),
                // gotcha #10: capture the error instead of dropping it — if the
                // stream errors before any Content chunk, `out` would stay empty,
                // the router would reply "" → Telegram 400 → reply() silently
                // swallowed, leaving the user stuck on "思考中…".
                neomind_agent::AgentEvent::Error { message } => last_error = Some(message),
                neomind_agent::AgentEvent::End { .. } => break,
                _ => {}
            }
        }
        // gotcha #10: never return empty silently — surface the error text so
        // the router replies something instead of empty→400→silent. When content
        // was produced we keep it as-is (an error mid-stream after partial output
        // is still useful to the user).
        if out.is_empty() {
            if let Some(msg) = last_error {
                out = format!("（处理失败：{msg}）");
            }
        }
        Ok(out)
    }
}
