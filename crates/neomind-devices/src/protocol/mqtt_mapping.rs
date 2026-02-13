//! MQTT Protocol Mapping Implementation
//!
//! Maps device capabilities to MQTT topics and handles JSON/binary payload parsing.

use crate::mdl::{MetricDataType, MetricValue};
use crate::protocol::mapping::{
    Address, MappingConfig, MappingError, MappingResult, ProtocolMapping,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::collections::HashMap;

/// MQTT protocol mapping configuration.
#[derive(Debug, Clone)]
pub struct MqttMappingConfig {
    /// Device type this mapping is for
    pub device_type: String,
    /// Metric name -> topic template
    pub metric_topics: HashMap<String, String>,
    /// Command name -> topic template
    pub command_topics: HashMap<String, String>,
    /// Command name -> payload template
    pub payload_templates: HashMap<String, String>,
    /// Metric name -> value extraction config
    pub metric_parsers: HashMap<String, MqttValueParser>,
    /// Default QoS for messages
    pub default_qos: Option<u8>,
    /// Default retain flag
    pub default_retain: Option<bool>,
}

/// How to extract values from MQTT payloads.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum MqttValueParser {
    /// Direct value (payload IS the value)
    #[default]
    Direct,
    /// JSON path extraction
    JsonPath(String),
    /// Binary parser (specific format)
    Binary { format: BinaryFormat },
}

/// Binary data formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryFormat {
    /// Raw bytes (base64 encoded in transport)
    Raw,
    /// Little-endian float32
    Float32Le,
    /// Little-endian float64
    Float64Le,
    /// Little-endian int16
    Int16Le,
    /// Little-endian int32
    Int32Le,
    /// Big-endian float32
    Float32Be,
    /// Big-endian float64
    Float64Be,
    /// Hex string (e.g., "1A2B" → [26, 43])
    HexString,
    /// Base64 encoded hex string
    Base64Hex,
}

impl MqttValueParser {
    /// Create a JSON path parser.
    pub fn json_path(path: impl Into<String>) -> Self {
        Self::JsonPath(path.into())
    }

    /// Create a binary parser.
    pub fn binary(format: BinaryFormat) -> Self {
        Self::Binary { format }
    }
}

/// MQTT protocol mapping implementation.
pub struct MqttMapping {
    config: MqttMappingConfig,
    /// Capability definitions for data type info (reserved for future use).
    #[allow(dead_code)]
    capabilities: HashMap<String, MetricDataType>,
}

impl MqttMapping {
    /// Create a new MQTT mapping from configuration.
    pub fn new(config: MqttMappingConfig) -> Self {
        let capabilities = config
            .metric_topics
            .keys()
            .map(|k| (k.clone(), MetricDataType::Float))
            .collect();

        Self {
            config,
            capabilities,
        }
    }

    /// Create a new MQTT mapping with capability data types.
    pub fn with_capabilities(
        config: MqttMappingConfig,
        capabilities: HashMap<String, MetricDataType>,
    ) -> Self {
        Self {
            config,
            capabilities,
        }
    }

    /// Create from a generic MappingConfig.
    pub fn from_mapping_config(
        base_config: MappingConfig,
        metric_parsers: HashMap<String, MqttValueParser>,
    ) -> Self {
        let mut metric_topics = HashMap::new();
        let mut command_topics = HashMap::new();
        let mut payload_templates = HashMap::new();

        for (metric, config) in &base_config.metric_mappings {
            metric_topics.insert(metric.clone(), config.address_template.clone());
        }

        for (command, config) in &base_config.command_mappings {
            command_topics.insert(command.clone(), config.address_template.clone());
            if let Some(template) = &config.payload_template {
                payload_templates.insert(command.clone(), template.clone());
            }
        }

        let config = MqttMappingConfig {
            device_type: base_config.device_type,
            metric_topics,
            command_topics,
            payload_templates,
            metric_parsers,
            default_qos: None,
            default_retain: None,
        };

        Self::new(config)
    }

    /// Get the MQTT topic for a metric.
    pub fn metric_topic(&self, device_id: &str, capability_name: &str) -> Option<String> {
        self.config
            .metric_topics
            .get(capability_name)
            .map(|template| Self::render_topic(template, device_id))
    }

    /// Get the MQTT topic for a command.
    pub fn command_topic(&self, device_id: &str, command_name: &str) -> Option<String> {
        self.config
            .command_topics
            .get(command_name)
            .map(|template| Self::render_topic(template, device_id))
    }

    /// Render a topic template with device ID.
    fn render_topic(template: &str, device_id: &str) -> String {
        template
            .replace("${device_id}", device_id)
            .replace("${id}", device_id)
    }

    /// Parse a JSON value using JSON path.
    fn parse_json_value(data: &[u8], path: &str) -> MappingResult<MetricValue> {
        let json: serde_json::Value = serde_json::from_slice(data)
            .map_err(|e| MappingError::ParseError(format!("Invalid JSON: {}", e)))?;

        // Handle common patterns
        if path == "$.value" || path == "value" || path == "$" {
            return Self::json_value_to_metric(&json);
        }

        // Simple JSON path extraction
        let path_clean = path.trim_start_matches("$.");
        let parts: Vec<&str> = path_clean.split('.').collect();

        let mut current = &json;
        for part in parts {
            match current {
                serde_json::Value::Object(map) => {
                    current = map.get(part).ok_or_else(|| {
                        MappingError::ParseError(format!("Key '{}' not found in JSON", part))
                    })?;
                }
                _ => {
                    return Err(MappingError::ParseError(format!(
                        "Cannot access '{}' on non-object",
                        part
                    )));
                }
            }
        }

        Self::json_value_to_metric(current)
    }

    /// Convert a JSON value to MetricValue.
    fn json_value_to_metric(value: &serde_json::Value) -> MappingResult<MetricValue> {
        match value {
            serde_json::Value::Null => Ok(MetricValue::Null),
            serde_json::Value::Bool(b) => Ok(MetricValue::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(MetricValue::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(MetricValue::Float(f))
                } else {
                    Err(MappingError::ParseError(
                        "Invalid number format".to_string(),
                    ))
                }
            }
            serde_json::Value::String(s) => Ok(MetricValue::String(s.clone())),
            serde_json::Value::Array(arr) => {
                // Convert array recursively
                let mut result = Vec::new();
                for item in arr {
                    result.push(Self::json_value_to_metric(item)?);
                }
                Ok(MetricValue::Array(result))
            }
            serde_json::Value::Object(_) => {
                // Serialize objects as JSON string
                serde_json::to_string(value)
                    .map(MetricValue::String)
                    .map_err(|e| MappingError::SerializationError(format!("{}", e)))
            }
        }
    }

    /// Parse binary data according to format.
    fn parse_binary_data(data: &[u8], format: &BinaryFormat) -> MappingResult<MetricValue> {
        match format {
            BinaryFormat::Raw => {
                // Base64 decode if it's encoded
                let decoded = BASE64.decode(data).unwrap_or_else(|_| data.to_vec());
                Ok(MetricValue::Binary(decoded))
            }
            BinaryFormat::Float32Le => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for f32".to_string(),
                    ));
                }
                let bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let value = f32::from_le_bytes(bytes);
                Ok(MetricValue::Float(value as f64))
            }
            BinaryFormat::Float64Le => {
                if data.len() < 8 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for f64".to_string(),
                    ));
                }
                let bytes: [u8; 8] = [
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ];
                let value = f64::from_le_bytes(bytes);
                Ok(MetricValue::Float(value))
            }
            BinaryFormat::Int16Le => {
                if data.len() < 2 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for i16".to_string(),
                    ));
                }
                let bytes: [u8; 2] = [data[0], data[1]];
                let value = i16::from_le_bytes(bytes);
                Ok(MetricValue::Integer(value as i64))
            }
            BinaryFormat::Int32Le => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for i32".to_string(),
                    ));
                }
                let bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let value = i32::from_le_bytes(bytes);
                Ok(MetricValue::Integer(value as i64))
            }
            BinaryFormat::Float32Be => {
                if data.len() < 4 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for f32 BE".to_string(),
                    ));
                }
                let bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let value = f32::from_be_bytes(bytes);
                Ok(MetricValue::Float(value as f64))
            }
            BinaryFormat::Float64Be => {
                if data.len() < 8 {
                    return Err(MappingError::ParseError(
                        "Insufficient data for f64 BE".to_string(),
                    ));
                }
                let bytes: [u8; 8] = [
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ];
                let value = f64::from_be_bytes(bytes);
                Ok(MetricValue::Float(value))
            }
            BinaryFormat::HexString => {
                // Parse hex string (e.g., "1A2B" → [26, 43])
                let hex_str = std::str::from_utf8(data)
                    .map_err(|_| MappingError::ParseError("Invalid UTF-8 in hex string".into()))?;

                // Remove optional "0x" prefix and whitespace
                let hex_clean = hex_str
                    .trim()
                    .trim_start_matches("0x")
                    .replace([' ', '\n', '\r', '\t'], "");

                if hex_clean.len() % 2 != 0 {
                    return Err(MappingError::ParseError(
                        "Hex string must have even length".to_string(),
                    ));
                }

                let bytes = (0..hex_clean.len())
                    .step_by(2)
                    .map(|i| {
                        u8::from_str_radix(&hex_clean[i..i + 2], 16).map_err(|_| {
                            MappingError::ParseError(format!(
                                "Invalid hex characters at position {}",
                                i
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(MetricValue::Binary(bytes))
            }
            BinaryFormat::Base64Hex => {
                // First base64 decode, then parse as hex string
                let decoded = BASE64
                    .decode(data)
                    .map_err(|_| MappingError::ParseError("Invalid base64 encoding".into()))?;

                // Recursively parse as hex string
                Self::parse_binary_data(&decoded, &BinaryFormat::HexString)
            }
        }
    }

    /// Render a payload template with parameters.
    fn render_payload_template(
        template: &str,
        params: &HashMap<String, MetricValue>,
    ) -> MappingResult<String> {
        let mut result = template.to_string();

        // Replace ${param} placeholders
        for (key, value) in params {
            let placeholder = format!("${{{}}}", key);
            let replacement = match value {
                MetricValue::String(s) => {
                    // Add quotes if not already present
                    if !s.starts_with('"') && !s.starts_with('{') && !s.starts_with('[') {
                        format!("\"{}\"", s)
                    } else {
                        s.clone()
                    }
                }
                MetricValue::Integer(i) => i.to_string(),
                MetricValue::Float(f) => f.to_string(),
                MetricValue::Boolean(b) => b.to_string(),
                MetricValue::Array(a) => {
                    // Convert array to JSON string representation
                    let json_arr: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            MetricValue::String(s) => format!("\"{}\"", s),
                            MetricValue::Integer(i) => i.to_string(),
                            MetricValue::Float(f) => f.to_string(),
                            MetricValue::Boolean(b) => b.to_string(),
                            _ => "null".to_string(),
                        })
                        .collect();
                    format!("[{}]", json_arr.join(", "))
                }
                MetricValue::Null => "null".to_string(),
                MetricValue::Binary(_) => "\"<binary>\"".to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }

        Ok(result)
    }
}

impl ProtocolMapping for MqttMapping {
    fn protocol_type(&self) -> &'static str {
        "mqtt"
    }

    fn device_type(&self) -> &str {
        &self.config.device_type
    }

    fn metric_address(&self, capability_name: &str) -> Option<Address> {
        self.config
            .metric_topics
            .get(capability_name)
            .map(|topic| Address::MQTT {
                topic: topic.clone(),
                qos: self.config.default_qos,
                retain: self.config.default_retain,
            })
    }

    fn command_address(&self, command_name: &str) -> Option<Address> {
        self.config
            .command_topics
            .get(command_name)
            .map(|topic| Address::MQTT {
                topic: topic.clone(),
                qos: self.config.default_qos,
                retain: self.config.default_retain,
            })
    }

    fn parse_metric(&self, capability_name: &str, raw_data: &[u8]) -> MappingResult<MetricValue> {
        let parser = self
            .config
            .metric_parsers
            .get(capability_name)
            .cloned()
            .unwrap_or_default();

        match parser {
            MqttValueParser::Direct => {
                // Try JSON first, fall back to string
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(raw_data) {
                    Self::json_value_to_metric(&json)
                } else {
                    Ok(MetricValue::String(
                        String::from_utf8_lossy(raw_data).to_string(),
                    ))
                }
            }
            MqttValueParser::JsonPath(ref path) => Self::parse_json_value(raw_data, path),
            MqttValueParser::Binary { format } => Self::parse_binary_data(raw_data, &format),
        }
    }

    fn serialize_command(
        &self,
        command_name: &str,
        params: &HashMap<String, MetricValue>,
    ) -> MappingResult<Vec<u8>> {
        let template = self
            .config
            .payload_templates
            .get(command_name)
            .ok_or_else(|| MappingError::CommandNotFound(command_name.to_string()))?;

        let payload = Self::render_payload_template(template, params)?;

        // Validate it's valid JSON
        if payload.starts_with('{') || payload.starts_with('[') {
            serde_json::from_str::<serde_json::Value>(&payload)
                .map_err(|e| MappingError::SerializationError(format!("Invalid JSON: {}", e)))?;
        }

        Ok(payload.into_bytes())
    }

    fn mapped_capabilities(&self) -> Vec<String> {
        self.config.metric_topics.keys().cloned().collect()
    }

    fn mapped_commands(&self) -> Vec<String> {
        self.config.command_topics.keys().cloned().collect()
    }
}

/// Builder for creating MQTT mappings.
pub struct MqttMappingBuilder {
    device_type: String,
    metric_topics: HashMap<String, String>,
    command_topics: HashMap<String, String>,
    payload_templates: HashMap<String, String>,
    metric_parsers: HashMap<String, MqttValueParser>,
    default_qos: Option<u8>,
    default_retain: Option<bool>,
}

impl MqttMappingBuilder {
    /// Create a new builder for a device type.
    pub fn new(device_type: impl Into<String>) -> Self {
        Self {
            device_type: device_type.into(),
            metric_topics: HashMap::new(),
            command_topics: HashMap::new(),
            payload_templates: HashMap::new(),
            metric_parsers: HashMap::new(),
            default_qos: None,
            default_retain: None,
        }
    }

    /// Add a metric mapping.
    pub fn add_metric(mut self, name: impl Into<String>, topic: impl Into<String>) -> Self {
        self.metric_topics.insert(name.into(), topic.into());
        self
    }

    /// Add a metric mapping with custom parser.
    pub fn add_metric_with_parser(
        mut self,
        name: impl Into<String>,
        topic: impl Into<String>,
        parser: MqttValueParser,
    ) -> Self {
        let name = name.into();
        self.metric_topics.insert(name.clone(), topic.into());
        self.metric_parsers.insert(name, parser);
        self
    }

    /// Add a command mapping.
    pub fn add_command(mut self, name: impl Into<String>, topic: impl Into<String>) -> Self {
        self.command_topics.insert(name.into(), topic.into());
        self
    }

    /// Add a command mapping with payload template.
    pub fn add_command_with_payload(
        mut self,
        name: impl Into<String>,
        topic: impl Into<String>,
        payload_template: impl Into<String>,
    ) -> Self {
        let name = name.into();
        self.command_topics.insert(name.clone(), topic.into());
        self.payload_templates.insert(name, payload_template.into());
        self
    }

    /// Set default QoS.
    pub fn default_qos(mut self, qos: u8) -> Self {
        self.default_qos = Some(qos);
        self
    }

    /// Set default retain.
    pub fn default_retain(mut self, retain: bool) -> Self {
        self.default_retain = Some(retain);
        self
    }

    /// Build the mapping.
    pub fn build(self) -> MqttMapping {
        MqttMapping::new(MqttMappingConfig {
            device_type: self.device_type,
            metric_topics: self.metric_topics,
            command_topics: self.command_topics,
            payload_templates: self.payload_templates,
            metric_parsers: self.metric_parsers,
            default_qos: self.default_qos,
            default_retain: self.default_retain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let mapping = MqttMappingBuilder::new("dht22_sensor")
            .add_metric("temperature", "sensor/${id}/temperature")
            .add_metric("humidity", "sensor/${id}/humidity")
            .add_command_with_payload(
                "set_interval",
                "sensor/${id}/command",
                r#"{"action": "set_interval", "interval": ${interval}}"#,
            )
            .build();

        assert_eq!(mapping.device_type(), "dht22_sensor");
        assert_eq!(mapping.mapped_capabilities().len(), 2);
        assert_eq!(mapping.mapped_commands().len(), 1);
    }

    #[test]
    fn test_metric_topic_resolution() {
        let mapping = MqttMappingBuilder::new("test_device")
            .add_metric("temperature", "sensor/${device_id}/temp")
            .build();

        let topic = mapping.metric_topic("sensor01", "temperature").unwrap();
        assert_eq!(topic, "sensor/sensor01/temp");
    }

    #[test]
    fn test_command_topic_resolution() {
        let mapping = MqttMappingBuilder::new("test_device")
            .add_command("toggle", "relay/${id}/cmd")
            .build();

        let topic = mapping.command_topic("relay01", "toggle").unwrap();
        assert_eq!(topic, "relay/relay01/cmd");
    }

    #[test]
    fn test_parse_json_direct() {
        let mapping = MqttMappingBuilder::new("test")
            .add_metric("value", "test/value")
            .build();

        let data = br#"23.5"#;
        let result = mapping.parse_metric("value", data);
        assert!(matches!(result, Ok(MetricValue::Float(23.5))));
    }

    #[test]
    fn test_parse_json_number() {
        let mapping = MqttMappingBuilder::new("test")
            .add_metric_with_parser("temp", "test", MqttValueParser::json_path("$.temp"))
            .build();

        let data = br#"{"temp": 25.0, "humidity": 60}"#;
        let result = mapping.parse_metric("temp", data);
        assert!(matches!(result, Ok(MetricValue::Float(25.0))));
    }

    #[test]
    fn test_serialize_command_payload() {
        let mapping = MqttMappingBuilder::new("test")
            .add_command_with_payload(
                "set_interval",
                "cmd",
                r#"{"action": "set", "interval": ${interval}}"#,
            )
            .build();

        let mut params = HashMap::new();
        params.insert("interval".to_string(), MetricValue::Integer(60));

        let payload = mapping.serialize_command("set_interval", &params).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&payload),
            r#"{"action": "set", "interval": 60}"#
        );
    }

    #[test]
    fn test_parse_binary_float32_le() {
        let mapping = MqttMappingBuilder::new("test")
            .add_metric_with_parser(
                "value",
                "test",
                MqttValueParser::binary(BinaryFormat::Float32Le),
            )
            .build();

        // 23.5 as f32 little-endian bytes
        let data: [u8; 4] = [0x00, 0x00, 0xBC, 0x41];
        let result = mapping.parse_metric("value", &data);
        assert!(matches!(result, Ok(MetricValue::Float(f)) if (f - 23.5).abs() < 0.01));
    }

    #[test]
    fn test_parse_binary_int16_le() {
        let mapping = MqttMappingBuilder::new("test")
            .add_metric_with_parser(
                "value",
                "test",
                MqttValueParser::binary(BinaryFormat::Int16Le),
            )
            .build();

        // 1000 as i16 little-endian bytes
        let data: [u8; 2] = [0xE8, 0x03];
        let result = mapping.parse_metric("value", &data);
        assert!(matches!(result, Ok(MetricValue::Integer(1000))));
    }
}
