//! Extension Metrics Collection Service
//!
//! This module provides a background service that periodically collects metrics
//! from registered extensions and stores them in the extension-specific time-series database.
//!
//! Uses typed DataSourceId for clean data source identification.
//! Fully decoupled from device system - uses independent ExtensionMetricsStorage.
//! Now with circuit breaker integration for safety.
//! Publishes ExtensionOutput events for real-time dashboard updates.
//!
//! Supports per-extension collection intervals via config_parameters.collect_interval.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tracing::{debug, info, warn};

use neomind_core::{datasource::DataSourceId, event::NeoMindEvent, MetricValue as CoreMetricValue};

use base64;

// Use ExtensionMetricsStorage from extension_state instead of device TimeSeriesStorage
use crate::server::state::ExtensionMetricsStorage;

/// Convert neomind_devices::mdl::MetricValue to neomind_core::MetricValue
fn convert_metric_value(value: neomind_devices::mdl::MetricValue) -> CoreMetricValue {
    match value {
        neomind_devices::mdl::MetricValue::Float(f) => CoreMetricValue::Float(f),
        neomind_devices::mdl::MetricValue::Integer(i) => CoreMetricValue::Integer(i),
        neomind_devices::mdl::MetricValue::Boolean(b) => CoreMetricValue::Boolean(b),
        neomind_devices::mdl::MetricValue::String(s) => CoreMetricValue::String(s),
        neomind_devices::mdl::MetricValue::Array(arr) => {
            // Convert array to JSON
            CoreMetricValue::Json(serde_json::json!(arr))
        }
        neomind_devices::mdl::MetricValue::Binary(bytes) => {
            // Convert binary to base64 string
            CoreMetricValue::String(base64::encode(bytes))
        }
        neomind_devices::mdl::MetricValue::Null => {
            // Convert null to a string representation
            CoreMetricValue::String("null".to_string())
        }
    }
}

/// Per-extension collection state
struct ExtensionCollectionState {
    /// Last collection timestamp (Unix timestamp in seconds)
    last_collection: i64,
    /// Configured collection interval in seconds (0 = disabled, None = use default)
    collect_interval: Option<u64>,
}

/// Extension metrics collector - periodically collects and stores extension metrics.
pub struct ExtensionMetricsCollector {
    /// Extension registry for accessing extensions
    extension_registry: Arc<neomind_core::extension::registry::ExtensionRegistry>,
    /// Extension metrics storage (decoupled from device system)
    metrics_storage: Arc<ExtensionMetricsStorage>,
    /// Event bus for publishing metric update events
    event_bus: Option<Arc<neomind_core::EventBus>>,
    /// Default collection interval (60 seconds)
    default_interval: Duration,
    /// Per-extension collection state
    extension_states: RwLock<HashMap<String, ExtensionCollectionState>>,
}

impl ExtensionMetricsCollector {
    /// Create a new collector.
    pub fn new(
        extension_registry: Arc<neomind_core::extension::registry::ExtensionRegistry>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
        event_bus: Option<Arc<neomind_core::EventBus>>,
    ) -> Self {
        Self {
            extension_registry,
            metrics_storage,
            event_bus,
            default_interval: Duration::from_secs(60),
            extension_states: RwLock::new(HashMap::new()),
        }
    }

    /// Set the default collection interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.default_interval = interval;
        self
    }

    /// Get the collect_interval from extension config_parameters.
    /// Returns None if not configured (use default), Some(0) if disabled.
    fn get_collect_interval(info: &neomind_core::extension::registry::ExtensionInfo) -> Option<u64> {
        if let Some(ref params) = info.metadata.config_parameters {
            for param in params {
                if param.name == "collect_interval" {
                    // Try to get the value from default_value
                    if let Some(ref default) = param.default_value {
                        match default {
                            neomind_core::extension::system::ParamMetricValue::Integer(n) => {
                                return Some(*n as u64);
                            }
                            neomind_core::extension::system::ParamMetricValue::Float(f) => {
                                return Some(*f as u64);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        None
    }

    /// Run the collector as a background task.
    ///
    /// This periodically calls `produce_metrics()` on all registered extensions
    /// and stores the returned values in the extension metrics database.
    /// Each extension can have its own collection interval via config_parameters.collect_interval.
    pub async fn run(self) {
        // Wait for server to fully initialize before starting
        tokio::time::sleep(Duration::from_secs(5)).await;

        info!(
            category = "extensions",
            "Extension metrics collector started (default interval: {:?})", self.default_interval
        );

        // First loop iteration - wait before collecting
        tokio::time::sleep(self.default_interval).await;
        info!(
            category = "extensions",
            "About to collect metrics for first time"
        );

        if let Err(e) = self.collect_and_store().await {
            warn!(
                category = "extensions",
                error = %e,
                "Failed to collect extension metrics"
            );
        }

        loop {
            tokio::time::sleep(self.default_interval).await;
            info!(category = "extensions", "Collecting metrics");

            if let Err(e) = self.collect_and_store().await {
                warn!(
                    category = "extensions",
                    error = %e,
                    "Failed to collect extension metrics"
                );
            }
        }
    }

    /// Collect metrics from all extensions and store them.
    async fn collect_and_store(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(category = "extensions", "collect_and_store() - starting");
        let extensions = self.extension_registry.list().await;
        info!(
            category = "extensions",
            "Got {} extensions",
            extensions.len()
        );

        if extensions.is_empty() {
            debug!("No extensions to collect metrics from");
            return Ok(());
        }

        let now = chrono::Utc::now().timestamp();
        let mut total_metrics = 0;
        let mut total_errors = 0;

        for info in extensions {
            let extension_id = info.metadata.id.clone();

            // Skip extensions with no metrics
            if info.metrics.is_empty() {
                info!(
                    category = "extensions",
                    "Extension {} has no metrics, skipping", extension_id
                );
                continue;
            }

            // Check if we should collect metrics for this extension based on its collect_interval
            let should_collect = {
                let mut states = self.extension_states.write().unwrap();
                let state = states.entry(extension_id.clone()).or_insert_with(|| {
                    // Initialize state from extension config
                    let interval = Self::get_collect_interval(&info);
                    ExtensionCollectionState {
                        last_collection: 0,
                        collect_interval: interval,
                    }
                });

                // Update collect_interval in case config changed
                state.collect_interval = Self::get_collect_interval(&info);

                match state.collect_interval {
                    Some(0) => {
                        // collect_interval = 0 means disabled
                        debug!(
                            category = "extensions",
                            extension_id = %extension_id,
                            "Extension collection disabled (collect_interval=0)"
                        );
                        false
                    }
                    Some(interval_secs) => {
                        // Check if enough time has passed
                        let elapsed = now - state.last_collection;
                        if elapsed >= interval_secs as i64 {
                            state.last_collection = now;
                            true
                        } else {
                            debug!(
                                category = "extensions",
                                extension_id = %extension_id,
                                elapsed_secs = elapsed,
                                interval_secs = interval_secs,
                                "Skipping collection, interval not elapsed"
                            );
                            false
                        }
                    }
                    None => {
                        // No interval configured, use default (always collect)
                        state.last_collection = now;
                        true
                    }
                }
            };

            if !should_collect {
                continue;
            }

            info!(
                category = "extensions",
                "Extension {} has {} metrics",
                extension_id,
                info.metrics.len()
            );

            // Get the actual extension instance
            let extension = match self.extension_registry.get(&extension_id).await {
                Some(e) => e,
                None => {
                    warn!(
                        category = "extensions",
                        "Extension {} not found in registry", extension_id
                    );
                    continue;
                }
            };

            // For WASM extensions, try calling get_all_metrics first to populate cache
            // Check if extension has get_all_metrics command
            let has_get_all_metrics = info.commands.iter().any(|c| c.name == "get_all_metrics");
            if has_get_all_metrics {
                debug!(
                    category = "extensions",
                    "Calling get_all_metrics for extension {}", extension_id
                );
                let _ = self
                    .extension_registry
                    .execute_command(&extension_id, "get_all_metrics", &serde_json::json!({}))
                    .await;
            }

            // Call produce_metrics() to get current values
            // Note: produce_metrics is a synchronous method; we wrap it in catch_unwind
            // and also notify the safety manager on repeated failures.
            let ext = extension.read().await;
            let metric_values = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ext.produce_metrics().unwrap_or_default()
            })) {
                Ok(values) => values,
                Err(_) => {
                    warn!(
                        category = "extensions",
                        extension_id = %extension_id,
                        "Extension produce_metrics() call panicked, returning empty metrics"
                    );
                    // Record panic with safety manager so misbehaving extensions can be disabled
                    self.extension_registry
                        .safety_manager()
                        .record_panic(&extension_id)
                        .await;
                    Vec::new()
                }
            };

            info!(
                category = "extensions",
                "Got {} metric values",
                metric_values.len()
            );

            if metric_values.is_empty() {
                debug!(
                    category = "extensions",
                    extension_id = %extension_id,
                    "No metric values produced"
                );
                continue;
            }

            // Store each metric value
            for metric_value in metric_values {
                info!(
                    category = "extensions",
                    "Storing metric {}", metric_value.name
                );

                // Use typed DataSourceId for clean data source identification
                let source_id = DataSourceId::extension(&extension_id, &metric_value.name);

                // Convert milliseconds to seconds (ExtensionMetricValue uses milliseconds, but storage uses seconds)
                let timestamp = metric_value.timestamp / 1000;

                // Convert ExtensionMetricValue to MetricValue
                let value = match metric_value.value {
                    neomind_core::extension::ParamMetricValue::Integer(n) => {
                        neomind_devices::mdl::MetricValue::Integer(n)
                    }
                    neomind_core::extension::ParamMetricValue::Float(f) => {
                        neomind_devices::mdl::MetricValue::Float(f)
                    }
                    neomind_core::extension::ParamMetricValue::String(s) => {
                        neomind_devices::mdl::MetricValue::String(s)
                    }
                    neomind_core::extension::ParamMetricValue::Boolean(b) => {
                        neomind_devices::mdl::MetricValue::Boolean(b)
                    }
                    _ => {
                        // Default to string for unknown types
                        neomind_devices::mdl::MetricValue::String(format!(
                            "{:?}",
                            metric_value.value
                        ))
                    }
                };

                // Clone value for event publishing before moving into DataPoint
                let value_for_event = value.clone();
                let data_point = neomind_devices::telemetry::DataPoint::new(timestamp, value);

                // Use DataSourceId device_part() and metric_part() for storage API
                match self
                    .metrics_storage
                    .write(
                        &source_id.device_part(),
                        source_id.metric_part(),
                        data_point,
                    )
                    .await
                {
                    Ok(_) => {
                        debug!(
                            category = "extensions",
                            extension_id = %extension_id,
                            metric = %metric_value.name,
                            source = %source_id.storage_key(),
                            "Stored metric value"
                        );
                        total_metrics += 1;

                        // DISABLED: Event publishing causes runtime crashes
                        // ExtensionOutput events are no longer published to WebSocket streams
                    }
                    Err(e) => {
                        warn!(
                            category = "extensions",
                            extension_id = %extension_id,
                            metric = %metric_value.name,
                            source = %source_id.storage_key(),
                            error = %e,
                            "Failed to store metric value"
                        );
                        total_errors += 1;
                    }
                }
            }
        }

        if total_metrics > 0 || total_errors > 0 {
            info!(
                category = "extensions",
                total_metrics, total_errors, "Extension metrics collection completed"
            );
        }

        Ok(())
    }

    /// Perform a one-time collection of metrics (for manual triggering).
    pub async fn collect_once(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.collect_and_store().await
    }
}

/// Spawn the extension metrics collector as a background task.
pub fn spawn_extension_metrics_collector(
    extension_registry: Arc<neomind_core::extension::registry::ExtensionRegistry>,
    metrics_storage: Arc<ExtensionMetricsStorage>,
    event_bus: Option<Arc<neomind_core::EventBus>>,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    let collector = ExtensionMetricsCollector::new(extension_registry, metrics_storage, event_bus)
        .with_interval(Duration::from_secs(interval_secs));

    tokio::spawn(async move {
        collector.run().await;
    })
}
