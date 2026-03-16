//! Isolated Extension Manager
//!
//! This module provides a manager for process-isolated extensions that works
//! alongside the standard ExtensionRegistry. It allows extensions to be loaded
//! in isolated mode without modifying the core registry structure.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     API Layer                                │
//! │  (checks IsolatedExtensionManager first, then Registry)     │
//! └─────────────────────────────────────────────────────────────┘
//!           │                              │
//!           ▼                              ▼
//! ┌─────────────────────────┐    ┌─────────────────────────┐
//! │ IsolatedExtensionManager │    │   ExtensionRegistry     │
//! │ - Manages isolated exts  │    │ - Manages in-process    │
//! │ - Process lifecycle      │    │ - Standard loading      │
//! │ - IPC communication      │    │ - Direct calls          │
//! └─────────────────────────┘    └─────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::sync::broadcast;

use super::process::{IsolatedExtension, IsolatedExtensionConfig};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::loader::{IsolatedExtensionLoader, IsolatedLoaderConfig};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue};
use crate::extension::event_dispatcher::EventDispatcher;

/// Configuration for the isolated extension manager
#[derive(Debug, Clone)]
pub struct IsolatedManagerConfig {
    /// Base configuration for isolated extensions
    pub extension_config: IsolatedExtensionConfig,
    /// Whether to use isolated mode by default
    pub isolated_by_default: bool,
    /// Extensions that should always run in isolated mode
    pub force_isolated: Vec<String>,
    /// Extensions that should always run in-process
    pub force_in_process: Vec<String>,
}

impl Default for IsolatedManagerConfig {
    fn default() -> Self {
        Self {
            extension_config: IsolatedExtensionConfig::default(),
            // Default to isolated mode for safety
            isolated_by_default: true,
            force_isolated: Vec::new(),
            force_in_process: Vec::new(),
        }
    }
}

/// Information about a loaded isolated extension
#[derive(Debug, Clone)]
pub struct IsolatedExtensionInfo {
    /// Extension descriptor (unified capabilities)
    pub descriptor: crate::extension::system::ExtensionDescriptor,
    /// Path to extension binary
    pub path: PathBuf,
    /// Runtime state
    pub runtime: crate::extension::system::ExtensionRuntimeState,
}

// Keep backward-compatible accessor fields
impl IsolatedExtensionInfo {
    /// Get extension metadata
    pub fn metadata(&self) -> &ExtensionMetadata {
        &self.descriptor.metadata
    }

    /// Get extension commands
    pub fn commands(&self) -> &[crate::extension::system::ExtensionCommand] {
        &self.descriptor.commands
    }

    /// Get extension metrics
    pub fn metrics(&self) -> &[crate::extension::system::MetricDescriptor] {
        &self.descriptor.metrics
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.runtime.is_running
    }

    /// Get restart count
    pub fn restart_count(&self) -> u64 {
        self.runtime.restart_count
    }
}

/// Manager for process-isolated extensions
///
/// This manager handles extensions that run in separate processes,
/// providing complete isolation from the main NeoMind process.
pub struct IsolatedExtensionManager {
    /// Isolated extensions by ID
    extensions: AsyncRwLock<HashMap<String, Arc<IsolatedExtension>>>,
    /// Extension info cache
    info_cache: RwLock<HashMap<String, IsolatedExtensionInfo>>,
    /// Configuration
    config: IsolatedManagerConfig,
    /// Loader for isolated extensions
    loader: IsolatedExtensionLoader,
    /// Event dispatcher for pushing events to extensions
    event_dispatcher: Arc<EventDispatcher>,
    /// Capability provider for handling capability requests from extensions
    capability_provider: AsyncRwLock<Option<Arc<dyn super::super::context::ExtensionCapabilityProvider>>>,
    /// Death notification channel for monitoring extension crashes
    death_channel: (broadcast::Sender<()>, AsyncRwLock<broadcast::Receiver<()>>),
}

impl IsolatedExtensionManager {
    /// Create a new isolated extension manager
    pub fn new(config: IsolatedManagerConfig) -> Self {
        let loader_config = IsolatedLoaderConfig {
            isolated_config: config.extension_config.clone(),
            use_isolated_by_default: config.isolated_by_default,
            force_isolated: config.force_isolated.clone(),
            force_in_process: config.force_in_process.clone(),
        };

        // Create event dispatcher (simplified version)
        let event_dispatcher = Arc::new(EventDispatcher::new());

        // Create death notification channel
        let (death_tx, death_rx) = broadcast::channel(16);
        let death_channel = (death_tx, AsyncRwLock::new(death_rx));

        Self {
            extensions: AsyncRwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            config,
            loader: IsolatedExtensionLoader::new(loader_config),
            event_dispatcher,
            capability_provider: AsyncRwLock::new(None),
            death_channel,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(IsolatedManagerConfig::default())
    }

    /// Start the background task that monitors extension crashes and auto-restarts them
    ///
    /// This should be called once when the manager is created, in an async context.
    pub fn start_death_monitoring(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut rx = self.death_channel.1.read().await.resubscribe();
            
            tracing::info!("Extension death monitoring task started");
            
            loop {
                match rx.recv().await {
                    Ok(_) => {
                        // An extension died - check all extensions and restart dead ones
                        tracing::warn!("Received extension death notification, checking for dead extensions...");
                        
                        let extensions = self.extensions.read().await;
                        let dead_extensions: Vec<String> = extensions
                            .iter()
                            .filter(|(_, ext)| !ext.is_alive())
                            .map(|(id, _)| id.clone())
                            .collect();
                        drop(extensions);
                        
                        for ext_id in dead_extensions {
                            // 🔧 Phase 1: Check restart policy before attempting restart
                            let should_restart = {
                                let info_cache = self.info_cache.read();
                                info_cache.get(&ext_id)
                                    .map(|info| {
                                        let config = &self.config.extension_config;
                                        let can_restart = config.restart_on_crash;
                                        let within_limit = info.runtime.restart_count < config.max_restart_attempts as u64;

                                        // Check cooldown period
                                        let past_cooldown = if let Some(last_restart) = info.runtime.last_restart_at {
                                            let now = chrono::Utc::now().timestamp();
                                            (now - last_restart) >= config.restart_cooldown_secs as i64
                                        } else {
                                            true
                                        };

                                        can_restart && within_limit && past_cooldown
                                    })
                                    .unwrap_or(false)
                            };

                            if !should_restart {
                                tracing::warn!(
                                    extension_id = %ext_id,
                                    "Auto-restart skipped: policy limit reached (max_attempts={}, cooldown={}s)",
                                    self.config.extension_config.max_restart_attempts,
                                    self.config.extension_config.restart_cooldown_secs
                                );
                                continue;
                            }

                            tracing::warn!(extension_id = %ext_id, "Extension died, attempting auto-restart...");
                            
                            // Get the extension path from info cache
                            let path = {
                                let info = self.info_cache.read();
                                info.get(&ext_id).map(|info| info.path.clone())
                            };
                            
                            if let Some(path) = path {
                                // Remove the dead extension first
                                {
                                    let mut extensions = self.extensions.write().await;
                                    extensions.remove(&ext_id);
                                }
                                
                                // Reload the extension
                                match self.load(&path).await {
                                    Ok(_) => {
                                        // 🔧 Phase 1: Update restart tracking
                                        {
                                            let mut info_cache = self.info_cache.write();
                                            if let Some(info) = info_cache.get_mut(&ext_id) {
                                                info.runtime.last_restart_at = Some(chrono::Utc::now().timestamp());
                                                info.runtime.restart_count += 1;
                                            }
                                        }
                                        let restart_count = {
                                            let info_cache = self.info_cache.read();
                                            info_cache.get(&ext_id).map(|i| i.runtime.restart_count).unwrap_or(0)
                                        };
                                        tracing::info!(
                                            extension_id = %ext_id,
                                            restart_count,
                                            "Successfully restarted extension after crash"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!(extension_id = %ext_id, error = %e, "Failed to restart extension after crash");
                                    }
                                }
                            } else {
                                tracing::error!(extension_id = %ext_id, "Cannot restart extension - path not found in cache");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Death monitoring channel error, restarting task");
                        // Resubscribe and continue
                        rx = self.death_channel.1.read().await.resubscribe();
                    }
                }
            }
        });
    }


    /// Set the capability provider for handling capability requests from extensions
    pub async fn set_capability_provider(&self, provider: Arc<dyn super::super::context::ExtensionCapabilityProvider>) {
        *self.capability_provider.write().await = Some(provider.clone());
        
        // Update all existing extensions
        let extensions = self.extensions.read().await;
        for (_, ext) in extensions.iter() {
            ext.set_capability_provider(provider.clone());
        }
    }

    /// Get the event dispatcher
    pub fn event_dispatcher(&self) -> Arc<EventDispatcher> {
        self.event_dispatcher.clone()
    }

    /// Check if an extension should use isolated mode
    pub fn should_use_isolated(&self, extension_id: &str) -> bool {
        self.loader.should_use_isolated(extension_id)
    }

    /// Load an extension in isolated mode
    pub async fn load(&self, path: &Path) -> IsolatedResult<ExtensionMetadata> {
        tracing::debug!(
            path = %path.display(),
            "Loading extension in isolated mode"
        );

        let loaded = self.loader.load_isolated(path).await?;

        // Get the complete descriptor
        let descriptor = loaded.descriptor().await.ok_or_else(|| {
            IsolatedExtensionError::SpawnFailed("Failed to get extension descriptor".to_string())
        })?;

        let id = descriptor.id().to_string();

        // Get event subscriptions from extension
        tracing::debug!(
            extension_id = %id,
            "Getting event subscriptions from extension"
        );
        let event_types = match loaded.get_event_subscriptions().await {
            Ok(types) => {
                tracing::debug!(
                    extension_id = %id,
                    event_types = ?types,
                    "Got event subscriptions from extension"
                );
                types
            }
            Err(e) => {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Failed to get event subscriptions from extension"
                );
                vec![]
            }
        };

        // Get event push channel from extension
        let event_push_channel = loaded.get_event_push_channel().await;

        // Register extension with event dispatcher
        if let Some(channel) = event_push_channel {
            self.event_dispatcher.register_isolated_extension(id.clone(), event_types, channel);
        } else {
            tracing::warn!(
                extension_id = %id,
                "No event push channel available for extension"
            );
        }

        // Store extension
        self.extensions.write().await.insert(id.clone(), loaded.clone());

        // Set capability provider if configured
        if let Some(provider) = self.capability_provider.read().await.as_ref() {
            loaded.set_capability_provider(provider.clone());

        // Set up death notification for auto-restart
        loaded.set_death_notification(self.death_channel.0.clone()).await;
        }

        // Create runtime state
        let mut runtime = crate::extension::system::ExtensionRuntimeState::isolated();
        runtime.is_running = loaded.is_alive();
        runtime.loaded_at = Some(chrono::Utc::now().timestamp());

        // Store info
        self.info_cache.write().insert(
            id.clone(),
            IsolatedExtensionInfo {
                descriptor,
                path: path.to_path_buf(),
                runtime,
            },
        );

        tracing::debug!(
            extension_id = %id,
            "Extension loaded in isolated mode"
        );

        // Return metadata from the info cache
        let info = self.info_cache.read().get(&id).cloned();
        Ok(info.map(|i| i.descriptor.metadata).unwrap())
    }

    /// Unload an extension
    pub async fn unload(&self, id: &str) -> IsolatedResult<()> {
        let mut extensions = self.extensions.write().await;

        if let Some(isolated) = extensions.remove(id) {
            // Stop the extension process
            // Ignore NotRunning error - extension may have failed to start (e.g., missing .dylib)
            if let Err(e) = isolated.stop().await {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Error stopping extension during unload (continuing cleanup)"
                );
            }
            self.info_cache.write().remove(id);

            // ✅ FIX: Unregister from event dispatcher to prevent sending events to unloaded extension
            self.event_dispatcher.unregister_extension(id);

            tracing::debug!(
                extension_id = %id,
                "Extension unloaded"
            );
        }

        Ok(())
    }

    /// Execute a command on an isolated extension
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> IsolatedResult<serde_json::Value> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.execute_command(command, args).await
    }

    /// Get metrics from an isolated extension
    pub async fn get_metrics(&self, id: &str) -> IsolatedResult<Vec<ExtensionMetricValue>> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.produce_metrics().await
    }

    /// Check health of an isolated extension
    pub async fn health_check(&self, id: &str) -> IsolatedResult<bool> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.health_check().await
    }

    /// Get statistics from an isolated extension
    pub async fn get_stats(&self, id: &str) -> IsolatedResult<crate::extension::system::ExtensionStats> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.get_stats().await
    }

    /// Check if an extension is registered
    pub async fn contains(&self, id: &str) -> bool {
        self.extensions.read().await.contains_key(id)
    }

    /// Get extension info
    pub fn get_info(&self, id: &str) -> Option<IsolatedExtensionInfo> {
        self.info_cache.read().get(id).cloned()
    }

    /// List all isolated extensions
    pub async fn list(&self) -> Vec<IsolatedExtensionInfo> {
        self.info_cache.read().values().cloned().collect()
    }

    /// Get count of isolated extensions
    pub async fn count(&self) -> usize {
        self.extensions.read().await.len()
    }

    /// Check if an extension is running
    pub async fn is_running(&self, id: &str) -> bool {
        let extensions = self.extensions.read().await;
        extensions.get(id).map(|e| e.is_alive()).unwrap_or(false)
    }

    /// Get an isolated extension by ID
    pub async fn get(&self, id: &str) -> Option<Arc<IsolatedExtension>> {
        self.extensions.read().await.get(id).cloned()
    }

    /// Stop all extensions
    pub async fn stop_all(&self) {
        let mut extensions = self.extensions.write().await;

        for (id, isolated) in extensions.iter() {
            if let Err(e) = isolated.stop().await {
                tracing::warn!(
                    extension_id = %id,
                    error = %e,
                    "Failed to stop extension"
                );
            }
        }

        extensions.clear();
        self.info_cache.write().clear();

        tracing::debug!("All isolated extensions stopped");
    }

    /// Get the loader configuration
    pub fn config(&self) -> &IsolatedManagerConfig {
        &self.config
    }
}

impl Drop for IsolatedExtensionManager {
    fn drop(&mut self) {
        // Attempt to stop all extensions on drop
        // Note: This is a best-effort cleanup
        if let Ok(extensions) = self.extensions.try_read() {
            // Collect the extensions to stop
            let to_stop: Vec<(String, std::sync::Arc<IsolatedExtension>)> = extensions
                .iter()
                .filter(|(_, isolated)| isolated.is_alive())
                .map(|(id, isolated)| (id.clone(), isolated.clone()))
                .collect();

            drop(extensions); // Release read lock

            for (id, isolated) in to_stop {
                tracing::warn!(
                    extension_id = %id,
                    "Extension still running during drop, stopping"
                );
                // Use block_in_place to allow async inside drop
                tokio::task::block_in_place(|| {
                    // Create a new runtime for the stop operation
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok();
                    if let Some(rt) = rt {
                        rt.block_on(async {
                            let _ = isolated.stop().await;
                        });
                    }
                });
            }
        }

        // Clear the extensions map
        if let Ok(mut extensions) = self.extensions.try_write() {
            extensions.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = IsolatedManagerConfig::default();
        assert!(config.isolated_by_default);
        assert!(config.force_isolated.is_empty());
        assert!(config.force_in_process.is_empty());
    }

    #[test]
    fn test_manager_creation() {
        let manager = IsolatedExtensionManager::with_defaults();
        assert_eq!(tokio::runtime::Runtime::new().unwrap().block_on(async {
            manager.count().await
        }), 0);
    }
}
