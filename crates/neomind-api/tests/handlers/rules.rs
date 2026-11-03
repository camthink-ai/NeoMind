//! Tests for rules handlers.

use axum::extract::{Path, Query, State};
use axum::Json;
use neomind_api::handlers::rules::*;
use neomind_api::handlers::ServerState;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_rules_handler() {
        let state = create_test_server_state().await;
        let result = list_rules_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("rules").is_some());
        assert!(value.get("count").is_some());
        let rules = value.get("rules").unwrap().as_array().unwrap();
        // Should return an array even if empty
        let _ = rules; // rules is &Vec<Value>, empty is ok
    }

    #[tokio::test]
    async fn test_get_rule_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = get_rule_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_rule_handler_not_found() {
        let state = create_test_server_state().await;
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let result = get_rule_handler(State(state), Path(fake_id.to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_rule_handler_invalid_id() {
        let state = create_test_server_state().await;
        let req = json!({
            "name": "Updated Name",
            "enabled": true
        });
        let result =
            update_rule_handler(State(state), Path("invalid_id".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_rule_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = delete_rule_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_rule_status_handler_invalid_id() {
        let state = create_test_server_state().await;
        let req = SetRuleStatusRequest { enabled: true };
        let result =
            set_rule_status_handler(State(state), Path("invalid_id".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_test_rule_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = test_rule_handler(
            State(state),
            Path("invalid_id".to_string()),
            Query(std::collections::HashMap::new()),
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_rule_history_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = get_rule_history_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_rule_handler_missing_name() {
        let state = create_test_server_state().await;
        let req = json!({ "invalid": "data" });
        let result = create_rule_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Missing 'name' field"));
    }

    async fn seed_sensor_rule(state: &ServerState) -> neomind_rules::RuleId {
        // Rule shape from the T4 552 acceptance trace
        // full--multi-turn-correction--en--r0: created via `neomind rule create
        // --trigger-device sensor-001 --metric values.battery ...`.
        let rule: neomind_rules::CompiledRule = serde_json::from_value(json!({
            "id": "0defb220-a8fc-45e7-8705-ca0753f72bc1",
            "name": "ctx-correct-rule",
            "enabled": true,
            "trigger_count": 0,
            "created_at": "2026-11-03T10:07:46.137716968+00:00",
            "updated_at": "2026-11-03T10:07:46.137726241+00:00",
            "trigger": {
                "trigger_type": "data_change",
                "sources": ["device:sensor-001:values.battery"]
            },
            "condition": {
                "condition_type": "comparison",
                "source": "device:sensor-001:values.battery",
                "source_type": "device",
                "source_id": "sensor-001",
                "operator": "less_than",
                "threshold": 20.0
            },
            "actions": [
                {"type": "notify", "message": "Battery below 20: {value}%", "severity": "warning"}
            ]
        }))
        .expect("valid CompiledRule");
        let id = rule.id.clone();
        state.automation.rule_engine.update_rule(rule).await.unwrap();
        id
    }

    // Regression pinned from the raw trace full--multi-turn-correction--en--r0
    // (2026-11-03, T4 552 acceptance): `neomind rule update <id> --body
    // '{"name":"ctx-correct-rule","trigger_device":"sensor-002","metric":...,
    // "operator":...,"threshold":20,"notify":...}' returned success=true with
    // the rule STILL pointing at sensor-001 (only updated_at changed) — the
    // update endpoint silently dropped unknown fields and reported the no-op
    // partial update as "updated". Contract (user-spec 2026-11-04): unknown
    // fields — even mixed with valid ones — reject the WHOLE request with a
    // structured 400; nothing is written, rule and updated_at unchanged.
    #[tokio::test]
    async fn test_update_rule_rejects_unknown_field_noop_body() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;
        let before = state.automation.rule_engine.get_rule(&id).await.unwrap();

        // EXACT body from the raw trace (create-style flag names; the request
        // also carries a valid `name`, which must NOT let it through).
        let req = json!({
            "name": "ctx-correct-rule",
            "trigger_device": "sensor-002",
            "metric": "values.battery",
            "operator": "less_than",
            "threshold": 20,
            "notify": "Battery below 20: {value}%"
        });
        let result =
            update_rule_handler(State(state.clone()), Path(id.to_string()), Json(req)).await;
        assert!(
            result.is_err(),
            "unknown-field no-op update must not report success"
        );
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(
            err.message.contains("trigger_device"),
            "error must list the unknown fields"
        );

        // The whole request must not be written: rule content AND updated_at
        // stay exactly as before.
        let after = state.automation.rule_engine.get_rule(&id).await.unwrap();
        let source_of = |r: &neomind_rules::CompiledRule| {
            r.condition
                .as_ref()
                .and_then(|c| serde_json::to_value(c).ok())
                .and_then(|v| v.get("source_id").cloned())
                .unwrap_or(serde_json::Value::Null)
        };
        assert_eq!(
            source_of(&after),
            source_of(&before),
            "condition must be untouched after rejected update"
        );
        assert_eq!(
            after.updated_at, before.updated_at,
            "updated_at must not move on a rejected update"
        );
    }

    #[tokio::test]
    async fn test_update_rule_rejects_empty_body() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;
        let result =
            update_rule_handler(State(state), Path(id.to_string()), Json(json!({}))).await;
        assert!(result.is_err(), "empty body must be rejected");
        assert_eq!(result.unwrap_err().status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_rule_rejects_wrong_field_type() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;
        let result = update_rule_handler(
            State(state),
            Path(id.to_string()),
            Json(json!({ "name": 123 })),
        )
        .await;
        assert!(result.is_err(), "wrong-typed field must be rejected");
        assert_eq!(result.unwrap_err().status, axum::http::StatusCode::BAD_REQUEST);
    }

    // Idempotent updates (same value) are legal and must succeed with
    // changed=false, without moving updated_at.
    #[tokio::test]
    async fn test_update_rule_idempotent_same_value_succeeds_changed_false() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;
        let before = state.automation.rule_engine.get_rule(&id).await.unwrap();

        let req = json!({ "name": "ctx-correct-rule" });
        let result =
            update_rule_handler(State(state.clone()), Path(id.to_string()), Json(req)).await;
        assert!(result.is_ok(), "idempotent same-value update must succeed");
        let rule = result.unwrap().0.data.unwrap();
        assert_eq!(rule["changed"], false);

        let after = state.automation.rule_engine.get_rule(&id).await.unwrap();
        assert_eq!(after.updated_at, before.updated_at);
    }

    // A frontend-style PUT carrying CompiledRule metadata keys (tags/cooldown)
    // must not trip the unknown-field rejection.
    #[tokio::test]
    async fn test_update_rule_accepts_compiled_rule_metadata_keys() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;
        let req = json!({
            "name": "ctx-correct-rule",
            "enabled": true,
            "tags": [],
            "cooldown": 60000
        });
        let result =
            update_rule_handler(State(state), Path(id.to_string()), Json(req)).await;
        assert!(result.is_ok(), "metadata passthrough keys must be accepted");
    }

    // Positive control for the same contract: a recognized partial field must
    // keep working and must not disturb the rest of the rule.
    #[tokio::test]
    async fn test_update_rule_partial_rename_preserves_condition() {
        let state = create_test_server_state().await;
        let id = seed_sensor_rule(&state).await;

        let req = json!({ "name": "ctx-correct-rule-renamed" });
        let result = update_rule_handler(State(state), Path(id.to_string()), Json(req)).await;
        assert!(result.is_ok(), "recognized partial update should succeed");
        let response = result.unwrap();
        let rule = response.0.data.unwrap();
        assert_eq!(rule["rule"]["name"], "ctx-correct-rule-renamed");
        assert_eq!(rule["rule"]["condition"]["source_id"], "sensor-001");
        assert_eq!(rule["changed"], true);
    }
}
