//! Tests for sessions handlers.

use edge_ai_api::handlers::sessions::*;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::models::ChatRequest;
use axum::extract::{Path, Query, State};
use axum::Json;
use std::collections::HashMap;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session_handler() {
        let state = create_test_server_state().await;
        let result = create_session_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("sessionId").is_some());
    }

    #[tokio::test]
    async fn test_list_sessions_handler_default_params() {
        let state = create_test_server_state().await;
        let query = ListSessionsQuery {
            page: 1,
            page_size: 20,
        };
        let result = list_sessions_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        // value is Vec<serde_json::Value>
        assert!(!value.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_handler_with_pagination() {
        let state = create_test_server_state().await;
        let query = ListSessionsQuery {
            page: 1,
            page_size: 10,
        };
        let result = list_sessions_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.0.data.is_some());
        // Pagination info should be present
        assert!(response.0.meta.is_some());
    }

    #[tokio::test]
    async fn test_get_session_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_session_handler(State(state), Path("nonexistent_session".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_session_history_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_session_history_handler(State(state), Path("nonexistent_session".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_session_handler_not_found() {
        let state = create_test_server_state().await;
        let result = delete_session_handler(State(state), Path("nonexistent_session".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_session_handler_not_found() {
        let state = create_test_server_state().await;
        let req = UpdateSessionRequest {
            title: Some("Test Title".to_string()),
        };
        let result = update_session_handler(State(state), Path("nonexistent_session".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cleanup_sessions_handler() {
        let state = create_test_server_state().await;
        let result = cleanup_sessions_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("cleaned").is_some());
        assert!(value.get("message").is_some());
    }

    #[tokio::test]
    async fn test_chat_handler_not_found() {
        let state = create_test_server_state().await;
        let req = ChatRequest {
            message: "Hello".to_string(),
            session_id: None,
        };
        let result = chat_handler(State(state), Path("nonexistent_session".to_string()), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_chat_handler_creates_session() {
        let state = create_test_server_state().await;

        // First create a session
        let create_result = create_session_handler(State(state.clone())).await.unwrap();
        let session_id = create_result.0.data.unwrap().get("sessionId").unwrap().as_str().unwrap().to_string();

        // Then send a chat message (will likely fail due to no LLM configured, but shouldn't be 404)
        let req = ChatRequest {
            message: "Hello".to_string(),
            session_id: None,
        };
        let result = chat_handler(State(state), Path(session_id.clone()), Json(req)).await;
        // Either Ok with timeout message or Err with something other than NOT_FOUND
        match result {
            Ok(resp) => {
                assert!(!resp.session_id.is_empty());
            }
            Err(err) => {
                assert_ne!(err.status, axum::http::StatusCode::NOT_FOUND);
            }
        }
    }

    #[tokio::test]
    async fn test_session_list_item() {
        let item = SessionListItem {
            id: "test-id".to_string(),
            message_count: 5,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(item.id, "test-id");
        assert_eq!(item.message_count, 5);
        assert!(item.created_at.contains("T"));
    }

    #[tokio::test]
    async fn test_update_session_request() {
        let req = UpdateSessionRequest {
            title: Some("New Title".to_string()),
        };
        assert_eq!(req.title.unwrap(), "New Title");

        let req_empty = UpdateSessionRequest {
            title: None,
        };
        assert!(req_empty.title.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions_query_defaults() {
        let query = ListSessionsQuery {
            page: 1,
            page_size: 20,
        };
        assert_eq!(query.page, 1);
        assert_eq!(query.page_size, 20);
    }

    #[tokio::test]
    async fn test_chat_request() {
        let req = ChatRequest {
            message: "Test message".to_string(),
            session_id: Some("session123".to_string()),
        };
        assert_eq!(req.message, "Test message");
        assert_eq!(req.session_id.unwrap(), "session123");
    }

    #[tokio::test]
    async fn test_list_sessions_query_params() {
        let query = ListSessionsQuery {
            page: 1,
            page_size: 20,
        };
        assert_eq!(query.page, 1);
        assert_eq!(query.page_size, 20);
    }
}
