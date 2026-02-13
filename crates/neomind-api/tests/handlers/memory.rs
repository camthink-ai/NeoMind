//! Tests for memory system handlers.

use axum::extract::{Path, Query, State};
use axum::Json;
use neomind_api::handlers::memory::*;
use neomind_api::handlers::ServerState;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_memory_stats_handler() {
        let state = create_test_server_state().await;
        let result = get_memory_stats_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("stats").is_some());
        let stats = value.get("stats").unwrap();
        assert!(stats.get("short_term_messages").is_some());
        assert!(stats.get("mid_term_entries").is_some());
        assert!(stats.get("long_term_entries").is_some());
    }

    #[tokio::test]
    async fn test_query_memory_handler() {
        let state = create_test_server_state().await;
        let params = MemoryQueryParams {
            q: "temperature".to_string(),
            top_k: 5,
        };
        let result = query_memory_handler(State(state), Query(params)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("query").unwrap().as_str().unwrap(), "temperature");
        assert!(value.get("results").is_some());
        let results = value.get("results").unwrap();
        assert!(results.get("short_term").is_some());
        assert!(results.get("mid_term").is_some());
        assert!(results.get("long_term").is_some());
    }

    #[tokio::test]
    async fn test_consolidate_memory_handler() {
        let state = create_test_server_state().await;
        let session_id = "test_session_123";
        let result = consolidate_memory_handler(State(state), Path(session_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Memory consolidated"
        );
        assert_eq!(
            value.get("session_id").unwrap().as_str().unwrap(),
            session_id
        );
    }

    #[tokio::test]
    async fn test_get_short_term_handler() {
        let state = create_test_server_state().await;
        let result = get_short_term_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("messages").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_add_short_term_handler() {
        let state = create_test_server_state().await;
        let req = AddShortTermMemoryRequest {
            role: "user".to_string(),
            content: "Test message".to_string(),
        };
        let result = add_short_term_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("message").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_clear_short_term_handler() {
        let state = create_test_server_state().await;
        let result = clear_short_term_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Short-term memory cleared"
        );
    }

    #[tokio::test]
    async fn test_get_session_history_handler() {
        let state = create_test_server_state().await;
        let session_id = "test_session_456";
        let result = get_session_history_handler(State(state), Path(session_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("session_id").unwrap().as_str().unwrap(),
            session_id
        );
        assert!(value.get("entries").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_add_mid_term_handler() {
        let state = create_test_server_state().await;
        let req = AddMidTermMemoryRequest {
            session_id: "session_789".to_string(),
            user_input: "What is the temperature?".to_string(),
            assistant_response: "The temperature is 22 degrees.".to_string(),
        };
        let result = add_mid_term_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Added to mid-term memory"
        );
        assert_eq!(
            value.get("session_id").unwrap().as_str().unwrap(),
            "session_789"
        );
    }

    #[tokio::test]
    async fn test_search_mid_term_handler() {
        let state = create_test_server_state().await;
        let params = MemoryQueryParams {
            q: "temperature".to_string(),
            top_k: 10,
        };
        let result = search_mid_term_handler(State(state), Query(params)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("query").unwrap().as_str().unwrap(), "temperature");
        assert!(value.get("results").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_clear_mid_term_handler() {
        let state = create_test_server_state().await;
        let result = clear_mid_term_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Mid-term memory cleared"
        );
    }

    #[tokio::test]
    async fn test_search_knowledge_handler() {
        let state = create_test_server_state().await;
        let params = MemoryQueryParams {
            q: "best practices".to_string(),
            top_k: 5,
        };
        let result = search_knowledge_handler(State(state), Query(params)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("query").unwrap().as_str().unwrap(),
            "best practices"
        );
        assert!(value.get("results").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_get_knowledge_by_category_handler() {
        let state = create_test_server_state().await;
        let category = "best_practice";
        let result =
            get_knowledge_by_category_handler(State(state), Path(category.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("category").unwrap().as_str().unwrap(), category);
        assert!(value.get("results").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_get_device_knowledge_handler() {
        let state = create_test_server_state().await;
        let device_id = "sensor_temp_1";
        let result = get_device_knowledge_handler(State(state), Path(device_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("device_id").unwrap().as_str().unwrap(), device_id);
        assert!(value.get("results").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_get_popular_knowledge_handler() {
        let state = create_test_server_state().await;
        let mut params = serde_json::Map::new();
        params.insert("n".to_string(), serde_json::json!(10));
        let result =
            get_popular_knowledge_handler(State(state), Query(serde_json::Value::Object(params)))
                .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("results").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_add_knowledge_handler() {
        let state = create_test_server_state().await;
        let req = AddKnowledgeRequest {
            title: "Test Knowledge".to_string(),
            content: "This is test knowledge content.".to_string(),
            category: "best_practice".to_string(),
            tags: vec!["test".to_string(), "knowledge".to_string()],
            device_ids: vec![],
        };
        let result = add_knowledge_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Knowledge added"
        );
    }

    #[tokio::test]
    async fn test_clear_long_term_handler() {
        let state = create_test_server_state().await;
        let result = clear_long_term_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(
            value.get("message").unwrap().as_str().unwrap(),
            "Long-term memory cleared"
        );
    }

    #[tokio::test]
    async fn test_memory_query_params() {
        let params = MemoryQueryParams {
            q: "test query".to_string(),
            top_k: 10,
        };
        assert_eq!(params.q, "test query");
        assert_eq!(params.top_k, 10);
    }

    #[tokio::test]
    async fn test_add_short_term_memory_request() {
        let req = AddShortTermMemoryRequest {
            role: "assistant".to_string(),
            content: "Hello, how can I help?".to_string(),
        };
        assert_eq!(req.role, "assistant");
        assert_eq!(req.content, "Hello, how can I help?");
    }

    #[tokio::test]
    async fn test_add_mid_term_memory_request() {
        let req = AddMidTermMemoryRequest {
            session_id: "session_123".to_string(),
            user_input: "User question".to_string(),
            assistant_response: "Assistant answer".to_string(),
        };
        assert_eq!(req.session_id, "session_123");
        assert_eq!(req.user_input, "User question");
        assert_eq!(req.assistant_response, "Assistant answer");
    }

    #[tokio::test]
    async fn test_add_knowledge_request() {
        let req = AddKnowledgeRequest {
            title: "Test Entry".to_string(),
            content: "Test content".to_string(),
            category: "troubleshooting".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            device_ids: vec!["device1".to_string()],
        };
        assert_eq!(req.title, "Test Entry");
        assert_eq!(req.content, "Test content");
        assert_eq!(req.category, "troubleshooting");
        assert_eq!(req.tags.len(), 2);
        assert_eq!(req.device_ids.len(), 1);
    }
}
