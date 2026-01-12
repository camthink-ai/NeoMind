//! DSL (Domain Specific Language) knowledge base tools.
//!
//! These tools provide LLM access to rule definitions and history,
//! enabling it to understand rules and generate proper DSL.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use edge_ai_rules::{
    RuleEngine, CompiledRule, RuleId, RuleStatus,
    RuleHistoryStorage, HistoryFilter,
    dsl::{RuleDslParser, ComparisonOperator, RuleAction},
};
use edge_ai_tools::{Tool, ToolOutput, ToolError, error::Result as ToolResult, tool::{object_schema, string_property, boolean_property, number_property, array_property}};

/// ListRules tool - queries all rules with filtering.
pub struct ListRulesTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
}

impl ListRulesTool {
    /// Create a new ListRules tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Get rule summaries with optional filtering.
    async fn get_summaries(&self, status: Option<&str>, device_id: Option<&str>) -> Vec<RuleSummary> {
        let guard = self.engine.read().await;
        let engine = match guard.as_ref() {
            Some(e) => e,
            None => return Vec::new(),
        };

        let rules = engine.list_rules().await;

        rules
            .into_iter()
            .filter(|rule| {
                if let Some(s) = status {
                    let status_matches = match s.to_lowercase().as_str() {
                        "active" => rule.status == RuleStatus::Active,
                        "paused" => rule.status == RuleStatus::Paused,
                        "triggered" => rule.status == RuleStatus::Triggered,
                        "disabled" => rule.status == RuleStatus::Disabled,
                        _ => true,
                    };
                    if !status_matches {
                        return false;
                    }
                }
                if let Some(d) = device_id {
                    if rule.condition.device_id != d {
                        return false;
                    }
                }
                true
            })
            .map(|rule| RuleSummary::from_compiled(&rule))
            .collect()
    }
}

impl Default for ListRulesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListRulesTool {
    fn name(&self) -> &str {
        "list_rules"
    }

    fn description(&self) -> &str {
        "List all rules in the system with optional filtering by status or device ID. Use this to understand what rules are configured."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "status": string_property("Filter by rule status: 'active', 'paused', 'triggered', or 'disabled'. Optional."),
                "device_id": string_property("Filter by device ID that the rule monitors. Optional.")
            }),
            vec![]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let status = args["status"].as_str();
        let device_id = args["device_id"].as_str();

        let summaries = self.get_summaries(status, device_id).await;

        let result = serde_json::json!({
            "rules": summaries,
            "count": summaries.len(),
        });

        Ok(ToolOutput::success(result))
    }
}

/// GetRule tool - gets full rule definition.
pub struct GetRuleTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
}

impl GetRuleTool {
    /// Create a new GetRule tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Find a rule by ID.
    async fn find_rule(&self, rule_id: &str) -> Option<CompiledRule> {
        let guard = self.engine.read().await;
        let engine = guard.as_ref()?;
        let id = RuleId::from_string(rule_id).ok()?;
        engine.get_rule(&id).await
    }

    /// Generate DSL from a compiled rule.
    fn to_dsl(&self, rule: &CompiledRule) -> String {
        let mut dsl = format!("RULE \"{}\"\n", rule.name);

        // WHEN clause
        dsl.push_str(&format!(
            "WHEN {}.{} {} {}\n",
            rule.condition.device_id,
            rule.condition.metric,
            rule.condition.operator.as_str(),
            rule.condition.threshold
        ));

        // FOR clause (optional)
        if let Some(duration) = rule.for_duration {
            let secs = duration.as_secs();
            if secs % 3600 == 0 {
                dsl.push_str(&format!("FOR {} hours\n", secs / 3600));
            } else if secs % 60 == 0 {
                dsl.push_str(&format!("FOR {} minutes\n", secs / 60));
            } else {
                dsl.push_str(&format!("FOR {} seconds\n", secs));
            }
        }

        // DO clause
        dsl.push_str("DO\n");
        for action in &rule.actions {
            match action {
                RuleAction::Notify { message } => {
                    dsl.push_str(&format!("    NOTIFY \"{}\"\n", message));
                }
                RuleAction::Execute { device_id, command, params } => {
                    dsl.push_str(&format!("    EXECUTE {}.{}(", device_id, command));
                    let param_strs: Vec<String> = params
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    dsl.push_str(&param_strs.join(", "));
                    dsl.push_str(")\n");
                }
                RuleAction::Log { level, message, severity } => {
                    if let Some(sev) = severity {
                        dsl.push_str(&format!("    LOG {}, \"{}\", severity=\"{}\"\n", level, message, sev));
                    } else {
                        dsl.push_str(&format!("    LOG {}, \"{}\"\n", level, message));
                    }
                }
            }
        }
        dsl.push_str("END\n");

        dsl
    }
}

impl Default for GetRuleTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GetRuleTool {
    fn name(&self) -> &str {
        "get_rule"
    }

    fn description(&self) -> &str {
        "Get detailed definition of a specific rule including its DSL representation, condition, and actions."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("Rule ID (UUID format)")
            }),
            vec!["rule_id".to_string()]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let rule_id = args["rule_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id must be a string".to_string()))?;

        let rule = self.find_rule(rule_id)
            .await
            .ok_or_else(|| ToolError::NotFound(format!("Rule '{}' not found", rule_id)))?;

        let dsl = self.to_dsl(&rule);

        let result = serde_json::json!({
            "rule_id": rule.id.to_string(),
            "name": rule.name,
            "dsl": dsl,
            "status": format!("{:?}", rule.status),
            "condition": {
                "device_id": rule.condition.device_id,
                "metric": rule.condition.metric,
                "operator": rule.condition.operator.as_str(),
                "threshold": rule.condition.threshold
            },
            "for_duration": rule.for_duration.map(|d| d.as_secs()),
            "actions": rule.actions.len(),
            "trigger_count": rule.state.trigger_count,
            "last_triggered": rule.state.last_triggered,
            "created_at": rule.created_at,
        });

        Ok(ToolOutput::success(result))
    }
}

/// ExplainRule tool - converts DSL rule to natural language.
pub struct ExplainRuleTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
}

impl ExplainRuleTool {
    /// Create a new ExplainRule tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Find a rule by ID.
    async fn find_rule(&self, rule_id: &str) -> Option<CompiledRule> {
        let guard = self.engine.read().await;
        let engine = guard.as_ref()?;
        let id = RuleId::from_string(rule_id).ok()?;
        engine.get_rule(&id).await
    }

    /// Generate natural language explanation.
    fn explain(&self, rule: &CompiledRule, language: &str) -> RuleExplanation {
        let condition_desc = if language == "zh" {
            self.explain_condition_zh(rule)
        } else {
            self.explain_condition_en(rule)
        };

        let actions_desc = if language == "zh" {
            self.explain_actions_zh(rule)
        } else {
            self.explain_actions_en(rule)
        };

        let operator_desc = if language == "zh" {
            format!("条件：{}", condition_desc)
        } else {
            format!("Condition: {}", condition_desc)
        };

        RuleExplanation {
            rule_id: rule.id.to_string(),
            name: rule.name.clone(),
            status: format!("{:?}", rule.status),
            condition_description: condition_desc,
            operator_description: operator_desc,
            actions_description: actions_desc,
            trigger_count: rule.state.trigger_count,
            last_triggered: rule.state.last_triggered.map(|t| t.to_rfc3339()),
            usage_example: if language == "zh" {
                self.example_usage_zh(rule)
            } else {
                self.example_usage_en(rule)
            },
        }
    }

    fn explain_condition_zh(&self, rule: &CompiledRule) -> String {
        let duration_desc = if let Some(duration) = rule.for_duration {
            let secs = duration.as_secs();
            let time_str = if secs % 3600 == 0 {
                format!("{}小时", secs / 3600)
            } else if secs % 60 == 0 {
                format!("{}分钟", secs / 60)
            } else {
                format!("{}秒", secs)
            };
            format!("持续{}", time_str)
        } else {
            "立即".to_string()
        };

        let operator_desc = match rule.condition.operator {
            ComparisonOperator::GreaterThan => format!("大于 {}", rule.condition.threshold),
            ComparisonOperator::LessThan => format!("小于 {}", rule.condition.threshold),
            ComparisonOperator::GreaterEqual => format!("大于等于 {}", rule.condition.threshold),
            ComparisonOperator::LessEqual => format!("小于等于 {}", rule.condition.threshold),
            ComparisonOperator::Equal => format!("等于 {}", rule.condition.threshold),
            ComparisonOperator::NotEqual => format!("不等于 {}", rule.condition.threshold),
        };

        format!(
            "当设备'{}'的指标'{}'{}时，{}触发。",
            rule.condition.device_id,
            rule.condition.metric,
            operator_desc,
            duration_desc
        )
    }

    fn explain_condition_en(&self, rule: &CompiledRule) -> String {
        let duration_desc = if let Some(duration) = rule.for_duration {
            let secs = duration.as_secs();
            let time_str = if secs % 3600 == 0 {
                format!("{} hours", secs / 3600)
            } else if secs % 60 == 0 {
                format!("{} minutes", secs / 60)
            } else {
                format!("{} seconds", secs)
            };
            format!("for {}", time_str)
        } else {
            "immediately".to_string()
        };

        let operator_desc = match rule.condition.operator {
            ComparisonOperator::GreaterThan => format!("greater than {}", rule.condition.threshold),
            ComparisonOperator::LessThan => format!("less than {}", rule.condition.threshold),
            ComparisonOperator::GreaterEqual => format!("greater than or equal to {}", rule.condition.threshold),
            ComparisonOperator::LessEqual => format!("less than or equal to {}", rule.condition.threshold),
            ComparisonOperator::Equal => format!("equal to {}", rule.condition.threshold),
            ComparisonOperator::NotEqual => format!("not equal to {}", rule.condition.threshold),
        };

        format!(
            "When metric '{}' on device '{}' is {}, trigger {}.",
            rule.condition.metric,
            rule.condition.device_id,
            operator_desc,
            duration_desc
        )
    }

    fn explain_actions_zh(&self, rule: &CompiledRule) -> String {
        if rule.actions.is_empty() {
            return "该规则不执行任何操作".to_string();
        }

        let mut parts = vec![format!("该规则触发时将执行{}个操作：", rule.actions.len())];
        for (i, action) in rule.actions.iter().enumerate() {
            match action {
                RuleAction::Notify { message } => {
                    parts.push(format!("{}. 发送通知：{}", i + 1, message));
                }
                RuleAction::Execute { device_id, command, params } => {
                    let param_str = if params.is_empty() {
                        String::new()
                    } else {
                        format!("，参数：{:?}", params)
                    };
                    parts.push(format!("{}. 执行设备命令：{}.{}{}", i + 1, device_id, command, param_str));
                }
                RuleAction::Log { level, .. } => {
                    parts.push(format!("{}. 记录日志，级别：{}", i + 1, level));
                }
            }
        }
        parts.join("\n")
    }

    fn explain_actions_en(&self, rule: &CompiledRule) -> String {
        if rule.actions.is_empty() {
            return "This rule does not execute any actions".to_string();
        }

        let mut parts = vec![format!("This rule executes {} actions when triggered:", rule.actions.len())];
        for (i, action) in rule.actions.iter().enumerate() {
            match action {
                RuleAction::Notify { message } => {
                    parts.push(format!("{}. Send notification: {}", i + 1, message));
                }
                RuleAction::Execute { device_id, command, params } => {
                    let param_str = if params.is_empty() {
                        String::new()
                    } else {
                        format!(" with params {:?}", params)
                    };
                    parts.push(format!("{}. Execute command: {}.{}{}", i + 1, device_id, command, param_str));
                }
                RuleAction::Log { level, .. } => {
                    parts.push(format!("{}. Log message at level: {}", i + 1, level));
                }
            }
        }
        parts.join("\n")
    }

    fn example_usage_zh(&self, rule: &CompiledRule) -> String {
        format!(
            "示例：规则'{}'已触发{}次。",
            rule.name,
            rule.state.trigger_count
        )
    }

    fn example_usage_en(&self, rule: &CompiledRule) -> String {
        format!(
            "Example: Rule '{}' has been triggered {} times.",
            rule.name,
            rule.state.trigger_count
        )
    }
}

impl Default for ExplainRuleTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExplainRuleTool {
    fn name(&self) -> &str {
        "explain_rule"
    }

    fn description(&self) -> &str {
        "Explain a rule in natural language (Chinese or English). Converts DSL rule definitions to human-readable descriptions."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("Rule ID (UUID format)"),
                "language": string_property("Output language: 'zh' for Chinese, 'en' for English. Defaults to 'zh'.")
            }),
            vec!["rule_id".to_string()]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let rule_id = args["rule_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id must be a string".to_string()))?;

        let language = args["language"].as_str().unwrap_or("zh");

        let rule = self.find_rule(rule_id)
            .await
            .ok_or_else(|| ToolError::NotFound(format!("Rule '{}' not found", rule_id)))?;

        let explanation = self.explain(&rule, language);

        Ok(ToolOutput::success(serde_json::to_value(explanation).unwrap()))
    }
}

/// GetRuleHistory tool - queries rule execution history.
pub struct GetRuleHistoryTool {
    /// History storage
    history: Arc<RwLock<Option<Arc<RuleHistoryStorage>>>>,
}

impl GetRuleHistoryTool {
    /// Create a new GetRuleHistory tool.
    pub fn new() -> Self {
        Self {
            history: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the history storage.
    pub async fn set_history(&self, storage: Arc<RuleHistoryStorage>) {
        let mut guard = self.history.write().await;
        *guard = Some(storage);
    }

    /// Query history with filters.
    async fn query_history(
        &self,
        rule_id: Option<&str>,
        success: Option<bool>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Vec<HistoryEntry> {
        let guard = self.history.read().await;
        let storage = match guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut filter = HistoryFilter::new();
        if let Some(rid) = rule_id {
            filter = filter.with_rule_id(rid);
        }
        if let Some(s) = success {
            filter = filter.with_success(s);
        }
        if let Some(l) = limit {
            filter = filter.with_limit(l);
        }
        if let Some(o) = offset {
            filter = filter.with_offset(o);
        }

        match storage.query(&filter).await {
            Ok(entries) => entries.into_iter().map(HistoryEntry::from_storage).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Get statistics for a rule.
    async fn get_stats(&self, rule_id: &str) -> Option<RuleStatistics> {
        let guard = self.history.read().await;
        let storage = guard.as_ref()?;
        let id = RuleId::from_string(rule_id).ok()?;
        storage.get_stats(&id).await.ok().map(RuleStatistics::from_storage)
    }
}

impl Default for GetRuleHistoryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GetRuleHistoryTool {
    fn name(&self) -> &str {
        "get_rule_history"
    }

    fn description(&self) -> &str {
        "Query rule execution history with optional filtering. Get statistics and past executions for specific rules."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("Filter by rule ID (UUID format). Optional."),
                "success": boolean_property("Filter by success status. Optional."),
                "limit": number_property("Maximum number of results to return. Optional."),
                "offset": number_property("Offset for pagination. Optional."),
                "include_stats": boolean_property("Include statistics for the rule. Optional.")
            }),
            vec![]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let rule_id = args["rule_id"].as_str();
        let success = args["success"].as_bool();
        let limit = args["limit"].as_u64().map(|v| v as usize);
        let offset = args["offset"].as_u64().map(|v| v as usize);
        let include_stats = args["include_stats"].as_bool().unwrap_or(false);

        let entries = self.query_history(rule_id, success, limit, offset).await;

        let mut result = serde_json::json!({
            "history": entries,
            "count": entries.len(),
        });

        if include_stats {
            if let Some(rid) = rule_id {
                if let Some(stats) = self.get_stats(rid).await {
                    result["stats"] = serde_json::to_value(stats).unwrap();
                }
            }
        }

        Ok(ToolOutput::success(result))
    }
}

/// Summary of a rule for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSummary {
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub name: String,
    /// Current status
    pub status: String,
    /// Device being monitored
    pub device_id: String,
    /// Metric being monitored
    pub metric: String,
    /// Number of actions
    pub actions_count: usize,
    /// Trigger count
    pub trigger_count: u64,
}

impl RuleSummary {
    fn from_compiled(rule: &CompiledRule) -> Self {
        Self {
            rule_id: rule.id.to_string(),
            name: rule.name.clone(),
            status: format!("{:?}", rule.status),
            device_id: rule.condition.device_id.clone(),
            metric: rule.condition.metric.clone(),
            actions_count: rule.actions.len(),
            trigger_count: rule.state.trigger_count,
        }
    }
}

/// Natural language explanation of a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExplanation {
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub name: String,
    /// Current status
    pub status: String,
    /// Condition description
    pub condition_description: String,
    /// Operator description
    pub operator_description: String,
    /// Actions description
    pub actions_description: String,
    /// Total trigger count
    pub trigger_count: u64,
    /// Last triggered time
    pub last_triggered: Option<String>,
    /// Usage example
    pub usage_example: String,
}

/// History entry for output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Entry ID
    pub id: String,
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub rule_name: String,
    /// Success status
    pub success: bool,
    /// Actions executed
    pub actions_executed: Vec<String>,
    /// Error if any
    pub error: Option<String>,
    /// Duration in ms
    pub duration_ms: u64,
    /// Timestamp
    pub timestamp: String,
}

impl HistoryEntry {
    fn from_storage(entry: edge_ai_rules::RuleHistoryEntry) -> Self {
        Self {
            id: entry.id,
            rule_id: entry.rule_id,
            rule_name: entry.rule_name,
            success: entry.success,
            actions_executed: entry.actions_executed,
            error: entry.error,
            duration_ms: entry.duration_ms,
            timestamp: entry.timestamp.to_rfc3339(),
        }
    }
}

/// Rule statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleStatistics {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Success rate percentage
    pub success_rate: f64,
    /// Average duration
    pub avg_duration_ms: f64,
    /// Min duration
    pub min_duration_ms: u64,
    /// Max duration
    pub max_duration_ms: u64,
}

impl RuleStatistics {
    fn from_storage(stats: edge_ai_rules::RuleHistoryStats) -> Self {
        Self {
            total_executions: stats.total_executions,
            successful_executions: stats.successful_executions,
            failed_executions: stats.failed_executions,
            success_rate: stats.success_rate(),
            avg_duration_ms: stats.avg_duration_ms,
            min_duration_ms: stats.min_duration_ms,
            max_duration_ms: stats.max_duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_rules::InMemoryValueProvider;

    #[tokio::test]
    async fn test_list_rules_empty() {
        let tool = ListRulesTool::new();
        let summaries = tool.get_summaries(None, None).await;
        assert!(summaries.is_empty());
    }

    #[tokio::test]
    async fn test_rule_summary_from_compiled() {
        let dsl = r#"
            RULE "Test Rule"
            WHEN sensor.temperature > 50
            DO
                NOTIFY "High temperature"
            END
        "#;

        let parsed = RuleDslParser::parse(dsl).unwrap();
        let compiled = CompiledRule::from_parsed(parsed);

        let summary = RuleSummary::from_compiled(&compiled);
        assert_eq!(summary.name, "Test Rule");
        assert_eq!(summary.device_id, "sensor");
        assert_eq!(summary.metric, "temperature");
        assert_eq!(summary.actions_count, 1);
    }

    #[tokio::test]
    async fn test_history_entry_from_storage() {
        let storage_entry = edge_ai_rules::RuleHistoryEntry {
            id: "test-1".to_string(),
            rule_id: RuleId::new().to_string(),
            rule_name: "Test Rule".to_string(),
            success: true,
            actions_executed: vec!["notify:test".to_string()],
            error: None,
            duration_ms: 100,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };

        let entry = HistoryEntry::from_storage(storage_entry);
        assert_eq!(entry.id, "test-1");
        assert_eq!(entry.rule_name, "Test Rule");
        assert!(entry.success);
        assert_eq!(entry.duration_ms, 100);
    }

    #[test]
    fn test_comparison_operators() {
        assert_eq!(ComparisonOperator::GreaterThan.as_str(), ">");
        assert_eq!(ComparisonOperator::LessThan.as_str(), "<");
        assert_eq!(ComparisonOperator::GreaterEqual.as_str(), ">=");
        assert_eq!(ComparisonOperator::LessEqual.as_str(), "<=");
        assert_eq!(ComparisonOperator::Equal.as_str(), "==");
        assert_eq!(ComparisonOperator::NotEqual.as_str(), "!=");
    }
}
