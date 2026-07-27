//! Resource validation for rules.
//!
//! Provides validation functions to check that referenced resources
//! (devices, metrics, extensions) exist and are properly configured.

use crate::models::{ComparisonOperator, CompiledRule, ExecuteTarget, RuleAction, RuleCondition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Virtual metrics emitted by the rule engine itself (not by devices).
/// These bypass the strict device-metric-existence check in [`validate_simple_condition`].
pub const VIRTUAL_METRICS: &[&str] = &["__last_seen_age_secs"];

/// Result type for validation operations.
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validation error with details about what failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("Metric '{metric}' not supported by device '{device_id}'")]
    MetricNotSupported { device_id: String, metric: String },

    #[error("Command '{command}' not supported by device '{device_id}'")]
    CommandNotSupported { device_id: String, command: String },

    #[error("Invalid threshold value: {value}")]
    InvalidThreshold { value: f64 },

    #[error("Validation error: {message}")]
    Other { message: String },
}

impl ValidationError {
    pub fn code(&self) -> &str {
        match self {
            Self::DeviceNotFound { .. } => "DEVICE_NOT_FOUND",
            Self::MetricNotSupported { .. } => "METRIC_NOT_SUPPORTED",
            Self::CommandNotSupported { .. } => "COMMAND_NOT_SUPPORTED",
            Self::InvalidThreshold { .. } => "INVALID_THRESHOLD",
            Self::Other { .. } => "VALIDATION_ERROR",
        }
    }
}

/// Information about available devices for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub metrics: Vec<MetricInfo>,
    pub commands: Vec<CommandInfo>,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    pub data_type: MetricDataType,
    pub unit: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricDataType {
    Number,
    Boolean,
    String,
    Enum(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertChannelInfo {
    pub id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
}

/// Context for validation — contains available resources.
#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    pub devices: HashMap<String, DeviceInfo>,
    pub extensions: HashMap<String, ExtensionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub metrics: Vec<String>,
}

impl ValidationContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_device(&mut self, device: DeviceInfo) {
        self.devices.insert(device.id.clone(), device);
    }

    pub fn get_device(&self, device_id: &str) -> Option<&DeviceInfo> {
        self.devices.get(device_id)
    }

    pub fn add_extension(&mut self, ext: ExtensionInfo) {
        self.extensions.insert(ext.id.clone(), ext);
    }

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
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
            RuleCondition::Comparison {
                source,
                operator,
                threshold,
                threshold_value: _,
            } => {
                match source.source_type {
                    neomind_core::datasource::DataSourceType::Device => {
                        issues.extend(Self::validate_simple_condition(
                            &source.source_id,
                            &source.field_path,
                            operator,
                            threshold,
                            context,
                        )?);
                    }
                    neomind_core::datasource::DataSourceType::Extension => {
                        if context.get_extension(&source.source_id).is_none() {
                            issues.push(ValidationIssue {
                                code: "EXTENSION_NOT_FOUND".to_string(),
                                message: format!(
                                    "Extension '{}' is not registered",
                                    source.source_id
                                ),
                                field: Some("condition.comparison.source".to_string()),
                                severity: ValidationSeverity::Warning,
                            });
                        }
                    }
                    neomind_core::datasource::DataSourceType::Transform => {
                        // Transform sources: just check non-empty
                        if source.source_id.is_empty() {
                            issues.push(ValidationIssue {
                                code: "EMPTY_SOURCE_ID".to_string(),
                                message: "Source ID cannot be empty".to_string(),
                                field: Some("condition.comparison.source".to_string()),
                                severity: ValidationSeverity::Error,
                            });
                        }
                    }
                }
            }
            RuleCondition::Range { source, min, max } => {
                if min > max {
                    issues.push(ValidationIssue {
                        code: "INVALID_RANGE".to_string(),
                        message: format!("Range min ({}) > max ({})", min, max),
                        field: Some("condition.range".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
                if source.source_id.is_empty() {
                    issues.push(ValidationIssue {
                        code: "EMPTY_SOURCE_ID".to_string(),
                        message: "Source ID cannot be empty".to_string(),
                        field: Some("condition.range.source".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
            RuleCondition::Logical {
                operator,
                conditions,
            } => {
                // NOT with multiple children evaluates as NOR (!any), not NAND —
                // almost certainly not the author's intent. Warn (don't error,
                // so existing multi-child NOT rules keep their current behavior).
                if matches!(operator, crate::models::LogicalOperator::Not) && conditions.len() != 1
                {
                    issues.push(ValidationIssue {
                        code: "not_multi_child".to_string(),
                        message: format!(
                            "NOT operator with {} conditions evaluates as NOR \
                             (none-true), not NAND. Use exactly one condition.",
                            conditions.len()
                        ),
                        field: None,
                        severity: ValidationSeverity::Warning,
                    });
                }
                for cond in conditions {
                    issues.extend(Self::validate_condition(cond, context)?);
                }
            }
        }

        Ok(issues)
    }

    fn validate_simple_condition(
        device_id: &str,
        metric: &str,
        operator: &ComparisonOperator,
        threshold: &f64,
        context: &ValidationContext,
    ) -> ValidationResult<Vec<ValidationIssue>> {
        if VIRTUAL_METRICS.contains(&metric) {
            return Self::validate_virtual_metric_condition(
                device_id, metric, operator, threshold, context,
            );
        }

        let mut issues = Vec::new();

        let device =
            context
                .get_device(device_id)
                .ok_or_else(|| ValidationError::DeviceNotFound {
                    device_id: device_id.to_string(),
                })?;

        let metric_info = device
            .metrics
            .iter()
            .find(|m| m.name == metric)
            .ok_or_else(|| ValidationError::MetricNotSupported {
                device_id: device_id.to_string(),
                metric: metric.to_string(),
            })?;

        // Validate threshold against metric range
        if let Some(min) = metric_info.min_value {
            if *threshold < min {
                issues.push(ValidationIssue {
                    code: "THRESHOLD_BELOW_MIN".to_string(),
                    message: format!("Threshold {} is below metric minimum {}", threshold, min),
                    field: Some("condition.comparison.threshold".to_string()),
                    severity: ValidationSeverity::Warning,
                });
            }
        }
        if let Some(max) = metric_info.max_value {
            if *threshold > max {
                issues.push(ValidationIssue {
                    code: "THRESHOLD_ABOVE_MAX".to_string(),
                    message: format!("Threshold {} is above metric maximum {}", threshold, max),
                    field: Some("condition.comparison.threshold".to_string()),
                    severity: ValidationSeverity::Warning,
                });
            }
        }

        let _ = operator; // Operator is always valid
        Ok(issues)
    }

    /// Validate a condition that references a virtual metric (e.g. `__last_seen_age_secs`).
    ///
    /// Virtual metrics bypass the strict device-metric-existence check because they
    /// are emitted by the rule engine itself, not by the device. The device must
    /// still exist (otherwise [`ValidationError::DeviceNotFound`] is returned).
    ///
    /// Operators other than `>` / `>=` (and the equality forms `==` / `!=`) are
    /// semantically meaningless for a monotonically-increasing age counter, so a
    /// [`ValidationIssue`] with code `VIRTUAL_METRIC_BAD_OPERATOR` is emitted as a
    /// warning. The rule remains valid — the user may have a legitimate reason.
    fn validate_virtual_metric_condition(
        device_id: &str,
        _metric: &str,
        operator: &ComparisonOperator,
        _threshold: &f64,
        context: &ValidationContext,
    ) -> ValidationResult<Vec<ValidationIssue>> {
        if context.get_device(device_id).is_none() {
            return Err(ValidationError::DeviceNotFound {
                device_id: device_id.to_string(),
            });
        }

        let mut issues = Vec::new();
        let bad_op = matches!(
            operator,
            ComparisonOperator::LessThan
                | ComparisonOperator::LessEqual
                | ComparisonOperator::Contains
                | ComparisonOperator::StartsWith
                | ComparisonOperator::EndsWith
                | ComparisonOperator::Regex
        );
        if bad_op {
            issues.push(ValidationIssue {
                code: "VIRTUAL_METRIC_BAD_OPERATOR".to_string(),
                message: format!(
                    "Operator '{}' is not meaningful for virtual metric (use > or >=)",
                    operator.symbol()
                ),
                field: Some("condition.comparison.operator".to_string()),
                severity: ValidationSeverity::Warning,
            });
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
            RuleAction::Notify { message, .. } => {
                if message.is_empty() {
                    issues.push(ValidationIssue {
                        code: "EMPTY_MESSAGE".to_string(),
                        message: "Notify message cannot be empty".to_string(),
                        field: Some("actions.notify.message".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
            RuleAction::Execute {
                target,
                target_type,
                command,
                params,
            } => match target_type {
                ExecuteTarget::Device => {
                    let device = context.get_device(target).ok_or_else(|| {
                        ValidationError::DeviceNotFound {
                            device_id: target.clone(),
                        }
                    })?;

                    let cmd_info = device
                        .commands
                        .iter()
                        .find(|c| c.name == *command)
                        .ok_or_else(|| ValidationError::CommandNotSupported {
                            device_id: target.clone(),
                            command: command.clone(),
                        })?;

                    if let Some(obj) = params.as_object() {
                        for param in &cmd_info.parameters {
                            if param.required && !obj.contains_key(&param.name) {
                                issues.push(ValidationIssue {
                                    code: "MISSING_PARAMETER".to_string(),
                                    message: format!("Missing required parameter: {}", param.name),
                                    field: Some(format!(
                                        "actions.{}.params.{}",
                                        command, param.name
                                    )),
                                    severity: ValidationSeverity::Error,
                                });
                            }
                        }
                    }
                }
                ExecuteTarget::Extension => {
                    if target.is_empty() {
                        issues.push(ValidationIssue {
                            code: "EMPTY_EXTENSION_ID".to_string(),
                            message: "Extension target ID cannot be empty".to_string(),
                            field: Some("actions.execute.target".to_string()),
                            severity: ValidationSeverity::Error,
                        });
                    }
                }
            },
            RuleAction::TriggerAgent { agent_id, .. } => {
                if agent_id.is_empty() {
                    issues.push(ValidationIssue {
                        code: "EMPTY_AGENT_ID".to_string(),
                        message: "Agent ID cannot be empty".to_string(),
                        field: Some("actions.trigger_agent.agent_id".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
        }

        Ok(issues)
    }

    /// Validate a complete rule (condition + actions).
    pub fn validate_rule(
        condition: &Option<RuleCondition>,
        actions: &[RuleAction],
        context: &ValidationContext,
    ) -> RuleValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        if let Some(cond) = condition {
            match Self::validate_condition(cond, context) {
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
        }

        for action in actions {
            match Self::validate_action(action, context) {
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
                        field: Some("actions".to_string()),
                        severity: ValidationSeverity::Error,
                    });
                }
            }
        }

        RuleValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        }
    }

    /// Minimum cooldown enforced on rules that subscribe to virtual metrics.
    ///
    /// The virtual-metric emitter refreshes `__last_seen_age_secs` on a 60s
    /// tick. The cooldown floor matches the tick interval — anything shorter
    /// would still be equivalent to "no cooldown" and let the rule fire on
    /// every tick. Users can pick any value from 60s (testing) up to hours
    /// (production). See SimpleRuleBuilderSplit cooldown hint for soft guidance.
    pub const MIN_VIRTUAL_METRIC_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);

    /// Enforce a minimum cooldown on rules that subscribe to virtual metrics.
    ///
    /// Virtual metrics like `__last_seen_age_secs` are refreshed by a periodic
    /// emitter (60s tick). Without a cooldown, a rule whose condition is true
    /// would fire every 60 seconds. This guard requires cooldown >= 60s so
    /// that at most one firing per emitter tick is possible.
    ///
    /// Returns `Ok(())` if the rule does not use any virtual metric, or if the
    /// configured cooldown meets the floor. Returns `Err(message)` otherwise.
    pub fn validate_virtual_metric_cooldown(rule: &CompiledRule) -> Result<(), String> {
        let uses_virtual = rule
            .condition
            .as_ref()
            .map(|c| {
                c.extract_sources().iter().any(|s| {
                    s.source_type == neomind_core::datasource::DataSourceType::Device
                        && VIRTUAL_METRICS.contains(&s.field_path.as_str())
                })
            })
            .unwrap_or(false);

        if !uses_virtual {
            return Ok(());
        }

        if rule.cooldown < Self::MIN_VIRTUAL_METRIC_COOLDOWN {
            return Err(format!(
                "Rules using virtual metrics ({}) must set cooldown >= {} ms (got {} ms). \
                 Without a cooldown the rule would fire every 60 seconds.",
                VIRTUAL_METRICS.join(", "),
                Self::MIN_VIRTUAL_METRIC_COOLDOWN.as_millis(),
                rule.cooldown.as_millis()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use neomind_core::datasource::DataSourceId;

    #[test]
    fn test_validate_device_not_found() {
        let context = ValidationContext::new();
        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("nonexistent", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        };
        let result = RuleValidator::validate_condition(&condition, &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_device_found() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "temperature".to_string(),
            metrics: vec![MetricInfo {
                name: "temperature".to_string(),
                data_type: MetricDataType::Number,
                unit: Some("C".to_string()),
                min_value: Some(-40.0),
                max_value: Some(125.0),
            }],
            commands: vec![],
            online: true,
        });

        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        };
        let result = RuleValidator::validate_condition(&condition, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_threshold_out_of_range() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "temperature".to_string(),
            metrics: vec![MetricInfo {
                name: "temperature".to_string(),
                data_type: MetricDataType::Number,
                unit: Some("C".to_string()),
                min_value: Some(-40.0),
                max_value: Some(125.0),
            }],
            commands: vec![],
            online: true,
        });

        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 150.0,
            threshold_value: None,
        };
        let issues = RuleValidator::validate_condition(&condition, &context).unwrap();
        assert!(!issues.is_empty()); // Warning about threshold above max
    }

    #[test]
    fn test_validate_action_execute() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "fan-001".to_string(),
            name: "Fan".to_string(),
            device_type: "fan".to_string(),
            metrics: vec![],
            commands: vec![CommandInfo {
                name: "turn_on".to_string(),
                description: "Turn on".to_string(),
                parameters: vec![ParameterInfo {
                    name: "speed".to_string(),
                    param_type: "number".to_string(),
                    required: true,
                    default_value: None,
                }],
            }],
            online: true,
        });

        // Valid action
        let action = RuleAction::Execute {
            target: "fan-001".to_string(),
            target_type: ExecuteTarget::Device,
            command: "turn_on".to_string(),
            params: serde_json::json!({"speed": 100}),
        };
        let issues = RuleValidator::validate_action(&action, &context).unwrap();
        assert!(issues.is_empty());

        // Missing required param
        let action = RuleAction::Execute {
            target: "fan-001".to_string(),
            target_type: ExecuteTarget::Device,
            command: "turn_on".to_string(),
            params: serde_json::json!({}),
        };
        let issues = RuleValidator::validate_action(&action, &context).unwrap();
        assert!(!issues.is_empty());
    }

    // ----- Virtual-metric allowlist tests (Task 1) ---------------------------

    /// Helper: build a context with one real device that exposes a single
    /// `temperature` metric (no virtual metrics are registered on the device).
    fn make_ctx_with_temp_device() -> ValidationContext {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "temperature".to_string(),
            metrics: vec![MetricInfo {
                name: "temperature".to_string(),
                data_type: MetricDataType::Number,
                unit: Some("C".to_string()),
                min_value: Some(-40.0),
                max_value: Some(125.0),
            }],
            commands: vec![],
            online: true,
        });
        context
    }

    /// 1. Virtual metric `__last_seen_age_secs` bypasses the metric-existence
    ///    check; `GreaterThan` is a valid operator → Ok with no warnings.
    #[test]
    fn test_virtual_metric_bypasses_metric_check() {
        let context = make_ctx_with_temp_device();
        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 3600.0,
            threshold_value: None,
        };
        let issues = RuleValidator::validate_condition(&condition, &context)
            .expect("virtual metric should bypass metric-existence check");
        assert!(
            issues.is_empty(),
            "expected no warnings for GreaterThan, got: {:?}",
            issues
        );
    }

    /// 2. Regular unknown metric on a real device still fails with
    ///    METRIC_NOT_SUPPORTED.
    #[test]
    fn test_unknown_metric_still_rejected() {
        let context = make_ctx_with_temp_device();
        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "nonexistent_metric"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 10.0,
            threshold_value: None,
        };
        let err = RuleValidator::validate_condition(&condition, &context)
            .expect_err("unknown metric must be rejected");
        assert_eq!(err.code(), "METRIC_NOT_SUPPORTED");
    }

    /// 3. Virtual metric on unknown device still fails with DEVICE_NOT_FOUND
    ///    — device existence is enforced even for virtual metrics.
    #[test]
    fn test_virtual_metric_unknown_device_rejected() {
        let context = ValidationContext::new();
        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("ghost-device", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 3600.0,
            threshold_value: None,
        };
        let err = RuleValidator::validate_condition(&condition, &context)
            .expect_err("unknown device must be rejected even for virtual metrics");
        assert_eq!(err.code(), "DEVICE_NOT_FOUND");
    }

    /// 4. Virtual metric with a bad operator (LessThan) emits
    ///    VIRTUAL_METRIC_BAD_OPERATOR warning, rule still valid (Ok).
    #[test]
    fn test_virtual_metric_bad_operator_warns() {
        let context = make_ctx_with_temp_device();
        let condition = RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "__last_seen_age_secs"),
            operator: ComparisonOperator::LessThan,
            threshold: 60.0,
            threshold_value: None,
        };
        let issues = RuleValidator::validate_condition(&condition, &context)
            .expect("bad-operator case must stay valid (warning only)");
        assert_eq!(issues.len(), 1, "expected exactly one warning");
        assert_eq!(issues[0].code, "VIRTUAL_METRIC_BAD_OPERATOR");
        assert!(matches!(issues[0].severity, ValidationSeverity::Warning));
    }

    // ----- Virtual-metric cooldown tests (Task 2) ---------------------------

    /// Short cooldown (30s) on a virtual-metric rule must be rejected — shorter
    /// than the 60s emitter tick, so the rule would still fire on every tick.
    #[test]
    fn test_validate_virtual_metric_cooldown_rejects_short() {
        let mut rule = CompiledRule::new("test");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-001", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 3600.0,
            threshold_value: None,
        });
        rule.cooldown = std::time::Duration::from_secs(30); // below 60s floor
        rule.finalize();

        let result = RuleValidator::validate_virtual_metric_cooldown(&rule);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("cooldown"),
            "error message must mention 'cooldown', got: {}",
            msg
        );
    }

    /// Exactly 60s cooldown matches the emitter tick interval — accepted.
    /// This is the minimum that prevents more than one firing per tick.
    #[test]
    fn test_validate_virtual_metric_cooldown_accepts_min() {
        let mut rule = CompiledRule::new("test");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-001", "__last_seen_age_secs"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 60.0,
            threshold_value: None,
        });
        rule.cooldown = std::time::Duration::from_secs(60);
        rule.finalize();

        let result = RuleValidator::validate_virtual_metric_cooldown(&rule);
        assert!(result.is_ok());
    }

    /// Regular (non-virtual) metrics are not subject to the cooldown floor —
    /// the validator must skip them entirely.
    #[test]
    fn test_validate_virtual_metric_cooldown_ignores_regular_metrics() {
        let mut rule = CompiledRule::new("test");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("dev-001", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.cooldown = std::time::Duration::from_secs(0); // would be invalid for virtual
        rule.finalize();

        let result = RuleValidator::validate_virtual_metric_cooldown(&rule);
        assert!(result.is_ok(), "regular metrics must skip cooldown check");
    }
}
