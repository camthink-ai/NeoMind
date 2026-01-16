//! Tests for Home Assistant integration handlers.

use edge_ai_api::handlers::hass::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_hass_status_handler() {
        let state = create_test_server_state().await;
        let result = get_hass_status_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("status").is_some());
        let status = value.get("status").unwrap();
        assert!(status.get("enabled").is_some());
        assert!(status.get("connected").is_some());
        assert!(status.get("url").is_some());
    }

    #[tokio::test]
    async fn test_connect_hass_handler() {
        let state = create_test_server_state().await;
        let req = HassConnectRequest {
            url: "http://localhost:8123".to_string(),
            token: "test_token".to_string(),
            verify_ssl: false,
            auto_import: false,
        };
        let result = connect_hass_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("settings").is_some());
        let settings = value.get("settings").unwrap();
        assert_eq!(settings.get("enabled").unwrap().as_bool().unwrap(), true);
        assert!(settings.get("url").is_some());
    }

    #[tokio::test]
    async fn test_connect_hass_handler_with_auto_import() {
        let state = create_test_server_state().await;
        let req = HassConnectRequest {
            url: "http://192.168.1.100:8123".to_string(),
            token: "bearer_token_123".to_string(),
            verify_ssl: true,
            auto_import: true,
        };
        let result = connect_hass_handler(State(state), Json(req)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        let settings = value.get("settings").unwrap();
        assert_eq!(settings.get("auto_import").unwrap().as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn test_disconnect_hass_handler() {
        let state = create_test_server_state().await;
        let result = disconnect_hass_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("disconnected").unwrap().as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn test_get_hass_entities_handler_not_connected() {
        let state = create_test_server_state().await;
        let result = get_hass_entities_handler(State(state)).await;
        // Should return error when not connected
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("not connected") || err.status == axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_import_hass_entities_handler_not_connected() {
        let state = create_test_server_state().await;
        let req = HassImportRequest {
            entity_ids: vec!["light.living_room".to_string(), "sensor.temperature".to_string()],
            auto_sync: true,
        };
        let result = import_hass_entities_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("not connected") || err.status == axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_get_hass_devices_handler() {
        let state = create_test_server_state().await;
        let result = get_hass_devices_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("devices").is_some());
        assert!(value.get("count").is_some());
        // Should be empty since no devices are imported
        assert_eq!(value.get("count").unwrap().as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_sync_hass_entity_handler() {
        let state = create_test_server_state().await;
        let entity_id = "sensor.temperature_123";
        let result = sync_hass_entity_handler(State(state), Path(entity_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("entity_id").unwrap().as_str().unwrap(), entity_id);
        assert_eq!(value.get("synced").unwrap().as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn test_remove_hass_device_handler() {
        let state = create_test_server_state().await;
        let entity_id = "light.living_room";
        let result = remove_hass_device_handler(State(state), Path(entity_id.to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("entity_id").unwrap().as_str().unwrap(), entity_id);
        assert_eq!(value.get("removed").unwrap().as_bool().unwrap(), true);
    }

    #[tokio::test]
    async fn test_hass_connect_request() {
        let req = HassConnectRequest {
            url: "https://homeassistant.local:8123".to_string(),
            token: "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test".to_string(),
            verify_ssl: true,
            auto_import: false,
        };
        assert_eq!(req.url, "https://homeassistant.local:8123");
        assert_eq!(req.token, "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test");
        assert_eq!(req.verify_ssl, true);
        assert_eq!(req.auto_import, false);
    }

    #[tokio::test]
    async fn test_hass_import_request() {
        let req = HassImportRequest {
            entity_ids: vec![
                "light.kitchen".to_string(),
                "light.bedroom".to_string(),
                "sensor.temperature".to_string(),
            ],
            auto_sync: true,
        };
        assert_eq!(req.entity_ids.len(), 3);
        assert_eq!(req.entity_ids[0], "light.kitchen");
        assert_eq!(req.auto_sync, true);
    }

    #[tokio::test]
    async fn test_hass_status_dto() {
        let dto = HassStatusDto {
            enabled: true,
            connected: true,
            url: "http://localhost:8123".to_string(),
            last_sync: Some(1234567890),
            entity_count: 15,
        };
        assert_eq!(dto.enabled, true);
        assert_eq!(dto.connected, true);
        assert_eq!(dto.url, "http://localhost:8123");
        assert_eq!(dto.last_sync.unwrap(), 1234567890);
        assert_eq!(dto.entity_count, 15);
    }

    #[tokio::test]
    async fn test_hass_entity_dto() {
        let dto = HassEntityDto {
            entity_id: "sensor.temperature".to_string(),
            state: "22.5".to_string(),
            friendly_name: "Temperature".to_string(),
            domain: "sensor".to_string(),
            device_class: Some("temperature".to_string()),
            unit_of_measurement: Some("°C".to_string()),
        };
        assert_eq!(dto.entity_id, "sensor.temperature");
        assert_eq!(dto.state, "22.5");
        assert_eq!(dto.friendly_name, "Temperature");
        assert_eq!(dto.domain, "sensor");
        assert_eq!(dto.device_class.unwrap(), "temperature");
        assert_eq!(dto.unit_of_measurement.unwrap(), "°C");
    }
}
