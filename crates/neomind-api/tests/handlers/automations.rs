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
}
