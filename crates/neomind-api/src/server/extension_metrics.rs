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
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use neomind_core::datasource::DataSourceId;

// Use ExtensionMetricsStorage from extension_state instead of device TimeSeriesStorage
use crate::server::state::ExtensionMetricsStorage;

/// Per-extension collection state
struct ExtensionCollectionState {
    /// Last collection timestamp (Unix timestamp in seconds)
    last_collection: i64,
    /// Configured collection interval in seconds (0 = disabled, None = use default)
    collect_interval: Option<u64>,
}

/// Extension metrics collector - periodically collects and stores extension metrics.
pub struct ExtensionMetricsCollector {
    /// Single-path extension runtime for isolated extensions.
    runtime: Arc<neomind_core::extension::ExtensionRuntime>,
    /// Extension metrics storage (decoupled from device system)
    metrics_storage: Arc<ExtensionMetricsStorage>,
    /// Default collection interval (60 seconds)
    default_interval: Duration,
    /// Per-extension collection state
    extension_states: RwLock<HashMap<String, ExtensionCollectionState>>,
    /// Per-extension timestamp of last descriptor refresh, used to throttle
    /// `refresh_descriptor` IPC calls so dynamic metrics become visible
    /// without imposing a per-poll round-trip on every extension.
    descriptor_refresh_at: RwLock<HashMap<String, Instant>>,
    /// Maximum staleness for the cached descriptor. After this duration the
    /// next collection cycle refreshes the descriptor from the extension.
    descriptor_ttl: Duration,
    /// Hard ceiling on a single `refresh_descriptor` IPC round-trip. The
    /// underlying `command_timeout_secs` default is 300s, which would stall
    /// the entire collector pipeline against an old runner that does not
    /// recognize the `GetDescriptor` message. We bound it here so a mixed
    /// deployment degrades gracefully (skip the refresh, keep stale cache).
    descriptor_refresh_timeout: Duration,
    /// Event bus for publishing ExtensionOutput events (triggers event-driven agents)
    event_bus: Option<Arc<neomind_core::EventBus>>,
}

impl ExtensionMetricsCollector {
    /// Create a new collector.
    pub fn new(
        runtime: Arc<neomind_core::extension::ExtensionRuntime>,
        metrics_storage: Arc<ExtensionMetricsStorage>,
    ) -> Self {
        Self {
            runtime,
            metrics_storage,
            default_interval: Duration::from_secs(60),
            extension_states: RwLock::new(HashMap::new()),
            descriptor_refresh_at: RwLock::new(HashMap::new()),
            descriptor_ttl: Duration::from_secs(60),
            descriptor_refresh_timeout: Duration::from_secs(10),
            event_bus: None,
        }
    }

    /// Set the default collection interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.default_interval = interval;
        self
    }

    /// Set the event bus for publishing ExtensionOutput events.
    pub fn with_event_bus(mut self, event_bus: Arc<neomind_core::EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set the descriptor refresh TTL. After this duration the next
    /// collection cycle asks the extension for a fresh descriptor so
    /// dynamic metrics become visible. Default: 60s.
    pub fn with_descriptor_ttl(mut self, ttl: Duration) -> Self {
        self.descriptor_ttl = ttl;
        self
    }

    /// Set the hard timeout for a single `refresh_descriptor` IPC call.
    /// This is a safety bound against mixed-version deployments where the
    /// runner may not recognize the request — without this the call would
    /// block for the full `command_timeout_secs` (default 300s) before
    /// failing. Default: 10s.
    pub fn with_descriptor_refresh_timeout(mut self, timeout: Duration) -> Self {
        self.descriptor_refresh_timeout = timeout;
        self
    }

    /// Get the collect_interval from extension config_parameters.
    /// Returns None if not configured (use default), Some(0) if disabled.
    fn get_collect_interval(info: &neomind_core::extension::ExtensionRuntimeInfo) -> Option<u64> {
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
        tokio::time::sleep(Duration::from_secs(3)).await;

        info!(
            category = "extensions",
            "Extension metrics collector started (default interval: {:?})", self.default_interval
        );

        // First collection after a short delay (no need to wait full interval)
        tokio::time::sleep(Duration::from_secs(2)).await;
        debug!(
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
            debug!(category = "extensions", "Collecting metrics");

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
        debug!(category = "extensions", "collect_and_store() - starting");
        let extensions = self.runtime.list().await;
        debug!(
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

            // Best-effort descriptor refresh so dynamic metrics (e.g.
            // `fps.cam1`) added since the last TTL window become visible
            // to `/api/extensions`. Errors and timeouts are non-fatal: the
            // collector continues with whatever descriptor is currently
            // cached. The timeout is a hard ceiling that protects the
            // pipeline against old runners that don't recognize the
            // `GetDescriptor` IPC message (which would otherwise stall for
            // the full `command_timeout_secs`).
            let need_refresh = {
                let map = self
                    .descriptor_refresh_at
                    .read()
                    .unwrap_or_else(|e| e.into_inner());
                map.get(&extension_id)
                    .map(|t| t.elapsed() >= self.descriptor_ttl)
                    .unwrap_or(true)
            };
            if need_refresh {
                debug!(
                    category = "extensions",
                    extension_id = %extension_id,
                    "Refreshing descriptor (TTL elapsed)"
                );
                match tokio::time::timeout(
                    self.descriptor_refresh_timeout,
                    self.runtime.refresh_descriptor(&extension_id),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!(
                        category = "extensions",
                        extension_id = %extension_id,
                        error = %e,
                        "Descriptor refresh failed; continuing with cached descriptor"
                    ),
                    Err(_) => warn!(
                        category = "extensions",
                        extension_id = %extension_id,
                        timeout_ms = self.descriptor_refresh_timeout.as_millis() as u64,
                        "Descriptor refresh timed out (old runner or stuck IPC); \
                         continuing with cached descriptor"
                    ),
                }
                self.descriptor_refresh_at
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(extension_id.clone(), Instant::now());
            }

            // Re-fetch runtime info after the (possible) refresh so this
            // cycle uses up-to-date descriptors. The list() call returns
            // cached info and is cheap. If refresh failed or nothing
            // changed, this returns the same data.
            let info = if need_refresh {
                match self
                    .runtime
                    .list()
                    .await
                    .into_iter()
                    .find(|i| i.metadata.id == extension_id)
                {
                    Some(fresh) => fresh,
                    None => info,
                }
            } else {
                info
            };

            // Skip extensions with no metrics
            if info.metrics.is_empty() {
                debug!(
                    category = "extensions",
                    "Extension {} has no metrics, skipping", extension_id
                );
                continue;
            }

            // Check if we should collect metrics for this extension based on its collect_interval
            let should_collect = {
                let mut states = self
                    .extension_states
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
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

            debug!(
                category = "extensions",
                "Extension {} has {} metrics",
                extension_id,
                info.metrics.len()
            );

            // For extensions with get_all_metrics command, call it first to populate cache
            let has_get_all_metrics = info.commands.iter().any(|c| c.name == "get_all_metrics");
            if has_get_all_metrics {
                debug!(
                    category = "extensions",
                    "Calling get_all_metrics for extension {}", extension_id
                );
                let _ = self
                    .runtime
                    .execute_command(&extension_id, "get_all_metrics", &serde_json::json!({}))
                    .await;
            }

            let metric_values = self.runtime.get_metrics(&extension_id).await;

            debug!(
                category = "extensions",
                "Got {} metric values",
                metric_values.len()
            );

            if metric_values.is_empty() {
                // An extension that DECLARES metrics returning zero values
                // is a broken IPC (stale registration, crashed runner) —
                // debug hides it until someone notices a flat chart.
                // Warn with the declared count so it's actionable.
                if !info.metrics.is_empty() {
                    warn!(
                        category = "extensions",
                        extension_id = %extension_id,
                        declared = info.metrics.len(),
                        "Extension declares {} metrics but produced 0 values —                          IPC may be broken (stale registration or crashed runner);                          consider reloading the extension",
                        info.metrics.len()
                    );
                } else {
                    debug!(
                        category = "extensions",
                        extension_id = %extension_id,
                        "No metric values produced"
                    );
                }
                continue;
            }

            // Store each metric value
            for metric_value in metric_values {
                debug!(
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

                // Use DataSourceId source_part() and metric_part() for storage API
                match self
                    .metrics_storage
                    .write(
                        &source_id.source_part(),
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

                        // Publish ExtensionOutput event to trigger event-driven agents
                        if let Some(ref bus) = self.event_bus {
                            let core_value = match &value_for_event {
                                neomind_devices::mdl::MetricValue::Integer(n) => {
                                    neomind_core::MetricValue::Integer(*n)
                                }
                                neomind_devices::mdl::MetricValue::Float(f) => {
                                    neomind_core::MetricValue::Float(*f)
                                }
                                neomind_devices::mdl::MetricValue::String(s) => {
                                    neomind_core::MetricValue::String(s.clone())
                                }
                                neomind_devices::mdl::MetricValue::Boolean(b) => {
                                    neomind_core::MetricValue::Boolean(*b)
                                }
                                other => neomind_core::MetricValue::String(format!("{:?}", other)),
                            };
                            bus.publish_sync(neomind_core::NeoMindEvent::ExtensionOutput {
                                extension_id: extension_id.clone(),
                                output_name: metric_value.name.clone(),
                                value: core_value,
                                timestamp: chrono::Utc::now().timestamp(),
                                labels: None,
                                quality: None,
                            });
                        }
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

        if total_errors > 0 {
            info!(
                category = "extensions",
                total_metrics, total_errors, "Extension metrics collection completed with errors"
            );
        } else {
            debug!(
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
    runtime: Arc<neomind_core::extension::ExtensionRuntime>,
    metrics_storage: Arc<ExtensionMetricsStorage>,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    let collector = ExtensionMetricsCollector::new(runtime, metrics_storage)
        .with_interval(Duration::from_secs(interval_secs));

    tokio::spawn(async move {
        collector.run().await;
    })
}
