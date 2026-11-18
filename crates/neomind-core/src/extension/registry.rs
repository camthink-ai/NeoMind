//! Extension registry for managing in-host extension proxies.
//!
//! The registry provides:
//! - Extension registration and lifecycle management
//! - Extension discovery from filesystem
//! - Health monitoring
//! - Safety management (circuit breaker, panic isolation)
//!
//! Note: Real extension execution is handled by the isolated runner/runtime.
//! This registry now only stores host-side proxy objects for streaming support.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::event::NeoMindEvent;
use crate::eventbus::EventBus;
use crate::extension::event_dispatcher::EventDispatcher;
use crate::extension::loader::NativeExtensionMetadataLoader;
use crate::extension::safety::ExtensionSafetyManager;
use crate::extension::system::{
    DynExtension, ExtensionError, ExtensionMetadata, ExtensionState, ExtensionStats,
};
use crate::extension::types::Result;

/// Information about a registered extension (load snapshot).
///
/// `commands`, `capability_generation`, and `effective_capabilities` are
/// captured together at register time — an old `ExtensionInfo` after reload
/// still carries the old token and the old resolved caps.
#[derive(Clone)]
pub struct ExtensionInfo {
    /// Extension metadata
    pub metadata: ExtensionMetadata,
    /// Current state
    pub state: ExtensionState,
    /// Runtime statistics
    pub stats: ExtensionStats,
    /// When the extension was loaded
    pub loaded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Metrics provided by this extension
    pub metrics: Vec<super::system::MetricDescriptor>,
    /// Commands provided by this extension
    pub commands: Vec<super::system::ExtensionCommand>,
    /// Capability-cache generation minted for this load.
    pub capability_generation: crate::extension::CapabilityGeneration,
    /// Effective capabilities resolved at this load (trusted snapshot).
    /// Published into the process classify cache at register so CLI-only
    /// cold start does not depend on prepare / direct ExtensionTool warm-up.
    pub effective_capabilities:
        std::collections::HashMap<String, crate::extension::EffectiveCommandCapability>,
    /// The instance handle FROM THIS LOAD. The entry (descriptor + token +
    /// handle) is the atomic execution snapshot: reading it once guarantees
    /// the three belong to the same load — a reload between separate reads
    /// used to allow old-descriptor+new-token or new-handle+old-token pairs.
    pub instance: DynExtension,
}

/// Registry for managing extensions.
pub struct ExtensionRegistry {
    /// Registered extensions.
    /// Using parking_lot::RwLock for better performance and no potential for
    /// blocking the async runtime (unlike std::sync::RwLock).
    extensions: RwLock<HashMap<String, DynExtension>>,
    /// Extension information cache.
    /// Using parking_lot::RwLock for consistent locking strategy.
    info_cache: RwLock<HashMap<String, ExtensionInfo>>,
    /// Native extension metadata loader
    native_loader: NativeExtensionMetadataLoader,
    /// Extension directories to scan
    extension_dirs: Vec<PathBuf>,
    /// Safety manager for circuit breaking and panic isolation
    safety_manager: std::sync::Arc<ExtensionSafetyManager>,
    /// Event bus for publishing lifecycle events (optional)
    event_bus: Option<std::sync::Arc<EventBus>>,
    /// Command execution timeout in seconds (default: 300)
    command_timeout_secs: u64,
    /// Event dispatcher for pushing events to extensions (optional)
    /// Using Option<Arc<>> for interior mutability
    event_dispatcher: parking_lot::RwLock<Option<std::sync::Arc<EventDispatcher>>>,
}

impl ExtensionRegistry {
    /// Create a new extension registry.
    pub fn new() -> Self {
        Self {
            extensions: RwLock::new(HashMap::new()),
            info_cache: RwLock::new(HashMap::new()),
            native_loader: NativeExtensionMetadataLoader::new(),
            extension_dirs: vec![],
            safety_manager: std::sync::Arc::new(ExtensionSafetyManager::new()),
            event_bus: None,
            event_dispatcher: parking_lot::RwLock::new(None),
            command_timeout_secs: 300,
        }
    }

    /// Set the command execution timeout in seconds.
    pub fn with_command_timeout_secs(mut self, secs: u64) -> Self {
        self.command_timeout_secs = secs;
        self
    }

    /// Set the event bus for publishing lifecycle events.
    pub fn set_event_bus(&mut self, event_bus: std::sync::Arc<EventBus>) {
        self.event_bus = Some(event_bus);
    }

    /// Set the event dispatcher for pushing events to extensions.
    pub fn set_event_dispatcher(&self, event_dispatcher: std::sync::Arc<EventDispatcher>) {
        *self.event_dispatcher.write() = Some(event_dispatcher);
    }

    /// Add an extension directory to scan.
    pub fn add_extension_dir(&mut self, path: PathBuf) {
        self.extension_dirs.push(path);
    }

    /// Register an extension instance.
    pub async fn register(&self, id: String, extension: DynExtension) -> Result<()> {
        self.register_with_path(id, extension, None).await
    }

    /// Register an extension with an optional file path.
    ///
    /// The file path is used to locate the extension's manifest.json for dashboard components.
    pub async fn register_with_path(
        &self,
        id: String,
        extension: DynExtension,
        file_path: Option<PathBuf>,
    ) -> Result<()> {
        let ext = extension.read().await;
        let mut metadata = ext.metadata().clone();
        let metrics = ext.metrics().to_vec();
        let commands = ext.commands().to_vec();
        drop(ext);

        // Set file path if provided
        metadata.file_path = file_path;

        // Check if already registered
        if self.extensions.read().contains_key(&id) {
            return Err(ExtensionError::AlreadyRegistered(id));
        }

        // Store extension
        self.extensions
            .write()
            .insert(id.clone(), extension.clone());

        // Mint a new capability-cache generation for this load (sole mint site),
        // resolve effective caps into the load snapshot, and publish them so
        // write-gate / CLI classify can read RO without a prior prepare or
        // direct ExtensionTool registration (CLI-only cold start).
        let capability_generation = crate::extension::begin_instance_generation(&id);
        let command_snapshot = crate::extension::ExtensionCommandSnapshot::build(
            id.clone(),
            metadata.version.clone(),
            None,
            commands.clone(),
            capability_generation.clone(),
        );
        if let Err(e) = command_snapshot.publish_capabilities() {
            tracing::warn!(
                extension_id = %id,
                error = %e,
                "failed to publish load-snapshot command capabilities; classify will fail closed to Mutating until repaired"
            );
        }

        // Register with safety manager for circuit breaking and panic tracking
        self.safety_manager.register_extension(id.clone()).await;

        // Store info
        let stats = ExtensionStats {
            start_count: 1, // First registration counts as a start
            ..Default::default()
        };

        self.info_cache.write().insert(
            id.clone(),
            ExtensionInfo {
                metadata,
                state: ExtensionState::Running,
                stats,
                loaded_at: Some(chrono::Utc::now()),
                metrics,
                commands,
                capability_generation,
                effective_capabilities: command_snapshot.effective_capabilities,
                instance: extension.clone(),
            },
        );

        // Register with event dispatcher for event subscriptions
        // This allows in-process extensions to receive events via handle_event()
        if let Some(ref dispatcher) = *self.event_dispatcher.read() {
            let dispatcher = std::sync::Arc::clone(dispatcher);
            let extension_clone = extension.clone();
            let id_clone = id.clone();
            // Use spawn instead of block_in_place — block_in_place panics on
            // current-thread runtimes (Tauri main thread). The registration is
            // fire-and-forget: the extension starts receiving events within ms.
            tokio::spawn(async move {
                dispatcher
                    .register_in_process_extension(id_clone, extension_clone)
                    .await;
            });
        }

        // Publish ExtensionLifecycle { state: "registered" } event
        // Use sync version to avoid issues with non-Tokio contexts
        if let Some(ref event_bus) = self.event_bus {
            let _ = event_bus.publish_with_source_sync(
                NeoMindEvent::ExtensionLifecycle {
                    extension_id: id.clone(),
                    state: "registered".to_string(),
                    message: Some(format!("Extension {} registered", id)),
                    timestamp: chrono::Utc::now().timestamp(),
                },
                "extension",
            );
        }

        tracing::info!("Extension registered: {}", id);
        Ok(())
    }

    /// Unregister an extension.
    pub async fn unregister(&self, id: &str) -> Result<()> {
        // Update stats before removing
        if let Some(info) = self.info_cache.write().get_mut(id) {
            info.stats.stop_count += 1;
        }

        // Publish ExtensionLifecycle { state: "unregistered" } event BEFORE removing
        // Use sync version to avoid issues with non-Tokio contexts
        if let Some(ref event_bus) = self.event_bus {
            let _ = event_bus.publish_with_source_sync(
                NeoMindEvent::ExtensionLifecycle {
                    extension_id: id.to_string(),
                    state: "unregistered".to_string(),
                    message: Some(format!("Extension {} unregistered", id)),
                    timestamp: chrono::Utc::now().timestamp(),
                },
                "extension",
            );
        }

        // Drop version-bound write-gate cache for this instance.
        super::invalidate_instance_capability(id);

        // Remove from memory
        self.extensions.write().remove(id);
        self.info_cache.write().remove(id);

        // Unregister from safety manager
        self.safety_manager.unregister_extension(id).await;

        // ✅ FIX: Unregister from event dispatcher to prevent sending events to unloaded extension
        if let Some(ref dispatcher) = *self.event_dispatcher.read() {
            dispatcher.unregister_extension(id);
        }

        tracing::info!("Extension unregistered: {}", id);
        Ok(())
    }

    /// Get an extension by ID.
    pub async fn get(&self, id: &str) -> Option<DynExtension> {
        self.extensions.read().get(id).cloned()
    }

    /// Get extension info by ID.
    pub async fn get_info(&self, id: &str) -> Option<ExtensionInfo> {
        self.info_cache.read().get(id).cloned()
    }

    /// Get current metric values from an extension.
    /// This calls the extension's `produce_metrics()` method and returns the current values.
    pub async fn get_current_metrics(&self, id: &str) -> Vec<super::system::ExtensionMetricValue> {
        if let Some(ext) = self.get(id).await {
            let ext = ext.read().await;
            // Call produce_metrics with panic handling
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ext.produce_metrics())) {
                Ok(Ok(metrics)) => metrics,
                Ok(Err(e)) => {
                    tracing::warn!(
                        extension_id = %id,
                        error = %e,
                        "[ExtensionRegistry] Extension failed to produce metrics"
                    );
                    Vec::new()
                }
                Err(_) => {
                    tracing::error!(
                        extension_id = %id,
                        "[ExtensionRegistry] Extension panicked while producing metrics"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Get extension statistics.
    /// This calls the extension's `get_stats()` method and returns the statistics.
    pub async fn get_stats(
        &self,
        id: &str,
    ) -> std::result::Result<super::system::ExtensionStats, ExtensionError> {
        if let Some(ext) = self.get(id).await {
            let ext = ext.read().await;
            // Call get_stats with panic handling
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ext.get_stats())) {
                Ok(stats) => Ok(stats),
                Err(_) => {
                    tracing::error!(
                        extension_id = %id,
                        "[ExtensionRegistry] Extension panicked while getting stats"
                    );
                    Err(ExtensionError::ExecutionFailed(
                        "Extension panicked while getting stats".to_string(),
                    ))
                }
            }
        } else {
            Err(ExtensionError::NotFound(format!(
                "Extension {} not found",
                id
            )))
        }
    }

    /// List all extensions.
    pub async fn list(&self) -> Vec<ExtensionInfo> {
        self.info_cache.read().values().cloned().collect()
    }

    /// Load an extension from a file path and register it.
    ///
    /// Note: WASM extensions should be loaded via the extension-runner process,
    /// not directly through this registry.
    pub async fn load_from_path(&self, path: &Path) -> Result<ExtensionMetadata> {
        let extension = path.extension().and_then(|e| e.to_str());

        match extension {
            Some("so") | Some("dylib") | Some("dll") => {
                Err(ExtensionError::InvalidFormat(
                    "Direct native loading has been removed; use ExtensionRuntime for isolated execution"
                        .to_string(),
                ))
            }
            Some("wasm") => Err(ExtensionError::InvalidFormat(
                "WASM extension support was removed (NeoMind 1.2.0) — build a native cdylib extension"
                    .to_string(),
            )),
            _ => Err(ExtensionError::InvalidFormat(format!(
                "Unsupported extension format: {:?}",
                path
            ))),
        }
    }

    /// Discover extensions in configured directories.
    ///
    /// Returns a list of (path, metadata) tuples for discovered extensions.
    /// Note: Only native extensions are discovered directly. WASM extensions
    /// should be discovered via the extension-runner process.
    pub async fn discover(&self) -> Vec<(PathBuf, ExtensionMetadata)> {
        let mut discovered = Vec::new();

        tracing::debug!(
            "Extension discover: starting, dirs: {:?}",
            self.extension_dirs
        );

        for dir in &self.extension_dirs {
            if !dir.exists() {
                tracing::debug!("Extension discover: directory does not exist: {:?}", dir);
                continue;
            }

            tracing::debug!("Extension discover: scanning directory: {:?}", dir);

            // Use the loader's discover method
            let native_found = self.native_loader.discover(dir).await;
            tracing::debug!(
                "Extension discover: found {} native extensions",
                native_found.len()
            );
            for (path, metadata) in native_found {
                discovered.push((path, metadata));
            }

            // Note: WASM extensions are discovered by the extension-runner,
            // not here in the registry
        }

        tracing::debug!(
            "Extension discover: complete, total found: {}",
            discovered.len()
        );
        discovered
    }

    /// Prepare from the current load snapshot, then [`Self::execute_prepared`].
    ///
    /// Single-call callers (Transform, etc.) get atomic prepare+execute.
    /// HTTP/CLI that already hold a [`PreparedInvoke`] must call
    /// [`Self::execute_prepared`] so a reload between prepare and execute
    /// cannot run a new instance under an old contract.
    pub async fn execute_command(
        &self,
        id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let snapshot = self.info_cache.read().get(id).cloned();
        let Some(info) = snapshot else {
            return Err(ExtensionError::NotFound(id.to_string()));
        };
        let Some(cmd_def) = info.commands.iter().find(|c| c.name == command) else {
            let available: Vec<&str> = info.commands.iter().map(|c| c.name.as_str()).collect();
            return Err(ExtensionError::CommandNotFound(format!(
                "'{command}' is not in the registered command catalog for '{id}'. \
                 Available: [{}]. Discover with: neomind extension commands {id}",
                available.join(", ")
            )));
        };
        let prepared = crate::extension::prepare_invoke(crate::extension::PrepareInvokeRequest {
            instance_id: id,
            command: cmd_def,
            raw_params: args,
            extension_version: Some(info.metadata.version.as_str()),
            package_digest: None,
            capability_generation: &info.capability_generation,
        })
        .map_err(ExtensionError::InvalidArguments)?;
        self.execute_prepared(&prepared).await
    }

    /// Execute a previously prepared invoke against the **bound** load
    /// snapshot (instance + generation). Does not re-resolve by id alone —
    /// if the generation expired after reload, the call is refused and the
    /// new instance is never entered.
    pub async fn execute_prepared(
        &self,
        prepared: &crate::extension::PreparedInvoke,
    ) -> Result<serde_json::Value> {
        let id = prepared.target.instance_id.as_str();
        let command = prepared.command.as_str();

        if !self.safety_manager.is_allowed(id).await {
            tracing::warn!(
                extension_id = %id,
                command = %command,
                "[ExtensionRegistry] Extension execution blocked by safety manager"
            );
            return Err(ExtensionError::SecurityError(format!(
                "Extension '{}' is temporarily disabled by safety policy",
                id
            )));
        }

        let snapshot = self.info_cache.read().get(id).cloned();
        let Some(info) = snapshot else {
            return Err(ExtensionError::NotFound(id.to_string()));
        };
        if info.capability_generation.as_str() != prepared.capability_generation {
            return Err(ExtensionError::InvalidArguments(format!(
                "stale prepared invoke for '{id}' (operation {}): capability generation \
                 expired after reload; re-discover and prepare again",
                prepared.operation_id
            )));
        }
        if !info.commands.iter().any(|c| c.name == command) {
            let available: Vec<&str> = info.commands.iter().map(|c| c.name.as_str()).collect();
            return Err(ExtensionError::CommandNotFound(format!(
                "'{command}' is not in the registered command catalog for '{id}'. \
                 Available: [{}]. Discover with: neomind extension commands {id}",
                available.join(", ")
            )));
        }

        let ext_clone = Arc::clone(&info.instance);
        let canonical_args = &prepared.canonical_params;
        let start_time = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.command_timeout_secs),
            async {
                let ext_guard = ext_clone.read().await;
                ext_guard.execute_command(command, canonical_args).await
            },
        )
        .await;
        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(value)) => {
                self.safety_manager.record_success(id).await;
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.commands_executed += 1;
                    info.stats.total_execution_time_ms += execution_time_ms;
                    info.stats.last_execution_time_ms = Some(chrono::Utc::now().timestamp_millis());
                }
                Ok(value)
            }
            Ok(Err(e)) => {
                self.safety_manager.record_failure(id).await;
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.error_count += 1;
                    info.stats.last_error = Some(e.to_string());
                }
                tracing::warn!(
                    extension_id = %id,
                    command = %command,
                    error = %e,
                    "[ExtensionRegistry] Extension command failed"
                );
                Err(e)
            }
            Err(_) => {
                self.safety_manager.record_failure(id).await;
                let error_msg = format!(
                    "Command '{}' timed out after {} seconds",
                    command, self.command_timeout_secs
                );
                if let Some(info) = self.info_cache.write().get_mut(id) {
                    info.stats.error_count += 1;
                    info.stats.last_error = Some(error_msg.clone());
                }
                tracing::error!(
                    extension_id = %id,
                    command = %command,
                    timeout_secs = self.command_timeout_secs,
                    "[ExtensionRegistry] Extension command timed out"
                );
                Err(ExtensionError::Timeout(format!(
                    "Command '{}' on extension '{}' timed out",
                    command, id
                )))
            }
        }
    }

    /// Perform health check on an extension.
    pub async fn health_check(&self, id: &str) -> Result<bool> {
        let ext = self
            .get(id)
            .await
            .ok_or_else(|| ExtensionError::NotFound(id.to_string()))?;

        let ext_clone = Arc::clone(&ext);
        let result = {
            let ext_guard = ext_clone.read().await;
            ext_guard.health_check().await
        };
        result
    }

    /// Check if an extension is registered.
    pub async fn contains(&self, id: &str) -> bool {
        self.extensions.read().contains_key(id)
    }

    /// Get the number of registered extensions.
    pub async fn count(&self) -> usize {
        self.extensions.read().len()
    }

    /// Get all registered extensions (alias for get_extensions() from trait).
    pub async fn get_all(&self) -> Vec<DynExtension> {
        self.extensions.read().values().cloned().collect()
    }

    /// Get the safety manager for this registry.
    pub fn safety_manager(&self) -> Arc<ExtensionSafetyManager> {
        Arc::clone(&self.safety_manager)
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for registries that manage extensions.
#[async_trait::async_trait]
pub trait ExtensionRegistryTrait: Send + Sync {
    /// Get all registered extensions.
    async fn get_extensions(&self) -> Vec<DynExtension>;

    /// Get a specific extension by ID.
    async fn get_extension(&self, id: &str) -> Option<DynExtension>;

    /// Execute a command on an extension.
    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String>;

    /// Get metrics from an extension.
    async fn get_metrics(&self, extension_id: &str) -> Vec<super::system::MetricDescriptor>;
}

#[async_trait::async_trait]
impl ExtensionRegistryTrait for ExtensionRegistry {
    async fn get_extensions(&self) -> Vec<DynExtension> {
        self.extensions.read().values().cloned().collect()
    }

    async fn get_extension(&self, id: &str) -> Option<DynExtension> {
        self.get(id).await
    }

    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String> {
        // Delegate to the main registry execute_command which includes timeout
        // and safety manager integration. This ensures all callers (including
        // tools and automation) go through the same protection layer.
        self.execute_command(extension_id, command, args)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_metrics(&self, extension_id: &str) -> Vec<super::system::MetricDescriptor> {
        if let Some(ext) = self.get(extension_id).await {
            // Clone the Arc to avoid holding the lock
            let ext_clone = Arc::clone(&ext);
            let metrics = {
                let ext_guard = ext_clone.read().await;
                ext_guard.metrics().to_vec()
            };
            metrics
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::system::{ExtensionCommand, MetricDefinition};
    use crate::extension::Extension;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ExtensionRegistry::new();
        assert_eq!(registry.count().await, 0);
    }

    /// Versioned fake extension: distinguishable metadata, command set and
    /// execute result, so a reload is observable through the registry entry.
    fn fake_ext(version: &'static str, cmds: &'static [&'static str]) -> DynExtension {
        use std::sync::OnceLock;
        struct Fake {
            version: &'static str,
            cmds: &'static [&'static str],
        }
        #[async_trait::async_trait]
        impl Extension for Fake {
            fn metadata(&self) -> &ExtensionMetadata {
                static M1: OnceLock<ExtensionMetadata> = OnceLock::new();
                static M2: OnceLock<ExtensionMetadata> = OnceLock::new();
                if self.version == "1.0.0" {
                    M1.get_or_init(|| ExtensionMetadata::new("fake-ext", "Fake", "1.0.0"))
                } else {
                    M2.get_or_init(|| ExtensionMetadata::new("fake-ext", "Fake", "2.0.0"))
                }
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                self.cmds
                    .iter()
                    .map(|n| ExtensionCommand {
                        name: n.to_string(),
                        display_name: n.to_string(),
                        description: String::new(),
                        payload_template: String::new(),
                        parameters: Vec::new(),
                        fixed_values: std::collections::HashMap::new(),
                        samples: Vec::new(),
                        parameter_groups: Vec::new(),
                        ai_exposed: true,
                        effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                        replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                        execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                    })
                    .collect()
            }
            async fn execute_command(
                &self,
                command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                if !self.cmds.contains(&command) {
                    // Real extensions answer CommandNotFound for unknown
                    // commands — the fake must too, or the removed-command
                    // assertion is meaningless.
                    return Err(crate::extension::types::ExtensionError::CommandNotFound(
                        command.to_string(),
                    ));
                }
                Ok(serde_json::json!({"saw": self.version, "cmd": command}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        Arc::new(tokio::sync::RwLock::new(Box::new(Fake { version, cmds })))
    }

    /// SEQUENTIAL reload coverage (review follow-up, honest scope): after a
    /// reload the entry never mixes the old descriptor with the new
    /// generation, and the pre-reload (descriptor, token) pair is refused.
    /// The single-entry structure closes the two-source splicing
    /// STRUCTURALLY; this is not a forced-concurrency interleaving test.
    #[tokio::test]
    async fn sequential_reload_entry_stays_consistent_and_old_pair_refused() {
        let registry = ExtensionRegistry::new();
        registry
            .register("mixed-ext".into(), fake_ext("1.0.0", &["cmdA"]))
            .await
            .unwrap();
        let e1 = registry.get_info("mixed-ext").await.unwrap();
        assert!(e1.commands.iter().any(|c| c.name == "cmdA"));

        // Reload: new command set, new generation.
        registry.unregister("mixed-ext").await.unwrap();
        registry
            .register("mixed-ext".into(), fake_ext("2.0.0", &["cmdB"]))
            .await
            .unwrap();
        let e2 = registry.get_info("mixed-ext").await.unwrap();
        assert!(
            e2.commands.iter().any(|c| c.name == "cmdB"),
            "new descriptor present: {:?}",
            e2.commands
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            !e2.commands.iter().any(|c| c.name == "cmdA"),
            "OLD descriptor must not pair with the NEW generation"
        );
        assert_ne!(e1.capability_generation, e2.capability_generation);

        // The pre-reload captured pair is dead: prepare with (old cmd, old
        // token) must be refused instead of executing.
        let old_cmd = e1
            .commands
            .iter()
            .find(|c| c.name == "cmdA")
            .expect("captured old descriptor");
        let err = crate::extension::prepare_invoke(crate::extension::PrepareInvokeRequest {
            instance_id: "mixed-ext",
            command: old_cmd,
            raw_params: &serde_json::json!({}),
            extension_version: Some(e1.metadata.version.as_str()),
            package_digest: None,
            capability_generation: &e1.capability_generation,
        })
        .unwrap_err();
        assert!(
            err.contains("stale") || err.contains("no active"),
            "old pair must be refused: {err}"
        );
    }

    /// Execution runs the instance bound to the SAME entry that supplied the
    /// descriptor+token — after a reload the new instance answers, and the
    /// removed command is no longer routable through the fast path.
    #[tokio::test]
    async fn execute_uses_the_snapshot_instance_after_reload() {
        let registry = ExtensionRegistry::new();
        registry
            .register("exec-ext".into(), fake_ext("1.0.0", &["cmdA"]))
            .await
            .unwrap();
        let v1 = registry
            .execute_command("exec-ext", "cmdA", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(v1["saw"], "1.0.0");

        registry.unregister("exec-ext").await.unwrap();
        registry
            .register("exec-ext".into(), fake_ext("2.0.0", &["cmdB"]))
            .await
            .unwrap();
        let v2 = registry
            .execute_command("exec-ext", "cmdB", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(v2["saw"], "2.0.0", "must run the reload's instance");
        // cmdA no longer exists on the new entry: catalog miss → CommandNotFound
        // (never reaches the extension business function).
        let err = registry
            .execute_command("exec-ext", "cmdA", &serde_json::json!({}))
            .await
            .unwrap_err();
        match err {
            ExtensionError::CommandNotFound(msg) => {
                assert!(msg.contains("cmdA"), "got: {msg}");
                assert!(
                    msg.contains("neomind extension commands"),
                    "must point at discovery: {msg}"
                );
            }
            other => panic!("expected CommandNotFound, got {other:?}"),
        }
    }

    /// Unknown names must NOT pass args through to the extension. A counting
    /// fake proves execute_command on the instance is never entered.
    #[tokio::test]
    async fn unknown_command_never_reaches_extension_business_fn() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::OnceLock;
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::SeqCst);

        struct CountingFake;
        #[async_trait::async_trait]
        impl Extension for CountingFake {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("count-ext", "Count", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "only_known".into(),
                    display_name: "Only".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"cmd": command}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let registry = ExtensionRegistry::new();
        registry
            .register(
                "count-ext".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(CountingFake))),
            )
            .await
            .unwrap();

        let err = registry
            .execute_command("count-ext", "totally_unknown", &serde_json::json!({"x": 1}))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ExtensionError::CommandNotFound(_)),
            "got {err:?}"
        );
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            0,
            "extension business fn must not run for unknown commands"
        );

        // Known command still reaches the instance once.
        registry
            .execute_command("count-ext", "only_known", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }

    /// Controllable interleaving: prepare → reload → execute_prepared must
    /// refuse; the new instance's execute count stays 0.
    #[tokio::test]
    async fn prepare_then_reload_refuses_stale_prepared_execute() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::OnceLock;
        static CALLS_V2: AtomicUsize = AtomicUsize::new(0);
        CALLS_V2.store(0, Ordering::SeqCst);

        struct V1;
        #[async_trait::async_trait]
        impl Extension for V1 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("bind-ext", "Bind", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                Ok(serde_json::json!({"v": 1}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        struct V2;
        #[async_trait::async_trait]
        impl Extension for V2 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("bind-ext", "Bind", "2.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                CALLS_V2.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"v": 2}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let registry = ExtensionRegistry::new();
        registry
            .register(
                "bind-ext".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V1))),
            )
            .await
            .unwrap();
        let info = registry.get_info("bind-ext").await.unwrap();
        let cmd = info.commands.iter().find(|c| c.name == "ping").unwrap();
        let prepared = crate::extension::prepare_invoke(crate::extension::PrepareInvokeRequest {
            instance_id: "bind-ext",
            command: cmd,
            raw_params: &serde_json::json!({}),
            extension_version: Some(info.metadata.version.as_str()),
            package_digest: None,
            capability_generation: &info.capability_generation,
        })
        .unwrap();

        // Pause → unload/reload → resume execute
        registry.unregister("bind-ext").await.unwrap();
        registry
            .register(
                "bind-ext".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V2))),
            )
            .await
            .unwrap();

        let err = registry.execute_prepared(&prepared).await.unwrap_err();
        match err {
            ExtensionError::InvalidArguments(msg) => {
                assert!(
                    msg.contains("stale prepared") || msg.contains("generation expired"),
                    "{msg}"
                );
            }
            other => panic!("expected stale InvalidArguments, got {other:?}"),
        }
        assert_eq!(
            CALLS_V2.load(Ordering::SeqCst),
            0,
            "new instance must not run for a stale prepared invoke"
        );

        // Fresh prepare on the new load still executes once.
        registry
            .execute_command("bind-ext", "ping", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(CALLS_V2.load(Ordering::SeqCst), 1);
    }

    /// Stronger interleaving: generation check has already passed and the
    /// bound instance has entered `execute_command`, then reload replaces the
    /// id — the new instance must still see call count 0 (bound Arc, not
    /// id re-lookup).
    #[tokio::test]
    async fn pass_gen_check_then_reload_before_finish_new_instance_untouched() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::OnceLock;

        static ENTERED_V1: AtomicBool = AtomicBool::new(false);
        static RELEASE_V1: AtomicBool = AtomicBool::new(false);
        static CALLS_V2: AtomicUsize = AtomicUsize::new(0);
        ENTERED_V1.store(false, Ordering::SeqCst);
        RELEASE_V1.store(false, Ordering::SeqCst);
        CALLS_V2.store(0, Ordering::SeqCst);

        struct V1;
        #[async_trait::async_trait]
        impl Extension for V1 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("bind-mid", "Bind Mid", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                ENTERED_V1.store(true, Ordering::SeqCst);
                while !RELEASE_V1.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
                Ok(serde_json::json!({"v": 1}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        struct V2;
        #[async_trait::async_trait]
        impl Extension for V2 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("bind-mid", "Bind Mid", "2.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ExtensionCommand {
                    name: "ping".into(),
                    display_name: "Ping".into(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    parameter_groups: Vec::new(),
                    ai_exposed: true,
                    effect: Some(crate::extension::system::CommandEffect::ReadOnly),
                    replay_safety: Some(crate::extension::system::CommandReplaySafety::Safe),
                    execution: Some(crate::extension::system::CommandExecutionKind::Immediate),
                }]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> Result<serde_json::Value> {
                CALLS_V2.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"v": 2}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let registry = ExtensionRegistry::new().with_command_timeout_secs(30);
        registry
            .register(
                "bind-mid".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V1))),
            )
            .await
            .unwrap();
        let info = registry.get_info("bind-mid").await.unwrap();
        let cmd = info.commands.iter().find(|c| c.name == "ping").unwrap();
        let prepared = crate::extension::prepare_invoke(crate::extension::PrepareInvokeRequest {
            instance_id: "bind-mid",
            command: cmd,
            raw_params: &serde_json::json!({}),
            extension_version: Some(info.metadata.version.as_str()),
            package_digest: None,
            capability_generation: &info.capability_generation,
        })
        .unwrap();

        let registry = Arc::new(registry);
        let reg_exec = Arc::clone(&registry);
        let prepared_clone = prepared.clone();
        let join = tokio::spawn(async move { reg_exec.execute_prepared(&prepared_clone).await });

        // Wait until gen check passed and bound V1 entered execute.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ENTERED_V1.load(Ordering::SeqCst) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "V1 never entered execute_command"
            );
            tokio::task::yield_now().await;
        }

        // Pause point: reload under the same id while V1 is mid-execute.
        registry.unregister("bind-mid").await.unwrap();
        registry
            .register(
                "bind-mid".into(),
                Arc::new(tokio::sync::RwLock::new(Box::new(V2))),
            )
            .await
            .unwrap();

        RELEASE_V1.store(true, Ordering::SeqCst);
        let out = join.await.expect("join").expect("bound V1 should finish");
        assert_eq!(out, serde_json::json!({"v": 1}));
        assert_eq!(
            CALLS_V2.load(Ordering::SeqCst),
            0,
            "new instance must not run when execute already bound to prior snapshot"
        );
    }

    /// NM-235: register publishes effective caps — first classify is RO without
    /// prepare / ExtensionTool; reload republishes for the new generation.
    #[tokio::test]
    async fn register_publishes_snapshot_caps_for_cli_only_cold_start() {
        let id = "cli-cold-ro";
        crate::extension::invalidate_instance_capability(id);
        let registry = ExtensionRegistry::new();
        registry
            .register(id.into(), fake_ext("1.0.0", &["get_weather"]))
            .await
            .unwrap();
        let info = registry.get_info(id).await.unwrap();
        assert_eq!(
            info.effective_capabilities
                .get("get_weather")
                .map(|c| c.effect),
            Some(crate::extension::system::CommandEffect::ReadOnly)
        );
        assert_eq!(
            crate::extension::sync_effect_for_invoke(id, "get_weather"),
            crate::extension::system::CommandEffect::ReadOnly,
            "cold classify after register must be RO without prepare/tool warm-up"
        );

        registry.unregister(id).await.unwrap();
        assert_eq!(
            crate::extension::sync_effect_for_invoke(id, "get_weather"),
            crate::extension::system::CommandEffect::Mutating
        );

        registry
            .register(id.into(), fake_ext("2.0.0", &["get_weather"]))
            .await
            .unwrap();
        assert_eq!(
            crate::extension::sync_effect_for_invoke(id, "get_weather"),
            crate::extension::system::CommandEffect::ReadOnly,
            "first classify after reload must still be RO"
        );
        crate::extension::invalidate_instance_capability(id);
    }
}
