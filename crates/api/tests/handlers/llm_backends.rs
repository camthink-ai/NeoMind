//! Tests for LLM backend management handlers.

use edge_ai_api::handlers::llm_backends::*;
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
    async fn test_list_backends_handler() {
        let state = create_test_server_state().await;
        let query = ListBackendsQuery {
            r#type: None,
            active_only: None,
        };
        let result = list_backends_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("backends").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_list_backends_handler_with_type_filter() {
        let state = create_test_server_state().await;
        let query = ListBackendsQuery {
            r#type: Some("ollama".to_string()),
            active_only: None,
        };
        let result = list_backends_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        let backends = value.get("backends").unwrap().as_array().unwrap();
        // All returned backends should be ollama type
        for backend in backends {
            if let Some(backend_obj) = backend.as_object() {
                let backend_type = backend_obj.get("backend_type").and_then(|v| v.as_str()).unwrap_or("");
                assert_eq!(backend_type, "ollama");
            }
        }
    }

    #[tokio::test]
    async fn test_list_backends_handler_active_only() {
        let state = create_test_server_state().await;
        let query = ListBackendsQuery {
            r#type: None,
            active_only: Some(true),
        };
        let result = list_backends_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("backends").is_some());
    }

    #[tokio::test]
    async fn test_get_backend_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_backend_handler(State(state), Path("nonexistent_backend".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_backend_handler_invalid_type() {
        let state = create_test_server_state().await;
        let req = CreateBackendRequest {
            name: "Test Backend".to_string(),
            backend_type: "invalid_type".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            model: "test-model".to_string(),
            api_key: None,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 4096,
            thinking_enabled: true,
            capabilities: None,
        };
        let result = create_backend_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Unknown backend type") || err.message.contains("invalid_type"));
    }

    #[tokio::test]
    async fn test_create_backend_handler_ollama() {
        let state = create_test_server_state().await;
        let req = CreateBackendRequest {
            name: "Test Ollama".to_string(),
            backend_type: "ollama".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            model: "qwen3:2b".to_string(),
            api_key: None,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 4096,
            thinking_enabled: true,
            capabilities: None,
        };
        let result = create_backend_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("id").is_some());
        assert!(value.get("message").is_some());
    }

    #[tokio::test]
    async fn test_create_backend_handler_openai() {
        let state = create_test_server_state().await;
        let req = CreateBackendRequest {
            name: "Test OpenAI".to_string(),
            backend_type: "openai".to_string(),
            endpoint: Some("https://api.openai.com/v1".to_string()),
            model: "gpt-4".to_string(),
            api_key: Some("sk-test-key".to_string()),
            temperature: 0.5,
            top_p: 1.0,
            max_tokens: 8192,
            thinking_enabled: false,
            capabilities: None,
        };
        let result = create_backend_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("id").is_some());
    }

    #[tokio::test]
    async fn test_update_backend_handler_not_found() {
        let state = create_test_server_state().await;
        let req = UpdateBackendRequest {
            name: Some("Updated Name".to_string()),
            endpoint: None,
            model: None,
            api_key: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            thinking_enabled: None,
        };
        let result = update_backend_handler(State(state), Path("nonexistent_backend".to_string()), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_backend_handler() {
        let state = create_test_server_state().await;
        // First create a backend
        let create_req = CreateBackendRequest {
            name: "To Delete".to_string(),
            backend_type: "ollama".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            model: "test-model".to_string(),
            api_key: None,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 4096,
            thinking_enabled: false,
            capabilities: None,
        };
        let create_result = create_backend_handler(State(state.clone()), Json(create_req)).await.unwrap();
        let backend_id = create_result.0.data.unwrap().get("id").unwrap().as_str().unwrap().to_string();

        // Then delete it
        let delete_result = delete_backend_handler(State(state), Path(backend_id)).await;
        assert!(delete_result.is_ok());
    }

    #[tokio::test]
    async fn test_activate_backend_handler_not_found() {
        let state = create_test_server_state().await;
        let result = activate_backend_handler(State(state), Path("nonexistent_backend".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_backend_types_handler() {
        let state = create_test_server_state().await;
        let result = list_backend_types_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("types").is_some());
        let types = value.get("types").unwrap().as_array().unwrap();
        assert!(!types.is_empty());
    }

    #[tokio::test]
    async fn test_get_backend_schema_handler() {
        let state = create_test_server_state().await;
        let result = get_backend_schema_handler(State(state), Path("ollama".to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("backend_type").unwrap().as_str().unwrap(), "ollama");
        assert!(value.get("schema").is_some());
    }

    #[tokio::test]
    async fn test_get_backend_stats_handler() {
        let state = create_test_server_state().await;
        let result = get_backend_stats_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("total").is_some());
        assert!(value.get("active_id").is_some());
        assert!(value.get("by_type").is_some());
    }

    #[tokio::test]
    async fn test_create_backend_request_defaults() {
        let req = CreateBackendRequest {
            name: "Test".to_string(),
            backend_type: "ollama".to_string(),
            endpoint: None,
            model: "qwen3".to_string(),
            api_key: None,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: usize::MAX,
            thinking_enabled: true,
            capabilities: None,
        };
        assert_eq!(req.temperature, 0.7);
        assert_eq!(req.top_p, 0.9);
        assert_eq!(req.max_tokens, usize::MAX);
        assert_eq!(req.thinking_enabled, true);
    }

    #[tokio::test]
    async fn test_update_backend_request() {
        let req = UpdateBackendRequest {
            name: Some("New Name".to_string()),
            endpoint: Some("http://new-endpoint".to_string()),
            model: Some("new-model".to_string()),
            api_key: Some("new-key".to_string()),
            temperature: Some(0.5),
            top_p: Some(1.0),
            max_tokens: Some(2048),
            thinking_enabled: Some(false),
        };
        assert_eq!(req.name.unwrap(), "New Name");
        assert_eq!(req.temperature.unwrap(), 0.5);
        assert_eq!(req.thinking_enabled.unwrap(), false);
    }

    #[tokio::test]
    async fn test_backend_instance_dto() {
        let dto = BackendInstanceDto {
            id: "backend1".to_string(),
            name: "Test Backend".to_string(),
            backend_type: "ollama".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            model: "qwen3:2b".to_string(),
            api_key_configured: false,
            is_active: true,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 4096,
            thinking_enabled: true,
            capabilities: edge_ai_storage::BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: true,
                supports_tools: true,
                max_context: 8192,
            },
            updated_at: 1234567890,
            healthy: Some(true),
        };
        assert_eq!(dto.id, "backend1");
        assert_eq!(dto.is_active, true);
        assert_eq!(dto.healthy.unwrap(), true);
        assert_eq!(dto.capabilities.max_context, 8192);
    }
}
