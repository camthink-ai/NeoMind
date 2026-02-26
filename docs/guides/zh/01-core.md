# Core 模块

**包名**: `neomind-core`
**版本**: 0.5.8
**完成度**: 90%
**用途**: 定义整个项目的核心trait和类型

## 概述

Core 模块是 NeoMind 项目的基石，定义了所有其他模块依赖的核心抽象和类型。它不包含具体实现，只提供接口定义。

## 模块结构

```
crates/core/src/
├── lib.rs                  # 公开接口导出
├── event.rs                # 事件类型定义
├── eventbus.rs             # 事件总线实现
├── priority_eventbus.rs    # 优先级事件总线
├── message.rs              # 消息类型定义
├── session.rs              # 会话类型定义
├── llm/
│   ├── backend.rs          # LLM运行时trait
│   ├── modality.rs         # 多模态内容支持
│   └── memory_consolidation.rs  # 内存整合
├── tools/
│   └── mod.rs              # 工具trait定义
├── storage/
│   └── mod.rs              # 存储trait定义
├── integration/
│   ├── mod.rs              # 集成trait
│   ├── connector.rs        # 连接器trait
│   └── transformer.rs      # 数据转换trait
├── datasource/
│   ├── mod.rs              # 数据源ID系统
│   └── types.rs            # DataSourceId类型
├── extension/
│   ├── mod.rs              # 扩展系统
│   ├── types.rs            # 扩展类型
│   ├── registry.rs         # 扩展注册表
│   └── loader/             # 扩展加载器
├── alerts/
│   └── mod.rs              # 告警系统
├── config.rs               # 配置常量
├── error.rs                # 错误类型
└── macros.rs               # 宏定义
```

## 核心Trait

### 1. LlmRuntime - LLM运行时接口

定义了所有LLM后端必须实现的接口。

```rust
#[async_trait]
pub trait LlmRuntime: Send + Sync {
    /// 获取后端能力
    fn capabilities(&self) -> BackendCapabilities;

    /// 生成文本（非流式）
    fn generate(&self, input: &LlmInput) -> Result<LlmOutput>;

    /// 生成文本（流式）
    fn generate_stream(&self, input: &LlmInput) -> StreamResult;

    /// 嵌入向量生成（可选）
    fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}
```

**BackendCapabilities**:
```rust
pub struct BackendCapabilities {
    /// 支持流式输出
    pub streaming: bool,
    /// 支持函数调用
    pub function_calling: bool,
    /// 支持视觉输入
    pub vision: bool,
    /// 支持thinking模式
    pub thinking: bool,
}
```

### 2. Tool - 工具接口

定义了AI可调用工具的接口。

```rust
pub trait Tool: Send + Sync {
    /// 工具定义（名称、描述、参数）
    fn definition(&self) -> &ToolDefinition;

    /// 执行工具
    fn execute(&self, input: &serde_json::Value) -> Result<ToolOutput>;

    /// 验证输入
    fn validate(&self, input: &serde_json::Value) -> Result<()> {
        // 默认实现
    }
}
```

### 3. Integration - 集成接口

定义了外部系统集成的接口。

```rust
#[async_trait]
pub trait Integration: Send + Sync {
    /// 获取元数据
    fn metadata(&self) -> &IntegrationMetadata;

    /// 获取当前状态
    fn state(&self) -> IntegrationState;

    /// 启动集成
    async fn start(&self) -> Result<()>;

    /// 停止集成
    async fn stop(&self) -> Result<()>;

    /// 订阅事件流
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>>;

    /// 发送命令
    async fn send_command(&self, command: IntegrationCommand) -> Result<IntegrationResponse>;
}
```

### 4. Extension - 扩展接口

定义了动态加载扩展的接口。V2 Extension trait 将指标和命令分离：

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    /// 获取扩展元数据
    fn metadata(&self) -> &ExtensionMetadata;

    /// 声明扩展提供的指标
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// 声明扩展支持的命令
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// 执行命令（异步）
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// 生成指标数据（同步，兼容动态库）
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// 健康检查（异步，可选）
    async fn health_check(&self) -> Result<bool> { Ok(true) }

    /// 运行时配置（可选）
    async fn configure(&mut self, config: &Value) -> Result<()> { Ok(()) }
}
```

完整 API 详情请参考 [扩展开发指南](16-extension-dev.md)。

### 5. DataSourceId - 数据源标识

提供类型安全的数据源标识，支持设备、扩展和转换数据源。

```rust
pub struct DataSourceId {
    /// 数据源类型
    pub source_type: DataSourceType,
    /// 数据源ID
    pub source_id: String,
    /// 字段路径
    pub field_path: String,
}

pub enum DataSourceType {
    Device,
    Extension,
    Transform,
}

impl DataSourceId {
    /// 解析数据源ID
    pub fn new(id: &str) -> Result<Self>;

    /// 获取TimeSeriesStorage使用的device_id部分
    pub fn device_part(&self) -> String;

    /// 获取TimeSeriesStorage使用的metric部分
    pub fn metric_part(&self) -> &str;

    /// 获取完整的存储键
    pub fn storage_key(&self) -> String;
}
```

**格式**: `{source_type}:{source_id}:{field_path}`

| 类型 | 格式 | device_part | metric_part |
|------|------|-------------|-------------|
| Device | `{device_id}:{field_path}` | `{device_id}` | `{field_path}` |
| Extension | `extension:{ext_id}:{field_path}` | `extension:{ext_id}` | `{field_path}` |
| Transform | `transform:{trans_id}:{field_path}` | `transform:{trans_id}` | `{field_path}` |

### 6. StorageBackend - 存储接口

定义了通用存储接口。

```rust
pub trait StorageBackend: Send + Sync {
    /// 写入数据
    async fn write(&self, key: &str, value: &[u8]) -> Result<()>;

    /// 读取数据
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// 删除数据
    async fn delete(&self, key: &str) -> Result<()>;

    /// 列出键
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;
}
```

## 核心类型

### Message - 消息类型

```rust
pub struct Message {
    pub role: MessageRole,
    pub content: Content,
    pub timestamp: Option<i64>,
    pub metadata: Option<EventMetadata>,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),  // 支持多模态
}

pub enum ContentPart {
    Text { text: String },
    Image { image: ImageContent },
    Thinking { content: String },  // 推理内容
}
```

### NeoMindEvent - 事件类型

```rust
pub enum NeoMindEvent {
    // 设备事件
    DeviceOnline { device_id: String, timestamp: i64 },
    DeviceOffline { device_id: String, timestamp: i64 },
    DeviceMetric { device_id: String, metric: String, value: MetricValue },
    DeviceCommandResult { device_id: String, command: String, success: bool },

    // 规则事件
    RuleEvaluated { rule_id: String, result: bool },
    RuleTriggered { rule_id: String, trigger_value: serde_json::Value },

    // 工作流事件
    WorkflowTriggered { workflow_id: String },
    WorkflowStepCompleted { workflow_id: String, step: String },
    WorkflowCompleted { workflow_id: String },

    // LLM事件
    PeriodicReviewTriggered { review_id: String },
    LlmDecisionProposed { decision_id: String, title: String },
    LlmDecisionExecuted { decision_id: String, success: bool },

    // 消息事件
    MessageCreated { message_id: String, severity: MessageSeverity },
    MessageAcknowledged { message_id: String },
}
```

### Session - 会话类型

```rust
pub struct Session {
    pub id: SessionId,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<Message>,
    pub metadata: SessionMetadata,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);
```

## EventBus - 事件总线

```rust
pub struct EventBus {
    // 通道容量
    capacity: usize,
    // 广播发送器
    sender: broadcast::Sender<NeoMindEvent>,
}

impl EventBus {
    /// 创建新的事件总线
    pub fn new() -> Self;

    /// 创建带容量的事件总线
    pub fn with_capacity(capacity: usize) -> Self;

    /// 发布事件
    pub fn publish(&self, event: NeoMindEvent);

    /// 订阅所有事件
    pub fn subscribe(&self) -> EventBusReceiver;

    /// 创建过滤器
    pub fn filter(&self) -> FilterBuilder;
}
```

### FilterBuilder - 事件过滤

```rust
pub struct FilterBuilder<'a> {
    bus: &'a EventBus,
    filters: Vec<FilterFn>,
}

impl<'a> FilterBuilder<'a> {
    /// 只接收设备事件
    pub fn device_events(self) -> FilteredReceiver;

    /// 只接收规则事件
    pub fn rule_events(self) -> FilteredReceiver;

    /// 自定义过滤
    pub fn custom<F>(self, f: F) -> FilteredReceiver
    where
        F: Fn(&NeoMindEvent) -> bool + Send + 'static;
}
```

## 配置常量

```rust
// LLM提供商配置
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://localhost:11434";
pub const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1";

// 默认模型
pub fn models() -> Vec<&'static str> {
    vec![
        "qwen3-vl:2b",     // Ollama默认
        "gpt-4o-mini",     // OpenAI
        "claude-3-5-sonnet", // Anthropic
    ]
}

// API端点
pub fn endpoints() -> HashMap<String, String> {
    // ...
}
```

## 使用示例

### 创建EventBus

```rust
use neomind-core::EventBus;
use neomind-core::NeoMindEvent;

#[tokio::main]
async fn main() {
    let bus = EventBus::new();

    // 订阅所有事件
    let mut rx = bus.subscribe();

    // 订阅设备事件
    let mut device_rx = bus.filter().device_events();

    // 发布事件
    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "sensor_1".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    });

    // 接收事件
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            println!("Received: {:?}", event);
        }
    });
}
```

### 使用LlmRuntime trait

```rust
use neomind-core::llm::backend::{LlmRuntime, LlmInput, GenerationParams};

async fn call_llm(runtime: &dyn LlmRuntime, prompt: &str) -> Result<String> {
    let input = LlmInput {
        messages: vec![
            Message::user(prompt)
        ],
        params: GenerationParams::default(),
        model: None,
    };

    let output = runtime.generate(&input)?;
    Ok(output.text)
}
```

## 错误处理

```rust
pub enum Error {
    /// LLM相关错误
    Llm(LlmError),

    /// 存储错误
    Storage(StorageError),

    /// 工具错误
    Tool(ToolError),

    /// 集成错误
    Integration(IntegrationError),

    /// 扩展错误
    Extension(ExtensionError),

    /// IO错误
    Io(std::io::Error),

    /// 其他错误
    Other(anyhow::Error),
}
```

## 依赖关系

```
Core (neomind-core)
    │
    ├── 无外部依赖（仅定义trait）
    │
    └── 被所有其他crate依赖
        ├── llm
        ├── agent
        ├── devices
        ├── tools
        ├── storage
        ├── integrations
        └── ...
```

## 设计原则

1. **最小依赖**: Core模块不依赖任何其他业务模块
2. **trait优先**: 通过trait定义接口，允许不同实现
3. **类型安全**: 使用Rust类型系统确保正确性
4. **异步优先**: 所有I/O操作都是异步的
5. **可扩展性**: 通过trait和事件实现松耦合
