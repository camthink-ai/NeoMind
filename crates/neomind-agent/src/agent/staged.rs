//! Intent classification for user queries.
//!
//! Provides keyword-based intent classification used by the streaming agent
//! and planner modules to route user messages to appropriate handlers.

use serde::{Deserialize, Serialize};

/// Intent category for user queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentCategory {
    /// Device-related queries (list, control, query)
    Device,
    /// Rule-related queries (list, create, history)
    Rule,
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
    /// Get display name for this intent.
    pub fn display_name(&self) -> &'static str {
        match self {
            IntentCategory::Device => "设备管理",
            IntentCategory::Rule => "自动化规则",
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
                // Transform keywords
                "transform",
                "transforms",
                "数据转换",
                "数据解析",
                "数据处理",
                "数据加工",
                "转换规则",
                "转换",
                "data transform",
                "data processing",
                "data parsing",
                "js_code",
                "js transform",
                "javascript transform",
                "convert",
                "conversion",
                "parse data",
                "process data",
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

    /// Get all intent category variants.
    pub fn all_variants() -> Vec<IntentCategory> {
        vec![
            IntentCategory::Device,
            IntentCategory::Rule,
            IntentCategory::Data,
            IntentCategory::Alert,
            IntentCategory::System,
            IntentCategory::Help,
            IntentCategory::General,
        ]
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
                    let weighted_score: f32 = matched_keywords
                        .iter()
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_intent_keywords() {
        assert!(IntentCategory::Device.keywords().contains(&"设备"));
        assert!(IntentCategory::Rule.keywords().contains(&"规则"));
        assert!(IntentCategory::Data.keywords().contains(&"数据"));
    }
}
