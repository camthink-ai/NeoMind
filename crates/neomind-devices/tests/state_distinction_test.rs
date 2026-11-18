//! Device state-distinction tests (edge-AI plan iteration 4, 2026-11-13).
//!
//! The plan requires "registered" and "actually received fresh data" to be
//! DIFFERENT observable states, and per-device offline timeouts to resolve
//! in the documented precedence. These are the exact service-layer
//! contracts the iteration-0 audit found untested.

use std::sync::Arc;

use neomind_core::EventBus;
use neomind_devices::adapter::ConnectionStatus;
use neomind_devices::registry::{
    ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceTypeTemplate,
};
use neomind_devices::service::{DeviceService, DeviceStatus};
use neomind_devices::telemetry::{DataPoint, TimeSeriesStorage};
use neomind_devices::MetricValue;

fn config(id: &str, last_seen: i64, offline_timeout_secs: Option<u64>) -> DeviceConfig {
    DeviceConfig {
        device_id: id.to_string(),
        name: id.to_string(),
        device_type: "state_sensor".to_string(),
        // Non-mqtt: MQTT register requires a live adapter (BindIncomplete).
        adapter_type: "test".to_string(),
        connection_config: ConnectionConfig::new(),
        adapter_id: None,
        last_seen,
        offline_timeout_secs,
    }
}

async fn service_with_template() -> (Arc<DeviceService>, Arc<DeviceRegistry>) {
    let registry = Arc::new(DeviceRegistry::new());
    let service = Arc::new(DeviceService::new(registry.clone(), EventBus::new()));
    service
        .register_template(DeviceTypeTemplate::new("state_sensor", "State sensor"))
        .await
        .unwrap();
    (service, registry)
}

#[tokio::test]
async fn registered_without_data_is_not_online_with_data() {
    // DeviceStatus is the state cell behind the 4-state model.
    // Registered + never reported: no connection status, no fresh data.
    let mut fresh = DeviceStatus::new(ConnectionStatus::Connected);
    assert!(
        fresh.is_connected_within(300),
        "connected + fresh last_seen reads as online"
    );

    // Simulate "registered long ago, no data since": stale timestamp.
    fresh.last_seen = chrono::Utc::now().timestamp() - 3600;
    assert!(
        !fresh.is_connected_within(300),
        "connected status alone must NOT read as online when data is an hour old"
    );

    // Fresh timestamp but not transport-connected: registered-only state.
    let mut idle = DeviceStatus::new(ConnectionStatus::Disconnected);
    idle.last_seen = chrono::Utc::now().timestamp();
    assert!(
        !idle.is_connected_within(300),
        "recent data timestamp without connection is not 'online'"
    );
}

#[tokio::test]
async fn offline_timeout_resolves_device_over_template_over_global() {
    let (service, registry) = service_with_template().await;

    // Template default
    let mut tpl = DeviceTypeTemplate::new("tpl_default", "Template default");
    tpl.default_offline_timeout_secs = Some(120);
    service.register_template(tpl).await.unwrap();

    // Device A: no override → template default (120)
    service
        .register_device(config("dev-a", 0, None).tap_type("tpl_default"))
        .await
        .unwrap();
    // Device B: explicit override (45) wins over the template
    service
        .register_device(config("dev-b", 0, Some(45)).tap_type("tpl_default"))
        .await
        .unwrap();

    assert_eq!(
        service.effective_offline_timeout("dev-b"),
        45,
        "device override wins"
    );
    assert_eq!(
        service.effective_offline_timeout("dev-a"),
        120,
        "template default applies"
    );
    // Unknown device falls back to the global (>0)
    assert!(service.effective_offline_timeout("ghost") > 0);
    assert_eq!(registry.list_devices().len(), 2);
}

/// Small helper to set the device_type on a DeviceConfig literal.
trait TapType {
    fn tap_type(self, t: &str) -> DeviceConfig;
}
impl TapType for DeviceConfig {
    fn tap_type(mut self, t: &str) -> DeviceConfig {
        self.device_type = t.to_string();
        self
    }
}

#[tokio::test]
async fn migrate_last_seen_lifts_sentinel_from_real_telemetry() {
    let registry = Arc::new(DeviceRegistry::new());
    let service = DeviceService::new(registry.clone(), EventBus::new());
    service
        .register_template(DeviceTypeTemplate::new("state_sensor", "State sensor"))
        .await
        .unwrap();
    let telemetry = Arc::new(TimeSeriesStorage::memory().unwrap());
    service.set_telemetry_storage(telemetry.clone()).await;

    // A device registered with the sentinel last_seen == 1 that HAS telemetry.
    service
        .register_device(config("migrate-me", 1, None))
        .await
        .unwrap();
    let ts = chrono::Utc::now().timestamp() - 60;
    telemetry
        .write(
            "device:migrate-me",
            "temperature",
            DataPoint {
                timestamp: ts,
                value: MetricValue::Float(22.5),
                quality: None,
                metadata: None,
            },
        )
        .await
        .unwrap();
    // Writes are buffered; force them to the table list_metrics scans.
    telemetry.flush().unwrap();

    // A sentinel device with NO telemetry: migration must leave it alone.
    service
        .register_device(config("no-data", 1, None))
        .await
        .unwrap();

    // Sanity: the telemetry point is visible under the key migration reads.
    let latest = telemetry
        .latest("device:migrate-me", "temperature")
        .await
        .unwrap();
    assert!(
        latest.is_some(),
        "telemetry point must be visible via latest()"
    );
    let listed = telemetry.list_metrics("device:migrate-me").await.unwrap();
    assert!(
        !listed.is_empty(),
        "list_metrics must find the metric: {listed:?}"
    );

    service.start().await;
    // start() runs the migration synchronously before subscribing.

    let migrated = registry.get_device("migrate-me").unwrap();
    assert_eq!(
        migrated.last_seen, ts,
        "sentinel last_seen is replaced by the latest telemetry timestamp"
    );
    let untouched = registry.get_device("no-data").unwrap();
    assert_eq!(
        untouched.last_seen, 1,
        "no telemetry → sentinel stays (previously-seen)"
    );
}
