//! Shared types for device data discovery and AI-powered analysis.
//!
//! This module contains types used across the discovery subsystem for:
//! - Path extraction from raw data samples
//! - Semantic inference of field meanings
//! - Virtual metric generation
//! - Device type auto-generation

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result type for discovery operations
pub type Result<T> = std::result::Result<T, DiscoveryError>;

/// Errors that can occur during discovery
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Insufficient data: {0}")]
    InsufficientData(String),

    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// A device data sample for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSample {
    /// Raw data bytes
    pub raw_data: Vec<u8>,
    /// Parsed JSON (if successfully parsed)
    pub parsed: Option<serde_json::Value>,
    /// Source description (e.g., "MQTT: topic/sensor/001")
    pub source: String,
    /// Timestamp of sample
    #[serde(default)]
    pub timestamp: i64,
}

impl DeviceSample {
    /// Create a new sample from raw data
    pub fn from_raw(raw_data: Vec<u8>, source: impl Into<String>) -> Self {
        let source = source.into();
        let parsed = serde_json::from_slice(&raw_data).ok();
        Self {
            raw_data,
            parsed,
            source,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a new sample from JSON value
    pub fn from_json(value: serde_json::Value, source: impl Into<String>) -> Self {
        let source = source.into();
        let raw_data = serde_json::to_vec(&value).unwrap_or_default();
        Self {
            raw_data,
            parsed: Some(value),
            source,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Check if this sample contains valid JSON
    pub fn is_json(&self) -> bool {
        self.parsed.is_some()
    }
}

/// A discovered data path from samples
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPath {
    /// Extracted path (e.g., "payload.sensors[0].v" or "payload.sensors[].v" for array patterns)
    pub path: String,
    /// Data type at this path
    pub data_type: DataType,
    /// Whether this path exists in all samples
    pub is_consistent: bool,
    /// Percentage of samples containing this path
    pub coverage: f32,
    /// Sample values found at this path
    #[serde(default)]
    pub sample_values: Vec<serde_json::Value>,
    /// Value range (for numeric types)
    pub value_range: Option<ValueRange>,
    /// Array if path contains arrays
    pub is_array: bool,
    /// Object if path contains objects
    pub is_object: bool,
    /// If true, this path is an array pattern (e.g., "detections[].class_name")
    #[serde(default)]
    pub is_array_pattern: bool,
    /// For array patterns, the array field name (e.g., "detections" for "detections[].class_name")
    #[serde(default)]
    pub array_name: Option<String>,
    /// For array patterns, the inferred maximum array length
    #[serde(default)]
    pub inferred_array_length: Option<usize>,
}

impl DiscoveredPath {
    /// Normalize array paths by converting numeric indices to [] pattern
    /// e.g., "detections.0.class_name" -> "detections[].class_name"
    ///      "data.sensors.1.value" -> "data.sensors[].value"
    pub fn normalize_array_path(path: &str) -> (String, Option<String>, Option<usize>) {
        let parts: Vec<&str> = path.split('.').collect();
        let mut normalized_parts = Vec::new();
        let mut array_name: Option<String> = None;
        let mut array_index: Option<usize> = None;

        for (i, part) in parts.iter().enumerate() {
            // Check if this part is a numeric array index
            if let Ok(idx) = part.parse::<usize>() {
                // This is an array index - replace with []
                // The array name is the previous part
                if i > 0 {
                    array_name = Some(parts[i - 1].to_string());
                    array_index = Some(idx);
                }
                normalized_parts.push("[]".to_string());
            } else {
                normalized_parts.push(part.to_string());
            }
        }

        let normalized = normalized_parts.join(".");
        (normalized, array_name, array_index)
    }

    /// Check if this is an array index pattern (numeric part in path)
    pub fn contains_array_index(path: &str) -> bool {
        path.split('.').any(|part| part.parse::<usize>().is_ok())
    }

    /// Extract array name from a path with array indices
    /// e.g., "detections.0.class_name" -> Some("detections")
    ///      "data.sensors.1.value" -> Some("sensors")
    pub fn extract_array_name(path: &str) -> Option<String> {
        let parts: Vec<&str> = path.split('.').collect();
        for (i, part) in parts.iter().enumerate() {
            if part.parse::<usize>().is_ok() && i > 0 {
                return Some(parts[i - 1].to_string());
            }
        }
        None
    }

    /// Aggregate multiple paths that are array variations into a single pattern
    /// e.g., ["detections.0.class_name", "detections.1.class_name"]
    ///      -> DiscoveredPath { path: "detections[].class_name", inferred_array_length: 2 }
    pub fn aggregate_array_pattern(paths: Vec<DiscoveredPath>) -> Option<DiscoveredPath> {
        if paths.is_empty() {
            return None;
        }

        // Normalize all paths to find common pattern
        let normalized_info: Vec<_> = paths.iter()
            .map(|p| Self::normalize_array_path(&p.path))
            .collect();

        // Check if all normalize to the same pattern
        let first_normalized = &normalized_info[0].0;
        if !normalized_info.iter().all(|(norm, _, _)| norm == first_normalized) {
            return None; // Not the same pattern
        }

        // Find max array index
        let max_length = normalized_info.iter()
            .filter_map(|(_, _, idx)| *idx)
            .map(|idx| idx + 1) // Convert 0-based index to length
            .max();

        let first = &paths[0];
        Some(DiscoveredPath {
            path: first_normalized.clone(),
            is_array_pattern: true,
            array_name: normalized_info[0].1.clone(),
            inferred_array_length: max_length,
            // Merge other properties from the first path
            data_type: first.data_type.clone(),
            is_consistent: first.is_consistent,
            coverage: first.coverage,
            sample_values: first.sample_values.clone(),
            value_range: first.value_range.clone(),
            is_array: true,
            is_object: first.is_object,
        })
    }

    /// Create a pattern path from a concrete path
    /// e.g., "detections.0.class_name" -> "detections[].class_name"
    pub fn as_pattern(&self) -> DiscoveredPath {
        let (normalized_path, array_name, array_index) = Self::normalize_array_path(&self.path);
        DiscoveredPath {
            path: normalized_path,
            is_array_pattern: self.is_array || array_name.is_some(),
            array_name,
            inferred_array_length: array_index.map(|i| i + 1),
            data_type: self.data_type.clone(),
            is_consistent: self.is_consistent,
            coverage: self.coverage,
            sample_values: self.sample_values.clone(),
            value_range: self.value_range.clone(),
            is_array: self.is_array,
            is_object: self.is_object,
        }
    }
}

/// Data type inferred from values
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    /// Null/missing value
    Null,
    /// Boolean true/false
    Boolean,
    /// Integer number
    Integer,
    /// Floating point number
    Float,
    /// String value
    String,
    /// Array/list
    Array,
    /// Object/map
    Object,
    /// Binary data
    Binary,
    /// Unknown type
    Unknown,
}

impl DataType {
    /// Display name for the data type
    pub fn display_name(&self) -> &'static str {
        match self {
            DataType::Null => "null",
            DataType::Boolean => "boolean",
            DataType::Integer => "integer",
            DataType::Float => "float",
            DataType::String => "string",
            DataType::Array => "array",
            DataType::Object => "object",
            DataType::Binary => "binary",
            DataType::Unknown => "unknown",
        }
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, DataType::Integer | DataType::Float)
    }

    /// Infer type from a JSON value
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => DataType::Null,
            serde_json::Value::Bool(_) => DataType::Boolean,
            serde_json::Value::Number(n) => {
                if n.is_i64() {
                    DataType::Integer
                } else {
                    DataType::Float
                }
            }
            serde_json::Value::String(_) => DataType::String,
            serde_json::Value::Array(_) => DataType::Array,
            serde_json::Value::Object(_) => DataType::Object,
        }
    }
}

/// Value range for numeric data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueRange {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Average value
    pub avg: f64,
    /// Standard deviation
    pub std_dev: Option<f64>,
    /// Sample count
    pub count: usize,
}

impl ValueRange {
    /// Create a new value range from samples
    pub fn from_values(values: &[f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }

        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let sum: f64 = values.iter().sum();
        let avg = sum / values.len() as f64;

        let std_dev = if values.len() > 1 {
            let variance = values.iter()
                .map(|&x| (x - avg).powi(2))
                .sum::<f64>() / (values.len() - 1) as f64;
            Some(variance.sqrt())
        } else {
            None
        };

        Some(Self {
            min,
            max,
            avg,
            std_dev,
            count: values.len(),
        })
    }
}

/// Path validation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathValidity {
    /// Path is valid and accessible
    Valid,
    /// Path exists but value is null
    NullValue,
    /// Path does not exist in some samples
    Inconsistent,
    /// Path is invalid syntax
    Invalid,
    /// Path references non-existent field
    NotFound,
}

/// Semantic type inferred from field context
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticType {
    /// Temperature measurement
    Temperature,
    /// Humidity measurement
    Humidity,
    /// Pressure measurement
    Pressure,
    /// Light level
    Light,
    /// Motion detection
    Motion,
    /// Switch/on-off state
    Switch,
    /// Dimmer level
    Dimmer,
    /// Color value
    Color,
    /// Power consumption
    Power,
    /// Energy consumption
    Energy,
    /// CO2 level
    Co2,
    /// PM2.5 level
    Pm25,
    /// VOC level
    Voc,
    /// Speed/RPM
    Speed,
    /// Flow rate
    Flow,
    /// Liquid level
    Level,
    /// Generic status
    Status,
    /// Error/fault
    Error,
    /// Alarm/alert
    Alarm,
    /// Battery level
    Battery,
    /// Signal strength (RSSI)
    Rssi,
    /// Unknown semantic type
    Unknown,
}

impl SemanticType {
    /// Get display name for this semantic type
    pub fn display_name(&self) -> &'static str {
        match self {
            SemanticType::Temperature => "温度",
            SemanticType::Humidity => "湿度",
            SemanticType::Pressure => "压力",
            SemanticType::Light => "光照",
            SemanticType::Motion => "运动",
            SemanticType::Switch => "开关",
            SemanticType::Dimmer => "调光",
            SemanticType::Color => "颜色",
            SemanticType::Power => "功率",
            SemanticType::Energy => "能耗",
            SemanticType::Co2 => "二氧化碳",
            SemanticType::Pm25 => "PM2.5",
            SemanticType::Voc => "VOC",
            SemanticType::Speed => "速度",
            SemanticType::Flow => "流量",
            SemanticType::Level => "液位",
            SemanticType::Status => "状态",
            SemanticType::Error => "错误",
            SemanticType::Alarm => "告警",
            SemanticType::Battery => "电池",
            SemanticType::Rssi => "信号强度",
            SemanticType::Unknown => "未知",
        }
    }

    /// Get default unit for this semantic type
    pub fn default_unit(&self) -> Option<&'static str> {
        match self {
            SemanticType::Temperature => Some("°C"),
            SemanticType::Humidity => Some("%"),
            SemanticType::Pressure => Some("kPa"),
            SemanticType::Light => Some("lux"),
            SemanticType::Power => Some("W"),
            SemanticType::Energy => Some("kWh"),
            SemanticType::Co2 => Some("ppm"),
            SemanticType::Pm25 => Some("μg/m³"),
            SemanticType::Voc => Some("ppb"),
            SemanticType::Speed => Some("RPM"),
            SemanticType::Flow => Some("L/min"),
            SemanticType::Level => Some("cm"),
            SemanticType::Battery => Some("%"),
            SemanticType::Rssi => Some("dBm"),
            _ => None,
        }
    }

    /// Try to infer semantic type from field name and value
    pub fn infer_from_context(field_name: &str, value: &Option<serde_json::Value>) -> Self {
        let name_lower = field_name.to_lowercase();

        // For nested fields like "detections.0.class_name", also check the last segment
        let last_segment = name_lower.split('.').last().unwrap_or(&name_lower);

        // Check field name patterns - prioritize more specific patterns first

        // Temperature
        if name_lower.contains("temp") || name_lower.contains("温度") {
            return SemanticType::Temperature;
        }

        // Humidity
        if name_lower.contains("hum") || name_lower.contains("湿度") {
            return SemanticType::Humidity;
        }

        // Pressure
        if name_lower.contains("press") || name_lower.contains("压力") {
            return SemanticType::Pressure;
        }

        // Light
        if name_lower.contains("light") || name_lower.contains("lux") || name_lower.contains("光照") {
            return SemanticType::Light;
        }

        // Motion
        if name_lower.contains("motion") || name_lower.contains("pir") || name_lower.contains("移动") {
            return SemanticType::Motion;
        }

        // Switch
        if name_lower.contains("switch") || (name_lower.contains("power") && name_lower.contains("state")) {
            return SemanticType::Switch;
        }

        // Dimmer/Brightness
        if name_lower.contains("dimmer") || name_lower.contains("brightness") {
            return SemanticType::Dimmer;
        }

        // Color
        if name_lower.contains("color") || name_lower.contains("rgb") {
            return SemanticType::Color;
        }

        // Battery
        if name_lower.contains("battery") || name_lower.contains("batt") {
            return SemanticType::Battery;
        }

        // Power
        if name_lower == "power" || name_lower.contains("power") {
            return SemanticType::Power;
        }

        // Energy
        if name_lower.contains("energy") || name_lower.contains("kwh") || name_lower.contains("能耗") {
            return SemanticType::Energy;
        }

        // Speed
        if name_lower.contains("speed") || name_lower.contains("rpm") || name_lower.contains("velocity") {
            return SemanticType::Speed;
        }

        // Flow
        if name_lower.contains("flow") || name_lower.contains("rate") {
            return SemanticType::Flow;
        }

        // Level
        if name_lower.contains("level") || name_lower.contains("液位") {
            return SemanticType::Level;
        }

        // RSSI/Signal
        if name_lower.contains("rssi") || name_lower.contains("signal") || name_lower.contains("snr") {
            return SemanticType::Rssi;
        }

        // Status
        if name_lower == "status" || name_lower == "state" || name_lower.contains("状态") {
            return SemanticType::Status;
        }

        // Error
        if name_lower == "error" || name_lower.contains("fault") || name_lower.contains("错误") {
            return SemanticType::Error;
        }

        // Alarm
        if name_lower.contains("alarm") || name_lower.contains("alert") || name_lower.contains("告警") {
            return SemanticType::Alarm;
        }

        // Count / detection_count
        if name_lower.contains("count") || name_lower.ends_with("_count") {
            return SemanticType::Status; // Count is a status metric
        }

        // Width/Height (image dimensions) - check last segment for nested fields
        if last_segment == "width" || last_segment == "height" || name_lower.ends_with(".width") || name_lower.ends_with(".height") {
            return SemanticType::Status;
        }

        // Size - check last segment
        if last_segment == "size" || name_lower.ends_with(".size") {
            return SemanticType::Status;
        }

        // Timestamp
        if name_lower.contains("time") || name_lower.contains("timestamp") {
            return SemanticType::Status;
        }

        // Percentage (explicit)
        if name_lower.contains("percent") || name_lower.contains("%") {
            return SemanticType::Battery; // Often battery or generic percentage
        }

        // Identifier fields
        if name_lower.contains("id") || name_lower.contains("identifier") {
            return SemanticType::Status;
        }

        // Name
        if name_lower.contains("name") {
            return SemanticType::Status;
        }

        // Version
        if name_lower.contains("version") || name_lower.contains("ver") {
            return SemanticType::Status;
        }

        // Quality - check last segment
        if last_segment == "quality" || name_lower.ends_with(".quality") {
            return SemanticType::Status;
        }

        // Format - check last segment
        if last_segment == "format" || name_lower.ends_with(".format") {
            return SemanticType::Status;
        }

        // Type - prefer exact match, but allow device_type, encoding_type etc.
        if last_segment == "type" || name_lower.ends_with("_type") || name_lower.ends_with(".type") {
            return SemanticType::Status;
        }

        // Confidence
        if name_lower.contains("confidence") || name_lower.contains("conf") {
            return SemanticType::Status;
        }

        // Threshold
        if name_lower.contains("threshold") || name_lower.contains("thresh") {
            return SemanticType::Status;
        }

        // Index - check last segment for nested fields
        if last_segment == "index" || name_lower.ends_with(".index") {
            return SemanticType::Status;
        }

        // Class / class_name
        if name_lower.contains("class") {
            return SemanticType::Status;
        }

        // X/Y coordinates - check last segment for nested fields
        if last_segment == "x" || last_segment == "y" || name_lower.ends_with(".x") || name_lower.ends_with(".y") {
            return SemanticType::Status;
        }

        // Latency / inference time
        if name_lower.contains("latency") || name_lower.contains("inference_time") || name_lower.contains("delay") {
            return SemanticType::Speed;
        }

        // Try to infer from value range
        if let Some(v) = value
            && let Some(n) = v.as_f64() {
                // Temperature range check (typical: -40 to 100)
                if (-50.0..=150.0).contains(&n) {
                    // Could be temperature, but need more context
                }
                // Humidity range check (0-100)
                if (0.0..=100.0).contains(&n) {
                    // Could be humidity or percentage
                }
            }

        SemanticType::Unknown
    }
}

/// Field semantic inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSemantic {
    /// Inferred semantic type
    pub semantic_type: SemanticType,
    /// Standardized name (e.g., "temperature", "humidity")
    pub standard_name: String,
    /// Display name in user's language
    pub display_name: String,
    /// Recommended unit
    pub recommended_unit: Option<String>,
    /// Confidence score (0-1)
    pub confidence: f32,
    /// Reasoning for the inference
    pub reasoning: String,
    /// Field name(s) that led to this inference
    pub source_fields: Vec<String>,
}

impl FieldSemantic {
    /// Create a new field semantic
    pub fn new(
        semantic_type: SemanticType,
        standard_name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        let recommended_unit = semantic_type.default_unit().map(|u| u.to_string());
        Self {
            semantic_type,
            standard_name: standard_name.into(),
            display_name: display_name.into(),
            recommended_unit,
            confidence: 0.5,
            reasoning: String::new(),
            source_fields: Vec::new(),
        }
    }

    /// Set confidence level
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Set reasoning
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = reasoning.into();
        self
    }

    /// Add source field
    pub fn with_source_field(mut self, field: impl Into<String>) -> Self {
        self.source_fields.push(field.into());
        self
    }
}

/// Discovered metric that can be used as a virtual metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredMetric {
    /// Metric name (standardized)
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Path to extract value (JSONPath or dot notation)
    pub path: String,
    /// Data type
    pub data_type: DataType,
    /// Semantic type
    pub semantic_type: SemanticType,
    /// Unit
    pub unit: Option<String>,
    /// Value range (if numeric)
    pub value_range: Option<ValueRange>,
    /// Whether readable
    pub is_readable: bool,
    /// Whether writable
    pub is_writable: bool,
    /// Confidence in this discovery (0-1)
    pub confidence: f32,
}

impl Default for DiscoveredMetric {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            description: String::new(),
            path: String::new(),
            data_type: DataType::Unknown,
            semantic_type: SemanticType::Unknown,
            unit: None,
            value_range: None,
            is_readable: true,
            is_writable: false,
            confidence: 0.0,
        }
    }
}

/// Discovered command that can be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredCommand {
    /// Command name (standardized)
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Parameters for this command
    pub parameters: Vec<CommandParameter>,
    /// Confidence in this discovery (0-1)
    pub confidence: f32,
}

/// Command parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandParameter {
    /// Parameter name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Parameter type
    pub param_type: DataType,
    /// Whether required
    pub required: bool,
    /// Default value
    pub default_value: Option<serde_json::Value>,
    /// Valid value range
    pub valid_range: Option<ValueRange>,
    /// Valid options (for enum)
    pub valid_options: Vec<serde_json::Value>,
}

/// Device category inferred from capabilities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCategory {
    /// Temperature sensor
    TemperatureSensor,
    /// Humidity sensor
    HumiditySensor,
    /// Multi-sensor (temperature, humidity, etc.)
    MultiSensor,
    /// Motion sensor
    MotionSensor,
    /// Light sensor
    LightSensor,
    /// Switch/relay
    Switch,
    /// Dimmer/light
    Dimmer,
    /// Thermostat/HVAC
    Thermostat,
    /// Camera
    Camera,
    /// Energy monitor
    EnergyMonitor,
    /// Gateway/hub
    Gateway,
    /// Controller/PLC
    Controller,
    /// Actuator/valve
    Actuator,
    /// Display/screen
    Display,
    /// Alarm/siren
    Alarm,
    /// Lock
    Lock,
    /// Unknown/unclassified
    Unknown,
}

impl DeviceCategory {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            DeviceCategory::TemperatureSensor => "温度传感器",
            DeviceCategory::HumiditySensor => "湿度传感器",
            DeviceCategory::MultiSensor => "多参数传感器",
            DeviceCategory::MotionSensor => "运动传感器",
            DeviceCategory::LightSensor => "光照传感器",
            DeviceCategory::Switch => "开关",
            DeviceCategory::Dimmer => "调光器",
            DeviceCategory::Thermostat => "温控器",
            DeviceCategory::Camera => "摄像头",
            DeviceCategory::EnergyMonitor => "能耗监控",
            DeviceCategory::Gateway => "网关",
            DeviceCategory::Controller => "控制器",
            DeviceCategory::Actuator => "执行器",
            DeviceCategory::Display => "显示屏",
            DeviceCategory::Alarm => "报警器",
            DeviceCategory::Lock => "门锁",
            DeviceCategory::Unknown => "未知设备",
        }
    }
}

/// Binary format detection result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryFormatDetection {
    /// Raw bytes
    Raw,
    /// Hex string
    HexString,
    /// Base64 encoded
    Base64,
    /// Little-endian int16
    Int16Le,
    /// Big-endian int16
    Int16Be,
    /// Little-endian int32
    Int32Le,
    /// Big-endian int32
    Int32Be,
    /// Little-endian float32
    Float32Le,
    /// Big-endian float32
    Float32Be,
    /// Unknown format
    Unknown,
}

/// Discovery made during analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "discovery_type", rename_all = "snake_case")]
pub enum Discovery {
    /// Discovered a readable metric
    Metric(DiscoveredMetric),
    /// Discovered a writable command
    Command(DiscoveredCommand),
    /// Discovered binary encoding format
    Encoding(BinaryFormatDetection),
    /// Discovered device category
    Category(DeviceCategory),
    /// Text description of discovery
    Note { message: String },
}

/// Confirmation point for user to verify
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationPoint {
    /// Type of confirmation needed
    pub confirmation_type: ConfirmationType,
    /// Question to ask user
    pub question: String,
    /// Suggested answer (from AI)
    pub suggested_answer: serde_json::Value,
    /// Options (if multiple choice)
    pub options: Vec<serde_json::Value>,
    /// Whether this is required or optional
    pub required: bool,
}

/// Type of confirmation needed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationType {
    /// Confirm device type
    DeviceType,
    /// Confirm metric meaning
    MetricSemantic,
    /// Confirm unit
    Unit,
    /// Confirm threshold value
    Threshold,
    /// Confirm command parameter
    CommandParameter,
    /// Multiple choice
    MultipleChoice,
    /// Free text input
    TextInput,
}

/// Inference context for semantic analysis
#[derive(Debug, Clone, Default)]
pub struct InferenceContext {
    /// Device type hint (if known)
    pub device_type_hint: Option<String>,
    /// Manufacturer hint
    pub manufacturer_hint: Option<String>,
    /// Application context
    pub application_context: Option<String>,
    /// Known semantic mappings
    pub known_mappings: HashMap<String, SemanticType>,
}

/// Device type inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeInference {
    /// Inferred device category
    pub category: DeviceCategory,
    /// Suggested device type ID
    pub suggested_id: String,
    /// Suggested display name
    pub suggested_name: String,
    /// Description
    pub description: String,
    /// Confidence score (0-1)
    pub confidence: f32,
    /// Supporting evidence
    pub evidence: Vec<String>,
}

/// ============================================================================
/// Zero-Config Auto-Onboarding Types
/// ============================================================================

/// Status of a draft device in the auto-onboarding process
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftDeviceStatus {
    /// Collecting samples from unknown device
    Collecting,
    /// AI is analyzing samples and generating device type
    Analyzing,
    /// Waiting for user processing (review, type selection, approval)
    WaitingProcessing,
    /// User approved, device type being registered
    Registering,
    /// Successfully registered as active device
    Registered,
    /// User rejected the draft
    Rejected,
    /// Analysis failed (e.g., invalid data, LLM error)
    Failed,
}

/// A draft device discovered through zero-config auto-onboarding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftDevice {
    /// Unique identifier for this draft (generated)
    pub id: String,
    /// Proposed device ID (can be edited by user)
    pub device_id: String,
    /// Source of discovery (mqtt_topic, webhook_url, etc.)
    pub source: String,
    /// Current status
    pub status: DraftDeviceStatus,
    /// Collected data samples
    pub samples: Vec<DeviceSample>,
    /// Maximum samples to collect before analysis
    pub max_samples: usize,
    /// Generated device type (available after analysis)
    pub generated_type: Option<GeneratedDeviceType>,
    /// When first discovered
    pub discovered_at: i64,
    /// When status last changed
    pub updated_at: i64,
    /// Error message if status is Failed
    pub error_message: Option<String>,
    /// User-provided name override
    pub user_name: Option<String>,
    /// User-provided description override
    pub user_description: Option<String>,
    /// Whether to auto-approve (skip manual review)
    #[serde(default)]
    pub auto_approve: bool,
    /// Whether this device sends binary/hex data (not JSON)
    #[serde(default)]
    pub is_binary: bool,
}

/// A generated device type from AI analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedDeviceType {
    /// Generated device type ID (can be edited)
    pub device_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Inferred category
    pub category: DeviceCategory,
    /// Generated metrics
    pub metrics: Vec<DiscoveredMetric>,
    /// Generated commands
    pub commands: Vec<DiscoveredCommand>,
    /// Confidence score (0-1)
    pub confidence: f32,
    /// Raw MDL definition (JSON)
    pub mdl_definition: serde_json::Value,
    /// Processing summary for user review
    pub summary: ProcessingSummary,
}

/// Summary of the AI processing for user review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingSummary {
    /// Number of samples analyzed
    pub samples_analyzed: usize,
    /// Number of fields discovered
    pub fields_discovered: usize,
    /// Number of metrics generated
    pub metrics_generated: usize,
    /// Number of commands generated
    pub commands_generated: usize,
    /// Inferred device category
    pub inferred_category: String,
    /// Key insights about the device
    pub insights: Vec<String>,
    /// Any warnings or issues
    pub warnings: Vec<String>,
    /// Recommended next steps
    pub recommendations: Vec<String>,
}

/// Configuration for auto-onboarding behavior
/// Re-exported from auto_onboard module
pub use crate::discovery::auto_onboard::AutoOnboardConfig;

/// Event emitted during auto-onboarding process
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum AutoOnboardEvent {
    /// New draft device created
    DraftCreated {
        draft_id: String,
        device_id: String,
        source: String,
    },
    /// Sample collected
    SampleCollected {
        draft_id: String,
        sample_count: usize,
    },
    /// Analysis started
    AnalysisStarted {
        draft_id: String,
        sample_count: usize,
    },
    /// Analysis completed
    AnalysisCompleted {
        draft_id: String,
        device_type: String,
        confidence: f32,
    },
    /// Device approved and registered
    DeviceRegistered {
        draft_id: String,
        device_id: String,
        device_type: String,
    },
    /// Device rejected
    DeviceRejected {
        draft_id: String,
        reason: String,
    },
    /// Analysis failed
    AnalysisFailed {
        draft_id: String,
        error: String,
    },
}

impl DraftDevice {
    /// Create a new draft device
    pub fn new(device_id: String, source: String, max_samples: usize) -> Self {
        let id = format!("draft-{}-{}", device_id, chrono::Utc::now().timestamp());
        Self {
            id,
            device_id,
            source,
            status: DraftDeviceStatus::Collecting,
            samples: Vec::new(),
            max_samples,
            generated_type: None,
            discovered_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            error_message: None,
            user_name: None,
            user_description: None,
            auto_approve: false,
            is_binary: false,
        }
    }

    /// Add a sample to the draft
    pub fn add_sample(&mut self, sample: DeviceSample) -> bool {
        if self.samples.len() >= self.max_samples {
            return false;
        }
        self.samples.push(sample);
        self.updated_at = chrono::Utc::now().timestamp();
        true
    }

    /// Check if ready for analysis
    pub fn ready_for_analysis(&self, min_samples: usize) -> bool {
        self.status == DraftDeviceStatus::Collecting
            && self.samples.len() >= min_samples
    }

    /// Check if should trigger analysis by timeout
    pub fn should_trigger_analysis(&self, min_samples: usize, timeout_secs: u64) -> bool {
        self.status == DraftDeviceStatus::Collecting
            && self.samples.len() >= min_samples
            && (chrono::Utc::now().timestamp() - self.updated_at) > timeout_secs as i64
    }

    /// Get samples as JSON values
    pub fn json_samples(&self) -> Vec<serde_json::Value> {
        self.samples.iter()
            .filter_map(|s| s.parsed.clone())
            .collect()
    }

    /// Update status
    pub fn set_status(&mut self, status: DraftDeviceStatus) {
        self.status = status;
        self.updated_at = chrono::Utc::now().timestamp();
    }
}

impl GeneratedDeviceType {
    /// Create from discovered metrics and commands
    pub fn from_discovered(
        device_type: String,
        name: String,
        metrics: Vec<DiscoveredMetric>,
        commands: Vec<DiscoveredCommand>,
        category: DeviceCategory,
        mdl_definition: serde_json::Value,
        samples_analyzed: usize,
    ) -> Self {
        let metrics_count = metrics.len();
        let commands_count = commands.len();
        let category_name = category.display_name().to_string();

        let confidence = if metrics.is_empty() {
            0.0
        } else {
            metrics.iter().map(|m| m.confidence).sum::<f32>() / metrics.len() as f32
        };

        let (insights, warnings) = Self::generate_insights(&metrics, &commands);

        Self {
            device_type,
            name,
            description: format!("Auto-generated device type for {}", category_name),
            category,
            metrics,
            commands,
            confidence,
            mdl_definition,
            summary: ProcessingSummary {
                samples_analyzed,
                fields_discovered: metrics_count,
                metrics_generated: metrics_count,
                commands_generated: commands_count,
                inferred_category: category_name,
                insights,
                warnings,
                recommendations: vec![
                    "Review the generated metrics for accuracy".to_string(),
                    "Test the device with actual data".to_string(),
                    "Add custom transforms if needed".to_string(),
                ],
            },
        }
    }

    fn generate_insights(metrics: &[DiscoveredMetric], commands: &[DiscoveredCommand]) -> (Vec<String>, Vec<String>) {
        let mut insights = Vec::new();
        let mut warnings = Vec::new();

        // Analyze metrics
        let temp_count = metrics.iter().filter(|m| m.semantic_type == SemanticType::Temperature).count();
        let humid_count = metrics.iter().filter(|m| m.semantic_type == SemanticType::Humidity).count();

        if temp_count > 0 && humid_count > 0 {
            insights.push("Device measures both temperature and humidity".to_string());
        }

        let low_conf = metrics.iter().filter(|m| m.confidence < 0.6).count();
        if low_conf > 0 {
            warnings.push(format!("{} metrics have low confidence (<60%), manual review recommended", low_conf));
        }

        if !commands.is_empty() {
            insights.push(format!("Device supports {} commands", commands.len()));
        }

        (insights, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_inference() {
        assert_eq!(DataType::from_json(&serde_json::json!(42)), DataType::Integer);
        assert_eq!(DataType::from_json(&serde_json::json!(3.14)), DataType::Float);
        assert_eq!(DataType::from_json(&serde_json::json!("hello")), DataType::String);
        assert_eq!(DataType::from_json(&serde_json::json!(true)), DataType::Boolean);
        assert_eq!(DataType::from_json(&serde_json::json!(null)), DataType::Null);
        assert_eq!(DataType::from_json(&serde_json::json!([])), DataType::Array);
        assert_eq!(DataType::from_json(&serde_json::json!({})), DataType::Object);
    }

    #[test]
    fn test_semantic_type_inference() {
        assert_eq!(
            SemanticType::infer_from_context("temperature", &Some(serde_json::json!(25.5))),
            SemanticType::Temperature
        );
        assert_eq!(
            SemanticType::infer_from_context("humidity", &Some(serde_json::json!(60))),
            SemanticType::Humidity
        );
        assert_eq!(
            SemanticType::infer_from_context("battery_level", &Some(serde_json::json!(85))),
            SemanticType::Battery
        );
    }

    #[test]
    fn test_value_range() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let range = ValueRange::from_values(&values).unwrap();
        assert_eq!(range.min, 10.0);
        assert_eq!(range.max, 50.0);
        assert_eq!(range.avg, 30.0);
        assert_eq!(range.count, 5);
    }

    #[test]
    fn test_device_sample_from_json() {
        let json = serde_json::json!({"temp": 25.5, "hum": 60});
        let sample = DeviceSample::from_json(json.clone(), "test");
        assert!(sample.is_json());
        assert_eq!(sample.parsed, Some(json));
    }
}
