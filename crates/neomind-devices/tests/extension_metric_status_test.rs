//! Regression tests: extension-relayed metrics must refresh device status.
//!
//! Bug (fixed here): metrics written by extensions through the
//! `device_metrics_write` capability are published as `DeviceMetric` events
//! tagged `is_virtual: Some(true)` — the flag exists purely to prevent
//! feedback loops in extension event subscriptions. Device status tracking
//! treated *every* virtual metric as synthetic and skipped it, so
//! extension-managed devices (router-bridge Wi-Fi clients, phones, gateways)
//! showed "offline" on dashboards forever while their telemetry kept
//! flowing: the time-series data was fresh but the status map's `last_seen`
//! froze at registration, and nothing could ever flip the device back to
//! Connected.
//!
//! The fix scopes the synthetic exclusion to derived transform metrics
//! (`transform.*` metric namespace / `transform:` device namespace) — real
//! device data relayed by an extension now counts as liveness again.

use std::sync::Arc;
use std::time::Duration;

use neomind_core::{EventBus, MetricValue, NeoMindEvent};
use neomind_devices::{ConnectionStatus, DeviceRegistry, DeviceService};

/// Give the service's spawned event listener a moment to subscribe before
/// publishing, then a moment to process what we publish.
async fn listener_settle() {
    tokio::time::sleep(Duration::from_millis(150)).await;
}

fn service() -> (Arc<DeviceService>, EventBus) {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = Arc::new(DeviceService::new(registry, event_bus.clone()));
    (service, event_bus)
}

/// Extension-relayed metric (`is_virtual: true`, real metric name): this is
/// how router-bridge keeps Wi-Fi clients online — it MUST mark the device
/// connected and refresh last_seen, or every extension-managed device is
/// permanently "offline" on the dashboard.
#[tokio::test]
async fn extension_relayed_metric_marks_device_connected() {
    let (service, event_bus) = service();
    service.start().await;
    listener_settle().await;

    let before = chrono::Utc::now().timestamp();
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "router-rk-wifi-dev".to_string(),
            metric: "dtime".to_string(),
            value: MetricValue::Integer(1_728_000_000),
            timestamp: before,
            quality: Some(1.0),
            is_virtual: Some(true),
        })
        .await;
    listener_settle().await;

    let status = service.get_device_status("router-rk-wifi-dev").await;
    assert_eq!(
        status.status,
        ConnectionStatus::Connected,
        "extension-relayed metric must mark the device connected"
    );
    assert!(
        status.last_seen >= before,
        "last_seen must be refreshed by the metric (got {}, before {})",
        status.last_seen,
        before
    );
}

/// Derived transform metrics stay synthetic: a rule-engine transform result
/// must not make an otherwise-silent device look alive. Covers both
/// synthetic namespaces (`transform.*` metric, `transform:` device).
#[tokio::test]
async fn transform_metrics_do_not_mark_devices_connected() {
    let (service, event_bus) = service();
    service.start().await;
    listener_settle().await;

    let ts = chrono::Utc::now().timestamp();
    for (device_id, metric) in [
        ("sensor-1", "transform.rolling_avg"),
        ("transform:t-42", "rolling_avg"),
    ] {
        event_bus
            .publish(NeoMindEvent::DeviceMetric {
                device_id: device_id.to_string(),
                metric: metric.to_string(),
                value: MetricValue::Float(21.5),
                timestamp: ts,
                quality: Some(1.0),
                is_virtual: Some(true),
            })
            .await;
    }
    listener_settle().await;

    for device_id in ["sensor-1", "transform:t-42"] {
        let status = service.get_device_status(device_id).await;
        assert_eq!(
            status.status,
            ConnectionStatus::Disconnected,
            "transform-derived metric must not mark {} connected",
            device_id
        );
    }
}

/// Positive control: adapter metrics (not virtual) keep their pre-existing
/// behavior — they mark the device connected.
#[tokio::test]
async fn adapter_metric_still_marks_device_connected() {
    let (service, event_bus) = service();
    service.start().await;
    listener_settle().await;

    let before = chrono::Utc::now().timestamp();
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "mqtt-sensor-9".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(23.0),
            timestamp: before,
            quality: Some(1.0),
            is_virtual: None,
        })
        .await;
    listener_settle().await;

    let status = service.get_device_status("mqtt-sensor-9").await;
    assert_eq!(status.status, ConnectionStatus::Connected);
    assert!(status.last_seen >= before);
}
