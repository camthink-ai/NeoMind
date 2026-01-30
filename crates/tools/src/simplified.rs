//! Simplified tool wrappers with smart defaults and user-friendly responses.
//!
//! This module provides simplified versions of tools that:
//! 1. Have minimal required parameters
//! 2. Use smart defaults for common use cases
//! 3. Return user-friendly error messages
//! 4. Support natural language parameter names
//!
//! NOTE: Tool names here must match the actual tool names registered in the tool registry.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::tool::ToolOutput;

/// User-friendly error response format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendlyError {
    /// User-facing error message in simple language
    pub message: String,
    /// Suggested fixes the user can try
    pub suggestions: Vec<String>,
    /// Whether this is a critical error or just a warning
    pub is_warning: bool,
}

impl FriendlyError {
    /// Create a new friendly error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            suggestions: Vec::new(),
            is_warning: false,
        }
    }

    /// Add a suggestion to the error.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Set as warning instead of error.
    pub fn as_warning(mut self) -> Self {
        self.is_warning = true;
        self
    }

    /// Convert to ToolOutput.
    pub fn to_output(&self) -> ToolOutput {

        if self.is_warning {
            ToolOutput::warning_with_metadata(
                &self.message,
                serde_json::json!({
                    "suggestions": self.suggestions,
                    "type": "friendly_warning"
                })
            )
        } else {
            ToolOutput::error_with_metadata(
                &self.message,
                serde_json::json!({
                    "suggestions": self.suggestions,
                    "type": "friendly_error"
                })
            )
        }
    }
}

/// Common friendly error messages.
pub struct ErrorMessages;

impl ErrorMessages {
    /// Device not found error with helpful suggestions.
    pub fn device_not_found(device_id: &str) -> FriendlyError {
        FriendlyError::new(format!("找不到设备 '{}'", device_id))
            .with_suggestion("使用 'list_devices' 查看所有可用设备".to_string())
            .with_suggestion("检查设备ID是否正确".to_string())
            .with_suggestion(format!("可能是设备 '{}' 尚未添加到系统", device_id))
    }

    /// Parameter missing error with usage hint.
    pub fn parameter_missing(param_name: &str, usage_hint: &str) -> FriendlyError {
        FriendlyError::new(format!("缺少必要参数: {}", param_name))
            .with_suggestion(format!("用法: {}", usage_hint))
    }

    /// No data available error.
    pub fn no_data_available(device_id: &str) -> FriendlyError {
        FriendlyError::new(format!("设备 '{}' 暂无数据", device_id))
            .with_suggestion("设备可能刚添加，需要等待数据采集".to_string())
            .with_suggestion("检查设备是否在线".to_string())
            .as_warning()
    }

    /// Device offline error.
    pub fn device_offline(device_id: &str) -> FriendlyError {
        FriendlyError::new(format!("设备 '{}' 当前离线", device_id))
            .with_suggestion("检查设备电源连接".to_string())
            .with_suggestion("查看网络连接状态".to_string())
    }

    /// Rule not found error.
    pub fn rule_not_found(rule_name: &str) -> FriendlyError {
        FriendlyError::new(format!("找不到规则 '{}'", rule_name))
            .with_suggestion("使用 'list_rules' 查看所有可用规则".to_string())
            .with_suggestion("规则名称可能输入错误".to_string())
    }

    /// Invalid command error with valid commands.
    pub fn invalid_command(device_id: &str, valid_commands: &[String]) -> FriendlyError {
        FriendlyError::new(format!("设备 '{}' 不支持此命令", device_id))
            .with_suggestion(format!("支持的命令: {}", valid_commands.join(", ")))
    }

    /// Permission denied error.
    pub fn permission_denied(resource: &str) -> FriendlyError {
        FriendlyError::new(format!("没有权限访问: {}", resource))
            .with_suggestion("请联系管理员获取权限".to_string())
    }

    /// General error with context.
    pub fn general(context: &str, details: &str) -> FriendlyError {
        FriendlyError::new(format!("{}: {}", context, details))
            .with_suggestion("请稍后重试".to_string())
            .with_suggestion("如果问题持续，请联系技术支持".to_string())
    }
}

/// Simplified tool configuration.
///
/// This configures how a tool should be simplified for LLM use.
#[derive(Debug, Clone)]
pub struct SimplifiedConfig {
    /// Tool name (used by LLM)
    pub name: String,
    /// Natural language description
    pub description: String,
    /// Natural language aliases for this tool
    pub aliases: Vec<String>,
    /// Required parameters with natural language names
    pub required_params: Vec<String>,
    /// Optional parameters with smart defaults
    pub optional_params: HashMap<String, Value>,
    /// Example calls in natural language
    pub examples: Vec<String>,
}

impl SimplifiedConfig {
    /// Create a new simplified config.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            aliases: Vec::new(),
            required_params: Vec::new(),
            optional_params: HashMap::new(),
            examples: Vec::new(),
        }
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add an alias.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Add a required parameter.
    pub fn with_required_param(mut self, param: impl Into<String>) -> Self {
        self.required_params.push(param.into());
        self
    }

    /// Add an optional parameter with default value.
    pub fn with_optional_param(mut self, param: impl Into<String>, default: Value) -> Self {
        self.optional_params.insert(param.into(), default);
        self
    }

    /// Add an example.
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples.push(example.into());
        self
    }
}

/// Tool definition optimized for LLM consumption.
///
/// This is a simplified version of the tool schema that:
/// 1. Uses natural language parameter names
/// 2. Provides clear examples
/// 3. Includes common use cases
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolDefinition {
    /// Tool name (short, memorable) - MUST match actual tool name in registry
    pub name: String,
    /// Natural language description
    pub description: String,
    /// Natural language aliases
    pub aliases: Vec<String>,
    /// Required parameters (simplified names)
    pub required: Vec<String>,
    /// Optional parameters with defaults
    pub optional: HashMap<String, ParameterInfo>,
    /// Usage examples
    pub examples: Vec<Example>,
    /// When to use this tool
    pub use_when: Vec<String>,
}

/// Parameter information for LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    /// Natural language description
    pub description: String,
    /// Default value
    pub default: Value,
    /// Example values
    pub examples: Vec<String>,
}

/// Usage example for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    /// User query that triggers this tool
    pub user_query: String,
    /// Expected tool call (simplified format)
    pub tool_call: String,
    /// Description of what this does
    pub explanation: String,
}

/// Generate simplified tool definitions for common tools.
///
/// IMPORTANT: Tool names MUST match the actual tool names registered in the tool registry.
/// See real.rs, agent_tools.rs, and system_tools.rs for actual tool names.
pub fn get_simplified_tools() -> Vec<LlmToolDefinition> {
    vec![
        // === Device Tools ===

        // list_devices - actual tool name in registry
        LlmToolDefinition {
            name: "list_devices".to_string(),
            description: "列出系统中所有设备".to_string(),
            aliases: vec![
                "查看设备".to_string(),
                "有哪些设备".to_string(),
                "所有设备".to_string(),
                "设备列表".to_string(),
                "显示设备".to_string(),
            ],
            required: vec![],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "有哪些设备？".to_string(),
                    tool_call: "list_devices()".to_string(),
                    explanation: "显示所有设备".to_string(),
                },
            ],
            use_when: vec!["用户询问设备".to_string()],
        },

        // get_device_data - actual tool name in registry (simpler than query_data)
        LlmToolDefinition {
            name: "get_device_data".to_string(),
            description: "获取设备的所有当前数据（仅当前值，无历史）。用于查看设备实时状态。如果要分析数据变化/趋势，必须使用query_data工具并指定时间范围".to_string(),
            aliases: vec![
                "设备数据".to_string(),
                "查询数据".to_string(),
                "温度多少".to_string(),
                "当前温度".to_string(),
                "获取数据".to_string(),
            ],
            required: vec!["device_id".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "温度传感器1的数据".to_string(),
                    tool_call: "get_device_data(device_id='sensor_1')".to_string(),
                    explanation: "获取设备所有指标".to_string(),
                },
            ],
            use_when: vec![
                "用户询问温度/湿度/数据".to_string(),
                "用户想查看读数".to_string(),
            ],
        },

        // query_data - for historical data and trend analysis with time range
        LlmToolDefinition {
            name: "query_data".to_string(),
            description: "查询设备历史数据并分析趋势。必须指定时间范围来分析数据变化。用当前时间戳减去秒数得到start_time。例如：今天数据=今天0点到现在，最近24小时=当前时间-86400".to_string(),
            aliases: vec![
                "历史数据".to_string(),
                "数据趋势".to_string(),
                "电量变化".to_string(),
                "温度变化".to_string(),
                "分析数据".to_string(),
                "今天数据".to_string(),
            ],
            required: vec!["device_id".to_string(), "metric".to_string()],
            optional: HashMap::from_iter(vec![
                ("start_time".to_string(), ParameterInfo {
                    description: "起始时间戳（Unix秒），分析今天=今天0点，最近X小时=当前时间-X*3600".to_string(),
                    default: serde_json::json!(null),
                    examples: vec!["1738368000".to_string(), "当前时间-86400".to_string()],
                }),
                ("end_time".to_string(), ParameterInfo {
                    description: "结束时间戳，默认为当前时间".to_string(),
                    default: serde_json::json!(null),
                    examples: vec!["1738454400".to_string(), "当前时间".to_string()],
                }),
            ]),
            examples: vec![
                Example {
                    user_query: "分析今天ne101的电池变化".to_string(),
                    tool_call: "query_data(device_id='4t1vcbefzk', metric='battery', start_time=今天0点时间戳, end_time=当前时间戳)".to_string(),
                    explanation: "查询今天的历史数据分析趋势".to_string(),
                },
                Example {
                    user_query: "ne101最近24小时温度变化如何".to_string(),
                    tool_call: "query_data(device_id='4t1vcbefzk', metric='temperature', start_time=当前时间-86400, end_time=当前时间)".to_string(),
                    explanation: "查询历史数据对比温度变化".to_string(),
                },
                Example {
                    user_query: "电量有没有下降".to_string(),
                    tool_call: "query_data(device_id='ne101', metric='battery', start_time=当前时间-3600, end_time=当前时间)".to_string(),
                    explanation: "查询历史数据判断电量变化趋势".to_string(),
                },
            ],
            use_when: vec![
                "用户询问数据变化/趋势".to_string(),
                "用户问电量变化/温度变化".to_string(),
                "用户要分析今天/最近的数据".to_string(),
                "用户问有没有下降/上升".to_string(),
            ],
        },

        // control_device - actual tool name in registry
        LlmToolDefinition {
            name: "control_device".to_string(),
            description: "控制设备（打开、关闭、设置值）".to_string(),
            aliases: vec![
                "控制设备".to_string(),
                "打开".to_string(),
                "关闭".to_string(),
                "开启".to_string(),
                "调节".to_string(),
                "设置".to_string(),
            ],
            required: vec!["device_id".to_string(), "command".to_string()],
            optional: {
                let mut map = HashMap::new();
                map.insert("value".to_string(), ParameterInfo {
                    description: "命令值（可选）".to_string(),
                    default: Value::Null,
                    examples: vec!["26".to_string()],
                });
                map
            },
            examples: vec![
                Example {
                    user_query: "打开客厅灯".to_string(),
                    tool_call: "control_device(device_id='light_living', command='turn_on')".to_string(),
                    explanation: "打开设备".to_string(),
                },
            ],
            use_when: vec![
                "用户要打开/关闭设备".to_string(),
                "用户要调节设备".to_string(),
            ],
        },

        // analyze_device - actual tool name in registry (was device.analyze)
        LlmToolDefinition {
            name: "analyze_device".to_string(),
            description: "分析设备数据趋势".to_string(),
            aliases: vec![
                "分析数据".to_string(),
                "数据分析".to_string(),
                "趋势分析".to_string(),
            ],
            required: vec!["device_id".to_string()],
            optional: {
                let mut map = HashMap::new();
                map.insert("analysis_type".to_string(), ParameterInfo {
                    description: "分析类型".to_string(),
                    default: serde_json::json!("summary"),
                    examples: vec!["trend".to_string()],
                });
                map
            },
            examples: vec![
                Example {
                    user_query: "分析温度趋势".to_string(),
                    tool_call: "analyze_device(device_id='sensor_1', analysis_type='trend')".to_string(),
                    explanation: "分析趋势".to_string(),
                },
            ],
            use_when: vec!["用户要求分析".to_string()],
        },

        // === Rule Tools ===

        // list_rules - actual tool name in registry
        LlmToolDefinition {
            name: "list_rules".to_string(),
            description: "列出所有自动化规则".to_string(),
            aliases: vec![
                "规则列表".to_string(),
                "显示规则".to_string(),
                "查看规则".to_string(),
                "有哪些规则".to_string(),
            ],
            required: vec![],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "有哪些规则？".to_string(),
                    tool_call: "list_rules()".to_string(),
                    explanation: "列出所有规则".to_string(),
                },
            ],
            use_when: vec!["用户询问规则".to_string()],
        },

        // create_rule - actual tool name in registry
        LlmToolDefinition {
            name: "create_rule".to_string(),
            description: "创建自动化规则".to_string(),
            aliases: vec![
                "创建规则".to_string(),
                "新建规则".to_string(),
                "添加规则".to_string(),
                "自动化".to_string(),
            ],
            required: vec!["name".to_string(), "dsl".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "创建温度告警规则".to_string(),
                    tool_call: "create_rule(name='高温告警', dsl='RULE 高温告警 WHEN temperature > 50 DO NOTIFY 通知 END')".to_string(),
                    explanation: "创建规则".to_string(),
                },
            ],
            use_when: vec!["用户要创建规则".to_string()],
        },

        // delete_rule - actual tool name in registry
        LlmToolDefinition {
            name: "delete_rule".to_string(),
            description: "删除指定的规则".to_string(),
            aliases: vec![
                "删除规则".to_string(),
                "移除规则".to_string(),
                "删除自动化".to_string(),
            ],
            required: vec!["rule_id".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "删除规则123".to_string(),
                    tool_call: "delete_rule(rule_id='123')".to_string(),
                    explanation: "删除规则".to_string(),
                },
            ],
            use_when: vec!["用户要删除规则".to_string()],
        },

        // === Agent Tools ===

        // list_agents - actual tool name in registry
        LlmToolDefinition {
            name: "list_agents".to_string(),
            description: "列出所有AI Agent".to_string(),
            aliases: vec![
                "列出Agent".to_string(),
                "有哪些Agent".to_string(),
                "Agent列表".to_string(),
                "查看Agent".to_string(),
            ],
            required: vec![],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "有哪些Agent？".to_string(),
                    tool_call: "list_agents()".to_string(),
                    explanation: "列出所有Agent".to_string(),
                },
            ],
            use_when: vec!["用户询问Agent".to_string()],
        },

        // get_agent - actual tool name in registry
        LlmToolDefinition {
            name: "get_agent".to_string(),
            description: "获取Agent详细信息和执行历史。返回执行统计、最后执行时间、成功/失败次数。用户询问Agent执行情况/任务/结果时必须调用此工具".to_string(),
            aliases: vec![
                "Agent详情".to_string(),
                "Agent信息".to_string(),
                "Agent执行情况".to_string(),
                "Agent执行历史".to_string(),
            ],
            required: vec!["agent_id".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "Agent最近执行了什么".to_string(),
                    tool_call: "get_agent(agent_id='agent_1')".to_string(),
                    explanation: "获取Agent执行统计和历史".to_string(),
                },
                Example {
                    user_query: "温度监控Agent的详情".to_string(),
                    tool_call: "get_agent(agent_id='agent_1')".to_string(),
                    explanation: "获取Agent详情".to_string(),
                },
            ],
            use_when: vec![
                "用户询问Agent详情".to_string(),
                "用户询问Agent执行情况".to_string(),
                "用户询问Agent执行历史".to_string(),
                "用户询问Agent执行结果".to_string(),
                "用户问Agent做了什么".to_string(),
            ],
        },

        // execute_agent - actual tool name in registry
        LlmToolDefinition {
            name: "execute_agent".to_string(),
            description: "手动执行Agent".to_string(),
            aliases: vec![
                "执行Agent".to_string(),
                "运行Agent".to_string(),
            ],
            required: vec!["agent_id".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "执行温度监控".to_string(),
                    tool_call: "execute_agent(agent_id='agent_1')".to_string(),
                    explanation: "执行Agent".to_string(),
                },
            ],
            use_when: vec!["用户要执行Agent".to_string()],
        },

        // control_agent - actual tool name in registry
        LlmToolDefinition {
            name: "control_agent".to_string(),
            description: "控制Agent（暂停/恢复/删除）".to_string(),
            aliases: vec![
                "暂停Agent".to_string(),
                "恢复Agent".to_string(),
                "删除Agent".to_string(),
            ],
            required: vec!["agent_id".to_string(), "action".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "暂停温度监控".to_string(),
                    tool_call: "control_agent(agent_id='agent_1', action='pause')".to_string(),
                    explanation: "暂停Agent".to_string(),
                },
            ],
            use_when: vec!["用户要控制Agent".to_string()],
        },

        // create_agent - actual tool name in registry
        LlmToolDefinition {
            name: "create_agent".to_string(),
            description: "通过自然语言创建新AI Agent。描述应包含：目标设备、监控指标、触发条件、执行动作、执行频率。Agent类型：监控型(monitor)、执行型(executor)、分析型(analyst)".to_string(),
            aliases: vec![
                "创建Agent".to_string(),
                "新建Agent".to_string(),
                "添加Agent".to_string(),
                "创建智能体".to_string(),
            ],
            required: vec!["description".to_string()],
            optional: HashMap::from_iter(vec![
                ("name".to_string(), ParameterInfo {
                    description: "可选，Agent名称".to_string(),
                    default: Value::Null,
                    examples: vec!["温度监控Agent".to_string()],
                }),
            ]),
            examples: vec![
                Example {
                    user_query: "帮我创建一个监控ne101温度的Agent，每5分钟检查一次".to_string(),
                    tool_call: "create_agent(description='监控ne101设备(4t1vcbefzk)的温度指标，每5分钟检查一次，如果温度超过30度就发送告警通知')".to_string(),
                    explanation: "创建监控型Agent，先查询设备信息".to_string(),
                },
                Example {
                    user_query: "创建一个分析电池状态的Agent".to_string(),
                    tool_call: "create_agent(description='每天早上8点分析所有NE101设备的电池状态，识别电池电量低于20%的设备并生成报告')".to_string(),
                    explanation: "创建分析型Agent，按时间执行".to_string(),
                },
                Example {
                    user_query: "湿度低的时候自动开加湿器".to_string(),
                    tool_call: "create_agent(description='监控室内湿度，当湿度低于30%时自动打开加湿器，每分钟检查一次')".to_string(),
                    explanation: "创建执行型Agent，自动控制设备".to_string(),
                },
            ],
            use_when: vec!["用户要创建Agent".to_string(), "用户要添加智能体".to_string(), "用户要做自动化".to_string()],
        },

        // agent_memory - actual tool name in registry
        LlmToolDefinition {
            name: "agent_memory".to_string(),
            description: "查询Agent记忆和学习内容".to_string(),
            aliases: vec![
                "Agent记忆".to_string(),
                "Agent学习".to_string(),
            ],
            required: vec!["agent_id".to_string()],
            optional: HashMap::new(),
            examples: vec![
                Example {
                    user_query: "Agent学到了什么".to_string(),
                    tool_call: "agent_memory(agent_id='agent_1', query_type='patterns')".to_string(),
                    explanation: "查询学习内容".to_string(),
                },
            ],
            use_when: vec!["用户询问Agent学习".to_string()],
        },

        // === System Tools ===

        // system_help - actual tool name in registry
        LlmToolDefinition {
            name: "system_help".to_string(),
            description: "获取系统帮助和功能介绍".to_string(),
            aliases: vec![
                "帮助".to_string(),
                "使用帮助".to_string(),
                "功能介绍".to_string(),
                "怎么用".to_string(),
            ],
            required: vec![],
            optional: {
                let mut map = HashMap::new();
                map.insert("topic".to_string(), ParameterInfo {
                    description: "帮助主题".to_string(),
                    default: Value::Null,
                    examples: vec!["overview".to_string()],
                });
                map
            },
            examples: vec![
                Example {
                    user_query: "这个系统能做什么？".to_string(),
                    tool_call: "system_help(topic='overview')".to_string(),
                    explanation: "显示系统概览".to_string(),
                },
            ],
            use_when: vec!["新用户求助".to_string()],
        },
    ]
}

/// Format simplified tools into a prompt for the LLM.
pub fn format_tools_for_llm() -> String {
    let tools = get_simplified_tools();
    let mut prompt = String::from("## 可用工具\n\n");
    prompt.push_str("你可以调用以下工具来完成任务。工具调用格式：\n");
    prompt.push_str("```\n[{\"name\":\"工具名\",\"arguments\":{\"参数\":\"值\"}}]\n```\n\n");

    for tool in tools {
        prompt.push_str(&format!("### {} ({})\n", tool.name, tool.description));

        if !tool.aliases.is_empty() {
            prompt.push_str(&format!("**别名**: {}\n", tool.aliases.join(", ")));
        }

        prompt.push_str("**参数**:\n");
        if tool.required.is_empty() && tool.optional.is_empty() {
            prompt.push_str("  无需参数\n");
        } else {
            for param in &tool.required {
                prompt.push_str(&format!("  - **{}** (必需)\n", param));
            }
            for (param, info) in &tool.optional {
                prompt.push_str(&format!("  - **{}** (可选，默认: {}) - {}\n",
                    param, info.default, info.description));
            }
        }

        if !tool.examples.is_empty() {
            prompt.push_str("\n**示例**:\n");
            for ex in &tool.examples {
                prompt.push_str(&format!("  - 用户: \"{}\"\n", ex.user_query));
                prompt.push_str(&format!("    → `{}`\n", ex.tool_call));
            }
        }

        if !tool.use_when.is_empty() {
            prompt.push_str(&format!("\n**使用场景**: {}\n", tool.use_when.join("、")));
        }

        prompt.push('\n');
    }

    prompt
}

/// Format simplified tools as JSON for function calling.
pub fn format_tools_as_json() -> Vec<Value> {
    let tools = get_simplified_tools();
    tools.into_iter().map(|tool| {
        let mut properties = serde_json::Map::new();

        // Build properties from required and optional params
        for param in &tool.required {
            properties.insert(param.clone(), serde_json::json!({
                "type": "string",
                "description": format!("{} (必需)", param)
            }));
        }
        for (param, info) in &tool.optional {
            properties.insert(param.clone(), serde_json::json!({
                "type": "string",
                "description": info.description,
                "default": info.default
            }));
        }

        serde_json::json!({
            "name": tool.name,
            "description": tool.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": tool.required
            }
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tools_for_llm() {
        let prompt = format_tools_for_llm();
        assert!(prompt.contains("list_devices"));
        assert!(prompt.contains("get_device_data"));
        assert!(prompt.contains("control_device"));
        assert!(prompt.contains("create_rule"));
    }

    #[test]
    fn test_friendly_error() {
        let err = ErrorMessages::device_not_found("sensor_1");
        assert_eq!(err.message, "找不到设备 'sensor_1'");
        assert_eq!(err.suggestions.len(), 3);
        assert!(!err.is_warning);
    }

    #[test]
    fn test_friendly_warning() {
        let err = ErrorMessages::no_data_available("sensor_1");
        assert!(err.is_warning);
    }

    #[test]
    fn test_simplified_config() {
        let config = SimplifiedConfig::new("test_tool")
            .with_description("A test tool")
            .with_alias("测试工具")
            .with_required_param("input")
            .with_example("test_tool(input='hello')");

        assert_eq!(config.name, "test_tool");
        assert_eq!(config.description, "A test tool");
        assert_eq!(config.aliases.len(), 1);
        assert_eq!(config.required_params.len(), 1);
        assert_eq!(config.examples.len(), 1);
    }
}
