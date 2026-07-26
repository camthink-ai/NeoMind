//! Embedded MQTT Broker
//!
//! This module provides an embedded MQTT broker using rmqtt.
//! The broker runs in the same process as NeoMind, eliminating the need
//! for an external MQTT broker installation.
//!
//! ## Authentication
//!
//! A `ClientAuthenticate` hook is always registered. The hook reads
//! `auth_enabled` from a shared `Arc<AtomicBool>` that is updated by
//! the broker config API. The hook also validates credentials by reading
//! from a shared credential store (passed in at construction time).
//!
//! Changing `auth_enabled` takes effect immediately without restarting
//! the broker. Only `listen`, `port`, or `tls_enabled` changes require
//! a broker restart.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use neomind_core::{EventBus, NeoMindEvent};

/// Topic-to-device-id resolver used by `DevicePresenceHook` to learn which
/// NeoMind `device_id` corresponds to an MQTT `client_id`.
///
/// Devices don't always set their MQTT `client_id` equal to their NeoMind
/// `device_id` (e.g. an NE301 camera may use `NE302-000000` as its MQTT
/// client_id while registered as `2819FD`). The resolver lets the broker
/// learn the mapping by observing PUBLISH topics: when a client publishes
/// to a topic owned by a registered device, we record `client_id → device_id`
/// and reuse it for transport connect/disconnect events.
///
/// The closure is called with the publish topic and returns the owning
/// device_id, if any. Provided by the caller (typically wraps
/// `DeviceRegistry::find_device_by_telemetry_topic`).
pub type TopicResolverFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Embedded MQTT broker error type
#[derive(Debug, Error)]
pub enum EmbeddedBrokerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Broker error: {0}")]
    Broker(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Configuration for the embedded broker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedBrokerConfig {
    #[serde(default = "default_listen_addr")]
    pub listen: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    #[serde(default = "default_max_payload")]
    pub max_payload_size: usize,

    #[serde(default = "default_dynamic_filters")]
    pub dynamic_filters: bool,

    #[serde(default)]
    pub auth_enabled: bool,

    #[serde(default)]
    pub tls_enabled: bool,

    #[serde(default)]
    pub tls_cert_path: Option<String>,

    #[serde(default)]
    pub tls_key_path: Option<String>,

    #[serde(default)]
    pub tls_ca_path: Option<String>,
}

fn default_listen_addr() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    1883
}
fn default_max_connections() -> usize {
    1000
}
fn default_max_payload() -> usize {
    268435456 // 256 MB
}
fn default_dynamic_filters() -> bool {
    true
}

impl Default for EmbeddedBrokerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen_addr(),
            port: default_port(),
            max_connections: default_max_connections(),
            max_payload_size: default_max_payload(),
            dynamic_filters: default_dynamic_filters(),
            auth_enabled: false,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            tls_ca_path: None,
        }
    }
}

impl EmbeddedBrokerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_listen(mut self, listen: impl Into<String>) -> Self {
        self.listen = listen.into();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, EmbeddedBrokerError> {
        format!("{}:{}", self.listen, self.port)
            .parse()
            .map_err(|e| EmbeddedBrokerError::Config(format!("Invalid address: {}", e)))
    }
}

/// Credential validator function type.
/// Takes (username, password) and returns true if valid.
type CredentialValidatorFn = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Auth handler for the embedded MQTT broker.
///
/// Reads `auth_enabled` from a shared AtomicBool (updated by the API).
/// When auth is disabled, all connections (including anonymous) are allowed.
/// When auth is enabled, validates credentials via the credential validator.
struct NeoMindAuthHandler {
    auth_enabled: Arc<AtomicBool>,
    credential_validator: CredentialValidatorFn,
}

#[async_trait]
impl rmqtt::hook::Handler for NeoMindAuthHandler {
    async fn hook(
        &self,
        param: &rmqtt::hook::Parameter,
        acc: Option<rmqtt::hook::HookResult>,
    ) -> rmqtt::hook::ReturnType {
        if let rmqtt::hook::Parameter::ClientAuthenticate(connect_info) = param {
            let auth_enabled = self.auth_enabled.load(Ordering::Relaxed);

            if !auth_enabled {
                // Auth disabled — allow all connections (including anonymous)
                return (
                    false,
                    Some(rmqtt::hook::HookResult::AuthResult(
                        rmqtt::types::AuthResult::Allow(false, None),
                    )),
                );
            }

            // Auth enabled — validate credentials
            let username = connect_info.username();
            let password = connect_info.password();

            tracing::debug!(
                "Auth hook: username={:?}, has_password={}",
                username.map(|u| -> &str { u.as_ref() }),
                password.is_some()
            );

            // Must have both username and password
            if let (Some(uname), Some(pwd)) = (username, password) {
                let uname_str: &str = uname.as_ref();
                let pwd_bytes = pwd.as_ref();

                // Convert password bytes to str
                if let Ok(pwd_str) = std::str::from_utf8(pwd_bytes) {
                    tracing::debug!("Auth hook: validating user='{}'", uname_str);
                    if (self.credential_validator)(uname_str, pwd_str) {
                        let is_superuser = uname_str == "__neomind_internal__";
                        tracing::debug!(
                            "Auth hook: user='{}' authenticated (super={})",
                            uname_str,
                            is_superuser
                        );
                        return (
                            false,
                            Some(rmqtt::hook::HookResult::AuthResult(
                                rmqtt::types::AuthResult::Allow(is_superuser, None),
                            )),
                        );
                    }
                    tracing::warn!(
                        "Auth hook: credential validation failed for user='{}'",
                        uname_str
                    );
                } else {
                    tracing::warn!(
                        "Auth hook: password is not valid UTF-8 for user='{}'",
                        uname_str
                    );
                }
            } else {
                tracing::debug!("Auth hook: missing username or password (anonymous connection)");
            }

            // No valid credentials
            return (
                false,
                Some(rmqtt::hook::HookResult::AuthResult(
                    rmqtt::types::AuthResult::BadUsernameOrPassword,
                )),
            );
        }
        (true, acc) // Continue to next handler
    }
}

/// Hook that emits `DeviceTransportOnline` / `DeviceTransportOffline` events
/// to the NeoMind EventBus whenever an MQTT client connects or disconnects
/// at the transport layer.
///
/// This decouples "MQTT session is alive" from "device has recently sent
/// data", which is what `DeviceStatus::last_seen` tracks. Without this hook,
/// a device that's connected to the broker but hasn't published yet shows
/// up as "Never Connected" in the UI — a common customer-reported bug.
///
/// ## client_id → device_id resolution
///
/// Devices don't always set their MQTT `client_id` equal to their NeoMind
/// `device_id` (e.g. an NE301 camera may use `NE302-000000` as its MQTT
/// client_id while registered as `2819FD`). To handle this:
///
/// 1. On every `MessagePublish`, look up the publishing topic via
///    `topic_resolver`. If it matches a registered device's telemetry
///    topic, cache `client_id → device_id` in `client_id_cache`.
/// 2. On `ClientConnected` / `ClientDisconnected`, check the cache first.
///    If a mapping is known, fire the event with the cached `device_id`.
///    Otherwise, fall back to the legacy passthrough (MQTT client_id IS
///    the device_id).
///
/// The cache is shared across all hook instances (connect/disconnect/publish)
/// via `Arc<RwLock<...>>` so a single observed publish teaches the broker
/// the correct device_id for all future transport events from that client.
struct DevicePresenceHook {
    event_bus: Arc<EventBus>,
    /// Shared cache: MQTT client_id → NeoMind device_id, learned from
    /// observed PUBLISH topics. Falls back to client_id verbatim on miss.
    client_id_cache: Arc<RwLock<HashMap<String, String>>>,
    /// Optional resolver that maps a publish topic to a registered device_id.
    /// If `None`, no caching occurs and passthrough is always used.
    topic_resolver: Option<TopicResolverFn>,
}

impl DevicePresenceHook {
    fn new(
        event_bus: Arc<EventBus>,
        client_id_cache: Arc<RwLock<HashMap<String, String>>>,
        topic_resolver: Option<TopicResolverFn>,
    ) -> Self {
        Self {
            event_bus,
            client_id_cache,
            topic_resolver,
        }
    }

    /// Resolve an rmqtt client_id to a NeoMind device_id.
    ///
    /// Priority:
    /// 1. Cached mapping (learned from prior MessagePublish observations)
    /// 2. Passthrough (client_id == device_id) — legacy convention
    fn resolve_device_id(&self, client_id: &rmqtt::types::ClientId) -> String {
        let client_id_str = client_id.to_string();
        if let Ok(cache) = self.client_id_cache.read() {
            if let Some(device_id) = cache.get(&client_id_str) {
                return device_id.clone();
            }
        }
        client_id_str
    }

    /// True if this client_id belongs to a NeoMind-internal MQTT client
    /// (the embedded broker itself, or one of the adapter's broker-bridge
    /// connections to external brokers). Such clients must NOT fire
    /// `DeviceTransportOnline/Offline` events — they would create phantom
    /// devices and pollute the device-status map.
    ///
    /// The `neomind-` prefix is reserved for internal use. User-registered
    /// devices that need a stable client_id should use a different prefix
    /// (typically the device_id itself, which is unconstrained).
    fn is_internal_client(client_id: &rmqtt::types::ClientId) -> bool {
        client_id.to_string().starts_with("neomind-")
    }
}

#[async_trait]
impl rmqtt::hook::Handler for DevicePresenceHook {
    async fn hook(
        &self,
        param: &rmqtt::hook::Parameter,
        _acc: Option<rmqtt::hook::HookResult>,
    ) -> rmqtt::hook::ReturnType {
        let now = chrono::Utc::now().timestamp();
        match param {
            rmqtt::hook::Parameter::ClientConnected(session) => {
                // Skip NeoMind-internal clients (embedded broker's own
                // connections, external-broker bridges). Otherwise they'd
                // fire DeviceTransportOnline for phantom devices like
                // "neomind-<broker_id>-<uuid>" and pollute the status map.
                if Self::is_internal_client(&session.id.client_id) {
                    tracing::trace!(
                        "DevicePresenceHook: skipping internal client '{}'",
                        session.id.client_id
                    );
                    return (true, _acc);
                }
                let client_id_str = session.id.client_id.to_string();
                let device_id = self.resolve_device_id(&session.id.client_id);
                let cached = device_id != client_id_str;
                tracing::debug!(
                    "DevicePresenceHook: client_connected client_id='{}' -> device_id='{}' (cached={})",
                    client_id_str,
                    device_id,
                    cached
                );
                self.event_bus
                    .publish(NeoMindEvent::DeviceTransportOnline {
                        device_id,
                        client_id: client_id_str,
                        timestamp: now,
                    })
                    .await;
            }
            rmqtt::hook::Parameter::ClientDisconnected(session, reason) => {
                if Self::is_internal_client(&session.id.client_id) {
                    return (true, _acc);
                }
                let client_id_str = session.id.client_id.to_string();
                let device_id = self.resolve_device_id(&session.id.client_id);
                let cached = device_id != client_id_str;
                // Clean up cache on disconnect — was never removed, so rotating
                // client_ids (firmware per-boot UUIDs, transient bridges) leaked
                // for the process lifetime (slow OOM on long-running edge boxes).
                if cached {
                    if let Ok(mut cache) = self.client_id_cache.write() {
                        cache.remove(&client_id_str);
                    }
                }
                let reason_str = match reason {
                    rmqtt::types::Reason::Unknown => None,
                    other => Some(format!("{:?}", other)),
                };
                tracing::debug!(
                    "DevicePresenceHook: client_disconnected client_id='{}' -> device_id='{}' (cached={}) reason={:?}",
                    client_id_str,
                    device_id,
                    cached,
                    reason
                );
                self.event_bus
                    .publish(NeoMindEvent::DeviceTransportOffline {
                        device_id,
                        client_id: client_id_str,
                        reason: reason_str,
                        timestamp: now,
                    })
                    .await;
            }
            rmqtt::hook::Parameter::MessagePublish(_session, from, publish) => {
                // Learn client_id → device_id from observed publish topics.
                // Skip internal clients (broker self-publish, bridge traffic).
                let client_id_str = from.id.client_id.to_string();
                if client_id_str.starts_with("neomind-") {
                    return (true, _acc);
                }
                // Avoid holding the resolver across the cache write lock.
                let device_id_opt = self
                    .topic_resolver
                    .as_ref()
                    .and_then(|resolver| resolver(&publish.topic));
                if let Some(device_id) = device_id_opt {
                    let mut changed = false;
                    if let Ok(mut cache) = self.client_id_cache.write() {
                        match cache.get(&client_id_str) {
                            Some(existing) if existing == &device_id => {
                                // Already cached, no-op.
                            }
                            _ => {
                                cache.insert(client_id_str.clone(), device_id.clone());
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        tracing::debug!(
                            "DevicePresenceHook: learned mapping client_id='{}' -> device_id='{}' from topic='{}'",
                            client_id_str,
                            device_id,
                            publish.topic
                        );
                    }
                }
            }
            _ => {}
        }
        // Always continue to next handler — presence tracking is observe-only.
        (true, _acc)
    }
}

///
/// Manages the lifecycle of the embedded broker running as a tokio task.
/// The broker can be stopped and restarted with new configuration.
pub struct EmbeddedBroker {
    config: Mutex<EmbeddedBrokerConfig>,
    running: AtomicBool,
    abort_handle: Mutex<Option<tokio::task::AbortHandle>>,
    /// Shared auth_enabled flag — updated by the broker config API.
    /// The auth hook reads this on every connection.
    auth_enabled: Arc<AtomicBool>,
    /// Credential validator function — validates (username, password).
    credential_validator: CredentialValidatorFn,
    /// Optional EventBus for emitting transport-level presence events
    /// (DeviceTransportOnline/Offline). Set via `set_event_bus` before
    /// `start()` is called; if `None`, the broker runs without presence
    /// tracking (legacy behavior).
    event_bus: Mutex<Option<Arc<EventBus>>>,
    /// Optional topic-to-device-id resolver. Set via `set_topic_resolver`
    /// before `start()` is called. When set, the broker observes
    /// `MessagePublish` events to learn the mapping from MQTT client_id
    /// to NeoMind device_id (necessary when devices use a client_id
    /// different from their registered device_id).
    topic_resolver: Mutex<Option<TopicResolverFn>>,
}

impl EmbeddedBroker {
    /// Create a new embedded broker with the given configuration and credential validator.
    ///
    /// The `credential_validator` closure takes (username, password) and returns true
    /// if the credentials are valid. It's called on every MQTT connection when auth is enabled.
    pub fn new(config: EmbeddedBrokerConfig, credential_validator: CredentialValidatorFn) -> Self {
        let auth_enabled = Arc::new(AtomicBool::new(config.auth_enabled));
        Self {
            config: Mutex::new(config),
            running: AtomicBool::new(false),
            abort_handle: Mutex::new(None),
            auth_enabled,
            credential_validator,
            event_bus: Mutex::new(None),
            topic_resolver: Mutex::new(None),
        }
    }

    /// Create with default configuration
    pub fn with_default() -> Self {
        Self::new(EmbeddedBrokerConfig::default(), Arc::new(|_, _| false))
    }

    /// Provide the EventBus used to publish transport-level presence events.
    /// Must be called before `start()`. Has no effect on an already-running
    /// broker — call `stop()` then `start()` again to pick up a new bus.
    pub fn set_event_bus(&self, bus: Arc<EventBus>) {
        *self.event_bus.lock().unwrap() = Some(bus);
    }

    /// Provide a topic-to-device-id resolver. Must be called before `start()`.
    ///
    /// When set, the broker observes `MessagePublish` events to learn the
    /// mapping from MQTT client_id to NeoMind device_id. This is necessary
    /// when devices use an MQTT client_id different from their registered
    /// device_id (common for cameras that ship with a hardcoded client_id).
    /// Without it, transport connect/disconnect events fire for an unknown
    /// device_id (the raw client_id), and the frontend can't correlate them
    /// with registered devices.
    pub fn set_topic_resolver(&self, resolver: TopicResolverFn) {
        *self.topic_resolver.lock().unwrap() = Some(resolver);
    }

    /// Check if the broker is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get the broker configuration
    pub fn config(&self) -> EmbeddedBrokerConfig {
        self.config.lock().unwrap().clone()
    }

    /// Update the auth_enabled flag dynamically.
    /// Takes effect immediately on new connections — no broker restart needed.
    pub fn set_auth_enabled(&self, enabled: bool) {
        let old = self.auth_enabled.swap(enabled, Ordering::Relaxed);
        if old != enabled {
            tracing::info!(
                "Embedded broker auth_enabled changed: {} -> {}",
                old,
                enabled
            );
        }
    }

    /// Stop the broker by aborting the server task.
    ///
    /// rmqtt's server.run() is a pure tokio async future, so aborting
    /// the spawned task immediately cancels it and releases the port.
    pub async fn stop(&self) -> Result<(), EmbeddedBrokerError> {
        if !self.is_running() {
            return Ok(());
        }

        tracing::info!("Stopping embedded MQTT broker...");

        if let Some(handle) = self.abort_handle.lock().unwrap().take() {
            handle.abort();
        }

        self.running.store(false, Ordering::Relaxed);
        tracing::info!("Embedded MQTT broker stopped");
        Ok(())
    }

    /// Start the embedded broker as a tokio task.
    ///
    /// Registers the auth hook that reads auth_enabled from the shared AtomicBool.
    /// Supports both TCP and TLS listeners based on configuration.
    pub async fn start(&self) -> Result<(), EmbeddedBrokerError> {
        if self.is_running() {
            tracing::warn!("Embedded broker is already running");
            return Ok(());
        }

        let config = self.config.lock().unwrap().clone();

        // If port is in use, wait briefly for it to be released
        if check_port_sync(config.port) {
            tracing::info!("Port {} still in use, waiting for release...", config.port);
            let wait_start = std::time::Instant::now();
            let max_wait = std::time::Duration::from_secs(5);
            loop {
                if !check_port_async(config.port).await {
                    break;
                }
                if wait_start.elapsed() >= max_wait {
                    return Err(EmbeddedBrokerError::Broker(format!(
                        "Port {} still in use after 5s",
                        config.port
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            // Extra delay after port appears free — OS may still be cleaning up the socket
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }

        let addr = config.socket_addr()?;

        // Build rmqtt server
        let scx = rmqtt::context::ServerContext::new().build().await;

        // Register auth hook with shared state
        let auth_handler = NeoMindAuthHandler {
            auth_enabled: self.auth_enabled.clone(),
            credential_validator: self.credential_validator.clone(),
        };
        let reg = scx.extends.hook_mgr().register();
        reg.add(
            rmqtt::hook::Type::ClientAuthenticate,
            Box::new(auth_handler),
        )
        .await;

        // Register presence hook (transport-level connect/disconnect) if an
        // EventBus was provided. This is what makes "device connected to MQTT
        // but hasn't published yet" show up correctly in the UI instead of
        // appearing as "Never Connected".
        let presence_bus = self.event_bus.lock().unwrap().clone();
        if let Some(bus) = presence_bus {
            // Shared cache: MQTT client_id → NeoMind device_id, learned from
            // observed publishes. All three hook instances (connect/disconnect/
            // publish) share this Arc so a single publish teaches the broker
            // the correct device_id for all future transport events.
            let client_id_cache: Arc<RwLock<HashMap<String, String>>> =
                Arc::new(RwLock::new(HashMap::new()));
            let topic_resolver = self.topic_resolver.lock().unwrap().clone();

            let presence_handler =
                DevicePresenceHook::new(bus, client_id_cache.clone(), topic_resolver.clone());
            reg.add(
                rmqtt::hook::Type::ClientConnected,
                Box::new(presence_handler),
            )
            .await;
            // rmqtt shares Handler instances across hook Types registered with
            // the same `reg`, so we register the same handler for the
            // disconnect hook too. We need a second boxed instance because
            // `reg.add` takes ownership of the Box.
            let presence_handler_disc = DevicePresenceHook::new(
                self.event_bus
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("bus re-acquired"),
                client_id_cache.clone(),
                topic_resolver.clone(),
            );
            reg.add(
                rmqtt::hook::Type::ClientDisconnected,
                Box::new(presence_handler_disc),
            )
            .await;
            // Register MessagePublish hook to learn client_id → device_id
            // mappings from observed publish topics. Only effective when
            // set_topic_resolver was called with a resolver; otherwise the
            // hook is a no-op (resolver is None).
            if topic_resolver.is_some() {
                let presence_handler_pub = DevicePresenceHook::new(
                    self.event_bus
                        .lock()
                        .unwrap()
                        .clone()
                        .expect("bus re-acquired"),
                    client_id_cache.clone(),
                    topic_resolver.clone(),
                );
                reg.add(
                    rmqtt::hook::Type::MessagePublish,
                    Box::new(presence_handler_pub),
                )
                .await;
                tracing::info!(
                    "Embedded broker: DevicePresenceHook registered with topic resolver (transport events + client_id learning)"
                );
            } else {
                tracing::info!(
                    "Embedded broker: DevicePresenceHook registered (transport events only; set_topic_resolver for client_id → device_id mapping)"
                );
            }
        }

        reg.start().await;

        tracing::info!(
            "MQTT Broker Listening on neomind-broker {} (auth_enabled={})",
            addr,
            self.auth_enabled.load(Ordering::Relaxed)
        );

        // Build listener
        // allow_anonymous=false forces rmqtt to always call our auth hook,
        // even for anonymous connections. Our hook dynamically decides
        // whether to allow based on the shared auth_enabled flag.
        let builder = rmqtt::net::Builder::new()
            .name("neomind-broker")
            .laddr(addr)
            .reuseaddr(Some(true))
            .allow_anonymous(false);

        let listener = if config.tls_enabled {
            let cert_path = config.tls_cert_path.as_deref().ok_or_else(|| {
                EmbeddedBrokerError::Config("TLS certificate path required".to_string())
            })?;
            let key_path = config
                .tls_key_path
                .as_deref()
                .ok_or_else(|| EmbeddedBrokerError::Config("TLS key path required".to_string()))?;

            // Verify cert and key files exist and are readable before passing to rmqtt
            if !std::path::Path::new(cert_path).exists() {
                return Err(EmbeddedBrokerError::Config(format!(
                    "TLS certificate file not found: {}",
                    cert_path
                )));
            }
            if !std::path::Path::new(key_path).exists() {
                return Err(EmbeddedBrokerError::Config(format!(
                    "TLS key file not found: {}",
                    key_path
                )));
            }

            tracing::info!("TLS enabled with cert: {}, key: {}", cert_path, key_path);
            builder
                .tls_cert(Some(cert_path.to_string()))
                .tls_key(Some(key_path.to_string()))
                .bind()
                .map_err(|e| EmbeddedBrokerError::Broker(format!("TLS bind failed: {}", e)))?
                .tls()
                .map_err(|e| EmbeddedBrokerError::Broker(format!("TLS setup failed: {}", e)))?
        } else {
            builder
                .bind()
                .map_err(|e| EmbeddedBrokerError::Broker(format!("TCP bind failed: {}", e)))?
                .tcp()
                .map_err(|e| EmbeddedBrokerError::Broker(format!("TCP setup failed: {}", e)))?
        };

        let server = rmqtt::server::MqttServer::new(scx)
            .listener(listener)
            .build();

        self.running.store(true, Ordering::Relaxed);

        // Spawn server.run() as a tokio task and save the abort handle
        let handle = tokio::spawn(async move {
            if let Err(e) = server.run().await {
                tracing::error!("Embedded MQTT broker error: {}", e);
            }
        });

        *self.abort_handle.lock().unwrap() = Some(handle.abort_handle());

        // Wait for the broker to become ready
        let port = config.port;
        let max_wait = std::time::Duration::from_secs(5);
        let start = std::time::Instant::now();
        loop {
            if check_port_async(port).await {
                break;
            }
            if start.elapsed() >= max_wait {
                self.running.store(false, Ordering::Relaxed);
                return Err(EmbeddedBrokerError::Broker(
                    "Broker failed to start within 5s".to_string(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        tracing::info!(
            "Embedded broker started on port {} (auth_enabled read dynamically from config)",
            config.port,
        );
        Ok(())
    }
}

/// Check if a broker is listening on the given port.
pub fn is_broker_running(port: u16) -> bool {
    check_port_sync(port)
}

fn check_port_sync(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, TcpStream};
    let addr = (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    TcpStream::connect_timeout(&addr.into(), std::time::Duration::from_millis(200)).is_ok()
}

async fn check_port_async(port: u16) -> bool {
    tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EmbeddedBrokerConfig::default();
        assert_eq!(config.listen, "0.0.0.0");
        assert_eq!(config.port, 1883);
        assert_eq!(config.max_connections, 1000);
        assert!(!config.auth_enabled);
    }

    #[test]
    fn test_config_builder() {
        let config = EmbeddedBrokerConfig::new()
            .with_port(8883)
            .with_listen("127.0.0.1")
            .with_max_connections(500);
        assert_eq!(config.port, 8883);
        assert_eq!(config.listen, "127.0.0.1");
        assert_eq!(config.max_connections, 500);
    }

    #[test]
    fn test_socket_addr() {
        let config = EmbeddedBrokerConfig::new()
            .with_port(1883)
            .with_listen("0.0.0.0");
        let addr = config.socket_addr().expect("Failed to get socket address");
        assert_eq!(addr.port(), 1883);
        assert_eq!(addr.ip(), std::net::Ipv4Addr::new(0, 0, 0, 0));
    }
}
