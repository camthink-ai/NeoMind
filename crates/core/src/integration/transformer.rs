//! Transformer trait for data format conversion.
//!
//! Transformers are responsible for converting data between external formats
//! and NeoTalk's internal representation. They handle:
//!
//! - External format → NeoTalk Event
//! - NeoTalk Command → External format
//! - Entity/Device discovery mapping
//! - Data type validation and normalization
//!
//! ## Architecture
//!
//! ```text
//! External Format              Transformer              NeoTalk
//! ┌─────────────┐             ┌─────────────┐          ┌──────────┐
//! │ MQTT JSON   │             │             │  Event   │          │
//! │     payload │────────────▶│  Transformer│──────────▶│ EventBus │
//! │             │             │  - Parsing  │          │          │
//! │             │             │  - Mapping  │  Command │          │
//! │             │◀────────────│  - Format   │◀─────────│  Agent   │
//! └─────────────┘             └─────────────┘          └──────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use edge_ai_core::integration::transformer::{Transformer, TransformationContext, TransformationError};
//! use edge_ai_core::event::NeoTalkEvent;
//!
//! struct MqttTransformer;
//!
//! impl Transformer for MqttTransformer {
//!     fn to_event(&self, data: &[u8], ctx: &TransformationContext) -> Result<NeoTalkEvent, TransformationError> {
//!         // Parse JSON payload
//!         let json: serde_json::Value = serde_json::from_slice(data)
//!             .map_err(|e| TransformationError::ParseError(e.to_string()))?;
//!
//!         // Transform to NeoTalk event
//!         Ok(NeoTalkEvent::Metric {
//!             name: "temperature".to_string(),
//!             value: MetricValue::Number(json["value"].as_f64().unwrap_or(0.0)),
//!             source: ctx.source_system.clone(),
//!             timestamp: ctx.timestamp,
//!         })
//!     }
//!
//!     fn to_external(&self, command: &IntegrationCommand, target_format: &str) -> Result<Vec<u8>, TransformationError> {
//!         // Transform NeoTalk command to external format
//!         match command {
//!             IntegrationCommand::SendData { payload, .. } => Ok(payload.clone()),
//!             _ => Err(TransformationError::UnsupportedCommand(command.to_string())),
//!         }
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result type for transformer operations.
pub type Result<T> = std::result::Result<T, TransformationError>;

/// Transformer error types.
#[derive(Debug, thiserror::Error)]
pub enum TransformationError {
    /// Failed to parse input data.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Failed to serialize output data.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Unsupported data type.
    #[error("Unsupported data type: {0}")]
    UnsupportedType(String),

    /// Invalid data format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// Missing required field.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Type conversion failed.
    #[error("Type conversion failed: {0}")]
    ConversionFailed(String),

    /// Unsupported command.
    #[error("Unsupported command: {0}")]
    UnsupportedCommand(String),

    /// Mapping not found.
    #[error("Mapping not found: {0}")]
    MappingNotFound(String),

    /// Other error.
    #[error("Transformation error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Context for data transformation.
///
/// Provides metadata and context information for transforming
/// external data into NeoTalk events.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransformationContext {
    /// Source system identifier (hass, mqtt, modbus, etc.).
    pub source_system: String,

    /// Source data type (sensor.discovery, metric, etc.).
    pub source_type: String,

    /// Event timestamp.
    pub timestamp: i64,

    /// Additional metadata from the source.
    pub metadata: serde_json::Value,

    /// Entity ID if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,

    /// Topic/path for message-based protocols.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

impl TransformationContext {
    /// Create a new transformation context.
    pub fn new(
        source_system: impl Into<String>,
        source_type: impl Into<String>,
    ) -> Self {
        Self {
            source_system: source_system.into(),
            source_type: source_type.into(),
            timestamp: chrono::Utc::now().timestamp(),
            metadata: serde_json::json!({}),
            entity_id: None,
            topic: None,
        }
    }

    /// Set the entity ID.
    pub fn with_entity_id(mut self, entity_id: impl Into<String>) -> Self {
        self.entity_id = Some(entity_id.into());
        self
    }

    /// Set the topic.
    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    /// Set the timestamp.
    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Get a metadata value by key.
    pub fn get_metadata<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.metadata.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set a metadata value.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert(key.into(), value);
        }
    }
}

impl Default for TransformationContext {
    fn default() -> Self {
        Self {
            source_system: "unknown".to_string(),
            source_type: "unknown".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            metadata: serde_json::json!({}),
            entity_id: None,
            topic: None,
        }
    }
}

/// Entity mapping information.
///
/// Maps external entities to NeoTalk's internal representation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityMapping {
    /// External entity ID.
    pub external_id: String,

    /// Internal entity ID.
    pub internal_id: String,

    /// Entity type (sensor, switch, etc.).
    pub entity_type: String,

    /// Mapping configuration.
    pub config: MappingConfig,

    /// Attribute mappings (external field → internal field).
    pub attribute_map: HashMap<String, String>,

    /// Additional mapping metadata.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Configuration for entity mapping.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct MappingConfig {
    /// Whether to auto-map unknown attributes.
    #[serde(default)]
    pub auto_map: bool,

    /// Value transformation rules.
    #[serde(default)]
    pub value_transforms: Vec<ValueTransform>,

    /// Unit conversions.
    #[serde(default)]
    pub unit_conversions: HashMap<String, UnitConversion>,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            auto_map: true,
            value_transforms: Vec::new(),
            unit_conversions: HashMap::new(),
        }
    }
}

/// Value transformation rule.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct ValueTransform {
    /// Source field path.
    pub source: String,

    /// Target field path.
    pub target: String,

    /// Transformation type.
    #[serde(rename = "type")]
    pub transform_type: TransformType,

    /// Transformation parameters.
    pub params: serde_json::Value,
}

/// Type of value transformation.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub enum TransformType {
    /// Direct mapping.
    Direct,

    /// Scale and offset: value * scale + offset.
    Scale { scale: f64, offset: f64 },

    /// Enum mapping.
    Enum { mapping: HashMap<String, serde_json::Value> },

    /// Format string.
    Format { template: String },

    /// Custom expression (evaluated at runtime).
    Expression { expr: String },
}

/// Unit conversion definition.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct UnitConversion {
    /// Source unit.
    pub from: String,

    /// Target unit.
    pub to: String,

    /// Conversion function.
    pub conversion: ConversionFunction,
}

/// Unit conversion function.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub enum ConversionFunction {
    /// Multiply by factor.
    Scale { factor: f64 },

    /// Add offset.
    Offset { value: f64 },

    /// Custom formula.
    Formula { expr: String },
}

/// Transformer trait for data format conversion.
///
/// Transformers are responsible for converting between external formats
/// and NeoTalk's internal representation.
pub trait Transformer: Send + Sync {
    /// Transform external data to a NeoTalk event.
    ///
    /// # Arguments
    /// * `data` - Raw data bytes from the external system
    /// * `ctx` - Transformation context with metadata
    ///
    /// # Returns
    /// A NeoTalk event if transformation succeeds
    fn to_event(&self, data: &[u8], ctx: &TransformationContext) -> Result<serde_json::Value>;

    /// Transform a NeoTalk command to external format.
    ///
    /// # Arguments
    /// * `command` - Integration command to transform
    /// * `target_format` - Target format identifier
    ///
    /// # Returns
    /// External format as bytes if transformation succeeds
    fn to_external(&self, command: &serde_json::Value, target_format: &str) -> Result<Vec<u8>>;

    /// Validate data format before transformation.
    ///
    /// # Arguments
    /// * `data` - Raw data to validate
    /// * `format_type` - Expected format type
    ///
    /// # Returns
    /// Ok if data is valid, Err with validation error otherwise
    fn validate(&self, data: &[u8], format_type: &str) -> Result<()> {
        // Default implementation: check if data is non-empty
        if data.is_empty() {
            return Err(TransformationError::InvalidFormat("Empty data".to_string()));
        }

        // Try to parse as JSON for most formats
        if format_type.contains("json") || matches!(format_type, "mqtt" | "hass" | "ws") {
            serde_json::from_slice::<serde_json::Value>(data)
                .map_err(|e| TransformationError::ParseError(e.to_string()))?;
        }

        Ok(())
    }

    /// Get supported input formats.
    ///
    /// Returns a list of format identifiers this transformer can handle.
    fn supported_input_formats(&self) -> Vec<String>;

    /// Get supported output formats.
    ///
    /// Returns a list of format identifiers this transformer can produce.
    fn supported_output_formats(&self) -> Vec<String>;

    /// Get entity mappings if supported.
    ///
    /// Returns mappings between external and internal entities.
    fn get_mappings(&self) -> Option<Vec<EntityMapping>> {
        None
    }

    /// Add or update an entity mapping.
    fn add_mapping(&mut self, _mapping: EntityMapping) -> Result<()> {
        Err(TransformationError::Other(anyhow::anyhow!(
            "This transformer does not support entity mappings"
        )))
    }
}

/// Dynamic transformer wrapper for trait objects.
pub type DynTransformer = std::sync::Arc<std::sync::Mutex<dyn Transformer>>;

/// Base transformer with common functionality.
///
/// Provides a foundation for transformer implementations
/// with common validation and format handling.
pub struct BaseTransformer {
    /// Supported input formats.
    input_formats: Vec<String>,

    /// Supported output formats.
    output_formats: Vec<String>,

    /// Entity mappings.
    mappings: HashMap<String, EntityMapping>,
}

impl BaseTransformer {
    /// Create a new base transformer.
    pub fn new(
        input_formats: Vec<String>,
        output_formats: Vec<String>,
    ) -> Self {
        Self {
            input_formats,
            output_formats,
            mappings: HashMap::new(),
        }
    }

    /// Get supported input formats.
    pub fn supported_input_formats(&self) -> Vec<String> {
        self.input_formats.clone()
    }

    /// Get supported output formats.
    pub fn supported_output_formats(&self) -> Vec<String> {
        self.output_formats.clone()
    }

    /// Get entity mappings.
    pub fn get_mappings(&self) -> Option<Vec<EntityMapping>> {
        if self.mappings.is_empty() {
            None
        } else {
            Some(self.mappings.values().cloned().collect())
        }
    }

    /// Add or update an entity mapping.
    pub fn add_mapping(&mut self, mapping: EntityMapping) -> Result<()> {
        self.mappings.insert(mapping.external_id.clone(), mapping);
        Ok(())
    }

    /// Get a mapping by external ID.
    pub fn get_mapping(&self, external_id: &str) -> Option<&EntityMapping> {
        self.mappings.get(external_id)
    }

    /// Remove a mapping.
    pub fn remove_mapping(&mut self, external_id: &str) -> bool {
        self.mappings.remove(external_id).is_some()
    }

    /// Apply value transformation.
    pub fn apply_transform(&self, value: &serde_json::Value, transform: &ValueTransform) -> Result<serde_json::Value> {
        match &transform.transform_type {
            TransformType::Direct => Ok(value.clone()),
            TransformType::Scale { scale, offset } => {
                let num = value.as_f64()
                    .ok_or_else(|| TransformationError::ConversionFailed(
                        "Cannot apply scale to non-numeric value".to_string()
                    ))?;
                Ok(serde_json::json!(num * scale + offset))
            }
            TransformType::Enum { mapping } => {
                let key = value.as_str()
                    .ok_or_else(|| TransformationError::ConversionFailed(
                        "Cannot apply enum mapping to non-string value".to_string()
                    ))?;
                mapping.get(key)
                    .cloned()
                    .ok_or_else(|| TransformationError::MappingNotFound(key.to_string()))
            }
            TransformType::Format { template } => {
                // Simple format with {value} placeholder
                let formatted = template.replace("{value}", &value.to_string());
                Ok(serde_json::json!(formatted))
            }
            TransformType::Expression { .. } => {
                // Expression evaluation would require an expression engine
                // For now, return an error
                Err(TransformationError::Other(anyhow::anyhow!(
                    "Expression transformations not yet implemented"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transformation_context() {
        let ctx = TransformationContext::new("mqtt", "sensor")
            .with_entity_id("sensor.temp")
            .with_topic("sensors/temperature")
            .with_timestamp(1234567890);

        assert_eq!(ctx.source_system, "mqtt");
        assert_eq!(ctx.source_type, "sensor");
        assert_eq!(ctx.entity_id, Some("sensor.temp".to_string()));
        assert_eq!(ctx.topic, Some("sensors/temperature".to_string()));
        assert_eq!(ctx.timestamp, 1234567890);
    }

    #[test]
    fn test_transformation_context_metadata() {
        let mut ctx = TransformationContext::new("hass", "entity");
        ctx.set_metadata("device_id", serde_json::json!("device_123"));

        let device_id: String = ctx.get_metadata("device_id").unwrap();
        assert_eq!(device_id, "device_123");
    }

    #[test]
    fn test_base_transformer() {
        let transformer = BaseTransformer::new(
            vec!["json".to_string(), "mqtt".to_string()],
            vec!["json".to_string()],
        );

        assert_eq!(transformer.supported_input_formats(), vec!["json", "mqtt"]);
        assert_eq!(transformer.supported_output_formats(), vec!["json"]);
    }

    #[test]
    fn test_scale_transform() {
        let transformer = BaseTransformer::new(vec![], vec![]);
        let transform = ValueTransform {
            source: "value".to_string(),
            target: "scaled".to_string(),
            transform_type: TransformType::Scale { scale: 1.8, offset: 32.0 },
            params: serde_json::json!({}),
        };

        let result = transformer.apply_transform(&serde_json::json!(0.0), &transform).unwrap();
        assert_eq!(result, serde_json::json!(32.0));

        let result = transformer.apply_transform(&serde_json::json!(100.0), &transform).unwrap();
        assert_eq!(result, serde_json::json!(212.0));
    }

    #[test]
    fn test_enum_transform() {
        let transformer = BaseTransformer::new(vec![], vec![]);

        let mut mapping = HashMap::new();
        mapping.insert("on".to_string(), serde_json::json!(true));
        mapping.insert("off".to_string(), serde_json::json!(false));

        let transform = ValueTransform {
            source: "state".to_string(),
            target: "power".to_string(),
            transform_type: TransformType::Enum { mapping },
            params: serde_json::json!({}),
        };

        let result = transformer.apply_transform(&serde_json::json!("on"), &transform).unwrap();
        assert_eq!(result, serde_json::json!(true));

        let result = transformer.apply_transform(&serde_json::json!("off"), &transform).unwrap();
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn test_entity_mapping() {
        let mut transformer = BaseTransformer::new(vec![], vec![]);
        let mapping = EntityMapping {
            external_id: "sensor.temp_123".to_string(),
            internal_id: "temperature.living_room".to_string(),
            entity_type: "sensor".to_string(),
            config: MappingConfig::default(),
            attribute_map: {
                let mut map = HashMap::new();
                map.insert("temperature".to_string(), "temp".to_string());
                map.insert("humidity".to_string(), "hum".to_string());
                map
            },
            extra: serde_json::json!({}),
        };

        transformer.add_mapping(mapping).unwrap();
        assert!(transformer.get_mapping("sensor.temp_123").is_some());
        assert!(transformer.get_mappings().is_some());
    }
}
