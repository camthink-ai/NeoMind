use async_trait::async_trait;
use neomind_extension_sdk::capabilities::CapabilityContext;
use neomind_extension_sdk::{
    neomind_export, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, ParamMetricValue, ParameterDefinition,
    Result,
};
use serde_json::json;
use std::sync::Mutex;

pub struct NativeCapabilitySmokeExtension {
    last_event_result: Mutex<Option<serde_json::Value>>,
}

impl Default for NativeCapabilitySmokeExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeCapabilitySmokeExtension {
    pub fn new() -> Self {
        Self {
            last_event_result: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Extension for NativeCapabilitySmokeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "smoke-test",
                "Smoke Test Extension",
                "0.1.0",
            )
            .with_description("Testing extension for verifying extension system functionality (not for production)")
            .with_author("NeoMind")
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "write_virtual_metric".to_string(),
                display_name: "Write Virtual Metric".to_string(),
                description: "Writes a virtual metric through native capability IPC".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: String::new(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "metric".to_string(),
                        display_name: "Metric".to_string(),
                        description: String::new(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        description: String::new(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: std::collections::HashMap::new(),
                samples: vec![json!({
                    "device_id": "device-1",
                    "metric": "virtual.test.status",
                    "value": "ok",
                })],
                parameter_groups: Vec::new(),
                ai_exposed: true,
            },
            ExtensionCommand {
                name: "get_last_event_result".to_string(),
                display_name: "Get Last Event Result".to_string(),
                description: "Returns the last capability response produced from handle_event"
                    .to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: std::collections::HashMap::new(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
                ai_exposed: true,
            },
        ]
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![MetricDescriptor {
            name: "invocations".to_string(),
            display_name: "Invocations".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        }]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "write_virtual_metric" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("missing device_id".to_string())
                    })?;
                let metric = args
                    .get("metric")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("missing metric".to_string())
                    })?;
                let value = args
                    .get("value")
                    .cloned()
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing value".to_string()))?;

                let context = CapabilityContext::default();
                let response = context.invoke_capability(
                    "device_metrics_write",
                    &json!({
                        "device_id": device_id,
                        "metric": metric,
                        "value": value,
                        "is_virtual": true,
                    }),
                );

                Ok(json!({
                    "capability_response": response,
                }))
            }
            "get_last_event_result" => Ok(json!({
                "last_event_result": self.last_event_result.lock().unwrap().clone(),
            })),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["CapabilitySmokeEvent"]
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        if event_type != "CapabilitySmokeEvent" {
            return Ok(());
        }

        let context = CapabilityContext::default();
        let response = context.invoke_capability(
            "device_metrics_write",
            &json!({
                "device_id": payload.get("device_id").cloned().unwrap_or(json!("unknown-device")),
                "metric": payload.get("metric").cloned().unwrap_or(json!("virtual.test.event")),
                "value": payload.get("value").cloned().unwrap_or(json!("event")),
                "is_virtual": true,
            }),
        );
        *self.last_event_result.lock().unwrap() = Some(response);
        Ok(())
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![ExtensionMetricValue::new(
            "invocations",
            ParamMetricValue::Integer(1),
        )])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_export!(NativeCapabilitySmokeExtension);
