//! Rule DSL (Domain Specific Language) parser and compiler.
//!
//! The DSL allows defining rules in a human-readable format:
//!
//! ## Simple Rule
//! ```text
//! RULE "高温告警"
//! WHEN sensor.temperature > 50
//! FOR 5 minutes
//! DO
//!     NOTIFY "设备温度过高: {temperature}°C"
//!     EXECUTE device.fan(speed=100)
//!     LOG alert, severity="high"
//! END
//! ```
//!
//! ## Complex Rule with AND/OR
//! ```text
//! RULE "复合条件告警"
//! WHEN (sensor.temperature > 30) AND (sensor.humidity < 20)
//! DO
//!     NOTIFY "温度高且湿度低"
//!     EXECUTE device.humidifier(on=true)
//! END
//! ```
//!
//! ## Rule with Range Condition
//! ```text
//! RULE "温度范围告警"
//! WHEN sensor.temperature BETWEEN 20 AND 25
//! DO
//!     NOTIFY "温度在舒适范围内"
//! END
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Parsed rule from DSL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedRule {
    /// Rule name.
    pub name: String,
    /// Condition to evaluate.
    pub condition: RuleCondition,
    /// Duration for condition to be true before triggering.
    pub for_duration: Option<Duration>,
    /// Actions to execute when rule triggers.
    pub actions: Vec<RuleAction>,
    /// Rule description (optional).
    pub description: Option<String>,
    /// Rule tags (optional).
    pub tags: Vec<String>,
}

/// Rule condition - supports simple and complex conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCondition {
    /// Simple condition: device.metric operator value
    Simple {
        device_id: String,
        metric: String,
        operator: ComparisonOperator,
        threshold: f64,
    },
    /// Range condition: value BETWEEN min AND max
    Range {
        device_id: String,
        metric: String,
        min: f64,
        max: f64,
    },
    /// Logical AND of multiple conditions
    And(Vec<RuleCondition>),
    /// Logical OR of multiple conditions
    Or(Vec<RuleCondition>),
    /// Logical NOT
    Not(Box<RuleCondition>),
}

impl RuleCondition {
    /// Get all device/metric pairs referenced in this condition.
    pub fn get_device_metrics(&self) -> Vec<(String, String)> {
        match self {
            RuleCondition::Simple { device_id, metric, .. } => {
                vec![(device_id.clone(), metric.clone())]
            }
            RuleCondition::Range { device_id, metric, .. } => {
                vec![(device_id.clone(), metric.clone())]
            }
            RuleCondition::And(conditions) | RuleCondition::Or(conditions) => {
                conditions.iter().flat_map(|c| c.get_device_metrics()).collect()
            }
            RuleCondition::Not(condition) => condition.get_device_metrics(),
        }
    }
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    GreaterEqual,
    LessEqual,
    Equal,
    NotEqual,
}

impl ComparisonOperator {
    /// Evaluate the comparison.
    pub fn evaluate(&self, left: f64, right: f64) -> bool {
        match self {
            Self::GreaterThan => left > right,
            Self::LessThan => left < right,
            Self::GreaterEqual => left >= right,
            Self::LessEqual => left <= right,
            Self::Equal => (left - right).abs() < 0.0001,
            Self::NotEqual => (left - right).abs() >= 0.0001,
        }
    }

    /// Get operator as string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::GreaterThan => ">",
            Self::LessThan => "<",
            Self::GreaterEqual => ">=",
            Self::LessEqual => "<=",
            Self::Equal => "==",
            Self::NotEqual => "!=",
        }
    }
}

/// Rule action to execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleAction {
    /// Send a notification.
    Notify {
        message: String,
        channels: Option<Vec<String>>,
    },
    /// Execute a device command.
    Execute {
        device_id: String,
        command: String,
        params: HashMap<String, serde_json::Value>,
    },
    /// Log a message.
    Log {
        level: LogLevel,
        message: String,
        severity: Option<String>,
    },
    /// Set a device property/value.
    Set {
        device_id: String,
        property: String,
        value: serde_json::Value,
    },
    /// Delay execution.
    Delay {
        duration: Duration,
    },
    /// Create an alert.
    CreateAlert {
        title: String,
        message: String,
        severity: AlertSeverity,
    },
    /// Send HTTP request.
    HttpRequest {
        method: HttpMethod,
        url: String,
        headers: Option<HashMap<String, String>>,
        body: Option<String>,
    },
}

/// HTTP methods for HttpRequest action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// Alert severity for CreateAlert action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Log level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Alert,
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alert => write!(f, "alert"),
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Rule DSL parser.
pub struct RuleDslParser;

impl RuleDslParser {
    /// Preprocess DSL string to handle common LLM output formats.
    /// Handles:
    /// - Markdown code blocks (```...```)
    /// - Extra whitespace
    /// - Lowercase keywords (Rule, When, Do, End -> RULE, WHEN, DO, END)
    /// - JSON escaping
    fn preprocess(input: &str) -> String {
        let mut processed = input.to_string();

        // Remove markdown code blocks
        if processed.contains("```") {
            let mut result = String::new();
            let mut in_code_block = false;

            for line in processed.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("```") {
                    in_code_block = !in_code_block;
                } else if in_code_block {
                    result.push_str(line);
                    result.push('\n');
                } else if !trimmed.starts_with("```") && !result.is_empty() {
                    // Content before any code block
                    result.push_str(line);
                    result.push('\n');
                }
            }
            // If we found code blocks, use the extracted content
            if processed.contains("```") {
                processed = result;
            }
        }

        // Remove JSON string escaping if present (quotes around entire DSL)
        let trimmed = processed.trim();
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            if let Ok(unescaped) = serde_json::from_str::<String>(trimmed) {
                processed = unescaped;
            } else {
                // Simple quote removal - unescape JSON escapes
                let inner = &trimmed[1..trimmed.len() - 1];
                processed = inner.replace("\\\"", "\"");
            }
        } else {
            // Not wrapped in quotes, but may still have escaped quotes
            processed = processed.replace("\\\"", "\"");
        }

        // Handle JSON escape sequences like \n, \t
        processed = processed.replace("\\n", "\n");
        processed = processed.replace("\\t", "\t");
        processed = processed.replace("\\r", "\r");

        // Convert keywords to uppercase (preserving rest of line)
        // This works for both "Rule" at line start and indented actions like "  notify"
        fn normalize_keywords(line: &str) -> String {
            let mut result = String::new();
            let mut chars = line.chars().peekable();
            let mut start = 0;

            // Find first non-whitespace position
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                result.push(chars.next().unwrap());
                start += 1;
            }

            let rest = &line[start..];
            let upper = rest.to_uppercase();

            // Extended keyword list including new actions
            for keyword in &[
                "RULE", "WHEN", "FOR", "DO", "END",
                "NOTIFY", "EXECUTE", "LOG", "SET", "DELAY",
                "TRIGGER", "WORKFLOW", "ALERT", "HTTP",
                "DESCRIPTION", "TAGS",
            ] {
                let keyword_with_space = format!("{} ", keyword);
                if upper.starts_with(&keyword_with_space) || upper == *keyword {
                    // Found keyword - convert to uppercase
                    result.push_str(keyword);
                    if let Some(remaining) = rest.get(keyword.len()..) {
                        // Handle space after keyword
                        if remaining.starts_with(' ') {
                            result.push(' ');
                            result.push_str(&remaining[1..]);
                        } else {
                            result.push_str(remaining);
                        }
                    }
                    return result;
                }
            }

            // No keyword found, keep original
            result.push_str(rest);
            result
        }

        let lines: Vec<String> = processed
            .lines()
            .map(normalize_keywords)
            .map(|l| l.trim().to_string())
            .collect();

        lines.join("\n")
    }

    /// Parse a rule from DSL string.
    pub fn parse(input: &str) -> Result<ParsedRule, RuleError> {
        let preprocessed = Self::preprocess(input);
        let mut lines: Vec<&str> = preprocessed.lines().collect();

        // Find and extract the rule name
        let (name, mut description, mut tags) = Self::extract_rule_header(&mut lines)?;

        // Find and parse the WHEN clause (now supports complex conditions)
        let condition = Self::parse_when_clause(&mut lines)?;

        // Find and parse the FOR clause (optional)
        let for_duration = Self::parse_for_clause(&mut lines);

        // Find and parse the DO clause actions
        let actions = Self::parse_do_clause(&mut lines)?;

        Ok(ParsedRule {
            name,
            condition,
            for_duration,
            actions,
            description: if description.is_empty() { None } else { Some(description) },
            tags,
        })
    }

    /// Extract rule name and optional description/tags from RULE line.
    fn extract_rule_header(lines: &mut Vec<&str>) -> Result<(String, String, Vec<String>), RuleError> {
        let mut name = String::new();
        let mut description = String::new();
        let mut tags = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("RULE") {
                let rest = line[4..].trim(); // Skip "RULE"
                if let Some(rule_name) = Self::extract_quoted_string(rest) {
                    name = rule_name;
                }

                // Check for DESCRIPTION keyword on same or next lines
                let mut idx = i + 1;
                while idx < lines.len() {
                    let next_line = lines[idx].trim();
                    if next_line.starts_with("DESCRIPTION") {
                        if let Some(desc) = Self::extract_quoted_string(&next_line[11..]) {
                            description = desc;
                        }
                        lines.remove(idx);
                        continue;
                    } else if next_line.starts_with("TAGS") {
                        let tags_str = &next_line[4..].trim();
                        tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
                        lines.remove(idx);
                        continue;
                    } else if next_line.starts_with("WHEN") {
                        break;
                    }
                    idx += 1;
                }

                lines.remove(i);
                return Ok((name, description, tags));
            }
        }
        Err(RuleError::Parse("Rule name not found".to_string()))
    }

    /// Parse WHEN clause - now supports complex conditions with AND/OR/NOT/BETWEEN.
    fn parse_when_clause(lines: &mut Vec<&str>) -> Result<RuleCondition, RuleError> {
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("WHEN") {
                let condition_str = line[4..].trim(); // Skip "WHEN"
                lines.remove(i);
                return Self::parse_condition(condition_str);
            }
        }
        Err(RuleError::Parse("WHEN clause not found".to_string()))
    }

    /// Parse condition - supports simple, range, and logical expressions.
    fn parse_condition(input: &str) -> Result<RuleCondition, RuleError> {
        let input = input.trim();

        // Handle NOT first (highest precedence)
        let upper = input.to_uppercase();
        if upper.starts_with("NOT ") {
            let inner = &input[4..].trim();
            let condition = Self::parse_condition(inner)?;
            return Ok(RuleCondition::Not(Box::new(condition)));
        }

        // Handle BETWEEN ... AND ... (before AND/OR, since it contains AND)
        if let Some(between_pos) = upper.find(" BETWEEN ") {
            let left_part = &input[..between_pos];
            let after_between = &input[between_pos + 9..].trim();

            if let Some(and_pos) = after_between.to_uppercase().find(" AND ") {
                let (device_id, metric) = Self::parse_device_metric(left_part.trim())?;

                let min_str = after_between[..and_pos].trim();
                let max_str = after_between[and_pos + 5..].trim();

                let min = min_str.parse().map_err(|_| {
                    RuleError::Parse(format!("Invalid min value: {}", min_str))
                })?;
                let max = max_str.parse().map_err(|_| {
                    RuleError::Parse(format!("Invalid max value: {}", max_str))
                })?;

                return Ok(RuleCondition::Range { device_id, metric, min, max });
            }
        }

        // Handle parenthesized expressions
        if input.starts_with('(') {
            if let Some(close_pos) = Self::find_matching_paren(input, 0) {
                let inner = &input[1..close_pos];
                let rest = input[close_pos + 1..].trim();

                // Check for AND/OR after the parenthesized expression
                if rest.to_uppercase().starts_with("AND ") {
                    let left = Self::parse_condition(inner)?;
                    let right_str = rest[3..].trim(); // Skip "AND"
                    let right = Self::parse_condition(right_str)?;
                    return Ok(RuleCondition::And(vec![left, right]));
                } else if rest.to_uppercase().starts_with("OR ") {
                    let left = Self::parse_condition(inner)?;
                    let right_str = rest[2..].trim(); // Skip "OR"
                    let right = Self::parse_condition(right_str)?;
                    return Ok(RuleCondition::Or(vec![left, right]));
                } else {
                    // Just a parenthesized condition
                    return Self::parse_condition(inner);
                }
            }
        }

        // Handle AND (higher precedence than OR)
        if let Some(pos) = Self::find_operator_ignore_parens(input, "AND") {
            let left = Self::parse_condition(&input[..pos])?;
            let right = Self::parse_condition(&input[pos + 5..].trim())?;
            return Ok(RuleCondition::And(vec![left, right]));
        }

        // Handle OR (lower precedence than AND)
        if let Some(pos) = Self::find_operator_ignore_parens(input, "OR") {
            let left = Self::parse_condition(&input[..pos])?;
            let right = Self::parse_condition(&input[pos + 4..].trim())?;
            return Ok(RuleCondition::Or(vec![left, right]));
        }

        // Simple condition
        let (device_id, metric, operator, threshold) = Self::parse_simple_condition(input)?;
        Ok(RuleCondition::Simple { device_id, metric, operator, threshold })
    }

    /// Find matching closing parenthesis.
    fn find_matching_paren(input: &str, start: usize) -> Option<usize> {
        let mut depth = 0;
        for (i, c) in input.chars().enumerate() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Find operator position ignoring parentheses.
    /// Searches for op with space on both sides (e.g., " AND ", " OR ").
    fn find_operator_ignore_parens(input: &str, op: &str) -> Option<usize> {
        let target = format!(" {} ", op); // " AND ", " OR "
        let upper_input = input.to_uppercase();
        let upper_target = target.to_uppercase();

        let mut depth = 0;
        let bytes = input.as_bytes();

        for i in 0..bytes.len() {
            let c = bytes[i];
            if c == b'(' {
                depth += 1;
            } else if c == b')' {
                depth -= 1;
            } else if depth == 0 {
                // Check if we're at the start of the target operator
                if i + upper_target.len() <= upper_input.len() {
                    let slice = &upper_input[i..i + upper_target.len()];
                    if slice == upper_target {
                        return Some(i);
                    }
                }
            }
        }

        None
    }

    /// Parse device.metric from a condition string.
    fn parse_device_metric(input: &str) -> Result<(String, String), RuleError> {
        let parts: Vec<&str> = input.trim().split('.').collect();
        if parts.len() >= 2 {
            Ok((parts[0].to_string(), parts[1].to_string()))
        } else {
            // No device specified, use the whole thing as metric
            Ok((String::new(), parts[0].to_string()))
        }
    }

    /// Parse simple condition like "device.metric > 50".
    fn parse_simple_condition(
        input: &str,
    ) -> Result<(String, String, ComparisonOperator, f64), RuleError> {
        let input = input.trim();

        // Try each operator in order of specificity
        let op_patterns = [
            (">=", ComparisonOperator::GreaterEqual),
            ("<=", ComparisonOperator::LessEqual),
            ("==", ComparisonOperator::Equal),
            ("!=", ComparisonOperator::NotEqual),
            (">", ComparisonOperator::GreaterThan),
            ("<", ComparisonOperator::LessThan),
        ];

        for (op_str, op) in &op_patterns {
            if let Some((left, right)) = input.split_once(op_str) {
                let (device_id, metric) = Self::parse_device_metric(left.trim())?;

                let threshold = right
                    .trim()
                    .parse()
                    .map_err(|_| RuleError::Parse(format!("Invalid threshold value: {}", right)))?;

                return Ok((device_id, metric, *op, threshold));
            }
        }

        Err(RuleError::Parse(format!("Invalid condition: {}", input)))
    }

    /// Parse FOR clause to extract duration.
    fn parse_for_clause(lines: &mut Vec<&str>) -> Option<Duration> {
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("FOR")
                && let Some(rest) = line.strip_prefix("FOR") {
                    let duration_str = rest.trim();
                    lines.remove(i);
                    return Self::parse_duration(duration_str);
                }
        }
        None
    }

    /// Parse DO clause to extract actions.
    fn parse_do_clause(lines: &mut Vec<&str>) -> Result<Vec<RuleAction>, RuleError> {
        let mut actions = Vec::new();
        let mut in_do_block = false;

        for line in lines.iter() {
            if line.starts_with("DO") {
                in_do_block = true;
                continue;
            }
            if *line == "END" {
                break;
            }
            if in_do_block && !line.is_empty()
                && let Some(action) = Self::parse_action(line)? {
                    actions.push(action);
                }
        }

        Ok(actions)
    }

    /// Parse duration string like "5 minutes".
    fn parse_duration(input: &str) -> Option<Duration> {
        let input = input.trim();
        let mut parts = input.split_whitespace();

        if let (Some(num_str), Some(unit)) = (parts.next(), parts.next())
            && let Ok(value) = num_str.parse::<u64>() {
                let duration = match unit {
                    "second" | "seconds" => Duration::from_secs(value),
                    "minute" | "minutes" => Duration::from_secs(value * 60),
                    "hour" | "hours" => Duration::from_secs(value * 3600),
                    _ => return None,
                };
                return Some(duration);
            }

        None
    }

    /// Parse a single action line - supports all action types.
    fn parse_action(line: &str) -> Result<Option<RuleAction>, RuleError> {
        let line = line.trim();

        if line.is_empty() {
            return Ok(None);
        }

        // NOTIFY "message" [channel1, channel2, ...]
        if line.starts_with("NOTIFY") {
            let rest = &line[6..].trim(); // Skip "NOTIFY"
            if let Some(msg) = Self::extract_quoted_string(rest) {
                return Ok(Some(RuleAction::Notify {
                    message: msg.clone(),
                    channels: None, // TODO: Parse channels from rest of line
                }));
            }
        }

        // EXECUTE device.command(params...)
        if line.starts_with("EXECUTE") {
            let rest = line[7..].trim(); // Skip "EXECUTE"
            if let Some((device_cmd, params_part)) = rest.split_once('(') {
                let parts: Vec<&str> = device_cmd.trim().split('.').collect();
                if parts.len() == 2 {
                    let device_id = parts[0].to_string();
                    let command = parts[1].to_string();

                    let params_str = params_part.trim_end_matches(')').trim();
                    let params = Self::parse_params(params_str);

                    return Ok(Some(RuleAction::Execute {
                        device_id,
                        command,
                        params,
                    }));
                }
            }
        }

        // SET device.property = value (supports nested properties like device.fan.speed)
        if line.starts_with("SET") {
            let rest = line[3..].trim(); // Skip "SET"
            if let Some(eq_pos) = rest.find('=') {
                let left_part = &rest[..eq_pos].trim();
                let value_str = &rest[eq_pos + 1..].trim();

                let parts: Vec<&str> = left_part.split('.').collect();
                if parts.len() >= 2 {
                    // Last part is the property, everything before is the device_id
                    let property = parts.last().unwrap().to_string();
                    let device_id = parts[..parts.len() - 1].join(".");

                    let value = if let Ok(num) = value_str.parse::<i64>() {
                        serde_json::Value::Number(serde_json::Number::from(num))
                    } else if let Ok(num) = value_str.parse::<f64>() {
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(num).unwrap_or_else(|| serde_json::Number::from(0))
                        )
                    } else if *value_str == "true" {
                        serde_json::Value::Bool(true)
                    } else if *value_str == "false" {
                        serde_json::Value::Bool(false)
                    } else if value_str.starts_with('"') {
                        serde_json::Value::String(value_str[1..value_str.len() - 1].to_string())
                    } else {
                        serde_json::Value::String(value_str.to_string())
                    };

                    return Ok(Some(RuleAction::Set {
                        device_id,
                        property,
                        value,
                    }));
                }
            }
        }

        // DELAY duration
        if line.starts_with("DELAY") {
            let rest = line[5..].trim(); // Skip "DELAY"
            if let Some(duration) = Self::parse_duration(rest) {
                return Ok(Some(RuleAction::Delay { duration }));
            }
        }

        // ALERT "title" "message" severity
        if line.starts_with("ALERT") {
            let rest = line[5..].trim(); // Skip "ALERT"
            let parts = Self::extract_all_quoted_strings(rest);

            if parts.len() >= 2 {
                let title = parts[0].clone();
                let message = parts[1].clone();

                // Check for severity
                let remaining = &rest[rest.find('"').unwrap()..];
                let severity_str = if remaining.to_uppercase().contains(" CRITICAL") {
                    "CRITICAL"
                } else if remaining.to_uppercase().contains(" ERROR") {
                    "ERROR"
                } else if remaining.to_uppercase().contains(" WARNING") {
                    "WARNING"
                } else {
                    "INFO"
                };

                let severity = match severity_str {
                    "CRITICAL" => AlertSeverity::Critical,
                    "ERROR" => AlertSeverity::Error,
                    "WARNING" => AlertSeverity::Warning,
                    _ => AlertSeverity::Info,
                };

                return Ok(Some(RuleAction::CreateAlert { title, message, severity }));
            }
        }

        // HTTP GET/POST/PUT/DELETE/PATCH url
        if line.starts_with("HTTP") {
            let rest = line[4..].trim(); // Skip "HTTP"
            let parts: Vec<&str> = rest.split_whitespace().collect();

            if parts.len() >= 2 {
                let method = match parts[0].to_uppercase().as_str() {
                    "GET" => HttpMethod::Get,
                    "POST" => HttpMethod::Post,
                    "PUT" => HttpMethod::Put,
                    "DELETE" => HttpMethod::Delete,
                    "PATCH" => HttpMethod::Patch,
                    _ => HttpMethod::Get,
                };

                let url = parts[1].to_string();

                return Ok(Some(RuleAction::HttpRequest {
                    method,
                    url,
                    headers: None,
                    body: None,
                }));
            }
        }

        // LOG level [severity="..."] ["message"]
        if line.starts_with("LOG") {
            let rest = line[3..].trim(); // Skip "LOG"
            let level = if rest.to_uppercase().starts_with("ALERT") {
                LogLevel::Alert
            } else if rest.to_uppercase().starts_with("INFO") {
                LogLevel::Info
            } else if rest.to_uppercase().starts_with("WARNING") {
                LogLevel::Warning
            } else if rest.to_uppercase().starts_with("ERROR") {
                LogLevel::Error
            } else {
                LogLevel::Info
            };

            // Try to extract message
            let message = if let Some(msg) = Self::extract_quoted_string(rest) {
                msg
            } else {
                "Rule triggered".to_string()
            };

            let severity = if rest.contains("severity=") {
                Self::extract_quoted_string(rest)
            } else {
                None
            };

            return Ok(Some(RuleAction::Log {
                level,
                message,
                severity,
            }));
        }

        Ok(None)
    }

    /// Extract all quoted strings from input.
    fn extract_all_quoted_strings(input: &str) -> Vec<String> {
        let mut results = Vec::new();
        let mut chars = input.chars().peekable();
        let mut in_quotes = false;
        let mut current = String::new();
        let mut escape_next = false;

        while let Some(c) = chars.next() {
            if escape_next {
                current.push(c);
                escape_next = false;
                continue;
            }

            if c == '\\' {
                escape_next = true;
                continue;
            }

            if c == '"' {
                if in_quotes {
                    results.push(current.clone());
                    current.clear();
                    in_quotes = false;
                } else {
                    in_quotes = true;
                }
            } else if in_quotes {
                current.push(c);
            }
        }

        results
    }

    /// Extract string from quotes.
    fn extract_quoted_string(input: &str) -> Option<String> {
        let start = input.find('"')?;
        let end = input[start + 1..].find('"')?;
        Some(input[start + 1..start + 1 + end].to_string())
    }

    /// Parse parameters string like "speed=100, mode=auto".
    fn parse_params(input: &str) -> HashMap<String, serde_json::Value> {
        let mut params = HashMap::new();

        if input.is_empty() {
            return params;
        }

        for pair in input.split(',') {
            if let Some((key, value)) = pair.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim();

                let json_value = if value.starts_with('"') {
                    // String value
                    serde_json::Value::String(value.trim_matches('"').to_string())
                } else if let Ok(num) = value.parse::<i64>() {
                    // Integer value
                    serde_json::Value::Number(serde_json::Number::from(num))
                } else if let Ok(num) = value.parse::<f64>() {
                    // Float value
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(num)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    )
                } else if value == "true" {
                    serde_json::Value::Bool(true)
                } else if value == "false" {
                    serde_json::Value::Bool(false)
                } else {
                    // Identifier as string
                    serde_json::Value::String(value.to_string())
                };

                params.insert(key, json_value);
            }
        }

        params
    }
}

/// Rule compilation error.
#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Compilation error: {0}")]
    Compilation(String),

    #[error("Execution error: {0}")]
    Execution(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rule() {
        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "Temperature is high"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.name, "Test Rule");
        match &rule.condition {
            RuleCondition::Simple { device_id, metric, operator, threshold } => {
                assert_eq!(device_id, "sensor");
                assert_eq!(metric, "temperature");
                assert_eq!(*operator, ComparisonOperator::GreaterThan);
                assert_eq!(*threshold, 50.0);
            }
            _ => panic!("Expected Simple condition"),
        }
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn test_parse_rule_with_duration() {
        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            FOR 5 minutes
            DO
                NOTIFY "High temperature"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.for_duration, Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_parse_execute_action() {
        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                EXECUTE device.fan(speed=100)
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.actions.len(), 1);
        match &rule.actions[0] {
            RuleAction::Execute {
                device_id,
                command,
                params,
            } => {
                assert_eq!(device_id, "device");
                assert_eq!(command, "fan");
                assert_eq!(params.get("speed").and_then(|v| v.as_i64()), Some(100));
            }
            _ => panic!("Expected Execute action"),
        }
    }

    #[test]
    fn test_parse_multiple_actions() {
        let dsl = r#"
            RULE "Complex Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
                EXECUTE device.fan(speed=100)
                LOG info, severity="low"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.actions.len(), 3);
    }

    #[test]
    fn test_parse_and_condition() {
        let dsl = r#"
            RULE "And Condition"
            WHEN (sensor.temperature > 30) AND (sensor.humidity < 20)
            DO
                NOTIFY "High temp and low humidity"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        match &rule.condition {
            RuleCondition::And(conditions) => {
                assert_eq!(conditions.len(), 2);
            }
            _ => panic!("Expected And condition"),
        }
    }

    #[test]
    fn test_parse_or_condition() {
        let dsl = r#"
            RULE "Or Condition"
            WHEN sensor.temp > 50 OR sensor.temp < 10
            DO
                NOTIFY "Temperature out of range"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        match &rule.condition {
            RuleCondition::Or(conditions) => {
                assert_eq!(conditions.len(), 2);
            }
            _ => panic!("Expected Or condition"),
        }
    }

    #[test]
    fn test_parse_range_condition() {
        let dsl = r#"
            RULE "Range Condition"
            WHEN sensor.temperature BETWEEN 20 AND 25
            DO
                NOTIFY "Temperature in range"
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        match &rule.condition {
            RuleCondition::Range { device_id, metric, min, max } => {
                assert_eq!(device_id, "sensor");
                assert_eq!(metric, "temperature");
                assert_eq!(*min, 20.0);
                assert_eq!(*max, 25.0);
            }
            _ => panic!("Expected Range condition"),
        }
    }

    #[test]
    fn test_parse_set_action() {
        let dsl = r#"
            RULE "Set Action"
            WHEN sensor.temperature > 50
            DO
                SET device.fan.speed = 100
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        match &rule.actions[0] {
            RuleAction::Set { device_id, property, value } => {
                // device.fan.speed -> device_id="device.fan", property="speed"
                assert_eq!(device_id, "device.fan");
                assert_eq!(property, "speed");
                assert_eq!(value, &serde_json::json!(100));
            }
            _ => panic!("Expected Set action"),
        }
    }

    #[test]
    fn test_parse_delay_action() {
        let dsl = r#"
            RULE "Delay Action"
            WHEN sensor.temperature > 50
            DO
                DELAY 5 seconds
            END
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        match &rule.actions[0] {
            RuleAction::Delay { duration } => {
                assert_eq!(*duration, Duration::from_secs(5));
            }
            _ => panic!("Expected Delay action"),
        }
    }

    #[test]
    fn test_comparison_operators() {
        assert!(ComparisonOperator::GreaterThan.evaluate(10.0, 5.0));
        assert!(ComparisonOperator::LessThan.evaluate(5.0, 10.0));
        assert!(ComparisonOperator::GreaterEqual.evaluate(10.0, 10.0));
        assert!(ComparisonOperator::LessEqual.evaluate(10.0, 10.0));
        assert!(ComparisonOperator::Equal.evaluate(10.0, 10.0));
        assert!(ComparisonOperator::NotEqual.evaluate(10.0, 5.0));
    }

    #[test]
    fn test_all_comparison_operators_in_dsl() {
        let operators = [
            (">", ComparisonOperator::GreaterThan),
            ("<", ComparisonOperator::LessThan),
            (">=", ComparisonOperator::GreaterEqual),
            ("<=", ComparisonOperator::LessEqual),
            ("==", ComparisonOperator::Equal),
            ("!=", ComparisonOperator::NotEqual),
        ];

        for (op_str, expected_op) in operators {
            let dsl = format!(
                r#"
                    RULE "Test"
                    WHEN sensor.temp {} 50
                    DO
                        NOTIFY "Test"
                    END
                "#,
                op_str
            );

            let rule = RuleDslParser::parse(&dsl).unwrap();
            match &rule.condition {
                RuleCondition::Simple { operator, .. } => {
                    assert_eq!(*operator, expected_op);
                }
                _ => panic!("Expected Simple condition"),
            }
        }
    }

    #[test]
    fn test_preprocess_lowercase_keywords() {
        // Test that lowercase keywords are converted to uppercase
        let dsl = r#"
            rule "Test Rule"
            when sensor.temperature > 50
            do
                notify "High temperature"
            end
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.name, "Test Rule");
        match &rule.condition {
            RuleCondition::Simple { device_id, .. } => {
                assert_eq!(device_id, "sensor");
            }
            _ => panic!("Expected Simple condition"),
        }
    }

    #[test]
    fn test_preprocess_markdown_code_blocks() {
        // Test that markdown code blocks are removed
        let dsl = r#"```dsl
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
            END
            ```"#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.name, "Test Rule");
    }

    #[test]
    fn test_preprocess_json_string_wrapping() {
        // Test JSON unescaping - using \\n in JSON to represent newline
        let dsl_with_escapes = r#"RULE \"Test Rule\"\nWHEN sensor.temperature > 50\nDO NOTIFY \"High temperature\" END"#;

        let rule = RuleDslParser::parse(dsl_with_escapes).unwrap();
        assert_eq!(rule.name, "Test Rule");
    }

    #[test]
    fn test_preprocess_escaped_quotes() {
        // Test handling of escaped quotes (common in LLM tool responses)
        let dsl = r#"RULE \"Test Rule\"
WHEN sensor.temperature > 50
DO NOTIFY \"High temperature\" END"#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.name, "Test Rule");
    }

    #[test]
    fn test_preprocess_mixed_case_keywords() {
        // Test mixed case keywords (Rule, When, Do, End)
        let dsl = r#"
            Rule "Test Rule"
            When sensor.temperature > 50
            Do
                Notify "High temperature"
            End
        "#;

        let rule = RuleDslParser::parse(dsl).unwrap();
        assert_eq!(rule.name, "Test Rule");
        assert_eq!(rule.actions.len(), 1);
    }
}
