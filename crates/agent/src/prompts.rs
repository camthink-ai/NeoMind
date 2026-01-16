//! LLM System Prompt management.
//!
//! Provides dynamic prompt generation with context-aware content injection.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::context_selector::{ContextBundle, DeviceTypeReference, RuleReference};
use crate::translation::{DslTranslator, Language, MdlTranslator};

/// System prompt template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptTemplate {
    /// Agent role description
    pub role: String,
    /// Capabilities description
    pub capabilities: Vec<String>,
    /// Tool descriptions
    pub tools: Vec<ToolDescription>,
    /// DSL syntax examples
    pub dsl_examples: Vec<DslExample>,
    /// Usage guidelines
    pub guidelines: Vec<String>,
}

/// Tool description for prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Example usage
    pub example: String,
}

/// DSL syntax example.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslExample {
    /// Example title
    pub title: String,
    /// DSL code
    pub dsl: String,
    /// Natural language explanation
    pub explanation: String,
}

/// Few-shot example for LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FewShotExample {
    /// Example category
    pub category: ExampleCategory,
    /// User input
    pub user_input: String,
    /// Expected LLM response
    pub assistant_response: String,
    /// Tool calls made (if any)
    pub tool_calls: Vec<String>,
}

/// Example category for few-shot learning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExampleCategory {
    /// Rule creation
    RuleCreation,
    /// Device control
    DeviceControl,
    /// Data query
    DataQuery,
    /// Status inquiry
    StatusInquiry,
    /// Alert management
    AlertManagement,
    /// Multi-tool calling (parallel execution)
    MultiToolCalling,
}

/// Generated system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedPrompt {
    /// Full system prompt
    pub system_prompt: String,
    /// Number of tokens (estimated)
    pub estimated_tokens: usize,
    /// Included context IDs
    pub context_ids: Vec<String>,
}

/// Dynamic prompt generator.
pub struct PromptGenerator {
    /// System prompt template
    template: Arc<RwLock<SystemPromptTemplate>>,
    /// Few-shot examples
    examples: Arc<RwLock<Vec<FewShotExample>>>,
    /// Maximum tokens in generated prompt
    max_tokens: usize,
    /// Current language
    language: Arc<RwLock<Language>>,
}

impl PromptGenerator {
    /// Create a new prompt generator.
    pub fn new() -> Self {
        Self {
            template: Arc::new(RwLock::new(Self::default_template())),
            examples: Arc::new(RwLock::new(Self::default_examples())),
            max_tokens: 3000,
            language: Arc::new(RwLock::new(Language::Chinese)),
        }
    }

    /// Set maximum token budget.
    pub fn with_max_tokens(mut self, tokens: usize) -> Self {
        self.max_tokens = tokens;
        self
    }

    /// Set language.
    pub async fn set_language(&self, language: Language) {
        let mut lang = self.language.write().await;
        *lang = language;
    }

    /// Generate system prompt without context.
    pub async fn generate_base_prompt(&self) -> GeneratedPrompt {
        let template = self.template.read().await;
        let language = self.language.read().await;

        let prompt = match *language {
            Language::Chinese => Self::format_prompt_zh(&template),
            Language::English => Self::format_prompt_en(&template),
        };

        let estimated_tokens = prompt.len() / 2; // Rough estimate: 2 chars per token

        GeneratedPrompt {
            system_prompt: prompt,
            estimated_tokens,
            context_ids: Vec::new(),
        }
    }

    /// Generate system prompt with context bundle.
    pub async fn generate_with_context(&self, context: &ContextBundle) -> GeneratedPrompt {
        let template = self.template.read().await;
        let language = self.language.read().await;
        let mut context_ids = Vec::new();

        let mut prompt = match *language {
            Language::Chinese => Self::format_prompt_zh(&template),
            Language::English => Self::format_prompt_en(&template),
        };

        let mut current_tokens = prompt.len() / 2;
        let budget = self.max_tokens.saturating_sub(current_tokens);

        // Add device type context
        if !context.device_types.is_empty() && budget > 0 {
            let section = match *language {
                Language::Chinese => Self::format_devices_zh(&context.device_types),
                Language::English => Self::format_devices_en(&context.device_types),
            };
            let section_tokens = section.len() / 2;
            if section_tokens <= budget {
                prompt.push_str(&section);
                current_tokens += section_tokens;
                context_ids.extend(context.device_types.iter().map(|d| d.device_type.clone()));
            }
        }

        // Add rule context
        if !context.rules.is_empty() && budget > current_tokens {
            let section = match *language {
                Language::Chinese => Self::format_rules_zh(&context.rules),
                Language::English => Self::format_rules_en(&context.rules),
            };
            let section_tokens = section.len() / 2;
            if section_tokens <= budget - current_tokens {
                prompt.push_str(&section);
                context_ids.extend(context.rules.iter().map(|r| r.rule_id.clone()));
            }
        }

        GeneratedPrompt {
            system_prompt: prompt,
            estimated_tokens: current_tokens,
            context_ids,
        }
    }

    /// Generate system prompt with few-shot examples.
    pub async fn generate_with_examples(&self, categories: &[ExampleCategory]) -> GeneratedPrompt {
        let mut base = self.generate_base_prompt().await;
        let examples = self.examples.read().await;
        let language = self.language.read().await;

        let separator = match *language {
            Language::Chinese => "\n\n## 示例对话\n\n",
            Language::English => "\n\n## Example Conversations\n\n",
        };

        base.system_prompt.push_str(separator);

        for example in examples.iter() {
            if categories.contains(&example.category) {
                let example_text = match *language {
                    Language::Chinese => Self::format_example_zh(example),
                    Language::English => Self::format_example_en(example),
                };
                base.system_prompt.push_str(&example_text);
                base.system_prompt.push_str("\n\n");
            }
        }

        base.estimated_tokens = base.system_prompt.len() / 2;
        base
    }

    /// Add a few-shot example.
    pub async fn add_example(&self, example: FewShotExample) {
        let mut examples = self.examples.write().await;
        examples.push(example);
    }

    /// Set tool descriptions.
    pub async fn set_tools(&self, tools: Vec<ToolDescription>) {
        let mut template = self.template.write().await;
        template.tools = tools;
    }

    fn default_template() -> SystemPromptTemplate {
        SystemPromptTemplate {
            role: "NeoTalk 智能物联网系统助手".to_string(),
            capabilities: vec![
                "设备状态查询与监控".to_string(),
                "规则创建与管理 (DSL)".to_string(),
                "设备控制与命令下发".to_string(),
                "数据分析与异常检测".to_string(),
                "工作流自动化".to_string(),
            ],
            tools: vec![
                ToolDescription {
                    name: "list_device_types".to_string(),
                    description: "列出所有支持的设备类型".to_string(),
                    example: "{\"name\": \"list_device_types\", \"arguments\": {\"category\": \"sensor\"}}".to_string(),
                },
                ToolDescription {
                    name: "get_device_type".to_string(),
                    description: "获取设备类型的详细信息".to_string(),
                    example: "{\"name\": \"get_device_type\", \"arguments\": {\"device_type\": \"dht22_sensor\"}}".to_string(),
                },
                ToolDescription {
                    name: "list_rules".to_string(),
                    description: "列出所有规则".to_string(),
                    example: "{\"name\": \"list_rules\", \"arguments\": {}}".to_string(),
                },
                ToolDescription {
                    name: "create_rule".to_string(),
                    description: "创建新规则".to_string(),
                    example: "{\"name\": \"create_rule\", \"arguments\": {\"dsl\": \"RULE \\\"High Temp\\\" WHEN sensor.temp > 50 DO NOTIFY \\\"Hot\\\" END\"}}".to_string(),
                },
            ],
            dsl_examples: vec![
                DslExample {
                    title: "高温告警规则".to_string(),
                    dsl: r#"RULE "高温告警"
WHEN sensor.temperature > 50
DO
    NOTIFY "温度过高: ${temperature}°C"
END"#.to_string(),
                    explanation: "当传感器温度超过50度时发送通知，使用 ${temperature} 引用当前值".to_string(),
                },
                DslExample {
                    title: "带持续时间的规则".to_string(),
                    dsl: r#"RULE "高温持续告警"
WHEN sensor.temperature > 50
FOR 5 minutes
DO
    NOTIFY "温度持续5分钟超过50度"
END"#.to_string(),
                    explanation: "只有当温度连续5分钟超过阈值时才触发，避免瞬时波动".to_string(),
                },
                DslExample {
                    title: "多动作规则".to_string(),
                    dsl: r#"RULE "高温综合处理"
WHEN sensor.temperature > 60
DO
    NOTIFY "高温警告！正在开启降温设备"
    EXECUTE fan.device(speed=100)
    LOG alert, severity="high"
END"#.to_string(),
                    explanation: "高温时同时执行多个动作：发送通知、开启风扇、记录日志".to_string(),
                },
                DslExample {
                    title: "不同比较运算符".to_string(),
                    dsl: r#"RULE "低温告警"
WHEN sensor.temperature <= 10
DO
    NOTIFY "温度过低: ${temperature}°C"
    EXECUTE heater.device(power=80)
END"#.to_string(),
                    explanation: "支持的比较运算符: >, <, >=, <=, ==, !=".to_string(),
                },
                DslExample {
                    title: "设备控制规则".to_string(),
                    dsl: r#"RULE "自动灌溉"
WHEN soil_sensor.moisture < 30
DO
    EXECUTE pump.device(duration=300)
    LOG info, severity="low"
    NOTIFY "土壤干燥，已启动灌溉系统"
END"#.to_string(),
                    explanation: "根据土壤湿度自动控制灌溉设备，EXECUTE 可传多个参数".to_string(),
                },
                DslExample {
                    title: "复杂条件组合".to_string(),
                    dsl: r#"RULE "设备故障检测"
WHEN device.status == 1
FOR 10 minutes
DO
    NOTIFY "设备异常运行已超过10分钟"
    EXECUTE alert.device(type="fault", code=101)
    LOG error, severity="critical"
END"#.to_string(),
                    explanation: "使用 == 检测状态值，配合持续时间判断设备故障".to_string(),
                },
            ],
            guidelines: vec![
                "使用简洁准确的自然语言描述".to_string(),
                "创建规则前先查询相关设备能力".to_string(),
                "规则名称应清晰描述其用途".to_string(),
                "注意设置合理的阈值避免频繁触发".to_string(),
                "多工具调用规则：如果多个工具之间没有依赖关系，应该在同一个JSON数组中一次性调用，这样可以并行执行，提高响应速度".to_string(),
                "直接回答问题，不要过度思考或展开冗长的推理过程".to_string(),
            ],
        }
    }

    fn default_examples() -> Vec<FewShotExample> {
        vec![
            FewShotExample {
                category: ExampleCategory::MultiToolCalling,
                user_input: "列出所有设备和规则，告诉我当前状态".to_string(),
                assistant_response: "我来同时查询设备列表和规则列表。".to_string(),
                tool_calls: vec!["list_devices()".to_string(), "list_rules()".to_string()],
            },
            FewShotExample {
                category: ExampleCategory::MultiToolCalling,
                user_input: "查看所有设备类型和相关规则".to_string(),
                assistant_response: "我来同时查询设备类型列表和规则列表。".to_string(),
                tool_calls: vec![
                    "list_device_types()".to_string(),
                    "list_rules()".to_string(),
                ],
            },
            FewShotExample {
                category: ExampleCategory::RuleCreation,
                user_input: "创建一个规则，当温度传感器读数超过30度时发送通知".to_string(),
                assistant_response:
                    "好的，我来创建一个高温告警规则。首先让我查询一下可用的设备类型。".to_string(),
                tool_calls: vec!["list_device_types()".to_string()],
            },
            FewShotExample {
                category: ExampleCategory::DeviceControl,
                user_input: "把客厅的灯打开".to_string(),
                assistant_response: "好的，我来发送打开客厅灯的命令。".to_string(),
                tool_calls: vec![
                    "control_device(device_id: 'living_room_light', command: 'ON')".to_string(),
                ],
            },
            FewShotExample {
                category: ExampleCategory::DataQuery,
                user_input: "当前所有传感器的温度是多少？".to_string(),
                assistant_response: "让我查询一下所有温度传感器的当前读数。".to_string(),
                tool_calls: vec![
                    "query_data(device_id: 'temp_sensor', metric: 'temperature')".to_string(),
                ],
            },
        ]
    }

    fn format_prompt_zh(template: &SystemPromptTemplate) -> String {
        let mut prompt = format!("# {}\n\n", template.role);

        // 重要指令：禁用思考模式，提高响应速度
        prompt.push_str("## 重要指令\n\n");
        prompt.push_str(
            "/no_think - 请直接回答问题，不要使用思考模式（think），不要展开冗长的推理过程。\n\n",
        );

        prompt.push_str("## 核心能力\n\n");
        for cap in &template.capabilities {
            prompt.push_str(&format!("- {}\n", cap));
        }

        prompt.push_str("\n## 可用工具\n\n");
        for tool in &template.tools {
            prompt.push_str(&format!("**{}**: {}\n", tool.name, tool.description));
            prompt.push_str(&format!("  示例: `{}`\n", tool.example));
        }

        // 添加工具调用格式说明
        prompt.push_str("\n## 工具调用格式\n\n");
        prompt.push_str("当需要调用工具时，使用以下JSON格式：\n\n");
        prompt.push_str("**单个工具调用**：\n");
        prompt.push_str("```json\n");
        prompt.push_str(r#"{"name": "tool_name", "arguments": {"param1": "value1"}}"#);
        prompt.push_str("\n```\n\n");
        prompt.push_str("**多个工具调用（并行）**：\n");
        prompt.push_str("```json\n");
        prompt.push_str(r#"[{"name": "tool1", "arguments": {}}, {"name": "tool2", "arguments": {}}]"#);
        prompt.push_str("\n```\n\n");

        prompt.push_str("\n## DSL 规则语法\n\n");
        for ex in &template.dsl_examples {
            prompt.push_str(&format!("### {}\n", ex.title));
            prompt.push_str("```dsl\n");
            prompt.push_str(ex.dsl.trim());
            prompt.push_str("\n```\n");
            prompt.push_str(&format!("说明: {}\n\n", ex.explanation));
        }

        prompt.push_str("\n## 使用指南\n\n");
        for guide in &template.guidelines {
            prompt.push_str(&format!("- {}\n", guide));
        }

        prompt
    }

    fn format_prompt_en(template: &SystemPromptTemplate) -> String {
        let mut prompt = format!("# {}\n\n", template.role);

        prompt.push_str("## Core Capabilities\n\n");
        for cap in &template.capabilities {
            prompt.push_str(&format!("- {}\n", cap));
        }

        prompt.push_str("\n## Available Tools\n\n");
        for tool in &template.tools {
            prompt.push_str(&format!("**{}**: {}\n", tool.name, tool.description));
            prompt.push_str(&format!("  Example: `{}`\n", tool.example));
        }

        prompt.push_str("\n## DSL Rule Syntax\n\n");
        for ex in &template.dsl_examples {
            prompt.push_str(&format!("### {}\n", ex.title));
            prompt.push_str("```dsl\n");
            prompt.push_str(ex.dsl.trim());
            prompt.push_str("\n```\n");
            prompt.push_str(&format!("Description: {}\n\n", ex.explanation));
        }

        prompt.push_str("\n## Guidelines\n\n");
        for guide in &template.guidelines {
            prompt.push_str(&format!("- {}\n", guide));
        }

        prompt
    }

    fn format_devices_zh(devices: &[DeviceTypeReference]) -> String {
        let mut section = String::from("\n## 可用设备\n\n");
        for device in devices {
            section.push_str(&format!("**{}** ({})\n", device.name, device.device_type));
            if !device.metrics.is_empty() {
                section.push_str(&format!("  指标: {}\n", device.metrics.join(", ")));
            }
            if !device.commands.is_empty() {
                section.push_str(&format!("  命令: {}\n", device.commands.join(", ")));
            }
        }
        section
    }

    fn format_devices_en(devices: &[DeviceTypeReference]) -> String {
        let mut section = String::from("\n## Available Devices\n\n");
        for device in devices {
            section.push_str(&format!("**{}** ({})\n", device.name, device.device_type));
            if !device.metrics.is_empty() {
                section.push_str(&format!("  Metrics: {}\n", device.metrics.join(", ")));
            }
            if !device.commands.is_empty() {
                section.push_str(&format!("  Commands: {}\n", device.commands.join(", ")));
            }
        }
        section
    }

    fn format_rules_zh(rules: &[RuleReference]) -> String {
        let mut section = String::from("\n## 现有规则\n\n");
        for rule in rules {
            section.push_str(&format!("**{}** ({})\n", rule.name, rule.rule_id));
            section.push_str(&format!("  条件: {}\n", rule.condition));
        }
        section
    }

    fn format_rules_en(rules: &[RuleReference]) -> String {
        let mut section = String::from("\n## Existing Rules\n\n");
        for rule in rules {
            section.push_str(&format!("**{}** ({})\n", rule.name, rule.rule_id));
            section.push_str(&format!("  Condition: {}\n", rule.condition));
        }
        section
    }

    fn format_example_zh(example: &FewShotExample) -> String {
        let mut text = format!("**用户**: {}\n", example.user_input);
        text.push_str(&format!("**助手**: {}\n", example.assistant_response));
        if !example.tool_calls.is_empty() {
            text.push_str("**工具调用**:\n");
            for call in &example.tool_calls {
                text.push_str(&format!("  - {}\n", call));
            }
        }
        text
    }

    fn format_example_en(example: &FewShotExample) -> String {
        let mut text = format!("**User**: {}\n", example.user_input);
        text.push_str(&format!("**Assistant**: {}\n", example.assistant_response));
        if !example.tool_calls.is_empty() {
            text.push_str("**Tool Calls**:\n");
            for call in &example.tool_calls {
                text.push_str(&format!("  - {}\n", call));
            }
        }
        text
    }
}

impl Default for PromptGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_base_prompt() {
        let generator = PromptGenerator::new();
        let prompt = generator.generate_base_prompt().await;

        assert!(!prompt.system_prompt.is_empty());
        assert!(prompt.system_prompt.contains("NeoTalk"));
        assert!(prompt.estimated_tokens > 0);
    }

    #[tokio::test]
    async fn test_generate_with_context() {
        let generator = PromptGenerator::new();
        let context = ContextBundle {
            device_types: vec![],
            rules: vec![],
            commands: vec![],
            estimated_tokens: 0,
        };
        let prompt = generator.generate_with_context(&context).await;

        assert!(!prompt.system_prompt.is_empty());
    }

    #[tokio::test]
    async fn test_set_language() {
        let generator = PromptGenerator::new();

        generator.set_language(Language::English).await;
        let prompt = generator.generate_base_prompt().await;

        assert!(!prompt.system_prompt.is_empty());
    }

    #[test]
    fn test_example_category() {
        let example = FewShotExample {
            category: ExampleCategory::RuleCreation,
            user_input: "test".to_string(),
            assistant_response: "response".to_string(),
            tool_calls: vec![],
        };

        assert_eq!(example.category, ExampleCategory::RuleCreation);
    }

    #[tokio::test]
    async fn test_add_example() {
        let generator = PromptGenerator::new();

        let example = FewShotExample {
            category: ExampleCategory::DeviceControl,
            user_input: "打开灯".to_string(),
            assistant_response: "好的".to_string(),
            tool_calls: vec![],
        };

        generator.add_example(example).await;

        let examples = generator.examples.read().await;
        assert!(examples.len() >= 4); // 3 default + 2 multi-tool + 1 added
    }

    #[tokio::test]
    async fn test_generate_with_examples() {
        let generator = PromptGenerator::new();

        let prompt = generator
            .generate_with_examples(&[ExampleCategory::RuleCreation])
            .await;

        assert!(prompt.system_prompt.contains("示例") || prompt.system_prompt.contains("Example"));
    }
}
