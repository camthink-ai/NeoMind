//! Extension management state.
//!
//! This module provides a fully decoupled extension state with independent storage.
//! Extensions no longer depend on the device system's TimeSeriesStorage.
//!
//! ## Architecture
//!
//! - `ExtensionState` manages extensions with their own metrics storage
//! - Uses SHARED TimeSeriesStorage with devices (telemetry.redb) for unified data access
//! - Extension metrics are stored with "extension:" prefix for isolation
//! - AI Agents can query both device and extension data from the same storage
//!
//! ## Process Isolation
//!
//! Extensions are loaded via `ExtensionRuntime`:
//! - **Process isolation**: All extensions run in separate processes
//! - **Crash safety**: Extension crashes don't affect the main NeoMind process
//! - **Single-path host integration**: API/state/metrics talk to one runtime service

use std::path::Path;
use std::sync::Arc;

use futures::future::join_all;
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::{ExtensionRuntime, ExtensionRuntimeConfig};

// Import ExtensionStore for loading persisted extensions
pub use neomind_storage::extensions::ExtensionStore;

/// Extension-specific time-series storage.
///
/// This wraps the device TimeSeriesStorage and shares the same database.
/// Extension metrics are isolated using the "extension:" prefix in metric names.
#[derive(Clone)]
pub struct ExtensionMetricsStorage {
    /// Shared storage with devices - same database, unified access for AI Agents
    inner: Arc<neomind_devices::TimeSeriesStorage>,
}

impl ExtensionMetricsStorage {
    /// Create extension metrics storage that shares the device TimeSeriesStorage.
    ///
    /// This is the RECOMMENDED approach - extension data is stored in the same
    /// database as device data (telemetry.redb), isolated by prefix.
    /// This allows AI Agents to query all data sources from one storage.
    pub fn with_shared_storage(storage: Arc<neomind_devices::TimeSeriesStorage>) -> Self {
        Self { inner: storage }
    }

    /// Open extension metrics storage at a separate path (DEPRECATED - use shared storage).
    ///
    /// This creates a separate database which AI Agents cannot access.
    /// Prefer using `with_shared_storage()` instead.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let storage = neomind_devices::TimeSeriesStorage::open(path)
            .map_err(|e| format!("Failed to open extension storage: {}", e))?;
        Ok(Self {
            inner: Arc::new(storage),
        })
    }

    /// Create in-memory storage for testing.
    pub fn memory() -> Result<Self, String> {
        let storage = neomind_devices::TimeSeriesStorage::memory()
            .map_err(|e| format!("Failed to create memory storage: {}", e))?;
        Ok(Self {
            inner: Arc::new(storage),
        })
    }

    /// Write a metric value to storage.
    pub async fn write(
        &self,
        source_id: &str,
        metric: &str,
        data_point: neomind_devices::telemetry::DataPoint,
    ) -> Result<(), String> {
        self.inner
            .write(source_id, metric, data_point)
            .await
            .map_err(|e| format!("Storage write failed: {}", e))
    }

    /// Query metric data from storage.
    pub async fn query(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<neomind_devices::telemetry::DataPoint>, String> {
        self.inner
            .query(source_id, metric, start, end)
            .await
            .map_err(|e| format!("Storage query failed: {}", e))
    }

    /// Flush buffered writes to disk.
    pub fn flush(&self) -> Result<(), String> {
        self.inner
            .flush()
            .map_err(|e| format!("Storage flush failed: {}", e))
    }

    /// Query the latest value for a metric.
    pub async fn query_latest(
        &self,
        source_id: &str,
        metric: &str,
    ) -> Result<Option<neomind_devices::telemetry::DataPoint>, String> {
        self.inner
            .latest(source_id, metric)
            .await
            .map_err(|e| format!("Storage query latest failed: {}", e))
    }

    /// Get available metrics for a device/extension.
    pub async fn list_metrics(&self, source_id: &str) -> Result<Vec<String>, String> {
        self.inner
            .list_metrics(source_id)
            .await
            .map_err(|e| format!("Failed to list metrics: {}", e))
    }

    /// Get all source IDs in storage.
    pub async fn list_sources(&self) -> Result<Vec<String>, String> {
        self.inner
            .list_sources()
            .await
            .map_err(|e| format!("Failed to list sources: {}", e))
    }
}

/// Extension management state.
///
/// Fully decoupled from device system with independent storage.
/// Uses ExtensionRuntime for process-isolated extension loading.
#[derive(Clone)]
pub struct ExtensionState {
    /// Internal proxy registry for streaming support and host adapters.
    pub registry: Arc<ExtensionRegistry>,

    /// Single-path extension runtime.
    pub runtime: Arc<ExtensionRuntime>,

    /// Extension metrics storage (separate from device telemetry)
    pub metrics_storage: Arc<ExtensionMetricsStorage>,

    /// Persistent extension store (shared Arc, avoids per-request open())
    pub store: Arc<ExtensionStore>,
}

impl ExtensionState {
    /// Get the event dispatcher for extension event distribution.
    ///
    /// This returns the EventDispatcher from the IsolatedExtensionManager,
    /// which is used to push events to subscribed extensions.
    pub fn get_event_dispatcher(&self) -> Option<Arc<neomind_core::extension::EventDispatcher>> {
        Some(self.runtime.get_event_dispatcher())
    }

    /// Set the capability provider for isolated extensions.
    ///
    /// This allows isolated extensions to invoke capabilities on the host process,
    /// such as writing virtual metrics to devices.
    pub async fn set_capability_provider(
        &self,
        provider: Arc<dyn neomind_core::extension::context::ExtensionCapabilityProvider>,
    ) {
        self.runtime.set_capability_provider(provider).await;
    }

    /// Create a new extension state with process isolation enabled by default.
    pub fn new(
        registry: Arc<ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
        store: Arc<ExtensionStore>,
    ) -> Self {
        let config = ExtensionRuntimeConfig::default();
        let runtime = Arc::new(ExtensionRuntime::new(Arc::clone(&registry), config));

        Self {
            registry,
            runtime,
            metrics_storage,
            store,
        }
    }

    /// Create extension state with custom isolation configuration.
    pub fn with_config(
        registry: Arc<ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
        store: Arc<ExtensionStore>,
        config: ExtensionRuntimeConfig,
    ) -> Self {
        let runtime = Arc::new(ExtensionRuntime::new(Arc::clone(&registry), config));

        Self {
            registry,
            runtime,
            metrics_storage,
            store,
        }
    }

    /// Create extension state with persistent storage.
    pub async fn with_persistence(storage_path: &str) -> Result<Self, String> {
        // Ensure data directory exists
        if let Err(e) = std::fs::create_dir_all("data") {
            return Err(format!("Failed to create data directory: {}", e));
        }

        // Create extension registry
        let registry = Arc::new(ExtensionRegistry::new());

        // Open extension metrics storage
        let metrics_storage = Arc::new(ExtensionMetricsStorage::open(std::path::Path::new(
            storage_path,
        ))?);

        // Open extension store (singleton-cached internally)
        let store = ExtensionStore::open(crate::server::paths::extension_store_path())
            .map_err(|e| format!("Failed to open extension store: {}", e))?;

        let config = ExtensionRuntimeConfig::default();
        let runtime = Arc::new(ExtensionRuntime::new(Arc::clone(&registry), config));

        Ok(Self {
            registry,
            runtime,
            metrics_storage,
            store,
        })
    }

    /// Create a minimal extension state for testing.
    #[cfg(test)]
    pub async fn minimal() -> Self {
        let registry = Arc::new(ExtensionRegistry::new());
        let runtime = Arc::new(ExtensionRuntime::with_defaults(registry.clone()));
        // Use a temp file so tests don't depend on data/ existing
        let temp_dir = std::env::temp_dir().join(format!("neomind_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("extensions.redb");
        let store = ExtensionStore::open(db_path.to_str().unwrap_or("data/extensions.redb"))
            .expect("Failed to open extension store for test");
        Self {
            registry,
            runtime,
            metrics_storage: Arc::new(
                ExtensionMetricsStorage::memory().expect("Failed to create memory storage"),
            ),
            store,
        }
    }

    /// Load extensions from persistent storage.
    ///
    /// This should be called AFTER the server is fully initialized in an async context.
    /// It loads all extensions marked with `auto_start=true` from the extension store.
    ///
    /// Extensions are loaded via ExtensionRuntime with process isolation by default.
    /// Multiple extensions are loaded concurrently with bounded parallelism (Semaphore(4)).
    pub async fn load_from_storage(&self) -> Result<usize, String> {
        // Use the shared extension store from state
        let store = &self.store;

        // Load all auto-start extensions
        let records = store
            .load_auto_start()
            .map_err(|e| format!("Failed to load extensions from storage: {}", e))?;

        if records.is_empty() {
            tracing::info!("No auto-start extensions found in storage");
            // Don't return early - continue to auto-discovery
        } else {
            tracing::info!("Found {} auto-start extension(s) in storage", records.len());
        }

        let total = records.len();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
        let mut handles = Vec::with_capacity(records.len());

        for record in records {
            let file_path = Path::new(&record.file_path).to_path_buf();

            // Check if file still exists before spawning
            if !file_path.exists() {
                // Error, not warn: a registered extension whose binary is
                // gone silently orphans dashboard widgets rendering its
                // components ("Component Load Failed / Module not found").
                tracing::error!(
                    extension_id = %record.id,
                    file_path = %record.file_path,
                    "Extension file not found, skipping auto-start — the record                      stays registered; reinstall the extension or remove stale                      dashboard widgets referencing it"
                );
                continue;
            }

            let runtime = self.runtime.clone();
            let store = self.store.clone();
            let permit = semaphore.clone();
            let record_id = record.id.clone();
            let record_name = record.name.clone();
            let record_type = record.extension_type.clone();
            let record_config = record.config.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit.acquire().await.unwrap();
                tracing::info!("Loading extension: {}", record_id);

                let load_result = runtime.load(&file_path).await;

                match load_result {
                    Ok(metadata) => {
                        // Apply saved config via ConfigUpdate IPC (NOT execute_command,
                        // because "configure" is a lifecycle method, not a registered
                        // command — execute_command would silently fail).
                        if let Some(ref config) = record_config {
                            if let Err(e) = runtime.send_config_update(&metadata.id, config).await {
                                tracing::warn!(
                                    extension_id = %metadata.id,
                                    error = %e,
                                    "Failed to apply saved config to extension"
                                );
                            } else {
                                tracing::info!(
                                    extension_id = %metadata.id,
                                    "Applied saved config to extension"
                                );
                            }
                        }

                        tracing::info!(
                            extension_id = %metadata.id,
                            name = %record_name,
                            extension_type = %record_type,
                            has_config = record_config.is_some(),
                            is_isolated = true,
                            "Loaded extension from storage"
                        );
                        Ok(metadata.id)
                    }
                    Err(e) => {
                        tracing::error!(
                            extension_id = %record_id,
                            error = %e,
                            "Failed to load extension from storage"
                        );
                        if let Err(update_e) = store.update_error_status(&record_id, &e.to_string())
                        {
                            tracing::warn!(
                                extension_id = %record_id,
                                error = %update_e,
                                "Failed to update extension error status"
                            );
                        }
                        Err(())
                    }
                }
            }));
        }

        // Collect results
        let results = join_all(handles).await;
        let loaded_count = results
            .into_iter()
            .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
            .count();

        tracing::info!(
            total = total,
            loaded = loaded_count,
            "Extension loading complete"
        );

        // If no extensions were loaded, auto-discover and register
        let loaded_count = if loaded_count == 0 {
            tracing::info!("No extensions loaded, attempting auto-discovery...");
            match self.auto_discover_and_register().await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Auto-discovered and registered {} extension(s)", count);
                    }
                    count
                }
                Err(e) => {
                    tracing::warn!("Auto-discovery failed: {}", e);
                    0
                }
            }
        } else {
            loaded_count
        };

        Ok(loaded_count)
    }

    /// Auto-discover and register extensions from default directories.
    ///
    /// Extensions are loaded via ExtensionRuntime with process isolation by default.
    ///
    /// # Safety
    ///
    /// Discovery is safe - it only reads sidecar JSON metadata files and never
    /// loads native libraries during discovery. Actual loading happens in the
    /// isolated extension-runner process.
    pub async fn auto_discover_and_register(&self) -> Result<usize, String> {
        // Discover extensions using the registry (scans filesystem, reads sidecar JSON only)
        let discovered = self.registry.discover().await;

        if discovered.is_empty() {
            tracing::info!("No extensions discovered from filesystem");
            return Ok(0);
        }

        tracing::info!("Discovered {} potential extension(s)", discovered.len());

        // Log discovered extensions for debugging
        for (path, metadata) in &discovered {
            tracing::info!(
                extension_id = %metadata.id,
                path = %path.display(),
                "Found extension"
            );
        }

        // Use the shared extension store from state
        let store = &self.store;

        let mut registered_count = 0;
        // Limit the number of extensions to load during auto-discovery to prevent resource exhaustion
        let max_to_load = 3;

        for (path, metadata) in discovered {
            if registered_count >= max_to_load {
                tracing::info!(
                    "Reached auto-discovery limit of {} extensions, skipping remaining",
                    max_to_load
                );
                break;
            }

            if self.runtime.contains(&metadata.id).await {
                continue;
            }

            // Check if extension was previously uninstalled by user
            // Skip auto-discovery for uninstalled extensions
            match store.is_uninstalled(&metadata.id) {
                Ok(true) => {
                    tracing::debug!(
                        extension_id = %metadata.id,
                        "Skipping auto-discovery for uninstalled extension"
                    );
                    continue;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        extension_id = %metadata.id,
                        error = %e,
                        "Failed to check uninstalled status"
                    );
                }
            }

            // Rate limit extension loading to prevent resource exhaustion
            if registered_count > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }

            tracing::info!(
                extension_id = %metadata.id,
                path = %path.display(),
                "Loading discovered extension"
            );

            match self.runtime.load(&path).await {
                Ok(loaded_metadata) => {
                    // Save to storage with auto_start enabled (clear uninstalled flag if set)
                    let record = neomind_storage::ExtensionRecord::new(
                        loaded_metadata.id.clone(),
                        loaded_metadata.name.clone(),
                        path.to_string_lossy().to_string(),
                        "native".to_string(),
                        loaded_metadata.version.to_string(),
                    )
                    .with_description(loaded_metadata.description.clone())
                    .with_author(loaded_metadata.author.clone())
                    .with_auto_start(true);

                    if let Err(e) = store.save(&record) {
                        tracing::warn!("Failed to save extension record: {}", e);
                    }

                    tracing::info!(
                        extension_id = %loaded_metadata.id,
                        is_isolated = true,
                        "Auto-registered extension"
                    );
                    registered_count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        extension_id = %metadata.id,
                        error = %e,
                        "Failed to load discovered extension"
                    );
                    // Record the error in the extension store
                    if let Err(update_e) = store.update_error_status(&metadata.id, &e.to_string()) {
                        tracing::warn!(
                            extension_id = %metadata.id,
                            error = %update_e,
                            "Failed to update extension error status"
                        );
                    }
                }
            }
        }

        Ok(registered_count)
    }

    /// Restore extensions from storage (alias for load_from_storage).
    pub async fn restore(&self) -> Result<usize, String> {
        self.load_from_storage().await
    }
}

// ============================================================================
// Adapter for neomind_rules ExtensionRegistry
// ============================================================================

/// Adapter that implements neomind_rules::ExtensionRegistry for ExtensionRuntime.
pub struct ExtensionRegistryAdapter {
    runtime: Arc<ExtensionRuntime>,
}

impl ExtensionRegistryAdapter {
    pub fn new(runtime: Arc<ExtensionRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl neomind_rules::extension_integration::ExtensionRegistry for ExtensionRegistryAdapter {
    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.runtime
            .execute_command(extension_id, command, args)
            .await
            .map_err(|e| e.to_string())
    }

    async fn has_extension(&self, extension_id: &str) -> bool {
        self.runtime.contains(extension_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extension_storage_write_query() {
        let storage = ExtensionMetricsStorage::memory().unwrap();

        // Write a data point
        let point = neomind_devices::telemetry::DataPoint::new(
            1234567890,
            neomind_devices::mdl::MetricValue::Float(42.5),
        );

        storage
            .write("ext_test", "test_metric", point)
            .await
            .expect("Write failed");

        // Flush write buffer to ensure data is queryable
        storage.flush().expect("Flush failed");

        // Query it back
        let points = storage
            .query("ext_test", "test_metric", 0, i64::MAX)
            .await
            .expect("Query failed");

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].timestamp, 1234567890);
    }

    #[tokio::test]
    async fn test_extension_state_create() {
        let state = ExtensionState::minimal().await;
        assert!(Arc::strong_count(&state.registry) > 0);
        assert!(Arc::strong_count(&state.metrics_storage) > 0);
    }
}
