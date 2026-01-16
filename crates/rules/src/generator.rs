//! LLM-based rule generation.
//!
//! This module provides functionality to generate rules from natural language descriptions
//! using LLM assistance.

use crate::dsl::{ComparisonOperator, ParsedRule, RuleAction, RuleCondition};
use crate::validator::{DeviceInfo, ValidationContext};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result type for LLM generation.
pub type Result<T> = std::result::Result<T, GeneratorError>;

/// Error type for LLM rule generation.
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Missing required information: {0}")]
    MissingInfo(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

/// Configuration for the LLM rule generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorConfig {
    /// Model to use for generation.
    pub model: String,

    /// Maximum number of tokens to generate.
    pub max_tokens: Option<usize>,

    /// Temperature for sampling.
    pub temperature: Option<f32>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            model: "qwen3-vl:2b".to_string(),
            max_tokens: Some(512),
            temperature: Some(0.3),
        }
    }
}

/// Extracted information from natural language description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRuleInfo {
    /// Rule name (generated or extracted).
    pub name: String,

    /// Device identifier mentioned in the description.
    pub device_id: Option<String>,

    /// Metric name.
    pub metric: Option<String>,

    /// Comparison operator.
    pub operator: Option<String>,

    /// Threshold value.
    pub threshold: Option<f64>,

    /// Duration for FOR clause (if mentioned).
    pub for_duration: Option<u64>,

    /// Action type (notify, execute, log).
    pub action_type: Option<String>,

    /// Action message/command.
    pub action_message: Option<String>,

    /// Confidence score for the extraction (0-1).
    pub confidence: f64,

    /// Any missing required information.
    pub missing_info: Vec<String>,

    /// Warnings about the extraction.
    pub warnings: Vec<String>,
}

/// Generated rule with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedRule {
    /// The parsed rule ready to use.
    pub rule: ParsedRule,

    /// Explanation of what was generated.
    pub explanation: String,

    /// Confidence score (0-1).
    pub confidence: f64,

    /// Any validation warnings.
    pub warnings: Vec<String>,

    /// Suggested edits if confidence is low.
    pub suggested_edits: Vec<SuggestedEdit>,
}

/// Suggested edit for improving the generated rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedEdit {
    /// Field path that needs editing.
    pub field: String,

    /// Current value.
    pub current_value: String,

    /// Suggested value.
    pub suggested_value: String,

    /// Reason for the suggestion.
    pub reason: String,
}

/// Rule generator using LLM.
pub struct RuleGenerator;

impl RuleGenerator {
    /// Generate a rule from natural language description.
    ///
    /// # Arguments
    ///
    /// * `description` - Natural language description of the desired rule
    /// * `context` - Available resources (devices, alert channels) for validation
    /// * `config` - Optional generator configuration
    ///
    /// # Examples
    ///
    /// ```
    /// let info = RuleGenerator::extract_info("当温度超过50度时发送告警");
    /// let generated = RuleGenerator::generate(info, &context, None)?;
    /// ```
    pub fn generate(
        description: &str,
        context: &ValidationContext,
        config: Option<&GeneratorConfig>,
    ) -> Result<GeneratedRule> {
        // Step 1: Extract information from description
        let extracted = Self::extract_info(description, context)?;

        // Step 2: Build the rule
        let rule = Self::build_rule(&extracted, context)?;

        // Step 3: Validate and generate suggestions
        let (warnings, suggested_edits) = Self::validate_and_suggest(&extracted, context);

        // Step 4: Generate explanation
        let explanation = Self::generate_explanation(&extracted, &rule);

        Ok(GeneratedRule {
            rule,
            explanation,
            confidence: extracted.confidence,
            warnings,
            suggested_edits,
        })
    }

    /// Extract structured information from natural language.
    fn extract_info(description: &str, context: &ValidationContext) -> Result<ExtractedRuleInfo> {
        let description_lower = description.to_lowercase();

        // Extract device ID
        let device_id = Self::extract_device_id(&description_lower, context);

        // Extract metric
        let metric = Self::extract_metric(&description_lower);

        // Extract operator and threshold
        let (operator, threshold) = Self::extract_condition(&description_lower);

        // Extract action
        let (action_type, action_message) = Self::extract_action(&description_lower);

        // Check for duration (FOR clause)
        let for_duration = Self::extract_duration(&description_lower);

        // Calculate confidence
        let mut confidence = 0.5;
        let mut missing_info = Vec::new();
        let mut warnings = Vec::new();

        if device_id.is_some() {
            confidence += 0.15;
        } else {
            missing_info.push("device_id".to_string());
        }

        if metric.is_some() {
            confidence += 0.1;
        } else {
            missing_info.push("metric".to_string());
        }

        if operator.is_some() {
            confidence += 0.1;
        } else {
            missing_info.push("operator".to_string());
        }

        if threshold.is_some() {
            confidence += 0.1;
        } else {
            missing_info.push("threshold".to_string());
        }

        if action_type.is_some() {
            confidence += 0.05;
        }

        // Generate rule name
        let name = Self::generate_rule_name(&description_lower, &action_message, &metric);

        Ok(ExtractedRuleInfo {
            name,
            device_id,
            metric,
            operator,
            threshold,
            for_duration,
            action_type,
            action_message,
            confidence,
            missing_info,
            warnings,
        })
    }

    /// Extract device ID from description, matching against available devices.
    fn extract_device_id(description: &str, context: &ValidationContext) -> Option<String> {
        // Try to find device by name or ID in the description
        for (id, device) in context.devices.iter() {
            if description.contains(id.to_lowercase().as_str())
                || description.contains(device.name.to_lowercase().as_str())
            {
                return Some(id.clone());
            }
        }

        // Look for common patterns like "device 1", "sensor", etc.
        if description.contains("温度") || description.contains("temperature") {
            // Try to find a temperature sensor
            for (id, device) in context.devices.iter() {
                if device.device_type.contains("temperature")
                    || device.device_type.contains("sensor")
                {
                    return Some(id.clone());
                }
            }
        }

        // Return first device if available
        if !context.devices.is_empty() {
            return Some(context.devices.keys().next()?.clone());
        }

        None
    }

    /// Extract metric name from description.
    fn extract_metric(description: &str) -> Option<String> {
        if description.contains("温度") || description.contains("temperature") {
            return Some("temperature".to_string());
        }
        if description.contains("湿度") || description.contains("humidity") {
            return Some("humidity".to_string());
        }
        if description.contains("状态") || description.contains("state") {
            return Some("state".to_string());
        }
        if description.contains("power") || description.contains("功率") {
            return Some("power".to_string());
        }
        Some("value".to_string())
    }

    /// Extract comparison operator and threshold from description.
    fn extract_condition(description: &str) -> (Option<String>, Option<f64>) {
        let operator = if description.contains("大于") || description.contains(">") {
            Some(">".to_string())
        } else if description.contains("小于") || description.contains("<") {
            Some("<".to_string())
        } else if description.contains("等于") || description.contains("==") {
            Some("==".to_string())
        } else if description.contains("超过") {
            Some(">".to_string())
        } else {
            None
        };

        // Extract numbers from description
        let threshold = description
            .split_whitespace()
            .find_map(|s| s.parse::<f64>().ok())
            .or_else(|| {
                // Look for numbers after "超过" or degree symbols
                if let Some(pos) = description.find("超过") {
                    description[pos + 6..]
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                } else if let Some(pos) = description.find("度") {
                    // Look before "度"
                    let before = &description[..pos];
                    before
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.parse::<f64>().ok())
                } else {
                    None
                }
            });

        (operator, threshold)
    }

    /// Extract action type and message from description.
    fn extract_action(description: &str) -> (Option<String>, Option<String>) {
        let action_type = if description.contains("告警") || description.contains("通知") {
            Some("notify".to_string())
        } else if description.contains("执行") || description.contains("开") {
            Some("execute".to_string())
        } else {
            Some("notify".to_string()) // Default to notify
        };

        let action_message = if description.contains("告警") {
            Some("告警触发".to_string())
        } else if description.contains("高温") {
            Some("温度过高".to_string())
        } else if description.contains("低温") {
            Some("温度过低".to_string())
        } else {
            Some("规则触发".to_string())
        };

        (action_type, action_message)
    }

    /// Extract duration for FOR clause.
    fn extract_duration(description: &str) -> Option<u64> {
        // Look for patterns like "持续30秒", "for 30 seconds", "30s"
        if let Some(pos) = description.find("持续") {
            let after = &description[pos + 6..];
            if let Some(end) = after.find(['秒', 's', ' ']) {
                after[..end].trim().parse::<u64>().ok()
            } else {
                None
            }
        } else if let Some(pos) = description.find("for") {
            let after = &description[pos + 3..];
            if let Some(end) = after.find(['秒', 's', ' ']) {
                after[..end].trim().parse::<u64>().ok()
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Generate a meaningful rule name.
    fn generate_rule_name(description: &str, action_message: &Option<String>, metric: &Option<String>) -> String {
        if let Some(msg) = action_message {
            return format!("{}告警", msg);
        }

        if let Some(m) = metric {
            return format!("{}监控", m);
        }

        "新规则".to_string()
    }

    /// Build a ParsedRule from extracted information.
    fn build_rule(extracted: &ExtractedRuleInfo, context: &ValidationContext) -> Result<ParsedRule> {
        // Use defaults if missing
        let device_id = extracted
            .device_id
            .as_ref()
            .or_else(|| context.devices.keys().next())
            .ok_or_else(|| GeneratorError::MissingInfo("No device available".to_string()))?;

        let metric = extracted.metric.as_ref().unwrap_or(&"value".to_string()).clone();

        let operator = extracted.operator.as_ref().unwrap_or(&">".to_string()).clone();

        let threshold = extracted.threshold.unwrap_or(50.0);

        let condition = RuleCondition {
            device_id: device_id.clone(),
            metric,
            operator: Self::parse_operator(&operator)?,
            threshold,
        };

        let action_type = extracted.action_type.as_ref().unwrap_or(&"notify".to_string()).clone();

        let action = match action_type.as_str() {
            "notify" => RuleAction::Notify {
                message: extracted
                    .action_message
                    .clone()
                    .unwrap_or_else(|| format!("{} 触发", extracted.name)),
            },
            "log" => RuleAction::Log {
                level: crate::dsl::LogLevel::Info,
                message: format!("{} 触发", extracted.name),
                severity: None,
            },
            _ => RuleAction::Notify {
                message: extracted
                    .action_message
                    .clone()
                    .unwrap_or_else(|| format!("{} 触发", extracted.name)),
            },
        };

        Ok(ParsedRule {
            name: extracted.name.clone(),
            condition,
            actions: vec![action],
            for_duration: extracted.for_duration.map(std::time::Duration::from_secs),
        })
    }

    /// Parse operator string to ComparisonOperator.
    fn parse_operator(op: &str) -> Result<ComparisonOperator> {
        match op.trim() {
            ">" | "大于" | "超过" => Ok(ComparisonOperator::GreaterThan),
            "<" | "小于" => Ok(ComparisonOperator::LessThan),
            ">=" | ">=" => Ok(ComparisonOperator::GreaterEqual),
            "<=" | "<=" => Ok(ComparisonOperator::LessEqual),
            "==" | "等于" => Ok(ComparisonOperator::Equal),
            "!=" | "不等于" => Ok(ComparisonOperator::NotEqual),
            _ => Err(GeneratorError::InvalidInput(format!("Unknown operator: {}", op))),
        }
    }

    /// Validate the extracted info and generate suggestions.
    fn validate_and_suggest(extracted: &ExtractedRuleInfo, context: &ValidationContext) -> (Vec<String>, Vec<SuggestedEdit>) {
        let mut warnings = Vec::new();
        let mut suggested_edits = Vec::new();

        // Check device validity
        if let Some(ref device_id) = extracted.device_id {
            if !context.has_device(device_id) {
                warnings.push(format!("设备 '{}' 不存在", device_id));
                suggested_edits.push(SuggestedEdit {
                    field: "condition.device_id".to_string(),
                    current_value: device_id.clone(),
                    suggested_value: context.devices.keys().next().unwrap_or(&"".to_string()).clone(),
                    reason: "设备不存在".to_string(),
                });
            }
        }

        // Check threshold reasonableness
        if let Some(threshold) = extracted.threshold {
            if threshold < 0.0 || threshold > 1000.0 {
                warnings.push(format!("阈值 {} 可能超出合理范围", threshold));
            }
        }

        // Suggest adding FOR duration if not present
        if extracted.for_duration.is_none() {
            suggested_edits.push(SuggestedEdit {
                field: "for_duration".to_string(),
                current_value: "null".to_string(),
                suggested_value: "30".to_string(),
                reason: "建议添加持续时间以避免瞬时波动".to_string(),
            });
        }

        (warnings, suggested_edits)
    }

    /// Generate a human-readable explanation.
    fn generate_explanation(extracted: &ExtractedRuleInfo, rule: &ParsedRule) -> String {
        format!(
            "规则 \"{}\": 当设备 {} 的 {} {} {} 时，{}",
            rule.name,
            rule.condition.device_id,
            rule.condition.metric,
            rule.condition.operator.as_str(),
            rule.condition.threshold,
            if !rule.actions.is_empty() {
                match &rule.actions[0] {
                    RuleAction::Notify { message } => format!("发送通知: {}", message),
                    RuleAction::Execute { command, .. } => format!("执行命令: {}", command),
                    RuleAction::Log { .. } => "记录日志".to_string(),
                }
            } else {
                "无动作".to_string()
            }
        )
    }
}

/// Predefined rule templates for common scenarios.
pub struct RuleTemplates;

impl RuleTemplates {
    /// Get all available templates.
    pub fn all() -> Vec<RuleTemplate> {
        vec![
            RuleTemplate {
                id: "high_temp_alert".to_string(),
                name: "高温告警".to_string(),
                category: "alert".to_string(),
                description: "当温度传感器检测到温度超过阈值时发送告警".to_string(),
                dsl_template: r#"RULE "{rule_name}"
  WHEN {device_id}.temperature > {threshold}
  DO NOTIFY "{device_name} 温度超过 {threshold}°C"
  FOR {duration}
END"#.to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "rule_name".to_string(),
                        label: "规则名称".to_string(),
                        default: Some("高温告警".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "device_id".to_string(),
                        label: "温度传感器ID".to_string(),
                        default: None,
                        required: true,
                    },
                    TemplateParameter {
                        name: "device_name".to_string(),
                        label: "设备名称".to_string(),
                        default: Some("温度传感器".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "threshold".to_string(),
                        label: "温度阈值 (°C)".to_string(),
                        default: Some("50".to_string()),
                        required: true,
                    },
                    TemplateParameter {
                        name: "duration".to_string(),
                        label: "持续时间 (秒)".to_string(),
                        default: Some("30".to_string()),
                        required: false,
                    },
                ],
            },
            RuleTemplate {
                id: "device_offline_alert".to_string(),
                name: "设备离线告警".to_string(),
                category: "alert".to_string(),
                description: "当设备不再上报数据时发送告警".to_string(),
                dsl_template: r#"RULE "{rule_name}"
  WHEN {device_id}.state == 0
  DO NOTIFY "{device_name} 已离线"
END"#.to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "rule_name".to_string(),
                        label: "规则名称".to_string(),
                        default: Some("设备离线告警".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "device_id".to_string(),
                        label: "监控设备ID".to_string(),
                        default: None,
                        required: true,
                    },
                    TemplateParameter {
                        name: "device_name".to_string(),
                        label: "设备名称".to_string(),
                        default: Some("设备".to_string()),
                        required: false,
                    },
                ],
            },
            RuleTemplate {
                id: "auto_control".to_string(),
                name: "自动控制".to_string(),
                category: "automation".to_string(),
                description: "根据传感器数值自动控制设备".to_string(),
                dsl_template: r#"RULE "{rule_name}"
  WHEN {sensor_id}.value > {threshold}
  DO EXECUTE {device_id} on
END"#.to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "rule_name".to_string(),
                        label: "规则名称".to_string(),
                        default: Some("自动控制".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "sensor_id".to_string(),
                        label: "传感器设备".to_string(),
                        default: None,
                        required: true,
                    },
                    TemplateParameter {
                        name: "threshold".to_string(),
                        label: "触发阈值".to_string(),
                        default: Some("50".to_string()),
                        required: true,
                    },
                    TemplateParameter {
                        name: "device_id".to_string(),
                        label: "控制设备".to_string(),
                        default: None,
                        required: true,
                    },
                ],
            },
            RuleTemplate {
                id: "energy_saving".to_string(),
                name: "节能控制".to_string(),
                category: "automation".to_string(),
                description: "在无人时自动关闭灯光".to_string(),
                dsl_template: r#"RULE "{rule_name}"
  WHEN motion_sensor.value == 0
  FOR {duration}
  DO EXECUTE light off
END"#.to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "rule_name".to_string(),
                        label: "规则名称".to_string(),
                        default: Some("节能控制".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "duration".to_string(),
                        label: "无人持续时间 (秒)".to_string(),
                        default: Some("600".to_string()),
                        required: false,
                    },
                ],
            },
            RuleTemplate {
                id: "batch_control".to_string(),
                name: "批量控制".to_string(),
                category: "automation".to_string(),
                description: "根据一个传感器的状态控制多个设备".to_string(),
                dsl_template: r#"RULE "{rule_name}"
  WHEN {sensor_id}.value > {threshold}
  DO
    EXECUTE {device_1} on
    EXECUTE {device_2} on
    NOTIFY "批量控制已触发"
  END"#.to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "rule_name".to_string(),
                        label: "规则名称".to_string(),
                        default: Some("批量控制".to_string()),
                        required: false,
                    },
                    TemplateParameter {
                        name: "sensor_id".to_string(),
                        label: "主传感器".to_string(),
                        default: None,
                        required: true,
                    },
                    TemplateParameter {
                        name: "threshold".to_string(),
                        label: "触发阈值".to_string(),
                        default: Some("50".to_string()),
                        required: true,
                    },
                    TemplateParameter {
                        name: "device_1".to_string(),
                        label: "控制设备1".to_string(),
                        default: None,
                        required: true,
                    },
                    TemplateParameter {
                        name: "device_2".to_string(),
                        label: "控制设备2".to_string(),
                        default: None,
                        required: true,
                    },
                ],
            },
        ]
    }

    /// Get a template by ID.
    pub fn get(id: &str) -> Option<RuleTemplate> {
        Self::all().into_iter().find(|t| t.id == id)
    }
}

/// A rule template with parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTemplate {
    /// Unique template identifier.
    pub id: String,

    /// Template name.
    pub name: String,

    /// Template category (alert, automation, etc.).
    pub category: String,

    /// Human-readable description.
    pub description: String,

    /// DSL template with {parameter} placeholders.
    pub dsl_template: String,

    /// Parameters for this template.
    pub parameters: Vec<TemplateParameter>,
}

/// Template parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name.
    pub name: String,

    /// Human-readable label.
    pub label: String,

    /// Default value (if any).
    pub default: Option<String>,

    /// Whether this parameter is required.
    pub required: bool,
}

/// Rule filled from a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplatedRule {
    /// The rule DSL.
    pub dsl: String,

    /// Template ID used.
    pub template_id: String,

    /// Parameter values used.
    pub parameters: HashMap<String, String>,
}

impl RuleTemplate {
    /// Fill the template with given parameters.
    pub fn fill(&self, params: &HashMap<String, String>) -> Result<TemplatedRule> {
        let mut dsl = self.dsl_template.clone();

        // Check required parameters
        for param in &self.parameters {
            if param.required && !params.contains_key(&param.name) {
                return Err(GeneratorError::MissingInfo(format!(
                    "Missing required parameter: {}",
                    param.label
                )));
            }
        }

        // Fill parameters
        for param in &self.parameters {
            let value = params
                .get(&param.name)
                .or(param.default.as_ref())
                .ok_or_else(|| {
                    GeneratorError::MissingInfo(format!("Missing parameter: {}", param.name))
                })?;

            dsl = dsl.replace(&format!("{{{}}}", param.name), value);
        }

        Ok(TemplatedRule {
            dsl,
            template_id: self.id.clone(),
            parameters: params.clone(),
        })
    }

    /// Get parameters with defaults filled.
    pub fn get_default_parameters(&self) -> HashMap<String, String> {
        self.parameters
            .iter()
            .filter_map(|p| {
                p.default.as_ref().map(|default| (p.name.clone(), default.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_device_id() {
        let mut context = ValidationContext::new();
        context.add_device(DeviceInfo {
            id: "sensor1".to_string(),
            name: "温度传感器".to_string(),
            device_type: "sensor".to_string(),
            metrics: vec![],
            commands: vec![],
            online: true,
        });

        let description = "当温度传感器超过50度时";
        let extracted = RuleGenerator::extract_info(description, &context).unwrap();

        assert_eq!(extracted.device_id, Some("sensor1".to_string()));
        assert_eq!(extracted.metric, Some("temperature".to_string()));
        assert_eq!(extracted.operator, Some(">".to_string()));
        assert_eq!(extracted.threshold, Some(50.0));
    }

    #[test]
    fn test_parse_operator() {
        assert_eq!(
            RuleGenerator::parse_operator(">").unwrap(),
            ComparisonOperator::GreaterThan
        );
        assert_eq!(
            RuleGenerator::parse_operator("小于").unwrap(),
            ComparisonOperator::LessThan
        );
    }

    #[test]
    fn test_template_fill() {
        let template = RuleTemplate {
            id: "test".to_string(),
            name: "Test".to_string(),
            category: "test".to_string(),
            description: "Test template".to_string(),
            dsl_template: "RULE \"{name}\" WHEN {device}.value > {threshold} DO NOTIFY \"Alert\" END"
                .to_string(),
            parameters: vec![
                TemplateParameter {
                    name: "name".to_string(),
                    label: "Name".to_string(),
                    default: Some("Test Rule".to_string()),
                    required: false,
                },
                TemplateParameter {
                    name: "device".to_string(),
                    label: "Device".to_string(),
                    default: None,
                    required: true,
                },
                TemplateParameter {
                    name: "threshold".to_string(),
                    label: "Threshold".to_string(),
                    default: Some("50".to_string()),
                    required: true,
                },
            ],
        };

        let mut params = HashMap::new();
        params.insert("device".to_string(), "sensor1".to_string());
        params.insert("threshold".to_string(), "75".to_string());

        let result = template.fill(&params).unwrap();
        assert!(result.dsl.contains("sensor1"));
        assert!(result.dsl.contains("75"));
        assert!(result.dsl.contains("Test Rule"));
    }
}
