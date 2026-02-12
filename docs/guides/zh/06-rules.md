# Rules 模块

**包名**: `neomind-rules`
**版本**: 0.5.8
**完成度**: 75%
**用途**: DSL规则引擎

## 概述

Rules模块实现了基于DSL（领域特定语言）的规则引擎，支持从自然语言创建和管理自动化规则。

## 模块结构

```
crates/rules/src/
├── lib.rs                      # 公开接口
├── parser/                     # Pest DSL解析器
│   ├── mod.rs
│   └── grammar.pest             # Pest语法文件
├── engine/                     # 规则执行引擎
│   ├── mod.rs
│   ├── executor.rs
│   └── context.rs
├── types.rs                    # 规则类型
└── store.rs                    # 规则存储
```

## DSL语法

### 规则结构

```neo
# 触发器
ON <trigger>

# 条件
WHEN <conditions>

# 动作
THEN <actions>
```

### 完整示例

```neo
# 简单规则
ON device.temperature > 30
WHEN device.location == "greenhouse"
THEN send_alert("温度过高: {temperature}°C")

# 复杂规则
ON device.temperature
WHEN device.temperature > 30
   AND device.location == "greenhouse"
   AND time.between(9, 18)
THEN device.set_fan(true)
   AND send_alert("已开启风扇")

# 定时规则
ON schedule.every(1h)
THEN device.read_all_sensors()
   AND log_temperature()

# 多条件规则
ON ANY(
    device.temperature > 35,
    device.humidity > 80
)
WHEN device.status == "online"
THEN send_alert("温室环境异常")
```

## 核心类型

### 1. Rule - 规则定义

```rust
pub struct Rule {
    /// 规则ID
    pub id: String,

    /// 规则名称
    pub name: String,

    /// 规则描述
    pub description: String,

    /// 是否启用
    pub enabled: bool,

    /// 触发器
    pub trigger: Trigger,

    /// 条件列表
    pub conditions: Vec<Condition>,

    /// 动作列表
    pub actions: Vec<Action>,

    /// 元数据
    pub metadata: RuleMetadata,
}
```

### 2. Trigger - 触发器

```rust
pub enum Trigger {
    /// 设备事件触发
    Device {
        device_id: String,
        event_type: DeviceEventType,
    },

    /// 定时器触发
    Schedule {
        schedule: ScheduleConfig,
    },

    /// 数据条件触发
    Data {
        metric: String,
        comparison: ComparisonOperator,
        value: f64,
    },

    /// 复合触发器
    Any(Vec<Trigger>),
    All(Vec<Trigger>),
}

pub enum ScheduleConfig {
    /// 间隔执行
    Every {
        value: u64,
        unit: TimeUnit,
    },

    /// Cron表达式
    Cron(String),

    /// 特定时间
    At {
        hour: u8,
        minute: u8,
    },
}
```

### 3. Condition - 条件

```rust
pub struct Condition {
    /// 左值
    pub left: ConditionValue,

    /// 比较操作
    pub operator: ComparisonOperator,

    /// 右值
    pub right: ConditionValue,

    /// 逻辑连接符
    pub logic: Option<LogicOperator>,
}

pub enum ComparisonOperator {
    Eq,    // ==
    Ne,    // !=
    Gt,    // >
    Ge,    // >=
    Lt,    // <
    Le,    // <=
    Contains,
    Matches,  // 正则匹配
}

pub enum LogicOperator {
    And,
    Or,
    Xor,
}
```

### 4. Action - 动作

```rust
pub enum Action {
    /// 设备控制
    Device {
        device_id: String,
        command: String,
        parameters: serde_json::Value,
    },

    /// 发送消息
    SendMessage {
        channel: String,
        message: String,
    },

    /// 发送告警
    SendAlert {
        severity: AlertSeverity,
        message: String,
    },

    /// HTTP请求
    Http {
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<String>,
    },

    /// 设置变量
    SetVariable {
        name: String,
        value: serde_json::Value,
    },

    /// 延迟
    Delay {
        duration_ms: u64,
    },
}
```

## 规则解析器

```rust
pub struct RuleParser {
    /// Pest解析器
    parser: PestParser,
}

impl RuleParser {
    /// 解析规则文本
    pub fn parse(&self, input: &str) -> Result<Rule>;

    /// 验证规则语法
    pub fn validate(&self, input: &str) -> Result<()>;
}
```

### Pest语法

```pest
// grammar.pest

rule = { SOI ~ trigger ~ conditions? ~ actions ~ EOI }

trigger = { "ON" ~ ~condition }

condition = { comparison }

comparison = {
    ~ value ~ operator ~ value
    | "ANY(" ~ condition_list ~ ")"
    | "ALL(" ~ condition_list ~ ")"
}

operator = {
    "==" | "!=" | ">" | ">=" | "<" | "<="
    | "contains" | "matches"
}

action = {
    device_action
    | send_alert_action
    | send_message_action
    | http_action
    | delay_action
}

value = {
    string | number | boolean
    | device_ref | time_ref
}
```

## 规则执行引擎

```rust
pub struct RuleEngine {
    /// 规则存储
    store: Arc<RuleStore>,

    /// 事件总线
    event_bus: Arc<EventBus>,

    /// 设备服务
    device_service: Arc<DeviceService>,

    /// 消息服务
    message_service: Arc<MessageService>,
}

impl RuleEngine {
    /// 创建规则引擎
    pub fn new(
        store: Arc<RuleStore>,
        event_bus: Arc<EventBus>,
    ) -> Self;

    /// 注册规则
    pub async fn register_rule(&self, rule: Rule) -> Result<()>;

    /// 启用/禁用规则
    pub async fn set_rule_enabled(&self, id: &str, enabled: bool) -> Result<()>;

    /// 评估规则
    pub async fn evaluate_rule(
        &self,
        rule_id: &str,
        context: &EvaluationContext,
    ) -> RuleResult;

    /// 执行动作
    pub async fn execute_actions(
        &self,
        actions: &[Action],
        context: &EvaluationContext,
    ) -> Result<Vec<ActionResult>>;

    /// 启动引擎
    pub async fn start(&self) -> Result<()>;

    /// 停止引擎
    pub async fn stop(&self) -> Result<()>;
}
```

### 评估上下文

```rust
pub struct EvaluationContext {
    /// 当前时间
    pub timestamp: i64,

    /// 设备状态
    pub device_states: HashMap<String, DeviceState>,

    /// 变量
    pub variables: HashMap<String, serde_json::Value>,

    /// 触发数据
    pub trigger_data: Option<serde_json::Value>,
}
```

## 规则历史

```rust
pub struct RuleExecutionRecord {
    /// 执行ID
    pub id: String,

    /// 规则ID
    pub rule_id: String,

    /// 触发时间
    pub triggered_at: i64,

    /// 执行结果
    pub result: RuleExecutionResult,

    /// 动作结果
    pub action_results: Vec<ActionResult>,

    /// 耗时
    pub duration_ms: u64,
}

pub enum RuleExecutionResult {
    /// 触发并成功
    Triggered,

    /// 触发但失败
    Failed { error: String },

    /// 未触发
    NotTriggered,
}
```

## API端点

```
# Rules CRUD
GET    /api/rules                           # 列出规则
POST   /api/rules                           # 创建规则
GET    /api/rules/:id                       # 获取规则
PUT    /api/rules/:id                       # 更新规则
DELETE /api/rules/:id                       # 删除规则
POST   /api/rules/:id/enable                # 启用/禁用规则

# Rule Operations
POST   /api/rules/:id/test                  # 测试规则
GET    /api/rules/:id/history               # 规则执行历史
POST   /api/rules/validate                  # 验证规则DSL

# Rule Templates
GET    /api/rules/templates                 # 规则模板
POST   /api/rules/from-nl                   # 从自然语言生成规则
```

## 使用示例

### 创建规则

```rust
use neomind-rules::{Rule, Trigger, Condition, Action, ComparisonOperator};

let rule = Rule {
    id: "temp_alert".to_string(),
    name: "温度告警".to_string(),
    description: "温室温度过高时告警".to_string(),
    enabled: true,
    trigger: Trigger::Data {
        metric: "temperature".to_string(),
        comparison: ComparisonOperator::Gt,
        value: 30.0,
    },
    conditions: vec![
        Condition {
            left: ConditionValue::DeviceField("location".to_string()),
            operator: ComparisonOperator::Eq,
            right: ConditionValue::String("greenhouse".to_string()),
            logic: None,
        },
    ],
    actions: vec![
        Action::SendAlert {
            severity: AlertSeverity::Warning,
            message: "温度过高: {temperature}°C".to_string(),
        },
    ],
    metadata: RuleMetadata::default(),
};
```

### DSL解析

```rust
use neomind-rules::RuleParser;

let parser = RuleParser::new();

let rule_text = r#"
ON device.temperature > 30
WHEN device.location == "greenhouse"
THEN send_alert("温度过高")
"#;

let rule = parser.parse(rule_text)?;
engine.register_rule(rule).await?;
```

### 测试规则

```bash
curl -X POST http://localhost:3000/api/rules/test \
  -H "Content-Type: application/json" \
  -d '{
    "rule": "ON device.temperature > 30 THEN send_alert(\"高温\")",
    "context": {
      "device": {
        "temperature": 32,
        "location": "greenhouse"
      }
    }
  }'
```

## 设计原则

1. **DSL优先**: 使用简洁的DSL语法
2. **可测试**: 所有规则都可以测试
3. **事件驱动**: 基于EventBus触发
4. **可组合**: 支持复杂条件组合
