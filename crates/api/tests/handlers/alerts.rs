//! Tests for alerts handlers.

use edge_ai_api::handlers::alerts::*;
use edge_ai_api::handlers::ServerState;
use edge_ai_api::models::ErrorResponse;
use axum::extract::State;
use axum::Json;
use edge_ai_alerts::{Alert, AlertId, AlertManager, AlertSeverity, AlertStatus};

async fn create_test_alert_manager() -> AlertManager {
    let manager = AlertManager::new();
    let alert1 = Alert::new(
        AlertSeverity::Info,
        "Test Alert 1".to_string(),
        "This is a test alert".to_string(),
        "test".to_string(),
    );
    let _ = manager.create_alert(alert1).await;
    let alert2 = Alert::new(
        AlertSeverity::Warning,
        "Test Alert 2".to_string(),
        "This is a warning alert".to_string(),
        "test".to_string(),
    );
    let _ = manager.create_alert(alert2).await;
    manager
}

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_alerts_handler() {
        let state = create_test_server_state().await;
        let result = list_alerts_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response.0.data;
        assert!(data.is_some());
        let value = data.unwrap();
        assert!(value.get("alerts").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_create_alert_handler() {
        let state = create_test_server_state().await;
        let request = CreateAlertRequest {
            title: "Test Alert".to_string(),
            message: "This is a test alert".to_string(),
            severity: "info".to_string(),
            source: "test".to_string(),
        };
        let result = create_alert_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response.0.data;
        assert!(data.is_some());
        let value = data.unwrap();
        assert!(value.get("id").is_some());
        assert_eq!(value.get("title").unwrap().as_str().unwrap(), "Test Alert");
    }

    #[tokio::test]
    async fn test_create_alert_with_warning_severity() {
        let state = create_test_server_state().await;
        let request = CreateAlertRequest {
            title: "Warning Alert".to_string(),
            message: "This is a warning".to_string(),
            severity: "warning".to_string(),
            source: "test".to_string(),
        };
        let result = create_alert_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("severity").unwrap().as_str().unwrap(), "warning");
    }

    #[tokio::test]
    async fn test_create_alert_with_critical_severity() {
        let state = create_test_server_state().await;
        let request = CreateAlertRequest {
            title: "Critical Alert".to_string(),
            message: "This is critical".to_string(),
            severity: "critical".to_string(),
            source: "test".to_string(),
        };
        let result = create_alert_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("severity").unwrap().as_str().unwrap(), "critical");
    }

    #[tokio::test]
    async fn test_create_alert_with_empty_source_uses_api() {
        let state = create_test_server_state().await;
        let request = CreateAlertRequest {
            title: "Test Alert".to_string(),
            message: "Empty source".to_string(),
            severity: "info".to_string(),
            source: "".to_string(),
        };
        let result = create_alert_handler(State(state), Json(request)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_alert_handler_success() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Info,
            "Get Test".to_string(),
            "Test message".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let result = get_alert_handler(State(state), axum::extract::Path(created.id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("title").unwrap().as_str().unwrap(), "Get Test");
    }

    #[tokio::test]
    async fn test_get_alert_handler_not_found() {
        let state = create_test_server_state().await;
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let result = get_alert_handler(State(state), axum::extract::Path(fake_id.to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_acknowledge_alert_handler() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Warning,
            "Ack Test".to_string(),
            "Test acknowledge".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let result = acknowledge_alert_handler(State(state), axum::extract::Path(created.id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("acknowledged").unwrap().as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn test_resolve_alert_handler() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Warning,
            "Resolve Test".to_string(),
            "Test resolve".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let alert_id = created.id.clone();
        let result = resolve_alert_handler(State(state.clone()), axum::extract::Path(alert_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("resolved").unwrap().as_bool().unwrap(), true);
        let retrieved = state.alert_manager.get_alert(&alert_id).await.unwrap();
        assert!(matches!(retrieved.status, AlertStatus::Resolved));
    }

    #[tokio::test]
    async fn test_mark_false_positive_alert_handler() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Info,
            "False Positive Test".to_string(),
            "Test false positive".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let alert_id = created.id.clone();
        let result = mark_false_positive_alert_handler(State(state.clone()), axum::extract::Path(alert_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("false_positive").unwrap().as_bool().unwrap(), true);
        let retrieved = state.alert_manager.get_alert(&alert_id).await.unwrap();
        assert!(matches!(retrieved.status, AlertStatus::FalsePositive));
    }

    #[tokio::test]
    async fn test_delete_alert_handler() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Info,
            "Delete Test".to_string(),
            "Test delete".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let alert_id = created.id.clone();
        let result = delete_alert_handler(State(state.clone()), axum::extract::Path(alert_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("deleted").unwrap().as_bool().unwrap(), true);
        let retrieved = state.alert_manager.get_alert(&alert_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_update_alert_handler_status_change() {
        let state = create_test_server_state().await;
        let alert = Alert::new(
            AlertSeverity::Warning,
            "Update Test".to_string(),
            "Test update".to_string(),
            "test".to_string(),
        );
        let created = state.alert_manager.create_alert(alert).await.unwrap();
        let alert_id = created.id.clone();
        let request = UpdateAlertRequest {
            title: None,
            message: None,
            severity: None,
            status: Some("resolved".to_string()),
        };
        let result = update_alert_handler(
            State(state.clone()),
            axum::extract::Path(alert_id.to_string()),
            Json(request),
        )
        .await;
        assert!(result.is_ok());
        let retrieved = state.alert_manager.get_alert(&alert_id).await.unwrap();
        assert!(matches!(retrieved.status, AlertStatus::Resolved));
    }

    #[tokio::test]
    async fn test_alert_dto_fields() {
        let state = create_test_server_state().await;
        // Create an alert first
        let alert = edge_ai_alerts::Alert::new(
            edge_ai_alerts::AlertSeverity::Warning,
            "Test Alert".to_string(),
            "Test message".to_string(),
            "test".to_string(),
        );
        let alert = state.alert_manager.create_alert(alert).await;
        assert!(alert.is_ok());
        let alert = alert.unwrap();

        let dto = AlertDto {
            id: alert.id.to_string(),
            title: alert.title.clone(),
            message: alert.message.clone(),
            severity: alert.severity.as_str().to_string(),
            status: alert.status.as_str().to_string(),
            acknowledged: matches!(
                alert.status,
                AlertStatus::Acknowledged | AlertStatus::Resolved | AlertStatus::FalsePositive
            ),
            timestamp: alert.timestamp.to_rfc3339(),
        };
        assert!(!dto.id.is_empty());
        assert!(!dto.title.is_empty());
        assert!(!dto.message.is_empty());
        assert!(!dto.severity.is_empty());
        assert!(!dto.status.is_empty());
        assert!(!dto.timestamp.is_empty());
    }
}
