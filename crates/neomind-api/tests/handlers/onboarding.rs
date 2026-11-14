//! Draft→approve→register chain tests (edge-AI plan iteration 4).
//!
//! The discovery stack (drafts, rate limits, suggestions) already existed;
//! these tests pin the acceptance-critical chain deterministically: an
//! unknown MQTT device produces a DRAFT (suggestion, never a silent
//! registration), approving with a user-chosen type registers it, the
//! draft is consumed, and unknown drafts fail honestly with 404.

use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use neomind_api::automation::discovery::auto_onboard::AutoOnboardManager;
use neomind_api::handlers::devices::auto_onboard::{
    approve_draft_device, ApproveDraftDeviceRequest,
};
use neomind_api::handlers::ServerState;
use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, FinishReason, LlmError, LlmInput, LlmOutput, LlmRuntime,
    TokenUsage,
};
use neomind_devices::MetricValue;
use serde_json::json;

struct DummyLlm;
#[async_trait::async_trait]
impl LlmRuntime for DummyLlm {
    fn backend_id(&self) -> BackendId {
        BackendId::new("ollama")
    }
    fn model_name(&self) -> &str {
        "dummy"
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }
    fn max_context_length(&self) -> usize {
        4096
    }
    async fn generate(&self, _input: LlmInput) -> Result<LlmOutput, LlmError> {
        // Suggestions would come from here; approve-with-existing-type must
        // never need the LLM.
        Ok(LlmOutput {
            text: String::new(),
            thinking: None,
            finish_reason: FinishReason::Stop,
            usage: Some(TokenUsage::new(0, 0)),
            tool_calls: None,
        })
    }
    async fn generate_stream(
        &self,
        _input: LlmInput,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<(String, bool), LlmError>> + Send>>,
        LlmError,
    > {
        Ok(Box::pin(futures::stream::empty()))
    }
}

async fn state_with_manager() -> ServerState {
    let state = crate::common::create_test_server_state().await;
    let bus = state
        .core
        .event_bus
        .as_ref()
        .expect("event bus in test state")
        .clone();
    let manager = Arc::new(AutoOnboardManager::new(
        Arc::new(DummyLlm) as Arc<dyn LlmRuntime>,
        bus,
    ));
    *state.auto_onboard_manager.write().await = Some(manager);
    state
}

#[tokio::test]
async fn unknown_device_creates_draft_then_approve_registers_with_user_type() {
    let state = state_with_manager().await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    // A type the operator can choose at approval (user confirmation — the
    // system never guesses a type silently).
    let template: neomind_devices::registry::DeviceTypeTemplate = serde_json::from_value(json!({
        "device_type": "greenhouse_sensor",
        "name": "Greenhouse sensor"
    }))
    .unwrap();
    state
        .devices
        .registry
        .register_template(template)
        .await
        .unwrap();

    // 1) Unknown device data → draft, NOT a registration.
    let accepted = manager
        .process_unknown_device(
            "gh-001",
            "mqtt",
            &json!({"temperature": 21.5, "humidity": 60}),
            false,
        )
        .await
        .unwrap();
    assert!(accepted, "first sample must be accepted into the draft");
    let draft = manager.get_draft("gh-001").await.expect("draft exists");
    assert!(!draft.samples.is_empty());
    assert!(
        state
            .devices
            .service
            .get_device_with_template("gh-001")
            .await
            .is_err(),
        "no device may be registered before approval"
    );

    // 2) Approve with the user-chosen existing type.
    let result = approve_draft_device(
        State(state.clone()),
        Path("gh-001".to_string()),
        Json(Some(ApproveDraftDeviceRequest {
            existing_type: Some("greenhouse_sensor".to_string()),
            new_type: None,
            device_name: Some("温室传感器 1 号".to_string()),
        })),
    )
    .await
    .expect("approve succeeds");
    let resp = result.0.data.expect("approve response data");
    assert!(resp.registered, "registered flag set");
    assert_eq!(resp.device_type, "greenhouse_sensor");

    // 3) The device now exists with the chosen type and name.
    let (config, _tpl) = state
        .devices
        .service
        .get_device_with_template("gh-001")
        .await
        .expect("device registered after approval");
    assert_eq!(config.name, "温室传感器 1 号");
    assert_eq!(config.device_type, "greenhouse_sensor");

    // 4) The draft is consumed — re-approval fails honestly.
    assert!(
        manager.get_draft("gh-001").await.is_none(),
        "draft consumed"
    );
    let dup = approve_draft_device(
        State(state),
        Path("gh-001".to_string()),
        Json(Some(ApproveDraftDeviceRequest {
            existing_type: Some("greenhouse_sensor".to_string()),
            new_type: None,
            device_name: None,
        })),
    )
    .await;
    assert!(dup.is_err(), "approving a consumed draft must fail");
}

#[tokio::test]
async fn approve_unknown_draft_is_404_not_silent_success() {
    let state = state_with_manager().await;
    let result = approve_draft_device(
        State(state),
        Path("ghost-device".to_string()),
        Json(Some(ApproveDraftDeviceRequest {
            existing_type: Some("whatever".to_string()),
            new_type: None,
            device_name: None,
        })),
    )
    .await;
    assert!(result.is_err(), "unknown draft must fail explicitly");
}

#[tokio::test]
async fn registered_device_data_is_queryable_after_approval() {
    // The chain's last leg: after registration, NEW data for the device is
    // readable through telemetry (the "received real data" state).
    let state = state_with_manager().await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let template: neomind_devices::registry::DeviceTypeTemplate = serde_json::from_value(json!({
        "device_type": "simple_temp",
        "name": "Simple temp"
    }))
    .unwrap();
    state
        .devices
        .registry
        .register_template(template)
        .await
        .unwrap();
    manager
        .process_unknown_device("t-90", "mqtt", &json!({"temp": 1}), false)
        .await
        .unwrap();
    let _ = approve_draft_device(
        State(state.clone()),
        Path("t-90".to_string()),
        Json(Some(ApproveDraftDeviceRequest {
            existing_type: Some("simple_temp".to_string()),
            new_type: None,
            device_name: None,
        })),
    )
    .await
    .unwrap();

    let telemetry = state.devices.telemetry.clone();
    telemetry
        .write(
            "device:t-90",
            "temp",
            neomind_devices::telemetry::DataPoint {
                timestamp: chrono::Utc::now().timestamp(),
                value: MetricValue::Float(23.0),
                quality: None,
                metadata: None,
            },
        )
        .await
        .unwrap();
    telemetry.flush().unwrap();
    let latest = telemetry
        .latest("device:t-90", "temp")
        .await
        .unwrap()
        .expect("new data readable");
    match latest.value {
        MetricValue::Float(v) => assert_eq!(v, 23.0),
        other => panic!("unexpected value type: {other:?}"),
    }
}
