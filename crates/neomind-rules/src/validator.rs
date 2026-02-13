//! Resource validation for rules.
//!
//! Provides validation functions to check that referenced resources
//! (devices, metrics, alert channels) exist and are properly configured.

use crate::dsl::{ComparisonOperator, RuleAction, RuleCondition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result type for validation operations.
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validation error with details about what failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("Metric '{metric}' not supported by device '{device_id}'")]
    MetricNotSupported { device_id: String, metric: String },

    #[error("Alert channel not found: {channel_id}")]
    AlertChannelNotFound { channel_id: String },

    #[error("Command '{command}' not supported by device '{device_id}'")]
    CommandNotSupported { device_id: String, command: String },

    #[error("Invalid threshold value: {value}")]
    InvalidThreshold { value: f64 },

    #[error("Validation error: {message}")]
    Other { message: String },
}

impl ValidationError {
    /// Get error code for client handling.
    pub fn code(&self) -> &str {
        match self {
            Self::DeviceNotFound { .. } => "DEVICE_NOT_FOUND",
            Self::MetricNotSupported { .. } => "METRIC_NOT_SUPPORTED",
            Self::AlertChannelNotFound { .. } => "ALERT_CHANNEL_NOT_FOUND",
            Self::CommandNotSupported { .. } => "COMMAND_NOT_SUPPORTED",
            Self::InvalidThreshold { .. } => "INVALID_THRESHOLD",
            Self::Other { .. } => "VALIDATION_ERROR",
        }
    }

    /// Get error details as a map.
    pub fn details(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        match self {
            Self::DeviceNotFound { device_id } => {
                map.insert("device_id".to_string(), device_id.clone());
            }
            Self::MetricNotSupported { device_id, metric } => {
                map.insert("device_id".to_string(), device_id.clone());
                map.insert("metric".to_string(), metric.clone());
            }
            Self::AlertChannelNotFound { channel_id } => {
                map.insert("channel_id".to_string(), channel_id.clone());
            }
            Self::CommandNotSupported { device_id, command } => {
                map.insert("device_id".to_string(), device_id.clone());
                map.insert("command".to_string(), command.clone());
            }
            Self::InvalidThreshold { value } => {
                map.insert("value".to_string(), value.to_string());
            }
            Self::Other { message } => {
                map.insert("message".to_string(), message.clone());
            }
        }
        map
    }
}

/// Information about available devices for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    /// Supported metrics for this device.
    pub metrics: Vec<MetricInfo>,
    /// Supported commands for this device.
    pub commands: Vec<CommandInfo>,
    /// Writable properties for this device.
    pub properties: Vec<PropertyInfo>,
    /// Whether the device is currently online.
    pub online: bool,
}

/// Information about a device property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyInfo {
    pub name: String,
    pub property_type: String,
    pub writable: bool,
}

/// Information about a device metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    pub data_type: MetricDataType,
    pub unit: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

/// Data type for a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricDataType {
    Number,
    Boolean,
    String,
    Enum(Vec<String>),
}

/// Information about a device command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParameterInfo>,
}

/// Information about a command parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
}

/// Information about available alert channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertChannelInfo {
    pub id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
}

/// Information about available workflows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// Context for validation - contains available resources.
#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    /// Available devices indexed by ID.
    pub devices: HashMap<String, DeviceInfo>,
    /// Available alert channels indexed by ID.
    pub alert_channels: HashMap<String, AlertChannelInfo>,
    /// Available workflows indexed by ID.
    pub workflows: Vec<WorkflowInfo>,
    /// Available extensions indexed by ID (for extension-based rules).
    pub extensions: HashMap<String, ExtensionInfo>,
}

/// Information about available extensions for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub metrics: Vec<String>,
}

impl ValidationContext {
    /// Create a new empty validation context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a device to the context.
    pub fn add_device(&mut self, device: DeviceInfo) {
        self.devices.insert(device.id.clone(), device);
    }

    /// Add an alert channel to the context.
    pub fn add_alert_channel(&mut self, channel: AlertChannelInfo) {
        self.alert_channels.insert(channel.id.clone(), channel);
    }

    /// Check if a device exists.
    pub fn has_device(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    /// Get device info.
    pub fn get_device(&self, device_id: &str) -> Option<&DeviceInfo> {
        self.devices.get(device_id)
    }

    /// Check if an alert channel exists.
    pub fn has_alert_channel(&self, channel_id: &str) -> bool {
        self.alert_channels.contains_key(channel_id)
    }

    /// Get alert channel info.
    pub fn get_alert_channel(&self, channel_id: &str) -> Option<&AlertChannelInfo> {
        self.alert_channels.get(channel_id)
    }

    /// Add an extension to the context.
    pub fn add_extension(&mut self, extension: ExtensionInfo) {
        self.extensions.insert(extension.id.clone(), extension);
    }

    /// Check if an extension exists.
    pub fn has_extension(&self, extension_id: &str) -> bool {
        self.extensions.contains_key(extension_id)
    }

    /// Get extension info.
    pub fn get_extension(&self, extension_id: &str) -> Option<&ExtensionInfo> {
        self.extensions.get(extension_id)
    }
}

/// Validation result for a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub available_resources: AvailableResources,
}

/// A validation issue (error or warning).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub severity: ValidationSeverity,
}

/// Severity of a validation issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

/// Summary of available resources for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableResources {
    pub devices: Vec<ResourceSummary>,
    pub alert_channels: Vec<ResourceSummary>,
}

/// Summary of a resource for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub available: bool,
}

/// Resource validator for rules.
pub struct RuleValidator;

impl RuleValidator {
    /// Validate a rule condition against available resources.
    pub fn validate_condition(
        condition: &RuleCondition,
        context: &ValidationContext,
    ) -> ValidationResult<Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        match condition {
            RuleCondition::Device {
                device_id,
                metric,
                operator,
                threshold,
            } => {
                issues.extend(Self::validate_simple_condition(
                    device_id, metric, operator, threshold, context,
                )?);
            }
            RuleCondition::Extension {
                extension_id,
                metric,
                operator,
                threshold,
            } => {
                // Extension validation - check if extension exists
                if context.get_extension(extension_id).is_none() {
                    issues.push(ValidationIssue {
                        code: "EXTENSION_NOT_FOUND".to_string(),
                        message: format!("Extension '{}' is not registered", extension_id),
                        field: Some("condition.extension_id".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
                // Note: More detailed extension validation could be added here
                let _ = (extension_id, metric, operator, threshold);
            }
            RuleCondition::DeviceRange {
                device_id,
                metric,
                min: _,
                max: _,
            } => {
                // Check device exists
                let device = context.get_device(device_id).ok_or_else(|| {
                    ValidationError::DeviceNotFound {
                        device_id: device_id.clone(),
                    }
                })?;

                // Check device is online (warning only)
                if !device.online {
                    issues.push(ValidationIssue {
                        code: "DEVICE_OFFLINE".to_string(),
                        message: format!("Device '{}' is currently offline", device.name),
                        field: Some("condition.device_id".to_string()),
                        severity: ValidationSeverity::Warning,
                    });
                }

                // Check metric is supported
                let _metric_info = device
                    .metrics
                    .iter()
                    .find(|m| m.name == *metric)
                    .ok_or_else(|| ValidationError::MetricNotSupported {
                        device_id: device_id.clone(),
                        metric: metric.clone(),
                    })?;
            }
            RuleCondition::ExtensionRange {
                extension_id,
                metric,
                min: _,
                max: _,
            } => {
                // Extension range validation - check if extension exists
                if context.get_extension(extension_id).is_none() {
                    issues.push(ValidationIssue {
                        code: "EXTENSION_NOT_FOUND".to_string(),
                        message: format!("Extension '{}' is not registered", extension_id),
                        field: Some("condition.extension_id".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
                // Note: More detailed extension validation could be added here
                let _ = (extension_id, metric);
            }
            RuleCondition::And(conditions) | RuleCondition::Or(conditions) => {
                // Recursively validate each sub-condition
                for cond in conditions {
                    let sub_issues = Self::validate_condition(cond, context)?;
                    issues.extend(sub_issues);
                }
            }
            RuleCondition::Not(cond) => {
                let sub_issues = Self::validate_condition(cond, context)?;
                issues.extend(sub_issues);
            }
        }

        Ok(issues)
    }

    /// Validate a simple condition.
    fn validate_simple_condition(
        device_id: &str,
        metric: &str,
        operator: &ComparisonOperator,
        threshold: &f64,
        context: &ValidationContext,
    ) -> ValidationResult<Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        // Check device exists
        let device =
            context
                .get_device(device_id)
                .ok_or_else(|| ValidationError::DeviceNotFound {
                    device_id: device_id.to_string(),
                })?;

        // Check device is online (warning only)
        if !device.online {
            issues.push(ValidationIssue {
                code: "DEVICE_OFFLINE".to_string(),
                message: format!("Device '{}' is currently offline", device.name),
                field: Some("condition.device_id".to_string()),
                severity: ValidationSeverity::Warning,
            });
        }

        // Check metric is supported
        let metric_info = device
            .metrics
            .iter()
            .find(|m| m.name == *metric)
            .ok_or_else(|| ValidationError::MetricNotSupported {
                device_id: device_id.to_string(),
                metric: metric.to_string(),
            })?;

        // Validate threshold against metric constraints
        if let (Some(min), Some(max)) = (metric_info.min_value, metric_info.max_value) {
            if *threshold < min || *threshold > max {
                issues.push(ValidationIssue {
                    code: "THRESHOLD_OUT_OF_RANGE".to_string(),
                    message: format!(
                        "Threshold {} is outside valid range [{}, {}]",
                        threshold, min, max
                    ),
                    field: Some("condition.threshold".to_string()),
                    severity: ValidationSeverity::Warning,
                });
            }
        }

        // Check if operator is compatible with metric type
        match metric_info.data_type {
            MetricDataType::Boolean => {
                if !matches!(
                    operator,
                    ComparisonOperator::Equal | ComparisonOperator::NotEqual
                ) {
                    issues.push(ValidationIssue {
                        code: "OPERATOR_NOT_COMPATIBLE".to_string(),
                        message: "Only == and != operators are supported for boolean metrics"
                            .to_string(),
                        field: Some("condition.operator".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
                if *threshold != 0.0 && *threshold != 1.0 {
                    issues.push(ValidationIssue {
                        code: "INVALID_BOOLEAN_THRESHOLD".to_string(),
                        message: "Boolean thresholds should be 0 (false) or 1 (true)".to_string(),
                        field: Some("condition.threshold".to_string()),
                        severity: ValidationSeverity::Warning,
                    });
                }
            }
            MetricDataType::Enum(ref values) => {
                let idx = *threshold as usize;
                if idx >= values.len() {
                    issues.push(ValidationIssue {
                        code: "INVALID_ENUM_VALUE".to_string(),
                        message: format!(
                            "Threshold {} is not a valid enum value (max: {})",
                            threshold,
                            values.len() - 1
                        ),
                        field: Some("condition.threshold".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
            _ => {}
        }

        Ok(issues)
    }

    /// Validate a rule action against available resources.
    pub fn validate_action(
        action: &RuleAction,
        context: &ValidationContext,
    ) -> ValidationResult<Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        match action {
            RuleAction::Notify { .. } => {
                // Notify actions don't require specific resources
                // They could use a default alert channel
            }
            RuleAction::Execute {
                device_id,
                command,
                params,
            } => {
                // Check device exists
                let device = context.get_device(device_id).ok_or_else(|| {
                    ValidationError::DeviceNotFound {
                        device_id: device_id.clone(),
                    }
                })?;

                // Check command is supported
                let cmd_info = device
                    .commands
                    .iter()
                    .find(|c| c.name == *command)
                    .ok_or_else(|| ValidationError::CommandNotSupported {
                        device_id: device_id.clone(),
                        command: command.clone(),
                    })?;

                // Validate required parameters
                for param in &cmd_info.parameters {
                    if param.required && !params.contains_key(&param.name) {
                        issues.push(ValidationIssue {
                            code: "MISSING_PARAMETER".to_string(),
                            message: format!("Missing required parameter: {}", param.name),
                            field: Some(format!("actions.{}.params.{}", command, param.name)),
                            severity: ValidationSeverity::Error,
                        });
                    }
                }

                // Warn about unknown parameters
                for param_name in params.keys() {
                    if !cmd_info.parameters.iter().any(|p| &p.name == param_name) {
                        issues.push(ValidationIssue {
                            code: "UNKNOWN_PARAMETER".to_string(),
                            message: format!("Unknown parameter: {}", param_name),
                            field: Some(format!("actions.{}.params.{}", command, param_name)),
                            severity: ValidationSeverity::Warning,
                        });
                    }
                }
            }
            RuleAction::Log { .. } => {
                // Log actions don't require specific resources
            }
            RuleAction::Set {
                device_id,
                property,
                ..
            } => {
                // Check device exists
                let device = context.get_device(device_id).ok_or_else(|| {
                    ValidationError::DeviceNotFound {
                        device_id: device_id.clone(),
                    }
                })?;

                // Check if property is a valid writable property
                if !device
                    .properties
                    .iter()
                    .any(|p| p.name == *property && p.writable)
                {
                    issues.push(ValidationIssue {
                        code: "PROPERTY_NOT_WRITABLE".to_string(),
                        message: format!(
                            "Property '{}' is not writable or doesn't exist",
                            property
                        ),
                        field: Some("actions.set.property".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
            RuleAction::Delay { .. } => {
                // Delay actions don't require specific resources
            }
            RuleAction::CreateAlert { .. } => {
                // Alert creation doesn't require specific resources
            }
            RuleAction::HttpRequest { url, .. } => {
                // Validate URL format
                if url::Url::parse(url).is_err() {
                    issues.push(ValidationIssue {
                        code: "INVALID_URL".to_string(),
                        message: format!("Invalid URL: {}", url),
                        field: Some("actions.http.url".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
        }

        Ok(issues)
    }

    /// Validate a complete rule against available resources.
    pub fn validate_rule(
        condition: &RuleCondition,
        actions: &[RuleAction],
        context: &ValidationContext,
    ) -> RuleValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate condition
        match Self::validate_condition(condition, context) {
            Ok(issues) => {
                for issue in issues {
                    match issue.severity {
                        ValidationSeverity::Error => errors.push(issue),
                        ValidationSeverity::Warning => warnings.push(issue),
                        ValidationSeverity::Info => {}
                    }
                }
            }
            Err(e) => {
                errors.push(ValidationIssue {
                    code: e.code().to_string(),
                    message: e.to_string(),
                    field: Some("condition".to_string()),
                    severity: ValidationSeverity::Error,
                });
            }
        }

        // Validate each action
        for (idx, action) in actions.iter().enumerate() {
            match Self::validate_action(action, context) {
                Ok(issues) => {
                    for mut issue in issues {
                        if issue.field.is_none() {
                            issue.field = Some(format!("actions[{}]", idx));
                        }
                        match issue.severity {
                            ValidationSeverity::Error => errors.push(issue),
                            ValidationSeverity::Warning => warnings.push(issue),
                            ValidationSeverity::Info => {}
                        }
                    }
                }
                Err(e) => {
                    errors.push(ValidationIssue {
                        code: e.code().to_string(),
                        message: e.to_string(),
                        field: Some(format!("actions[{}]", idx)),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
        }

        // Build available resources summary
        let available_resources = AvailableResources {
            devices: context
                .devices
                .values()
                .map(|d| ResourceSummary {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    resource_type: d.device_type.clone(),
                    available: d.online,
                })
                .collect(),
            alert_channels: context
                .alert_channels
                .values()
                .map(|c| ResourceSummary {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    resource_type: c.channel_type.clone(),
                    available: c.enabled,
                })
                .collect(),
        };

        RuleValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            available_resources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::{ComparisonOperator, RuleAction};

    #[test]
    fn test_validate_device_not_found() {
        let context = ValidationContext::new();
        let condition = RuleCondition::Device {
            device_id: "nonexistent".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
        };

        let result = RuleValidator::validate_condition(&condition, &context);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ValidationError::DeviceNotFound { .. }));
    }

    #[test]
    fn test_validate_metric_not_supported() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "sensor".to_string(),
            metrics: vec![],
            commands: vec![],
            properties: vec![],
            online: true,
        });

        let condition = RuleCondition::Device {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
        };

        let result = RuleValidator::validate_condition(&condition, &context);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ValidationError::MetricNotSupported { .. }));
    }

    #[test]
    fn test_validate_success() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "sensor".to_string(),
            metrics: vec![MetricInfo {
                name: "temperature".to_string(),
                data_type: MetricDataType::Number,
                unit: Some("°C".to_string()),
                min_value: Some(-50.0),
                max_value: Some(150.0),
            }],
            commands: vec![],
            properties: vec![],
            online: true,
        });

        let condition = RuleCondition::Device {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
        };

        let result = RuleValidator::validate_condition(&condition, &context);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_validate_threshold_out_of_range() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "sensor".to_string(),
            metrics: vec![MetricInfo {
                name: "temperature".to_string(),
                data_type: MetricDataType::Number,
                unit: Some("°C".to_string()),
                min_value: Some(-50.0),
                max_value: Some(100.0),
            }],
            commands: vec![],
            properties: vec![],
            online: true,
        });

        let condition = RuleCondition::Device {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 150.0, // Out of range
        };

        let result = RuleValidator::validate_condition(&condition, &context).unwrap();
        assert!(!result.is_empty());
        assert_eq!(result[0].code, "THRESHOLD_OUT_OF_RANGE");
    }
}
