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

use tracing::{debug, error, info, warn};

use neomind_core::datasource::DataSourceId;

use crate::automation::metric_publish;
// Use ExtensionMetricsStorage from extension_state instead of device TimeSeriesStorage
use crate::server::state::ExtensionMetricsStorage;

/// Per-extension collection state
struct ExtensionCollectionState {
    /// Last collection timestamp (Unix timestamp in seconds)
    last_collection: i64,
    /// Configured collection interval in seconds (0 = disabled, None = use default)
    collect_interval: Option<u64>,
    /// Consecutive cycles where the extension DECLARED metrics but produced
    /// zero values — drives the self-heal reload below.
    consecutive_empty: u32,
    /// Self-heal reloads that did NOT restore metric flow. Some failures are
    /// not process-level (a reload can't fix them) — after MAX_HEALS the
    /// collector stops reloading and escalates instead of looping forever.
    failed_heals: u32,
    /// Whether this extension has EVER produced a value in this process.
    /// "Ok(empty) forever" is the SDK's default `produce_metrics` (declared
    /// metrics, unimplemented producer — paddle-ocr-v6 shipped like this);
    /// "Ok(empty) after a history of values" is a real regression. Only the
    /// latter deserves the broken-IPC warn and self-heal reloads.
    ever_produced: bool,
}

/// What to do about an extension that DECLARED metrics but produced zero
/// values this cycle. Pure — the boundary table is unit-tested.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum HealAction {
    /// Below the streak threshold — keep watching.
    Watch,
    /// Threshold reached, reloads remain — reload (increments failed_heals).
    Reload,
    /// Max ineffective reloads reached for the first time — escalate ONCE
    /// (increments failed_heals past the cap).
    Escalate,
    /// Already escalated — silent until a healthy collection resets state.
    Silent,
}

fn heal_action(streak: u32, threshold: u32, failed_heals: u32, max_heals: u32) -> HealAction {
    if streak < threshold {
        HealAction::Watch
    } else if failed_heals < max_heals {
        HealAction::Reload
    } else if failed_heals == max_heals {
        HealAction::Escalate
    } else {
        HealAction::Silent
    }
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
    /// Persistent extension store — used by the self-heal reload to re-apply
    /// the extension's saved config (a bare load() would reset it to defaults).
    config_store: Option<Arc<neomind_storage::extensions::ExtensionStore>>,
    /// Consecutive zero-value cycles before a self-heal reload fires.
    selfheal_threshold: u32,
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
            config_store: None,
            selfheal_threshold: 3,
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

    /// Set the persistent extension store used by the self-heal reload to
    /// re-apply an extension's saved config after a load().
    pub fn with_config_store(
        mut self,
        store: Arc<neomind_storage::extensions::ExtensionStore>,
    ) -> Self {
        self.config_store = Some(store);
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

    /// Reload an extension whose metrics IPC has been unresponsive, mirroring
    /// the manual `/extensions/{id}/reload` handler: unload → load from the
    /// on-disk binary → re-apply the saved config (a bare load() would reset
    /// the extension to defaults and silently change its behavior).
    async fn heal_extension(
        &self,
        extension_id: &str,
        info: &neomind_core::extension::ExtensionRuntimeInfo,
    ) {
        let Some(path) = info.path.clone() else {
            warn!(
                category = "extensions",
                extension_id = %extension_id,
                "self-heal: extension has no binary path recorded — cannot reload automatically"
            );
            return;
        };
        warn!(
            category = "extensions",
            extension_id = %extension_id,
            path = %path.display(),
            "self-heal: metrics IPC unresponsive for {} consecutive cycles — reloading extension",
            self.selfheal_threshold
        );
        if let Err(e) = self.runtime.unload(extension_id).await {
            warn!(
                category = "extensions",
                extension_id = %extension_id,
                error = %e,
                "self-heal: unload failed — trying load() anyway"
            );
        }
        match self.runtime.load(&path).await {
            Ok(metadata) => {
                if let Some(store) = &self.config_store {
                    if let Ok(Some(record)) = store.load(extension_id) {
                        if let Some(ref cfg) = record.config {
                            if let Err(e) =
                                self.runtime.send_config_update(&metadata.id, cfg).await
                            {
                                warn!(
                                    category = "extensions",
                                    extension_id = %extension_id,
                                    error = %e,
                                    "self-heal: failed to re-apply saved config"
                                );
                            }
                        }
                    }
                }
                info!(
                    category = "extensions",
                    extension_id = %extension_id,
                    "self-heal: extension reloaded"
                );
            }
            Err(e) => {
                error!(
                    category = "extensions",
                    extension_id = %extension_id,
                    error = %e,
                    "self-heal: reload FAILED — extension left unloaded, manual intervention needed"
                );
            }
        }
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
                        consecutive_empty: 0,
                        failed_heals: 0,
                        ever_produced: false,
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
                    // "Ok(empty) since process start" is the SDK default
                    // produce_metrics (metrics declared, producer never
                    // implemented — paddle-ocr-v6 shipped like this). A
                    // reload cannot fix that; warn + self-heal only when
                    // values USED to flow (a real regression).
                    let ever_produced = {
                        let states = self
                            .extension_states
                            .read()
                            .unwrap_or_else(|e| e.into_inner());
                        states
                            .get(&extension_id)
                            .map(|s| s.ever_produced)
                            .unwrap_or(false)
                    };
                    if !ever_produced {
                        debug!(
                            category = "extensions",
                            extension_id = %extension_id,
                            declared = info.metrics.len(),
                            "Extension declares metrics but has never produced a value — \
                             treating as unimplemented produce_metrics (not broken IPC)"
                        );
                        continue;
                    }
                    warn!(
                        category = "extensions",
                        extension_id = %extension_id,
                        declared = info.metrics.len(),
                        "Extension declares {} metrics but produced 0 values — IPC may be broken (stale registration or crashed runner)",
                        info.metrics.len()
                    );

                    // [self-heal] N consecutive zero-value cycles ⇒ the IPC
                    // is dead, not flaky. Reload the extension (unload →
                    // load → re-apply saved config) instead of warning
                    // forever — the old behavior spammed this warn every
                    // cycle for the rest of the process lifetime.
                    // Bounded: a failure a reload cannot fix (extension-side
                    // metrics bug) would otherwise reload every N cycles
                    // forever — after MAX_HEALS consecutive ineffective
                    // reloads, stop and escalate.
                    const MAX_HEALS: u32 = 3;
                    let (streak, failed_heals) = {
                        let mut states = self
                            .extension_states
                            .write()
                            .unwrap_or_else(|e| e.into_inner());
                        let state = states
                            .entry(extension_id.clone())
                            .or_insert(ExtensionCollectionState {
                                last_collection: 0,
                                collect_interval: Self::get_collect_interval(&info),
                                consecutive_empty: 0,
                                failed_heals: 0,
                                ever_produced: false,
                            });
                        state.consecutive_empty = state.consecutive_empty.saturating_add(1);
                        (state.consecutive_empty, state.failed_heals)
                    };
                    match heal_action(streak, self.selfheal_threshold, failed_heals, MAX_HEALS) {
                        HealAction::Watch => {}
                        HealAction::Reload | HealAction::Escalate => {
                            // Reset the streak and count the attempt BEFORE
                            // acting, so a failed attempt cannot re-arm every
                            // cycle (reload storm) and the escalation fires
                            // exactly once (failed_heals moves past the cap).
                            {
                                let mut states = self
                                    .extension_states
                                    .write()
                                    .unwrap_or_else(|e| e.into_inner());
                                if let Some(state) = states.get_mut(&extension_id) {
                                    state.consecutive_empty = 0;
                                    state.failed_heals += 1;
                                }
                            }
                            if failed_heals < MAX_HEALS {
                                self.heal_extension(&extension_id, &info).await;
                            } else {
                                error!(
                                    category = "extensions",
                                    extension_id = %extension_id,
                                    "self-heal gave up: {} reloads did not restore metric \
                                     flow — the failure is not process-level (likely an \
                                     extension-side metrics bug); manual intervention needed",
                                    MAX_HEALS
                                );
                            }
                        }
                        HealAction::Silent => {
                            // Already escalated: consume the streak, stay quiet
                            // until a healthy collection resets everything.
                            let mut states = self
                                .extension_states
                                .write()
                                .unwrap_or_else(|e| e.into_inner());
                            if let Some(state) = states.get_mut(&extension_id) {
                                state.consecutive_empty = 0;
                            }
                        }
                    }
                } else {
                    debug!(
                        category = "extensions",
                        extension_id = %extension_id,
                        "No metric values produced"
                    );
                }
                continue;
            }

            // Healthy collection — clear the self-heal streak, the
            // ineffective-reload counter, and latch ever_produced.
            {
                let mut states = self
                    .extension_states
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(state) = states.get_mut(&extension_id) {
                    state.consecutive_empty = 0;
                    state.failed_heals = 0;
                    state.ever_produced = true;
                }
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
                            // Unified conversion: the producer above only emits
                            // Float/String/Boolean device values, so the shared
                            // converter's Array/Binary/Null arms are unreachable here.
                            let core_value = metric_publish::devices_to_core(&value_for_event);
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

#[cfg(test)]
mod heal_action_tests {
    use super::*;

    const THRESHOLD: u32 = 3;
    const MAX_HEALS: u32 = 3;

    #[test]
    fn below_threshold_watches() {
        assert_eq!(heal_action(0, THRESHOLD, 0, MAX_HEALS), HealAction::Watch);
        assert_eq!(heal_action(2, THRESHOLD, 0, MAX_HEALS), HealAction::Watch);
        // Even an already-given-up extension stays quiet below the streak
        assert_eq!(heal_action(1, THRESHOLD, 4, MAX_HEALS), HealAction::Watch);
    }

    #[test]
    fn threshold_boundary_reloads_while_budget_remains() {
        assert_eq!(heal_action(3, THRESHOLD, 0, MAX_HEALS), HealAction::Reload);
        assert_eq!(heal_action(9, THRESHOLD, 2, MAX_HEALS), HealAction::Reload);
    }

    #[test]
    fn exactly_max_heals_escalates_once() {
        assert_eq!(heal_action(3, THRESHOLD, 3, MAX_HEALS), HealAction::Escalate);
    }

    #[test]
    fn past_max_heals_stays_silent() {
        assert_eq!(heal_action(3, THRESHOLD, 4, MAX_HEALS), HealAction::Silent);
        assert_eq!(heal_action(99, THRESHOLD, 100, MAX_HEALS), HealAction::Silent);
    }

    #[test]
    fn full_cycle_walkthrough() {
        // Cycles 1-2 watch, 3 reloads; ×3; then one escalation; then silent.
        let mut failed = 0;
        for cycle in 1..=3 {
            let streak = (cycle % 3) + 1; // 2,3,1 → streaks 2,3,1
            let action = heal_action(streak, THRESHOLD, failed, MAX_HEALS);
            if streak == 3 {
                assert_eq!(action, HealAction::Reload);
                failed += 1;
            } else {
                assert_eq!(action, HealAction::Watch);
            }
        }
        // three ineffective reloads later:
        assert_eq!(heal_action(3, THRESHOLD, 3, MAX_HEALS), HealAction::Escalate);
        assert_eq!(heal_action(3, THRESHOLD, 4, MAX_HEALS), HealAction::Silent);
        assert_eq!(heal_action(3, THRESHOLD, 5, MAX_HEALS), HealAction::Silent);
    }
}
