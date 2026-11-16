//! NM-188: Conflict / Unverifiable must yield a customer-visible pending
//! association-exception draft through the auto-onboard event listener — not a
//! silent drop when the claimed identity is already registered.
//!
//! Handling-path (no metrics / no formal map / pending discovery id) is covered
//! in `neomind-devices` `handling_path::*`. These tests pin the **listener →
//! visible draft** half that the prior suite never checked.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use neomind_api::automation::discovery::auto_onboard::AutoOnboardManager;
use neomind_api::handlers::ServerState;
use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, FinishReason, LlmError, LlmInput, LlmOutput, LlmRuntime,
    TokenUsage,
};
use neomind_core::NeoMindEvent;
use neomind_devices::adapters::mqtt::{
    pending_association_candidate_id, MQTT_ASSOCIATION_EXCEPTION_KEY, MQTT_ASSOCIATION_KIND_KEY,
    MQTT_CLAIMED_IDENTITY_KEY, MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY,
    MQTT_PENDING_CANDIDATE_ID_KEY,
};
use neomind_devices::registry::{ConnectionConfig, DeviceConfig, DeviceTypeTemplate};
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

async fn state_with_listener() -> ServerState {
    let state = crate::common::create_test_server_state().await;
    let bus = state.core.event_bus.as_ref().expect("event bus").clone();
    let manager = Arc::new(AutoOnboardManager::new(
        Arc::new(DummyLlm) as Arc<dyn LlmRuntime>,
        bus,
    ));
    *state.auto_onboard_manager.write().await = Some(manager);
    state.init_auto_onboarding_events().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    state
}

async fn approve_registered_a(state: &ServerState) {
    state
        .devices
        .registry
        .register_template(DeviceTypeTemplate::new("gw", "gw"))
        .await
        .unwrap();
    let mut extra = HashMap::new();
    extra.insert(MQTT_IDENTITY_SOURCE_KEY.to_string(), json!("auto_payload"));
    extra.insert(MQTT_IDENTITY_FIELD_KEY.to_string(), json!("sn"));
    state
        .devices
        .registry
        .register_device(DeviceConfig {
            device_id: "A".to_string(),
            name: "A".to_string(),
            device_type: "gw".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some("gateway/data".to_string()),
                extra,
                ..Default::default()
            },
            adapter_id: Some("test-mqtt".to_string()),
            last_seen: 0,
            offline_timeout_secs: None,
        })
        .await
        .unwrap();
}

async fn wait_for_draft(
    manager: &AutoOnboardManager,
    device_id: &str,
) -> neomind_api::automation::DraftDevice {
    for _ in 0..50 {
        if let Some(d) = manager.get_draft(device_id).await {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("expected visible pending draft for {device_id}");
}

/// Conflict-shaped DeviceDiscovered (as handling emits after the fix) → visible
/// pending draft via the live event listener. No formal registration of A/B.
#[tokio::test]
async fn listener_conflict_exception_creates_visible_pending_draft() {
    let state = state_with_listener().await;
    approve_registered_a(&state).await;

    let topic = "device/gw/A/uplink";
    let claimed = "A";
    let pending = pending_association_candidate_id(topic, claimed);
    let sample = json!({
        "device_id": pending,
        "claimed_identity": claimed,
        "topic": topic,
        "data": {"sn": "B", "device_id": "A", "v": 1},
        "format": "json",
        "is_binary": false,
    });
    let metadata = json!({
        "original_topic": topic,
        MQTT_ASSOCIATION_EXCEPTION_KEY: true,
        MQTT_ASSOCIATION_KIND_KEY: "conflict",
        MQTT_CLAIMED_IDENTITY_KEY: claimed,
        MQTT_PENDING_CANDIDATE_ID_KEY: pending,
        "mqtt_blocked_device_ids": ["A"],
        "mqtt_identity_source": "topic_strong",
    });

    let bus = state.core.event_bus.as_ref().unwrap().clone();
    bus.publish(NeoMindEvent::DeviceDiscovered {
        device_id: pending.clone(),
        source: "mqtt".to_string(),
        adapter_id: Some("test".to_string()),
        metadata,
        sample,
        is_binary: false,
        timestamp: chrono::Utc::now().timestamp(),
    })
    .await;

    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let draft = wait_for_draft(&manager, &pending).await;
    assert!(
        draft.association_exception,
        "pending record must be marked association_exception"
    );
    assert_eq!(
        draft.association_exception_kind.as_deref(),
        Some("conflict")
    );
    assert_eq!(draft.claimed_identity.as_deref(), Some("A"));
    assert!(draft.blocked_device_ids.iter().any(|id| id == "A"));
    assert_eq!(draft.original_topic.as_deref(), Some(topic));
    assert!(
        !draft.samples.is_empty(),
        "sample retained for human confirm"
    );

    // No formal map / overwrite / auto-register via sn=B.
    assert!(state.devices.service.get_device("A").is_some());
    assert!(state.devices.service.get_device("B").is_none());
    assert!(state.devices.service.get_device(&pending).is_none());
    assert!(
        manager.get_draft("A").await.is_none(),
        "must not create a draft keyed as registered A"
    );
}

/// Unverifiable-shaped exception → visible pending (same listener path).
#[tokio::test]
async fn listener_unverifiable_exception_creates_visible_pending_draft() {
    let state = state_with_listener().await;
    approve_registered_a(&state).await;

    let topic = "device/gw/A/uplink";
    let pending = pending_association_candidate_id(topic, "A");
    let bus = state.core.event_bus.as_ref().unwrap().clone();
    bus.publish(NeoMindEvent::DeviceDiscovered {
        device_id: pending.clone(),
        source: "mqtt".to_string(),
        adapter_id: Some("test".to_string()),
        metadata: json!({
            "original_topic": topic,
            MQTT_ASSOCIATION_EXCEPTION_KEY: true,
            MQTT_ASSOCIATION_KIND_KEY: "unverifiable",
            MQTT_CLAIMED_IDENTITY_KEY: "A",
            MQTT_PENDING_CANDIDATE_ID_KEY: pending,
            "mqtt_blocked_device_ids": ["A"],
            "mqtt_identity_source": "topic_strong",
        }),
        sample: json!({
            "data": {"device_id": "A", "v": 1},
        }),
        is_binary: false,
        timestamp: chrono::Utc::now().timestamp(),
    })
    .await;

    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let draft = wait_for_draft(&manager, &pending).await;
    assert!(draft.association_exception);
    assert_eq!(
        draft.association_exception_kind.as_deref(),
        Some("unverifiable")
    );
    assert!(state.devices.service.get_device("B").is_none());
}

/// Defense: registered id + exception flag remaps to pending (no silent drop).
#[tokio::test]
async fn listener_registered_id_with_exception_flag_still_pending() {
    let state = state_with_listener().await;
    approve_registered_a(&state).await;

    let topic = "device/gw/A/uplink";
    let pending = pending_association_candidate_id(topic, "A");
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let accepted = ServerState::process_device_discovered_for_onboarding(
        manager.as_ref(),
        state.devices.service.as_ref(),
        "A".to_string(),
        "mqtt".to_string(),
        Some("test".to_string()),
        &json!({
            "original_topic": topic,
            MQTT_ASSOCIATION_EXCEPTION_KEY: true,
            MQTT_ASSOCIATION_KIND_KEY: "conflict",
            MQTT_CLAIMED_IDENTITY_KEY: "A",
            MQTT_PENDING_CANDIDATE_ID_KEY: pending,
            "mqtt_blocked_device_ids": ["A"],
        }),
        &json!({"data": {"sn": "B", "v": 1}}),
        false,
    )
    .await;
    assert!(
        accepted,
        "exception against registered A must still onboard pending"
    );
    let draft = manager.get_draft(&pending).await.expect("visible pending");
    assert!(draft.association_exception);
    assert_eq!(draft.claimed_identity.as_deref(), Some("A"));
    assert!(manager.get_draft("A").await.is_none());
}

/// Regression: plain DeviceDiscovered for a registered id (no exception) still skipped.
#[tokio::test]
async fn listener_registered_id_without_exception_still_skipped() {
    let state = state_with_listener().await;
    approve_registered_a(&state).await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let accepted = ServerState::process_device_discovered_for_onboarding(
        manager.as_ref(),
        state.devices.service.as_ref(),
        "A".to_string(),
        "mqtt".to_string(),
        None,
        &json!({"original_topic": "device/gw/A/uplink"}),
        &json!({"data": {"v": 1}}),
        false,
    )
    .await;
    assert!(!accepted);
    assert!(manager.get_drafts().await.is_empty());
}
