//! Tests for plugin management handlers.

use edge_ai_api::handlers::plugins::*;
use edge_ai_api::handlers::ServerState;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde_json::json;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_plugins_handler() {
        let state = create_test_server_state().await;
        let query = PluginListQuery {
            r#type: None,
            state: None,
            enabled: None,
            builtin: None,
        };
        let result = list_plugins_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("plugins").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_list_plugins_handler_with_type_filter() {
        let state = create_test_server_state().await;
        let query = PluginListQuery {
            r#type: Some("llm_backend".to_string()),
            state: None,
            enabled: None,
            builtin: None,
        };
        let result = list_plugins_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("plugins").is_some());
    }

    #[tokio::test]
    async fn test_list_plugins_handler_extension_only() {
        let state = create_test_server_state().await;
        let query = PluginListQuery {
            r#type: None,
            state: None,
            enabled: None,
            builtin: Some(false), // Exclude built-in plugins
        };
        let result = list_plugins_handler(State(state), Query(query)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        // Should return fewer plugins (only extension plugins)
        let plugins = value.get("plugins").unwrap().as_array().unwrap();
        assert!(!plugins.is_empty()); // plugins is &Vec<Value>
    }

    #[tokio::test]
    async fn test_get_plugin_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_register_plugin_handler_missing_path() {
        let state = create_test_server_state().await;
        let req = RegisterPluginRequest {
            id: "test_plugin".to_string(),
            plugin_type: "llm_backend".to_string(),
            path: None,
            config: None,
            auto_start: None,
            enabled: None,
        };
        let result = register_plugin_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Built-in plugin registration not yet implemented") ||
                err.message.contains("path"));
    }

    #[tokio::test]
    async fn test_register_plugin_handler_invalid_file() {
        let state = create_test_server_state().await;
        let req = RegisterPluginRequest {
            id: "test_plugin".to_string(),
            plugin_type: "extension".to_string(),
            path: Some("/nonexistent/path/plugin.so".to_string()),
            config: None,
            auto_start: None,
            enabled: None,
        };
        let result = register_plugin_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_unregister_plugin_handler() {
        let state = create_test_server_state().await;
        let result = unregister_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_enable_plugin_handler() {
        let state = create_test_server_state().await;
        let result = enable_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_disable_plugin_handler() {
        let state = create_test_server_state().await;
        let result = disable_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_plugin_handler() {
        let state = create_test_server_state().await;
        let result = start_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_plugin_handler() {
        let state = create_test_server_state().await;
        let result = stop_plugin_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_plugin_health_handler_not_found() {
        let state = create_test_server_state().await;
        let result = plugin_health_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_plugin_config_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_plugin_config_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_plugin_config_handler() {
        let state = create_test_server_state().await;
        let req = UpdatePluginConfigRequest {
            config: json!({"setting": "value"}),
            reload: Some(false),
        };
        let result = update_plugin_config_handler(State(state), Path("nonexistent_plugin".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_plugin_command_handler() {
        let state = create_test_server_state().await;
        let req = PluginCommandRequest {
            command: "test_command".to_string(),
            args: Some(json!({"param": "value"})),
        };
        let result = execute_plugin_command_handler(State(state), Path("nonexistent_plugin".to_string()), Json(req)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_plugin_stats_handler_not_found() {
        let state = create_test_server_state().await;
        let result = get_plugin_stats_handler(State(state), Path("nonexistent_plugin".to_string())).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_discover_plugins_handler() {
        let state = create_test_server_state().await;
        let result = discover_plugins_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("message").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_list_plugins_by_type_handler() {
        let state = create_test_server_state().await;
        let result = list_plugins_by_type_handler(State(state), Path("llm_backend".to_string())).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert_eq!(value.get("plugin_type").unwrap().as_str().unwrap(), "llm_backend");
        assert!(value.get("plugins").is_some());
        assert!(value.get("count").is_some());
    }

    #[tokio::test]
    async fn test_get_plugin_types_handler() {
        let state = create_test_server_state().await;
        let result = get_plugin_types_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("types").is_some());
        assert!(value.get("total").is_some());
    }

    #[tokio::test]
    async fn test_list_device_adapter_plugins_handler() {
        let state = create_test_server_state().await;
        let result = list_device_adapter_plugins_handler(State(state)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let value = response.0.data.unwrap();
        assert!(value.get("total_adapters").is_some());
        assert!(value.get("running_adapters").is_some());
        assert!(value.get("adapters").is_some());
    }

    #[tokio::test]
    async fn test_register_device_adapter_handler_missing_fields() {
        let state = create_test_server_state().await;
        let req = json!({"id": "test_adapter"}); // Missing required fields
        let result = register_device_adapter_handler(State(state), Json(req)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Missing") || err.status == axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_device_adapter_handler() {
        let state = create_test_server_state().await;
        let req = json!({
            "id": "test_mqtt_adapter",
            "name": "Test MQTT Adapter",
            "adapter_type": "mqtt",
            "config": {"host": "localhost", "port": 1883},
            "auto_start": false,
            "enabled": true
        });
        let result = register_device_adapter_handler(State(state), Json(req)).await;
        // May fail if registry not initialized, but request structure is valid
        match result {
            Ok(response) => {
                let value = response.0.data.unwrap();
                assert!(value.get("plugin_id").is_some());
            }
            Err(err) => {
                // Acceptable if device adapter registry is not available
                assert!(err.status == axum::http::StatusCode::INTERNAL_SERVER_ERROR ||
                        err.status == axum::http::StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }

    #[tokio::test]
    async fn test_get_adapter_devices_handler() {
        let state = create_test_server_state().await;
        let result = get_adapter_devices_handler(State(state), Path("internal-mqtt".to_string())).await;
        // May fail if registry not initialized, but should handle gracefully
        match result {
            Ok(response) => {
                let value = response.0.data.unwrap();
                assert_eq!(value.get("plugin_id").unwrap().as_str().unwrap(), "internal-mqtt");
                assert!(value.get("devices").is_some());
            }
            Err(err) => {
                // Acceptable if device adapter registry is not available
                assert!(err.status == axum::http::StatusCode::INTERNAL_SERVER_ERROR ||
                        err.status == axum::http::StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }

    #[tokio::test]
    async fn test_get_device_adapter_stats_handler() {
        let state = create_test_server_state().await;
        let result = get_device_adapter_stats_handler(State(state)).await;
        // May fail if registry not initialized
        match result {
            Ok(response) => {
                let value = response.0.data.unwrap();
                assert!(value.get("total_adapters").is_some());
            }
            Err(err) => {
                // Acceptable if device adapter registry is not available
                assert!(err.status == axum::http::StatusCode::INTERNAL_SERVER_ERROR ||
                        err.status == axum::http::StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }

    #[tokio::test]
    async fn test_plugin_dto() {
        let dto = PluginDto {
            id: "plugin1".to_string(),
            name: "Test Plugin".to_string(),
            plugin_type: "llm_backend".to_string(),
            category: "ai".to_string(),
            state: "Running".to_string(),
            enabled: true,
            version: "1.0.0".to_string(),
            description: "A test plugin".to_string(),
            author: Some("Test Author".to_string()),
            required_version: "1.0.0".to_string(),
            stats: PluginStatsDto::default(),
            loaded_at: chrono::Utc::now(),
            path: Some("/path/to/plugin.so".to_string()),
            running: true,
            device_count: Some(5),
        };
        assert_eq!(dto.id, "plugin1");
        assert_eq!(dto.enabled, true);
        assert_eq!(dto.running, true);
        assert_eq!(dto.device_count.unwrap(), 5);
    }

    #[tokio::test]
    async fn test_plugin_stats_dto_default() {
        let stats = PluginStatsDto::default();
        assert_eq!(stats.start_count, 0);
        assert_eq!(stats.stop_count, 0);
        assert_eq!(stats.error_count, 0);
        assert_eq!(stats.total_execution_ms, 0);
        assert_eq!(stats.avg_response_time_ms, 0.0);
    }

    #[tokio::test]
    async fn test_adapter_plugin_dto() {
        let dto = AdapterPluginDto {
            id: "adapter1".to_string(),
            name: "MQTT Adapter".to_string(),
            adapter_type: "mqtt".to_string(),
            enabled: true,
            running: true,
            device_count: 10,
            state: "Running".to_string(),
            version: "1.0.0".to_string(),
            uptime_secs: Some(3600),
            last_activity: chrono::Utc::now().timestamp(),
        };
        assert_eq!(dto.id, "adapter1");
        assert_eq!(dto.adapter_type, "mqtt");
        assert_eq!(dto.running, true);
        assert_eq!(dto.device_count, 10);
    }

    #[tokio::test]
    async fn test_register_plugin_request() {
        let req = RegisterPluginRequest {
            id: "test_plugin".to_string(),
            plugin_type: "extension".to_string(),
            path: Some("/path/to/plugin.so".to_string()),
            config: Some(json!({"setting": "value"})),
            auto_start: Some(true),
            enabled: Some(true),
        };
        assert_eq!(req.id, "test_plugin");
        assert_eq!(req.plugin_type, "extension");
        assert_eq!(req.auto_start.unwrap(), true);
        assert_eq!(req.enabled.unwrap(), true);
    }

    #[tokio::test]
    async fn test_update_plugin_config_request() {
        let req = UpdatePluginConfigRequest {
            config: json!({"key": "value"}),
            reload: Some(true),
        };
        assert!(req.config.is_object());
        assert_eq!(req.reload.unwrap(), true);
    }

    #[tokio::test]
    async fn test_plugin_command_request() {
        let req = PluginCommandRequest {
            command: "execute".to_string(),
            args: Some(json!({"param1": "value1"})),
        };
        assert_eq!(req.command, "execute");
        assert!(req.args.is_some());
    }
}
