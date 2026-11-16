//! NM-188: Conflict / Unverifiable must yield a customer-visible pending
//! association-exception draft through the auto-onboard event listener — not a
//! silent drop when the claimed identity is already registered.
//!
//! NM-189: Ordinary `/approve` must refuse association-exception drafts (no
//! `pending-assoc-*` registration). Explicit `/confirm-association` with a
//! real device id + parse field registers/attaches that identity; later
//! routing for that shape is covered in `neomind-devices` handling_path.
//!
//! Handling-path (no metrics / no formal map / pending discovery id) is covered
//! in `neomind-devices` `handling_path::*`. These tests pin the **listener →
//! visible draft** half that the prior suite never checked.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use futures::Stream;
use neomind_api::automation::discovery::auto_onboard::AutoOnboardManager;
use neomind_api::handlers::devices::auto_onboard::{
    approve_draft_device, confirm_association_draft, ApproveDraftDeviceRequest,
    ConfirmAssociationRequest,
};
use neomind_api::handlers::ServerState;
use neomind_core::llm::backend::{
    BackendCapabilities, BackendId, FinishReason, LlmError, LlmInput, LlmOutput, LlmRuntime,
    TokenUsage,
};
use neomind_core::NeoMindEvent;
use neomind_devices::adapter::{AdapterResult, DeviceAdapter, DeviceEvent};
use neomind_devices::adapters::mqtt::{
    pending_association_candidate_id, MqttAdapter, MqttAdapterConfig,
    MQTT_ASSOCIATION_EXCEPTION_KEY, MQTT_ASSOCIATION_KIND_KEY, MQTT_CLAIMED_IDENTITY_KEY,
    MQTT_IDENTITY_FIELD_KEY, MQTT_IDENTITY_SOURCE_KEY, MQTT_PENDING_CANDIDATE_ID_KEY,
};
use neomind_devices::registry::{ConnectionConfig, DeviceConfig, DeviceTypeTemplate};
use neomind_devices::ConnectionStatus;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// Register a real MQTT adapter under the draft's source id so confirm's
/// subscribe switch can succeed. Installs a synthetic broker slot (clients
/// non-empty) without a live TCP session — empty clients now fail closed.
async fn register_confirm_adapter(state: &ServerState, adapter_id: &str) -> Arc<MqttAdapter> {
    let adapter = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
        adapter_id,
        "localhost:1883",
    )));
    adapter
        .set_shared_device_registry(state.devices.registry.clone())
        .await;
    adapter.install_test_broker_slot(adapter_id).await;
    state
        .devices
        .service
        .register_adapter(
            adapter_id.to_string(),
            adapter.clone() as Arc<dyn DeviceAdapter>,
        )
        .await;
    adapter
}

/// Real MqttAdapter registered but **not** connected (empty mqtt_clients).
async fn register_disconnected_mqtt_adapter(
    state: &ServerState,
    adapter_id: &str,
) -> Arc<MqttAdapter> {
    let adapter = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
        adapter_id,
        "localhost:1883",
    )));
    adapter
        .set_shared_device_registry(state.devices.registry.clone())
        .await;
    assert!(
        adapter.list_brokers().await.is_empty(),
        "test requires disconnected adapter"
    );
    state
        .devices
        .service
        .register_adapter(
            adapter_id.to_string(),
            adapter.clone() as Arc<dyn DeviceAdapter>,
        )
        .await;
    adapter
}

/// Adapter that fails subscribe_device — used for failure-recovery acceptance.
struct FailSubscribeAdapter {
    name: String,
    fail: AtomicBool,
    subscribe_calls: AtomicUsize,
}

impl FailSubscribeAdapter {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail: AtomicBool::new(true),
            subscribe_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl DeviceAdapter for FailSubscribeAdapter {
    fn name(&self) -> &str {
        &self.name
    }
    fn adapter_type(&self) -> &'static str {
        "mqtt"
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
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
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
    fn connection_status(&self) -> ConnectionStatus {
        ConnectionStatus::Disconnected
    }
    async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            Err(neomind_devices::adapter::AdapterError::Communication(
                "forced subscribe failure".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }
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

async fn seed_conflict_pending(state: &ServerState) -> String {
    approve_registered_a(state).await;
    let topic = "device/gw/A/uplink";
    let claimed = "A";
    let pending = pending_association_candidate_id(topic, claimed);
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let accepted = ServerState::process_device_discovered_for_onboarding(
        manager.as_ref(),
        state.devices.service.as_ref(),
        pending.clone(),
        "mqtt".to_string(),
        Some("test".to_string()),
        &json!({
            "original_topic": topic,
            MQTT_ASSOCIATION_EXCEPTION_KEY: true,
            MQTT_ASSOCIATION_KIND_KEY: "conflict",
            MQTT_CLAIMED_IDENTITY_KEY: claimed,
            MQTT_PENDING_CANDIDATE_ID_KEY: pending,
            "mqtt_blocked_device_ids": ["A"],
            "mqtt_identity_source": "topic_strong",
        }),
        &json!({"sn": "B", "device_id": "A", "v": 1}),
        false,
    )
    .await;
    assert!(accepted);
    wait_for_draft(manager.as_ref(), &pending).await;
    pending
}

/// NM-189: ordinary approve of association-exception draft must fail; placeholder
/// must never become a registered device.
#[tokio::test]
async fn ordinary_approve_of_association_exception_refused() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;

    let result = approve_draft_device(
        State(state.clone()),
        Path(pending.clone()),
        Json(Some(ApproveDraftDeviceRequest {
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("Should Fail".to_string()),
        })),
    )
    .await;
    assert!(
        result.is_err(),
        "ordinary approve must refuse association-exception drafts"
    );

    assert!(
        state.devices.service.get_device(&pending).is_none(),
        "placeholder pending-assoc-* must not be registered"
    );
    assert!(
        state.devices.service.get_device("A").is_some(),
        "blocked device A must remain"
    );
    assert!(
        state.devices.service.get_device("B").is_none(),
        "failed approve must not invent B either"
    );
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    assert!(
        manager.get_draft(&pending).await.is_some(),
        "draft stays for confirm/dismiss after refused approve"
    );
}

/// NM-189: confirm with pending-assoc as device_id is refused.
#[tokio::test]
async fn confirm_rejects_pending_assoc_as_real_identity() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: pending.clone(),
            identity_field: "sn".to_string(),
            identity_source: None,
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(result.is_err());
    assert!(state.devices.service.get_device(&pending).is_none());
}

/// NM-189: confirm without overwrite must not retarget blocked A.
#[tokio::test]
async fn confirm_refuses_default_overwrite_of_blocked_a() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "A".to_string(),
            identity_field: "sn".to_string(),
            identity_source: None,
            existing_type: None,
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(result.is_err(), "must not default-overwrite blocked A");

    let a = state.devices.service.get_device("A").expect("A remains");
    assert_eq!(
        a.connection_config
            .extra
            .get(MQTT_IDENTITY_FIELD_KEY)
            .and_then(|v| v.as_str()),
        Some("sn")
    );
    assert_eq!(
        a.connection_config.telemetry_topic.as_deref(),
        Some("gateway/data"),
        "A's topic must stay unchanged"
    );
}

/// NM-189: explicit confirm of B + sn registers B; placeholder gone; A untouched.
#[tokio::test]
async fn confirm_real_identity_registers_b_not_placeholder() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let _adapter = register_confirm_adapter(&state, "test").await;
    let topic = "device/gw/A/uplink";

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("Device B".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("confirm B+sn must succeed");
    let resp = result.0.data.expect("confirm response");
    assert!(resp.registered);
    assert_eq!(resp.system_device_id, "B");
    assert_eq!(resp.original_device_id, pending);
    assert_eq!(resp.recommended_topic, topic);

    assert!(
        state.devices.service.get_device(&pending).is_none(),
        "placeholder must never be registered"
    );
    let b = state
        .devices
        .service
        .get_device("B")
        .expect("B registered after confirm");
    assert_eq!(b.name, "Device B");
    assert_eq!(b.connection_config.telemetry_topic.as_deref(), Some(topic));
    assert_eq!(
        b.connection_config
            .extra
            .get(MQTT_IDENTITY_FIELD_KEY)
            .and_then(|v| v.as_str()),
        Some("sn")
    );
    assert_eq!(
        b.connection_config
            .extra
            .get(MQTT_IDENTITY_SOURCE_KEY)
            .and_then(|v| v.as_str()),
        Some("auto_payload")
    );

    let a = state.devices.service.get_device("A").expect("A untouched");
    assert_eq!(
        a.connection_config.telemetry_topic.as_deref(),
        Some("gateway/data")
    );

    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    assert!(
        manager.get_draft(&pending).await.is_none(),
        "pending draft consumed after confirm"
    );
    assert!(
        resp.subscription_ready && !resp.online,
        "confirm success means subscription ready, never marks online"
    );
    let status = state.devices.service.get_device_status("B").await;
    assert_eq!(
        status.status,
        ConnectionStatus::Disconnected,
        "config ≠ online"
    );
}

/// NM-190: attach to existing B must update topic + identity (not register's
/// name-only duplicate branch). Draft consumed only after a real config write.
#[tokio::test]
async fn confirm_attach_existing_b_updates_topic_and_identity() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let _adapter = register_confirm_adapter(&state, "test").await;
    let topic = "device/gw/A/uplink";

    // Pre-existing B on a stale topic / identity (the attach target).
    let mut stale_extra = HashMap::new();
    stale_extra.insert(MQTT_IDENTITY_SOURCE_KEY.to_string(), json!("topic_strong"));
    stale_extra.insert("plant_tag".to_string(), json!("keep-me"));
    state
        .devices
        .registry
        .register_device(DeviceConfig {
            device_id: "B".to_string(),
            name: "Old B".to_string(),
            device_type: "gw".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some("device/gw/B/uplink".to_string()),
                command_topic: Some("device/gw/B/downlink".to_string()),
                extra: stale_extra,
                ..Default::default()
            },
            adapter_id: Some("test-mqtt".to_string()),
            last_seen: 0,
            offline_timeout_secs: None,
        })
        .await
        .unwrap();

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: None,
            new_type: None,
            device_name: Some("Device B attached".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("confirm attach to existing B must succeed");
    let resp = result.0.data.expect("confirm response");
    assert!(resp.registered);
    assert_eq!(resp.system_device_id, "B");

    let b = state
        .devices
        .service
        .get_device("B")
        .expect("B still registered");
    assert_eq!(b.name, "Device B attached");
    assert_eq!(b.connection_config.telemetry_topic.as_deref(), Some(topic));
    assert_eq!(
        b.connection_config
            .extra
            .get(MQTT_IDENTITY_FIELD_KEY)
            .and_then(|v| v.as_str()),
        Some("sn")
    );
    assert_eq!(
        b.connection_config
            .extra
            .get(MQTT_IDENTITY_SOURCE_KEY)
            .and_then(|v| v.as_str()),
        Some("auto_payload")
    );

    assert_eq!(
        b.connection_config.command_topic.as_deref(),
        Some("device/gw/B/downlink"),
        "unrelated command topic must be preserved"
    );
    assert_eq!(
        b.connection_config.extra.get("plant_tag"),
        Some(&json!("keep-me")),
        "non-identity extra must be preserved"
    );
    assert_eq!(
        b.adapter_id.as_deref(),
        Some("test"),
        "cross-source confirm must use draft adapter_id, not stale test-mqtt"
    );

    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    assert!(
        manager.get_draft(&pending).await.is_none(),
        "pending draft consumed after successful attach"
    );
}

/// NM-190: bad identity config must not consume the draft or claim success.
#[tokio::test]
async fn confirm_bad_identity_config_does_not_consume_draft() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    // Invalid identity_source enum.
    let bad_source = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("not_a_source".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(bad_source.is_err(), "unknown identity_source must fail");
    assert!(
        manager.get_draft(&pending).await.is_some(),
        "draft must remain after bad identity_source"
    );
    assert!(state.devices.service.get_device("B").is_none());

    // Empty identity_field for payload source.
    let empty_field = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "   ".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(empty_field.is_err(), "empty identity_field must fail");
    assert!(manager.get_draft(&pending).await.is_some());

    // Sample has sn=B; confirming C with field sn must fail closed.
    let mismatch = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "C".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(
        mismatch.is_err(),
        "sample sn=B must not confirm device_id=C"
    );
    assert!(
        manager.get_draft(&pending).await.is_some(),
        "draft must remain after sample mismatch"
    );
    assert!(
        state.devices.service.get_device("C").is_none(),
        "failed confirm must not register C"
    );
    assert!(
        state.devices.service.get_device("B").is_none(),
        "failed confirm must not invent B either"
    );
}

/// NM-191: missing adapter → not fake success; draft kept; config may apply; retry works.
#[tokio::test]
async fn confirm_missing_adapter_keeps_draft_and_retries() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    let first = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("B retry".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(first.is_err(), "missing adapter must not claim success");
    assert!(
        manager.get_draft(&pending).await.is_some(),
        "draft retained for retry"
    );
    // Config may already be applied — that is recoverable.
    let applied = state.devices.service.get_device("B").is_some();
    assert!(
        applied,
        "bind persists config so retry updates in place (no double-register)"
    );
    assert_eq!(
        state.devices.service.get_device_status("B").await.status,
        ConnectionStatus::Disconnected,
        "must not mark online on incomplete bind"
    );

    let _adapter = register_confirm_adapter(&state, "test").await;
    let second = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("B retry".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("retry after adapter register must succeed");
    let resp = second.0.data.expect("response");
    assert!(resp.registered && resp.subscription_ready && !resp.online);
    assert!(manager.get_draft(&pending).await.is_none());
    // Still a single B — no double-register / dual mapping inventing another id.
    assert!(state.devices.service.get_device("B").is_some());
    assert!(state.devices.service.get_device(&pending).is_none());
}

/// NM-191: subscribe failure → not success; draft kept; retry after fix recovers.
#[tokio::test]
async fn confirm_subscribe_failure_keeps_draft_then_recovers() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let failing = Arc::new(FailSubscribeAdapter::new("test"));
    state
        .devices
        .service
        .register_adapter(
            "test".to_string(),
            failing.clone() as Arc<dyn DeviceAdapter>,
        )
        .await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    let first = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(first.is_err(), "subscribe failure must not claim success");
    assert!(manager.get_draft(&pending).await.is_some());
    assert!(state.devices.service.get_device("B").is_some());
    assert!(failing.subscribe_calls.load(Ordering::SeqCst) >= 1);

    failing.fail.store(false, Ordering::SeqCst);
    let second = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: None,
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("retry after subscribe fix must succeed");
    assert!(second.0.data.expect("resp").subscription_ready);
    assert!(manager.get_draft(&pending).await.is_none());
}

/// NM-191: nested data.sn raw sample — discover/confirm same path.
#[tokio::test]
async fn confirm_nested_data_sn_raw_sample() {
    let state = state_with_listener().await;
    approve_registered_a(&state).await;
    let _adapter = register_confirm_adapter(&state, "test").await;
    let topic = "device/gw/A/uplink";
    let pending = pending_association_candidate_id(topic, "A");
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();
    let accepted = ServerState::process_device_discovered_for_onboarding(
        manager.as_ref(),
        state.devices.service.as_ref(),
        pending.clone(),
        "mqtt".to_string(),
        Some("test".to_string()),
        &json!({
            "original_topic": topic,
            MQTT_ASSOCIATION_EXCEPTION_KEY: true,
            MQTT_ASSOCIATION_KIND_KEY: "conflict",
            MQTT_CLAIMED_IDENTITY_KEY: "A",
            MQTT_PENDING_CANDIDATE_ID_KEY: pending,
            "mqtt_blocked_device_ids": ["A"],
            "mqtt_identity_source": "auto_payload",
            "mqtt_identity_field": "data.sn",
        }),
        // Envelope form (as handling emits): outer `data` = full wire payload.
        &json!({
            "device_id": pending,
            "topic": topic,
            "data": {"data": {"sn": "NEST-B", "v": 1}, "device_id": "A"},
            "format": "json",
            "is_binary": false,
        }),
        false,
    )
    .await;
    assert!(accepted);
    let draft = wait_for_draft(manager.as_ref(), &pending).await;
    // Stored sample must retain nested data.sn (not peeled to top-level sn).
    let stored = draft.samples.first().and_then(|s| s.parsed.as_ref());
    assert_eq!(
        stored
            .and_then(|v| v.pointer("/data/sn"))
            .and_then(|v| v.as_str()),
        Some("NEST-B"),
        "draft sample must keep full wire JSON with nested data.sn"
    );

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "NEST-B".to_string(),
            identity_field: "data.sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("Nested B".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("data.sn confirm must succeed against raw sample");
    assert_eq!(result.0.data.expect("r").system_device_id, "NEST-B");
    let b = state
        .devices
        .service
        .get_device("NEST-B")
        .expect("registered");
    assert_eq!(
        b.connection_config
            .extra
            .get(MQTT_IDENTITY_FIELD_KEY)
            .and_then(|v| v.as_str()),
        Some("data.sn")
    );
}

/// NM-191: consecutive confirm — second call does not re-exec / overwrite wrongly.
#[tokio::test]
async fn confirm_consecutive_does_not_double_consume() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let _adapter = register_confirm_adapter(&state, "test").await;

    let first = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("First".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("first confirm");
    assert!(first.0.data.expect("r").registered);

    let second = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("Second overwrite?".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(
        second.is_err(),
        "consumed draft must 404 — no silent re-exec"
    );
    let b = state.devices.service.get_device("B").expect("B");
    assert_eq!(b.name, "First", "second confirm must not overwrite name");
}

/// NM-191: cross-broker confirm uses draft adapter_id and preserves unrelated fields.
/// Runtime topic-map clear is covered in neomind-devices `handling_path::cross_broker_*`.
#[tokio::test]
async fn confirm_cross_broker_uses_draft_adapter_and_preserves_fields() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let _old = register_confirm_adapter(&state, "old-broker").await;
    let _new = register_confirm_adapter(&state, "test").await;

    let mut extra = HashMap::new();
    extra.insert(MQTT_IDENTITY_SOURCE_KEY.to_string(), json!("topic_strong"));
    extra.insert("site".to_string(), json!("line-3"));
    state
        .devices
        .registry
        .register_device(DeviceConfig {
            device_id: "B".to_string(),
            name: "Old B".to_string(),
            device_type: "gw".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some("device/gw/B/uplink".to_string()),
                command_topic: Some("cmd/B".to_string()),
                extra,
                ..Default::default()
            },
            adapter_id: Some("old-broker".to_string()),
            last_seen: 0,
            offline_timeout_secs: None,
        })
        .await
        .unwrap();

    let result = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: None,
            new_type: None,
            device_name: Some("B moved".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("cross-broker confirm");
    assert!(result.0.data.expect("r").subscription_ready);

    let b = state.devices.service.get_device("B").expect("B");
    assert_eq!(
        b.adapter_id.as_deref(),
        Some("test"),
        "must bind draft source adapter, not leave old-broker"
    );
    assert_eq!(b.connection_config.command_topic.as_deref(), Some("cmd/B"));
    assert_eq!(
        b.connection_config.extra.get("site"),
        Some(&json!("line-3"))
    );
}

/// NM-195: real MqttAdapter registered but not connected → not fake success;
/// draft kept; retry after installing a broker slot recovers.
#[tokio::test]
async fn confirm_disconnected_mqtt_adapter_keeps_draft_then_recovers() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let adapter = register_disconnected_mqtt_adapter(&state, "test").await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    let first = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("B".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await;
    assert!(
        first.is_err(),
        "empty mqtt_clients must not claim subscription_ready / delete draft"
    );
    assert!(manager.get_draft(&pending).await.is_some());
    assert!(state.devices.service.get_device("B").is_some());
    assert_eq!(
        state.devices.service.get_device_status("B").await.status,
        ConnectionStatus::Disconnected
    );

    // Connect the same real adapter (synthetic slot) and retry.
    adapter.install_test_broker_slot("test").await;
    let second = confirm_association_draft(
        State(state.clone()),
        Path(pending.clone()),
        Json(ConfirmAssociationRequest {
            device_id: "B".to_string(),
            identity_field: "sn".to_string(),
            identity_source: Some("auto_payload".to_string()),
            existing_type: Some("gw".to_string()),
            new_type: None,
            device_name: Some("B".to_string()),
            overwrite_blocked: None,
        }),
    )
    .await
    .expect("retry after broker slot must succeed");
    assert!(second.0.data.expect("resp").subscription_ready);
    assert!(manager.get_draft(&pending).await.is_none());
}

/// NM-195: concurrent same-draft confirms — exactly one success; draft consumed
/// once; failed/losing call does not overwrite; retry after fail still works.
#[tokio::test]
async fn confirm_concurrent_same_draft_serializes() {
    let state = state_with_listener().await;
    let pending = seed_conflict_pending(&state).await;
    let _adapter = register_confirm_adapter(&state, "test").await;
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    let req = ConfirmAssociationRequest {
        device_id: "B".to_string(),
        identity_field: "sn".to_string(),
        identity_source: Some("auto_payload".to_string()),
        existing_type: Some("gw".to_string()),
        new_type: None,
        device_name: Some("Concurrent B".to_string()),
        overwrite_blocked: None,
    };

    let s1 = state.clone();
    let s2 = state.clone();
    let p1 = pending.clone();
    let p2 = pending.clone();
    let r1 = req.clone();
    let r2 = req.clone();

    let (a, b) = tokio::join!(
        confirm_association_draft(State(s1), Path(p1), Json(r1)),
        confirm_association_draft(State(s2), Path(p2), Json(r2)),
    );

    let successes = [&a, &b].iter().filter(|r| r.is_ok()).count();
    let failures = [&a, &b].iter().filter(|r| r.is_err()).count();
    assert_eq!(successes, 1, "exactly one concurrent confirm must succeed");
    assert_eq!(failures, 1, "loser must see consumed draft / not re-exec");
    assert!(manager.get_draft(&pending).await.is_none());
    let device = state.devices.service.get_device("B").expect("B once");
    assert_eq!(device.name, "Concurrent B");

    // After success the draft is gone — a later retry is a clean 404, not a hang.
    let again =
        confirm_association_draft(State(state.clone()), Path(pending.clone()), Json(req)).await;
    assert!(again.is_err());
}

/// NM-195: concurrent confirms targeting the same device id from two drafts
/// are serialized via the per-device bind lock (no interleaved corruption).
#[tokio::test]
async fn confirm_concurrent_same_device_from_two_drafts_coordinates() {
    let state = state_with_listener().await;
    let _adapter = register_confirm_adapter(&state, "test").await;
    approve_registered_a(&state).await;

    // Two association drafts that both want to attach real id B.
    let topic_a = "device/gw/A/uplink";
    let topic_c = "device/gw/C/uplink";
    let pending_a = pending_association_candidate_id(topic_a, "A");
    let pending_c = pending_association_candidate_id(topic_c, "C");
    let manager = state.auto_onboard_manager.read().await.clone().unwrap();

    for (pending, topic, claimed) in [
        (pending_a.clone(), topic_a, "A"),
        (pending_c.clone(), topic_c, "C"),
    ] {
        let accepted = ServerState::process_device_discovered_for_onboarding(
            manager.as_ref(),
            state.devices.service.as_ref(),
            pending.clone(),
            "mqtt".to_string(),
            Some("test".to_string()),
            &json!({
                "original_topic": topic,
                MQTT_ASSOCIATION_EXCEPTION_KEY: true,
                MQTT_ASSOCIATION_KIND_KEY: "conflict",
                MQTT_CLAIMED_IDENTITY_KEY: claimed,
                MQTT_PENDING_CANDIDATE_ID_KEY: pending,
                "mqtt_blocked_device_ids": [claimed],
                "mqtt_identity_source": "auto_payload",
                "mqtt_identity_field": "sn",
            }),
            &json!({
                "device_id": pending,
                "topic": topic,
                "data": {"sn": "B", "v": 1},
                "format": "json",
                "is_binary": false,
            }),
            false,
        )
        .await;
        assert!(accepted);
        let _ = wait_for_draft(manager.as_ref(), &pending).await;
    }

    let req_a = ConfirmAssociationRequest {
        device_id: "B".to_string(),
        identity_field: "sn".to_string(),
        identity_source: Some("auto_payload".to_string()),
        existing_type: Some("gw".to_string()),
        new_type: None,
        device_name: Some("From A draft".to_string()),
        overwrite_blocked: None,
    };
    let req_c = ConfirmAssociationRequest {
        device_id: "B".to_string(),
        identity_field: "sn".to_string(),
        identity_source: Some("auto_payload".to_string()),
        existing_type: Some("gw".to_string()),
        new_type: None,
        device_name: Some("From C draft".to_string()),
        overwrite_blocked: None,
    };

    let (ra, rc) = tokio::join!(
        confirm_association_draft(State(state.clone()), Path(pending_a.clone()), Json(req_a)),
        confirm_association_draft(State(state.clone()), Path(pending_c.clone()), Json(req_c)),
    );

    // Both may succeed (serialized updates in place) or one may fail if sample
    // validation races — but we must never leave two B devices / panic / hang,
    // and a subsequent retry on any remaining draft must be possible.
    assert!(
        ra.is_ok() || rc.is_ok(),
        "at least one same-device confirm must complete"
    );
    assert!(state.devices.service.get_device("B").is_some());
    assert!(state.devices.service.get_device("B2").is_none());

    // Whichever draft remains (if any) must still be retryable after a fail.
    for (pending, name) in [(pending_a, "Retry A"), (pending_c, "Retry C")] {
        if manager.get_draft(&pending).await.is_some() {
            let retry = confirm_association_draft(
                State(state.clone()),
                Path(pending.clone()),
                Json(ConfirmAssociationRequest {
                    device_id: "B".to_string(),
                    identity_field: "sn".to_string(),
                    identity_source: Some("auto_payload".to_string()),
                    existing_type: Some("gw".to_string()),
                    new_type: None,
                    device_name: Some(name.to_string()),
                    overwrite_blocked: None,
                }),
            )
            .await;
            assert!(
                retry.is_ok(),
                "remaining draft {pending} must be retryable after concurrent same-device bind"
            );
        }
    }
}
