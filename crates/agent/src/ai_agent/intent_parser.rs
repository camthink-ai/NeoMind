//! Intent parser for understanding user natural language requirements.

use edge_ai_storage::{IntentType, ParsedIntent};
use serde::{Deserialize, Serialize};

/// Intent parser for extracting structured intent from natural language.
pub struct IntentParser;

impl IntentParser {
    /// Create a new intent parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse user intent from natural language description.
    pub fn parse(&self, user_prompt: &str) -> Result<ParsedIntent, ParseError> {
        let prompt_lower = user_prompt.to_lowercase();

        // Detect intent type
        let intent_type = self.detect_intent_type(&prompt_lower);

        // Extract target metrics
        let target_metrics = self.extract_metrics(&prompt_lower);

        // Extract conditions
        let conditions = self.extract_conditions(&prompt_lower);

        // Extract actions
        let actions = self.extract_actions(&prompt_lower);

        // Calculate confidence
        let confidence = self.calculate_confidence(&intent_type, &target_metrics, &conditions);

        Ok(ParsedIntent {
            intent_type,
            target_metrics,
            conditions,
            actions,
            confidence,
        })
    }

    /// Detect the type of intent from the user prompt.
    fn detect_intent_type(&self, prompt: &str) -> IntentType {
        // Keywords for different intent types
        let report_keywords = ["报告", "汇总", "总结", "日报", "周报", "生成报告"];
        let anomaly_keywords = ["异常", "检测", "异常检测", "偏离", "不正常"];
        let control_keywords = ["控制", "开关", "打开", "关闭", "执行命令", "调节"];
        let automation_keywords = ["自动化", "联动", "自动", "多条件", "级联"];

        // Check for report generation
        if report_keywords.iter().any(|kw| prompt.contains(kw)) {
            return IntentType::ReportGeneration;
        }

        // Check for anomaly detection
        if anomaly_keywords.iter().any(|kw| prompt.contains(kw)) {
            return IntentType::AnomalyDetection;
        }

        // Check for control
        if control_keywords.iter().any(|kw| prompt.contains(kw)) {
            return IntentType::Control;
        }

        // Check for automation
        if automation_keywords.iter().any(|kw| prompt.contains(kw)) {
            return IntentType::Automation;
        }

        // Default to monitoring
        IntentType::Monitoring
    }

    /// Extract target metrics from the user prompt.
    fn extract_metrics(&self, prompt: &str) -> Vec<String> {
        let mut metrics = Vec::new();

        // Common metric keywords
        let metric_mappings = [
            ("温度", "temperature"),
            ("湿度", "humidity"),
            ("气压", "pressure"),
            ("光照", "illuminance"),
            ("能耗", "power"),
            ("功率", "power"),
            ("电量", "energy"),
            ("aqi", "aqi"),
            ("空气质量", "air_quality"),
            ("二氧化碳", "co2"),
            ("pm2.5", "pm25"),
            ("pm10", "pm10"),
            ("运动", "motion"),
            ("开关", "state"),
            ("门磁", "door"),
            ("窗户", "window"),
        ];

        for (keyword, metric) in metric_mappings {
            if prompt.contains(keyword) {
                if !metrics.contains(&metric.to_string()) {
                    metrics.push(metric.to_string());
                }
            }
        }

        // If no metrics found, add temperature as default for monitoring
        if metrics.is_empty() {
            metrics.push("temperature".to_string());
        }

        metrics
    }

    /// Extract conditions from the user prompt.
    fn extract_conditions(&self, prompt: &str) -> Vec<String> {
        let mut conditions = Vec::new();

        // Pattern: "大于X", "小于X", "超过X", "低于X", "高于X"
        let comparison_patterns = [
            ("大于", ">"),
            ("小于", "<"),
            ("超过", ">"),
            ("低于", "<"),
            ("高于", ">"),
            ("等于", "=="),
        ];

        for (keyword, operator) in comparison_patterns {
            if let Some(pos) = prompt.find(keyword) {
                let after = &prompt[pos..];
                // Try to extract a number
                // Note: keyword.chars().count() gives us the character count, not byte length
                let keyword_char_count = keyword.chars().count();
                let number_match: Vec<char> = after
                    .chars()
                    .skip(keyword_char_count)
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();

                if !number_match.is_empty() {
                    let number: String = number_match.iter().collect();
                    conditions.push(format!("{}{}", operator, number));
                }
            }
        }

        // Range patterns: "X到Y之间", "X-Y之间"
        if let Some(start) = prompt.find("到") {
            let before = &prompt[..start];
            let after = &prompt[start + 3..];

            // Try to extract numbers
            let before_numbers: Vec<String> = before
                .split(|c: char| !c.is_ascii_digit() && c != '.')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            let after_numbers: Vec<String> = after
                .split(|c: char| !c.is_ascii_digit() && c != '.')
                .filter(|s| !s.is_empty())
                .take(1)
                .map(|s| s.to_string())
                .collect();

            if !before_numbers.is_empty() && !after_numbers.is_empty() {
                conditions.push(format!(
                    "between {} and {}",
                    before_numbers.last().unwrap(),
                    after_numbers.first().unwrap()
                ));
            }
        }

        conditions
    }

    /// Extract actions from the user prompt.
    fn extract_actions(&self, prompt: &str) -> Vec<String> {
        let mut actions = Vec::new();

        let action_keywords = [
            ("报警", "send_alert"),
            ("通知", "send_notification"),
            ("发送消息", "send_notification"),
            ("打开", "turn_on"),
            ("关闭", "turn_off"),
            ("调节", "adjust"),
            ("生成报告", "generate_report"),
            ("记录", "log_data"),
            ("控制", "send_command"),
        ];

        for (keyword, action) in action_keywords {
            if prompt.contains(keyword) {
                actions.push(action.to_string());
            }
        }

        actions
    }

    /// Calculate confidence in the parsing result.
    fn calculate_confidence(
        &self,
        intent_type: &IntentType,
        metrics: &[String],
        conditions: &[String],
    ) -> f32 {
        let mut confidence: f32 = 0.5; // Base confidence

        // Increase confidence if we found metrics
        if !metrics.is_empty() {
            confidence += 0.2;
        }

        // Increase confidence if we found conditions
        if !conditions.is_empty() {
            confidence += 0.2;
        }

        // Adjust based on intent type
        match intent_type {
            IntentType::Monitoring => confidence += 0.1,
            IntentType::ReportGeneration => confidence += 0.1,
            IntentType::AnomalyDetection => confidence += 0.0,
            IntentType::Control => confidence += 0.1,
            IntentType::Automation => confidence -= 0.1, // Complex, lower confidence
        }

        confidence.min(1.0)
    }
}

impl Default for IntentParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Error during intent parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParseError {
    /// Empty input
    EmptyInput,
    /// Unable to determine intent
    UnknownIntent,
    /// Invalid syntax
    InvalidSyntax(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_monitoring_intent() {
        let parser = IntentParser::new();
        let result = parser.parse("监控仓库温度，如果温度大于30度就报警");

        assert_eq!(result.unwrap().intent_type, IntentType::Monitoring);
    }

    #[test]
    fn test_parse_report_intent() {
        let parser = IntentParser::new();
        let result = parser.parse("每天生成能耗汇总报告");

        assert_eq!(result.unwrap().intent_type, IntentType::ReportGeneration);
    }

    #[test]
    fn test_extract_metrics() {
        let parser = IntentParser::new();
        let metrics = parser.extract_metrics("监控温度、湿度和能耗");

        assert!(metrics.contains(&"temperature".to_string()));
        assert!(metrics.contains(&"humidity".to_string()));
        assert!(metrics.contains(&"power".to_string()));
    }

    #[test]
    fn test_extract_conditions() {
        let parser = IntentParser::new();
        let conditions = parser.extract_conditions("温度大于30度");

        println!("Extracted conditions: {:?}", conditions);
        assert!(conditions.contains(&">30".to_string()), "Expected '>30' in conditions: {:?}", conditions);
    }
}
