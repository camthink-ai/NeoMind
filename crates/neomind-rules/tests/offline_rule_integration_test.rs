//! End-to-end integration test for the Device-Offline Rule Alert feature (Task 7).
//!
//! Verifies two paths:
//!
//! 1. **Rule fires + cooldown**: When the `__last_seen_age_secs` virtual metric
//!    exceeds the threshold, the bound rule fires exactly once; subsequent
//!    data updates within the cooldown window are suppressed.
//!
//! 2. **Never-connected skip**: `DeviceStatusEmitter::emit_once` fetches
//!    `DeviceService::get_device_last_seen` for every device referenced by a
//!    rule subscription. Devices that were never registered (or have
//!    `last_seen <= 0`) are skipped — no metric update reaches the engine,
//!    so the rule never fires even after multiple ticks.
//!
//! # Deviation from the original Task 7 spec
//!
//! The spec suggested driving staleness via `DeviceService::update_last_seen`.
//! However `update_last_seen` writes to `DeviceRegistry` while the emitter
//! reads from the `device_status` map (`get_device_last_seen` →
//! `get_device_status`). The status map is only mutated by real metric events
//! flowing through the EventBus (see `service.rs` line ~512) or by
//! `register_device` (which sets `last_seen = now`). There is no public API
//! to back-date the status map entry, so for Test 1 we seed the
//! `UnifiedValueProvider` directly and invoke `RuleEngine::on_data_update`
//! to simulate the emitter's data-flow step. Test 2 exercises the emitter
//! loop end-to-end against a device that was never registered, which is the
//! more important real-world case (rule created for a device that hasn't
//! reported yet).

use std::sync::Arc;
use std::time::Duration;

use neomind_core::datasource::DataSourceId;
use neomind_core::EventBus;
use neomind_devices::{
    ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceService, DeviceTypeTemplate,
};
use neomind_rules::{
    ComparisonOperator, CompiledRule, DeviceStatusEmitter, NotifySeverity, RuleAction,
    RuleCondition, RuleEngine, RuleTrigger, RuleValue, UnifiedValueProvider, ValueProvider,
};

/// Name of the virtual metric exported by `DeviceStatusEmitter`.
const VIRTUAL_METRIC: &str = "__last_seen_age_secs";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fully-populated `DeviceConfig` (the struct does NOT derive `Default`).
///
/// `offline_timeout_secs` is set to 60s so that test fixtures that seed an
/// age >= 60 correctly model a device the emitter considers OFFLINE (the
/// emitter pushes 0 while the device is still within its offline_timeout).
fn make_device_config(device_id: &str) -> DeviceConfig {
    DeviceConfig {
        device_id: device_id.to_string(),
        name: format!("Test device {}", device_id),
        device_type: "generic".to_string(),
        // Non-mqtt: MQTT register requires a live adapter (BindIncomplete).
        adapter_type: "test".to_string(),
        connection_config: ConnectionConfig::default(),
        adapter_id: None,
        last_seen: 0,
        offline_timeout_secs: Some(60),
    }
}

/// Common setup: registry + service + "generic" template registered.
async fn setup_service() -> Arc<DeviceService> {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = Arc::new(DeviceService::new(registry, event_bus));
    service
        .register_template(DeviceTypeTemplate::new("generic", "Generic Device"))
        .await
        .expect("register_template must succeed");
    service
}

/// Build a comparison rule `device:<id>:__last_seen_age_secs <op> <threshold>`.
fn make_offline_rule(
    name: &str,
    device_id: &str,
    operator: ComparisonOperator,
    threshold: f64,
    cooldown: Duration,
) -> CompiledRule {
    let mut rule = CompiledRule::new(name);
    rule.condition = Some(RuleCondition::Comparison {
        source: DataSourceId::device(device_id, VIRTUAL_METRIC),
        operator,
        threshold,
        threshold_value: None,
    });
    rule.trigger = RuleTrigger::from_condition(&rule.condition);
    rule.actions = vec![RuleAction::Notify {
        message: "device went stale".to_string(),
        severity: NotifySeverity::Warning,
    }];
    rule.cooldown = cooldown;
    rule.finalize();
    rule
}

// ---------------------------------------------------------------------------
// Test 1: stale virtual metric fires the rule once; cooldown blocks repeats
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_offline_rule_fires_once_and_respects_cooldown() {
    let device_id = "test-device-1";

    // 1. Register the device so the service knows about it. In production
    //    the `device_status` map's `last_seen` is advanced by metric events
    //    (not by `register_device` alone, which sets it to `now`). Here we
    //    only need the rule infrastructure to be wired.
    let service = setup_service().await;
    service
        .register_device(make_device_config(device_id))
        .await
        .expect("register_device must succeed after template registration");

    // 2. Build the provider + rule engine.
    let provider = Arc::new(UnifiedValueProvider::new());
    let engine = Arc::new(RuleEngine::new(provider.clone() as Arc<dyn ValueProvider>));

    // 3. Add a rule: fire when age > 60s; 1h cooldown to block repeats.
    let rule = make_offline_rule(
        "offline-after-60s",
        device_id,
        ComparisonOperator::GreaterThan,
        60.0,
        Duration::from_secs(3600),
    );
    let rule_id = rule.id.clone();
    engine.add_rule(rule).await.expect("add_rule must succeed");

    // Sanity: subscription index should now include the device.
    let subscribed = engine.subscribed_virtual_metric_devices(VIRTUAL_METRIC);
    assert!(
        subscribed.contains(&device_id.to_string()),
        "device should be subscribed after rule.finalize(), got {:?}",
        subscribed
    );

    // 4. Seed the provider with a stale age (90s). The test device has
    //    `offline_timeout_secs = 60`, so age=90 >= 60 means the emitter
    //    considers it OFFLINE and would push the actual age value.
    provider
        .update_device_value(device_id, VIRTUAL_METRIC, 90.0)
        .await;

    // 5. Simulate the emitter's data-push step. The rule should fire once.
    engine
        .on_data_update(
            &DataSourceId::device(device_id, VIRTUAL_METRIC),
            RuleValue::Number(90.0),
        )
        .await;

    let trigger_count_after_first = engine
        .get_rule(&rule_id)
        .await
        .expect("rule must exist")
        .state
        .trigger_count;
    assert_eq!(
        trigger_count_after_first, 1,
        "rule should have fired exactly once after stale data update"
    );

    // 6. Push another stale update — the 1h cooldown must suppress it.
    provider
        .update_device_value(device_id, VIRTUAL_METRIC, 95.0)
        .await;
    engine
        .on_data_update(
            &DataSourceId::device(device_id, VIRTUAL_METRIC),
            RuleValue::Number(95.0),
        )
        .await;

    let trigger_count_after_second = engine
        .get_rule(&rule_id)
        .await
        .expect("rule must exist")
        .state
        .trigger_count;
    assert_eq!(
        trigger_count_after_second, 1,
        "cooldown should block additional firings (got {})",
        trigger_count_after_second
    );
}

// ---------------------------------------------------------------------------
// Test 2: never-connected device never fires (emitter skips last_seen <= 0)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_offline_rule_never_connected_device_never_fires() {
    // A device ID that is NOT registered with DeviceService. The emitter
    // will still pick it up from the rule's subscription index, but
    // `DeviceService::get_device_last_seen` returns 0 (default DeviceStatus),
    // and the emitter's `if last_seen <= 0 { continue; }` fast-path skips it.
    let device_id = "ghost-device-never-registered";

    // 1. Build service (template registered, but NOT the device itself).
    let service = setup_service().await;

    // 2. Provider + engine.
    let provider = Arc::new(UnifiedValueProvider::new());
    let engine = Arc::new(RuleEngine::new(provider.clone() as Arc<dyn ValueProvider>));

    // 3. Rule with a low threshold so it would fire if the metric were ever
    //    populated. Because the emitter skips never-seen devices, no update
    //    ever flows into the engine.
    let rule = make_offline_rule(
        "offline-after-5s",
        device_id,
        ComparisonOperator::GreaterThan,
        5.0,
        Duration::from_secs(3600),
    );
    let rule_id = rule.id.clone();
    engine.add_rule(rule).await.expect("add_rule must succeed");

    // Confirm the device shows up in the subscription index even though it
    // isn't registered — this is the exact path the emitter exercises.
    let subscribed = engine.subscribed_virtual_metric_devices(VIRTUAL_METRIC);
    assert!(
        subscribed.contains(&device_id.to_string()),
        "ghost device should be in subscription index (rule references it), got {:?}",
        subscribed
    );

    // And confirm the service reports last_seen = 0 for the ghost device,
    // which is what makes the emitter skip it.
    let last_seen = service.get_device_last_seen(device_id).await;
    assert_eq!(last_seen, 0, "unregistered device must report last_seen=0");

    // 4. Start the emitter with a fast tick.
    let emitter = Arc::new(
        DeviceStatusEmitter::new(engine.clone(), provider.clone(), service.clone())
            .with_tick_interval(Duration::from_millis(50)),
    );
    let _join = emitter.start();

    // 5. After several ticks the rule must not have fired.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let trigger_count = engine
        .get_rule(&rule_id)
        .await
        .expect("rule must exist")
        .state
        .trigger_count;
    assert_eq!(
        trigger_count, 0,
        "never-connected device must never fire the rule (emitter skip path)"
    );
}
