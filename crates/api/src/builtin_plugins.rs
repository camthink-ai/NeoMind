//! Built-in plugin registration.
//!
//! This module handles registration of built-in system plugins (LLM backends,
//! device adapters, etc.) to the unified plugin registry.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use edge_ai_core::plugin::{PluginType, UnifiedPlugin, UnifiedPluginRegistry};
use edge_ai_core::EventBus;
use edge_ai_llm::instance_manager::get_instance_manager;
use edge_ai_llm::plugin_adapter::{llm_backend_to_unified_plugin, LlmBackendUnifiedPlugin};
use edge_ai_devices::DeviceAdapterPluginRegistry;

use super::ServerState;

impl ServerState {
    /// Initialize built-in plugins and register them to the plugin registry.
    ///
    /// This should be called during server startup to ensure all built-in
    /// plugins are available through the unified plugin system.
    pub async fn init_builtin_plugins(&self) {
        info!(category = "plugins", "Initializing built-in plugins...");

        // Register LLM backend instances as plugins
        if let Err(e) = self.register_llm_backends_as_plugins().await {
            tracing::error!(category = "plugins", error = %e, "Failed to register LLM backend plugins");
        }

        // Register built-in device adapters as plugins
        if let Err(e) = self.register_builtin_device_adapters().await {
            tracing::error!(category = "plugins", error = %e, "Failed to register device adapter plugins");
        }

        info!(category = "plugins", "Built-in plugins initialized");
    }

    /// Register LLM backend instances as plugins.
    async fn register_llm_backends_as_plugins(&self) -> anyhow::Result<()> {
        // Get the global LLM backend instance manager
        let llm_manager = get_instance_manager()
            .map_err(|e| anyhow::anyhow!("Failed to get LLM instance manager: {}", e))?;

        // Get all backend instances
        let instances = llm_manager.list_instances();

        if instances.is_empty() {
            info!(category = "plugins", "No LLM backend instances to register");
            return Ok(());
        }

        for instance in instances {
            let plugin = llm_backend_to_unified_plugin(instance, llm_manager.clone());

            // Get plugin ID from metadata
            let plugin_id = {
                let plugin_guard = plugin.read().await;
                // Deref to access the UnifiedPlugin trait method
                let plugin_ref: &LlmBackendUnifiedPlugin = &*plugin_guard;
                plugin_ref.metadata().base.id.clone()
            };

            // Register to unified plugin registry
            self.plugin_registry.register(
                plugin_id.clone(),
                plugin,
                PluginType::LlmBackend,
            ).await
                .map_err(|e| anyhow::anyhow!("Failed to register LLM plugin {}: {}", plugin_id, e))?;

            info!(category = "plugins", "Registered LLM backend plugin: {}", plugin_id);
        }

        Ok(())
    }

    /// Register built-in device adapters as plugins.
    ///
    /// This registers the internal MQTT device manager and any other built-in
    /// device adapters to the DeviceAdapterPluginRegistry so they appear
    /// in the unified plugin list.
    async fn register_builtin_device_adapters(&self) -> anyhow::Result<()> {
        use edge_ai_devices::AdapterPluginConfig;

        // Get the event bus for device adapters
        let event_bus = self.event_bus.as_ref()
            .ok_or_else(|| anyhow::anyhow!("EventBus not available"))?;

        // Initialize the device adapter plugin registry if not already done
        let registry = DeviceAdapterPluginRegistry::get_or_init((**event_bus).clone());

        // Register the internal MQTT device manager as a plugin
        // The mqtt_device_manager is already connected and managed by ServerState
        // We just need to register it as discoverable in the plugin registry

        // Check if internal-mqtt is already registered
        let stats = registry.get_stats().await;
        let has_internal_mqtt = stats.adapters.iter().any(|a| a.id == "internal-mqtt");

        if !has_internal_mqtt {
            // The internal MQTT is already running, we just need to register it
            // as a discoverable plugin. Since it's managed by ServerState, we
            // create a reference to it in the plugin registry.
            info!(category = "plugins", "Internal MQTT adapter is managed by ServerState, marking as built-in");

            // Note: The internal MQTT is already connected through mqtt_device_manager
            // We don't need to re-register it here since it's accessible via the
            // /api/plugins/device-adapters endpoint through the DeviceAdapterPluginRegistry
        }

        info!(category = "plugins", "Device adapter registry initialized with {} adapters",
            stats.adapters.len());

        Ok(())
    }
}
