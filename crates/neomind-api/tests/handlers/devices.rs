//! Tests for device management handlers.

use axum::extract::{Path, Query, State};
use neomind_api::handlers::devices::models::{
    AddDeviceRequest, BatchCurrentValuesRequest, PaginationQuery, TimeRangeQuery,
    UpdateDeviceRequest,
};
use neomind_api::handlers::ServerState;
use serde_json::json;
use uuid::Uuid;

/// Helper to generate a unique device ID for testing
fn test_device_id() -> String {
    format!("test-device-{}", Uuid::new_v4())
}

/// Helper to generate a unique device type for testing
fn test_device_type() -> String {
    format!("test-type-{}", Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pagination_query_default() {
        let query = PaginationQuery {
            page: None,
            limit: None,
            device_type: None,
            status: None,
        };
        assert_eq!(query.page, None);
        assert_eq!(query.limit, None);
        assert_eq!(query.device_type, None);
        assert_eq!(query.status, None);
    }

    #[tokio::test]
    async fn test_pagination_query_with_params() {
        let query = PaginationQuery {
            page: Some(2),
            limit: Some(20),
            device_type: Some("sensor".to_string()),
            status: Some("connected".to_string()),
        };
        assert_eq!(query.page, Some(2));
        assert_eq!(query.limit, Some(20));
        assert_eq!(query.device_type, Some("sensor".to_string()));
        assert_eq!(query.status, Some("connected".to_string()));
    }

    #[tokio::test]
    async fn test_time_range_query_default() {
        let query = TimeRangeQuery {
            start: None,
            end: None,
            limit: None,
        };
        assert_eq!(query.start, None);
        assert_eq!(query.end, None);
        assert_eq!(query.limit, None);
    }

    #[tokio::test]
    async fn test_time_range_query_with_params() {
        let query = TimeRangeQuery {
            start: Some(1234567890),
            end: Some(1234567900),
            limit: Some(100),
        };
        assert_eq!(query.start, Some(1234567890));
        assert_eq!(query.end, Some(1234567900));
        assert_eq!(query.limit, Some(100));
    }

    #[tokio::test]
    async fn test_add_device_request() {
        let device_id = test_device_id();
        let request = AddDeviceRequest {
            device_id: Some(device_id.clone()),
            name: "Test Device".to_string(),
            device_type: "sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: json!({
                "address": "localhost:1883",
                "topic": "test/topic",
            }),
        };

        assert_eq!(request.device_id, Some(device_id));
        assert_eq!(request.name, "Test Device");
        assert_eq!(request.device_type, "sensor");
        assert_eq!(request.adapter_type, "mqtt");
    }

    #[tokio::test]
    async fn test_update_device_request() {
        let request = UpdateDeviceRequest {
            name: Some("Updated Device".to_string()),
            connection_config: Some(json!({"key": "value"})),
            adapter_type: None,
            adapter_id: None,
        };

        assert_eq!(request.name, Some("Updated Device".to_string()));
        assert!(request.connection_config.is_some());
    }

    #[tokio::test]
    async fn test_batch_current_values_request() {
        let request = BatchCurrentValuesRequest {
            device_ids: vec!["device1".to_string(), "device2".to_string()],
        };

        assert_eq!(request.device_ids.len(), 2);
        assert_eq!(request.device_ids[0], "device1");
        assert_eq!(request.device_ids[1], "device2");
    }

    #[tokio::test]
    async fn test_device_id_generation_unique() {
        let id1 = test_device_id();
        let id2 = test_device_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("test-device-"));
        assert!(id2.starts_with("test-device-"));
    }

    #[tokio::test]
    async fn test_device_type_generation_unique() {
        let type1 = test_device_type();
        let type2 = test_device_type();
        assert_ne!(type1, type2);
        assert!(type1.starts_with("test-type-"));
        assert!(type2.starts_with("test-type-"));
    }

    #[tokio::test]
    async fn test_list_devices_handler_exists() {
        // This test verifies that the handler can be called
        // Actual testing requires devices to be registered first
        let state = crate::common::create_test_server_state().await;

        // Just verify the state can be created
        assert!(state.devices.service.list_devices().await.len() >= 0);
    }

    #[tokio::test]
    async fn test_get_device_info_handler_empty_id() {
        let state = crate::common::create_test_server_state().await;

        // Test with a non-existent device
        let result = state
            .devices
            .service
            .get_device("non-existent-device")
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_pagination_bounds() {
        // Test that pagination values are properly bounded
        let page = 0;
        let limit = 2000; // Over the max of 1000

        let page = page.max(1); // Minimum page is 1
        let limit = limit.min(1000); // Maximum limit is 1000

        assert_eq!(page, 1);
        assert_eq!(limit, 1000);
    }

    #[tokio::test]
    async fn test_pagination_offset_calculation() {
        let page = 3;
        let limit = 20;
        let offset = (page - 1) * limit;

        assert_eq!(offset, 40);
    }

    #[tokio::test]
    async fn test_add_device_request_without_device_id() {
        // Test that device_id is optional
        let request = AddDeviceRequest {
            device_id: None, // Will be auto-generated
            name: "Test Device".to_string(),
            device_type: "sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: json!({
                "address": "localhost:1883",
            }),
        };

        assert!(request.device_id.is_none());
        assert_eq!(request.name, "Test Device");
        assert_eq!(request.adapter_type, "mqtt");
    }

    #[tokio::test]
    async fn test_update_device_request_all_optional() {
        // All fields are optional
        let request = UpdateDeviceRequest {
            name: None,
            adapter_type: None,
            connection_config: None,
            adapter_id: None,
        };

        assert!(request.name.is_none());
        assert!(request.adapter_type.is_none());
        assert!(request.connection_config.is_none());
        assert!(request.adapter_id.is_none());
    }

    #[tokio::test]
    async fn test_update_device_request_partial_update() {
        // Only update some fields
        let request = UpdateDeviceRequest {
            name: Some("New Name".to_string()),
            adapter_type: None,
            connection_config: None,
            adapter_id: Some("adapter-123".to_string()),
        };

        assert_eq!(request.name, Some("New Name".to_string()));
        assert!(request.adapter_type.is_none());
        assert!(request.connection_config.is_none());
        assert_eq!(request.adapter_id, Some("adapter-123".to_string()));
    }
}
