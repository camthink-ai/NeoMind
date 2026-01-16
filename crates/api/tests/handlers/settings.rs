//! Tests for settings handlers.

use edge_ai_api::handlers::settings::*;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::config::LlmSettingsRequest;
use axum::extract::{Query, State};
use axum::Json;
use std::collections::HashMap;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_llm_settings_handler_no_settings() {
        // This test will pass if no settings are configured
        let result = get_llm_settings_handler().await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        // Should return null values when no settings configured
        assert!(value.get("backend").is_some());
    }

    #[tokio::test]
    async fn test_set_llm_handler_invalid_backend() {
        let state = create_test_server_state().await;
        let settings = LlmSettingsRequest {
            backend: "invalid_backend".to_string(),
            model: "test-model".to_string(),
            endpoint: None,
            api_key: None,
        };
        let result = set_llm_handler(State(state), Json(settings)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Invalid backend") || err.message.contains("not supported"));
    }

    #[tokio::test]
    async fn test_list_ollama_models_handler_empty_params() {
        let params = HashMap::new();
        let result = list_ollama_models_handler(Query(params)).await;
        // This may fail if Ollama is not running, but should return empty array
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("models").is_some() || value.get("error").is_some());
    }

    #[tokio::test]
    async fn test_list_ollama_models_handler_with_endpoint() {
        let mut params = HashMap::new();
        params.insert("endpoint".to_string(), "http://localhost:11434".to_string());
        let result = list_ollama_models_handler(Query(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_llm_generate_handler_no_config() {
        let state = create_test_server_state().await;
        let req = LlmGenerateRequest {
            prompt: "Hello".to_string(),
        };
        let result = llm_generate_handler(State(state), Json(req)).await;
        // Should fail because LLM is not configured
        assert!(result.is_err());
    }
}
