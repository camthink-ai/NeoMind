//! Tests for automations handlers — transform test-endpoint isolation.

use axum::extract::{Path, State};
use axum::Json;
use neomind_api::automation::types::{TransformAutomation, TransformScope};
use neomind_api::handlers::automations::*;
use neomind_api::handlers::ServerState;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

fn sample_req() -> ProcessDataRequest {
    serde_json::from_value(json!({
        "device_id": "sensor-1",
        "data": {"temperature": 25.5}
    }))
    .expect("valid ProcessDataRequest")
}

// ---- create/update JS syntax-check regression (2026-11-17 audit) ----

#[cfg(test)]
mod syntax_gate_tests {
    use super::*;
    use neomind_api::automation::types::TransformAutomation;
    use std::collections::HashSet;

    fn create_req(name: &str, js: &str) -> Json<CreateAutomationRequest> {
        Json(CreateAutomationRequest {
            name: name.to_string(),
            description: String::new(),
            r#type: None,
            enabled: true,
            definition: json!({
                "scope": "global",
                "js_code": js,
                "output_prefix": "out",
            }),
        })
    }

    #[tokio::test]
    async fn create_rejects_broken_js_and_saves_nothing() {
        let state = create_test_server_state().await;
        let resp = create_automation_handler(
            State(state.clone()),
            create_req("broken", "function transform("),
        )
        .await;
        let err = resp.unwrap_err();
        assert_eq!(err.status.as_u16(), 400);
        assert!(err.message.contains("syntax error"), "{err:?}");
        // Nothing was saved.
        let all = state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .list_automations()
            .await
            .unwrap_or_default();
        assert!(
            all.iter().all(|a| a.metadata.name != "broken"),
            "broken transform must not be persisted"
        );
    }

    #[tokio::test]
    async fn create_accepts_valid_js() {
        let state = create_test_server_state().await;
        let resp = create_automation_handler(
            State(state.clone()),
            create_req("valid-tx", "return input * 2;"),
        )
        .await;
        assert!(resp.is_ok(), "{resp:?}");
    }

    /// Review scenario: a LEGACY record already holds broken JS (saved before
    /// the gate existed). A name-only update must still succeed — the gate
    /// applies only to code this request writes.
    #[tokio::test]
    async fn name_only_update_of_legacy_broken_record_succeeds() {
        let state = create_test_server_state().await;
        let mut t = TransformAutomation::new("legacy-bad", "legacy-bad", TransformScope::Global);
        t.js_code = Some("function transform(".to_string());
        state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .save_automation(&t)
            .await
            .unwrap();

        let resp = update_automation_handler(
            Path("legacy-bad".into()),
            State(state.clone()),
            Json(UpdateAutomationRequest {
                name: Some("renamed".into()),
                description: None,
                definition: None,
                enabled: None,
            }),
        )
        .await;
        assert!(resp.is_ok(), "name-only edit must pass: {resp:?}");
        let stored = state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .get_automation("legacy-bad")
            .await
            .unwrap()
            .expect("still there");
        assert_eq!(stored.metadata.name, "renamed");
        // Untouched broken code is preserved verbatim (execution-path
        // honesty handles it; iteration-1).
        assert_eq!(stored.js_code.as_deref(), Some("function transform("));
    }

    /// Updating the CODE to something broken is rejected; the stored record
    /// keeps its previous definition.
    #[tokio::test]
    async fn update_with_new_broken_js_is_rejected_and_kept() {
        let state = create_test_server_state().await;
        let mut t = TransformAutomation::new("tx-u", "tx-u", TransformScope::Global);
        t.js_code = Some("return input;".to_string());
        state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .save_automation(&t)
            .await
            .unwrap();

        let resp = update_automation_handler(
            Path("tx-u".into()),
            State(state.clone()),
            Json(UpdateAutomationRequest {
                name: None,
                description: None,
                definition: Some(json!({"js_code": "return {{{"})),
                enabled: None,
            }),
        )
        .await;
        let err = resp.unwrap_err();
        assert_eq!(err.status.as_u16(), 400);
        let stored = state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .get_automation("tx-u")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.js_code.as_deref(), Some("return input;"));
    }

    /// Compile-only gate must not reject runtime-error code on create.
    #[tokio::test]
    async fn create_allows_runtime_errors_but_not_syntax_errors() {
        let state = create_test_server_state().await;
        let resp = create_automation_handler(
            State(state.clone()),
            create_req("runtime-only", "nope(input); return 1;"),
        )
        .await;
        assert!(resp.is_ok(), "{resp:?}");
        let _ = HashSet::<String>::new(); // silence unused import variance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn save_js_transform(state: &ServerState, id: &str, js: &str) {
        let mut t = TransformAutomation::new(id, id, TransformScope::Global);
        t.js_code = Some(js.to_string());
        state
            .automation
            .automation_store
            .as_ref()
            .expect("automation store wired in test state")
            .save_automation(&t)
            .await
            .expect("save transform");
    }

    async fn execution_rows(state: &ServerState, id: &str) -> usize {
        state
            .automation
            .automation_store
            .as_ref()
            .unwrap()
            .get_executions(id, 10)
            .await
            .unwrap()
            .len()
    }

    #[tokio::test]
    async fn test_endpoint_writes_no_history_and_registers_nothing() {
        let state = create_test_server_state().await;
        save_js_transform(&state, "h-isolation", "return {count: 1}").await;

        let result = test_transform_handler(
            Path("h-isolation".into()),
            State(state.clone()),
            Json(sample_req()),
        )
        .await;
        assert!(result.is_ok());
        let value = result.unwrap().0.data.expect("response data");
        assert_eq!(value.get("count").and_then(|c| c.as_u64()), Some(1));
        assert_eq!(
            value
                .get("unverified")
                .and_then(|u| u.as_array())
                .map(|a| a.len()),
            Some(0),
            "pure-JS preview is fully verified"
        );

        assert_eq!(
            execution_rows(&state, "h-isolation").await,
            0,
            "the /test endpoint must not write execution history"
        );
        assert!(
            state
                .automation
                .transform_engine
                .as_ref()
                .unwrap()
                .output_registry()
                .get_transform_outputs("h-isolation")
                .await
                .is_empty(),
            "the /test endpoint must not register outputs"
        );
    }

    #[tokio::test]
    async fn test_endpoint_surfaces_js_failure_without_recording() {
        let state = create_test_server_state().await;
        save_js_transform(&state, "h-failing", "throw new Error('boom')").await;

        let result = test_transform_handler(
            Path("h-failing".into()),
            State(state.clone()),
            Json(sample_req()),
        )
        .await;
        // The endpoint answers 200 with the failure spelled out in warnings —
        // an honest preview, not a silent "Completed"-style success.
        assert!(result.is_ok());
        let value = result.unwrap().0.data.expect("response data");
        let warnings = value
            .get("warnings")
            .and_then(|w| w.as_array())
            .expect("warnings present");
        assert!(
            warnings
                .iter()
                .any(|w| w.as_str().unwrap_or_default().contains("failed")),
            "failure is visible in warnings: {warnings:?}"
        );
        assert_eq!(
            value.get("count").and_then(|c| c.as_u64()),
            Some(0),
            "no fake metrics on failure"
        );

        assert_eq!(
            execution_rows(&state, "h-failing").await,
            0,
            "a failing preview must not write history either"
        );
    }

    #[tokio::test]
    async fn test_endpoint_reports_extension_preview_as_unverified() {
        let state = create_test_server_state().await;
        save_js_transform(
            &state,
            "h-ext-preview",
            "const r = extensions.invoke('weather', 'get', {}); return {v: 1}",
        )
        .await;

        let result = test_transform_handler(
            Path("h-ext-preview".into()),
            State(state.clone()),
            Json(sample_req()),
        )
        .await
        .expect("preview answers");
        let value = result.0.data.unwrap();
        let unverified = value
            .get("unverified")
            .and_then(|u| u.as_array())
            .expect("unverified list present");
        assert!(
            unverified.iter().any(|u| u
                .as_str()
                .unwrap_or_default()
                .contains("blocked in dry-run")),
            "blocked extension preview is unverified, not silent: {unverified:?}"
        );
        assert!(
            execution_rows(&state, "h-ext-preview").await == 0,
            "still zero history writes"
        );
    }

    #[tokio::test]
    async fn test_endpoint_unknown_transform_is_404() {
        let state = create_test_server_state().await;
        let result = test_transform_handler(
            Path("no-such-transform".into()),
            State(state),
            Json(sample_req()),
        )
        .await;
        assert!(result.is_err());
    }

    /// P1-5: `/transforms/test-code` returns a clean DTO (no inner `success`)
    /// on success and HTTP 400 `ErrorResponse` with a hint on failure — never
    /// HTTP 200 with embedded `"success": false`.
    #[tokio::test]
    async fn test_code_returns_clean_dto_and_non_200_error_on_failure() {
        let state = create_test_server_state().await;

        let ok_resp = test_transform_code_handler(
            State(state.clone()),
            Json(TestTransformCodeRequest {
                code: "return { doubled: 42 };".to_string(),
                input_data: json!({ "value": 21 }),
                output_prefix: "out".to_string(),
            }),
        )
        .await
        .expect("valid JS must succeed");
        assert!(ok_resp.0.success, "outer ApiResponse carries success=true");
        let data = ok_resp.0.data.expect("response payload");
        assert!(
            data.get("success").is_none(),
            "inner DTO must not duplicate `success`: {data}"
        );
        assert_eq!(
            data["output"]["doubled"].as_f64(),
            Some(42.0),
            "unexpected output payload: {data}"
        );

        let err = test_transform_code_handler(
            State(state),
            Json(TestTransformCodeRequest {
                code: "throw new Error('bad transform');".to_string(),
                input_data: json!({}),
                output_prefix: String::new(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status.as_u16(), 400);
        assert!(err.hint.is_some(), "error should carry recovery hint");
    }

    /// P1-5: MQTT subscribe/unsubscribe without an adapter returns 503
    /// `ErrorResponse` (never HTTP 200 with `"success": false`), and memory
    /// file endpoints return clean `ApiResponse` DTOs without inner `success`.
    #[tokio::test]
    async fn mqtt_subscription_and_memory_handlers_return_clean_envelopes() {
        use neomind_api::handlers::memory::{
            get_memory_file, update_memory_file, UpdateMemoryRequest,
        };
        use neomind_api::handlers::mqtt::models::{MqttSubscribeRequest, MqttUnsubscribeRequest};
        use neomind_api::handlers::mqtt::{subscribe_handler, unsubscribe_handler};

        let state = create_test_server_state().await;

        let sub_err = subscribe_handler(
            State(state.clone()),
            Json(MqttSubscribeRequest {
                topic: "sensors/#".to_string(),
                qos: 1,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(sub_err.status.as_u16(), 503);
        assert!(sub_err.hint.is_some());

        let unsub_err = unsubscribe_handler(
            State(state.clone()),
            Json(MqttUnsubscribeRequest {
                topic: "sensors/#".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(unsub_err.status.as_u16(), 503);
        assert!(unsub_err.hint.is_some());

        let bad_mem = get_memory_file(State(state.clone()), Path("invalid".to_string()))
            .await
            .unwrap_err();
        assert_eq!(bad_mem.status.as_u16(), 400);
        assert!(bad_mem.hint.is_some());

        let updated = update_memory_file(
            State(state.clone()),
            Path("user".to_string()),
            Json(UpdateMemoryRequest {
                content: "# User Notes\n".to_string(),
            }),
        )
        .await
        .expect("update_memory_file must succeed");
        assert!(updated.0.success);
        let updated_data = updated.0.data.expect("payload");
        assert!(
            updated_data.get("success").is_none(),
            "inner DTO must not duplicate `success`: {updated_data}"
        );

        let fetched = get_memory_file(State(state), Path("user".to_string()))
            .await
            .expect("get_memory_file must succeed");
        assert!(fetched.0.success);
        let fetched_data = fetched.0.data.expect("payload");
        assert!(
            fetched_data.get("success").is_none(),
            "inner DTO must not duplicate `success`: {fetched_data}"
        );
        assert_eq!(fetched_data["target"], json!("user"));
    }
}
