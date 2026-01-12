//! Adapter Manager for centralized device adapter lifecycle management.
//!
//! This module provides the `AdapterManager` which is responsible for:
//! - Managing all adapter lifecycles (start/stop)
//! - Aggregating device events from all adapters to the event bus
//! - Providing unified control and monitoring of all adapters

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, broadcast};
use tokio::task::JoinHandle;
use serde::{Deserialize, Serialize};
use futures::StreamExt;

use crate::adapter::{DeviceAdapter, DeviceEvent, AdapterError};

/// Adapter status information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdapterStatus {
    /// Adapter is stopped
    Stopped,
    /// Adapter is starting
    Starting,
    /// Adapter is running
    Running,
    /// Adapter is stopping
    Stopping,
    /// Adapter has encountered an error
    Error(String),
}

/// Adapter information summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInfo {
    /// Adapter name
    pub name: String,
    /// Adapter type
    pub adapter_type: String,
    /// Current status
    pub status: AdapterStatus,
    /// Number of devices managed
    pub device_count: usize,
    /// Uptime in seconds (if running)
    pub uptime_secs: Option<u64>,
    /// Last activity timestamp
    pub last_activity: i64,
}

/// Adapter lifecycle state.
struct AdapterState {
    /// The adapter instance
    adapter: Arc<dyn DeviceAdapter>,
    /// Event forwarding task handle
    event_task: Option<JoinHandle<()>>,
    /// Current status
    status: AdapterStatus,
    /// Start timestamp
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Last activity timestamp
    last_activity: i64,
}

impl AdapterState {
    /// Create a new adapter state.
    fn new(adapter: Arc<dyn DeviceAdapter>) -> Self {
        Self {
            adapter,
            event_task: None,
            status: AdapterStatus::Stopped,
            started_at: None,
            last_activity: chrono::Utc::now().timestamp(),
        }
    }

    /// Check if adapter is running.
    fn is_running(&self) -> bool {
        matches!(self.status, AdapterStatus::Running)
    }

    /// Get uptime in seconds.
    fn uptime_secs(&self) -> Option<u64> {
        self.started_at.map(|start| {
            (chrono::Utc::now() - start).num_seconds().max(0) as u64
        })
    }

    /// Get adapter info.
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            name: self.adapter.name().to_string(),
            adapter_type: self.adapter.adapter_type().to_string(),
            status: self.status.clone(),
            device_count: self.adapter.device_count(),
            uptime_secs: self.uptime_secs(),
            last_activity: self.last_activity,
        }
    }
}

/// Aggregate statistics for all adapters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterStats {
    /// Total number of adapters
    pub total_adapters: usize,
    /// Number of running adapters
    pub running_adapters: usize,
    /// Number of stopped adapters
    pub stopped_adapters: usize,
    /// Number of adapters with errors
    pub error_adapters: usize,
    /// Total number of devices across all adapters
    pub total_devices: usize,
    /// Per-adapter statistics
    pub adapter_stats: Vec<AdapterInfo>,
}

/// Configuration for the adapter manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterManagerConfig {
    /// Whether to auto-start adapters on registration
    pub auto_start: bool,
    /// Grace period for stopping adapters (seconds)
    pub stop_timeout_secs: u64,
    /// Whether to restart adapters on error
    pub restart_on_error: bool,
    /// Maximum restart attempts per adapter
    pub max_restart_attempts: u32,
}

impl Default for AdapterManagerConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            stop_timeout_secs: 30,
            restart_on_error: false,
            max_restart_attempts: 3,
        }
    }
}

/// Device event wrapper with adapter context.
#[derive(Debug, Clone)]
pub struct ModifiedDeviceEvent {
    /// Name of the adapter that generated the event
    pub adapter_name: String,
    /// The NeoTalk event
    pub neotalk_event: edge_ai_core::NeoTalkEvent,
}

/// Event emitted by the adapter manager.
#[derive(Debug, Clone)]
pub enum ManagerEvent {
    /// Adapter started
    AdapterStarted { name: String },
    /// Adapter stopped
    AdapterStopped { name: String },
    /// Adapter encountered an error
    AdapterError { name: String, error: String },
    /// Device event from any adapter
    DeviceEvent { adapter_name: String, event: DeviceEvent },
}

/// Manager for all device adapters.
///
/// The AdapterManager provides centralized control over all device adapters,
/// handles event aggregation, and provides monitoring and statistics.
pub struct AdapterManager {
    /// Registered adapter states
    adapters: Arc<RwLock<HashMap<String, AdapterState>>>,
    /// Event channel for device events (with adapter context)
    device_event_tx: broadcast::Sender<ModifiedDeviceEvent>,
    /// Manager event channel
    manager_event_tx: mpsc::Sender<ManagerEvent>,
    /// Configuration
    config: AdapterManagerConfig,
    /// Running state
    running: Arc<RwLock<bool>>,
}

impl AdapterManager {
    /// Create a new adapter manager.
    pub fn new() -> Self {
        let (device_event_tx, _) = broadcast::channel(10000);
        let (manager_event_tx, _) = mpsc::channel(1000);

        Self {
            adapters: Arc::new(RwLock::new(HashMap::new())),
            device_event_tx,
            manager_event_tx,
            config: AdapterManagerConfig::default(),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a new adapter manager with custom configuration.
    pub fn with_config(config: AdapterManagerConfig) -> Self {
        let (device_event_tx, _) = broadcast::channel(10000);
        let (manager_event_tx, _) = mpsc::channel(1000);

        Self {
            adapters: Arc::new(RwLock::new(HashMap::new())),
            device_event_tx,
            manager_event_tx,
            config,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Register an adapter with the manager.
    ///
    /// Returns an error if an adapter with the same name is already registered.
    pub async fn register(&self, adapter: Arc<dyn DeviceAdapter>) -> Result<(), AdapterError> {
        let name = adapter.name().to_string();
        let mut adapters = self.adapters.write().await;

        if adapters.contains_key(&name) {
            return Err(AdapterError::Configuration(format!(
                "Adapter '{}' is already registered",
                name
            )));
        }

        let state = AdapterState::new(adapter);
        adapters.insert(name.clone(), state);

        // Auto-start if configured
        if self.config.auto_start {
            drop(adapters);
            self.start_adapter(&name).await?;
        }

        tracing::info!("Registered adapter: {}", name);
        Ok(())
    }

    /// Unregister an adapter.
    ///
    /// This will stop the adapter if it's running.
    pub async fn unregister(&self, name: &str) -> Result<(), AdapterError> {
        // Stop the adapter first
        {
            let adapters = self.adapters.read().await;
            if let Some(state) = adapters.get(name) {
                if state.is_running() {
                    drop(adapters);
                    self.stop_adapter(name).await?;
                }
            }
        }

        // Remove from registry
        let mut adapters = self.adapters.write().await;
        adapters.remove(name)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!(
                "Adapter '{}' not found",
                name
            )))?;

        tracing::info!("Unregistered adapter: {}", name);
        Ok(())
    }

    /// Start a specific adapter by name.
    pub async fn start_adapter(&self, name: &str) -> Result<(), AdapterError> {
        let mut adapters = self.adapters.write().await;
        let state = adapters.get_mut(name)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!(
                "Adapter '{}' not found",
                name
            )))?;

        if state.is_running() {
            return Ok(());
        }

        // Update status to starting
        state.status = AdapterStatus::Starting;
        let adapter = state.adapter.clone();
        let name = name.to_string();

        // Spawn event forwarding task
        let device_event_tx = self.device_event_tx.clone();
        let _manager_event_tx = self.manager_event_tx.clone();
        let adapter_name = name.clone();
        let adapters_ref = self.adapters.clone();

        let handle = tokio::spawn(async move {
            let mut event_rx = adapter.subscribe();

            while adapter.is_running() {
                match event_rx.next().await {
                    Some(event) => {
                        // Update last activity
                        let mut adapters = adapters_ref.write().await;
                        if let Some(state) = adapters.get_mut(&adapter_name) {
                            state.last_activity = chrono::Utc::now().timestamp();
                        }
                        drop(adapters);

                        // Convert to NeoTalk event and publish to device event channel
                        let neotalk_event = event.to_neotalk_event();
                        let device_id = match &neotalk_event {
                            edge_ai_core::NeoTalkEvent::DeviceMetric { device_id, .. }
                            | edge_ai_core::NeoTalkEvent::DeviceOnline { device_id, .. }
                            | edge_ai_core::NeoTalkEvent::DeviceOffline { device_id, .. }
                            | edge_ai_core::NeoTalkEvent::DeviceCommandResult { device_id, .. } => {
                                device_id.clone()
                            }
                            _ => "unknown".to_string(),
                        };

                        let _source = format!("adapter:{}", device_id);
                        let _ = device_event_tx.send(ModifiedDeviceEvent {
                            adapter_name: adapter_name.clone(),
                            neotalk_event,
                        });
                    }
                    None => {
                        tracing::debug!("Adapter {} event stream ended", adapter_name);
                        break;
                    }
                }
            }
        });

        // Start the adapter
        drop(adapters);
        let name_ref = name.as_str();
        let adapters = self.adapters.read().await;
        let state = adapters.get(name_ref)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!("Adapter '{}' not found", name)))?;
        let adapter = state.adapter.clone();
        drop(adapters);

        adapter.start().await?;

        // Update state
        let mut adapters = self.adapters.write().await;
        let state = adapters.get_mut(name_ref)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!("Adapter '{}' not found", name)))?;
        state.status = AdapterStatus::Running;
        state.started_at = Some(chrono::Utc::now());
        state.event_task = Some(handle);

        // Publish manager event
        let _ = self.manager_event_tx.send(ManagerEvent::AdapterStarted {
            name: name.to_string(),
        }).await;

        tracing::info!("Started adapter: {}", name);
        Ok(())
    }

    /// Stop a specific adapter by name.
    pub async fn stop_adapter(&self, name: &str) -> Result<(), AdapterError> {
        let mut adapters = self.adapters.write().await;
        let state = adapters.get_mut(name)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!(
                "Adapter '{}' not found",
                name
            )))?;

        if !state.is_running() {
            return Ok(());
        }

        state.status = AdapterStatus::Stopping;
        let adapter = state.adapter.clone();
        let name = name.to_string();

        // Abort event task
        if let Some(handle) = state.event_task.take() {
            handle.abort();
        }

        drop(adapters);

        // Stop the adapter
        adapter.stop().await?;

        // Update state
        let mut adapters = self.adapters.write().await;
        let state = adapters.get_mut(&name)
            .ok_or_else(|| AdapterError::DeviceNotFound(format!("Adapter '{}' not found", name)))?;
        state.status = AdapterStatus::Stopped;
        state.started_at = None;

        // Publish manager event
        let _ = self.manager_event_tx.send(ManagerEvent::AdapterStopped {
            name: name.clone(),
        }).await;

        tracing::info!("Stopped adapter: {}", name);
        Ok(())
    }

    /// Restart a specific adapter.
    pub async fn restart_adapter(&self, name: &str) -> Result<(), AdapterError> {
        self.stop_adapter(name).await?;
        self.start_adapter(name).await?;
        Ok(())
    }

    /// Start all registered adapters.
    ///
    /// Returns the number of adapters successfully started.
    pub async fn start_all(&self) -> usize {
        let adapter_names: Vec<String> = {
            let adapters = self.adapters.read().await;
            adapters.keys().cloned().collect()
        };

        let mut started = 0;
        for name in adapter_names {
            match self.start_adapter(&name).await {
                Ok(_) => started += 1,
                Err(e) => {
                    tracing::error!("Failed to start adapter '{}': {}", name, e);

                    // Update status to error
                    let mut adapters = self.adapters.write().await;
                    if let Some(state) = adapters.get_mut(&name) {
                        state.status = AdapterStatus::Error(e.to_string());
                    }

                    // Publish error event
                    let _ = self.manager_event_tx.send(ManagerEvent::AdapterError {
                        name,
                        error: e.to_string(),
                    }).await;
                }
            }
        }

        if started > 0 {
            *self.running.write().await = true;
        }

        started
    }

    /// Stop all running adapters.
    pub async fn stop_all(&self) {
        let adapter_names: Vec<String> = {
            let adapters = self.adapters.read().await;
            adapters.keys().cloned().collect()
        };

        for name in adapter_names {
            if let Err(e) = self.stop_adapter(&name).await {
                tracing::error!("Failed to stop adapter '{}': {}", name, e);
            }
        }

        *self.running.write().await = false;
    }

    /// Get information about a specific adapter.
    pub async fn get_adapter_info(&self, name: &str) -> Option<AdapterInfo> {
        let adapters = self.adapters.read().await;
        adapters.get(name).map(|state| state.info())
    }

    /// Get information about all adapters.
    pub async fn list_adapters(&self) -> Vec<AdapterInfo> {
        let adapters = self.adapters.read().await;
        adapters.values().map(|state| state.info()).collect()
    }

    /// Get aggregate statistics for all adapters.
    pub async fn stats(&self) -> AdapterStats {
        let adapters = self.adapters.read().await;
        let adapter_infos: Vec<AdapterInfo> = adapters.values()
            .map(|state| state.info())
            .collect();

        let total_adapters = adapter_infos.len();
        let running_adapters = adapter_infos.iter()
            .filter(|i| matches!(i.status, AdapterStatus::Running))
            .count();
        let stopped_adapters = adapter_infos.iter()
            .filter(|i| matches!(i.status, AdapterStatus::Stopped))
            .count();
        let error_adapters = adapter_infos.iter()
            .filter(|i| matches!(i.status, AdapterStatus::Error(_)))
            .count();
        let total_devices = adapter_infos.iter()
            .map(|i| i.device_count)
            .sum();

        AdapterStats {
            total_adapters,
            running_adapters,
            stopped_adapters,
            error_adapters,
            total_devices,
            adapter_stats: adapter_infos,
        }
    }

    /// Get total device count across all adapters.
    pub async fn total_device_count(&self) -> usize {
        let adapters = self.adapters.read().await;
        adapters.values()
            .map(|state| state.adapter.device_count())
            .sum()
    }

    /// List all device IDs from all adapters.
    pub async fn list_all_devices(&self) -> Vec<String> {
        let adapters = self.adapters.read().await;
        let mut devices = Vec::new();
        for state in adapters.values() {
            devices.extend(state.adapter.list_devices());
        }
        devices.sort();
        devices.dedup();
        devices
    }

    /// Subscribe to device events from all adapters.
    pub fn subscribe_device_events(&self) -> broadcast::Receiver<ModifiedDeviceEvent> {
        self.device_event_tx.subscribe()
    }

    /// Subscribe to manager events.
    pub fn subscribe_manager_events(&self) -> mpsc::Receiver<ManagerEvent> {
        let (_tx, rx) = mpsc::channel(1000);
        // Note: This would need a more sophisticated implementation
        // to properly clone the sender
        rx
    }

    /// Check if the manager is running (any adapter is active).
    pub async fn is_running(&self) -> bool {
        let adapters = self.adapters.read().await;
        adapters.values().any(|state| state.is_running())
    }

    /// Get the configuration.
    pub fn config(&self) -> &AdapterManagerConfig {
        &self.config
    }

    /// Update the configuration.
    pub async fn update_config(&mut self, config: AdapterManagerConfig) {
        self.config = config;
    }

    /// Perform health check on all adapters.
    pub async fn health_check(&self) -> HashMap<String, bool> {
        let adapters = self.adapters.read().await;
        let mut health = HashMap::new();

        for (name, state) in adapters.iter() {
            let is_healthy = match &state.status {
                AdapterStatus::Running => true,
                AdapterStatus::Error(_) => false,
                AdapterStatus::Stopped | AdapterStatus::Starting | AdapterStatus::Stopping => false,
            };
            health.insert(name.clone(), is_healthy);
        }

        health
    }

    /// Find which adapter manages a specific device.
    pub async fn find_adapter_for_device(&self, device_id: &str) -> Option<String> {
        let adapters = self.adapters.read().await;
        for (name, state) in adapters.iter() {
            if state.adapter.list_devices().contains(&device_id.to_string()) {
                return Some(name.clone());
            }
        }
        None
    }
}

impl Default for AdapterManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MockAdapter;

    #[tokio::test]
    async fn test_manager_register() {
        let manager = AdapterManager::new();
        let adapter = Arc::new(MockAdapter::new("test_adapter"));

        assert!(manager.register(adapter).await.is_ok());

        let adapters = manager.list_adapters().await;
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "test_adapter");
    }

    #[tokio::test]
    async fn test_manager_duplicate_register() {
        let manager = AdapterManager::new();
        let adapter1 = Arc::new(MockAdapter::new("test_adapter"));
        let adapter2 = Arc::new(MockAdapter::new("test_adapter"));

        assert!(manager.register(adapter1).await.is_ok());
        assert!(manager.register(adapter2).await.is_err());
    }

    #[tokio::test]
    async fn test_manager_start_adapter() {
        let manager = AdapterManager::new();
        let adapter = Arc::new(
            MockAdapter::new("test_adapter")
                .with_device("device1")
        );

        manager.register(adapter).await.unwrap();
        manager.start_adapter("test_adapter").await.unwrap();

        let info = manager.get_adapter_info("test_adapter").await;
        assert!(info.is_some());
        assert!(matches!(info.unwrap().status, AdapterStatus::Running));
    }

    #[tokio::test]
    async fn test_manager_start_all() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1").with_device("d1")
        )).await.unwrap();
        manager.register(Arc::new(
            MockAdapter::new("adapter2").with_device("d2")
        )).await.unwrap();

        let started = manager.start_all().await;
        assert_eq!(started, 2);

        let stats = manager.stats().await;
        assert_eq!(stats.running_adapters, 2);
    }

    #[tokio::test]
    async fn test_manager_stop_all() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1").with_device("d1")
        )).await.unwrap();

        manager.start_all().await;
        manager.stop_all().await;

        let stats = manager.stats().await;
        assert_eq!(stats.running_adapters, 0);
    }

    #[tokio::test]
    async fn test_manager_stats() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1")
                .with_device("d1")
                .with_device("d2")
        )).await.unwrap();
        manager.register(Arc::new(
            MockAdapter::new("adapter2")
                .with_device("d3")
        )).await.unwrap();

        let stats = manager.stats().await;
        assert_eq!(stats.total_adapters, 2);
        assert_eq!(stats.total_devices, 3);
    }

    #[tokio::test]
    async fn test_manager_list_all_devices() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1")
                .with_device("d1")
                .with_device("d2")
        )).await.unwrap();
        manager.register(Arc::new(
            MockAdapter::new("adapter2")
                .with_device("d2")
                .with_device("d3")
        )).await.unwrap();

        let devices = manager.list_all_devices().await;
        assert_eq!(devices.len(), 3); // d1, d2, d3 (d2 deduplicated)
    }

    #[tokio::test]
    async fn test_manager_find_adapter_for_device() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1")
                .with_device("d1")
        )).await.unwrap();
        manager.register(Arc::new(
            MockAdapter::new("adapter2")
                .with_device("d2")
        )).await.unwrap();

        let adapter = manager.find_adapter_for_device("d1").await;
        assert_eq!(adapter, Some("adapter1".to_string()));
    }

    #[tokio::test]
    async fn test_manager_unregister() {
        let manager = AdapterManager::new();
        let adapter = Arc::new(MockAdapter::new("test_adapter"));

        manager.register(adapter).await.unwrap();
        assert!(manager.get_adapter_info("test_adapter").await.is_some());

        manager.unregister("test_adapter").await.unwrap();
        assert!(manager.get_adapter_info("test_adapter").await.is_none());
    }

    #[tokio::test]
    async fn test_manager_health_check() {
        let manager = AdapterManager::new();

        manager.register(Arc::new(
            MockAdapter::new("adapter1")
        )).await.unwrap();

        let health = manager.health_check().await;
        assert_eq!(health.get("adapter1"), Some(&false));

        manager.start_adapter("adapter1").await.unwrap();
        let health = manager.health_check().await;
        assert_eq!(health.get("adapter1"), Some(&true));
    }

    #[tokio::test]
    async fn test_manager_config() {
        let config = AdapterManagerConfig {
            auto_start: true,
            ..Default::default()
        };
        let manager = AdapterManager::with_config(config.clone());

        assert_eq!(manager.config().auto_start, true);

        let adapter = Arc::new(MockAdapter::new("test_adapter"));
        manager.register(adapter).await.unwrap();

        // Should auto-start
        let info = manager.get_adapter_info("test_adapter").await.unwrap();
        assert!(matches!(info.status, AdapterStatus::Running));
    }
}
