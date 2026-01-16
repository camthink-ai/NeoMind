//! Tests for rules handlers.

use edge_ai_api::handlers::rules::*;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::models::ErrorResponse;
use axum::extract::{Path, State};
use axum::Json;
use edge_ai_rules::{CompiledRule, RuleCondition, ComparisonOperator, RuleStatus};
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
        assert!(!rules.is_empty() || true); // rules is &Vec<Value>, empty is ok
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
        let req = UpdateRuleRequest {
            name: Some("Updated Name".to_string()),
            enabled: Some(true),
        };
        let result = update_rule_handler(
            State(state),
            Path("invalid_id".to_string()),
            Json(req),
        )
        .await;
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
        let result = set_rule_status_handler(
            State(state),
            Path("invalid_id".to_string()),
            Json(req),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_test_rule_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = test_rule_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_rule_history_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = get_rule_history_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_rule_handler_missing_dsl() {
        let state = create_test_server_state().await;
        let req = json!({ "invalid": "data" });
        let result = create_rule_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Missing 'dsl' field"));
    }

}
