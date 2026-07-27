//! Periodic emitter that refreshes the `__last_seen_age_secs` virtual metric
//! in `UnifiedValueProvider` for every device currently subscribed by a rule.
//!
//! Spawned once at server startup (see `crates/neomind-api/src/server/types.rs`).
//! Tick interval: 60s default, configurable via `with_tick_interval`.
//!
//! ## Metric semantics
//!
//! `__last_seen_age_secs` reflects **offline duration**, not raw data staleness:
//!
//! - Device online (age < `effective_offline_timeout`): metric = 0
//! - Device offline (age >= `effective_offline_timeout`): metric = actual age
//!
//! On reconnect the emitter pushes 0 once to clear the offline state, then
//! subsequent ticks skip the push (value unchanged) until the device goes
//! offline again.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use neomind_core::datasource::DataSourceId;
use neomind_devices::DeviceService;
use tokio::task::JoinHandle;

use crate::engine::RuleEngine;
use crate::models::RuleValue;
use crate::unified_provider::UnifiedValueProvider;

/// 60s default tick. Override via [`DeviceStatusEmitter::with_tick_interval`].
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Virtual metric name emitted by this task.
pub const VIRTUAL_METRIC_NAME: &str = "__last_seen_age_secs";

pub struct DeviceStatusEmitter {
    rule_engine: Arc<RuleEngine>,
    provider: Arc<UnifiedValueProvider>,
    device_service: Arc<DeviceService>,
    tick_interval: Duration,
    /// Last value pushed per device. Used to skip redundant `on_data_update`
    /// calls while a device stays online (metric pinned to 0). On reconnect
    /// the value transitions from `age` back to 0, which triggers exactly one
    /// clearing push.
    last_pushed: Mutex<HashMap<String, f64>>,
}

impl DeviceStatusEmitter {
    pub fn new(
        rule_engine: Arc<RuleEngine>,
        provider: Arc<UnifiedValueProvider>,
        device_service: Arc<DeviceService>,
    ) -> Self {
        Self {
            rule_engine,
            provider,
            device_service,
            tick_interval: DEFAULT_TICK_INTERVAL,
            last_pushed: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_tick_interval(mut self, d: Duration) -> Self {
        self.tick_interval = d;
        self
    }

    /// Spawn the background tick task. Holds an `Arc<Self>`.
    ///
    /// The returned `JoinHandle` should be held by `AppState` to keep the task alive.
    /// Dropping the handle does NOT cancel the task — it keeps running as long as the
    /// Tokio runtime is alive.
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.tick_interval);
            interval.tick().await; // skip the immediate first tick
            loop {
                interval.tick().await;
                self.emit_once().await;
            }
        })
    }

    /// One tick: refresh every subscribed device's `__last_seen_age_secs`.
    ///
    /// Push semantics:
    /// - `age < offline_timeout` → push 0 (device online, clear stale offline state)
    /// - `age >= offline_timeout` → push actual age (device offline)
    /// - Skip the push if the value is unchanged since the last tick (avoids
    ///   waking the rule engine every 60s for healthy devices).
    pub(crate) async fn emit_once(&self) {
        let device_ids = self
            .rule_engine
            .subscribed_virtual_metric_devices(VIRTUAL_METRIC_NAME);
        if device_ids.is_empty() {
            return;
        }
        let now = Utc::now().timestamp();
        for device_id in device_ids {
            // DeviceService::get_device_last_seen returns i64; 0 for unknown/never-connected.
            let last_seen = self.device_service.get_device_last_seen(&device_id).await;
            if last_seen <= 0 {
                continue;
            }
            let age = (now - last_seen).max(0) as f64;
            let offline_timeout = self.device_service.effective_offline_timeout(&device_id) as f64;
            // Online: metric pinned to 0. Offline: metric tracks actual age.
            let metric_value = if age >= offline_timeout { age } else { 0.0 };

            // Skip redundant pushes (value unchanged since last tick).
            let push = {
                let mut last = self.last_pushed.lock().unwrap_or_else(|e| e.into_inner());
                if last.get(&device_id) == Some(&metric_value) {
                    false
                } else {
                    last.insert(device_id.clone(), metric_value);
                    true
                }
            };
            if !push {
                continue;
            }

            self.provider
                .update_device_value(&device_id, VIRTUAL_METRIC_NAME, metric_value)
                .await;
            self.rule_engine
                .on_data_update(
                    &DataSourceId::device(&device_id, VIRTUAL_METRIC_NAME),
                    RuleValue::Number(metric_value),
                )
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: full integration tests (mock DeviceService, verify on_data_update called,
    // verify never-connected skip) live in `tests/offline_rule_integration_test.rs`
    // because they need a real DeviceService instance. See Task 7.

    #[test]
    fn test_virtual_metric_name_constant() {
        assert_eq!(VIRTUAL_METRIC_NAME, "__last_seen_age_secs");
    }
}
