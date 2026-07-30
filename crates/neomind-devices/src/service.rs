//! Device Service - Unified service layer for device operations
//!
//! This service provides a high-level API for:
//! - Device registration and management
//! - Command sending (automatically uses templates)
//! - Data querying
//! - Integration with adapters and telemetry storage

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use super::adapter::{ConnectionStatus, DeviceAdapter};
use super::mdl::{DeviceError, MetricValue};
use super::registry::{DeviceConfig, DeviceRegistry, DeviceTypeTemplate};
use super::telemetry::TimeSeriesStorage;
use neomind_core::EventBus;
use std::sync::atomic::{AtomicU64, Ordering};

/// Callback type for routing commands to extensions.
/// Receives (extension_id, device_id, command_name, params) and returns Ok(()) on success.
pub type ExtensionCommandRouter =
    dyn Fn(
        String,                             // extension_id
        String,                             // device_id
        String,                             // command_name
        HashMap<String, serde_json::Value>, // params
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;

/// Callback type for routing commands to extensions, Send + Sync version.
pub type ExtensionCommandRouterFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            HashMap<String, serde_json::Value>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

// Import storage types for command history persistence
use neomind_storage::device_registry::{
    CommandHistoryRecord as StorageCommandRecord, CommandStatus as StorageCommandStatus,
};

/// Command history record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandHistoryRecord {
    /// Unique command ID
    pub command_id: String,
    /// Device ID
    pub device_id: String,
    /// Command name
    pub command_name: String,
    /// Command parameters
    pub parameters: HashMap<String, serde_json::Value>,
    /// Command status
    pub status: CommandStatus,
    /// Result message (if available)
    pub result: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Timestamp when command was created
    pub created_at: i64,
    /// Timestamp when command completed (if applicable)
    pub completed_at: Option<i64>,
}

/// Command execution status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum CommandStatus {
    /// Command is pending execution
    Pending,
    /// Command is currently executing
    Executing,
    /// Command completed successfully
    Success,
    /// Command failed
    Failed,
    /// Command timed out
    Timeout,
}

impl CommandStatus {
    /// Check if command is a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Timeout)
    }

    /// Convert to storage command status
    fn to_storage(&self) -> StorageCommandStatus {
        match self {
            Self::Pending => StorageCommandStatus::Pending,
            Self::Executing => StorageCommandStatus::Sent,
            Self::Success => StorageCommandStatus::Completed,
            Self::Failed => StorageCommandStatus::Failed,
            Self::Timeout => StorageCommandStatus::Timeout,
        }
    }

    /// Convert from storage command status
    fn from_storage(status: StorageCommandStatus) -> Self {
        match status {
            StorageCommandStatus::Pending => Self::Pending,
            StorageCommandStatus::Sent => Self::Executing,
            StorageCommandStatus::Completed => Self::Success,
            StorageCommandStatus::Failed => Self::Failed,
            StorageCommandStatus::Timeout => Self::Timeout,
        }
    }
}

/// Convert local command record to storage format
fn command_to_storage(record: &CommandHistoryRecord) -> StorageCommandRecord {
    StorageCommandRecord {
        command_id: record.command_id.clone(),
        device_id: record.device_id.clone(),
        command_name: record.command_name.clone(),
        parameters: record.parameters.clone(),
        status: record.status.to_storage(),
        result: record.result.clone(),
        error: record.error.clone(),
        created_at: record.created_at,
        completed_at: record.completed_at,
    }
}

/// Convert storage command record to local format
fn command_from_storage(record: StorageCommandRecord) -> CommandHistoryRecord {
    CommandHistoryRecord {
        command_id: record.command_id,
        device_id: record.device_id,
        command_name: record.command_name,
        parameters: record.parameters,
        status: CommandStatus::from_storage(record.status),
        result: record.result,
        error: record.error,
        created_at: record.created_at,
        completed_at: record.completed_at,
    }
}

/// Adapter information for API responses.
///
/// This provides a simplified view of adapter state without the plugin system overhead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterInfo {
    /// Adapter ID
    pub id: String,
    /// Adapter name
    pub name: String,
    /// Adapter type (mqtt, http, webhook, etc.)
    pub adapter_type: String,
    /// Whether the adapter is running
    pub running: bool,
    /// Number of devices managed by this adapter
    pub device_count: usize,
    /// Connection status
    pub status: String,
    /// Last activity timestamp
    pub last_activity: i64,
}

/// Aggregated adapter statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterStats {
    /// Total number of adapters
    pub total_adapters: usize,
    /// Number of running adapters
    pub running_adapters: usize,
    /// Total number of devices across all adapters
    pub total_devices: usize,
    /// Per-adapter information
    pub adapters: Vec<AdapterInfo>,
}

/// Device status information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceStatus {
    /// Current connection status (legacy field; derived from data activity).
    /// Kept for backward compatibility. New code should prefer
    /// `transport_connected` + `last_seen` for accurate 4-state status.
    pub status: ConnectionStatus,
    /// Last data-activity timestamp (when the device last reported a metric).
    pub last_seen: i64,
    /// Adapter that manages this device
    pub adapter_id: Option<String>,
    /// Transport-layer (MQTT session) connected flag.
    /// Set by the rmqtt `client_connected`/`client_disconnected` hooks or by
    /// the external-broker `$SYS` topic listener. Decoupled from `last_seen`
    /// so a connected-but-idle device can be distinguished from a truly
    /// offline one. Older clients that don't send this field default to
    /// `false`, which preserves legacy behavior (status derived from data).
    #[serde(default)]
    pub transport_connected: bool,
    /// Timestamp of the last `transport_connected` state change. Useful for
    /// diagnosing "connected but no data" scenarios.
    #[serde(default)]
    pub transport_changed_at: i64,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            // Use 0 as default to indicate "never seen" - this ensures proper
            // offline detection for devices that haven't sent any metrics yet
            last_seen: 0,
            adapter_id: None,
            transport_connected: false,
            transport_changed_at: 0,
        }
    }
}

/// Heartbeat configuration for device health monitoring
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Heartbeat interval in seconds (default: 60)
    pub heartbeat_interval: u64,
    /// Device offline timeout in seconds (default: 300 = 5 minutes)
    pub offline_timeout: u64,
    /// Whether to automatically mark stale devices as offline
    pub auto_mark_offline: bool,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: 60,
            offline_timeout: 300,
            auto_mark_offline: true,
        }
    }
}

impl HeartbeatConfig {
    /// Create a new heartbeat configuration
    pub fn new(interval_secs: u64, timeout_secs: u64) -> Self {
        Self {
            heartbeat_interval: interval_secs,
            offline_timeout: timeout_secs,
            auto_mark_offline: true,
        }
    }

    /// Get the interval as Duration
    pub fn interval_duration(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval)
    }

    /// Check if a device is stale based on last_seen timestamp.
    ///
    /// # Deprecated
    /// Uses the GLOBAL `offline_timeout` and silently ignores per-device overrides
    /// and template defaults. Callers MUST use
    /// `DeviceService::effective_offline_timeout(device_id)` instead, which resolves
    /// the correct timeout via: device override > template default > global.
    #[deprecated(
        since = "0.8.18",
        note = "use DeviceService::effective_offline_timeout() instead"
    )]
    pub fn is_stale(&self, last_seen: i64) -> bool {
        let now = chrono::Utc::now().timestamp();
        let elapsed = (now - last_seen) as u64;
        elapsed > self.offline_timeout
    }
}

impl DeviceStatus {
    /// Create a new device status
    pub fn new(status: ConnectionStatus) -> Self {
        Self {
            status,
            last_seen: chrono::Utc::now().timestamp(),
            adapter_id: None,
            transport_connected: false,
            transport_changed_at: 0,
        }
    }

    /// Update the status and timestamp
    pub fn update(&mut self, status: ConnectionStatus) {
        self.status = status;
        self.last_seen = chrono::Utc::now().timestamp();
    }

    /// Check if device is connected within a configurable timeout window.
    /// Returns true only if status is Connected AND last_seen was within `timeout_secs`.
    pub fn is_connected_within(&self, timeout_secs: u64) -> bool {
        if !matches!(self.status, ConnectionStatus::Connected) {
            return false;
        }
        let now = chrono::Utc::now().timestamp();
        let elapsed = now - self.last_seen;
        elapsed < timeout_secs as i64
    }
}

/// Device Service
/// Provides unified interface for device operations
pub struct DeviceService {
    /// Device registry for templates and configurations
    registry: Arc<DeviceRegistry>,
    /// Event bus for publishing events
    event_bus: EventBus,
    /// Telemetry storage (optional)
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Adapter registry for command sending (plugin registry will provide this)
    /// This will be populated by device adapters that register themselves
    adapters: Arc<RwLock<HashMap<String, Arc<dyn DeviceAdapter>>>>,
    /// Device status cache (device_id -> DeviceStatus)
    device_status: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// Command history (device_id -> Vec<CommandHistoryRecord>)
    command_history: Arc<RwLock<HashMap<String, Vec<CommandHistoryRecord>>>>,
    /// Command ID counter
    command_id_counter: Arc<AtomicU64>,
    /// Maximum history entries per device
    max_history_entries: usize,
    /// Heartbeat configuration
    heartbeat_config: HeartbeatConfig,
    /// Whether heartbeat monitoring is running
    heartbeat_running: Arc<RwLock<bool>>,
    /// Extension command router for extension-registered devices
    /// When set, commands for devices with adapter_type="extension" are routed through this callback
    extension_command_router: Arc<RwLock<Option<ExtensionCommandRouterFn>>>,
}

impl DeviceService {
    /// Create a new device service
    pub fn new(registry: Arc<DeviceRegistry>, event_bus: EventBus) -> Self {
        Self {
            registry,
            event_bus,
            telemetry_storage: Arc::new(RwLock::new(None)),
            adapters: Arc::new(RwLock::new(HashMap::new())),
            device_status: Arc::new(RwLock::new(HashMap::new())),
            command_history: Arc::new(RwLock::new(HashMap::new())),
            command_id_counter: Arc::new(AtomicU64::new(1)),
            max_history_entries: 100, // Keep last 100 commands per device
            heartbeat_config: HeartbeatConfig::default(),
            heartbeat_running: Arc::new(RwLock::new(false)),
            extension_command_router: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new device service with custom heartbeat configuration
    pub fn with_heartbeat(
        registry: Arc<DeviceRegistry>,
        event_bus: EventBus,
        heartbeat_config: HeartbeatConfig,
    ) -> Self {
        Self {
            registry,
            event_bus,
            telemetry_storage: Arc::new(RwLock::new(None)),
            adapters: Arc::new(RwLock::new(HashMap::new())),
            device_status: Arc::new(RwLock::new(HashMap::new())),
            command_history: Arc::new(RwLock::new(HashMap::new())),
            command_id_counter: Arc::new(AtomicU64::new(1)),
            max_history_entries: 100,
            heartbeat_config,
            heartbeat_running: Arc::new(RwLock::new(false)),
            extension_command_router: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the extension command router for routing commands to extensions
    pub async fn set_extension_command_router(&self, router: ExtensionCommandRouterFn) {
        let mut r = self.extension_command_router.write().await;
        *r = Some(router);
    }

    /// Set heartbeat configuration
    pub fn set_heartbeat_config(&mut self, config: HeartbeatConfig) {
        self.heartbeat_config = config;
    }

    /// Get current heartbeat configuration
    pub fn heartbeat_config(&self) -> &HeartbeatConfig {
        &self.heartbeat_config
    }

    /// Resolve the effective offline timeout (seconds) for a specific device.
    ///
    /// Priority order (highest first):
    /// 1. Per-device override (`DeviceConfig::offline_timeout_secs`)
    /// 2. Template default (`DeviceTypeTemplate::default_offline_timeout_secs`)
    /// 3. Global `HeartbeatConfig::offline_timeout`
    ///
    /// This is the canonical resolution used by API handlers when building
    /// `DeviceDto.online` and any consumer-visible "online within" check.
    pub fn effective_offline_timeout(&self, device_id: &str) -> u64 {
        // Global fallback: user-configured DeviceDefaults (live), or default 300s
        let global = neomind_storage::DeviceDefaults::get().default_offline_timeout_secs;

        // Try per-device override
        if let Some(device) = self.registry.get_device(device_id) {
            if let Some(secs) = device.offline_timeout_secs {
                return secs;
            }
            // Try template default
            if let Some(template) = self.registry.get_template(&device.device_type) {
                if let Some(secs) = template.default_offline_timeout_secs {
                    return secs;
                }
            }
        }
        global
    }

    /// Start the device service - listens for device events and updates status
    /// Also loads command history from storage if available
    pub async fn start(&self) {
        tracing::info!("DeviceService::start() called - subscribing to EventBus events");

        // Load command history from storage if available
        self.load_command_history_from_storage()
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load command history from storage: {}", e);
            });

        // Migrate last_seen for old devices that have telemetry data but last_seen == 1 (sentinel)
        self.migrate_last_seen_from_telemetry().await;

        let event_bus = self.event_bus.clone();
        let device_status = self.device_status.clone();
        let telemetry_storage = self.telemetry_storage.clone();
        let registry = self.registry.clone();

        tokio::spawn(async move {
            let event_bus_for_publish = event_bus.clone();
            // Filter for device-related events
            let filter = |event: &neomind_core::NeoMindEvent| -> bool {
                matches!(
                    event,
                    neomind_core::NeoMindEvent::DeviceOnline { .. }
                        | neomind_core::NeoMindEvent::DeviceOffline { .. }
                        | neomind_core::NeoMindEvent::DeviceTransportOnline { .. }
                        | neomind_core::NeoMindEvent::DeviceTransportOffline { .. }
                        | neomind_core::NeoMindEvent::DeviceMetric { .. }
                )
            };
            let mut rx = event_bus.subscribe_filtered(filter);
            while let Some((event, _)) = rx.recv().await {
                match event {
                    neomind_core::NeoMindEvent::DeviceOnline { device_id, .. } => {
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.update(ConnectionStatus::Connected);
                    }
                    neomind_core::NeoMindEvent::DeviceOffline { device_id, .. } => {
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.update(ConnectionStatus::Disconnected);
                    }
                    neomind_core::NeoMindEvent::DeviceTransportOnline {
                        device_id,
                        timestamp,
                        ..
                    } => {
                        // Skip unknown clients (e.g., MQTT tools, test clients)
                        // to avoid creating phantom status entries
                        if registry.get_device(&device_id).is_none() {
                            continue;
                        }
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.transport_connected = true;
                        entry.transport_changed_at = timestamp;
                        tracing::debug!(
                            "Device {} transport session online (t={})",
                            device_id,
                            timestamp
                        );
                    }
                    neomind_core::NeoMindEvent::DeviceTransportOffline {
                        device_id,
                        timestamp,
                        ..
                    } => {
                        if registry.get_device(&device_id).is_none() {
                            continue;
                        }
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.transport_connected = false;
                        entry.transport_changed_at = timestamp;
                        tracing::debug!(
                            "Device {} transport session offline (t={})",
                            device_id,
                            timestamp
                        );
                    }
                    neomind_core::NeoMindEvent::DeviceMetric {
                        device_id,
                        metric,
                        value,
                        timestamp,
                        quality: _,
                        is_virtual,
                        ..
                    } => {
                        // Skip virtual metrics for device status tracking.
                        // Virtual metrics come from extensions/transforms, not real devices.
                        // We still write them to telemetry storage below.
                        if !neomind_core::NeoMindEvent::is_virtual_device_metric(
                            is_virtual, &metric,
                        ) {
                            let mut status = device_status.write().await;
                            let entry = status.entry(device_id.clone()).or_default();
                            let now_ts = chrono::Utc::now().timestamp();
                            entry.last_seen = now_ts;
                            // If status was disconnected, mark as connected
                            if entry.status == ConnectionStatus::Disconnected {
                                entry.status = ConnectionStatus::Connected;
                                tracing::info!(
                                    "Device {} marked as connected due to metric activity",
                                    device_id
                                );
                                drop(status);
                                // Publish DeviceOnline event so frontend can refresh
                                let device_type = "_unknown".to_string(); // Will be looked up by frontend
                                event_bus_for_publish
                                    .publish(neomind_core::NeoMindEvent::DeviceOnline {
                                        device_id: device_id.clone(),
                                        device_type,
                                        timestamp: chrono::Utc::now().timestamp(),
                                    })
                                    .await;
                            } else {
                                drop(status);
                            }
                            // Persist last_seen to registry (redb) for server-restart survival
                            registry.update_last_seen(&device_id, now_ts).await;
                        }

                        // Write to telemetry storage if available.
                        // Use the event's timestamp (not Utc::now()) so we don't create duplicate
                        // data points. Adapters (MQTT, HTTP, Webhook) already write with their
                        // receive time; using the same timestamp here causes the second write to
                        // overwrite the first (same key), avoiding duplicate entries with ~2s gap.
                        let ts_storage = telemetry_storage.read().await;
                        if let Some(storage) = ts_storage.as_ref() {
                            // Convert core MetricValue to devices MetricValue
                            let metric_value: MetricValue = match &value {
                                neomind_core::MetricValue::Integer(i) => MetricValue::Integer(*i),
                                neomind_core::MetricValue::Float(f) => MetricValue::Float(*f),
                                neomind_core::MetricValue::String(s) => {
                                    MetricValue::String(s.clone())
                                }
                                neomind_core::MetricValue::Boolean(b) => MetricValue::Boolean(*b),
                                neomind_core::MetricValue::Json(j) => {
                                    // Try to convert JSON to appropriate type
                                    if let Some(n) = j.as_i64() {
                                        MetricValue::Integer(n)
                                    } else if let Some(f) = j.as_f64() {
                                        MetricValue::Float(f)
                                    } else if let Some(s) = j.as_str() {
                                        MetricValue::String(s.to_string())
                                    } else if let Some(b) = j.as_bool() {
                                        MetricValue::Boolean(b)
                                    } else {
                                        MetricValue::String(j.to_string())
                                    }
                                }
                            };

                            let data_point = super::telemetry::DataPoint {
                                timestamp,
                                value: metric_value,
                                quality: None,
                            };

                            if let Err(e) = storage
                                .write(&format!("device:{}", device_id), &metric, data_point)
                                .await
                            {
                                tracing::warn!("Failed to write telemetry to storage: {}", e);
                            }
                        } else {
                            tracing::warn!(
                                "DeviceService telemetry_storage is None, cannot write metric {} for device {}",
                                metric,
                                device_id
                            );
                        }
                    }
                    _ => {}
                }
            }
        });

        // Start heartbeat monitoring task
        self.start_heartbeat_monitor();
    }

    /// Start the heartbeat monitoring task
    fn start_heartbeat_monitor(&self) {
        let device_status = self.device_status.clone();
        let event_bus = self.event_bus.clone();
        let heartbeat_config = self.heartbeat_config.clone();
        let heartbeat_running = self.heartbeat_running.clone();
        let registry = self.registry.clone();

        tokio::spawn(async move {
            // Mark heartbeat as running
            *heartbeat_running.write().await = true;

            let mut timer = interval(heartbeat_config.interval_duration());
            timer.tick().await; // Skip first tick

            loop {
                timer.tick().await;

                let config = heartbeat_config.clone();
                if !config.auto_mark_offline {
                    continue;
                }

                let now = chrono::Utc::now().timestamp();
                let mut stale_devices = Vec::new();

                // Check for stale devices
                // IMPORTANT: We must iterate over ALL registered devices from the registry,
                // not just devices in the status map. This ensures devices that were registered
                // but haven't sent any metrics yet are also monitored for stale status.
                {
                    // First, get all registered device IDs from the registry
                    let registered_devices = registry.list_devices();

                    // Precompute effective timeouts OUTSIDE the write lock to minimize
                    // contention with event handlers that also need status_map access.
                    let device_checks: Vec<(_, u64)> = registered_devices
                        .iter()
                        .map(|dc| {
                            let timeout = {
                                let mut t = config.offline_timeout;
                                if let Some(secs) = dc.offline_timeout_secs {
                                    t = secs;
                                } else if let Some(tpl) = registry.get_template(&dc.device_type) {
                                    if let Some(secs) = tpl.default_offline_timeout_secs {
                                        t = secs;
                                    }
                                }
                                t
                            };
                            (dc.clone(), timeout)
                        })
                        .collect();

                    // Acquire write lock only for status map operations
                    let mut status_map = device_status.write().await;

                    for (device_config, effective_timeout) in device_checks {
                        let device_id = &device_config.device_id;

                        // Get or create status entry for this device
                        // Devices without status entries are considered "never seen" (last_seen = 0)
                        let status = status_map.entry(device_id.clone()).or_insert_with(|| {
                            tracing::debug!(
                                "Creating initial status entry for registered device '{}' during heartbeat check",
                                device_id
                            );
                            DeviceStatus {
                                status: ConnectionStatus::Disconnected,
                                last_seen: 0, // Never seen - very old timestamp to trigger offline
                                adapter_id: device_config.adapter_id.clone(),
                                transport_connected: false,
                                transport_changed_at: 0,
                            }
                        });

                        let elapsed = now - status.last_seen;
                        if matches!(status.status, ConnectionStatus::Connected)
                            && elapsed > effective_timeout as i64
                            && !status.transport_connected
                        {
                            // MQTT session still alive but no data → keep Connected so frontend
                            // can render `connectedIdle`. Only collapse to Offline when transport
                            // itself is gone.
                            stale_devices.push((device_id.clone(), status.last_seen));
                        } else if status.last_seen == 0 {
                            // Device was never seen - mark as disconnected
                            // But don't emit events for never-seen devices to avoid noise
                            status.status = ConnectionStatus::Disconnected;
                        }
                    }
                }

                // Mark stale devices as offline.
                //
                // CRITICAL: re-check each device's current state before marking
                // it offline. Between the scan phase (above) and this mark
                // phase, a DeviceMetric event may have arrived and set the
                // device back to Connected with a fresh last_seen. Without
                // this re-check, we'd overwrite the fresh Connected status
                // with Disconnected and fire a spurious DeviceOffline event
                // — the frontend list would show offline despite just having
                // received data.
                for (device_id, stale_last_seen) in stale_devices {
                    // Re-check under lock: skip if the device has received
                    // fresh data since the scan phase.
                    {
                        let status_map = device_status.read().await;
                        if let Some(entry) = status_map.get(&device_id) {
                            if entry.last_seen > stale_last_seen {
                                tracing::info!(
                                    "Device {} received fresh data after stale scan (last_seen {} > {}), skipping offline mark",
                                    device_id, entry.last_seen, stale_last_seen
                                );
                                continue;
                            }
                        }
                    }

                    let elapsed = now - stale_last_seen;
                    tracing::info!(
                        "Device {} is stale (last seen {}s ago), marking as offline",
                        device_id,
                        elapsed
                    );

                    // Update status
                    {
                        let mut status_map = device_status.write().await;
                        if let Some(entry) = status_map.get_mut(&device_id) {
                            entry.status = ConnectionStatus::Disconnected;
                        }
                    }

                    // Publish offline event with reason
                    let _ = event_bus
                        .publish(neomind_core::NeoMindEvent::DeviceOffline {
                            device_id: device_id.clone(),
                            reason: Some(format!(
                                "Heartbeat timeout: no activity for {} seconds",
                                elapsed
                            )),
                            timestamp: now,
                        })
                        .await;
                }
            }
        });
    }

    /// Check if a device is currently stale (hasn't been seen within timeout)
    /// Uses per-device effective timeout (device > template > global).
    pub async fn is_device_stale(&self, device_id: &str) -> bool {
        let status_map = self.device_status.read().await;
        if let Some(status) = status_map.get(device_id) {
            let effective_timeout = self.effective_offline_timeout(device_id);
            let elapsed = (chrono::Utc::now().timestamp() - status.last_seen) as u64;
            elapsed > effective_timeout
        } else {
            // Unknown devices are considered stale
            true
        }
    }

    /// Get health status for all devices
    pub async fn get_device_health(&self) -> HashMap<String, DeviceHealth> {
        let status_map = self.device_status.read().await;
        let now = chrono::Utc::now().timestamp();

        status_map
            .iter()
            .map(|(device_id, status)| {
                let elapsed = now - status.last_seen;
                let effective_timeout = self.effective_offline_timeout(device_id) as i64;
                let is_stale = elapsed > effective_timeout;
                let health_score = if is_stale {
                    0
                } else if elapsed < effective_timeout / 3 {
                    100
                } else if elapsed < (effective_timeout * 2) / 3 {
                    50
                } else {
                    25
                };

                (
                    device_id.clone(),
                    DeviceHealth {
                        device_id: device_id.clone(),
                        status: status.status,
                        last_seen: status.last_seen,
                        elapsed_since_last_seen: elapsed,
                        is_stale,
                        health_score,
                    },
                )
            })
            .collect()
    }

    /// Stop the heartbeat monitoring
    pub async fn stop_heartbeat(&self) {
        *self.heartbeat_running.write().await = false;
    }
}

/// Device health information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceHealth {
    /// Device ID
    pub device_id: String,
    /// Current connection status
    pub status: ConnectionStatus,
    /// Last activity timestamp
    pub last_seen: i64,
    /// Seconds since last activity
    pub elapsed_since_last_seen: i64,
    /// Whether device is considered stale
    pub is_stale: bool,
    /// Health score (0-100)
    pub health_score: u8,
}

impl DeviceService {
    /// Migrate last_seen for old devices: if last_seen == 1 (sentinel from registry migration),
    /// try to get the real timestamp from telemetry storage.
    async fn migrate_last_seen_from_telemetry(&self) {
        let devices_to_migrate: Vec<(String, String)> = self
            .registry
            .list_devices()
            .into_iter()
            .filter(|d| d.last_seen == 1)
            .map(|d| (d.device_id.clone(), d.device_type.clone()))
            .collect();

        if devices_to_migrate.is_empty() {
            return;
        }

        let telemetry_storage = self.telemetry_storage.read().await;
        let Some(storage) = telemetry_storage.as_ref() else {
            return;
        };

        tracing::info!(
            "Migrating last_seen from telemetry for {} devices",
            devices_to_migrate.len()
        );

        for (device_id, _device_type) in devices_to_migrate {
            // Get metrics for this device, then query the latest data point of the first metric
            match storage.list_metrics(&format!("device:{}", device_id)).await {
                Ok(metrics) if !metrics.is_empty() => {
                    // Pick the first metric and get its latest data point
                    if let Ok(Some(latest)) = storage
                        .latest(&format!("device:{}", device_id), &metrics[0])
                        .await
                    {
                        let ts = latest.timestamp;
                        tracing::debug!(
                            "Migrating last_seen for {} → {} from telemetry",
                            device_id,
                            ts
                        );
                        self.registry.update_last_seen(&device_id, ts).await;
                    }
                }
                _ => {
                    // No telemetry data — keep last_seen = 1 (treat as previously seen)
                }
            }
        }
    }

    /// Set telemetry storage
    pub async fn set_telemetry_storage(&self, storage: Arc<TimeSeriesStorage>) {
        *self.telemetry_storage.write().await = Some(storage);
    }

    /// Register an adapter for command sending
    pub async fn register_adapter(&self, adapter_id: String, adapter: Arc<dyn DeviceAdapter>) {
        // Set telemetry storage for the adapter if available
        let telemetry_storage = self.telemetry_storage.read().await;
        if let Some(storage) = telemetry_storage.as_ref() {
            tracing::info!("Setting telemetry storage for adapter '{}'", adapter_id);
            adapter.set_telemetry_storage(storage.clone());
        } else {
            tracing::warn!(
                "Cannot set telemetry storage for adapter '{}': DeviceService telemetry_storage is None",
                adapter_id
            );
        }
        drop(telemetry_storage);

        let mut adapters = self.adapters.write().await;
        adapters.insert(adapter_id, adapter);
    }

    /// Unregister an adapter
    pub async fn unregister_adapter(&self, adapter_id: &str) {
        let mut adapters = self.adapters.write().await;
        adapters.remove(adapter_id);
    }

    /// Get an adapter by ID
    pub async fn get_adapter(&self, adapter_id: &str) -> Option<Arc<dyn DeviceAdapter>> {
        let adapters = self.adapters.read().await;
        adapters.get(adapter_id).cloned()
    }

    /// List all registered adapter IDs
    pub async fn list_adapter_ids(&self) -> Vec<String> {
        let adapters = self.adapters.read().await;
        adapters.keys().cloned().collect()
    }

    /// Get adapter information for a specific adapter
    pub async fn get_adapter_info(&self, adapter_id: &str) -> Option<AdapterInfo> {
        let adapters = self.adapters.read().await;
        adapters.get(adapter_id).map(|adapter| AdapterInfo {
            id: adapter_id.to_string(),
            name: adapter.name().to_string(),
            adapter_type: adapter.adapter_type().to_string(),
            running: adapter.is_running(),
            device_count: adapter.device_count(),
            status: format!("{:?}", adapter.connection_status()),
            last_activity: chrono::Utc::now().timestamp(),
        })
    }

    /// List all adapters with their information
    pub async fn list_adapters(&self) -> Vec<AdapterInfo> {
        let adapters = self.adapters.read().await;
        adapters
            .iter()
            .map(|(id, adapter)| AdapterInfo {
                id: id.clone(),
                name: adapter.name().to_string(),
                adapter_type: adapter.adapter_type().to_string(),
                running: adapter.is_running(),
                device_count: adapter.device_count(),
                status: format!("{:?}", adapter.connection_status()),
                last_activity: chrono::Utc::now().timestamp(),
            })
            .collect()
    }

    /// Get aggregated statistics for all adapters
    pub async fn get_adapter_stats(&self) -> AdapterStats {
        let adapters = self.list_adapters().await;
        let total_adapters = adapters.len();
        let running_adapters = adapters.iter().filter(|a| a.running).count();
        let total_devices: usize = adapters.iter().map(|a| a.device_count).sum();

        AdapterStats {
            total_adapters,
            running_adapters,
            total_devices,
            adapters,
        }
    }

    /// Get device IDs managed by a specific adapter
    pub async fn get_adapter_device_ids(&self, adapter_id: &str) -> Option<Vec<String>> {
        let adapters = self.adapters.read().await;
        adapters.get(adapter_id).map(|a| a.list_devices())
    }

    /// Start an adapter by ID
    pub async fn start_adapter(&self, adapter_id: &str) -> Result<(), DeviceError> {
        let adapters = self.adapters.read().await;
        if let Some(adapter) = adapters.get(adapter_id) {
            adapter
                .start()
                .await
                .map_err(|e| DeviceError::Communication(format!("Failed to start adapter: {}", e)))
        } else {
            Err(DeviceError::NotFoundStr(format!(
                "Adapter not found: {}",
                adapter_id
            )))
        }
    }

    /// Stop an adapter by ID
    pub async fn stop_adapter(&self, adapter_id: &str) -> Result<(), DeviceError> {
        let adapters = self.adapters.read().await;
        if let Some(adapter) = adapters.get(adapter_id) {
            adapter
                .stop()
                .await
                .map_err(|e| DeviceError::Communication(format!("Failed to stop adapter: {}", e)))
        } else {
            Err(DeviceError::NotFoundStr(format!(
                "Adapter not found: {}",
                adapter_id
            )))
        }
    }

    // ========== Template Management ==========

    /// Register a device type template
    pub async fn register_template(&self, template: DeviceTypeTemplate) -> Result<(), DeviceError> {
        let device_type = template.device_type.clone();
        self.registry.register_template(template).await?;

        // Publish event for UI refresh
        tokio::spawn({
            let event_bus = self.event_bus.clone();
            async move {
                let _ = event_bus
                    .publish(neomind_core::NeoMindEvent::Custom {
                        event_type: "DeviceTypeRegistered".to_string(),
                        data: serde_json::json!({
                            "device_type": device_type,
                            "timestamp": chrono::Utc::now().timestamp(),
                        }),
                    })
                    .await;
            }
        });

        Ok(())
    }

    /// Get a device type template
    pub fn get_template(&self, device_type: &str) -> Option<DeviceTypeTemplate> {
        self.registry.get_template(device_type)
    }

    /// List all device type templates
    pub fn list_templates(&self) -> Vec<DeviceTypeTemplate> {
        self.registry.list_templates()
    }

    /// Unregister a device type template
    pub async fn unregister_template(&self, device_type: &str) -> Result<(), DeviceError> {
        self.registry.unregister_template(device_type).await?;

        // Publish event for UI refresh
        tokio::spawn({
            let event_bus = self.event_bus.clone();
            let device_type = device_type.to_string();
            async move {
                let _ = event_bus
                    .publish(neomind_core::NeoMindEvent::Custom {
                        event_type: "DeviceTypeUnregistered".to_string(),
                        data: serde_json::json!({
                            "device_type": device_type,
                            "timestamp": chrono::Utc::now().timestamp(),
                        }),
                    })
                    .await;
            }
        });

        Ok(())
    }

    // ========== Device Configuration Management ==========

    /// Register a device configuration
    /// This will also notify the appropriate adapter to subscribe to the device's telemetry topic
    pub async fn register_device(&self, config: DeviceConfig) -> Result<(), DeviceError> {
        let device_id = config.device_id.clone();
        let device_type = config.device_type.clone();
        let adapter_type = config.adapter_type.clone();
        let target_adapter_id = config.adapter_id.clone();

        // First register the device in the registry
        self.registry.register_device(config).await?;

        // Initialize device status in the status map
        // This ensures the device is tracked for heartbeat monitoring
        {
            let mut status_map = self.device_status.write().await;
            // Only initialize if not already present (preserve existing status on re-register)
            if !status_map.contains_key(&device_id) {
                status_map.insert(
                    device_id.clone(),
                    DeviceStatus {
                        status: ConnectionStatus::Disconnected,
                        last_seen: chrono::Utc::now().timestamp(),
                        adapter_id: target_adapter_id.clone(),
                        transport_connected: false,
                        transport_changed_at: 0,
                    },
                );
                tracing::debug!(
                    "Initialized status for device '{}' as Disconnected",
                    device_id
                );
            }
        }

        // Then notify adapter(s) to subscribe to this device's telemetry topic.
        // Skip adapter subscription for extension-managed devices - the extension
        // handles data collection itself via produce_metrics.
        //
        // Binding rules (resolves the adapter_id=None ambiguity):
        // - adapter_id specified  → notify ONLY that exact adapter (deterministic).
        // - adapter_id is None    → notify ALL adapters matching adapter_type.
        //   This is required because multiple MQTT adapters may coexist
        //   (internal-mqtt + external-{id}) and without an explicit adapter_id we
        //   cannot know which broker the device publishes to. Broadcasting ensures
        //   the device's telemetry topic is subscribed on every connected broker so
        //   data flows regardless. This matches the server-restart path where
        //   add_broker / add_broker_with_tls re-subscribe all registered devices'
        //   telemetry topics on every adapter.
        if adapter_type != "extension" {
            // Collect matching adapters first, then subscribe outside the lock to
            // avoid holding the adapters read lock across multiple async awaits.
            let matched = {
                let adapters = self.adapters.read().await;
                let mut result = Vec::new();
                for (adapter_id, adapter) in adapters.iter() {
                    let is_match = if let Some(ref target_id) = target_adapter_id {
                        adapter.adapter_type() == adapter_type && adapter_id == target_id
                    } else {
                        adapter.adapter_type() == adapter_type
                    };
                    if is_match {
                        result.push((adapter_id.clone(), adapter.clone()));
                    }
                }
                result
            };

            if matched.is_empty() {
                tracing::warn!(
                    "No adapter found for device '{}' (type: {}, adapter_id: {:?})",
                    device_id,
                    adapter_type,
                    target_adapter_id
                );
            } else {
                for (adapter_id, adapter) in &matched {
                    tracing::info!(
                        "Notifying adapter '{}' to subscribe to device '{}'",
                        adapter_id,
                        device_id
                    );
                    // No `break`: notify ALL matching adapters so a device with a
                    // custom telemetry_topic is subscribed on every connected broker.
                    if let Err(e) = adapter.subscribe_device(&device_id).await {
                        tracing::warn!(
                            device_id = %device_id,
                            adapter = %adapter_id,
                            error = %e,
                            "Adapter failed to subscribe to device during register (continuing)"
                        );
                    }
                }
            }
        } else {
            tracing::info!(
                "Extension-managed device '{}' registered (skipping adapter subscription)",
                device_id
            );
        }

        // Publish event for UI refresh
        tokio::spawn({
            let event_bus = self.event_bus.clone();
            async move {
                let _ = event_bus
                    .publish(neomind_core::NeoMindEvent::Custom {
                        event_type: "DeviceRegistered".to_string(),
                        data: serde_json::json!({
                            "device_id": device_id,
                            "device_type": device_type,
                            "timestamp": chrono::Utc::now().timestamp(),
                        }),
                    })
                    .await;
            }
        });

        Ok(())
    }

    /// Get a device configuration with its template
    pub async fn get_device_with_template(
        &self,
        device_id: &str,
    ) -> Result<(DeviceConfig, DeviceTypeTemplate), DeviceError> {
        let config = self
            .registry
            .get_device(device_id)
            .ok_or_else(|| DeviceError::NotFoundStr(device_id.to_string()))?;

        let template = self
            .registry
            .get_template(&config.device_type)
            .ok_or_else(|| {
                DeviceError::NotFoundStr(format!(
                    "Template '{}' not found for device '{}'",
                    config.device_type, device_id
                ))
            })?;

        Ok((config, template))
    }

    /// Get a device configuration
    pub fn get_device(&self, device_id: &str) -> Option<DeviceConfig> {
        self.registry.get_device(device_id)
    }

    /// Find a device by its name (not ID)
    /// Returns the device config if found, None otherwise
    pub async fn get_device_by_name(&self, name: &str) -> Option<DeviceConfig> {
        let devices = self.list_devices();
        for device in devices {
            if device.name == name {
                return Some(device);
            }
        }
        None
    }

    /// List all device configurations
    pub fn list_devices(&self) -> Vec<DeviceConfig> {
        self.registry.list_devices()
    }

    /// Find a device by its telemetry topic
    /// This is used by MQTT adapters to route messages from custom topics
    pub fn find_device_by_telemetry_topic(&self, topic: &str) -> Option<(String, DeviceConfig)> {
        self.registry.find_device_by_telemetry_topic(topic)
    }

    /// Get the device registry (for sharing with adapters)
    pub fn get_registry(&self) -> Arc<DeviceRegistry> {
        self.registry.clone()
    }

    /// List devices by type
    pub fn list_devices_by_type(&self, device_type: &str) -> Vec<DeviceConfig> {
        self.registry.list_devices_by_type(device_type)
    }

    /// Unregister a device configuration
    ///
    /// Iterates every adapter and asks it to drop any subscriptions / state
    /// it holds for this device BEFORE removing the registry entry. Without
    /// this step the MQTT adapter keeps the device's topic subscriptions on
    /// the broker as zombies after deletion — messages keep arriving and
    /// getting discarded until the broker connection drops or the server
    /// restarts. `DeviceUnregistered` has no subscribers, so relying on the
    /// event bus for cleanup never worked.
    ///
    /// Adapter failures are logged but do not abort the unregister: the
    /// registry removal is the source of truth for "device is gone", and a
    /// single misbehaving adapter shouldn't lock the device into the registry.
    pub async fn unregister_device(&self, device_id: &str) -> Result<(), DeviceError> {
        // Phase 1: ask each adapter to drop subscriptions for this device.
        let adapter_names: Vec<String> = {
            let adapters = self.adapters.read().await;
            adapters.keys().cloned().collect()
        };
        for name in adapter_names {
            let adapter = {
                let adapters = self.adapters.read().await;
                adapters.get(&name).cloned()
            };
            if let Some(adapter) = adapter {
                if let Err(e) = adapter.unsubscribe_device(device_id).await {
                    tracing::warn!(
                        device_id = %device_id,
                        adapter = %name,
                        error = %e,
                        "Adapter failed to unsubscribe device during unregister (continuing)"
                    );
                }
            }
        }

        // Phase 2: remove from registry.
        self.registry.unregister_device(device_id)?;

        // Publish event for UI refresh
        tokio::spawn({
            let event_bus = self.event_bus.clone();
            let device_id = device_id.to_string();
            async move {
                let _ = event_bus
                    .publish(neomind_core::NeoMindEvent::Custom {
                        event_type: "DeviceUnregistered".to_string(),
                        data: serde_json::json!({
                            "device_id": device_id,
                            "timestamp": chrono::Utc::now().timestamp(),
                        }),
                    })
                    .await;
            }
        });

        // Phase 3: best-effort purge of on-disk image files for this device.
        // v0.9.6 stores device images at <data_dir>/images/<device_id>/; without
        // this they'd linger until the age-based cleanup sweep expires them — up
        // to image_retention after the device is already gone.
        let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
        Self::purge_device_images(device_id, std::path::Path::new(&data_dir));

        Ok(())
    }

    /// Best-effort purge of a device's on-disk image files on unregister.
    ///
    /// Failures are logged, never propagated — unregister must not fail just
    /// because image cleanup couldn't run. Reuses `validate_path_component`
    /// (same as `save_image_binary`) so a hostile device_id can't traverse out
    /// of `images/`, plus a canonicalize guard against symlink escape.
    fn purge_device_images(device_id: &str, data_dir: &std::path::Path) {
        let images_dir = data_dir.join("images");
        let Ok(safe_device_id) = crate::image_storage::validate_path_component(device_id) else {
            return;
        };
        let device_dir = images_dir.join(&safe_device_id);
        if !device_dir.exists() {
            return;
        }
        // Symlink-escape guard: refuse if the resolved path isn't under images/.
        let canon_ok = match (images_dir.canonicalize(), device_dir.canonicalize()) {
            (Ok(base), Ok(target)) => target.starts_with(&base),
            _ => false,
        };
        if !canon_ok {
            tracing::warn!(
                device_id = %device_id,
                "Refusing to purge device image dir: resolves outside images/"
            );
            return;
        }
        match std::fs::remove_dir_all(&device_dir) {
            Ok(_) => {
                tracing::info!(device_id = %device_id, "Purged device image dir on unregister")
            }
            Err(e) => {
                tracing::warn!(device_id = %device_id, error = %e, "Failed to purge device image dir")
            }
        }
    }

    /// Update a device configuration
    pub async fn update_device(
        &self,
        device_id: &str,
        config: DeviceConfig,
    ) -> Result<(), DeviceError> {
        self.registry.update_device(device_id, config).await
    }

    // ========== Command Sending ==========

    /// Send a command to a device.
    /// Validates against the device template, sends via adapter, and records in command history
    /// with Success/Failed status based on the send result.
    pub async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        params: HashMap<String, serde_json::Value>,
    ) -> Result<Option<MetricValue>, DeviceError> {
        // Get device config and template
        let (config, template) = self.get_device_with_template(device_id).await?;

        // Find command definition in template
        let command_def = template
            .commands
            .iter()
            .find(|cmd| cmd.name == command_name)
            .ok_or_else(|| {
                DeviceError::InvalidCommand(format!(
                    "Command '{}' not found in template '{}'",
                    command_name, template.device_type
                ))
            })?;

        // Validate and convert parameters
        let validated_params = self.validate_command_params(command_def, params.clone())?;

        // Record in command history
        let command_id = self
            .add_command_to_history(device_id, command_name, params.clone())
            .await;

        // Route extension devices through the extension command router
        // (skip payload building — extensions receive raw params, not MQTT payloads)
        if config.adapter_type == "extension" {
            let extension_id = config.adapter_id.as_deref().ok_or_else(|| {
                DeviceError::InvalidParameter(
                    "Device has no adapter_id set. Re-install the extension to fix this.".into(),
                )
            })?;

            let router = {
                let r = self.extension_command_router.read().await;
                r.clone()
            };

            if let Some(router) = router {
                match router(
                    extension_id.to_string(),
                    device_id.to_string(),
                    command_name.to_string(),
                    params,
                )
                .await
                {
                    Ok(()) => {
                        self.update_command_status(
                            device_id,
                            &command_id,
                            CommandStatus::Success,
                            Some("Command sent to extension successfully".into()),
                            None,
                        )
                        .await;
                        return Ok(None);
                    }
                    Err(e) => {
                        self.update_command_status(
                            device_id,
                            &command_id,
                            CommandStatus::Failed,
                            None,
                            Some(e.clone()),
                        )
                        .await;
                        return Err(DeviceError::InvalidParameter(e));
                    }
                }
            } else {
                self.update_command_status(
                    device_id,
                    &command_id,
                    CommandStatus::Failed,
                    None,
                    Some("No extension command router configured".into()),
                )
                .await;
                return Err(DeviceError::InvalidParameter(
                    "No extension command router configured for extension devices".into(),
                ));
            }
        }

        // Build command payload from template (MQTT/adapter devices only)
        let payload = self.build_command_payload(command_def, &validated_params)?;

        // Determine command topic from device connection config
        let command_topic = config.connection_config.command_topic.clone();

        // Resolve target adapter(s). When adapter_id is explicitly set, send only
        // to that adapter (deterministic). When adapter_id is None (API-created
        // devices), broadcast to all adapters matching the device's adapter_type —
        // this mirrors the subscribe_device path so commands reach a device
        // regardless of which broker it lives on. Publishing to a broker the
        // device isn't listening on is harmless.
        let adapter_type = config.adapter_type.clone();
        let target_adapter_id = config.adapter_id.clone();
        let matched: Vec<std::sync::Arc<dyn DeviceAdapter>> = {
            let adapters = self.adapters.read().await;
            let mut result = Vec::new();
            for (aid, adapter) in adapters.iter() {
                let is_match = if let Some(ref target) = target_adapter_id {
                    adapter.adapter_type() == adapter_type && aid == target
                } else {
                    adapter.adapter_type() == adapter_type
                };
                if is_match {
                    result.push(adapter.clone());
                }
            }
            result
        };

        if matched.is_empty() {
            let err_msg = if let Some(ref target) = target_adapter_id {
                format!("Adapter '{}' not found for device '{}'", target, device_id)
            } else {
                format!(
                    "No adapter of type '{}' found for device '{}'",
                    adapter_type, device_id
                )
            };
            self.update_command_status(
                device_id,
                &command_id,
                CommandStatus::Failed,
                None,
                Some(err_msg.clone()),
            )
            .await;
            return Err(DeviceError::NotFoundStr(err_msg));
        }

        // Send to every matching adapter. Succeed if at least one accepts it.
        let mut success_count = 0u32;
        let mut last_error: Option<String> = None;
        for adapter in &matched {
            match adapter
                .send_command(
                    device_id,
                    command_name,
                    payload.clone(),
                    command_topic.clone(),
                )
                .await
            {
                Ok(()) => success_count += 1,
                Err(e) => {
                    last_error = Some(format!("Failed to send command via adapter: {}", e));
                }
            }
        }

        if success_count > 0 {
            self.update_command_status(
                device_id,
                &command_id,
                CommandStatus::Success,
                Some(format!("Command sent to {} adapter(s)", success_count)),
                None,
            )
            .await;
            Ok(None)
        } else {
            let err_msg =
                last_error.unwrap_or_else(|| "Failed to send command on any adapter".to_string());
            self.update_command_status(
                device_id,
                &command_id,
                CommandStatus::Failed,
                None,
                Some(err_msg.clone()),
            )
            .await;
            Err(DeviceError::InvalidParameter(err_msg))
        }
    }

    /// Validate command parameters against template definition
    fn validate_command_params(
        &self,
        command_def: &super::mdl_format::CommandDefinition,
        params: HashMap<String, serde_json::Value>,
    ) -> Result<HashMap<String, MetricValue>, DeviceError> {
        let mut validated = HashMap::new();

        for param_def in &command_def.parameters {
            // Check if parameter is provided or has default
            let value = if let Some(param_value) = params.get(&param_def.name) {
                // Convert JSON value to MetricValue
                self.json_to_metric_value(param_value, &param_def.data_type)?
            } else if let Some(default) = &param_def.default_value {
                // Use default value
                default.clone()
            } else {
                return Err(DeviceError::InvalidParameter(format!(
                    "Required parameter '{}' not provided and has no default",
                    param_def.name
                )));
            };

            // Validate min/max if applicable
            if let (Some(min), Some(max)) = (param_def.min, param_def.max) {
                match &value {
                    MetricValue::Integer(i) => {
                        let i_f64 = *i as f64;
                        if i_f64 < min || i_f64 > max {
                            return Err(DeviceError::InvalidParameter(format!(
                                "Parameter '{}' value {} out of range [{}, {}]",
                                param_def.name, i, min, max
                            )));
                        }
                    }
                    MetricValue::Float(f) => {
                        if *f < min || *f > max {
                            return Err(DeviceError::InvalidParameter(format!(
                                "Parameter '{}' value {} out of range [{}, {}]",
                                param_def.name, f, min, max
                            )));
                        }
                    }
                    _ => {}
                }
            }

            validated.insert(param_def.name.clone(), value);
        }

        Ok(validated)
    }

    /// Convert JSON value to MetricValue based on expected type
    fn json_to_metric_value(
        &self,
        json: &serde_json::Value,
        expected_type: &super::mdl::MetricDataType,
    ) -> Result<MetricValue, DeviceError> {
        match (json, expected_type) {
            (serde_json::Value::Number(n), super::mdl::MetricDataType::Integer) => {
                if let Some(i) = n.as_i64() {
                    Ok(MetricValue::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(MetricValue::Integer(f as i64))
                } else {
                    Err(DeviceError::InvalidParameter(
                        "Number out of range for Integer".into(),
                    ))
                }
            }
            (serde_json::Value::Number(n), super::mdl::MetricDataType::Float) => {
                if let Some(f) = n.as_f64() {
                    Ok(MetricValue::Float(f))
                } else {
                    Err(DeviceError::InvalidParameter(
                        "Number cannot be converted to Float".into(),
                    ))
                }
            }
            (serde_json::Value::Bool(b), super::mdl::MetricDataType::Boolean) => {
                Ok(MetricValue::Boolean(*b))
            }
            (serde_json::Value::String(s), super::mdl::MetricDataType::String) => {
                Ok(MetricValue::String(s.clone()))
            }
            (serde_json::Value::Null, _) => Ok(MetricValue::Null),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Integer) => s
                .trim()
                .parse::<i64>()
                .map(MetricValue::Integer)
                .map_err(|_| {
                    DeviceError::InvalidParameter(format!("Cannot convert '{}' to Integer", s))
                }),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Float) => s
                .trim()
                .parse::<f64>()
                .map(MetricValue::Float)
                .map_err(|_| {
                    DeviceError::InvalidParameter(format!("Cannot convert '{}' to Float", s))
                }),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Boolean) => {
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "true" | "1" | "yes" | "on" => Ok(MetricValue::Boolean(true)),
                    "false" | "0" | "no" | "off" => Ok(MetricValue::Boolean(false)),
                    _ => Err(DeviceError::InvalidParameter(format!(
                        "Cannot convert '{}' to Boolean",
                        s
                    ))),
                }
            }
            (v, _) => {
                // Try to convert to string as fallback
                Ok(MetricValue::String(v.to_string()))
            }
        }
    }

    /// Infer a `MetricValue` from a JSON value's shape, with no
    /// expected-type hint. Used for `fixed_values` entries which have
    /// no corresponding `ParameterDefinition`.
    fn infer_metric_from_json(json: &serde_json::Value) -> Result<MetricValue, DeviceError> {
        match json {
            serde_json::Value::Null => Ok(MetricValue::Null),
            serde_json::Value::Bool(b) => Ok(MetricValue::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(MetricValue::Integer(i))
                } else {
                    n.as_f64().map(MetricValue::Float).ok_or_else(|| {
                        DeviceError::InvalidParameter(format!(
                            "fixed_value number not representable: {n}"
                        ))
                    })
                }
            }
            serde_json::Value::String(s) => Ok(MetricValue::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for v in arr {
                    out.push(Self::infer_metric_from_json(v)?);
                }
                Ok(MetricValue::Array(out))
            }
            serde_json::Value::Object(_) => Err(DeviceError::InvalidParameter(format!(
                "fixed_value object cannot be converted to MetricValue: {json}"
            ))),
        }
    }

    /// Build command payload from template
    fn build_command_payload(
        &self,
        command_def: &super::mdl_format::CommandDefinition,
        params: &HashMap<String, MetricValue>,
    ) -> Result<String, DeviceError> {
        // Merge fixed_values (template-declared constants the user
        // never sees) under user-supplied params. User params win on
        // key collision — fixed_values are defaults, not overrides.
        let mut merged = HashMap::new();
        for (k, v) in &command_def.fixed_values {
            merged.insert(k.clone(), Self::infer_metric_from_json(v)?);
        }
        for (k, v) in params {
            merged.insert(k.clone(), v.clone());
        }

        // Auto-inject system-level placeholders that should never
        // surface to the user. Today this is just `request_id` (used
        // by request/response correlation over MQTT). If the template
        // references it but neither user nor fixed_values supplied
        // one, mint a fresh UUID. Template authors therefore don't
        // need to declare `request_id` in `parameters` — it's pure
        // system plumbing.
        if command_def.payload_template.contains("${request_id}")
            && !merged.contains_key("request_id")
        {
            merged.insert(
                "request_id".to_string(),
                MetricValue::String(format!("req-{}", uuid::Uuid::new_v4())),
            );
        }

        // Resolve any `/api/images/` internal URLs in the merged params to
        // base64 data URLs before rendering. Devices are external targets and
        // cannot read hostless internal paths, so an unresolved `/api/images/`
        // string would arrive as an unusable value. Mirrors the data-push
        // outbound fix (scheduler.rs::resolve_image_urls_in_value).
        let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
        Self::resolve_command_image_urls(&mut merged, std::path::Path::new(&data_dir));

        // Delegate to the structured JSON-aware renderer. See
        // `payload_template` module docs for why string substitution
        // is unsafe (placeholder syntax drift, quote collision, type
        // erasure, JSON injection).
        let bytes = super::payload_template::render(&command_def.payload_template, &merged)
            .map_err(|e| DeviceError::InvalidParameter(format!("payload render: {e}")))?;
        String::from_utf8(bytes)
            .map_err(|e| DeviceError::InvalidParameter(format!("payload not UTF-8: {e}")))
    }

    // ========== Data Querying ==========

    /// Resolve `/api/images/` internal URLs in command params to base64 data URLs.
    ///
    /// Devices are external targets (separate MQTT client / HTTP endpoint) and
    /// cannot resolve a hostless `/api/images/` path. Walk every string param and,
    /// if it is an internal image URL, replace it with a self-contained
    /// `data:<mime>;base64,...` string via the shared helper (same symlink/size/
    /// magic guards as `GET /api/images/`). Unresolvable values (missing file, too
    /// large, non-image) are left untouched so the device surfaces its own error
    /// rather than receiving an empty value.
    fn resolve_command_image_urls(
        params: &mut HashMap<String, MetricValue>,
        data_dir: &std::path::Path,
    ) {
        for value in params.values_mut() {
            if let MetricValue::String(s) = value {
                if s.starts_with("/api/images/") {
                    if let Some(data_url) =
                        crate::image_storage::resolve_internal_image_to_data_url(s, data_dir)
                    {
                        *s = data_url;
                    }
                }
            }
        }
    }

    /// Query telemetry data for a device metric
    pub async fn query_telemetry(
        &self,
        device_id: &str,
        metric_name: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<usize>,
    ) -> Result<Vec<(i64, MetricValue)>, DeviceError> {
        // Get device config and template
        let (_config, template) = self.get_device_with_template(device_id).await?;

        // Check if this is a virtual metric (generated by transforms)
        // Virtual metrics use dot notation: transform.count, virtual.avg, etc.
        let is_virtual_metric = metric_name.starts_with("transform.")
            || metric_name.starts_with("virtual.")
            || metric_name.starts_with("computed.")
            || metric_name.starts_with("derived.")
            || metric_name.starts_with("aggregated.");

        // Validate metric exists in template (skip validation in simple mode or for virtual metrics)
        if !is_virtual_metric
            && !template.metrics.is_empty()
            && !template.metrics.iter().any(|m| m.name == metric_name)
        {
            return Err(DeviceError::InvalidMetric(format!(
                "Metric '{}' not found in template '{}'",
                metric_name, template.device_type
            )));
        }

        if is_virtual_metric {
            tracing::trace!(
                "Querying virtual metric {} for device {}",
                metric_name,
                device_id
            );
        }

        // Query from telemetry storage
        let storage_guard = self.telemetry_storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            let start = start_time.unwrap_or(i64::MIN);
            let end = end_time.unwrap_or(i64::MAX);

            let points = storage
                .query_limited(
                    &format!("device:{}", device_id),
                    metric_name,
                    start,
                    end,
                    limit,
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Telemetry query failed for {}/{}: {}",
                        device_id,
                        metric_name,
                        e
                    );
                    DeviceError::Communication(format!("Telemetry query failed: {}", e))
                })?;

            Ok(points.into_iter().map(|p| (p.timestamp, p.value)).collect())
        } else {
            tracing::warn!(
                "Telemetry storage not configured when querying {}/{}",
                device_id,
                metric_name
            );
            Err(DeviceError::InvalidParameter(
                "Telemetry storage not configured".into(),
            ))
        }
    }

    /// Get current metric values for a device (latest from storage)
    /// For devices with no defined metrics (simple mode), returns all available metrics from storage
    ///
    /// This method returns the TRUE LATEST value for each metric, regardless of time.
    /// It uses the `latest()` method which queries the most recent data point directly.
    /// There is no time range limitation - this is intentional because "current value"
    /// should always be the most recent value available.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    pub async fn get_current_metrics(
        &self,
        device_id: &str,
    ) -> Result<HashMap<String, MetricValue>, DeviceError> {
        let mut result = HashMap::new();

        // Get telemetry storage reference
        let storage_guard = self.telemetry_storage.read().await;
        let storage = match storage_guard.as_ref() {
            Some(s) => s,
            None => {
                tracing::warn!("Telemetry storage not configured for device {}", device_id);
                return Ok(result);
            }
        };

        // Query ALL metrics stored under device:{device_id} namespace
        // This includes both template-defined metrics and virtual metrics from Transforms
        let device_source_id = format!("device:{}", device_id);
        if let Ok(all_metrics) = storage.list_metrics(&device_source_id).await {
            let metric_names: Vec<&str> = all_metrics
                .iter()
                .filter(|n| !n.is_empty())
                .map(|n| n.as_str())
                .collect();
            if !metric_names.is_empty() {
                match storage.latest_batch(&device_source_id, &metric_names).await {
                    Ok(batch) => {
                        for (name, point) in batch {
                            result.insert(name, point.value);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to get latest values for {} ({} metrics): {}",
                            device_id,
                            metric_names.len(),
                            e
                        );
                    }
                }
            }
        }

        Ok(result)
    }

    // ========== Helper Methods ==========

    /// Get registry reference (for external use)
    pub fn registry(&self) -> &Arc<DeviceRegistry> {
        &self.registry
    }

    /// Get event bus reference
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Get metric definition from a device type template
    pub async fn get_metric_definition(
        &self,
        device_type: &str,
        metric_name: &str,
    ) -> Option<super::mdl_format::MetricDefinition> {
        let template = self.registry.get_template(device_type)?;
        template.metrics.into_iter().find(|m| m.name == metric_name)
    }

    // ========== Device Status Management ==========

    /// Get the current status of a device
    pub async fn get_device_status(&self, device_id: &str) -> DeviceStatus {
        let status_map = self.device_status.read().await;
        status_map.get(device_id).cloned().unwrap_or_default()
    }

    /// Get the connection status for a device
    pub async fn get_device_connection_status(&self, device_id: &str) -> ConnectionStatus {
        self.get_device_status(device_id).await.status
    }

    /// Get the last seen timestamp for a device
    ///
    /// Falls back to the registry's persisted `last_seen` when the in-memory
    /// status map has no entry for this device (e.g. right after a server
    /// restart, before any new telemetry arrives). Without this fallback,
    /// `last_seen` reads as 0 for previously-seen devices, which breaks
    /// `__last_seen_age_secs` rule tests and offline detection until the
    /// device publishes again.
    pub async fn get_device_last_seen(&self, device_id: &str) -> i64 {
        let in_memory = self.get_device_status(device_id).await.last_seen;
        if in_memory > 0 {
            return in_memory;
        }
        // Fallback to persisted registry value (survives restarts)
        self.registry
            .get_device(device_id)
            .map(|c| c.last_seen)
            .unwrap_or(0)
    }

    /// Update device status (called by event listeners or adapters)
    pub async fn update_device_status(&self, device_id: &str, status: ConnectionStatus) {
        let mut status_map = self.device_status.write().await;
        let entry = status_map.entry(device_id.to_string()).or_default();
        entry.update(status);
    }

    /// Update last_seen timestamp for a device (called when metrics are written by extensions)
    pub async fn update_last_seen(&self, device_id: &str, last_seen_secs: i64) {
        self.registry
            .update_last_seen(device_id, last_seen_secs)
            .await;
    }

    /// Get all device statuses
    pub async fn get_all_device_statuses(&self) -> HashMap<String, DeviceStatus> {
        let status_map = self.device_status.read().await;
        status_map.clone()
    }

    /// Get devices filtered by status
    pub async fn get_devices_by_status(&self, status: ConnectionStatus) -> Vec<String> {
        let status_map = self.device_status.read().await;
        status_map
            .iter()
            .filter(|(_, s)| s.status == status)
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ========== Command History Management ==========

    /// Add a command to history
    pub async fn add_command_to_history(
        &self,
        device_id: &str,
        command_name: &str,
        parameters: HashMap<String, serde_json::Value>,
    ) -> String {
        let command_id = format!(
            "cmd_{}",
            self.command_id_counter.fetch_add(1, Ordering::Relaxed)
        );

        let record = CommandHistoryRecord {
            command_id: command_id.clone(),
            device_id: device_id.to_string(),
            command_name: command_name.to_string(),
            parameters,
            status: CommandStatus::Pending,
            result: None,
            error: None,
            created_at: chrono::Utc::now().timestamp(),
            completed_at: None,
        };

        // Clone for storage before moving into the HashMap
        let record_for_storage = record.clone();

        let mut history = self.command_history.write().await;
        let device_commands = history
            .entry(device_id.to_string())
            .or_insert_with(Vec::new);
        device_commands.push(record);

        // Trim history if exceeds max
        if device_commands.len() > self.max_history_entries {
            device_commands.remove(0);
        }
        drop(history);

        // Persist to storage
        self.save_command_to_storage(&record_for_storage).await;

        command_id
    }

    /// Update command status in history
    pub async fn update_command_status(
        &self,
        device_id: &str,
        command_id: &str,
        status: CommandStatus,
        result: Option<String>,
        error: Option<String>,
    ) {
        let is_terminal = status.is_terminal();
        let mut record_for_storage = None;

        {
            let mut history = self.command_history.write().await;
            if let Some(device_commands) = history.get_mut(device_id) {
                if let Some(record) = device_commands
                    .iter_mut()
                    .find(|r| r.command_id == command_id)
                {
                    record.status = status;
                    record.result = result;
                    record.error = error;
                    if is_terminal {
                        record.completed_at = Some(chrono::Utc::now().timestamp());
                    }
                    // Clone for storage after updating
                    record_for_storage = Some(record.clone());
                }
            }
        }

        // Persist to storage if record was found and updated
        if let Some(record) = record_for_storage {
            self.save_command_to_storage(&record).await;
        }
    }

    /// Get command history for a device
    pub async fn get_command_history(
        &self,
        device_id: &str,
        limit: Option<usize>,
    ) -> Vec<CommandHistoryRecord> {
        let history = self.command_history.read().await;
        if let Some(device_commands) = history.get(device_id) {
            let mut commands = device_commands.clone();
            // Sort by created_at descending (newest first)
            commands.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            if let Some(limit) = limit {
                commands.truncate(limit);
            }
            commands
        } else {
            vec![]
        }
    }

    /// Get a specific command by ID
    pub async fn get_command(
        &self,
        device_id: &str,
        command_id: &str,
    ) -> Option<CommandHistoryRecord> {
        let history = self.command_history.read().await;
        history
            .get(device_id)?
            .iter()
            .find(|r| r.command_id == command_id)
            .cloned()
    }

    /// Load command history from storage (called on startup)
    async fn load_command_history_from_storage(&self) -> Result<(), DeviceError> {
        let Some(store) = self.registry.storage() else {
            // No storage configured, that's fine
            return Ok(());
        };

        // List all commands from storage (limit to reasonable number)
        let storage_commands = store
            .list_all_commands(Some(1000))
            .map_err(|e| DeviceError::Storage(format!("Failed to load command history: {}", e)))?;

        if storage_commands.is_empty() {
            return Ok(());
        }

        let mut history = self.command_history.write().await;
        let mut max_counter = 0;

        for storage_record in storage_commands {
            let device_id = storage_record.device_id.clone();
            let record = command_from_storage(storage_record);

            // Update command ID counter based on loaded records
            if let Some(suffix) = record.command_id.strip_prefix("cmd_") {
                if let Ok(num) = suffix.parse::<u64>() {
                    max_counter = max_counter.max(num);
                }
            }

            history
                .entry(device_id)
                .or_insert_with(Vec::new)
                .push(record);
        }

        // Update the command ID counter to avoid collisions
        self.command_id_counter
            .fetch_max(max_counter + 1, Ordering::Relaxed);

        tracing::info!(
            "Loaded {} command history entries from storage",
            history.values().map(|v| v.len()).sum::<usize>()
        );

        Ok(())
    }

    /// Save a command record to storage
    async fn save_command_to_storage(&self, record: &CommandHistoryRecord) {
        let Some(store) = self.registry.storage() else {
            return;
        };

        let storage_record = command_to_storage(record);
        if let Err(e) = store.save_command(&storage_record) {
            tracing::warn!(
                "Failed to save command {} to storage: {}",
                record.command_id,
                e
            );
        }
    }

    /// Clear command history for a device
    pub async fn clear_command_history(&self, device_id: &str) {
        let mut history = self.command_history.write().await;
        history.remove(device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ConnectionConfig;

    #[tokio::test]
    async fn test_service_template_registration() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        service.register_template(template).await.unwrap();

        let retrieved = service.get_template("test_sensor");
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_service_device_registration() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        // Register template first
        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        service.register_template(template).await.unwrap();

        // Register device
        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "test_sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig::new(),
            adapter_id: None,
            last_seen: 0,
            offline_timeout_secs: None,
        };

        service.register_device(config).await.unwrap();

        let retrieved = service.get_device("sensor1");
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_command_parameter_validation() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl::MetricDataType;
        use crate::mdl_format::CommandDefinition;
        use crate::mdl_format::ParameterDefinition;

        let command_def = CommandDefinition {
            name: "set_temperature".to_string(),
            display_name: "Set Temperature".to_string(),
            payload_template: r#"{"action": "set_temperature", "value": ${value}}"#.to_string(),
            parameters: vec![ParameterDefinition {
                name: "value".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                default_value: None,
                min: Some(0.0),
                max: Some(100.0),
                unit: "°C".to_string(),
                allowed_values: vec![],
                required: true,
                visible_when: None,
                group: None,
                help_text: String::new(),
                validation: vec![],
            }],
            samples: vec![],
            description: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        };

        // Valid parameter
        let mut params = HashMap::new();
        params.insert("value".to_string(), serde_json::json!(25.5));
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_ok());

        // Invalid parameter (out of range)
        let mut params = HashMap::new();
        params.insert("value".to_string(), serde_json::json!(150.0));
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_err());

        // Missing parameter
        let params = HashMap::new();
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_command_payload() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl_format::CommandDefinition;

        let command_def = CommandDefinition {
            name: "set_temperature".to_string(),
            display_name: "Set Temperature".to_string(),
            payload_template: r#"{"action": "set_temperature", "value": ${value}}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            description: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        };

        let mut params = HashMap::new();
        params.insert("value".to_string(), MetricValue::Float(25.5));

        let payload = service
            .build_command_payload(&command_def, &params)
            .unwrap();
        assert!(payload.contains("25.5"));
    }

    #[test]
    fn test_resolve_command_image_urls_resolves_api_images() {
        // A command param carrying an /api/images/ internal URL must be resolved
        // to a base64 data URL before rendering — devices are external targets
        // and can't read hostless internal paths. Mirrors the data-push fix.
        let temp =
            std::env::temp_dir().join(format!("neomind_test_cmd_img_{}", std::process::id()));
        let images_dir = temp.join("images").join("dev").join("overlay");
        std::fs::create_dir_all(&images_dir).unwrap();
        let png = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52,
        ];
        std::fs::write(images_dir.join("1700000000.png"), png).unwrap();

        let mut params = HashMap::new();
        params.insert(
            "image".to_string(),
            MetricValue::String("/api/images/dev/overlay/1700000000.png".to_string()),
        );
        params.insert("label".to_string(), MetricValue::String("hi".to_string()));

        DeviceService::resolve_command_image_urls(&mut params, &temp);

        let img = match &params["image"] {
            MetricValue::String(s) => s.clone(),
            other => panic!("image should stay a string, got {other:?}"),
        };
        assert!(img.starts_with("data:image/png;base64,"), "got: {img}");
        assert!(!img.contains("/api/images/"), "got: {img}");
        // Non-image params are left untouched.
        match &params["label"] {
            MetricValue::String(s) => assert_eq!(s, "hi"),
            other => panic!("label should be untouched, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_resolve_command_image_urls_missing_falls_back() {
        // An unresolvable /api/images/ URL (missing file) is left as-is so the
        // device surfaces its own error rather than receiving an empty value.
        let temp =
            std::env::temp_dir().join(format!("neomind_test_cmd_empty_{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let mut params = HashMap::new();
        params.insert(
            "image".to_string(),
            MetricValue::String("/api/images/nope/overlay/0.png".to_string()),
        );
        DeviceService::resolve_command_image_urls(&mut params, &temp);
        match &params["image"] {
            MetricValue::String(s) => {
                assert_eq!(s, "/api/images/nope/overlay/0.png")
            }
            other => panic!("should fall back to original URL, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_purge_device_images_removes_device_dir_leaves_siblings() {
        // Unregistering a device must purge its on-disk image dir, leaving
        // other devices' image dirs intact.
        let temp = std::env::temp_dir().join(format!("neomind_test_purge_{}", std::process::id()));
        let images = temp.join("images");
        let dev_a = images.join("cam-a").join("image");
        let dev_b = images.join("cam-b").join("image");
        std::fs::create_dir_all(&dev_a).unwrap();
        std::fs::create_dir_all(&dev_b).unwrap();
        std::fs::write(dev_a.join("1.png"), b"data").unwrap();
        std::fs::write(dev_b.join("2.png"), b"data").unwrap();

        DeviceService::purge_device_images("cam-a", &temp);

        assert!(!images.join("cam-a").exists(), "cam-a dir should be purged");
        assert!(images.join("cam-b").exists(), "cam-b dir must be untouched");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_purge_device_images_rejects_traversal_device_id() {
        // A hostile device_id with path-traversal chars must not escape images/.
        let temp =
            std::env::temp_dir().join(format!("neomind_test_purge_trav_{}", std::process::id()));
        let images = temp.join("images");
        std::fs::create_dir_all(&images).unwrap();
        // Nothing to purge (validate_path_component rejects "../etc"), and no
        // panic / escape — just a no-op.
        DeviceService::purge_device_images("../../etc", &temp);
        assert!(images.exists());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_purge_device_images_missing_dir_is_noop() {
        let temp =
            std::env::temp_dir().join(format!("neomind_test_purge_noop_{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        // No images dir at all — must not panic.
        DeviceService::purge_device_images("never-existed", &temp);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[tokio::test]
    async fn test_build_command_payload_merges_fixed_values() {
        // Regression test: fixed_values declared in a CommandDefinition
        // must be merged into the params HashMap before rendering, and
        // user-supplied params override them on key collision.
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl_format::CommandDefinition;

        let mut fixed = std::collections::HashMap::new();
        fixed.insert("cmd".to_string(), serde_json::json!("capture"));
        fixed.insert("store_to_sd".to_string(), serde_json::json!(false));

        let command_def = CommandDefinition {
            name: "capture".to_string(),
            display_name: "Capture".to_string(),
            payload_template: r#"{"cmd": ${cmd}, "store_to_sd": ${store_to_sd}}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            description: String::new(),
            fixed_values: fixed,
            parameter_groups: vec![],
        };

        // User sends no params — fixed_values must fill in.
        let params = HashMap::new();
        let payload = service
            .build_command_payload(&command_def, &params)
            .expect("fixed_values should satisfy template");
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["cmd"], "capture");
        assert_eq!(parsed["store_to_sd"], false);

        // User overrides one fixed_value.
        let mut params = HashMap::new();
        params.insert("store_to_sd".to_string(), MetricValue::Boolean(true));
        let payload = service
            .build_command_payload(&command_def, &params)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["cmd"], "capture");
        assert_eq!(
            parsed["store_to_sd"], true,
            "user param must override fixed_value"
        );
    }

    #[tokio::test]
    async fn test_build_command_payload_auto_injects_request_id() {
        // Regression test: when a template references ${request_id}
        // but neither the user nor fixed_values supplied one, the
        // service mints a fresh UUID. This lets templates drop
        // `request_id` from `parameters` entirely — it's pure system
        // plumbing for request/response correlation.
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl_format::CommandDefinition;

        let command_def = CommandDefinition {
            name: "capture".to_string(),
            display_name: "Capture".to_string(),
            payload_template: r#"{"cmd": "capture", "request_id": "${request_id}"}"#.to_string(),
            parameters: vec![], // no request_id declared — system handles it
            samples: vec![],
            description: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        };

        let params = HashMap::new();
        let payload = service
            .build_command_payload(&command_def, &params)
            .expect("auto-injection should satisfy request_id");
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let generated = parsed["request_id"]
            .as_str()
            .expect("request_id must be a string");
        assert!(
            generated.starts_with("req-"),
            "auto-generated request_id should be prefixed with 'req-', got: {}",
            generated
        );
        assert!(
            generated.len() > "req-".len() + 8,
            "auto-generated request_id should contain a UUID, got: {}",
            generated
        );

        // User-supplied request_id must NOT be clobbered.
        let mut params = HashMap::new();
        params.insert(
            "request_id".to_string(),
            MetricValue::String("my-correlation-id".into()),
        );
        let payload = service
            .build_command_payload(&command_def, &params)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            parsed["request_id"], "my-correlation-id",
            "explicit request_id must override auto-injection"
        );
    }

    #[tokio::test]
    async fn test_ne301_capture_protocol_contract() {
        // End-to-end regression for the corrected NE301 capture
        // protocol: `{"cmd":"capture","request_id":"..."}` with NO
        // params field. User supplies zero parameters; the service
        // auto-injects request_id.
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl_format::CommandDefinition;

        let command_def = CommandDefinition {
            name: "capture".to_string(),
            display_name: "Capture".to_string(),
            payload_template: r#"{"cmd": "capture", "request_id": "${request_id}"}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            description: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        };

        let params = HashMap::new();
        let payload = service
            .build_command_payload(&command_def, &params)
            .expect("capture with no user params should render");
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();

        // Protocol contract — see user-provided NE301 spec.
        assert_eq!(parsed["cmd"], "capture");
        assert!(
            parsed["request_id"].is_string(),
            "request_id must be auto-injected"
        );
        assert!(
            parsed.get("params").is_none(),
            "capture must NOT carry a params field — the real protocol has none"
        );
        assert!(
            parsed.get("enable_ai").is_none()
                && parsed.get("chunk_size").is_none()
                && parsed.get("store_to_sd").is_none(),
            "fabricated fields from the old buggy template must be gone"
        );
    }
}
