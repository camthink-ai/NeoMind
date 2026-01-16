//! Tests for workflows handlers.

use edge_ai_api::handlers::workflows::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::State;
use axum::Json;
use edge_ai_workflow::{Step, Trigger, TriggerManager, Workflow, WorkflowEngine, WorkflowStatus};
use std::path::PathBuf;

async fn create_test_server_state_with_workflow() -> ServerState {
    crate::common::create_test_server_state_with_workflow().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_workflows_handler() {
        let state = create_test_server_state_with_workflow().await;
        let result = list_workflows_handler(State(state)).await;
        match result {
            Ok(response) => {
                let value = response.0.data.unwrap();
                assert!(value.get("workflows").is_some());
                assert!(value.get("count").is_some());
            }
            Err(e) => {
                assert!(matches!(e.status.as_u16(), 500 | 503));
            }
        }
    }

    #[tokio::test]
    async fn test_create_workflow_handler() {
        let state = create_test_server_state_with_workflow().await;
        let request = CreateWorkflowRequest {
            name: "Test Workflow".to_string(),
            description: "A test workflow".to_string(),
            steps: vec![
                Step::Delay {
                    id: "step1".to_string(),
                    duration_seconds: 1,
                }
            ],
            triggers: vec![],
            timeout_seconds: Some(30),
        };
        let result = create_workflow_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("message").is_some());
        assert!(value.get("workflow_id").is_some());
    }

    #[tokio::test]
    async fn test_create_workflow_handler_with_log_step() {
        let state = create_test_server_state_with_workflow().await;
        let request = CreateWorkflowRequest {
            name: "Log Workflow".to_string(),
            description: "A workflow with log step".to_string(),
            steps: vec![
                Step::Log {
                    id: "step1".to_string(),
                    message: "Test log message".to_string(),
                    level: "info".to_string(),
                }
            ],
            triggers: vec![],
            timeout_seconds: None,
        };
        let result = create_workflow_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_workflow_handler_not_found() {
        let state = create_test_server_state_with_workflow().await;
        let result = get_workflow_handler(
            State(state),
            axum::extract::Path("nonexistent_workflow".to_string()),
        )
        .await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.status == axum::http::StatusCode::NOT_FOUND || e.status == axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    #[tokio::test]
    async fn test_delete_workflow_handler() {
        let state = create_test_server_state_with_workflow().await;
        let create_request = CreateWorkflowRequest {
            name: "Workflow to Delete".to_string(),
            description: "This will be deleted".to_string(),
            steps: vec![
                Step::Log {
                    id: "step1".to_string(),
                    message: "Delete test".to_string(),
                    level: "info".to_string(),
                }
            ],
            triggers: vec![],
            timeout_seconds: None,
        };
        let create_result = create_workflow_handler(State(state.clone()), Json(create_request)).await.unwrap();
        let workflow_id = create_result.0.data.unwrap().get("workflow_id").unwrap().as_str().unwrap().to_string();
        let delete_result = delete_workflow_handler(
            State(state),
            axum::extract::Path(workflow_id.clone()),
        )
        .await;
        assert!(delete_result.is_ok());
        let response = delete_result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("message").unwrap().as_str().unwrap(), "Workflow deleted");
    }

    #[tokio::test]
    async fn test_set_workflow_status_handler() {
        let state = create_test_server_state_with_workflow().await;
        let create_request = CreateWorkflowRequest {
            name: "Toggle Workflow".to_string(),
            description: "Test enable/disable".to_string(),
            steps: vec![
                Step::Log {
                    id: "step1".to_string(),
                    message: "Test".to_string(),
                    level: "info".to_string(),
                }
            ],
            triggers: vec![],
            timeout_seconds: None,
        };
        let create_result = create_workflow_handler(State(state.clone()), Json(create_request)).await.unwrap();
        let workflow_id = create_result.0.data.unwrap().get("workflow_id").unwrap().as_str().unwrap().to_string();
        let disable_request = SetWorkflowStatusRequest { enabled: false };
        let disable_result = set_workflow_status_handler(
            State(state.clone()),
            axum::extract::Path(workflow_id.clone()),
            Json(disable_request),
        )
        .await;
        assert!(disable_result.is_ok());
        let response = disable_result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("enabled").unwrap().as_bool().unwrap(), false);
    }

    #[tokio::test]
    async fn test_execute_workflow_handler() {
        let state = create_test_server_state_with_workflow().await;
        let create_request = CreateWorkflowRequest {
            name: "Executable Workflow".to_string(),
            description: "Test execution".to_string(),
            steps: vec![
                Step::Log {
                    id: "step1".to_string(),
                    message: "Execution test".to_string(),
                    level: "info".to_string(),
                }
            ],
            triggers: vec![],
            timeout_seconds: Some(5),
        };
        let create_result = create_workflow_handler(State(state.clone()), Json(create_request)).await.unwrap();
        let workflow_id = create_result.0.data.unwrap().get("workflow_id").unwrap().as_str().unwrap().to_string();
        let execute_result = execute_workflow_handler(
            State(state),
            axum::extract::Path(workflow_id),
        )
        .await;
        assert!(execute_result.is_ok());
        let response = execute_result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("execution_id").is_some());
        assert!(value.get("status").is_some());
    }

    #[tokio::test]
    async fn test_update_workflow_handler() {
        let state = create_test_server_state_with_workflow().await;
        let create_request = CreateWorkflowRequest {
            name: "Original Name".to_string(),
            description: "Original description".to_string(),
            steps: vec![
                Step::Log {
                    id: "step1".to_string(),
                    message: "Test".to_string(),
                    level: "info".to_string(),
                }
            ],
            triggers: vec![],
            timeout_seconds: None,
        };
        let create_result = create_workflow_handler(State(state.clone()), Json(create_request)).await;
        assert!(create_result.is_ok(), "Create workflow should succeed");
        let data = create_result.unwrap().0.data.unwrap();
        let workflow_id = data.get("workflow_id").unwrap().as_str().unwrap().to_string();
        let update_request = UpdateWorkflowRequest {
            name: Some("Updated Name".to_string()),
            description: Some("Updated description".to_string()),
            enabled: Some(false),
        };
        let update_result = update_workflow_handler(
            State(state),
            axum::extract::Path(workflow_id),
            Json(update_request),
        )
        .await;
        if let Err(e) = &update_result {
            eprintln!("Update error: {} - {}", e.code, e.message);
        }
        assert!(update_result.is_ok());
        let response = update_result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("message").unwrap().as_str().unwrap(), "Workflow updated");
    }

}
