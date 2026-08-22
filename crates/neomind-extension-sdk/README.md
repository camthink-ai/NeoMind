# NeoMind Extension SDK V2

**版本**: 0.6.4 | **ABI 版本**: 3

统一的 NeoMind 扩展开发工具包，支持 Native 和 WASM 目标。

## 特性

- **统一 SDK** - Native 和 WASM 单一代码库
- **简化 FFI** - 一行宏导出所有 FFI 函数
- **ABI 版本 3** - 新的扩展接口，改进的安全性
- **类型安全** - 完整的类型定义和辅助宏
- **异步支持** - 基于 Tokio 的异步运行时
- **能力系统** - 20 种内置能力，支持自定义扩展

## 能力系统

扩展可以通过能力系统访问 NeoMind 平台功能：

### 内置能力

| 能力 | 名称 | 描述 |
|------|------|------|
| DeviceMetricsRead | `device_metrics_read` | 读取设备指标 |
| DeviceMetricsWrite | `device_metrics_write` | 写入虚拟指标 |
| DeviceControl | `device_control` | 发送设备命令 |
| StorageQuery | `storage_query` | 存储查询 |
| EventPublish | `event_publish` | 发布事件 |
| EventSubscribe | `event_subscribe` | 订阅事件 |
| TelemetryHistory | `telemetry_history` | 遥测历史查询 |
| MetricsAggregate | `metrics_aggregate` | 指标聚合计算 |
| ExtensionCall | `extension_call` | 扩展间调用 |
| AgentTrigger | `agent_trigger` | 触发代理执行 |
| RuleTrigger | `rule_trigger` | 触发规则执行 |
| ChatStream | `chat_stream` | 聊天流式推理 |
| ChatStreamCancel | `chat_stream_cancel` | 取消聊天流 |
| ChatSessionOpen | `chat_session_open` | 打开聊天会话 |
| ChatSessionSend | `chat_session_send` | 发送会话消息 |
| ChatSessionClose | `chat_session_close` | 关闭聊天会话 |
| ChatStreamCancelTurn | `chat_stream_cancel_turn` | 取消当前回合 |
| DeviceTemplateRegister | `device_template_register` | 注册设备类型模板 |
| DeviceRegister | `device_register` | 注册设备 |
| DeviceUnregister | `device_unregister` | 注销设备 |

### 使用能力 API

```rust
use neomind_extension_sdk::capabilities::{device, event, agent, rule};

// 读取设备指标
let metrics = device::get_metrics(&context, "device-1").await?;

// 写入虚拟指标
device::write_virtual_metric(&context, "device-1", "calculated_value", &json!(42.5)).await?;

// 发送设备命令
device::send_command(&context, "device-1", "set_level", &json!({"level": 80})).await?;

// 发布事件
event::publish(&context, event).await?;

// 触发代理
agent::trigger(&context, "analyzer-agent", &json!({"query": "analyze"})).await?;

// 触发规则
rule::trigger(&context, "alert-rule", &json!({"value": 85})).await?;
```

## 快速开始

### 安装

在扩展的 `Cargo.toml` 中添加：

```toml
[dependencies]
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }
```

### 基本用法

```rust
use neomind_extension_sdk::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self { counter: AtomicI64::new(0) }
    }
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| ExtensionMetadata {
            id: "my-extension".to_string(),
            name: "My Extension".to_string(),
            version: Version::parse("1.0.0").unwrap(),
            description: Some("My extension".to_string()),
            author: Some("Your Name".to_string()),
            ..Default::default()
        })
    }

    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
        match cmd {
            "increment" => {
                let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(cmd.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![ExtensionMetricValue {
            name: "counter".to_string(),
            value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }])
    }
}

// 导出 FFI - 只需要这一行！
neomind_extension_sdk::neomind_export!(MyExtension);
```

## API 参考

### Extension Trait

所有扩展必须实现 `Extension` trait：

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    /// 扩展元数据（必需）
    fn metadata(&self) -> &ExtensionMetadata;

    /// 声明指标（可选）
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// 声明命令（可选）
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// 执行命令（必需）
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// 生成指标数据（可选）
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// 健康检查（可选）
    async fn health_check(&self) -> Result<bool> { Ok(true) }
}
```

### 宏

#### `neomind_export!`

导出所有 FFI 函数：

```rust
// 基本用法
neomind_extension_sdk::neomind_export!(MyExtension);

// 自定义构造函数
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, with_config);
```

#### 辅助宏

```rust
// 创建度量值
metric_int!("counter", 42);
metric_float!("temperature", 23.5);
metric_bool!("active", true);
metric_string!("status", "running");

// 日志
ext_info!("Extension started");
ext_debug!("Processing item {}", id);
ext_warn!("Rate limit approaching");
ext_error!("Failed: {}", err);
```

### 辅助类型

```rust
// 构建度量描述符
let metric = MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-50.0)
    .max(50.0)
    .required()
    .build();

// 构建命令定义
let command = CommandBuilder::new("increment")
    .display_name("Increment")
    .llm_hints("Increment the counter")
    .param_simple("amount", "Amount", MetricDataType::Integer)
    .sample(json!({ "amount": 1 }))
    .build();

// 构建参数定义
let param = ParamBuilder::new("amount", MetricDataType::Integer)
    .display_name("Amount")
    .description("Amount to add")
    .default(ParamMetricValue::Integer(1))
    .min(1.0)
    .max(100.0)
    .build();
```

## 类型

### ExtensionMetadata

```rust
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub file_path: Option<PathBuf>,
    pub config_parameters: Option<Vec<ConfigParameter>>,
}
```

### ExtensionCommand

```rust
pub struct ExtensionCommand {
    pub name: String,
    pub display_name: String,
    pub payload_template: String,
    pub parameters: Vec<ParameterDefinition>,
    pub fixed_values: HashMap<String, Value>,
    pub samples: Vec<Value>,
    pub llm_hints: String,
    pub parameter_groups: Vec<ParameterGroup>,
}
```

### ExtensionMetricValue

```rust
pub struct ExtensionMetricValue {
    pub name: String,
    pub value: ParamMetricValue,
    pub timestamp: i64,
}

pub enum ParamMetricValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}
```

## 安全要求

扩展必须使用 `panic = "unwind"` 编译：

```toml
# Cargo.toml
[profile.release]
panic = "unwind"  # 安全性必需！
opt-level = 3
lto = "thin"
```

## 命名规范

```
扩展 ID: {category}-{name}-v{major}

示例:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2

库文件: libneomind_extension_{name}_v{major}.{ext}
```

## 示例

参考 [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) 仓库：

| 扩展 | 类型 | 说明 |
|------|------|------|
| weather-forecast-v2 | Native | 天气预报 API |
| image-analyzer-v2 | Native | YOLOv8 图像分析 |
| yolo-video-v2 | Native | 实时视频处理 |

## WASM 扩展开发

SDK 支持编译为 WebAssembly，提供与 Native 扩展相同的 API。

### 编译目标

```bash
# 添加 WASM 目标
rustup target add wasm32-unknown-unknown

# 编译 WASM 扩展
cargo build --target wasm32-unknown-unknown --release
```

### WASM 特性

```toml
# Cargo.toml
[dependencies]
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }

[lib]
crate-type = ["cdylib"]
```

### WASM 能力 API

WASM 扩展使用与 Native 相同的能力 API：

```rust
// Native: 异步 API
#[cfg(not(target_arch = "wasm32"))]
let metrics = device::get_metrics(&context, "device-1").await?;

// WASM: 同步 API（自动选择）
#[cfg(target_arch = "wasm32")]
let metrics = device::get_metrics(&context, "device-1")?;
```

### Host 函数接口

WASM 扩展通过 Host 函数与 NeoMind 平台交互：

| Host 函数 | 说明 |
|-----------|------|
| `host_invoke_capability` | 通用能力调用 |
| `host_event_subscribe` | 事件订阅 |
| `host_event_poll` | 事件轮询 |
| `host_event_unsubscribe` | 取消订阅 |
| `host_log` | 日志输出 |
| `host_timestamp_ms` | 获取时间戳 |
| `host_free` | 释放内存 |

### WASM 限制

- 同步 API（非异步）
- 通过 Host 函数访问平台能力
- 事件使用轮询模式
- 内存由 Host 管理

## 内置能力提供者

NeoMind 提供以下内置能力提供者：

| Provider | 提供能力 |
|----------|---------|
| `DeviceCapabilityProvider` | DeviceMetricsRead, DeviceMetricsWrite, DeviceControl |
| `EventCapabilityProvider` | EventPublish, EventSubscribe |
| `TelemetryCapabilityProvider` | TelemetryHistory, MetricsAggregate |
| `AgentCapabilityProvider` | AgentTrigger |
| `RuleCapabilityProvider` | RuleTrigger |
| `ExtensionCallCapabilityProvider` | ExtensionCall |
| Chat 系列能力 | ChatStream, ChatStreamCancel, ChatSessionOpen/Send/Close, ChatStreamCancelTurn |
| 设备注册能力 | DeviceTemplateRegister, DeviceRegister, DeviceUnregister |

## 许可证

Apache-2.0
