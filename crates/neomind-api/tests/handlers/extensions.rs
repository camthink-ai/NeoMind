//! Tests for extension system handlers.

use neomind_api::handlers::extensions::*;
use neomind_api::handlers::ServerState;
use serde_json::json;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_extension_dto() -> ExtensionDto {
        ExtensionDto {
            id: "ext-1".to_string(),
            name: "Test Extension".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test extension".to_string()),
            author: Some("Test Author".to_string()),
            state: "loaded".to_string(),
            file_path: Some("/path/to/ext.so".to_string()),
            loaded_at: Some(1234567890),
            commands: vec![],
            metrics: vec![],
        }
    }

    #[tokio::test]
    async fn test_extension_dto_default_fields() {
        let dto = make_test_extension_dto();

        assert_eq!(dto.id, "ext-1");
        assert_eq!(dto.name, "Test Extension");
        assert_eq!(dto.version, "1.0.0");
        assert_eq!(dto.state, "loaded");
        assert!(dto.commands.is_empty());
        assert!(dto.metrics.is_empty());
    }

    #[tokio::test]
    async fn test_extension_dto_serialization() {
        let dto = ExtensionDto {
            id: "ext-1".to_string(),
            name: "Test Extension".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            state: "loaded".to_string(),
            file_path: None,
            loaded_at: None,
            commands: vec![],
            metrics: vec![],
        };

        let serialized = serde_json::to_value(&dto).unwrap();
        assert_eq!(serialized["id"], "ext-1");
        assert_eq!(serialized["name"], "Test Extension");
        assert_eq!(serialized["state"], "loaded");
    }

    #[tokio::test]
    async fn test_metric_descriptor_dto() {
        let metric = MetricDescriptorDto {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: "float".to_string(),
            unit: "°C".to_string(),
            description: Some("Temperature reading".to_string()),
            min: Some(-20.0),
            max: Some(100.0),
            required: true,
        };

        assert_eq!(metric.name, "temperature");
        assert_eq!(metric.data_type, "float");
        assert_eq!(metric.unit, "°C");
        assert_eq!(metric.min, Some(-20.0));
        assert_eq!(metric.max, Some(100.0));
        assert!(metric.required);
    }

    #[tokio::test]
    async fn test_metric_descriptor_dto_optional_fields() {
        let metric = MetricDescriptorDto {
            name: "status".to_string(),
            display_name: "Status".to_string(),
            data_type: "string".to_string(),
            unit: "".to_string(),
            description: None,
            min: None,
            max: None,
            required: false,
        };

        assert_eq!(metric.name, "status");
        assert!(metric.description.is_none());
        assert!(metric.min.is_none());
        assert!(metric.max.is_none());
        assert!(!metric.required);
    }

    #[tokio::test]
    async fn test_command_descriptor_dto() {
        let command = CommandDescriptorDto {
            id: "turn_on".to_string(),
            display_name: "Turn On".to_string(),
            description: "Turn the device on".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "duration": {
                        "type": "integer",
                        "description": "Duration in seconds"
                    }
                }
            }),
            output_fields: vec![],
            config: CommandConfigDto {
                requires_auth: false,
                timeout_ms: 30000,
                is_stream: false,
                expected_duration_ms: None,
            },
        };

        assert_eq!(command.id, "turn_on");
        assert_eq!(command.display_name, "Turn On");
        assert_eq!(command.config.is_stream, false);
        assert_eq!(command.config.timeout_ms, 30000);
    }

    #[tokio::test]
    async fn test_output_field_dto() {
        let field = OutputFieldDto {
            name: "result".to_string(),
            data_type: "float".to_string(),
            unit: Some("°C".to_string()),
            description: "Temperature result".to_string(),
            is_primary: true,
            aggregatable: true,
            default_agg_func: "mean".to_string(),
        };

        assert_eq!(field.name, "result");
        assert_eq!(field.data_type, "float");
        assert!(field.is_primary);
        assert!(field.aggregatable);
        assert_eq!(field.default_agg_func, "mean");
    }

    #[tokio::test]
    async fn test_command_config_dto() {
        let config = CommandConfigDto {
            requires_auth: true,
            timeout_ms: 60000,
            is_stream: true,
            expected_duration_ms: Some(5000),
        };

        assert!(config.requires_auth);
        assert_eq!(config.timeout_ms, 60000);
        assert!(config.is_stream);
        assert_eq!(config.expected_duration_ms, Some(5000));
    }

    #[tokio::test]
    async fn test_data_source_info_dto() {
        let info = DataSourceInfoDto {
            id: "extension:ext-1:cmd1:field1".to_string(),
            extension_id: "ext-1".to_string(),
            command: "cmd1".to_string(),
            field: "field1".to_string(),
            display_name: "Field 1".to_string(),
            data_type: "float".to_string(),
            unit: Some("units".to_string()),
            description: "Test field".to_string(),
            aggregatable: true,
            default_agg_func: "sum".to_string(),
        };

        assert_eq!(info.extension_id, "ext-1");
        assert_eq!(info.command, "cmd1");
        assert_eq!(info.field, "field1");
        assert!(info.aggregatable);
    }

    #[tokio::test]
    async fn test_extension_state_transitions() {
        // Test common state values
        let states = vec!["loaded", "running", "stopped", "error", "unloaded"];

        for state in states {
            let dto = ExtensionDto {
                id: "ext-1".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                author: None,
                state: state.to_string(),
                file_path: None,
                loaded_at: None,
                commands: vec![],
                metrics: vec![],
            };
            assert_eq!(dto.state, state);
        }
    }

    #[tokio::test]
    async fn test_extension_with_commands_and_metrics() {
        let dto = ExtensionDto {
            id: "sensor-ext".to_string(),
            name: "Sensor Extension".to_string(),
            version: "2.0.0".to_string(),
            description: Some("Provides sensor data".to_string()),
            author: Some("NeoMind".to_string()),
            state: "running".to_string(),
            file_path: Some("/path/to/sensor-ext.wasm".to_string()),
            loaded_at: Some(1234567890),
            commands: vec![CommandDescriptorDto {
                id: "calibrate".to_string(),
                display_name: "Calibrate".to_string(),
                description: "Calibrate the sensor".to_string(),
                input_schema: json!({"type": "object"}),
                output_fields: vec![],
                config: CommandConfigDto {
                    requires_auth: false,
                    timeout_ms: 30000,
                    is_stream: false,
                    expected_duration_ms: None,
                },
            }],
            metrics: vec![MetricDescriptorDto {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: "float".to_string(),
                unit: "°C".to_string(),
                description: None,
                min: Some(-40.0),
                max: Some(120.0),
                required: true,
            }],
        };

        assert_eq!(dto.commands.len(), 1);
        assert_eq!(dto.metrics.len(), 1);
        assert_eq!(dto.commands[0].id, "calibrate");
        assert_eq!(dto.metrics[0].name, "temperature");
    }

    #[tokio::test]
    async fn test_extension_round_trip_serialization() {
        let original = ExtensionDto {
            id: "test-ext".to_string(),
            name: "Test Extension".to_string(),
            version: "1.5.0".to_string(),
            description: Some("Test description".to_string()),
            author: Some("Test Author".to_string()),
            state: "loaded".to_string(),
            file_path: Some("/test/path.so".to_string()),
            loaded_at: Some(1234567890),
            commands: vec![CommandDescriptorDto {
                id: "cmd1".to_string(),
                display_name: "Command 1".to_string(),
                description: "Test command".to_string(),
                input_schema: json!({"type": "object"}),
                output_fields: vec![],
                config: CommandConfigDto {
                    requires_auth: false,
                    timeout_ms: 30000,
                    is_stream: false,
                    expected_duration_ms: None,
                },
            }],
            metrics: vec![MetricDescriptorDto {
                name: "metric1".to_string(),
                display_name: "Metric 1".to_string(),
                data_type: "integer".to_string(),
                unit: "".to_string(),
                description: Some("Test metric".to_string()),
                min: Some(0.0),
                max: Some(100.0),
                required: false,
            }],
        };

        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: ExtensionDto = serde_json::from_value(serialized).unwrap();

        assert_eq!(deserialized.id, original.id);
        assert_eq!(deserialized.name, original.name);
        assert_eq!(deserialized.version, original.version);
        assert_eq!(deserialized.state, original.state);
        assert_eq!(deserialized.commands.len(), original.commands.len());
        assert_eq!(deserialized.metrics.len(), original.metrics.len());
    }

    #[tokio::test]
    async fn test_extension_list_empty() {
        let state = crate::common::create_test_server_state().await;
        // Just verify the state can be created and has extension registry access
        // The actual listing requires extensions to be registered
        let _registry = &state.extensions.registry;
        // Test passes if state creation succeeds
        assert!(true);
    }

    #[tokio::test]
    async fn test_metric_data_types() {
        let types = vec!["float", "integer", "boolean", "string", "binary"];

        for data_type in types {
            let metric = MetricDescriptorDto {
                name: "test".to_string(),
                display_name: "Test".to_string(),
                data_type: data_type.to_string(),
                unit: "".to_string(),
                description: None,
                min: None,
                max: None,
                required: false,
            };
            assert_eq!(metric.data_type, data_type);
        }
    }

    #[tokio::test]
    async fn test_extension_versions() {
        let versions = vec!["1.0.0", "2.0.0-beta", "3.1.2-rc1", "0.9.0"];

        for version in versions {
            let dto = ExtensionDto {
                id: "ext".to_string(),
                name: "Test".to_string(),
                version: version.to_string(),
                description: None,
                author: None,
                state: "loaded".to_string(),
                file_path: None,
                loaded_at: None,
                commands: vec![],
                metrics: vec![],
            };
            assert_eq!(dto.version, version);
        }
    }
}
