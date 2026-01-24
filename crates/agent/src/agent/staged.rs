//! Multi-stage agent for reduced thinking.
//!
//! Instead of sending all tools at once, this module implements a staged approach:
//! - Stage 1: Intent classification (no tools, minimal thinking)
//! - Stage 2: Tool selection (only relevant tools by namespace)
//! - Stage 3: Tool execution
//! - Stage 4: Response generation (optional)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Intent category for user queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentCategory {
    /// Device-related queries (list, control, query)
    Device,
    /// Rule-related queries (list, create, history)
    Rule,
    /// Workflow-related queries (list, trigger, status)
    Workflow,
    /// Data queries (time series, metrics)
    Data,
    /// Alert-related queries (list, acknowledge, status)
    Alert,
    /// System-related queries (status, health, configuration)
    System,
    /// Help and FAQ queries
    Help,
    /// General chat, greetings, unclear intent
    General,
}

impl IntentCategory {
    /// Get the namespace for this intent.
    pub fn namespace(&self) -> &'static str {
        match self {
            IntentCategory::Device => "device",
            IntentCategory::Rule => "rule",
            IntentCategory::Workflow => "workflow",
            IntentCategory::Data => "data",
            IntentCategory::Alert => "alert",
            IntentCategory::System => "system",
            IntentCategory::Help => "help",
            IntentCategory::General => "general",
        }
    }

    /// Get display name for this intent.
    pub fn display_name(&self) -> &'static str {
        match self {
            IntentCategory::Device => "设备管理",
            IntentCategory::Rule => "自动化规则",
            IntentCategory::Workflow => "工作流",
            IntentCategory::Data => "数据查询",
            IntentCategory::Alert => "告警管理",
            IntentCategory::System => "系统管理",
            IntentCategory::Help => "帮助中心",
            IntentCategory::General => "通用对话",
        }
    }

    /// Get keywords that trigger this intent.
    pub fn keywords(&self) -> &'static [&'static str] {
        match self {
            IntentCategory::Device => &[
                "设备",
                "device",
                "sensor",
                "传感器",
                "开关",
                "switch",
                "light",
                "lights",
                "lamp",
                "fan",
                "控制",
                "control",
                "turn on",
                "turn off",
                "open",
                "close",
                "list_devices",
                "get_device",
                "设备列表",
            ],
            IntentCategory::Rule => &[
                "规则",
                "rule",
                "自动化",
                "automation",
                "触发",
                "trigger",
                "list_rules",
                "create_rule",
                "规则列表",
                "创建规则",
                "create",
                "new",
                "add",
                "make",
            ],
            IntentCategory::Workflow => &[
                "工作流",
                "workflow",
                "流程",
                "场景",
                "scenario",
                "list_workflows",
                "trigger_workflow",
                "执行",
                "execute",
            ],
            IntentCategory::Data => &[
                "数据",
                "data",
                "查询",
                "query",
                "历史",
                "history",
                "metrics",
                "指标",
                "时间序列",
                "telemetry",
                "遥测",
                "温度",
                "temperature",
                "temp",
                "湿度",
                "humidity",
                "what's",
                "what is",
                "how much",
                "多少",
                "当前",
                "current",
            ],
            IntentCategory::Alert => &[
                "告警",
                "alert",
                "报警",
                "通知",
                "notification",
                "警告",
                "warning",
                "异常",
                "abnormal",
                "故障",
                "fault",
                "问题",
                "issue",
                "活跃告警",
                "active alerts",
                "确认告警",
                "acknowledge",
            ],
            IntentCategory::System => &[
                "系统",
                "system",
                "状态",
                "status",
                "健康",
                "health",
                "运行",
                "running",
                "正常",
                "ok",
                "版本",
                "version",
                "配置",
                "config",
                "设置",
                "settings",
                "服务器",
                "server",
                "连接",
                "connection",
                "在线",
                "online",
                "离线",
                "offline",
            ],
            IntentCategory::Help => &[
                "帮助",
                "help",
                "怎么用",
                "怎么使用",
                "how to",
                "如何使用",
                "how to use",
                "教程",
                "tutorial",
                "指南",
                "guide",
                "说明",
                "instruction",
                "文档",
                "documentation",
                "支持",
                "support",
                "faq",
            ],
            IntentCategory::General => &[
                "你好",
                "hello",
                "hi",
                "嗨",
                "hey",
                "早上好",
                "good morning",
                "下午好",
                "good afternoon",
                "晚上好",
                "good evening",
                "谢谢",
                "thank",
                "再见",
                "bye",
            ],
        }
    }
}

/// Intent classification result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResult {
    /// Primary intent category
    pub category: IntentCategory,
    /// Confidence score (0.0 to 1.0)
    #[serde(default)]
    pub confidence: f32,
    /// Detected keywords
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Intent classifier using keyword matching.
#[derive(Clone)]
pub struct IntentClassifier {
    /// Minimum confidence threshold
    confidence_threshold: f32,
}

impl Default for IntentClassifier {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.3,
        }
    }
}

impl IntentClassifier {
    /// Create a new classifier with custom threshold.
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            confidence_threshold: threshold,
        }
    }

    /// Classify user intent from message.
    pub fn classify(&self, message: &str) -> IntentResult {
        let message_lower = message.to_lowercase();

        // Count keyword matches for each category
        let mut scores: Vec<(IntentCategory, f32, Vec<String>)> = IntentCategory::all_variants()
            .iter()
            .map(|category| {
                let keywords = category.keywords();
                let mut matched_keywords = Vec::new();

                for &keyword in keywords {
                    if message_lower.contains(keyword) {
                        matched_keywords.push(keyword.to_string());
                    }
                }

                // Score based on matched keywords, weighted by keyword length
                let score = if matched_keywords.is_empty() {
                    0.0
                } else {
                    // Each match contributes based on keyword character count, capped at 1.0
                    // Longer keywords get more weight (use character count, not bytes)
                    let weighted_score: f32 = matched_keywords.iter()
                        .map(|kw| {
                            let char_count = kw.chars().count() as f32;
                            // 0.15 per character, with min 0.2 and max 0.5 per keyword
                            (char_count * 0.15).clamp(0.2, 0.5)
                        })
                        .sum();
                    weighted_score.min(1.0)
                };

                (category.clone(), score, matched_keywords)
            })
            .collect();

        // Sort by score descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (category, confidence, keywords) =
            scores
                .into_iter()
                .next()
                .unwrap_or((IntentCategory::General, 0.0, Vec::new()));

        // If no good match, default to General
        if confidence < self.confidence_threshold {
            IntentResult {
                category: IntentCategory::General,
                confidence: 0.5, // Default confidence for General
                keywords: Vec::new(),
            }
        } else {
            IntentResult {
                category,
                confidence,
                keywords,
            }
        }
    }

    /// Classify and return only the category.
    pub fn classify_category(&self, message: &str) -> IntentCategory {
        self.classify(message).category
    }
}

impl IntentCategory {
    /// Get all intent category variants.
    pub fn all_variants() -> Vec<IntentCategory> {
        vec![
            IntentCategory::Device,
            IntentCategory::Rule,
            IntentCategory::Workflow,
            IntentCategory::Data,
            IntentCategory::Alert,
            IntentCategory::System,
            IntentCategory::Help,
            IntentCategory::General,
        ]
    }
}

/// Tool filter for reducing tools sent to LLM.
#[derive(Clone)]
pub struct ToolFilter {
    /// Always-included tools (system tools, etc.)
    always_include: HashSet<String>,
}

impl Default for ToolFilter {
    fn default() -> Self {
        Self {
            always_include: {
                let mut set = HashSet::new();
                // System tools are always available
                set.insert("think".to_string());
                set.insert("tool_search".to_string());
                set
            },
        }
    }
}

impl ToolFilter {
    /// Create a new tool filter with custom always-include list.
    pub fn new(always_include: Vec<String>) -> Self {
        Self {
            always_include: always_include.into_iter().collect(),
        }
    }

    /// Filter tools by intent category.
    /// Returns only relevant tools (3-5 max) to reduce thinking.
    pub fn filter_by_intent(&self, all_tools: &[Value], intent: &IntentResult) -> Vec<Value> {
        let target_namespace = intent.category.namespace();

        // Helper to derive namespace from tool name
        let derive_namespace = |name: &str| -> &str {
            if name.starts_with("list_")
                || name.starts_with("get_")
                || name == "control_device"
                || name.contains("device")
            {
                "device"
            } else if name.contains("rule") || name.contains("automation") {
                "rule"
            } else if name.contains("workflow")
                || name.contains("scenario")
                || name.contains("trigger")
            {
                "workflow"
            } else if name.contains("data") || name.contains("query") || name.contains("metrics") {
                "data"
            } else if name == "think" || name == "tool_search" {
                "system"
            } else {
                "general"
            }
        };

        // Always include system tools
        let mut filtered: Vec<Value> = all_tools
            .iter()
            .filter(|tool| {
                let name = tool["name"].as_str().unwrap_or("");
                self.always_include.contains(name)
            })
            .cloned()
            .collect();

        // Add namespace-specific tools
        let namespace_tools: Vec<Value> = all_tools
            .iter()
            .filter(|tool| {
                let name = tool["name"].as_str().unwrap_or("");
                let ns = derive_namespace(name);
                ns == target_namespace
            })
            .cloned()
            .collect();

        filtered.extend(namespace_tools);

        // If General intent or no namespace tools, add commonly used tools
        if intent.category == IntentCategory::General || filtered.is_empty() {
            let common_tools: Vec<Value> = all_tools
                .iter()
                .filter(|tool| {
                    let name = tool["name"].as_str().unwrap_or("");
                    // Include list_* tools for general queries
                    name.starts_with("list_") || name == "query_data"
                })
                .cloned()
                .collect();

            // Add up to 3 common tools
            filtered.extend(common_tools.into_iter().take(3));
        }

        // Limit to 5 tools max to reduce thinking
        filtered.truncate(5);
        filtered
    }

    /// Get simple prompt for intent-based tool selection.
    pub fn intent_prompt(&self, intent: &IntentResult) -> String {
        match intent.category {
            IntentCategory::Device => {
                "用户想查询或控制设备。可用工具: list_devices, control_device, get_device_metrics。直接调用合适的工具。".to_string()
            }
            IntentCategory::Rule => {
                "用户想管理自动化规则。可用工具: list_rules, create_rule, query_rule_history。直接调用合适的工具。".to_string()
            }
            IntentCategory::Workflow => {
                "用户想执行工作流。可用工具: list_workflows, trigger_workflow, query_workflow_status。直接调用合适的工具。".to_string()
            }
            IntentCategory::Data => {
                "用户想查询数据。可用工具: query_data, get_device_metrics。直接调用合适的工具。".to_string()
            }
            IntentCategory::Alert => {
                "用户想查询告警信息。可用工具: list_alerts, acknowledge_alert, get_alert_status。直接调用合适的工具。".to_string()
            }
            IntentCategory::System => {
                "用户想了解系统状态。可用工具: get_system_status, get_health_status, get_version。直接调用合适的工具。".to_string()
            }
            IntentCategory::Help => {
                "用户需要帮助说明。提供清晰的使用说明和示例，不调用工具。".to_string()
            }
            IntentCategory::General => {
                "用户可能在闲聊或需要帮助。先尝试理解意图，必要时使用list_*工具查询信息。".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_intent_classification_device() {
        let classifier = IntentClassifier::default();

        let result = classifier.classify("有哪些设备?");
        assert_eq!(result.category, IntentCategory::Device);
        assert!(result.confidence > 0.0);

        let result = classifier.classify("控制客厅开关");
        assert_eq!(result.category, IntentCategory::Device);
    }

    #[test]
    fn test_intent_classification_rule() {
        let classifier = IntentClassifier::default();

        let result = classifier.classify("创建自动化规则");
        assert_eq!(result.category, IntentCategory::Rule);

        let result = classifier.classify("查看规则列表");
        assert_eq!(result.category, IntentCategory::Rule);
    }

    #[test]
    fn test_intent_classification_workflow() {
        let classifier = IntentClassifier::default();

        let result = classifier.classify("执行工作流");
        assert_eq!(result.category, IntentCategory::Workflow);
    }

    #[test]
    fn test_intent_classification_general() {
        let classifier = IntentClassifier::default();

        let result = classifier.classify("你好");
        assert_eq!(result.category, IntentCategory::General);

        let result = classifier.classify("怎么使用这个系统");
        // "怎么使用这个系统" matches "怎么使用" in Help keywords
        assert_eq!(result.category, IntentCategory::Help);

        // Test general query that doesn't match any specific category
        let result = classifier.classify("今天天气怎么样");
        assert_eq!(result.category, IntentCategory::General);
    }

    #[test]
    fn test_tool_filter() {
        let filter = ToolFilter::default();

        let all_tools = vec![
            json!({"name": "think", "namespace": "system"}),
            json!({"name": "tool_search", "namespace": "system"}),
            json!({"name": "list_devices", "namespace": "device"}),
            json!({"name": "control_device", "namespace": "device"}),
            json!({"name": "list_rules", "namespace": "rule"}),
            json!({"name": "create_rule", "namespace": "rule"}),
            json!({"name": "list_workflows", "namespace": "workflow"}),
            json!({"name": "trigger_workflow", "namespace": "workflow"}),
            json!({"name": "query_data", "namespace": "data"}),
        ];

        let device_intent = IntentResult {
            category: IntentCategory::Device,
            confidence: 0.9,
            keywords: vec!["设备".to_string()],
        };

        let filtered = filter.filter_by_intent(&all_tools, &device_intent);
        assert!(filtered.len() <= 5);
        // Should have system tools + device tools
        assert!(filtered.iter().any(|t| t["name"] == "list_devices"));
        assert!(filtered.iter().any(|t| t["name"] == "control_device"));
    }

    #[test]
    fn test_intent_keywords() {
        assert!(IntentCategory::Device.keywords().contains(&"设备"));
        assert!(IntentCategory::Rule.keywords().contains(&"规则"));
        assert!(IntentCategory::Workflow.keywords().contains(&"工作流"));
        assert!(IntentCategory::Data.keywords().contains(&"数据"));
    }
}
