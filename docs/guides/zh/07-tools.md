# Tools 模块

**包名**: `neomind-tools`
**版本**: 0.5.8
**完成度**: 80%
**用途**: AI函数调用工具

## 概述

Tools模块实现了AI可调用的工具系统，包括设备控制、规则管理、数据分析等功能。

## 模块结构

```
crates/tools/src/
├── lib.rs                      # 公开接口
├── tool.rs                     # Tool trait
├── registry.rs                 # 工具注册表
├── builtin.rs                  # 内置工具
├── core_tools.rs               # 核心业务工具
├── agent_tools.rs              # Agent工具
├── system_tools.rs             # 系统工具
├── real.rs                     # 真实实现（feature-gated）
└── simplified.rs               # 简化接口
```

## 核心Trait

### Tool - 工具接口

```rust
pub trait Tool: Send + Sync {
    /// 获取工具定义
    fn definition(&self) -> &ToolDefinition;

    /// 执行工具
    fn execute(&self, input: &serde_json::Value) -> Result<ToolOutput>;

    /// 验证输入
    fn validate(&self, input: &serde_json::Value) -> Result<()> {
        // 默认实现：基于schema验证
    }

    /// 获取工具schema（用于LLM）
    fn schema(&self) -> serde_json::Value {
        // 返回OpenAI function calling格式
    }
}
```

### ToolDefinition - 工具定义

```rust
pub struct ToolDefinition {
    /// 工具名称（唯一标识）
    pub name: String,

    /// 显示名称
    pub display_name: String,

    /// 工具描述（给AI看）
    pub description: String,

    /// 参数定义
    pub parameters: Vec<Parameter>,

    /// 返回值描述
    pub returns: Option<String>,

    /// 使用示例
    pub examples: Vec<ToolExample>,
}

pub struct Parameter {
    /// 参数名称
    pub name: String,

    /// 参数类型
    pub param_type: ParameterType,

    /// 描述
    pub description: String,

    /// 是否必需
    pub required: bool,

    /// 默认值
    pub default: Option<serde_json::Value>,

    /// 枚举值
    pub enum_values: Option<Vec<String>>,
}

pub enum ParameterType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}
```

### ToolOutput - 输出

```rust
pub struct ToolOutput {
    /// 是否成功
    pub success: bool,

    /// 输出数据
    pub data: serde_json::Value,

    /// 错误信息（如果失败）
    pub error: Option<String>,

    /// 元数据
    pub metadata: HashMap<String, String>,
}
```

## 工具注册表

```rust
pub struct ToolRegistry {
    /// 工具映射
    tools: HashMap<String, Arc<dyn Tool>>,

    /// 执行历史
    history: Arc<RwLock<ToolExecutionHistory>>,
}

impl ToolRegistry {
    /// 创建空注册表
    pub fn new() -> Self;

    /// 添加工具
    pub fn register(&mut self, tool: Arc<dyn Tool>);

    /// 批量添加工具
    pub fn register_all(&mut self, tools: Vec<Arc<dyn Tool>>);

    /// 获取工具
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;

    /// 列出所有工具
    pub fn list(&self) -> Vec<String>;

    /// 执行工具
    pub async fn execute(
        &self,
        name: &str,
        input: &serde_json::Value,
    ) -> Result<ToolOutput>;

    /// 格式化为LLM格式
    pub fn format_for_llm(&self) -> serde_json::Value;
}

pub struct ToolRegistryBuilder {
    registry: ToolRegistry,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self;

    pub fn with_tool(mut self, tool: Arc<dyn Tool>) -> Self;

    pub fn with_standard_tools(self) -> Self;

    pub fn build(self) -> ToolRegistry;
}
```

## 内置工具

### 设备工具

```rust
/// 列出设备
pub struct ListDevicesTool {
    device_service: Arc<DeviceService>,
}

// 输入: { "device_type": "sensor" }（可选过滤）
// 输出: { "devices": [...] }

/// 查询设备数据
pub struct QueryDataTool {
    device_service: Arc<DeviceService>,
    time_series: Arc<TimeSeriesStore>,
}

// 输入: { "device_id": "sensor_1", "metric": "temperature" }
// 输出: { "current": 25.5, "history": [...] }

/// 控制设备
pub struct ControlDeviceTool {
    device_service: Arc<DeviceService>,
}

// 输入: { "device_id": "relay_1", "command": "turn_on", "params": {} }
// 输出: { "success": true, "result": ... }

/// 设备状态
pub struct QueryDeviceStatusTool {
    device_service: Arc<DeviceService>,
}

// 输入: { "device_id": "sensor_1" }
// 输出: { "online": true, "last_seen": ..., "state": {...} }
```

### 规则工具

```rust
/// 列出规则
pub struct ListRulesTool {
    rule_service: Arc<RuleService>,
}

/// 创建规则
pub struct CreateRuleTool {
    rule_service: Arc<RuleService>,
    parser: RuleParser,
}

// 输入: { "name": "温度告警", "rule": "ON temp > 30 THEN alert()" }

/// 更新规则
pub struct UpdateRuleTool {
    rule_service: Arc<RuleService>,
}

/// 删除规则
pub struct DeleteRuleTool {
    rule_service: Arc<RuleService>,
}

/// 启用/禁用规则
pub struct EnableRuleTool {
    rule_service: Arc<RuleService>,
}

pub struct DisableRuleTool {
    rule_service: Arc<RuleService>,
}
```

### Agent工具

```rust
/// 列出Agent
pub struct ListAgentsTool {
    agent_service: Arc<AgentService>,
}

/// 获取Agent详情
pub struct GetAgentTool {
    agent_service: Arc<AgentService>,
}

/// 执行Agent
pub struct ExecuteAgentTool {
    agent_service: Arc<AgentService>,
}

/// 控制Agent
pub struct ControlAgentTool {
    agent_service: Arc<AgentService>,
}

/// 创建Agent
pub struct CreateAgentTool {
    agent_service: Arc<AgentService>,
}

/// Agent内存
pub struct AgentMemoryTool {
    agent_service: Arc<AgentService>,
}

/// Agent执行列表
pub struct GetAgentExecutionsTool {
    agent_service: Arc<AgentService>,
}

/// Agent执行详情
pub struct GetAgentExecutionDetailTool {
    agent_service: Arc<AgentService>,
}

/// Agent对话历史
pub struct GetAgentConversationTool {
    agent_service: Arc<AgentService>,
}
```

### 系统工具

```rust
/// 系统信息
pub struct SystemInfoTool;

// 输出: { "version": "...", "uptime": ..., "memory": ... }

/// 系统配置
pub struct SystemConfigTool;

/// 重启服务
pub struct ServiceRestartTool;

/// 系统帮助
pub struct SystemHelpTool;

/// 创建告警
pub struct CreateAlertTool {
    alert_service: Arc<AlertService>,
}

/// 列出告警
pub struct ListAlertsTool {
    alert_service: Arc<AlertService>,
}

/// 确认告警
pub struct AcknowledgeAlertTool {
    alert_service: Arc<AlertService>,
}

/// 导出CSV
pub struct ExportToCsvTool;

/// 导出JSON
pub struct ExportToJsonTool;

/// 生成报告
pub struct GenerateReportTool {
    report_service: Arc<ReportService>,
}
```

### 核心业务工具

```rust
/// 设备发现
pub struct DeviceDiscoverTool {
    discovery: Arc<DeviceDiscovery>,
}

/// 设备查询
pub struct DeviceQueryTool {
    device_service: Arc<DeviceService>,
}

/// 设备控制
pub struct DeviceControlTool {
    device_service: Arc<DeviceService>,
}

/// 设备分析
pub struct DeviceAnalyzeTool {
    device_service: Arc<DeviceService>,
    analytics: Arc<AnalyticsService>,
}

/// 从上下文提取规则
pub struct RuleFromContextTool {
    rule_service: Arc<RuleService>,
    nl2auto: Arc<Nl2Automation>,
}
```

## 工具执行历史

```rust
pub struct ToolExecutionHistory {
    /// 执行记录
    records: Vec<ToolExecutionRecord>,
}

pub struct ToolExecutionRecord {
    /// 执行ID
    pub id: String,

    /// 工具名称
    pub tool_name: String,

    /// 输入参数
    pub input: serde_json::Value,

    /// 输出结果
    pub output: Option<ToolOutput>,

    /// 执行时间
    pub executed_at: i64,

    /// 耗时（毫秒）
    pub duration_ms: u64,

    /// 是否成功
    pub success: bool,
}

pub struct ToolExecutionStats {
    /// 总执行次数
    pub total_executions: usize,

    /// 成功次数
    pub success_count: usize,

    /// 失败次数
    pub failure_count: usize,

    /// 平均耗时
    pub avg_duration_ms: f64,

    /// 最常用工具
    pub most_used_tools: Vec<(String, usize)>,
}
```

## 简化接口

```rust
/// 简化的工具定义（用于前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolDefinition {
    /// 工具名称
    pub name: String,

    /// 显示名称
    pub display_name: String,

    /// 描述
    pub description: String,

    /// 参数schema（JSON Schema格式）
    pub parameters: serde_json::Value,
}

/// 格式化工具为LLM格式
pub fn format_tools_for_llm(tools: &[Arc<dyn Tool>]) -> Vec<LlmToolDefinition> {
    tools.iter().map(|tool| {
        let def = tool.definition();
        LlmToolDefinition {
            name: def.name.clone(),
            display_name: def.display_name.clone(),
            description: def.description.clone(),
            parameters: tool.schema(),
        }
    }).collect()
}
```

## API端点

```
# Tools
GET    /api/tools                           # 列出工具
GET    /api/tools/:name/schema              # 获取工具schema
POST   /api/tools/:name/execute             # 执行工具
GET    /api/tools/format-for-llm            # 格式化为LLM格式
GET    /api/tools/metrics                   # 工具执行统计
```

## 使用示例

### 创建工具注册表

```rust
use neomind-tools::{ToolRegistryBuilder, QueryDataTool, ControlDeviceTool};
use std::sync::Arc;

let registry = ToolRegistryBuilder::new()
    .with_tool(Arc::new(QueryDataTool::mock()))
    .with_tool(Arc::new(ControlDeviceTool::mock()))
    .with_standard_tools()  // 添加所有标准工具
    .build();
```

### 执行工具

```rust
use neomind-tools::ToolRegistry;

let result = registry.execute(
    "query_data",
    serde_json::json!({
        "device_id": "sensor_1",
        "metric": "temperature"
    }),
).await?;

if result.success {
    println!("结果: {}", result.data);
} else {
    eprintln!("错误: {}", result.error.unwrap());
}
```

### 格式化为LLM格式

```rust
let tools_json = registry.format_for_llm();

// 输出OpenAI function calling格式
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "query_data",
        "description": "查询设备数据",
        "parameters": {
          "type": "object",
          "properties": {
            "device_id": { "type": "string" },
            "metric": { "type": "string" }
          },
          "required": ["device_id", "metric"]
        }
      }
    }
  ]
}
```

## 实现状态

| 工具类型 | 状态 | 说明 |
|---------|------|------|
| 设备工具 | ✅ | 完整实现 |
| 规则工具 | ✅ | 完整实现 |
| Agent工具 | ✅ | 完整实现 |
| 系统工具 | ✅ | 完整实现 |
| 业务工具 | ✅ | 完整实现 |
| Mock实现 | ✅ | 用于测试 |
| Real实现 | 🟡 | feature-gated |

## 设计原则

1. **接口统一**: 所有工具实现相同的Trait
2. **类型安全**: 输入输出使用强类型
3. **可测试**: 提供Mock实现
4. **LLM友好**: 生成标准化的函数调用格式
5. **可追踪**: 记录所有执行历史
