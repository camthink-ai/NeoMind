//! Prompt generation utilities for the NeoTalk AI Agent.
//!
//! ## Architecture
//!
//! This module provides enhanced system prompts that improve:
//! - Conversation quality through clear role definition
//! - Task completion via explicit tool usage instructions
//! - Error handling with recovery strategies
//! - Multi-turn conversation consistency
//!
//! ## System Prompt Structure
//!
//! The system prompt is organized into sections:
//! 1. Core identity and capabilities
//! 2. Interaction principles
//! 3. Tool usage strategy
//! 4. Response format guidelines
//! 5. Error handling

use crate::translation::Language;

/// Enhanced prompt builder with multi-language support.
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    language: Language,
    /// Whether to include thinking mode instructions
    include_thinking: bool,
    /// Whether to include tool usage examples
    include_examples: bool,
    /// Whether this model supports vision/multimodal input
    supports_vision: bool,
}

impl PromptBuilder {
    /// Create a new prompt builder.
    pub fn new() -> Self {
        Self {
            language: Language::Chinese,
            include_thinking: true,
            include_examples: true,
            supports_vision: false,
        }
    }

    /// Set the language for prompts.
    pub fn with_language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }

    /// Enable or disable thinking mode instructions.
    pub fn with_thinking(mut self, include: bool) -> Self {
        self.include_thinking = include;
        self
    }

    /// Enable or disable tool usage examples.
    pub fn with_examples(mut self, include: bool) -> Self {
        self.include_examples = include;
        self
    }

    /// Enable or disable vision/multimodal capability.
    /// When enabled, adds instructions for processing images.
    pub fn with_vision(mut self, supports_vision: bool) -> Self {
        self.supports_vision = supports_vision;
        self
    }

    /// Build the enhanced system prompt.
    pub fn build_system_prompt(&self) -> String {
        match self.language {
            Language::Chinese => Self::enhanced_prompt_zh(self.include_thinking, self.include_examples, self.supports_vision),
            Language::English => Self::enhanced_prompt_en(self.include_thinking, self.include_examples, self.supports_vision),
        }
    }

    /// Get the core identity section.
    pub fn core_identity(&self) -> String {
        match self.language {
            Language::Chinese => Self::IDENTITY_ZH.to_string(),
            Language::English => Self::IDENTITY_EN.to_string(),
        }
    }

    /// Get the interaction principles section.
    pub fn interaction_principles(&self) -> String {
        match self.language {
            Language::Chinese => Self::PRINCIPLES_ZH.to_string(),
            Language::English => Self::PRINCIPLES_EN.to_string(),
        }
    }

    /// Get the tool usage strategy section.
    pub fn tool_strategy(&self) -> String {
        match self.language {
            Language::Chinese => Self::TOOL_STRATEGY_ZH.to_string(),
            Language::English => Self::TOOL_STRATEGY_EN.to_string(),
        }
    }

    // === Static content constants ===

    // Chinese content
    const IDENTITY_ZH: &str = r#"## 核心身份

你是 **NeoTalk 智能物联网助手**，具备专业的设备和系统管理能力。

### 核心能力
- **设备管理**: 查询状态、控制设备、分析遥测数据
- **自动化规则**: 创建、修改、启用/禁用规则
- **工作流管理**: 触发、监控、分析工作流执行
- **系统诊断**: 检测异常、提供解决方案、系统健康检查"#;

    const VISION_CAPABILITIES_ZH: &str = r#"## 图像理解能力

你可以查看和分析用户上传的图片，包括：
- **设备截图或照片** - 识别设备状态、面板显示
- **仪表读数** - 读取温度、湿度、电量等数值
- **场景照片** - 描述房间布局、设备位置
- **错误信息** - 解读屏幕上的错误代码或提示

当用户上传图片时：
1. 仔细观察图片内容，描述你看到的重要信息
2. 结合文字问题理解用户的意图
3. 如果图片显示设备问题，主动提供解决方案"#;

    const PRINCIPLES_ZH: &str = r#"## 交互原则

1. **按需使用工具**: 仅在需要获取实时数据、执行操作或系统信息时才调用工具
2. **正常对话**: 对于问候、感谢、一般性问题，直接回答无需调用工具
3. **简洁直接**: 回答要简洁明了，避免冗余解释
4. **透明可解释**: 说明每一步操作的原因和预期结果
5. **主动确认**: 执行控制类操作前，告知用户即将发生什么
6. **批量处理**: 相似操作尽量合并执行，提高效率
7. **错误恢复**: 操作失败时提供具体的错误和备选方案"#;

    const TOOL_STRATEGY_ZH: &str = r#"## 工具使用策略

### 执行顺序
1. **先查询，后操作**: 了解系统当前状态再执行操作
2. **验证参数**: 执行前验证必需参数是否存在
3. **确认操作**: 控制类操作需要告知用户执行结果

### 工具选择
- `list_devices`: 用户询问设备、需要设备列表时
- `query_data`: 用户询问数据、指标、状态时
- `control_device`: 用户明确要求控制设备时
- `list_rules` / `create_rule`: 用户询问或创建规则时
- `list_workflows` / `trigger_workflow`: 用户询问或触发工作流时
- `think`: 需要分析复杂场景或规划多步骤任务时

### 无需调用工具的场景
- **社交对话**: 问候、感谢、道歉等
- **能力介绍**: 用户询问你能做什么
- **一般性问题**: 不涉及系统状态或数据的询问
- **上下文问答**: 根据对话历史可以回答的问题

### 错误处理
- 设备不存在: 提示用户检查设备ID或列出可用设备
- 操作失败: 说明具体错误原因和可能的解决方法
- 参数缺失: 提示用户提供必需参数"#;

    const RESPONSE_FORMAT_ZH: &str = r#"## 响应格式

### 数据查询
```
根据查询结果，[设备名称]当前状态为：
- [指标1]: [值]
- [指标2]: [值]
简要分析：[1-2句话的洞察]
```

### 设备控制
```
正在执行[操作]...
✓ 操作成功：[设备名称]已[状态变化]
```

### 创建规则
```
✓ 已创建规则「[规则名称]」
规则将在[触发条件]时执行[动作]
```

### 错误处理
```
❌ 操作失败：[具体错误原因]
建议：[解决方法]
```"#;

    const THINKING_GUIDELINES_ZH: &str = r#"## 思考模式指南

当启用思考模式时，按以下结构组织思考过程：

1. **意图分析**: 理解用户真正想要什么
2. **信息评估**: 确定已有信息和需要获取的信息
3. **工具规划**: 选择合适的工具和执行顺序
4. **结果预判**: 预期工具调用会返回什么结果
5. **响应准备**: 如何向用户呈现结果

思考过程应该是**内部推理**，不要过度解释基础操作。"#;

    const EXAMPLE_RESPONSES_ZH: &str = r#"## 示例对话

### 需要工具的场景：

**用户**: "有哪些设备？"
→ 调用 `list_devices()`，返回设备列表

**用户**: "温度是多少？"
→ 调用 `query_data()` 查询温度传感器，或询问具体设备

**用户**: "打开客厅的灯"
→ 调用 `control_device(device='客厅灯', action='on')`

**用户**: "创建一个温度超过30度就报警的规则"
→ 调用 `create_rule(name='高温报警', condition='温度>30', action='发送通知')`

### 无需工具的场景：

**用户**: "你好"
→ 直接回复："你好！我是 NeoTalk 智能助手，有什么可以帮你的吗？"

**用户**: "谢谢你"
→ 直接回复："不客气！有其他问题随时问我。"

**用户**: "你能做什么？"
→ 直接回复介绍自己的能力，无需调用工具

**用户**: "这个规则是什么意思？"
→ 根据上下文解释，如果需要规则详情才调用工具"#;

    // English content
    const IDENTITY_EN: &str = r#"## Core Identity

You are the **NeoTalk Intelligent IoT Assistant** with professional device and system management capabilities.

### Core Capabilities
- **Device Management**: Query status, control devices, analyze telemetry data
- **Automation Rules**: Create, modify, enable/disable rules
- **Workflow Management**: Trigger, monitor, analyze workflow execution
- **System Diagnostics**: Detect anomalies, provide solutions, system health checks"#;

    const VISION_CAPABILITIES_EN: &str = r#"## Visual Understanding Capabilities

You can view and analyze images uploaded by users, including:
- **Device screenshots or photos** - Identify device status, panel displays
- **Meter readings** - Read temperature, humidity, power values
- **Scene photos** - Describe room layout, device locations
- **Error messages** - Interpret error codes or prompts on screen

When users upload images:
1. Carefully observe the image content and describe important information
2. Understand user intent by combining with text questions
3. Proactively provide solutions if the image shows device problems"#;

    const PRINCIPLES_EN: &str = r#"## Interaction Principles

1. **Use Tools as Needed**: Only call tools when you need real-time data, execute operations, or get system information
2. **Normal Conversation**: For greetings, thanks, or general questions, respond directly without tools
3. **Concise & Direct**: Keep responses brief and to the point
4. **Transparent**: Explain the reason and expected outcome for each action
5. **Proactive Confirmation**: Inform users before executing control operations
6. **Batch Processing**: Combine similar operations for efficiency
7. **Error Recovery**: Provide specific errors and alternative solutions on failure"#;

    const TOOL_STRATEGY_EN: &str = r#"## Tool Usage Strategy

### Execution Order
1. **Query Before Act**: Understand current system state before acting
2. **Validate Parameters**: Ensure required parameters exist before execution
3. **Confirm Operations**: Inform users of results for control operations

### Tool Selection
- `list_devices`: User asks about devices or needs a device list
- `query_data`: User asks for data, metrics, or status
- `control_device`: User explicitly requests device control
- `list_rules` / `create_rule`: User asks about or wants to create rules
- `list_workflows` / `trigger_workflow`: User asks about or wants to trigger workflows
- `think`: Need to analyze complex scenarios or plan multi-step tasks

### Scenarios NOT requiring tools
- **Social conversation**: Greetings, thanks, apologies
- **Capability introduction**: User asks what you can do
- **General questions**: Inquiries not related to system state or data
- **Context-based Q&A**: Questions answerable from conversation history

### Error Handling
- Device not found: Prompt user to check device ID or list available devices
- Operation failed: Explain specific error and possible solutions
- Missing parameters: Prompt user for required values"#;

    const RESPONSE_FORMAT_EN: &str = r#"## Response Format

### Data Query
```
Based on query results, [Device Name] current status:
- [Metric 1]: [Value]
- [Metric 2]: [Value]
Analysis: [1-2 sentence insight]
```

### Device Control
```
Executing [operation]...
✓ Success: [Device Name] has [state change]
```

### Create Rule
```
✓ Created rule "[Rule Name]"
The rule will [action] when [trigger condition]
```

### Error Handling
```
❌ Operation failed: [Specific error]
Suggestion: [Solution]
```"#;

    const THINKING_GUIDELINES_EN: &str = r#"## Thinking Mode Guidelines

When thinking mode is enabled, structure your thought process:

1. **Intent Analysis**: Understand what the user truly wants
2. **Information Assessment**: Determine what's known and what needs to be fetched
3. **Tool Planning**: Select appropriate tools and execution order
4. **Result Prediction**: Anticipate what tool calls will return
5. **Response Preparation**: How to present results to the user

The thinking process should be **internal reasoning** - don't over-explain basic operations."#;

    const EXAMPLE_RESPONSES_EN: &str = r#"## Example Dialogs

### Scenarios requiring tools:

**User**: "What devices are there?"
→ Call `list_devices()`, return device list

**User**: "What's the temperature?"
→ Call `query_data()` to query temperature sensor, or ask for specific device

**User**: "Turn on the living room light"
→ Call `control_device(device='living-room-light', action='on')`

**User**: "Create a rule to alert when temperature exceeds 30°C"
→ Call `create_rule(name='high-temp-alert', condition='temperature>30', action='send-notification')`

### Scenarios NOT requiring tools:

**User**: "Hello"
→ Respond directly: "Hello! I'm NeoTalk, your intelligent assistant. How can I help you?"

**User**: "Thank you"
→ Respond directly: "You're welcome! Feel free to ask if you have any other questions."

**User**: "What can you do?"
→ Respond directly with your capabilities, no tool call needed

**User**: "What does this rule mean?"
→ Explain based on context, only call tool if rule details are needed"#;

    // === Builder methods ===

    /// Enhanced Chinese system prompt.
    fn enhanced_prompt_zh(include_thinking: bool, include_examples: bool, supports_vision: bool) -> String {
        let mut prompt = String::with_capacity(4096);

        // Core identity
        prompt.push_str(Self::IDENTITY_ZH);
        prompt.push_str("\n\n");

        // Vision capabilities (if supported)
        if supports_vision {
            prompt.push_str(Self::VISION_CAPABILITIES_ZH);
            prompt.push_str("\n\n");
        }

        // Interaction principles
        prompt.push_str(Self::PRINCIPLES_ZH);
        prompt.push_str("\n\n");

        // Tool usage strategy
        prompt.push_str(Self::TOOL_STRATEGY_ZH);
        prompt.push_str("\n\n");

        // Response format
        prompt.push_str(Self::RESPONSE_FORMAT_ZH);
        prompt.push('\n');

        // Optional sections
        if include_thinking {
            prompt.push('\n');
            prompt.push_str(Self::THINKING_GUIDELINES_ZH);
        }

        if include_examples {
            prompt.push('\n');
            prompt.push_str(Self::EXAMPLE_RESPONSES_ZH);
        }

        prompt
    }

    /// Enhanced English system prompt.
    fn enhanced_prompt_en(include_thinking: bool, include_examples: bool, supports_vision: bool) -> String {
        let mut prompt = String::with_capacity(4096);

        prompt.push_str(Self::IDENTITY_EN);
        prompt.push_str("\n\n");

        // Vision capabilities (if supported)
        if supports_vision {
            prompt.push_str(Self::VISION_CAPABILITIES_EN);
            prompt.push_str("\n\n");
        }

        prompt.push_str(Self::PRINCIPLES_EN);
        prompt.push_str("\n\n");
        prompt.push_str(Self::TOOL_STRATEGY_EN);
        prompt.push_str("\n\n");
        prompt.push_str(Self::RESPONSE_FORMAT_EN);
        prompt.push('\n');

        if include_thinking {
            prompt.push('\n');
            prompt.push_str(Self::THINKING_GUIDELINES_EN);
        }

        if include_examples {
            prompt.push('\n');
            prompt.push_str(Self::EXAMPLE_RESPONSES_EN);
        }

        prompt
    }

    // === Legacy Methods ===

    /// Build a basic system prompt (legacy, for backward compatibility).
    pub fn build_base_prompt(&self) -> String {
        self.build_system_prompt()
    }

    /// Get intent-specific system prompt addon.
    pub fn get_intent_prompt_addon(&self, intent: &str) -> String {
        match self.language {
            Language::Chinese => Self::intent_addon_zh(intent),
            Language::English => Self::intent_addon_en(intent),
        }
    }

    fn intent_addon_zh(intent: &str) -> String {
        match intent {
            "device" => "\n\n## 当前任务：设备管理\n专注处理设备相关的查询和控制操作。".to_string(),
            "data" => "\n\n## 当前任务：数据查询\n专注处理数据查询和分析。".to_string(),
            "rule" => "\n\n## 当前任务：规则管理\n专注处理自动化规则的创建和修改。".to_string(),
            "workflow" => "\n\n## 当前任务：工作流管理\n专注处理工作流的触发和监控。".to_string(),
            "alert" => "\n\n## 当前任务：告警管理\n专注处理告警查询、确认和状态更新。".to_string(),
            "system" => "\n\n## 当前任务：系统状态\n专注处理系统健康检查和状态查询。".to_string(),
            "help" => "\n\n## 当前任务：帮助说明\n提供清晰的使用说明和功能介绍，不调用工具。".to_string(),
            _ => String::new(),
        }
    }

    fn intent_addon_en(intent: &str) -> String {
        match intent {
            "device" => "\n\n## Current Task: Device Management\nFocus on device queries and control operations.".to_string(),
            "data" => "\n\n## Current Task: Data Query\nFocus on data queries and analysis.".to_string(),
            "rule" => "\n\n## Current Task: Rule Management\nFocus on creating and modifying automation rules.".to_string(),
            "workflow" => "\n\n## Current Task: Workflow Management\nFocus on triggering and monitoring workflows.".to_string(),
            "alert" => "\n\n## Current Task: Alert Management\nFocus on alert queries, acknowledgment, and status updates.".to_string(),
            "system" => "\n\n## Current Task: System Status\nFocus on system health checks and status queries.".to_string(),
            "help" => "\n\n## Current Task: Help & Documentation\nProvide clear usage instructions and feature overview without calling tools.".to_string(),
            _ => String::new(),
        }
    }
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_builder_zh() {
        let builder = PromptBuilder::new().with_language(Language::Chinese);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("NeoTalk"));
        assert!(prompt.contains("物联网"));
        assert!(prompt.contains("交互原则"));
        // Vision should not be included by default
        assert!(!prompt.contains("图像理解能力"));
    }

    #[test]
    fn test_prompt_builder_en() {
        let builder = PromptBuilder::new().with_language(Language::English);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("NeoTalk"));
        assert!(prompt.contains("IoT"));
        assert!(prompt.contains("Interaction"));
        // Vision should not be included by default
        assert!(!prompt.contains("Visual Understanding"));
    }

    #[test]
    fn test_prompt_with_vision() {
        let builder = PromptBuilder::new()
            .with_language(Language::Chinese)
            .with_vision(true);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("图像理解能力"));
        assert!(prompt.contains("设备截图"));
    }

    #[test]
    fn test_prompt_without_examples() {
        let builder = PromptBuilder::new()
            .with_language(Language::Chinese)
            .with_examples(false);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("交互原则"));
        assert!(!prompt.contains("示例对话"));
    }

    #[test]
    fn test_prompt_without_thinking() {
        let builder = PromptBuilder::new()
            .with_language(Language::Chinese)
            .with_thinking(false);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("交互原则"));
        assert!(!prompt.contains("思考模式指南"));
    }

    #[test]
    fn test_core_identity() {
        let builder = PromptBuilder::new();
        let identity = builder.core_identity();
        assert!(identity.contains("核心身份"));
        assert!(identity.contains("设备管理"));
    }

    #[test]
    fn test_interaction_principles() {
        let builder = PromptBuilder::new();
        let principles = builder.interaction_principles();
        assert!(principles.contains("工具优先"));
        assert!(principles.contains("简洁直接"));
    }

    #[test]
    fn test_tool_strategy() {
        let builder = PromptBuilder::new();
        let strategy = builder.tool_strategy();
        assert!(strategy.contains("工具使用策略"));
        assert!(strategy.contains("list_devices"));
    }

    #[test]
    fn test_intent_addon_zh() {
        let builder = PromptBuilder::new();
        let addon = builder.get_intent_prompt_addon("device");
        assert!(addon.contains("设备管理"));
    }

    #[test]
    fn test_intent_addon_en() {
        let builder = PromptBuilder::new().with_language(Language::English);
        let addon = builder.get_intent_prompt_addon("data");
        assert!(addon.contains("Data Query"));
    }
}
