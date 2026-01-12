//! Rule DSL (Domain Specific Language) parser and compiler.
//!
//! The DSL allows defining rules in a human-readable format:
//!
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
}

/// Rule condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    /// Device identifier.
    pub device_id: String,
    /// Metric name.
    pub metric: String,
    /// Comparison operator.
    pub operator: ComparisonOperator,
    /// Threshold value.
    pub threshold: f64,
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
    Notify { message: String },
    /// Execute a device command.
    Execute { device_id: String, command: String, params: HashMap<String, serde_json::Value> },
    /// Log a message.
    Log { level: LogLevel, message: String, severity: Option<String> },
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
    /// Parse a rule from DSL string.
    pub fn parse(input: &str) -> Result<ParsedRule, RuleError> {
        let mut lines: Vec<&str> = input.lines().map(|l| l.trim()).collect();

        // Find and extract the rule name
        let name = Self::extract_rule_name(&mut lines)?;

        // Find and parse the WHEN clause
        let (device_id, metric, operator, threshold) = Self::parse_when_clause(&mut lines)?;

        // Find and parse the FOR clause (optional)
        let for_duration = Self::parse_for_clause(&mut lines);

        // Find and parse the DO clause actions
        let actions = Self::parse_do_clause(&mut lines)?;

        Ok(ParsedRule {
            name,
            condition: RuleCondition {
                device_id,
                metric,
                operator,
                threshold,
            },
            for_duration,
            actions,
        })
    }

    /// Extract rule name from RULE "name" line.
    fn extract_rule_name(lines: &mut Vec<&str>) -> Result<String, RuleError> {
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("RULE") {
                if let Some(rest) = line.strip_prefix("RULE") {
                    let name_part = rest.trim();
                    if let Some(name) = Self::extract_quoted_string(name_part) {
                        lines.remove(i);
                        return Ok(name);
                    }
                }
            }
        }
        Err(RuleError::Parse("Rule name not found".to_string()))
    }

    /// Parse WHEN clause to extract condition.
    fn parse_when_clause(
        lines: &mut Vec<&str>,
    ) -> Result<(String, String, ComparisonOperator, f64), RuleError> {
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("WHEN") {
                if let Some(rest) = line.strip_prefix("WHEN") {
                    let condition_str = rest.trim();
                    let (device_id, metric, operator, threshold) =
                        Self::parse_condition(condition_str)?;
                    lines.remove(i);
                    return Ok((device_id, metric, operator, threshold));
                }
            }
        }
        Err(RuleError::Parse("WHEN clause not found".to_string()))
    }

    /// Parse FOR clause to extract duration.
    fn parse_for_clause(lines: &mut Vec<&str>) -> Option<Duration> {
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("FOR") {
                if let Some(rest) = line.strip_prefix("FOR") {
                    let duration_str = rest.trim();
                    lines.remove(i);
                    return Self::parse_duration(duration_str);
                }
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
            if in_do_block && !line.is_empty() {
                if let Some(action) = Self::parse_action(line)? {
                    actions.push(action);
                }
            }
        }

        Ok(actions)
    }

    /// Parse condition string like "device.metric > 50".
    fn parse_condition(
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
                let parts: Vec<&str> = left.trim().split('.').collect();
                let (device_id, metric) = if parts.len() >= 2 {
                    (parts[0].to_string(), parts[1].to_string())
                } else {
                    (String::new(), parts[0].to_string())
                };

                let threshold = right.trim().parse().map_err(|_| {
                    RuleError::Parse(format!("Invalid threshold value: {}", right))
                })?;

                return Ok((device_id, metric, *op, threshold));
            }
        }

        Err(RuleError::Parse(format!("Invalid condition: {}", input)))
    }

    /// Parse duration string like "5 minutes".
    fn parse_duration(input: &str) -> Option<Duration> {
        let input = input.trim();
        let mut parts = input.split_whitespace();

        if let (Some(num_str), Some(unit)) = (parts.next(), parts.next()) {
            if let Ok(value) = num_str.parse::<u64>() {
                let duration = match unit {
                    "second" | "seconds" => Duration::from_secs(value),
                    "minute" | "minutes" => Duration::from_secs(value * 60),
                    "hour" | "hours" => Duration::from_secs(value * 3600),
                    _ => return None,
                };
                return Some(duration);
            }
        }

        None
    }

    /// Parse a single action line.
    fn parse_action(line: &str) -> Result<Option<RuleAction>, RuleError> {
        let line = line.trim();

        if line.is_empty() {
            return Ok(None);
        }

        if line.starts_with("NOTIFY") {
            if let Some(msg) = Self::extract_quoted_string(line) {
                return Ok(Some(RuleAction::Notify { message: msg }));
            }
        } else if line.starts_with("EXECUTE") {
            let rest = line[7..].trim(); // Skip "EXECUTE"
            if let Some((device_cmd, params_part)) = rest.split_once('(') {
                let parts: Vec<&str> = device_cmd.trim().split('.').collect();
                if parts.len() == 2 {
                    let device_id = parts[0].to_string();
                    let command = parts[1].to_string();

                    // Extract parameters part (inside parentheses)
                    let params_str = params_part.trim_end_matches(')').trim();
                    let params = Self::parse_params(params_str);

                    return Ok(Some(RuleAction::Execute {
                        device_id,
                        command,
                        params,
                    }));
                }
            }
        } else if line.starts_with("LOG") {
            let rest = line[3..].trim(); // Skip "LOG"
            let level = if rest.starts_with("alert") {
                LogLevel::Alert
            } else if rest.starts_with("info") {
                LogLevel::Info
            } else if rest.starts_with("warning") {
                LogLevel::Warning
            } else if rest.starts_with("error") {
                LogLevel::Error
            } else {
                LogLevel::Info
            };

            let message = "Rule triggered".to_string();

            // Check for severity parameter
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
                        serde_json::Number::from_f64(num).unwrap_or_else(|| serde_json::Number::from(0)),
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
        assert_eq!(rule.condition.device_id, "sensor");
        assert_eq!(rule.condition.metric, "temperature");
        assert_eq!(rule.condition.operator, ComparisonOperator::GreaterThan);
        assert_eq!(rule.condition.threshold, 50.0);
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
            assert_eq!(rule.condition.operator, expected_op);
        }
    }
}
