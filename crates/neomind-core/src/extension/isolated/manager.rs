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
use tokio::sync::broadcast;
use tokio::sync::RwLock as AsyncRwLock;

use super::process::{IsolatedExtension, IsolatedExtensionConfig};
use super::{IsolatedExtensionError, IsolatedResult};
use crate::extension::event_dispatcher::EventDispatcher;
use crate::extension::loader::{IsolatedExtensionLoader, IsolatedLoaderConfig};
use crate::extension::system::{ExtensionMetadata, ExtensionMetricValue};

/// Configuration for the isolated extension manager
#[derive(Debug, Clone)]
pub struct IsolatedManagerConfig {
    /// Base configuration for isolated extensions
    pub extension_config: IsolatedExtensionConfig,
    /// Whether to use isolated mode by default
    pub isolated_by_default: bool,
    /// Extensions that should always run in isolated mode
    pub force_isolated: Vec<String>,
}

impl Default for IsolatedManagerConfig {
    fn default() -> Self {
        Self {
            extension_config: IsolatedExtensionConfig::default(),
            isolated_by_default: true,
            force_isolated: Vec::new(),
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
    capability_provider:
        AsyncRwLock<Option<Arc<dyn super::super::context::ExtensionCapabilityProvider>>>,
    /// Death notification channel for monitoring extension crashes
    death_channel: (broadcast::Sender<()>, AsyncRwLock<broadcast::Receiver<()>>),
    /// Optional callback invoked after crash recovery restart, to apply saved config etc.
    /// Parameters: (extension_id, extension_path)
    #[allow(clippy::type_complexity)]
    on_crash_recovery_restart: std::sync::RwLock<Option<Arc<dyn Fn(&str, &Path) + Send + Sync>>>,
    /// Optional callback invoked when crash recovery restart fails.
    /// Parameters: (extension_id, error_message)
    #[allow(clippy::type_complexity)]
    on_crash_recovery_failed: std::sync::RwLock<Option<Arc<dyn Fn(&str, &str) + Send + Sync>>>,
    /// Per-extension loading locks to prevent race conditions during concurrent loads
    /// Maps extension ID to a mutex that must be held during loading
    loading_locks: AsyncRwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl IsolatedExtensionManager {
    /// Create a new isolated extension manager
    pub fn new(config: IsolatedManagerConfig) -> Self {
        let loader_config = IsolatedLoaderConfig {
            isolated_config: config.extension_config.clone(),
            use_isolated_by_default: config.isolated_by_default,
            force_isolated: config.force_isolated.clone(),
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
            loading_locks: AsyncRwLock::new(HashMap::new()),
            on_crash_recovery_restart: std::sync::RwLock::new(None),
            on_crash_recovery_failed: std::sync::RwLock::new(None),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(IsolatedManagerConfig::default())
    }

    /// Set a callback to be invoked after crash recovery restart.
    /// The callback receives (extension_id, extension_path) and can apply saved config, etc.
    #[allow(clippy::type_complexity)]
    pub fn set_on_crash_recovery_restart(&self, callback: Arc<dyn Fn(&str, &Path) + Send + Sync>) {
        if let Ok(mut guard) = self.on_crash_recovery_restart.write() {
            *guard = Some(callback);
        }
    }

    /// Set a callback to be invoked when crash recovery restart fails.
    /// The callback receives (extension_id, error_message).
    #[allow(clippy::type_complexity)]
    pub fn set_on_crash_recovery_failed(&self, callback: Arc<dyn Fn(&str, &str) + Send + Sync>) {
        if let Ok(mut guard) = self.on_crash_recovery_failed.write() {
            *guard = Some(callback);
        }
    }

    /// Kill orphaned extension runner processes left over from a previous session.
    ///
    /// When NeoMind crashes or is force-killed, child `neomind-extension-runner`
    /// processes become orphans. They keep dylib files open, which can cause
    /// `dlopen()` hangs in newly spawned runners. This must be called **before**
    /// loading any extensions.
    pub fn cleanup_orphaned_runners() {
        // Find the extension runner binary name to search for
        let runner_name = if cfg!(windows) {
            "neomind-extension-runner.exe"
        } else {
            "neomind-extension-runner"
        };

        #[cfg(unix)]
        {
            use std::process::Command as StdCommand;

            // Locate candidate runner processes via `pgrep -f`.
            let pgrep_output = StdCommand::new("pgrep").arg("-f").arg(runner_name).output();
            let candidate_pids: Vec<u32> = match pgrep_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|line| line.trim().parse::<u32>().ok())
                    .collect(),
                // pgrep exits 1 when nothing matched — not an error.
                Ok(_) => Vec::new(),
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "pgrep not available, skipping orphan cleanup"
                    );
                    return;
                }
            };

            if candidate_pids.is_empty() {
                tracing::debug!("No orphaned extension runner processes found");
                return;
            }

            let current_pid = std::process::id();

            // Edge case: when NeoMind itself runs as PID 1 (container init
            // systems, sidecar images), every live runner we spawn is also
            // reparented to PID 1, so the "PPID == 1 means orphan" heuristic
            // no longer distinguishes our own runners from real orphans. In
            // that deployment the cleanup would murder our own freshly-loaded
            // extensions, so skip and let the caller manage cleanup some other
            // way (e.g. ephemeral container restart).
            if current_pid == 1 {
                tracing::debug!(
                    candidate_count = candidate_pids.len(),
                    "Running as PID 1; cannot distinguish orphaned runners from live ones, skipping cleanup"
                );
                return;
            }

            // Kill only true orphans: processes whose parent is init (PID 1).
            // When a process's parent dies, the kernel reparents it to init,
            // so PPID == 1 is the canonical orphan signature. Live runners of
            // *any* NeoMind instance on this host have their spawning NeoMind
            // process as the parent (not init), so they are untouched — this
            // fixes the previous `pkill -f neomind-extension-runner` behavior
            // that killed every runner system-wide and broke multi-instance
            // deployments (production + staging on the same host,
            // MultiInstanceManager children, etc.).
            let mut killed = 0usize;
            for &pid in &candidate_pids {
                if pid == current_pid {
                    continue;
                }
                let ppid_output = StdCommand::new("ps")
                    .args(["-o", "ppid=", "-p", &pid.to_string()])
                    .output();
                let ppid = match ppid_output {
                    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<u32>()
                        .ok(),
                    // Process may have exited between pgrep and ps — ignore.
                    _ => None,
                };
                if ppid == Some(1) {
                    let _ = StdCommand::new("kill").arg(pid.to_string()).output();
                    killed += 1;
                }
                // Some(other_pid): not an orphan — owned by a live parent
                // (another NeoMind instance, an interactive shell, etc.).
                // Leave alone.
            }

            if killed > 0 {
                tracing::info!(
                    killed,
                    "Cleaned up orphaned extension runner processes (PPID == 1)"
                );
                // Give processes time to fully exit and release dylib handles.
                std::thread::sleep(std::time::Duration::from_millis(500));
            } else {
                tracing::debug!(
                    candidates = candidate_pids.len(),
                    "No orphaned (PPID == 1) extension runner processes needed cleanup"
                );
            }
        }

        #[cfg(windows)]
        {
            use std::process::Command as StdCommand;

            // Enumerate all runner processes with their parent PIDs via
            // PowerShell CIM. Windows does NOT reparent orphans to PID 1
            // like Unix; instead the dead parent PID stays in the process
            // table. We kill only processes whose parent no longer exists.
            let ps_output = StdCommand::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Get-CimInstance Win32_Process -Filter \"name='{}'\" | ForEach-Object {{ \"{{}},{{}}\" -f $_.ProcessId,$_.ParentProcessId }}",
                        runner_name
                    ),
                ])
                .output();

            let candidates: Vec<(u32, u32)> = match ps_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.trim().split(',').collect();
                        if parts.len() == 2 {
                            let pid = parts[0].parse::<u32>().ok()?;
                            let ppid = parts[1].parse::<u32>().ok()?;
                            Some((pid, ppid))
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => {
                    tracing::debug!(
                        "PowerShell not available for orphan detection, skipping cleanup"
                    );
                    return;
                }
            };

            if candidates.is_empty() {
                tracing::debug!("No orphaned extension runner processes found");
                return;
            }

            let current_pid = std::process::id();

            // Kill only true orphans: processes whose parent PID no longer
            // exists. Live runners of ANY NeoMind instance on this host have
            // a live parent, so they are untouched. This prevents the old
            // `taskkill /F /IM` behavior that killed every runner system-wide
            // and broke multi-instance deployments.
            let mut killed = 0usize;
            for &(pid, ppid) in &candidates {
                if pid == current_pid || ppid == current_pid {
                    // Our own process or a runner we spawned — skip
                    continue;
                }
                // Check if parent is still alive
                let parent_alive = StdCommand::new("tasklist")
                    .args(["/FI", &format!("PID eq {}", ppid), "/NH"])
                    .output();
                let is_orphan = match parent_alive {
                    Ok(o) => {
                        let out = String::from_utf8_lossy(&o.stdout);
                        // If tasklist reports "No tasks running" → parent dead → orphan
                        !out.contains(&ppid.to_string())
                    }
                    Err(_) => false, // Can't determine — leave alone
                };
                if is_orphan {
                    let _ = StdCommand::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .output();
                    killed += 1;
                }
            }

            if killed > 0 {
                tracing::info!(
                    killed,
                    "Cleaned up orphaned extension runner processes (parent dead)"
                );
                std::thread::sleep(std::time::Duration::from_millis(500));
            } else {
                tracing::debug!(
                    candidates = candidates.len(),
                    "No orphaned extension runner processes needed cleanup"
                );
            }
        }
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
                                info_cache
                                    .get(&ext_id)
                                    .map(|info| {
                                        let config = &self.config.extension_config;
                                        let can_restart = config.restart_on_crash;
                                        let within_limit = info.runtime.restart_count
                                            < config.max_restart_attempts as u64;

                                        // Check cooldown period
                                        let past_cooldown = if let Some(last_restart) =
                                            info.runtime.last_restart_at
                                        {
                                            let now = chrono::Utc::now().timestamp();
                                            (now - last_restart)
                                                >= config.restart_cooldown_secs as i64
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
                                // Notify that this extension is dead and won't be restarted
                                if let Ok(guard) = self.on_crash_recovery_failed.read() {
                                    if let Some(ref callback) = *guard {
                                        callback(
                                            &ext_id,
                                            "Crash recovery skipped: restart policy limit reached",
                                        );
                                    }
                                }
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
                                        // Warm-up health check: verify the restarted extension
                                        // can actually respond to IPC commands before declaring it ready.
                                        // This prevents a race where the frontend immediately sends
                                        // commands (e.g., get_bindings) while the extension process
                                        // is still initializing its internal state.
                                        {
                                            let extensions = self.extensions.read().await;
                                            if let Some(ext) = extensions.get(&ext_id) {
                                                match ext.health_check().await {
                                                    Ok(true) => {
                                                        tracing::info!(
                                                            extension_id = %ext_id,
                                                            "Post-restart health check passed"
                                                        );
                                                    }
                                                    Ok(false) | Err(_) => {
                                                        tracing::warn!(
                                                            extension_id = %ext_id,
                                                            "Post-restart health check failed, extension may not be fully ready"
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        // 🔧 Phase 1: Update restart tracking
                                        {
                                            let mut info_cache = self.info_cache.write();
                                            if let Some(info) = info_cache.get_mut(&ext_id) {
                                                info.runtime.last_restart_at =
                                                    Some(chrono::Utc::now().timestamp());
                                                info.runtime.restart_count += 1;
                                            }
                                        }
                                        let restart_count = {
                                            let info_cache = self.info_cache.read();
                                            info_cache
                                                .get(&ext_id)
                                                .map(|i| i.runtime.restart_count)
                                                .unwrap_or(0)
                                        };
                                        tracing::info!(
                                            extension_id = %ext_id,
                                            restart_count,
                                            "Successfully restarted extension after crash"
                                        );

                                        // Invoke crash recovery callback (e.g., apply saved config)
                                        if let Ok(guard) = self.on_crash_recovery_restart.read() {
                                            if let Some(ref callback) = *guard {
                                                callback(&ext_id, &path);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(extension_id = %ext_id, error = %e, "Failed to restart extension after crash");
                                        // Notify callback so storage can record the error
                                        if let Ok(guard) = self.on_crash_recovery_failed.read() {
                                            if let Some(ref callback) = *guard {
                                                callback(&ext_id, &e.to_string());
                                            }
                                        }
                                    }
                                }
                            } else {
                                tracing::error!(extension_id = %ext_id, "Cannot restart extension - path not found in cache");
                                // Notify that this extension is dead with no path for recovery
                                if let Ok(guard) = self.on_crash_recovery_failed.read() {
                                    if let Some(ref callback) = *guard {
                                        callback(
                                            &ext_id,
                                            "Cannot restart: extension path not found in cache",
                                        );
                                    }
                                }
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
    pub async fn set_capability_provider(
        &self,
        provider: Arc<dyn super::super::context::ExtensionCapabilityProvider>,
    ) {
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

    /// Read extension ID from manifest.json without spawning a process
    ///
    /// This is used to acquire a loading lock BEFORE spawning to prevent
    /// race conditions where multiple concurrent loads could spawn duplicate processes.
    fn read_extension_id_from_manifest(path: &Path) -> Option<String> {
        // Try to find manifest.json in the extension directory
        // For .nep packages: path is binaries/<platform>/extension.dylib, manifest is at root
        // For legacy: path is extension.dylib, manifest is in same dir

        // Try different possible locations for manifest.json
        let possible_manifest_paths = vec![
            // .nep format: go up 3 levels from extension binary
            path.parent()?.parent()?.parent()?.join("manifest.json"),
            // Legacy format: same directory as extension binary
            path.parent()?.join("manifest.json"),
        ];

        for manifest_path in possible_manifest_paths {
            if !manifest_path.exists() {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(id) = manifest.get("id").and_then(|v| v.as_str()) {
                        tracing::debug!(
                            manifest_path = %manifest_path.display(),
                            extension_id = %id,
                            "Read extension ID from manifest.json for loading lock"
                        );
                        return Some(id.to_string());
                    }
                }
            }
        }

        None
    }

    /// Load an extension in isolated mode
    ///
    /// This method uses a per-extension loading lock to prevent race conditions
    /// where multiple concurrent requests could spawn duplicate extension processes.
    pub async fn load(&self, path: &Path) -> IsolatedResult<ExtensionMetadata> {
        tracing::debug!(
            path = %path.display(),
            "Loading extension in isolated mode"
        );

        // 🔒 CRITICAL: Try to get extension ID from manifest BEFORE spawning
        // This allows us to acquire a lock early and prevent duplicate spawns
        let preloaded_id = Self::read_extension_id_from_manifest(path);

        if let Some(ref id) = preloaded_id {
            // Check if already loaded before acquiring lock (fast path)
            if self.extensions.read().await.contains_key(id) {
                tracing::debug!(
                    extension_id = %id,
                    "Extension already loaded (fast path check), returning existing metadata"
                );
                let info = self.info_cache.read().get(id).cloned();
                if let Some(info) = info {
                    return Ok(info.descriptor.metadata);
                }
            }

            // Get or create a loading lock for this extension ID
            let loading_lock = {
                let mut locks = self.loading_locks.write().await;
                locks
                    .entry(id.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone()
            };

            // Acquire the loading lock - this will wait if another load is in progress
            let _guard = loading_lock.lock().await;

            // Double-check: extension might have been loaded while we waited for the lock
            if self.extensions.read().await.contains_key(id) {
                tracing::debug!(
                    extension_id = %id,
                    "Extension already loaded (loaded by concurrent request while waiting for lock), skipping duplicate load"
                );
                let info = self.info_cache.read().get(id).cloned();
                return info.map(|i| i.descriptor.metadata).ok_or_else(|| {
                    IsolatedExtensionError::IpcError(format!(
                        "Extension {} metadata not found in cache after load",
                        id
                    ))
                });
            }

            // Now safe to spawn - we hold the lock and extension is not loaded
            let result = self.load_internal(path).await;
            if result.is_err() {
                // Clean up loading lock on failure to prevent memory leak
                self.loading_locks.write().await.remove(id);
            }
            return result;
        }

        // Fallback: couldn't read ID from manifest, load directly (legacy behavior)
        // This path doesn't have the same race condition protection but maintains compatibility
        tracing::warn!(
            path = %path.display(),
            "Could not read extension ID from manifest.json, loading without pre-lock (may have race condition)"
        );
        self.load_internal(path).await
    }

    /// Internal load implementation - called after lock is acquired
    async fn load_internal(&self, path: &Path) -> IsolatedResult<ExtensionMetadata> {
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
            self.event_dispatcher
                .register_isolated_extension(id.clone(), event_types, channel);
        } else {
            tracing::warn!(
                extension_id = %id,
                "No event push channel available for extension"
            );
        }

        // Store extension
        self.extensions
            .write()
            .await
            .insert(id.clone(), loaded.clone());

        // Set capability provider if configured
        if let Some(provider) = self.capability_provider.read().await.as_ref() {
            loaded.set_capability_provider(provider.clone());

            // Set up death notification for auto-restart
            loaded
                .set_death_notification(self.death_channel.0.clone())
                .await;
        }

        // Create runtime state.
        //
        // CRASH-LOOP TRACKING PRESERVATION: when the death monitor restarts an
        // extension it calls `load(&path)` again, which previously built a
        // fresh `ExtensionRuntimeState` and OVERWROTE the info_cache entry —
        // zeroing `restart_count` and `last_restart_at`. As a result
        // `max_restart_attempts` (default 3) could never accumulate past 1
        // and crash-looping extensions restarted forever instead of being
        // disabled. Preserve the existing restart counters when we are
        // reloading the SAME extension at the SAME path (initial install
        // has no prior entry and correctly starts at 0).
        let preserved = self.info_cache.read().get(&id).and_then(|prev| {
            if prev.path == path {
                Some((prev.runtime.restart_count, prev.runtime.last_restart_at))
            } else {
                None
            }
        });
        let mut runtime = crate::extension::system::ExtensionRuntimeState::isolated();
        runtime.is_running = loaded.is_alive();
        runtime.loaded_at = Some(chrono::Utc::now().timestamp());
        if let Some((restart_count, last_restart_at)) = preserved {
            runtime.restart_count = restart_count;
            runtime.last_restart_at = last_restart_at;
        }

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
        info.map(|i| Ok(i.descriptor.metadata)).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!(
                "Extension {} metadata not found in cache after load",
                id
            ))
        })?
    }

    /// Unload an extension
    pub async fn unload(&self, id: &str) -> IsolatedResult<()> {
        let mut extensions = self.extensions.write().await;

        if let Some(isolated) = extensions.remove(id) {
            // ✅ FIX: Mark as stopping BEFORE calling stop()
            // This prevents death monitor from mistakenly treating this as a crash
            isolated.mark_stopping();

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

        // Clean up loading lock for this extension to prevent memory leak
        self.loading_locks.write().await.remove(id);

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

        let result = isolated.execute_command(command, args).await;

        // Lazy restart: if the extension is not running, attempt to restart it once
        if let Err(IsolatedExtensionError::NotRunning) = &result {
            drop(extensions); // release read lock before potential restart

            tracing::warn!(
                extension_id = %id,
                "Extension not running on command execution, attempting lazy restart..."
            );

            if let Some(restarted_isolated) = self.try_lazy_restart(id).await {
                restarted_isolated.execute_command(command, args).await
            } else {
                result
            }
        } else {
            result
        }
    }

    /// Attempt a lazy restart of an extension that is registered but not running.
    /// Returns the isolated extension handle on success, None on failure.
    ///
    /// Uses the same per-extension loading_lock as `load()` to avoid racing
    /// with the death monitoring task.
    async fn try_lazy_restart(&self, id: &str) -> Option<std::sync::Arc<IsolatedExtension>> {
        // Check restart policy (cooldown + max attempts)
        let should_restart = {
            let info_cache = self.info_cache.read();
            let config = &self.config.extension_config;
            info_cache
                .get(id)
                .map(|info| {
                    let can_restart = config.restart_on_crash;
                    let within_limit =
                        info.runtime.restart_count < config.max_restart_attempts as u64;
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
                extension_id = %id,
                "Lazy restart skipped: policy limit reached or restart disabled"
            );
            return None;
        }

        let path = {
            let info_cache = self.info_cache.read();
            info_cache.get(id).map(|info| info.path.clone())
        }?;

        // Acquire the per-extension loading lock to avoid racing with death monitor.
        // The death monitor also calls `load()` which acquires the same lock,
        // so whichever gets here first will perform the restart; the other will
        // see the extension already loaded and return the existing metadata.
        let loading_lock = {
            let mut locks = self.loading_locks.write().await;
            locks
                .entry(id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = loading_lock.lock().await;

        // Double-check: death monitor may have already restarted while we waited
        {
            let extensions = self.extensions.read().await;
            if let Some(ext) = extensions.get(id) {
                if ext.is_alive() {
                    tracing::debug!(
                        extension_id = %id,
                        "Extension already alive after acquiring lock, skipping lazy restart"
                    );
                    return Some(Arc::clone(ext));
                }
            }
        }

        // Gracefully stop the old process before removing
        {
            let extensions = self.extensions.read().await;
            if let Some(old_ext) = extensions.get(id) {
                tracing::debug!(extension_id = %id, "Stopping old extension process before lazy restart");
                let _ = old_ext.stop().await;
            }
        }

        // Remove the dead extension
        {
            let mut extensions = self.extensions.write().await;
            extensions.remove(id);
        }

        tracing::info!(
            extension_id = %id,
            path = %path.display(),
            "Lazy restarting extension..."
        );

        match self.load(&path).await {
            Ok(_) => {
                // Update restart tracking
                {
                    let mut info_cache = self.info_cache.write();
                    if let Some(info) = info_cache.get_mut(id) {
                        info.runtime.last_restart_at = Some(chrono::Utc::now().timestamp());
                        info.runtime.restart_count += 1;
                    }
                }

                let extensions = self.extensions.read().await;
                extensions.get(id).cloned()
            }
            Err(e) => {
                tracing::error!(
                    extension_id = %id,
                    error = %e,
                    "Lazy restart failed"
                );
                None
            }
        }
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

    /// Refresh the cached descriptor of an isolated extension so that
    /// runtime-discovered dynamic metrics become visible.
    pub async fn refresh_descriptor(&self, id: &str) -> IsolatedResult<()> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.refresh_descriptor().await
    }

    /// Send config hot-reload update to a running extension
    pub async fn send_config_update(
        &self,
        id: &str,
        config: &serde_json::Value,
    ) -> IsolatedResult<()> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.send_config_update(config).await
    }

    /// Get statistics from an isolated extension
    pub async fn get_stats(
        &self,
        id: &str,
    ) -> IsolatedResult<crate::extension::system::ExtensionStats> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.get_stats().await
    }

    /// Get log entries from an isolated extension
    pub async fn get_logs(
        &self,
        id: &str,
    ) -> IsolatedResult<Vec<super::process::ExtensionLogEntry>> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        Ok(isolated.get_logs())
    }

    /// Clear log entries for an isolated extension
    pub async fn clear_logs(&self, id: &str) -> IsolatedResult<()> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        isolated.clear_logs();
        Ok(())
    }

    /// Get active stream sessions for an extension
    pub async fn get_active_sessions(&self, id: &str) -> IsolatedResult<Vec<String>> {
        let extensions = self.extensions.read().await;

        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;

        Ok(isolated.get_active_sessions().await)
    }

    /// Get event subscriptions for an extension
    pub async fn get_event_subscriptions(&self, id: &str) -> IsolatedResult<Vec<String>> {
        let extensions = self.extensions.read().await;
        let isolated = extensions.get(id).ok_or_else(|| {
            IsolatedExtensionError::IpcError(format!("Extension {} not found", id))
        })?;
        isolated.get_event_subscriptions().await
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
    }

    #[test]
    fn test_manager_creation() {
        let manager = IsolatedExtensionManager::with_defaults();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime for test");
        assert_eq!(rt.block_on(async { manager.count().await }), 0);
    }
}
