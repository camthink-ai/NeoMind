//! Pre-registered end-to-end business scenarios (edge-AI plan §11), fully
//! deterministic — no LLM, no network, local notification capture only.
//!
//! Scenario A — simulated detection data → Transform → Rule → notification:
//!   camera-shaped device payload with a `detections` array flows through
//!   the PRODUCTION transform entry (`process_device_data`) and the
//!   PRODUCTION virtual-metric publisher (`VirtualMetricPublisher`, which
//!   feeds the rule value provider and fires rules), ending in a locally
//!   captured message. Evidence is asserted at every node; a repeat push is
//!   absorbed by cooldown (no duplicate fires or notifications).
//!
//! Scenario B — sensor → Rule → simulated device command:
//!   a threshold rule executes a command on a simulated actuator through a
//!   stub adapter (broker-style publish). The delivery report stays honest
//!   (adapter-accepted, NOT device-confirmed), and an adapter-less actuator
//!   path records an explicit failed execution instead of silent success.

use std::pin::Pin;
use std::sync::Arc;

use neomind_api::automation::metric_publish::VirtualMetricPublisher;
use neomind_api::automation::store::SharedAutomationStore;
use neomind_api::automation::transform::TransformEngine;
use neomind_api::automation::types::{TransformAutomation, TransformScope};
use neomind_api::handlers::ServerState;
use neomind_core::datasource::DataSourceId;
use neomind_core::MetricValue as CoreMetricValue;
use neomind_devices::adapter::{DeviceAdapter, DeviceEvent};
use neomind_devices::registry::{ConnectionConfig, DeviceConfig, DeviceTypeTemplate};
use neomind_devices::AdapterResult;
use neomind_rules::models::*;
use neomind_rules::RuleValue;
use serde_json::json;

// ---------------------------------------------------------------- helpers

struct StubPublishAdapter;
#[async_trait::async_trait]
impl DeviceAdapter for StubPublishAdapter {
    fn name(&self) -> &str {
        "scenario-stub"
    }
    fn adapter_type(&self) -> &'static str {
        "stub-mqtt"
    }
    fn is_running(&self) -> bool {
        true
    }
    async fn start(&self) -> AdapterResult<()> {
        Ok(())
    }
    async fn stop(&self) -> AdapterResult<()> {
        Ok(())
    }
    fn subscribe(&self) -> Pin<Box<dyn futures::Stream<Item = DeviceEvent> + Send + '_>> {
        Box::pin(futures::stream::empty())
    }
    fn device_count(&self) -> usize {
        0
    }
    fn list_devices(&self) -> Vec<String> {
        Vec::new()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn send_command(
        &self,
        _device_id: &str,
        _command_name: &str,
        _payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        Ok(())
    }
    fn connection_status(&self) -> neomind_devices::adapter::ConnectionStatus {
        neomind_devices::adapter::ConnectionStatus::Connected
    }
    async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }
    async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }
}

async fn register_template(state: &ServerState, device_type: &str, commands: serde_json::Value) {
    let template: DeviceTypeTemplate = serde_json::from_value(json!({
        "device_type": device_type,
        "name": device_type,
        "commands": commands,
    }))
    .unwrap();
    state
        .devices
        .registry
        .register_template(template)
        .await
        .unwrap();
}

async fn register_device(state: &ServerState, id: &str, device_type: &str, adapter: &str) {
    state
        .devices
        .service
        .register_device(DeviceConfig {
            device_id: id.to_string(),
            name: id.to_string(),
            device_type: device_type.to_string(),
            adapter_type: adapter.to_string(),
            connection_config: ConnectionConfig::new(),
            adapter_id: None,
            last_seen: chrono::Utc::now().timestamp(),
            offline_timeout_secs: None,
        })
        .await
        .unwrap();
}

/// Build a transform engine wired like production (store attached), run the
/// saved transform over sample data, then push each produced metric through
/// the production virtual-metric publisher.
async fn run_transform_and_publish(
    state: &ServerState,
    store: &Arc<SharedAutomationStore>,
    transform: &TransformAutomation,
    device_id: &str,
    device_type: &str,
    data: &serde_json::Value,
) -> Vec<(String, CoreMetricValue)> {
    let engine = TransformEngine::with_extension_registry(state.extensions.registry.clone())
        .with_automation_store(store.clone());
    let result = engine
        .process_device_data(
            std::slice::from_ref(transform),
            device_id,
            Some(device_type),
            data,
        )
        .await
        .expect("transform processes");

    let bus = state.core.event_bus.as_ref().expect("bus").clone();
    let publisher = VirtualMetricPublisher {
        event_bus: bus,
        time_series: state.devices.telemetry.clone(),
        value_provider: state.automation.value_provider.clone(),
        rule_engine: state.automation.rule_engine.clone(),
    };
    let mut published = Vec::new();
    for m in &result.metrics {
        let tid = m.transform_id.as_deref().expect("transform_id set");
        publisher
            .publish_virtual_metric(
                &m.storage_device_id(),
                Some(&format!("device:{device_id}")),
                &m.metric,
                &m.value,
                m.timestamp,
                m.quality,
                Some(("transform", tid)),
            )
            .await;
        published.push((m.metric.clone(), m.value.clone()));
    }
    published
}

/// Poll until `f` returns Some, up to `secs` — rule actions (device
/// commands, channel notifications) run in spawned tasks, so their effects
/// land asynchronously.
async fn poll_until<T, F, Fut>(secs: u64, mut f: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..(secs * 50) {
        if let Some(v) = f().await {
            return Some(v);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    f().await
}

// -------------------------------------------------------- Scenario A

#[tokio::test]
async fn scenario_a_detections_to_transform_rule_notification() {
    let state = crate::common::create_test_server_state().await;
    let store = state
        .automation
        .automation_store
        .clone()
        .expect("automation store");

    register_template(&state, "cam_detect", json!([])).await;
    register_device(&state, "cam-1", "cam_detect", "mqtt").await;

    // Transform: count detections (camera → virtual metric).
    let mut transform = TransformAutomation::new(
        "t-scenario-a",
        "人员计数",
        TransformScope::DeviceType("cam_detect".to_string()),
    );
    transform.js_code = Some("return {person_count: (input_raw.detections || []).filter(function(d){ return d.cls === 'person' }).length}".into());
    store.save_automation(&transform).await.unwrap();

    // First pass: discover the produced metric name, then bind the rule to it.
    let sample = json!({"detections": [{"cls": "person"}, {"cls": "person"}, {"cls": "cat"}]});
    let published =
        run_transform_and_publish(&state, &store, &transform, "cam-1", "cam_detect", &sample).await;
    assert_eq!(published.len(), 1, "one virtual metric");
    let metric_name = published[0].0.clone();
    assert_eq!(metric_name, "transform.cam_detect.person_count");

    let mut rule = CompiledRule::new("车间人员过多");
    rule.condition = Some(RuleCondition::Comparison {
        source: DataSourceId::transform("t-scenario-a", &metric_name),
        operator: ComparisonOperator::GreaterThan,
        threshold: 1.0,
        threshold_value: None,
    });
    rule.trigger = RuleTrigger::from_condition(&rule.condition); // DataChange: data-driven firing
    rule.actions = vec![RuleAction::Notify {
        message: "检测到 {value} 人,超过阈值".into(),
        severity: NotifySeverity::Warning,
    }];
    rule.finalize();
    let rule_id = rule.id.clone();
    state.automation.rule_engine.add_rule(rule).await.unwrap();

    // Second pass with the rule in place — the chain fires.
    let messages_before = state.core.message_manager.list_messages().await.len();
    let published2 =
        run_transform_and_publish(&state, &store, &transform, "cam-1", "cam_detect", &sample).await;
    assert_eq!(published2.len(), 1);

    // Node evidence — every link of the chain left a trace.
    let execs = store.get_executions("t-scenario-a", 10).await.unwrap();
    assert!(
        execs.iter().any(|e| e
            .output
            .as_ref()
            .and_then(|o| o.get("metric_count"))
            .and_then(|v| v.as_u64())
            == Some(1)),
        "transform execution record carries metric_count evidence: {execs:?}"
    );
    let rule_state = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .unwrap();
    assert_eq!(rule_state.state.trigger_count, 1, "rule fired exactly once");
    let history = state
        .automation
        .rule_engine
        .get_rule_history(&rule_id)
        .await;
    assert_eq!(history.len(), 1);
    assert!(history[0].success);
    assert!(
        !history[0].actions_executed.is_empty(),
        "the notify action is recorded: {:?}",
        history[0].actions_executed
    );

    // Notification captured locally (and only here — no external delivery).
    // Notify delivery runs in a spawned task — poll for its arrival.
    let sent = poll_until(3, || async {
        let msgs = state.core.message_manager.list_messages().await;
        (msgs.len() == messages_before + 1)
            .then(|| msgs.last().cloned())
            .flatten()
    })
    .await
    .expect("notification arrives");
    assert!(
        sent.message.contains("2"),
        "value substituted: {}",
        sent.message
    );

    // Repeat push is absorbed by cooldown: no duplicate fire or message.
    run_transform_and_publish(&state, &store, &transform, "cam-1", "cam_detect", &sample).await;
    let rule_state = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .unwrap();
    assert_eq!(
        rule_state.state.trigger_count, 1,
        "cooldown suppressed the re-fire"
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        state.core.message_manager.list_messages().await.len(),
        messages_before + 1,
        "no duplicate notification"
    );
}

// -------------------------------------------------------- Scenario B

#[tokio::test]
async fn scenario_b_sensor_rule_simulated_command_honest_delivery() {
    let state = crate::common::create_test_server_state().await;

    register_template(&state, "env_sensor", json!([])).await;
    register_template(
        &state,
        "valve_actuator",
        json!([{"name": "open_valve", "parameters": []}]),
    )
    .await;
    register_device(&state, "sensor-b", "env_sensor", "mqtt").await;
    register_device(&state, "act-b", "valve_actuator", "stub-mqtt").await;
    // Adapter-less actuator for the honest-failure branch.
    // act-c points at an adapter id that does not exist: the explicit-id
    // resolution path fails fast (non-retryable "not found"), which is the
    // honest-failure branch under test.
    state
        .devices
        .service
        .register_device(DeviceConfig {
            device_id: "act-c".into(),
            name: "Act C".into(),
            device_type: "valve_actuator".into(),
            adapter_type: "stub-mqtt".into(),
            connection_config: ConnectionConfig::new(),
            adapter_id: Some("ghost-adapter".into()),
            last_seen: chrono::Utc::now().timestamp(),
            offline_timeout_secs: None,
        })
        .await
        .unwrap();
    state
        .devices
        .service
        .register_adapter("stub-scenario".into(), Arc::new(StubPublishAdapter))
        .await;

    let mut rule = CompiledRule::new("高温联动开阀 B");
    rule.condition = Some(RuleCondition::Comparison {
        source: DataSourceId::device("sensor-b", "temperature"),
        operator: ComparisonOperator::GreaterThan,
        threshold: 30.0,
        threshold_value: None,
    });
    rule.trigger = RuleTrigger::from_condition(&rule.condition);
    rule.actions = vec![
        RuleAction::Execute {
            target: "act-b".into(),
            target_type: ExecuteTarget::Device,
            command: "open_valve".into(),
            params: json!({}),
        },
        RuleAction::Notify {
            message: "温度 {value} 触发开阀".into(),
            severity: NotifySeverity::Warning,
        },
    ];
    rule.finalize();
    let rule_id = rule.id.clone();
    state.automation.rule_engine.add_rule(rule).await.unwrap();

    // Sensor reading through the production provider + trigger entry.
    state
        .automation
        .value_provider
        .update_value("device", "sensor-b", "temperature", 35.5)
        .await;
    state
        .automation
        .rule_engine
        .on_data_update(
            &DataSourceId::device("sensor-b", "temperature"),
            RuleValue::Number(35.5),
        )
        .await;

    // Rule fired, both actions ran, evidence recorded.
    let rule_state = state
        .automation
        .rule_engine
        .get_rule(&rule_id)
        .await
        .unwrap();
    assert_eq!(rule_state.state.trigger_count, 1);
    let history = state
        .automation
        .rule_engine
        .get_rule_history(&rule_id)
        .await;
    assert_eq!(history.len(), 1);
    assert!(
        history[0].success,
        "both actions succeeded: {:?}",
        history[0].error
    );

    // The command executed on the simulated actuator — delivery honesty:
    // broker publish, explicitly NOT device-confirmed.
    let _cmd_history = poll_until(5, || async {
        state
            .devices
            .service
            .get_command_history("act-b", None)
            .await
            .into_iter()
            .find(|r| {
                r.status == neomind_devices::service::CommandStatus::Success
                    && r.result
                        .as_deref()
                        .unwrap_or_default()
                        .contains("adapter(s)")
            })
    })
    .await
    .expect("command history records the broker-level delivery");

    // Notification captured locally with the substituted value (spawned).
    let notified = poll_until(3, || async {
        state
            .core
            .message_manager
            .list_messages()
            .await
            .iter()
            .any(|m| m.message.contains("35.5"))
            .then_some(())
    })
    .await;
    assert!(
        notified.is_some(),
        "notification with substituted value arrives"
    );

    // Honest failure branch: adapter-less actuator → failed execution, not
    // silent success. Separate rule to avoid cooldown interference.
    let mut rule_c = CompiledRule::new("高温联动开阀 C");
    rule_c.condition = Some(RuleCondition::Comparison {
        source: DataSourceId::device("sensor-b", "temperature"),
        operator: ComparisonOperator::GreaterThan,
        threshold: 30.0,
        threshold_value: None,
    });
    rule_c.trigger = RuleTrigger::from_condition(&rule_c.condition);
    rule_c.actions = vec![RuleAction::Execute {
        target: "act-c".into(),
        target_type: ExecuteTarget::Device,
        command: "open_valve".into(),
        params: json!({}),
    }];
    rule_c.finalize();
    let rule_c_id = rule_c.id.clone();
    state.automation.rule_engine.add_rule(rule_c).await.unwrap();

    state
        .automation
        .value_provider
        .update_value("device", "sensor-b", "temperature", 36.0)
        .await;
    state
        .automation
        .rule_engine
        .on_data_update(
            &DataSourceId::device("sensor-b", "temperature"),
            RuleValue::Number(36.0),
        )
        .await;

    // The Execute runs spawned: the row is written as success first, then
    // corrected to failed once the command actually errors. Poll for the
    // corrected (honest) outcome.
    let corrected = poll_until(20, || async {
        state
            .automation
            .rule_engine
            .get_rule_history(&rule_c_id)
            .await
            .iter()
            .find(|e| !e.success && e.error.as_deref().unwrap_or_default().contains("adapter"))
            .cloned()
    })
    .await
    .expect("adapter-less actuator records a FAILED, corrected execution");
    assert!(!corrected.success);
}
