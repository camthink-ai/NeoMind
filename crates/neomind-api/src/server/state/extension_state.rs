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

use std::path::Path;
use std::sync::Arc;

use neomind_core::extension::registry::ExtensionRegistry;

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
#[derive(Clone)]
pub struct ExtensionState {
    /// Extension registry for managing dynamically loaded extensions
    pub registry: Arc<ExtensionRegistry>,

    /// Extension metrics storage (separate from device telemetry)
    pub metrics_storage: Arc<ExtensionMetricsStorage>,
}

impl ExtensionState {
    /// Create a new extension state.
    pub fn new(
        registry: Arc<ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
    ) -> Self {
        Self {
            registry,
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

        Ok(Self {
            registry,
            metrics_storage,
        })
    }

    /// Create a minimal extension state for testing.
    #[cfg(test)]
    pub async fn minimal() -> Self {
        Self {
            registry: Arc::new(ExtensionRegistry::new()),
            metrics_storage: Arc::new(
                ExtensionMetricsStorage::memory().expect("Failed to create memory storage"),
            ),
        }
    }

    /// Load extensions from persistent storage.
    ///
    /// This should be called AFTER the server is fully initialized in an async context.
    /// It loads all extensions marked with `auto_start=true` from the extension store.
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

            // Use async load for all extensions
            // Both WASM and native extensions use async loading to avoid Tokio runtime issues
            let load_result = self.registry.load_from_path(file_path).await;

            match load_result {
                Ok(_) => {
                    // Apply saved config if present
                    if let Some(ref config) = record.config {
                        if let Some(ext) = self.registry.get(&record.id).await {
                            let mut ext_guard = ext.write().await;
                            if let Err(e) = ext_guard.configure(config).await {
                                tracing::warn!(
                                    extension_id = %record.id,
                                    error = %e,
                                    "Failed to apply saved config to extension"
                                );
                            } else {
                                tracing::info!(
                                    extension_id = %record.id,
                                    "Applied saved config to extension"
                                );
                            }
                        }
                    }

                    tracing::info!(
                        extension_id = %record.id,
                        name = %record.name,
                        extension_type = %record.extension_type,
                        has_config = record.config.is_some(),
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
                }
            }
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
    pub async fn auto_discover_and_register(&self) -> Result<usize, String> {
        // Discover extensions using the registry
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
            // Check if already registered in memory
            if self.registry.contains(&metadata.id).await {
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

            // Load the extension using async method
            match self.registry.load_from_path(&path).await {
                Ok(_) => {
                    // Save to storage with auto_start enabled (clear uninstalled flag if set)
                    let record = neomind_storage::ExtensionRecord::new(
                        metadata.id.clone(),
                        metadata.name.clone(),
                        path.to_string_lossy().to_string(),
                        "native".to_string(),
                        metadata.version.to_string(),
                    )
                    .with_description(metadata.description.clone())
                    .with_author(metadata.author.clone())
                    .with_auto_start(true);

                    if let Err(e) = store.save(&record) {
                        tracing::warn!("Failed to save extension record: {}", e);
                    }

                    tracing::info!(
                        extension_id = %metadata.id,
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
