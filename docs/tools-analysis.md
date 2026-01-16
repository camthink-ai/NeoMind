# NeoTalk 工具系统分析报告

## 1. 现有工具清单

### 1.1 内置工具 (Built-in Tools)

| 工具名 | 命名空间 | 功能 | 必需参数 |
|--------|----------|------|----------|
| `query_data` | data | 查询设备时序数据 | `device_id` |
| `control_device` | device | 控制设备（发送命令） | `device_id`, `command` |
| `list_devices` | device | 列出所有设备 | - |
| `get_device_metrics` | device | 获取设备可用指标 | `device_id` |
| `get_device_type_schema` | device | 获取设备类型 MDL 定义 | - |
| `list_device_types` | device | 列出所有设备类型 | - |
| `create_rule` | rule | 创建自动化规则 | `name`, `dsl` |
| `list_rules` | rule | 列出所有规则 | - |
| `trigger_workflow` | workflow | 触发工作流执行 | `workflow_id` |

### 1.2 Agent 分析工具

| 工具名 | 命名空间 | 功能 | 文件位置 |
|--------|----------|------|----------|
| `analyze_trends` | analysis | 分析时序趋势并预测 | `trends.rs` |
| `detect_anomalies` | analysis | 检测数据异常 | `anomalies.rs` |
| `propose_decision` | decision | 提出 LLM 决策建议 | `decisions.rs` |
| `execute_decision` | decision | 执行决策 | `decisions.rs` |

### 1.3 Agent 系统工具

| 工具名 | 命名空间 | 功能 | 文件位置 |
|--------|----------|------|----------|
| `think` | system | 结构化推理工具 | `think.rs` |
| `tool_search` | system | 搜索可用工具 | `tool_search.rs` |

### 1.4 Agent MDL 工具

| 工具名 | 命名空间 | 功能 |
|--------|----------|------|
| `list_device_types` | mdl | 列出设备类型 |
| `get_device_type` | mdl | 获取特定设备类型详情 |
| `explain_device_type` | mdl | 解释设备类型能力 |

### 1.5 Agent DSL 工具

| 工具名 | 命名空间 | 功能 |
|--------|----------|------|
| `list_rules` | dsl | 列出规则 |
| `get_rule` | dsl | 获取规则详情 |
| `explain_rule` | dsl | 解释规则逻辑 |
| `get_rule_history` | dsl | 获取规则执行历史 |

### 1.6 Agent 规则生成工具

| 工具名 | 命名空间 | 功能 |
|--------|----------|------|
| `generate_rule_dsl` | rule_gen | 从自然语言生成 DSL |
| `validate_rule_dsl` | rule_gen | 验证 DSL 语法 |
| `create_rule` | rule_gen | 从验证后的 DSL 创建规则 |

## 2. 工具请求/响应格式

### 2.1 请求格式

```json
{
  "name": "tool_name",
  "arguments": {
    "param1": "value1",
    "param2": "value2"
  },
  "id": "optional_call_id"
}
```

### 2.2 响应格式

```json
{
  "success": true,
  "data": {
    // 工具特定结果数据
  },
  "error": null,
  "metadata": {
    // 可选元数据
  }
}
```

### 2.3 LLM 函数调用格式

工具通过 `ToolDefinition` 结构体暴露给 LLM：

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,        // JSON Schema 格式
    pub examples: Vec<ToolExample>,
    pub response_format: ResponseFormat,
    pub namespace: Option<String>,
}
```

## 3. 工具关系图

```
设备管理 (device)
├── list_devices        → get_device_metrics
├── get_device_metrics  → query_data
├── control_device
└── get_device_type_schema

规则管理 (rule)
├── list_rules          → get_rule
├── get_rule            → explain_rule
├── create_rule         → validate_rule_dsl
└── get_rule_history

工作流 (workflow)
└── trigger_workflow    → query_workflow_status

数据分析 (analysis)
├── query_data          → analyze_trends
├── query_data          → detect_anomalies
└── detect_anomalies    → propose_decision

系统 (system)
├── tool_search
└── think
```

## 4. 当前问题分析

### 4.1 工具定义问题

| 问题 | 影响 | 严重程度 |
|------|------|----------|
| 缺少工具关系元数据 | LLM 无法理解工具调用顺序 | 高 |
| 缺少工具依赖声明 | 无法自动预加载相关工具 | 中 |
| 参数类型验证简单 | 运行时错误可能较多 | 中 |
| 缺少工具版本管理 | 向后兼容问题 | 低 |

### 4.2 工具发现问题

| 问题 | 影响 | 严重程度 |
|------|------|----------|
| 无工具分类/标签 | 难以按场景过滤工具 | 中 |
| 工具搜索只匹配名称/描述 | 语义理解不足 | 中 |
| 缺少工具使用统计 | 无法优化提示顺序 | 低 |

### 4.3 工具执行问题

| 问题 | 影响 | 严重程度 |
|------|------|----------|
| 无工具执行超时控制 | 可能挂起 | 高 |
| 无工具执行重试策略 | 网络问题时失败 | 中 |
| 缺少工具执行缓存 | 重复调用浪费资源 | 中 |
| 并行执行缺少依赖管理 | 可能错误执行顺序 | 高 |

## 5. 缺失工具建议

### 5.1 高优先级缺失工具

| 工具名 | 功能 | 原因 |
|--------|------|------|
| `delete_rule` | 删除规则 | 已有 create 但无 delete |
| `enable_rule` / `disable_rule` | 启用/禁用规则 | 规则生命周期管理 |
| `update_rule` | 更新规则 | 已有 create 但无 update |
| `query_device_status` | 查询设备在线状态 | 设备管理核心功能 |
| `batch_control_devices` | 批量控制设备 | 场景常见 |
| `get_device_history` | 获取设备历史状态 | 时序数据查询 |

### 5.2 中优先级缺失工具

| 工具名 | 功能 | 原因 |
|--------|------|------|
| `search_devices` | 按条件搜索设备 | 过滤能力增强 |
| `get_device_config` | 获取设备配置 | 配置管理 |
| `set_device_config` | 设置设备配置 | 配置管理 |
| `export_rules` | 导出规则 | 备份/迁移 |
| `import_rules` | 导入规则 | 备份/迁移 |
| `get_alerts` | 获取告警列表 | 告警管理 |
| `acknowledge_alert` | 确认告警 | 告警管理 |

### 5.3 分析工具增强

| 工具名 | 功能 | 原因 |
|--------|------|------|
| `compare_metrics` | 比较多个指标 | 分析场景常见 |
| `aggregate_data` | 聚合时序数据 | 统计分析 |
| `forecast_data` | 预测数据趋势 | 预测性维护 |
| `correlation_analysis` | 相关性分析 | 根因分析 |

## 6. 优化建议

### 6.1 工具定义增强

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,

    // 新增字段
    pub category: ToolCategory,        // 工具分类
    pub dependencies: Vec<String>,      // 依赖的其他工具
    pub exclusive_with: Vec<String>,    // 互斥工具
    pub version: semver::Version,       // 工具版本
    pub deprecated: bool,               // 是否废弃
    pub replaced_by: Option<String>,    // 替代工具

    pub examples: Vec<ToolExample>,
    pub response_format: ResponseFormat,
    pub namespace: Option<String>,
}

pub enum ToolCategory {
    Device,      // 设备操作
    Data,        // 数据查询
    Analysis,    // 数据分析
    Rule,        // 规则管理
    Workflow,    // 工作流
    System,      // 系统操作
    Notification,// 通知
}
```

### 6.2 工具关系元数据

```rust
pub struct ToolRelationship {
    /// 工具可以调用的其他工具
    pub can_call: Vec<String>,

    /// 此工具应该被调用的条件
    pub conditions: Vec<CallCondition>,

    /// 此工具的输出是哪些工具的输入
    pub outputs_to: Vec<String>,

    /// 建议的工具调用顺序
    pub suggested_order: usize,
}

pub enum CallCondition {
    After(String),           // 在某工具之后调用
    WhenDataAvailable(String), // 当某数据可用时
    RequiresSuccess(String),  // 需要某工具成功
}
```

### 6.3 工具执行策略

```rust
pub struct ToolExecutionConfig {
    /// 超时时间
    pub timeout: Duration,

    /// 重试策略
    pub retry_policy: RetryPolicy,

    /// 是否可并行
    pub parallel_safe: bool,

    /// 缓存策略
    pub cache_policy: CachePolicy,

    /// 优先级
    pub priority: u8,
}

pub enum RetryPolicy {
    None,
    Fixed { attempts: u32, delay: Duration },
    Exponential { max_attempts: u32, base_delay: Duration },
}

pub enum CachePolicy {
    None,
    TTL(Duration),
    LRU { capacity: usize },
}
```

### 6.4 LLM 提示词优化

当前工具提示词格式较为简单，建议增强：

```
## 工具: query_data
**描述**: 查询设备时序数据
**用途**: 获取设备的历史或当前数据

**参数**:
- device_id (required): 设备ID
- metric (optional): 指标名称，不指定则返回所有
- start_time (optional): 开始时间戳，默认24小时前
- end_time (optional): 结束时间戳，默认现在
- limit (optional): 返回数据点数量限制

**使用场景**:
- 用户询问设备当前读数时
- 用户询问历史数据时
- 需要分析设备数据趋势时

**相关工具**:
- → analyze_trends: 分析返回的数据趋势
- → detect_anomalies: 检测返回的异常

**示例**:
输入: "温度传感器最近一小时的数据"
调用: query_data(device_id="sensor_1", metric="temperature", start_time=<1小时前>)
```

## 7. 实施计划

### Phase 1: 补充缺失工具 (高优先级)

1. `delete_rule` - 删除规则
2. `enable_rule` / `disable_rule` - 启用/禁用规则
3. `update_rule` - 更新规则
4. `query_device_status` - 查询设备状态
5. `batch_control_devices` - 批量控制

### Phase 2: 工具元数据增强

1. 添加工具分类
2. 添加工具依赖关系
3. 添加工具版本管理
4. 完善工具示例

### Phase 3: 执行策略优化

1. 添加超时控制
2. 添加重试机制
3. 添加结果缓存
4. 优化并行执行策略

### Phase 4: LLM 集成优化

1. 改进工具提示词格式
2. 添加工具使用场景说明
3. 添加工具调用引导
4. 实现工具推荐系统

## 8. 工具定义模板

```rust
// 创建新工具的模板

use async_trait::async_trait;
use serde_json::Value;
use edge_ai_tools::{Tool, ToolOutput, object_schema, string_property};

pub struct NewTool {
    // 工具依赖
    dependency: Arc<SomeDependency>,
}

impl NewTool {
    pub fn new(dependency: Arc<SomeDependency>) -> Self {
        Self { dependency }
    }
}

#[async_trait]
impl Tool for NewTool {
    fn name(&self) -> &str {
        "new_tool"
    }

    fn description(&self) -> &str {
        "工具的简短描述，说明何时使用"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "param1": string_property("参数1描述"),
                "param2": {
                    "type": "number",
                    "description": "参数2描述",
                    "default": 100
                }
            }),
            vec!["param1".to_string()], // 必需参数
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        // 1. 提取参数
        let param1 = args["param1"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("param1 required".to_string()))?;

        // 2. 执行逻辑
        let result = self.dependency
            .do_something(param1)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        // 3. 返回结果
        Ok(ToolOutput::success(serde_json::json!({
            "result": result
        })))
    }

    // 可选：覆盖命名空间
    fn namespace(&self) -> Option<&str> {
        Some("category")
    }
}
```
