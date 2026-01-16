//! Tests for basic handlers.

use edge_ai_api::handlers::basic::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::State;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_handler() {
        let result = health_handler().await;
        let value = result.0;
        assert_eq!(value.get("status").unwrap().as_str().unwrap(), "ok");
        assert!(value.get("service").is_some());
        assert!(value.get("version").is_some());
    }

    #[tokio::test]
    async fn test_health_status_handler() {
        let state = create_test_server_state().await;
        let result = health_status_handler(State(state)).await;
        assert_eq!(result.0.status, "healthy");
        assert_eq!(result.0.service, "edge-ai-agent");
        assert!(result.0.version.len() > 0);
        assert!(result.0.uptime >= 0);
    }

    #[tokio::test]
    async fn test_liveness_handler() {
        let result = liveness_handler().await;
        let value = result.0;
        assert_eq!(value.get("status").unwrap().as_str().unwrap(), "alive");
    }

    #[tokio::test]
    async fn test_readiness_handler() {
        let state = create_test_server_state().await;
        let result = readiness_handler(State(state)).await;
        assert!(result.0.ready);
        assert!(result.0.dependencies.llm || result.0.dependencies.mqtt || result.0.dependencies.database);
    }

    #[tokio::test]
    async fn test_dependency_status_all_ready() {
        let deps = DependencyStatus {
            llm: true,
            mqtt: true,
            database: true,
        };
        assert!(deps.all_ready());
    }

    #[tokio::test]
    async fn test_dependency_status_partial_ready() {
        let deps = DependencyStatus {
            llm: false,
            mqtt: false,
            database: true,
        };
        assert!(deps.all_ready()); // At least one is ready
    }

    #[tokio::test]
    async fn test_dependency_status_none_ready() {
        let deps = DependencyStatus {
            llm: false,
            mqtt: false,
            database: false,
        };
        assert!(!deps.all_ready());
    }
}
