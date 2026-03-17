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
//! ## Process Isolation (V2)
//!
//! Extensions are loaded via `UnifiedExtensionService` which provides:
//! - **Process isolation**: Extensions run in separate processes by default
//! - **Crash safety**: Extension crashes don't affect the main NeoMind process
//! - **Unified API**: Single interface for both isolated and in-process extensions

use std::path::Path;
use std::sync::Arc;

use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::unified::{UnifiedExtensionConfig, UnifiedExtensionService};

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
        device_id: &str,
        metric: &str,
        data_point: neomind_devices::telemetry::DataPoint,
    ) -> Result<(), String> {
        self.inner
            .write(device_id, metric, data_point)
            .await
            .map_err(|e| format!("Storage write failed: {}", e))
    }

    /// Query metric data from storage.
    pub async fn query(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<neomind_devices::telemetry::DataPoint>, String> {
        self.inner
            .query(device_id, metric, start, end)
            .await
            .map_err(|e| format!("Storage query failed: {}", e))
    }

    /// Query the latest value for a metric.
    pub async fn query_latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<neomind_devices::telemetry::DataPoint>, String> {
        self.inner
            .latest(device_id, metric)
            .await
            .map_err(|e| format!("Storage query latest failed: {}", e))
    }

    /// Get available metrics for a device/extension.
    pub async fn list_metrics(&self, device_id: &str) -> Result<Vec<String>, String> {
        self.inner
            .list_metrics(device_id)
            .await
            .map_err(|e| format!("Failed to list metrics: {}", e))
    }

    /// Get all device IDs in storage.
    pub async fn list_devices(&self) -> Result<Vec<String>, String> {
        self.inner
            .list_devices()
            .await
            .map_err(|e| format!("Failed to list devices: {}", e))
    }
}

/// Extension management state.
///
/// Fully decoupled from device system with independent storage.
/// Uses UnifiedExtensionService for process-isolated extension loading.
#[derive(Clone)]
pub struct ExtensionState {
    /// Extension registry for managing dynamically loaded extensions
    /// Note: This is kept for backward compatibility and direct access
    pub registry: Arc<ExtensionRegistry>,

    /// Unified extension service with process isolation support
    pub unified_service: Arc<UnifiedExtensionService>,

    /// Extension metrics storage (separate from device telemetry)
    pub metrics_storage: Arc<ExtensionMetricsStorage>,
}

impl ExtensionState {
    /// Get the event dispatcher for extension event distribution.
    ///
    /// This returns the EventDispatcher from the IsolatedExtensionManager,
    /// which is used to push events to subscribed extensions.
    pub fn get_event_dispatcher(&self) -> Option<Arc<neomind_core::extension::EventDispatcher>> {
        Some(self.unified_service.get_event_dispatcher())
    }

    /// Set the capability provider for isolated extensions.
    ///
    /// This allows isolated extensions to invoke capabilities on the host process,
    /// such as writing virtual metrics to devices.
    pub async fn set_capability_provider(&self, provider: Arc<dyn neomind_core::extension::context::ExtensionCapabilityProvider>) {
        self.unified_service.set_capability_provider(provider).await;
    }

    /// Create a new extension state with process isolation enabled by default.
    pub fn new(
        registry: Arc<ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
    ) -> Self {
        // Create unified service with process isolation by default
        let config = UnifiedExtensionConfig::default();
        let unified_service = Arc::new(UnifiedExtensionService::new(
            Arc::clone(&registry),
            config,
        ));

        Self {
            registry,
            unified_service,
            metrics_storage,
        }
    }

    /// Create extension state with custom isolation configuration.
    pub fn with_config(
        registry: Arc<ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
        config: UnifiedExtensionConfig,
    ) -> Self {
        let unified_service = Arc::new(UnifiedExtensionService::new(
            Arc::clone(&registry),
            config,
        ));

        Self {
            registry,
            unified_service,
            metrics_storage,
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

        // Create unified service with process isolation by default
        let config = UnifiedExtensionConfig::default();
        let unified_service = Arc::new(UnifiedExtensionService::new(
            Arc::clone(&registry),
            config,
        ));

        Ok(Self {
            registry,
            unified_service,
            metrics_storage,
        })
    }

    /// Create a minimal extension state for testing.
    #[cfg(test)]
    pub async fn minimal() -> Self {
        Self {
            registry: Arc::new(ExtensionRegistry::new()),
            unified_service: Arc::new(UnifiedExtensionService::with_defaults(Arc::new(
                ExtensionRegistry::new(),
            ))),
            metrics_storage: Arc::new(
                ExtensionMetricsStorage::memory().expect("Failed to create memory storage"),
            ),
        }
    }

    /// Load extensions from persistent storage.
    ///
    /// This should be called AFTER the server is fully initialized in an async context.
    /// It loads all extensions marked with `auto_start=true` from the extension store.
    ///
    /// Extensions are loaded via UnifiedExtensionService with process isolation by default.
    pub async fn load_from_storage(&self) -> Result<usize, String> {
        // Open extension store
        let store = ExtensionStore::open("data/extensions.redb")
            .map_err(|e| format!("Failed to open extension store: {}", e))?;

        // Load all auto-start extensions
        let records = store
            .load_auto_start()
            .map_err(|e| format!("Failed to load extensions from storage: {}", e))?;

        if records.is_empty() {
            tracing::info!("No auto-start extensions found in storage");
            // Don't return early - continue to auto-discovery
        }

        tracing::info!("Found {} auto-start extension(s) in storage", records.len());

        let mut loaded_count = 0;
        let total = records.len();
        let mut current = 0;


        for record in records {
            let file_path = Path::new(&record.file_path);

            // Check if file still exists
            if !file_path.exists() {
                tracing::warn!(
                    extension_id = %record.id,
                    file_path = %record.file_path,
                    "Extension file not found, skipping"
                );
                continue;
            }
            
            current += 1;
            tracing::info!(
                "Loading extension {}/{}: {}",
                current, total, record.id
            );

            // Use unified service for loading with process isolation
            let load_result = self.unified_service.load(file_path).await;

            match load_result {
                Ok(metadata) => {
                    // Apply saved config if present
                    if let Some(ref config) = record.config {
                        // Try to apply config via execute_command
                        if let Err(e) = self
                            .unified_service
                            .execute_command(&metadata.id, "configure", config)
                            .await
                        {
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

                    // Check if running in isolated mode
                    let is_isolated = self.unified_service.is_isolated(&metadata.id).await;

                    tracing::info!(
                        extension_id = %metadata.id,
                        name = %record.name,
                        extension_type = %record.extension_type,
                        has_config = record.config.is_some(),
                        is_isolated = is_isolated,
                        "Loaded extension from storage"
                    );
                    loaded_count += 1;
                }
                Err(e) => {
                    tracing::error!(
                        extension_id = %record.id,
                        error = %e,
                        "Failed to load extension from storage"
                    );
                    // Record the error in the extension store
                    if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
                        if let Err(update_e) = store.update_error_status(&record.id, &e.to_string()) {
                            tracing::warn!(
                                extension_id = %record.id,
                                error = %update_e,
                                "Failed to update extension error status"
                            );
                        }
                    }
                }
            }
        }
        
        // Small delay between extensions to reduce memory pressure
        // This prevents OOM when loading multiple heavy extensions
        if current < total {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        tracing::info!("Loaded {} extension(s) from storage", loaded_count);

        // If no extensions were loaded, auto-discover and register
        if loaded_count == 0 {
            tracing::info!("No extensions loaded, attempting auto-discovery...");
            match self.auto_discover_and_register().await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Auto-discovered and registered {} extension(s)", count);
                        loaded_count = count;
                    }
                }
                Err(e) => {
                    tracing::warn!("Auto-discovery failed: {}", e);
                }
            }
        }

        Ok(loaded_count)
    }

    /// Auto-discover and register extensions from default directories.
    ///
    /// Extensions are loaded via UnifiedExtensionService with process isolation by default.
    pub async fn auto_discover_and_register(&self) -> Result<usize, String> {
        // Discover extensions using the registry (scans filesystem)
        let discovered = self.registry.discover().await;

        if discovered.is_empty() {
            tracing::info!("No extensions discovered from filesystem");
            return Ok(0);
        }

        // Open the store for checking uninstalled status and saving records
        let store = ExtensionStore::open("data/extensions.redb")
            .map_err(|e| format!("Failed to open extension store: {}", e))?;

        let mut registered_count = 0;

        for (path, metadata) in discovered {
            // Check if already registered in memory (via unified service)
            if self.unified_service.contains(&metadata.id).await {
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

            // Load the extension using unified service (with process isolation)
            match self.unified_service.load(&path).await {
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

                    // Check if running in isolated mode
                    let is_isolated = self.unified_service.is_isolated(&loaded_metadata.id).await;

                    tracing::info!(
                        extension_id = %loaded_metadata.id,
                        is_isolated = is_isolated,
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

// ============================================================================
// Adapter for neomind_rules ExtensionRegistry
// ============================================================================

/// Adapter that implements neomind_rules::ExtensionRegistry for ExtensionRegistry.
///
/// This bridges neomind_core's ExtensionRegistry to neomind_rules' ExtensionRegistry trait.
pub struct ExtensionRegistryAdapter {
    registry: Arc<ExtensionRegistry>,
}

impl ExtensionRegistryAdapter {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self { registry }
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
        self.registry
            .execute_command(extension_id, command, args)
            .await
            .map_err(|e| e.to_string())
    }

    async fn has_extension(&self, extension_id: &str) -> bool {
        self.registry.get(extension_id).await.is_some()
    }
}

// ============================================================================
// Adapter for UnifiedValueProvider
// ============================================================================

/// Adapter that implements neomind_rules::ExtensionStorageLike for ExtensionMetricsStorage.
///
/// This allows ExtensionMetricsStorage to be used with UnifiedValueProvider.
pub struct ExtensionMetricsStorageAdapter {
    storage: Arc<ExtensionMetricsStorage>,
}

impl ExtensionMetricsStorageAdapter {
    pub fn new(storage: Arc<ExtensionMetricsStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait::async_trait]
impl neomind_rules::ExtensionStorageLike for ExtensionMetricsStorageAdapter {
    async fn query_latest(&self, extension_id: &str, metric: &str) -> Option<f64> {
        // Extension metrics are stored with "extension:" prefix
        let device_id = format!("extension:{}", extension_id);
        match self.storage.query_latest(&device_id, metric).await {
            Ok(Some(dp)) => match &dp.value {
                neomind_devices::MetricValue::Float(f) => Some(*f),
                neomind_devices::MetricValue::Integer(i) => Some(*i as f64),
                neomind_devices::MetricValue::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
                _ => None,
            },
            Ok(None) => None,
            Err(_) => None,
        }
    }
}
