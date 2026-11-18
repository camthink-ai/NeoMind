//! Shared command-validation feedback on `POST /devices/{id}/command/{cmd}`.
//!
//! The dedicated chat `device_command` tool was removed from model
//! registration (2026-11-17); its corrective feedback loop — wrong command
//! lists available commands, wrong params return the offending command's
//! schema — moved into this handler so every entry point (HTTP, CLI, chat
//! via shell) gets the same loop. These tests pin that contract.

use axum::extract::{Path, State};
use futures::Stream;
use neomind_api::handlers::devices::models::SendCommandRequest;
use neomind_api::handlers::send_command_handler;
use neomind_api::handlers::ServerState;
use neomind_devices::adapter::{ConnectionStatus, DeviceAdapter, DeviceEvent};
use neomind_devices::AdapterResult;
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

async fn setup() -> ServerState {
    let state = crate::common::create_test_server_state().await;
    let template = json!({
        "device_type": "validator_actuator",
        "name": "Validator Actuator",
        "commands": [
            {
                "name": "render_text",
                "display_name": "Render Text",
                "parameters": [
                    {"name": "text", "data_type": "String", "required": true}
                ]
            },
            {
                "name": "set_brightness",
                "display_name": "Set Brightness",
                "parameters": [
                    {
                        "name": "level",
                        "data_type": "Integer",
                        "required": true,
                        "min": 0,
                        "max": 100,
                        "default": 50,
                        "unit": "%"
                    }
                ]
            },
            {
                "name": "set_mode",
                "display_name": "Set Mode",
                "parameters": [
                    {
                        "name": "mode",
                        "data_type": {"Enum": {"options": ["auto", "manual"]}},
                        "required": true,
                        "default": "auto"
                    }
                ]
            }
        ]
    });
    state
        .devices
        .service
        .register_template(serde_json::from_value(template).expect("template parses"))
        .await
        .expect("register template");
    state
        .devices
        .service
        .register_device(neomind_devices::DeviceConfig {
            device_id: "val-1".into(),
            name: "Validator Dev".into(),
            device_type: "validator_actuator".into(),
            adapter_type: "mqtt".into(),
            connection_config: neomind_devices::ConnectionConfig::new(),
            adapter_id: None,
            last_seen: chrono::Utc::now().timestamp(),
            offline_timeout_secs: None,
        })
        .await
        .unwrap_or_else(|e| {
            // BindIncomplete still applies the config (config_applied=true) —
            // the device is registered, only the MQTT subscribe is deferred.
            // Tolerate it so these tests stay independent of adapter
            // bootstrap state (and of the pre-existing registration-strictness
            // regression tracked separately).
            eprintln!("register_device returned non-fatal: {e:?}");
        });
    assert!(
        state.devices.service.get_device("val-1").is_some(),
        "device registered despite deferred bind"
    );
    state
}

async fn send(
    state: &ServerState,
    command: &str,
    params: Value,
) -> Result<Value, neomind_api::models::ErrorResponse> {
    let req = SendCommandRequest {
        params: match params {
            Value::Object(m) => m.into_iter().collect(),
            _ => Default::default(),
        },
    };
    let resp = send_command_handler(
        State(state.clone()),
        Path(("val-1".into(), command.into())),
        axum::Json(req),
    )
    .await?;
    Ok(resp.0.data.expect("success payload carries data"))
}

fn hint_json(err: &neomind_api::models::ErrorResponse) -> Value {
    serde_json::from_str(err.hint.as_deref().expect("hint present")).expect("hint is json")
}

/// Counts `send_command` invocations so preflight rejects can assert
/// adapter call count stays zero.
struct CountingMqttAdapter {
    sends: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl DeviceAdapter for CountingMqttAdapter {
    fn name(&self) -> &str {
        "counting-mqtt"
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
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn connection_status(&self) -> ConnectionStatus {
        ConnectionStatus::Connected
    }
    async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }
    async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn unknown_command_lists_available_commands() {
    let state = setup().await;
    let err = send(&state, "turbo_boost", json!({})).await.unwrap_err();
    assert!(
        err.message.contains("Available commands"),
        "{}",
        err.message
    );
    assert!(err.message.contains("render_text"));
    let hint = hint_json(&err);
    let names: Vec<&str> = hint["available_commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(names.contains(&"set_mode"));
}

#[tokio::test]
async fn missing_required_param_returns_command_schema() {
    let state = setup().await;
    let err = send(&state, "render_text", json!({})).await.unwrap_err();
    assert!(err.message.contains("NOT sent"), "{}", err.message);
    assert!(
        err.message.contains("missing required top-level params"),
        "{}",
        err.message
    );
    assert!(err.message.contains("text"));
    let hint = hint_json(&err);
    assert_eq!(hint["command"], "render_text");
    assert_eq!(hint["params_example"]["text"], "Hello");
    assert!(hint["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "text" && p["required"] == true));
}

#[tokio::test]
async fn protocol_envelope_params_are_rejected_without_adapter_send() {
    let state = setup().await;
    let sends = Arc::new(AtomicUsize::new(0));
    state
        .devices
        .service
        .register_adapter(
            "counting-mqtt".into(),
            Arc::new(CountingMqttAdapter {
                sends: Arc::clone(&sends),
            }),
        )
        .await;

    let err = send(
        &state,
        "render_text",
        json!({"cmd": "render_text", "params": {"text": "hi"}}),
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("NOT sent"), "{}", err.message);
    assert!(err.message.contains("protocol envelope"), "{}", err.message);
    assert!(err.hint.as_deref().unwrap_or("").contains("params_example"));
    assert_eq!(
        sends.load(Ordering::SeqCst),
        0,
        "preflight must reject before any adapter send"
    );

    // Sanity: a valid call reaches the counting adapter (proves the counter works).
    let _ = send(&state, "render_text", json!({"text": "hi"})).await;
    assert!(
        sends.load(Ordering::SeqCst) >= 1,
        "valid call must exercise the counting adapter"
    );
}

#[tokio::test]
async fn range_violation_returns_constraints() {
    let state = setup().await;
    let err = send(&state, "set_brightness", json!({"level": 150}))
        .await
        .unwrap_err();
    assert!(err.message.contains("above maximum"), "{}", err.message);
    let hint = hint_json(&err);
    assert_eq!(hint["parameters"][0]["min"].as_f64(), Some(0.0));
    assert_eq!(hint["parameters"][0]["max"].as_f64(), Some(100.0));
    assert_eq!(hint["parameters"][0]["unit"], "%");
}

#[tokio::test]
async fn enum_violation_returns_allowed_values() {
    let state = setup().await;
    let err = send(&state, "set_mode", json!({"mode": "turbo"}))
        .await
        .unwrap_err();
    assert!(err.message.contains("allowed_values"), "{}", err.message);
    let hint = hint_json(&err);
    assert!(hint["parameters"][0]["allowed_values"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "manual"));
}

#[tokio::test]
async fn unknown_param_names_surface_allowed_list() {
    let state = setup().await;
    let err = send(&state, "render_text", json!({"text": "hi", "extra": 1}))
        .await
        .unwrap_err();
    assert!(err.message.contains("NOT sent"), "{}", err.message);
    assert!(err.message.contains("unknown param"), "{}", err.message);
    assert!(err.message.contains("extra"));
}

#[tokio::test]
async fn valid_command_reaches_service_with_honest_delivery() {
    let state = setup().await;
    // No adapter is connected for val-1: the service layer answers honestly
    // rather than reporting a device ACK that never happened.
    let out = send(&state, "render_text", json!({"text": "hello"})).await;
    // Either an honest adapter-less failure (service error) or a structured
    // delivery payload — both are acceptable here; the preflight layer must
    // NOT be what rejects a schema-valid command.
    match out {
        Ok(v) => {
            assert_eq!(v["sent"], true);
            assert!(v.get("delivery").is_some() || v.get("delivery_note").is_some());
        }
        Err(e) => {
            // The failure came from the service layer, not the preflight:
            // the message must NOT be a schema-correction message.
            assert!(!e.message.contains("Available commands"));
            assert!(!e.message.contains("missing required params"));
        }
    }
}
