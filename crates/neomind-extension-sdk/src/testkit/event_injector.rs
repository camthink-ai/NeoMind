//! Event Injector — simulates platform EventBus events flowing to
//! extensions, with support for batch injection, rate-limited streams,
//! and virtual metric simulation.

use std::sync::Arc;
use std::time::Duration;

use crate::host::Extension;

/// A DeviceMetric event to inject.
#[derive(Debug, Clone)]
pub struct DeviceMetricEvent {
    pub device_id: String,
    pub metric: String,
    pub value: serde_json::Value,
    pub is_virtual: bool,
    pub timestamp_ms: i64,
}

impl DeviceMetricEvent {
    pub fn new(device_id: &str, metric: &str, value: serde_json::Value) -> Self {
        Self {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value,
            is_virtual: false,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn virtual_metric(device_id: &str, metric: &str, value: serde_json::Value) -> Self {
        Self {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value,
            is_virtual: true,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn to_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": self.device_id,
                "metric": self.metric,
                "value": self.value,
                "is_virtual": self.is_virtual,
            },
            "timestamp": self.timestamp_ms,
        })
    }
}

/// Event injector — drives events through the extension's handle_event.
pub struct EventInjector<E: Extension> {
    extension: Arc<E>,
}

impl<E: Extension + Send + Sync + 'static> EventInjector<E> {
    pub fn new(extension: Arc<E>) -> Self {
        Self { extension }
    }

    /// Inject a single event and measure processing time.
    pub async fn inject(&self, event: &DeviceMetricEvent) -> Result<Duration, String> {
        let start = std::time::Instant::now();
        let payload = event.to_payload();
        self.extension
            .handle_event("DeviceMetric", &payload)
            .map_err(|e| e.to_string())?;
        Ok(start.elapsed())
    }

    /// Inject a batch of events (sequential).
    /// Returns per-event processing times.
    pub async fn inject_batch(
        &self,
        events: &[DeviceMetricEvent],
    ) -> Vec<Result<Duration, String>> {
        let mut timings = Vec::with_capacity(events.len());
        for event in events {
            timings.push(self.inject(event).await);
        }
        timings
    }

    /// Inject N copies of the same event with a delay between them
    /// (simulates a camera pushing frames at a given FPS).
    pub async fn inject_stream(
        &self,
        event: &DeviceMetricEvent,
        count: usize,
        interval: Duration,
    ) -> Vec<Duration> {
        let mut timings = Vec::with_capacity(count);
        let mut timer = tokio::time::interval(interval);
        for _ in 0..count {
            timer.tick().await;
            if let Ok(d) = self.inject(event).await {
                timings.push(d);
            }
        }
        timings
    }

    /// Inject events concurrently (tests for lock contention/deadlocks).
    /// Each event is dispatched on a separate task.
    pub async fn inject_concurrent(
        &self,
        events: &[DeviceMetricEvent],
    ) -> Vec<Result<Duration, String>> {
        let ext = self.extension.clone();
        let mut handles = Vec::with_capacity(events.len());

        for event in events {
            let ext = ext.clone();
            let payload = event.to_payload();
            handles.push(tokio::spawn(async move {
                let start = std::time::Instant::now();
                ext.handle_event("DeviceMetric", &payload)
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(start.elapsed())
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(Ok(d)) => results.push(Ok(d)),
                Ok(Err(e)) => results.push(Err(e)),
                Err(e) => results.push(Err(format!("task panicked: {e}"))),
            }
        }
        results
    }
}
