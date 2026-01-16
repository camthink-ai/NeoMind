//! Tests for tools handlers.

use edge_ai_api::handlers::tools::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::State;
use axum::Json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_tools_handler() {
        let state = create_test_server_state().await;
        let result = list_tools_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("tools").is_some());
        assert!(value.get("count").is_some());
        let tools = value.get("tools").unwrap().as_array().unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn test_get_tool_schema_handler_found() {
        let state = create_test_server_state().await;
        let list_result = list_tools_handler(State(state.clone())).await.unwrap();
        let data = list_result.0.data.unwrap();
        let tools = data.get("tools").unwrap().as_array().unwrap();
        let first_tool_name = tools[0].get("name").unwrap().as_str().unwrap();
        let result = get_tool_schema_handler(
            State(state),
            axum::extract::Path(first_tool_name.to_string()),
        )
        .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("tool").is_some());
        let tool = value.get("tool").unwrap();
        assert_eq!(tool.get("name").unwrap().as_str().unwrap(), first_tool_name);
        assert!(tool.get("description").is_some());
        assert!(tool.get("parameters").is_some());
    }

    #[tokio::test]
    async fn test_get_tool_schema_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_tool_schema_handler(
            State(state),
            axum::extract::Path("nonexistent_tool".to_string()),
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_tool_metrics_handler() {
        let state = create_test_server_state().await;
        let result = get_tool_metrics_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("metrics").is_some());
        let metrics = value.get("metrics").unwrap().as_object().unwrap();
        assert!(!metrics.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tool_handler_invalid_tool() {
        let state = create_test_server_state().await;
        let result = execute_tool_handler(
            State(state),
            axum::extract::Path("nonexistent_tool".to_string()),
            Json(serde_json::json!({})),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_format_for_llm_handler() {
        let state = create_test_server_state().await;
        let result = format_for_llm_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("formatted").is_some());
        let formatted = value.get("formatted").unwrap().as_str().unwrap();
        assert!(!formatted.is_empty());
        assert!(formatted.contains("Available tools"));
    }

}
