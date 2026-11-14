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
