//! Bidirectional translation layer between natural language and DSL/MDL.
//!
//! Provides conversion between technical formats and human-readable descriptions.

use serde::{Deserialize, Serialize};

use neomind_devices::mdl_format::DeviceTypeDefinition;
use neomind_rules::dsl::{ComparisonOperator, ParsedRule, RuleAction, RuleCondition};

/// Supported languages for translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Chinese (Simplified)
    Chinese,
    /// English
    English,
}

impl Default for Language {
    fn default() -> Self {
        Self::Chinese
    }
}

impl Language {
    /// Get language code.
    pub fn code(&self) -> &str {
        match self {
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }
}

/// MDL to natural language translator.
pub struct MdlTranslator;

impl MdlTranslator {
    /// Translate a device type definition to natural language.
    pub fn translate_device(
        device: &DeviceTypeDefinition,
        language: Language,
    ) -> DeviceDescription {
        let description = match language {
            Language::Chinese => Self::describe_device_zh(device),
            Language::English => Self::describe_device_en(device),
        };

        let capabilities = Self::list_capabilities(device, language);

        DeviceDescription {
            device_type: device.device_type.clone(),
            name: device.name.clone(),
            description: device.description.clone(),
            language: language.code().to_string(),
            natural_description: description,
            capabilities,
            metrics: device
                .uplink
                .metrics
                .iter()
                .map(|m| MetricInfo {
                    name: m.name.clone(),
                    display_name: m.display_name.clone(),
                    unit: m.unit.clone(),
                    description: format!("{:?}", m.data_type),
                })
                .collect(),
            commands: device
                .downlink
                .commands
                .iter()
                .map(|c| CommandInfo {
                    name: c.name.clone(),
                    display_name: c.display_name.clone(),
                    parameters: c
                        .parameters
                        .iter()
                        .map(|p| ParamInfo {
                            name: p.name.clone(),
                            display_name: p.display_name.clone(),
                            default_value: p
                                .default_value
                                .as_ref()
                                .map(|v| format!("{:?}", v))
                                .unwrap_or_default()
                                .into(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn describe_device_zh(device: &DeviceTypeDefinition) -> String {
        let mut desc = format!("{} 是一种", device.name);

        // Add categories
        if !device.categories.is_empty() {
            desc.push_str(&format!("{}类型设备", device.categories.join("/")));
        }

        // Add metrics info
        if !device.uplink.metrics.is_empty() {
            desc.push_str(&format!(
                "，支持上报{}个指标：",
                device.uplink.metrics.len()
            ));
            for (i, metric) in device.uplink.metrics.iter().enumerate() {
                if i > 0 {
                    desc.push('、');
                }
                desc.push_str(&metric.display_name);
            }
        }

        // Add commands info
        if !device.downlink.commands.is_empty() {
            desc.push_str(&format!(
                "，支持接收{}个命令：",
                device.downlink.commands.len()
            ));
            for (i, cmd) in device.downlink.commands.iter().enumerate() {
                if i > 0 {
                    desc.push('、');
                }
                desc.push_str(&cmd.display_name);
            }
        }

        desc.push('。');
        desc
    }

    fn describe_device_en(device: &DeviceTypeDefinition) -> String {
        let mut desc = format!("{} is a", device.name);

        // Add categories
        if !device.categories.is_empty() {
            desc.push_str(&format!(" {} type device", device.categories.join("/")));
        }

        // Add metrics info
        if !device.uplink.metrics.is_empty() {
            desc.push_str(&format!(
                " that reports {} metrics: ",
                device.uplink.metrics.len()
            ));
            for (i, metric) in device.uplink.metrics.iter().enumerate() {
                if i > 0 {
                    desc.push_str(", ");
                }
                desc.push_str(&metric.display_name);
            }
        }

        // Add commands info
        if !device.downlink.commands.is_empty() {
            desc.push_str(&format!(
                " and supports {} commands: ",
                device.downlink.commands.len()
            ));
            for (i, cmd) in device.downlink.commands.iter().enumerate() {
                if i > 0 {
                    desc.push_str(", ");
                }
                desc.push_str(&cmd.display_name);
            }
        }

        desc.push('.');
        desc
    }

    fn list_capabilities(device: &DeviceTypeDefinition, language: Language) -> Vec<String> {
        let mut caps = Vec::new();

        match language {
            Language::Chinese => {
                for category in &device.categories {
                    caps.push(format!("分类：{}", category));
                }
                for metric in &device.uplink.metrics {
                    caps.push(format!(
                        "可上报指标：{} (单位：{})",
                        metric.display_name, metric.unit
                    ));
                }
                for cmd in &device.downlink.commands {
                    caps.push(format!("可执行命令：{}", cmd.display_name));
                }
            }
            Language::English => {
                for category in &device.categories {
                    caps.push(format!("Category: {}", category));
                }
                for metric in &device.uplink.metrics {
                    caps.push(format!(
                        "Reports metric: {} (unit: {})",
                        metric.display_name, metric.unit
                    ));
                }
                for cmd in &device.downlink.commands {
                    caps.push(format!("Supports command: {}", cmd.display_name));
                }
            }
        }

        caps
    }
}

/// Device description in natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDescription {
    /// Device type ID
    pub device_type: String,
    /// Device name
    pub name: String,
    /// Original description
    pub description: String,
    /// Language code
    pub language: String,
    /// Natural language description
    pub natural_description: String,
    /// Capabilities list
    pub capabilities: Vec<String>,
    /// Available metrics
    pub metrics: Vec<MetricInfo>,
    /// Available commands
    pub commands: Vec<CommandInfo>,
}

/// Metric information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    /// Metric name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Unit
    pub unit: String,
    /// Description
    pub description: String,
}

/// Command information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    /// Command name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Parameters
    pub parameters: Vec<ParamInfo>,
}

/// Parameter information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    /// Parameter name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Default value
    pub default_value: serde_json::Value,
}

/// DSL to natural language translator.
pub struct DslTranslator;

impl DslTranslator {
    /// Translate a parsed rule to natural language.
    pub fn translate_rule(rule: &ParsedRule, language: Language) -> RuleDescription {
        let condition = Self::describe_condition(rule, language);
        let actions = Self::describe_actions(&rule.actions, language);
        let summary = Self::summarize_rule(rule, language);

        // Extract trigger condition info for raw format
        let trigger_condition = match &rule.condition {
            RuleCondition::Device {
                device_id,
                metric,
                operator,
                threshold,
            } => {
                format!(
                    "{}.{} {} {}",
                    device_id,
                    metric,
                    operator.as_str(),
                    threshold
                )
            }
            RuleCondition::Extension {
                extension_id,
                metric,
                operator,
                threshold,
            } => {
                format!(
                    "EXTENSION {}.{} {} {}",
                    extension_id,
                    metric,
                    operator.as_str(),
                    threshold
                )
            }
            RuleCondition::DeviceRange {
                device_id,
                metric,
                min,
                max,
            } => {
                format!("{}.{} BETWEEN {} AND {}", device_id, metric, min, max)
            }
            RuleCondition::ExtensionRange {
                extension_id,
                metric,
                min,
                max,
            } => {
                format!(
                    "EXTENSION {}.{} BETWEEN {} AND {}",
                    extension_id, metric, min, max
                )
            }
            RuleCondition::And(conditions) | RuleCondition::Or(conditions) => {
                format!(
                    "(complex condition with {} sub-conditions)",
                    conditions.len()
                )
            }
            RuleCondition::Not(_) => "(NOT condition)".to_string(),
        };

        RuleDescription {
            name: rule.name.clone(),
            language: language.code().to_string(),
            condition_description: condition,
            actions_description: actions,
            summary,
            trigger_condition,
            has_duration: rule.for_duration.is_some(),
            duration_seconds: rule.for_duration.map(|d| d.as_secs()),
        }
    }

    fn describe_condition(rule: &ParsedRule, language: Language) -> String {
        let duration_text = if let Some(duration) = rule.for_duration {
            let secs = duration.as_secs();
            let time_str = if secs % 3600 == 0 {
                format!(
                    "{} {}",
                    secs / 3600,
                    if language == Language::Chinese {
                        "小时"
                    } else {
                        "hours"
                    }
                )
            } else if secs % 60 == 0 {
                format!(
                    "{} {}",
                    secs / 60,
                    if language == Language::Chinese {
                        "分钟"
                    } else {
                        "minutes"
                    }
                )
            } else {
                format!(
                    "{} {}",
                    secs,
                    if language == Language::Chinese {
                        "秒"
                    } else {
                        "seconds"
                    }
                )
            };

            if language == Language::Chinese {
                format!("，持续{}后触发", time_str)
            } else {
                format!(" and trigger after {}", time_str)
            }
        } else {
            String::new()
        };

        match &rule.condition {
            RuleCondition::Device {
                device_id,
                metric,
                operator,
                threshold,
            } => {
                let operator_text = match language {
                    Language::Chinese => match operator {
                        ComparisonOperator::GreaterThan => "大于",
                        ComparisonOperator::LessThan => "小于",
                        ComparisonOperator::GreaterEqual => "大于等于",
                        ComparisonOperator::LessEqual => "小于等于",
                        ComparisonOperator::Equal => "等于",
                        ComparisonOperator::NotEqual => "不等于",
                    },
                    Language::English => match operator {
                        ComparisonOperator::GreaterThan => "greater than",
                        ComparisonOperator::LessThan => "less than",
                        ComparisonOperator::GreaterEqual => "greater than or equal to",
                        ComparisonOperator::LessEqual => "less than or equal to",
                        ComparisonOperator::Equal => "equal to",
                        ComparisonOperator::NotEqual => "not equal to",
                    },
                };

                match language {
                    Language::Chinese => {
                        format!(
                            "当设备 '{}' 的指标 '{}' {} {} 时{}",
                            device_id, metric, operator_text, threshold, duration_text
                        )
                    }
                    Language::English => {
                        format!(
                            "When metric '{}' on device '{}' is {} {}{}",
                            metric, device_id, operator_text, threshold, duration_text
                        )
                    }
                }
            }
            RuleCondition::Extension {
                extension_id,
                metric,
                operator,
                threshold,
            } => {
                let operator_text = match language {
                    Language::Chinese => match operator {
                        ComparisonOperator::GreaterThan => "大于",
                        ComparisonOperator::LessThan => "小于",
                        ComparisonOperator::GreaterEqual => "大于等于",
                        ComparisonOperator::LessEqual => "小于等于",
                        ComparisonOperator::Equal => "等于",
                        ComparisonOperator::NotEqual => "不等于",
                    },
                    Language::English => match operator {
                        ComparisonOperator::GreaterThan => "greater than",
                        ComparisonOperator::LessThan => "less than",
                        ComparisonOperator::GreaterEqual => "greater than or equal to",
                        ComparisonOperator::LessEqual => "less than or equal to",
                        ComparisonOperator::Equal => "equal to",
                        ComparisonOperator::NotEqual => "not equal to",
                    },
                };

                match language {
                    Language::Chinese => {
                        format!(
                            "当扩展 '{}' 的指标 '{}' {} {} 时{}",
                            extension_id, metric, operator_text, threshold, duration_text
                        )
                    }
                    Language::English => {
                        format!(
                            "When metric '{}' on extension '{}' is {} {}{}",
                            metric, extension_id, operator_text, threshold, duration_text
                        )
                    }
                }
            }
            RuleCondition::DeviceRange {
                device_id,
                metric,
                min,
                max,
            } => match language {
                Language::Chinese => {
                    format!(
                        "当设备 '{}' 的指标 '{}' 在 {} 到 {} 之间时{}",
                        device_id, metric, min, max, duration_text
                    )
                }
                Language::English => {
                    format!(
                        "When metric '{}' on device '{}' is between {} and {}{}",
                        metric, device_id, min, max, duration_text
                    )
                }
            },
            RuleCondition::ExtensionRange {
                extension_id,
                metric,
                min,
                max,
            } => match language {
                Language::Chinese => {
                    format!(
                        "当扩展 '{}' 的指标 '{}' 在 {} 到 {} 之间时{}",
                        extension_id, metric, min, max, duration_text
                    )
                }
                Language::English => {
                    format!(
                        "When metric '{}' on extension '{}' is between {} and {}{}",
                        metric, extension_id, min, max, duration_text
                    )
                }
            },
            RuleCondition::And(conditions) => match language {
                Language::Chinese => {
                    format!("当{}个条件同时满足时{}", conditions.len(), duration_text)
                }
                Language::English => {
                    format!(
                        "When all {} conditions are met{}",
                        conditions.len(),
                        duration_text
                    )
                }
            },
            RuleCondition::Or(conditions) => match language {
                Language::Chinese => {
                    format!("当{}个条件中任一满足时{}", conditions.len(), duration_text)
                }
                Language::English => {
                    format!(
                        "When any of {} conditions is met{}",
                        conditions.len(),
                        duration_text
                    )
                }
            },
            RuleCondition::Not(_) => match language {
                Language::Chinese => format!("当条件不满足时{}", duration_text),
                Language::English => format!("When condition is NOT met{}", duration_text),
            },
        }
    }

    fn describe_actions(actions: &[RuleAction], language: Language) -> Vec<String> {
        actions
            .iter()
            .enumerate()
            .map(|(i, action)| match action {
                RuleAction::Notify { message, .. } => {
                    if language == Language::Chinese {
                        format!("{}. 发送通知：{}", i + 1, message)
                    } else {
                        format!("{}. Send notification: {}", i + 1, message)
                    }
                }
                RuleAction::Execute {
                    device_id,
                    command,
                    params,
                } => {
                    let param_str = if params.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", serde_json::to_string(params).unwrap_or_default())
                    };
                    if language == Language::Chinese {
                        format!(
                            "{}. 执行命令：{}.{}{}",
                            i + 1,
                            device_id,
                            command,
                            param_str
                        )
                    } else {
                        format!(
                            "{}. Execute command: {}.{}{}",
                            i + 1,
                            device_id,
                            command,
                            param_str
                        )
                    }
                }
                RuleAction::Log { level, message, .. } => {
                    if language == Language::Chinese {
                        format!("{}. 记录日志：[{}] {}", i + 1, level, message)
                    } else {
                        format!("{}. Log: [{}] {}", i + 1, level, message)
                    }
                }
                // Handle other RuleAction variants
                _ => {
                    if language == Language::Chinese {
                        format!("{}. 其他动作", i + 1)
                    } else {
                        format!("{}. Other action", i + 1)
                    }
                }
            })
            .collect()
    }

    fn summarize_rule(rule: &ParsedRule, language: Language) -> String {
        // Extract device_id and metric from condition
        let (device_id, metric) = match &rule.condition {
            RuleCondition::Device {
                device_id, metric, ..
            }
            | RuleCondition::DeviceRange {
                device_id, metric, ..
            } => (device_id.clone(), metric.clone()),
            RuleCondition::Extension {
                extension_id,
                metric,
                ..
            }
            | RuleCondition::ExtensionRange {
                extension_id,
                metric,
                ..
            } => (extension_id.clone(), metric.clone()),
            RuleCondition::And(conditions) | RuleCondition::Or(conditions) => (
                format!("({} devices)", conditions.len()),
                "(complex)".to_string(),
            ),
            RuleCondition::Not(_) => ("(not)".to_string(), "(complex)".to_string()),
        };

        match language {
            Language::Chinese => {
                format!(
                    "规则 '{}'：监控设备 '{}' 的 '{}' 指标，包含 {} 个执行动作",
                    rule.name,
                    device_id,
                    metric,
                    rule.actions.len()
                )
            }
            Language::English => {
                format!(
                    "Rule '{}': Monitors metric '{}' on device '{}' with {} action(s)",
                    rule.name,
                    metric,
                    device_id,
                    rule.actions.len()
                )
            }
        }
    }
}

/// Rule description in natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDescription {
    /// Rule name
    pub name: String,
    /// Language code
    pub language: String,
    /// Condition description
    pub condition_description: String,
    /// Actions description
    pub actions_description: Vec<String>,
    /// Rule summary
    pub summary: String,
    /// Trigger condition (raw format)
    pub trigger_condition: String,
    /// Has duration condition
    pub has_duration: bool,
    /// Duration in seconds
    pub duration_seconds: Option<u64>,
}

/// Natural language to DSL converter.
pub struct NlToDslConverter;

impl NlToDslConverter {
    /// Parse a natural language description and generate DSL.
    ///
    /// This is a simplified implementation. A production version would use LLM
    /// to parse the intent and generate proper DSL.
    pub fn convert(
        description: &str,
        context: &ConversionContext,
    ) -> Result<DslGeneration, ConversionError> {
        let desc_lower = description.to_lowercase();

        // Detect rule name
        let name = Self::extract_name(description).unwrap_or_else(|| "未命名规则".to_string());

        // Detect device and metric
        let device_id = context.default_device_id.clone().unwrap_or_else(|| {
            Self::find_match(&desc_lower, &context.known_devices)
                .unwrap_or_else(|| "sensor".to_string())
        });

        let metric = context.default_metric.clone().unwrap_or_else(|| {
            Self::find_match(&desc_lower, &context.known_metrics)
                .unwrap_or_else(|| "value".to_string())
        });

        // Detect operator and threshold
        let (operator, threshold) = Self::extract_operator_threshold(&desc_lower)
            .unwrap_or((ComparisonOperator::GreaterThan, 50.0));

        // Detect duration
        let for_duration = Self::extract_duration(&desc_lower);

        // Detect actions
        let actions = Self::extract_actions(&desc_lower, &device_id, context);

        let dsl = Self::format_dsl(
            &name,
            &device_id,
            &metric,
            operator,
            threshold,
            for_duration.as_ref(),
            &actions,
        );

        Ok(DslGeneration {
            dsl,
            name,
            device_id,
            metric,
            operator: operator.as_str().to_string(),
            threshold,
            duration_seconds: for_duration.map(|d| d.as_secs()),
            actions_count: actions.len(),
            confidence: Self::calculate_confidence(&desc_lower, context),
        })
    }

    fn extract_name(description: &str) -> Option<String> {
        // Look for patterns like "规则名为..." or "名为..."
        let desc_lower = description.to_lowercase();

        if let Some(start) = desc_lower.find("规则名为") {
            let start = start + "规则名为".len();
            if let Some(end) = description[start..].find(['，', '。', ',']) {
                return Some(description[start..start + end].trim().to_string());
            }
        }

        if let Some(start) = desc_lower.find("名为") {
            let start = start + "名为".len();
            if let Some(end) = description[start..].find([' ', '，', '。', ',']) {
                return Some(description[start..start + end].trim().to_string());
            }
        }

        None
    }

    fn find_match(text: &str, options: &[String]) -> Option<String> {
        for option in options {
            if text.contains(&option.to_lowercase()) {
                return Some(option.clone());
            }
        }
        None
    }

    fn extract_operator_threshold(text: &str) -> Option<(ComparisonOperator, f64)> {
        // Try to find patterns like "> 50", "小于 30", etc.
        let patterns = [
            (">", ComparisonOperator::GreaterThan),
            ("<", ComparisonOperator::LessThan),
            (">=", ComparisonOperator::GreaterEqual),
            ("<=", ComparisonOperator::LessEqual),
            ("==", ComparisonOperator::Equal),
            ("!=", ComparisonOperator::NotEqual),
            ("大于", ComparisonOperator::GreaterThan),
            ("小于", ComparisonOperator::LessThan),
            ("超过", ComparisonOperator::GreaterThan),
        ];

        for (pattern, op) in &patterns {
            if let Some(pos) = text.find(pattern) {
                let after = text[pos + pattern.len()..].trim();
                // Try to parse number - if we find a non-digit char, use up to that point
                let num_str = if let Some(end) =
                    after.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                {
                    &after[..end]
                } else {
                    after // Use entire remaining string if no separator found
                };
                if let Ok(val) = num_str.parse::<f64>() {
                    return Some((*op, val));
                }
            }
        }

        None
    }

    fn extract_duration(text: &str) -> Option<std::time::Duration> {
        let keywords = [
            ("秒", 1),
            ("秒后", 1),
            ("second", 1),
            ("分钟", 60),
            ("分钟后", 60),
            ("minute", 60),
            ("小时", 3600),
            ("小时后", 3600),
            ("hour", 3600),
        ];

        for (keyword, multiplier) in &keywords {
            if let Some(pos) = text.find(keyword) {
                // Look for number before the keyword
                let before = &text[..pos];
                let trimmed = before.trim();
                // Find the last number
                if let Some(num_end) = trimmed
                    .chars()
                    .rev()
                    .position(|c| !c.is_ascii_digit() && c != '.' && c != ' ')
                {
                    let num_start = trimmed.len() - num_end;
                    let num_str = &trimmed[num_start..];
                    if let Ok(val) = num_str.trim().parse::<u64>() {
                        return Some(std::time::Duration::from_secs(val * multiplier));
                    }
                }
            }
        }

        None
    }

    fn extract_actions(
        text: &str,
        _device_id: &str,
        context: &ConversionContext,
    ) -> Vec<RuleAction> {
        let mut actions = Vec::new();

        // Check for notification intent
        if text.contains("通知")
            || text.contains("告警")
            || text.contains("提醒")
            || text.contains("notify")
        {
            let message = if let Some(start) = text.find("通知") {
                // Extract message content
                let after = &text[start + "通知".len()..];
                if let Some(end) = after.find(['，', '。', ',']) {
                    after[..end].trim().to_string()
                } else {
                    "规则触发".to_string()
                }
            } else {
                "规则触发".to_string()
            };
            actions.push(RuleAction::Notify {
                message,
                channels: None,
            });
        }

        // Check for logging intent
        if text.contains("记录") || text.contains("日志") || text.contains("log") {
            actions.push(RuleAction::Log {
                level: neomind_rules::dsl::LogLevel::Info,
                message: "规则已触发".to_string(),
                severity: None,
            });
        }

        // If no actions found, add a default notification
        if actions.is_empty() {
            actions.push(RuleAction::Notify {
                message: format!(
                    "规则 '{}' 已触发",
                    context.default_rule_name.as_deref().unwrap_or("规则")
                ),
                channels: None,
            });
        }

        actions
    }

    fn format_dsl(
        name: &str,
        device_id: &str,
        metric: &str,
        operator: ComparisonOperator,
        threshold: f64,
        for_duration: Option<&std::time::Duration>,
        actions: &[RuleAction],
    ) -> String {
        let mut dsl = format!("RULE \"{}\"\n", name);
        dsl.push_str(&format!(
            "WHEN {}.{} {} {}\n",
            device_id,
            metric,
            operator.as_str(),
            threshold
        ));

        if let Some(duration) = for_duration {
            let secs = duration.as_secs();
            if secs % 3600 == 0 {
                dsl.push_str(&format!("FOR {} hours\n", secs / 3600));
            } else if secs % 60 == 0 {
                dsl.push_str(&format!("FOR {} minutes\n", secs / 60));
            } else {
                dsl.push_str(&format!("FOR {} seconds\n", secs));
            }
        }

        dsl.push_str("DO\n");
        for action in actions {
            match action {
                RuleAction::Notify { message, .. } => {
                    dsl.push_str(&format!("    NOTIFY \"{}\"\n", message));
                }
                RuleAction::Log { level, .. } => {
                    dsl.push_str(&format!("    LOG {}\n", level));
                }
                _ => {}
            }
        }
        dsl.push_str("END\n");

        dsl
    }

    fn calculate_confidence(text: &str, context: &ConversionContext) -> f32 {
        let mut confidence: f32 = 0.5;

        // Higher confidence if we found known devices
        if context
            .known_devices
            .iter()
            .any(|d| text.contains(&d.to_lowercase()))
        {
            confidence += 0.1;
        }

        // Higher confidence if we found known metrics
        if context
            .known_metrics
            .iter()
            .any(|m| text.contains(&m.to_lowercase()))
        {
            confidence += 0.1;
        }

        // Higher confidence if we found operator
        if text.contains(">")
            || text.contains("<")
            || text.contains("大于")
            || text.contains("小于")
        {
            confidence += 0.2;
        }

        if confidence > 1.0 {
            1.0
        } else {
            confidence
        }
    }
}

/// Context for natural language to DSL conversion.
#[derive(Debug, Clone, Default)]
pub struct ConversionContext {
    /// Known device IDs
    pub known_devices: Vec<String>,
    /// Known metrics
    pub known_metrics: Vec<String>,
    /// Default device ID if none found
    pub default_device_id: Option<String>,
    /// Default metric if none found
    pub default_metric: Option<String>,
    /// Default rule name
    pub default_rule_name: Option<String>,
}

/// Generated DSL result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslGeneration {
    /// Generated DSL text
    pub dsl: String,
    /// Rule name
    pub name: String,
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Comparison operator
    pub operator: String,
    /// Threshold value
    pub threshold: f64,
    /// Duration in seconds (if any)
    pub duration_seconds: Option<u64>,
    /// Number of actions
    pub actions_count: usize,
    /// Confidence score (0-1)
    pub confidence: f32,
}

/// Conversion error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConversionError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_devices::mdl_format::MetricDefinition;
    use neomind_devices::MetricDataType;
    use neomind_rules::dsl::RuleDslParser;
    use std::collections::HashMap;

    #[test]
    fn test_language_code() {
        assert_eq!(Language::Chinese.code(), "zh");
        assert_eq!(Language::English.code(), "en");
    }

    #[test]
    fn test_nl_to_dsl_basic() {
        let context = ConversionContext {
            known_devices: vec!["sensor".to_string()],
            known_metrics: vec!["temperature".to_string()],
            default_device_id: Some("sensor".to_string()),
            default_metric: Some("temperature".to_string()),
            default_rule_name: Some("测试规则".to_string()),
        };

        let result = NlToDslConverter::convert("当传感器温度大于50时发送通知", &context).unwrap();

        assert!(result.dsl.contains("RULE"));
        assert!(result.dsl.contains("WHEN"));
        assert!(result.confidence > 0.5);
    }

    #[test]
    fn test_nl_to_dsl_extract_operator() {
        assert_eq!(
            NlToDslConverter::extract_operator_threshold("温度大于50"),
            Some((ComparisonOperator::GreaterThan, 50.0))
        );
        assert_eq!(
            NlToDslConverter::extract_operator_threshold("temperature > 30"),
            Some((ComparisonOperator::GreaterThan, 30.0))
        );
    }

    #[test]
    fn test_nl_to_dsl_extract_duration() {
        assert_eq!(
            NlToDslConverter::extract_duration("持续5分钟"),
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(
            NlToDslConverter::extract_duration("for 30 seconds"),
            Some(std::time::Duration::from_secs(30))
        );
    }

    #[test]
    fn test_find_match() {
        let options = vec!["sensor".to_string(), "temperature".to_string()];
        assert_eq!(
            NlToDslConverter::find_match("检查sensor状态", &options),
            Some("sensor".to_string())
        );
        assert_eq!(
            NlToDslConverter::find_match("temperature太高", &options),
            Some("temperature".to_string())
        );
        assert_eq!(NlToDslConverter::find_match("unknown", &options), None);
    }

    #[test]
    fn test_dsl_translator_basic() {
        let dsl = r#"
            RULE "高温告警"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "温度过高"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        let desc = DslTranslator::translate_rule(&rule, Language::Chinese);

        assert_eq!(desc.name, "高温告警");
        assert!(desc.condition_description.contains("sensor"));
        assert!(desc.condition_description.contains("temperature"));
        assert_eq!(desc.actions_description.len(), 1);
    }

    #[test]
    fn test_mdl_translator_capabilities() {
        use neomind_devices::mdl::MetricDataType;

        let device = DeviceTypeDefinition {
            device_type: "test_sensor".to_string(),
            name: "Test Sensor".to_string(),
            description: "A test sensor".to_string(),
            categories: vec!["sensor".to_string()],
            mode: neomind_devices::mdl_format::DeviceTypeMode::Full,
            uplink: neomind_devices::mdl_format::UplinkConfig {
                metrics: vec![MetricDefinition {
                    name: "temperature".to_string(),
                    display_name: "温度".to_string(),
                    unit: "°C".to_string(),
                    data_type: MetricDataType::Float,
                    min: Some(-40.0),
                    max: Some(120.0),
                    required: false,
                }],
                samples: vec![],
            },
            downlink: neomind_devices::mdl_format::DownlinkConfig { commands: vec![] },
        };

        let desc = MdlTranslator::translate_device(&device, Language::Chinese);
        assert!(!desc.capabilities.is_empty());
        assert_eq!(desc.metrics.len(), 1);
        assert_eq!(desc.metrics[0].name, "temperature");
    }
}
