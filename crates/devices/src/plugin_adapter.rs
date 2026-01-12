//! Device adapter as UnifiedPlugin implementation.
//!
//! This module provides a bridge between the DeviceAdapter trait and the UnifiedPlugin trait,
//! allowing device adapters (MQTT, Modbus, HASS, etc.) to be managed as plugins.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use futures::StreamExt;
use anyhow::anyhow;

use crate::adapter::{DeviceAdapter, AdapterError};
use edge_ai_core::EventBus;
use edge_ai_core::plugin::{
    ExtendedPluginMetadata, PluginType, PluginState, PluginError, PluginStats,
    StateMachine, PluginMetadata, PluginPermission,
};

/// Wrapper that makes a DeviceAdapter compatible with UnifiedPlugin.
pub struct DeviceAdapterPlugin {
    /// Metadata
    metadata: ExtendedPluginMetadata,
    /// State machine
    state_machine: StateMachine,
    /// The underlying adapter
    adapter: Arc<dyn DeviceAdapter>,
    /// Statistics
    stats: PluginStats,
    /// Event bus for publishing events
    event_bus: EventBus,
    /// Running flag for async coordination
    running: Arc<AtomicBool>,
    /// Event task handle
    event_task: Option<tokio::task::JoinHandle<()>>,
}

impl DeviceAdapterPlugin {
    /// Create a new device adapter plugin.
    pub fn new(
        adapter: Arc<dyn DeviceAdapter>,
        event_bus: EventBus,
    ) -> Self {
        let adapter_name = adapter.name();
        let adapter_type = adapter.adapter_type();

        // Build display name with type
        let display_name = format!("{} ({})", adapter_name, adapter_type);

        // Create base metadata
        let base = PluginMetadata::new(adapter_name, &display_name, "1.0.0", ">=1.0.0")
            .with_description(format!("{} device adapter", adapter_type))
            .with_type("device_adapter")
            .with_type("adapter");

        // Create extended metadata
        let version = semver::Version::new(1, 0, 0);
        let required_neotalk_version = semver::Version::new(1, 0, 0);

        let metadata = ExtendedPluginMetadata {
            base,
            plugin_type: PluginType::DeviceAdapter,
            version,
            required_neotalk_version,
            dependencies: vec![],
            config_schema: None,
            resource_limits: None,
            permissions: vec![
                PluginPermission::DeviceRead,
                PluginPermission::EventPublish,
                PluginPermission::EventSubscribe,
            ],
            homepage: None,
            repository: None,
            license: None,
        };

        Self {
            metadata,
            state_machine: StateMachine::new(),
            adapter,
            stats: PluginStats::default(),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            event_task: None,
        }
    }

    /// Get the underlying adapter.
    pub fn adapter(&self) -> &Arc<dyn DeviceAdapter> {
        &self.adapter
    }

    /// Get the number of devices managed by this adapter.
    pub fn device_count(&self) -> usize {
        self.adapter.device_count()
    }

    /// List device IDs managed by this adapter.
    pub fn list_devices(&self) -> Vec<String> {
        self.adapter.list_devices()
    }
}

#[async_trait::async_trait]
impl edge_ai_core::plugin::UnifiedPlugin for DeviceAdapterPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, _config: &serde_json::Value) -> Result<(), PluginError> {
        self.state_machine.transition(PluginState::Initialized, "Initialization".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        // Check if already running
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Start the underlying adapter
        self.adapter.start().await
            .map_err(|e| PluginError::InitializationFailed(format!("Failed to start adapter: {}", e)))?;

        // Update state
        self.state_machine.transition(PluginState::Running, "Start".to_string())
            .map_err(|e| PluginError::InitializationFailed(e.to_string()))?;

        self.running.store(true, Ordering::Relaxed);
        self.stats.record_start();

        // Spawn event forwarding task
        let adapter = self.adapter.clone();
        let running = self.running.clone();
        let event_bus = self.event_bus.clone();

        let handle = tokio::spawn(async move {
            let mut rx = adapter.subscribe();
            while running.load(Ordering::Relaxed) {
                match rx.next().await {
                    Some(event) => {
                        let neotalk_event = event.to_neotalk_event();
                        // Publish directly to event bus
                        event_bus.publish(neotalk_event).await;
                    }
                    None => break,
                }
            }
        });

        self.event_task = Some(handle);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        // Stop the event task
        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.event_task.take() {
            handle.abort();
        }

        // Stop the underlying adapter
        self.adapter.stop().await
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to stop adapter: {}", e)))?;

        // Update state
        self.state_machine.transition(PluginState::Stopped, "Stop".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        self.stats.record_stop(0);

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), PluginError> {
        // Stop if running
        if self.running.load(Ordering::Relaxed) {
            self.stop().await?;
        }

        // Transition to loaded state
        self.state_machine.transition(PluginState::Loaded, "Shutdown".to_string())
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state_machine.current().clone()
    }

    async fn health_check(&self) -> Result<(), PluginError> {
        if !self.running.load(Ordering::Relaxed) {
            return Err(PluginError::Other(anyhow!("Adapter not running")));
        }

        if self.adapter.is_running() {
            Ok(())
        } else {
            Err(PluginError::Other(anyhow!("Adapter is not healthy")))
        }
    }

    fn get_stats(&self) -> PluginStats {
        let mut stats = self.stats.clone();
        // Override start_count with device count
        stats.start_count = self.adapter.device_count() as u64;
        stats
    }

    async fn handle_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        match command {
            "list_devices" => {
                let devices = self.adapter.list_devices();
                Ok(serde_json::json!({
                    "devices": devices,
                    "count": devices.len()
                }))
            }
            "device_count" => {
                let count = self.adapter.device_count();
                Ok(serde_json::json!({ "count": count }))
            }
            "status" => {
                Ok(serde_json::json!({
                    "running": self.adapter.is_running(),
                    "state": self.get_state(),
                    "adapter_type": self.adapter.adapter_type(),
                    "name": self.adapter.name(),
                }))
            }
            "get_info" => {
                Ok(serde_json::json!({
                    "id": self.metadata.base.id,
                    "name": self.metadata.base.name,
                    "adapter_type": self.adapter.adapter_type(),
                    "running": self.adapter.is_running(),
                    "device_count": self.adapter.device_count(),
                    "version": self.metadata.version.to_string(),
                }))
            }
            _ => {
                Err(PluginError::Other(anyhow!("Unknown command: {}", command)))
            }
        }
    }
}

/// Factory for creating device adapter plugins.
pub struct DeviceAdapterPluginFactory;

impl DeviceAdapterPluginFactory {
    /// Create a plugin wrapper for any DeviceAdapter.
    pub fn create_plugin(
        adapter: Arc<dyn DeviceAdapter>,
        event_bus: EventBus,
    ) -> Arc<RwLock<DeviceAdapterPlugin>> {
        Arc::new(RwLock::new(DeviceAdapterPlugin::new(adapter, event_bus)))
    }

    /// Create a plugin wrapper with custom metadata.
    pub fn create_plugin_with_metadata(
        adapter: Arc<dyn DeviceAdapter>,
        event_bus: EventBus,
        metadata: ExtendedPluginMetadata,
    ) -> Arc<RwLock<DeviceAdapterPlugin>> {
        Arc::new(RwLock::new(DeviceAdapterPlugin {
            metadata,
            state_machine: StateMachine::new(),
            adapter,
            stats: PluginStats::default(),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            event_task: None,
        }))
    }
}

/// Information about a device managed by an adapter plugin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterDeviceInfo {
    /// Device ID
    pub id: String,
    /// Device name (if available)
    pub name: Option<String>,
    /// Device type
    pub device_type: String,
    /// Connection status
    pub status: String,
    /// Last seen timestamp
    pub last_seen: i64,
    /// Associated plugin ID
    pub plugin_id: String,
}

/// Statistics for device adapter plugins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceAdapterStats {
    /// Total number of device adapter plugins
    pub total_adapters: usize,
    /// Number of running adapters
    pub running_adapters: usize,
    /// Total number of devices across all adapters
    pub total_devices: usize,
    /// Per-adapter statistics
    pub adapters: Vec<AdapterPluginInfo>,
}

/// Information about an adapter plugin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterPluginInfo {
    /// Plugin ID
    pub id: String,
    /// Plugin name
    pub name: String,
    /// Adapter type (mqtt, modbus, hass, etc.)
    pub adapter_type: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
    /// Whether the plugin is running
    pub running: bool,
    /// Number of devices managed
    pub device_count: usize,
    /// Plugin state
    pub state: String,
    /// Version
    pub version: String,
    /// Uptime in seconds
    pub uptime_secs: Option<u64>,
    /// Last activity timestamp
    pub last_activity: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_plugin_info_serialization() {
        let info = AdapterPluginInfo {
            id: "test-mqtt".to_string(),
            name: "Test MQTT Adapter".to_string(),
            adapter_type: "mqtt".to_string(),
            enabled: true,
            running: true,
            device_count: 5,
            state: "Running".to_string(),
            version: "1.0.0".to_string(),
            uptime_secs: Some(3600),
            last_activity: 1234567890,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test-mqtt"));

        let deserialized: AdapterPluginInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-mqtt");
    }

    #[tokio::test]
    async fn test_device_adapter_stats_serialization() {
        let stats = DeviceAdapterStats {
            total_adapters: 2,
            running_adapters: 1,
            total_devices: 10,
            adapters: vec![],
        };

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"total_adapters\":2"));
    }
}
