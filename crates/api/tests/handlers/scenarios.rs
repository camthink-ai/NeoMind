//! Tests for scenarios handlers.

use edge_ai_api::handlers::scenarios::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::{Path, Query, State};
use axum::Json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_scenarios_handler() {
        let state = create_test_server_state().await;
        let result = list_scenarios_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("scenarios").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_list_active_scenarios_handler() {
        let state = create_test_server_state().await;
        let result = list_active_scenarios_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("scenarios").is_some());
    }

    #[tokio::test]
    async fn test_get_scenario_stats_handler() {
        let state = create_test_server_state().await;
        let result = get_scenario_stats_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("stats").is_some());
        let stats = value.get("stats").unwrap();
        assert!(stats.get("total").is_some());
        assert!(stats.get("active").is_some());
    }

    #[tokio::test]
    async fn test_get_scenario_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = get_scenario_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_scenario_handler_not_found() {
        let state = create_test_server_state().await;
        // Valid UUID format but likely doesn't exist
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let result = get_scenario_handler(State(state), Path(fake_id.to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_scenario_handler() {
        let state = create_test_server_state().await;
        let req = CreateScenarioRequest {
            name: "Test Scenario".to_string(),
            description: "A test scenario".to_string(),
            category: "monitoring".to_string(),
            environment: "office".to_string(),
            business_context: "testing".to_string(),
            tags: vec!["test".to_string()],
            priority: 5,
        };
        let result = create_scenario_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("message").is_some());
        assert!(value.get("scenario").is_some());
    }

    #[tokio::test]
    async fn test_update_scenario_handler_invalid_id() {
        let state = create_test_server_state().await;
        let req = UpdateScenarioRequest {
            name: Some("Updated Name".to_string()),
            description: Some("Updated description".to_string()),
            category: None,
            environment: None,
            business_context: None,
            tags: None,
            priority: None,
            is_active: None,
        };
        let result = update_scenario_handler(State(state), Path("invalid_id".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_scenario_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = delete_scenario_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_activate_scenario_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = activate_scenario_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deactivate_scenario_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = deactivate_scenario_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_device_handler() {
        let state = create_test_server_state().await;
        let req = AddDeviceRequest {
            device_id: "sensor1".to_string(),
        };
        let result = add_device_handler(State(state), Path("invalid_id".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_device_handler() {
        let state = create_test_server_state().await;
        let result = remove_device_handler(State(state), Path(("invalid_id".to_string(), "sensor1".to_string()))).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_rule_handler() {
        let state = create_test_server_state().await;
        let req = AddRuleRequest {
            rule_id: "rule1".to_string(),
        };
        let result = add_rule_handler(State(state), Path("invalid_id".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_rule_handler() {
        let state = create_test_server_state().await;
        let result = remove_rule_handler(State(state), Path(("invalid_id".to_string(), "rule1".to_string()))).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_templates_handler() {
        let state = create_test_server_state().await;
        let result = list_templates_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("templates").is_some());
        let templates = value.get("templates").unwrap().as_array().unwrap();
        assert!(!templates.is_empty());
    }

    #[tokio::test]
    async fn test_create_from_template_handler_unknown_template() {
        let state = create_test_server_state().await;
        let params = serde_json::json!({});
        let result = create_from_template_handler(State(state), Path("unknown_template".to_string()), Query(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Unknown template"));
    }

    #[tokio::test]
    async fn test_create_from_template_handler_datacenter() {
        let state = create_test_server_state().await;
        let params = serde_json::json!({ "name": "My Datacenter" });
        let result = create_from_template_handler(State(state), Path("datacenter_temperature".to_string()), Query(params)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("message").is_some());
        assert!(value.get("scenario").is_some());
    }

    #[tokio::test]
    async fn test_get_scenario_prompt_handler_invalid_id() {
        let state = create_test_server_state().await;
        let result = get_scenario_prompt_handler(State(state), Path("invalid_id".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_scenario_handler_not_found() {
        let state = create_test_server_state().await;
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let result = execute_scenario_handler(State(state), Path(fake_id.to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }
}
