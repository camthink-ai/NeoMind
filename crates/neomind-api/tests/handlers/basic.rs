//! Tests for basic handlers.

use axum::extract::State;
use neomind_api::handlers::basic::*;
use neomind_api::handlers::ServerState;

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
        assert!(!result.0.version.is_empty());
        let _ = result.0.uptime; // u64 is always >= 0
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
        // Readiness is now REAL: it gates on the verifiable dependencies
        // (database reachable && LLM configured). In the bare test state
        // there is no active LLM backend, so ready must be false and a
        // note must explain why — previously everything was hardcoded
        // true and a broker outage was invisible (2026-08-14 eval).
        assert!(!result.0.ready);
        assert!(!result.0.dependencies.llm);
        assert!(
            result.0.dependencies.database,
            "test state storage should be reachable"
        );
        assert!(
            !result.0.notes.is_empty(),
            "unready dependencies must be explained by notes"
        );
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
        // Readiness requires ALL gateable dependencies (was `||`, which
        // reported ready when only one was up).
        let deps = DependencyStatus {
            llm: false,
            mqtt: false,
            database: true,
        };
        assert!(!deps.all_ready());
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
