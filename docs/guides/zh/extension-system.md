# NeoMind 扩展体系完整指南

**版本**: 2.1.0  
**SDK 版本**: 2.0.0  
**ABI 版本**: 3  
**最后更新**: 2026-03-12

---

## 目录

1. [概述](#1-概述)
2. [架构设计](#2-架构设计)
3. [核心概念](#3-核心概念)
4. [扩展开发指南](#4-扩展开发指南)
5. [能力系统](#5-能力系统)
6. [进程隔离](#6-进程隔离)
7. [流式扩展](#7-流式扩展)
8. [扩展包格式](#8-扩展包格式)
9. [API 参考](#9-api-参考)
10. [最佳实践](#10-最佳实践)
11. [故障排除](#11-故障排除)
12. [附录](#12-附录)

---

## 1. 概述

### 1.1 什么是 NeoMind 扩展？

NeoMind 扩展是可动态加载的模块，用于扩展平台功能。扩展可以：

- **提供数据源** - 从外部系统获取数据（如天气 API、数据库连接器）
- **设备适配器** - 支持新的设备协议（如 Modbus、MQTT 自定义协议）
- **AI 工具** - 为 AI Agent 提供新能力（如图像分析、文本处理）
- **视频处理** - 实时视频分析和流处理
- **Dashboard 组件** - 提供自定义可视化组件

### 1.2 扩展类型

| 类型 | 格式 | 运行环境 | 特点 |
|------|------|----------|------|
| **Native** | `.so` / `.dylib` / `.dll` | 进程内或隔离进程 | 最高性能，完整功能 |
| **WASM** | `.wasm` | 隔离进程 + WASM 沙箱 | 跨平台，双重隔离 |
| **前端组件** | `.js` / `.css` | 浏览器 | Dashboard 可视化 |

### 1.3 核心特性

| 特性 | 说明 |
|------|------|
| **统一 SDK** | 单一依赖，同时支持 Native 和 WASM 目标 |
| **简化 FFI** | 一行宏导出所有 FFI 函数 |
| **ABI 版本 3** | 新的扩展接口，改进的安全性 |
| **进程隔离** | 扩展在独立进程中运行，确保主进程安全 |
| **能力系统** | 解耦的、版本化的 API，支持安全访问平台功能 |
| **流式支持** | 支持视频、音频、传感器等实时数据流处理 |

---

## 2. 架构设计

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          NeoMind 主进程                                  │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    UnifiedExtensionService                       │   │
│  │  - 统一 API 接口                                                 │   │
│  │  - 路由请求到适当的后端                                          │   │
│  │  - 生命周期管理                                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                      │                    │
│                              ▼                      ▼                    │
│  ┌───────────────────────────────┐  ┌───────────────────────────────┐  │
│  │      ExtensionRegistry         │  │   IsolatedExtensionManager    │  │
│  │  - 进程内扩展管理              │  │  - 进程隔离扩展管理           │  │
│  │  - 直接调用                    │  │  - IPC 通信                   │  │
│  │  - Native (.so/.dylib/.dll)    │  │  - Native + WASM              │  │
│  └───────────────────────────────┘  └───────────────────────────────┘  │
│                                                              │          │
│                                                              ▼          │
│                                           ┌───────────────────────────┐  │
│                                           │  neomind-extension-runner  │  │
│                                           │  - 独立进程                │  │
│                                           │  - WASM 运行时 (wasmtime)  │  │
│                                           │  - 能力转发                │  │
│                                           └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 核心模块

| 模块 | 路径 | 职责 |
|------|------|------|
| **system.rs** | `neomind-core/src/extension/` | Extension trait 定义、核心类型 |
| **registry.rs** | `neomind-core/src/extension/` | 扩展注册表，管理进程内扩展 |
| **unified.rs** | `neomind-core/src/extension/` | 统一扩展服务，整合两种模式 |
| **loader/native.rs** | `neomind-core/src/extension/loader/` | Native 扩展加载器 |
| **loader/isolated.rs** | `neomind-core/src/extension/loader/` | 隔离扩展加载器 |
| **isolated/manager.rs** | `neomind-core/src/extension/isolated/` | 隔离扩展管理器 |
| **isolated/process.rs** | `neomind-core/src/extension/isolated/` | 扩展进程管理 |
| **isolated/ipc.rs** | `neomind-core/src/extension/isolated/` | IPC 进程间通信 |
| **context.rs** | `neomind-core/src/extension/` | 能力系统和扩展上下文 |
| **stream.rs** | `neomind-core/src/extension/` | 流式扩展支持 |
| **package.rs** | `neomind-core/src/extension/` | .nep 包格式解析 |
| **safety.rs** | `neomind-core/src/extension/` | 安全管理器和熔断器 |

### 2.3 数据流

```
┌─────────────┐     ┌─────────────────────┐     ┌─────────────────┐
│   Client    │────►│   REST API / WebSocket │────►│ UnifiedService  │
└─────────────┘     └─────────────────────┘     └────────┬────────┘
                                                         │
                    ┌────────────────────────────────────┼────────────────────────────────────┐
                    │                                    │                                    │
                    ▼                                    ▼                                    ▼
          ┌─────────────────┐                  ┌─────────────────┐                  ┌─────────────────┐
          │  In-Process     │                  │  Isolated       │                  │  Event          │
          │  Extension      │                  │  Extension      │                  │  Dispatcher     │
          │  (Direct Call)  │                  │  (IPC)          │                  │                 │
          └─────────────────┘                  └────────┬────────┘                  └─────────────────┘
                                                        │
                                                        ▼
                                              ┌─────────────────┐
                                              │ Extension Runner│
                                              │ (Separate Proc) │
                                              └─────────────────┘
```

---

## 3. 核心概念

### 3.1 Extension Trait

所有扩展必须实现 `Extension` trait：

```rust
#[async_trait]
pub trait Extension: Send + Sync + 'static {
    // ===== 必需方法 =====
    
    /// 获取扩展元数据
    fn metadata(&self) -> &ExtensionMetadata;
    
    /// 执行命令
    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    
    // ===== 可选方法 =====
    
    /// 声明提供的指标
    fn metrics(&self) -> &[MetricDescriptor] { &[] }
    
    /// 声明支持的命令
    fn commands(&self) -> &[ExtensionCommand] { &[] }
    
    /// 生成指标数据
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }
    
    /// 健康检查
    async fn health_check(&self) -> Result<bool> { Ok(true) }
    
    /// 获取扩展统计信息
    fn get_stats(&self) -> ExtensionStats { ExtensionStats::default() }
    
    /// 运行时配置
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> { Ok(()) }
    
    // ===== 事件处理 =====
    
    /// 处理事件
    fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<()> { Ok(()) }
    
    /// 获取事件订阅列表
    fn event_subscriptions(&self) -> &[&str] { &[] }
    
    // ===== 流式支持 =====
    
    /// 获取流能力
    fn stream_capability(&self) -> Option<StreamCapability> { None }
    
    /// 处理数据块（无状态模式）
    async fn process_chunk(&self, _chunk: DataChunk) -> Result<StreamResult> { ... }
    
    /// 初始化会话（有状态模式）
    async fn init_session(&self, _session: &StreamSession) -> Result<()> { ... }
    
    /// 处理会话数据块
    async fn process_session_chunk(&self, _session_id: &str, _chunk: DataChunk) -> Result<StreamResult> { ... }
    
    /// 关闭会话
    async fn close_session(&self, _session_id: &str) -> Result<SessionStats> { ... }
    
    // ===== Push 模式支持 =====
    
    /// 设置输出发送器
    fn set_output_sender(&self, _sender: Arc<mpsc::Sender<PushOutputMessage>>) { }
    
    /// 开始推送数据
    async fn start_push(&self, _session_id: &str) -> Result<()> { ... }
    
    /// 停止推送数据
    async fn stop_push(&self, _session_id: &str) -> Result<()> { Ok(()) }
    
    /// 支持向下转型
    fn as_any(&self) -> &dyn std::any::Any;
}
```

### 3.2 ExtensionMetadata

扩展元数据描述扩展的基本信息：

```rust
pub struct ExtensionMetadata {
    /// 唯一扩展标识符
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 扩展版本
    pub version: semver::Version,
    /// 描述
    pub description: Option<String>,
    /// 作者
    pub author: Option<String>,
    /// 主页 URL
    pub homepage: Option<String>,
    /// 许可证
    pub license: Option<String>,
    /// 文件路径（内部使用）
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
    /// 配置参数定义
    pub config_parameters: Option<Vec<ParameterDefinition>>,
}
```

### 3.3 MetricDescriptor

指标描述符定义扩展提供的数据流：

```rust
pub struct MetricDescriptor {
    /// 指标名称
    pub name: String,
    /// 显示名称
    pub display_name: String,
    /// 数据类型
    pub data_type: MetricDataType,
    /// 单位
    pub unit: String,
    /// 最小值
    pub min: Option<f64>,
    /// 最大值
    pub max: Option<f64>,
    /// 是否必需
    pub required: bool,
}

/// 指标数据类型
pub enum MetricDataType {
    Float,
    Integer,
    Boolean,
    String,
    Binary,
    Enum { options: Vec<String> },
}
```

### 3.4 ExtensionCommand

命令定义扩展支持的操作：

```rust
pub struct ExtensionCommand {
    /// 命令名称
    pub name: String,
    /// 显示名称
    pub display_name: String,
    /// 描述（映射到 llm_hints）
    pub description: String,
    /// 负载模板
    pub payload_template: String,
    /// 参数定义
    pub parameters: Vec<ParameterDefinition>,
    /// 固定值
    pub fixed_values: HashMap<String, serde_json::Value>,
    /// 示例
    pub samples: Vec<serde_json::Value>,
    /// LLM 提示
    pub llm_hints: String,
    /// 参数分组
    pub parameter_groups: Vec<ParameterGroup>,
}
```

### 3.5 ExtensionDescriptor

扩展描述符是扩展能力的完整描述：

```rust
pub struct ExtensionDescriptor {
    /// 基本元数据
    pub metadata: ExtensionMetadata,
    /// 提供的命令
    pub commands: Vec<ExtensionCommand>,
    /// 提供的指标
    pub metrics: Vec<MetricDescriptor>,
}
```

### 3.6 ExtensionRuntimeState

运行时状态跟踪扩展的动态状态：

```rust
pub struct ExtensionRuntimeState {
    /// 是否正在运行
    pub is_running: bool,
    /// 是否在隔离模式运行
    pub is_isolated: bool,
    /// 加载时间（Unix 时间戳）
    pub loaded_at: Option<i64>,
    /// 重启次数
    pub restart_count: u64,
    /// 启动次数
    pub start_count: u64,
    /// 停止次数
    pub stop_count: u64,
    /// 错误次数
    pub error_count: u64,
    /// 最后错误信息
    pub last_error: Option<String>,
}
```

---

## 4. 扩展开发指南

### 4.1 快速开始

#### 步骤 1：创建项目

```bash
cargo new --lib my_extension
cd my_extension
```

#### 步骤 2：配置 Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"

[profile.release]
panic = "unwind"  # 安全性必需！
opt-level = 3
lto = "thin"
```

**重要配置说明**：
- `crate-type = ["cdylib"]` - 生成动态库
- `panic = "unwind"` - 必需，用于 panic 隔离

#### 步骤 3：实现扩展

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "my-extension".to_string(),
                name: "My Extension".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: Some("我的第一个 NeoMind 扩展".to_string()),
                author: Some("Your Name".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "counter".to_string(),
                    display_name: "Counter".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "increment".to_string(),
                    display_name: "Increment".to_string(),
                    description: "增加计数器".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "增加的数量".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({ "amount": 1 })],
                    llm_hints: "增加计数器的值".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            ExtensionMetricValue::new(
                "counter",
                ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst))
            ),
        ])
    }
}

// 导出 FFI - 只需要这一行！
neomind_extension_sdk::neomind_export!(MyExtension);
```

#### 步骤 4：编译

```bash
cargo build --release
```

输出文件：
- **macOS**: `target/release/libneomind_extension_my_extension.dylib`
- **Linux**: `target/release/libneomind_extension_my_extension.so`
- **Windows**: `target/release/neomind_extension_my_extension.dll`

#### 步骤 5：安装

```bash
# 创建扩展目录
mkdir -p ~/.neomind/extensions/my-extension

# 复制扩展文件
cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/my-extension/

# 通过 API 发现扩展
curl -X POST http://localhost:9375/api/extensions/discover
```

### 4.2 SDK Builder 类

SDK 提供了便捷的 Builder 类来简化开发：

#### MetricBuilder

```rust
use neomind_extension_sdk::MetricBuilder;

// 创建整数指标
let counter_metric = MetricBuilder::new("counter", "Counter")
    .integer()
    .min(0.0)
    .max(1000.0)
    .build();

// 创建浮点指标
let temp_metric = MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-40.0)
    .max(85.0)
    .build();

// 创建布尔指标
let active_metric = MetricBuilder::new("active", "Active Status")
    .boolean()
    .build();

// 创建枚举指标
let status_metric = MetricBuilder::new("status", "Status")
    .enum_type(vec!["running".to_string(), "stopped".to_string()])
    .build();
```

#### CommandBuilder

```rust
use neomind_extension_sdk::{CommandBuilder, ParamBuilder, MetricDataType, ParamMetricValue};

// 创建简单命令
let increment_cmd = CommandBuilder::new("increment")
    .display_name("Increment Counter")
    .llm_hints("增加计数器的值")
    .param_simple("amount", "Amount", MetricDataType::Integer)
    .sample(json!({ "amount": 1 }))
    .build();

// 创建带可选参数的命令
let set_cmd = CommandBuilder::new("set_value")
    .display_name("Set Value")
    .param_optional("unit", "Unit", MetricDataType::String)
    .param_with_default("precision", "Precision", MetricDataType::Integer, ParamMetricValue::Integer(2))
    .build();
```

#### ParamBuilder

```rust
use neomind_extension_sdk::{ParamBuilder, MetricDataType, ParamMetricValue};

// 创建必需参数
let required_param = ParamBuilder::new("temperature", MetricDataType::Float)
    .display_name("Temperature")
    .description("温度值（摄氏度）")
    .required()
    .min(-40.0)
    .max(85.0)
    .build();

// 创建带默认值的可选参数
let optional_param = ParamBuilder::new("scale", MetricDataType::String)
    .display_name("Scale")
    .description("温度单位")
    .default(ParamMetricValue::String("celsius".to_string()))
    .options(vec!["celsius".to_string(), "fahrenheit".to_string()])
    .build();
```

### 4.3 SDK 宏

#### neomind_export!

一行导出所有 FFI 函数：

```rust
// 基本用法
neomind_extension_sdk::neomind_export!(MyExtension);

// 自定义构造函数
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, with_config);
```

#### static_metadata!

创建静态元数据：

```rust
fn metadata(&self) -> &ExtensionMetadata {
    static_metadata!("my-extension", "My Extension", "1.0.0")
}
```

#### 度量宏

```rust
// 创建度量值
metric_int!("counter", 42);
metric_float!("temperature", 23.5);
metric_bool!("active", true);
metric_string!("status", "running");
```

#### 日志宏

```rust
ext_info!("Extension started");
ext_debug!("Processing item {}", id);
ext_warn!("Rate limit approaching");
ext_error!("Failed to connect: {}", err);
```

### 4.4 WASM 扩展开发

#### 配置 Cargo.toml

```toml
[package]
name = "my-wasm-extension"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = 3
lto = true
```

#### 编译 WASM

```bash
# 添加 WASM 目标
rustup target add wasm32-unknown-unknown

# 编译
cargo build --target wasm32-unknown-unknown --release
```

输出: `target/wasm32-unknown-unknown/release/my_wasm_extension.wasm`

#### WASM vs Native 对比

| 特性 | Native | WASM |
|------|--------|------|
| API 风格 | 异步 | 同步 |
| 内存 | 直接访问 | 沙箱隔离 |
| 性能 | 原生速度 | 接近原生 |
| 安全性 | 进程隔离 | 进程 + WASM 沙箱 |
| 文件访问 | 完整 | 通过能力 |
| 网络 | 完整 | 通过能力 |

---

## 5. 能力系统

### 5.1 概述

能力系统提供了解耦的、版本化的 API，允许扩展安全访问平台功能。

```
┌─────────────────────────────────────────────────────────────┐
│                     Extension                               │
└───────────────────┬─────────────────────────────────────────┘
                    │
                    │ ExtensionContext (稳定 API)
                    │
┌───────────────────▼─────────────────────────────────────────┐
│              ExtensionContext                               │
│  - register_provider()                                      │
│  - invoke_capability()                                      │
│  - check_capabilities()                                     │
│  - list_capabilities()                                      │
└───────────────────┬─────────────────────────────────────────┘
                    │
                    │ ExtensionCapabilityProvider (trait)
                    │
┌───────────────────▼─────────────────────────────────────────┐
│         Capability Providers                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   Device     │  │   Event      │  │   Storage    │     │
│  │   Provider   │  │   Provider   │  │   Provider   │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 标准能力

#### 设备能力

| 能力 | 名称 | 描述 | 访问类型 |
|------|------|------|----------|
| `DeviceMetricsRead` | `device_metrics_read` | 读取设备指标 | 只读 |
| `DeviceMetricsWrite` | `device_metrics_write` | 写入设备指标（包括虚拟指标） | 写入 |
| `DeviceControl` | `device_control` | 发送设备命令 | 控制 |
| `TelemetryHistory` | `telemetry_history` | 查询遥测历史 | 只读 |
| `MetricsAggregate` | `metrics_aggregate` | 聚合指标计算 | 只读 |

#### 事件能力

| 能力 | 名称 | 描述 | 访问类型 |
|------|------|------|----------|
| `EventPublish` | `event_publish` | 发布事件到事件总线 | 发布 |
| `EventSubscribe` | `event_subscribe` | 订阅系统事件 | 订阅 |

#### Agent 和规则能力

| 能力 | 名称 | 描述 | 访问类型 |
|------|------|------|----------|
| `ExtensionCall` | `extension_call` | 调用其他扩展 | 跨扩展 |
| `AgentTrigger` | `agent_trigger` | 触发 Agent 执行 | 触发 |
| `RuleTrigger` | `rule_trigger` | 触发规则评估 | 触发 |

### 5.3 使用能力

#### 在扩展中使用能力

```rust
use neomind_core::extension::context::*;

async fn inject_weather_data(&self, device_id: &str) -> Result<()> {
    if let Some(ctx) = &self.context {
        // 写入虚拟指标
        ctx.invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": device_id,
                "metric": "temperature",
                "value": 22.5,
            })
        ).await?;
    }
    Ok(())
}
```

#### 创建能力提供者

```rust
use neomind_core::extension::context::*;
use async_trait::async_trait;

pub struct MyCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MyCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceMetricsWrite,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "my-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                let device_id = params.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CapabilityError::InvalidArguments(
                        "device_id required".to_string()
                    ))?;
                
                // 实现能力逻辑
                Ok(json!({ "temperature": 22.5 }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}
```

### 5.4 能力清单

```rust
pub struct CapabilityManifest {
    /// 提供的能力列表
    pub capabilities: Vec<ExtensionCapability>,
    /// API 版本
    pub api_version: String,
    /// 最低核心版本
    pub min_core_version: String,
    /// 包名称
    pub package_name: String,
}
```

---

## 6. 进程隔离

### 6.1 概述

NeoMind 支持进程级隔离，确保扩展崩溃不会影响主服务器进程。

```
NeoMind 主进程                         Extension Runner 进程
┌─────────────────────┐                ┌─────────────────────┐
│ IsolatedExtension   │                │ extension-runner    │
│ ┌─────────────────┐ │    stdin       │ ┌─────────────────┐ │
│ │ stdin (pipe)    │ ├───────────────►│ │ IPC Receiver    │ │
│ └─────────────────┘ │                │ └─────────────────┘ │
│ ┌─────────────────┐ │    stdout      │ ┌─────────────────┐ │
│ │ stdout (pipe)   │ │◄───────────────┤ │ IPC Sender      │ │
│ └─────────────────┘ │                │ └─────────────────┘ │
│ ┌─────────────────┐ │    stderr      │ ┌─────────────────┐ │
│ │ stderr (pipe)   │ │◄───────────────┤ │ Logs/Errors     │ │
│ └─────────────────┘ │                │ └─────────────────┘ │
└─────────────────────┘                │ ┌─────────────────┐ │
                                       │ │ Extension       │ │
                                       │ └─────────────────┘ │
                                       └─────────────────────┘
```

### 6.2 安全保证

1. **进程隔离** - 扩展在独立进程中运行
2. **无共享内存** - 扩展无法破坏主进程内存
3. **受控通信** - 所有通信通过 IPC 协议
4. **自动恢复** - 扩展崩溃后可自动重启
5. **资源限制** - 可对扩展进程应用内存限制

### 6.3 配置

```toml
[extensions]
# 默认在隔离模式运行所有扩展
isolated_by_default = true

# 强制特定扩展在隔离模式运行
force_isolated = ["weather-extension", "untrusted-extension"]

# 强制特定扩展在进程内运行
force_in_process = ["core-extension"]

[extensions.isolated]
startup_timeout_secs = 30
command_timeout_secs = 30
max_memory_mb = 512
restart_on_crash = true
max_restart_attempts = 3
restart_cooldown_secs = 60
```

### 6.4 IPC 协议

IPC 协议使用带 4 字节长度前缀（小端序）的 JSON 消息。

#### 消息类型

**Host → Extension:**

| 消息 | 描述 |
|------|------|
| `Init { config }` | 使用配置初始化扩展 |
| `ExecuteCommand { command, args, request_id }` | 执行命令 |
| `ProduceMetrics { request_id }` | 请求当前指标 |
| `HealthCheck { request_id }` | 检查扩展健康状态 |
| `GetMetadata { request_id }` | 请求扩展元数据 |
| `Shutdown` | 优雅关闭 |
| `Ping { timestamp }` | 保活 ping |

**Extension → Host:**

| 消息 | 描述 |
|------|------|
| `Ready { metadata }` | 扩展就绪 |
| `Success { request_id, data }` | 命令执行成功 |
| `Error { request_id, error, kind }` | 发生错误 |
| `Metrics { request_id, metrics }` | 指标响应 |
| `Health { request_id, healthy }` | 健康检查响应 |
| `Metadata { request_id, metadata }` | 元数据响应 |
| `Pong { timestamp }` | Ping 响应 |

### 6.5 何时使用隔离模式

**使用隔离模式当：**
- 扩展来自不受信任的来源
- 扩展使用可能崩溃的 C/C++ 库
- 扩展有复杂的依赖
- 扩展需要资源限制
- 您希望崩溃后自动重启

**使用进程内模式当：**
- 扩展是可信且经过充分测试的
- 需要最大性能
- 扩展有复杂的异步操作
- 扩展需要共享内存访问

---

## 7. 流式扩展

### 7.1 概述

流式扩展支持实时数据处理，适用于：
- 图像分析（JPEG/PNG 帧）
- 视频流（H264/H265）
- 音频处理（PCM/MP3/AAC）
- 传感器数据
- 日志流

### 7.2 流方向

```rust
pub enum StreamDirection {
    /// 仅上传（客户端 → 扩展）
    Upload,
    /// 仅下载（扩展 → 客户端）
    Download,
    /// 双向
    Bidirectional,
}
```

### 7.3 流模式

```rust
pub enum StreamMode {
    /// 无状态 - 每个请求独立处理
    Stateless,
    /// 有状态 - 维护会话上下文
    Stateful,
    /// 推送 - 扩展主动推送数据
    Push,
}
```

### 7.4 数据类型

```rust
pub enum StreamDataType {
    Binary,
    Text,
    Json,
    Image { format: String },
    Audio { format: String, sample_rate: u32, channels: u16 },
    Video { codec: String, width: u32, height: u32, fps: u32 },
    Sensor { sensor_type: String },
    Custom { mime_type: String },
}
```

### 7.5 实现流式扩展

```rust
pub struct VideoAnalyzerExtension {
    sessions: RwLock<HashMap<String, SessionState>>,
}

#[async_trait]
impl Extension for VideoAnalyzerExtension {
    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateful,
            supported_data_types: vec![
                StreamDataType::Video {
                    codec: "h264".to_string(),
                    width: 1920,
                    height: 1080,
                    fps: 30,
                },
            ],
            max_chunk_size: 4 * 1024 * 1024, // 4MB
            preferred_chunk_size: 64 * 1024,  // 64KB
            max_concurrent_sessions: 10,
            flow_control: FlowControl::default(),
            config_schema: None,
        })
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), SessionState::new());
        Ok(())
    }

    async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: DataChunk,
    ) -> Result<StreamResult> {
        let start = std::time::Instant::now();
        
        // 处理视频帧
        let result = self.analyze_frame(&chunk.data)?;
        
        Ok(StreamResult::success(
            Some(chunk.sequence),
            chunk.sequence,
            serde_json::to_vec(&result)?,
            StreamDataType::Json,
            start.elapsed().as_millis() as f32,
        ))
    }

    async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(SessionStats::default())
    }
}
```

### 7.6 Push 模式

Push 模式允许扩展主动推送数据到客户端：

```rust
pub struct SensorStreamExtension {
    output_sender: RwLock<Option<Arc<mpsc::Sender<PushOutputMessage>>>>,
}

#[async_trait]
impl Extension for SensorStreamExtension {
    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability::push()
            .with_data_type(StreamDataType::Sensor {
                sensor_type: "temperature".to_string()
            })
        )
    }

    fn set_output_sender(&self, sender: Arc<mpsc::Sender<PushOutputMessage>>) {
        *self.output_sender.write() = Some(sender);
    }

    async fn start_push(&self, session_id: &str) -> Result<()> {
        let sender = self.output_sender.read().clone();
        if let Some(sender) = sender {
            // 启动数据推送循环
            tokio::spawn(async move {
                let mut sequence = 0u64;
                loop {
                    let data = read_sensor();
                    let msg = PushOutputMessage::json(
                        "session-id",
                        sequence,
                        json!({ "temperature": data }),
                    ).unwrap();
                    
                    if sender.send(msg).await.is_err() {
                        break;
                    }
                    sequence += 1;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }
        Ok(())
    }
}
```

---

## 8. 扩展包格式

### 8.1 .nep 包结构

.nep（NeoMind Extension Package）是 ZIP 格式的扩展包：

```
{extension-id}-{version}.nep
├── manifest.json           # 扩展清单
├── binaries/               # 平台特定二进制文件
│   ├── darwin_aarch64/
│   │   └── extension.dylib
│   ├── darwin_x86_64/
│   │   └── extension.dylib
│   ├── linux_amd64/
│   │   └── extension.so
│   ├── windows_amd64/
│   │   └── extension.dll
│   └── wasm/
│       ├── extension.wasm
│       └── extension.json
├── frontend/               # 前端组件
│   ├── dist/
│   │   ├── bundle.js
│   │   └── bundle.css
│   └── assets/
│       └── icons/
├── models/                 # AI/ML 模型文件
├── assets/                 # 静态资源
└── config/                 # 配置文件
```

### 8.2 manifest.json

```json
{
  "format": "neomind-extension-package",
  "abi_version": 3,
  "id": "weather-forecast",
  "name": "Weather Forecast",
  "version": "1.0.0",
  "description": "Weather forecast extension",
  "author": "CamThink Team",
  "license": "MIT",
  "type": "native",
  "binaries": {
    "darwin-aarch64": "binaries/darwin_aarch64/extension.dylib",
    "darwin-x64": "binaries/darwin_x86_64/extension.dylib",
    "linux-x64": "binaries/linux_amd64/extension.so",
    "windows-x64": "binaries/windows_amd64/extension.dll",
    "wasm": "binaries/wasm/extension.wasm"
  },
  "frontend": {
    "components": [
      {
        "type": "weather-card",
        "name": "Weather Card",
        "description": "Display weather information",
        "category": "visualization",
        "bundle_path": "dist/bundle.js",
        "export_name": "WeatherCard",
        "size_constraints": {
          "min_w": 2,
          "min_h": 2,
          "default_w": 4,
          "default_h": 4
        }
      }
    ]
  },
  "capabilities": {
    "metrics": [
      {
        "name": "temperature",
        "display_name": "Temperature",
        "data_type": "float",
        "unit": "°C"
      }
    ],
    "commands": [
      {
        "name": "get_forecast",
        "display_name": "Get Forecast",
        "description": "Get weather forecast for a location"
      }
    ]
  },
  "permissions": [
    "network"
  ]
}
```

### 8.3 平台检测

```rust
pub fn detect_platform() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x64",
        _ => "unknown",
    }.to_string()
}
```

### 8.4 安装扩展包

```bash
# 通过 API 安装
curl -X POST http://localhost:9375/api/extensions/install \
  -F "package=@weather-forecast-1.0.0.nep"

# 手动安装
mkdir -p ~/.neomind/extensions/weather-forecast
unzip weather-forecast-1.0.0.nep -d ~/.neomind/extensions/weather-forecast/
```

---

## 9. API 参考

### 9.1 REST API 端点

| 端点 | 方法 | 描述 |
|------|------|------|
| `/api/extensions` | GET | 列出所有扩展 |
| `/api/extensions/:id` | GET | 获取扩展详情 |
| `/api/extensions/:id/command` | POST | 执行扩展命令 |
| `/api/extensions/:id/health` | GET | 健康检查 |
| `/api/extensions/:id/stats` | GET | 获取统计信息 |
| `/api/extensions/discover` | POST | 发现新扩展 |
| `/api/extensions/install` | POST | 安装扩展包 |
| `/api/extensions/:id` | DELETE | 卸载扩展 |

### 9.2 执行命令

**请求：**
```bash
curl -X POST http://localhost:9375/api/extensions/my-extension/command \
  -H "Content-Type: application/json" \
  -d '{
    "command": "increment",
    "args": { "amount": 5 }
  }'
```

**响应：**
```json
{
  "success": true,
  "data": {
    "counter": 5
  }
}
```

### 9.3 WebSocket 流式 API

**连接：**
```javascript
const ws = new WebSocket('ws://localhost:9375/api/extensions/video-analyzer/stream');

// 发送数据
ws.send(JSON.stringify({
  type: 'init_session',
  config: { resolution: '1080p' }
}));

// 接收数据
ws.onmessage = (event) => {
  const result = JSON.parse(event.data);
  console.log('Received:', result);
};
```

---

## 10. 最佳实践

### 10.1 使用静态存储

```rust
// ✅ 正确：使用 OnceLock
fn metadata(&self) -> &ExtensionMetadata {
    static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
    META.get_or_init(|| { /* ... */ })
}

// ❌ 错误：每次创建新实例
fn metadata(&self) -> &ExtensionMetadata {
    &ExtensionMetadata { /* ... */ }  // 编译错误！
}
```

### 10.2 命名规范

```
扩展 ID: {category}-{name}-v{major}

示例:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2

库文件: libneomind_extension_{name}_v{major}.{ext}
```

### 10.3 线程安全

```rust
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,  // 使用原子类型
}

// 或使用 RwLock
pub struct MyExtension {
    data: RwLock<HashMap<String, Value>>,
}
```

### 10.4 错误处理

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    
    let response = reqwest::get(url).await
        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;
    
    Ok(response.json().await?)
}
```

### 10.5 日志记录

```rust
use tracing::{info, debug, warn, error};

pub async fn process(&self, data: &Data) -> Result<()> {
    debug!(data_len = data.len(), "Processing data");
    
    match self.internal_process(data).await {
        Ok(result) => {
            info!(result = ?result, "Processing completed");
            Ok(result)
        }
        Err(e) => {
            error!(error = %e, "Processing failed");
            Err(e)
        }
    }
}
```

### 10.6 资源清理

```rust
impl Drop for MyExtension {
    fn drop(&mut self) {
        // 清理资源
        if let Some(handle) = self.background_task.take() {
            handle.abort();
        }
    }
}
```

---

## 11. 故障排除

### 11.1 Extension Runner 未找到

**错误：**
```
Error: Could not find neomind-extension-runner binary
```

**解决方案：**
确保 runner 二进制文件与 neomind-api 在同一目录或在 PATH 中。

```bash
# 构建 extension runner
cargo build --release -p neomind-extension-runner

# 复制到正确位置
cp target/release/neomind-extension-runner /path/to/neomind/
```

### 11.2 扩展进程崩溃

**日志：**
```
Extension process crashed: signal: 11 (SIGSEGV)
```

**排查步骤：**
1. 检查扩展日志（stderr）
2. 检查扩展是否有内存安全问题
3. 确保扩展使用 `panic = "unwind"` 编译
4. 启用 `restart_on_crash` 自动恢复

### 11.3 超时错误

**错误：**
```
Error: Extension operation timed out after 30000ms
```

**解决方案：**
增加配置中的超时时间：

```toml
[extensions.isolated]
command_timeout_secs = 60
startup_timeout_secs = 60
```

### 11.4 ABI 版本不兼容

**错误：**
```
Error: Incompatible version: expected 3, got 2
```

**解决方案：**
使用正确的 SDK 版本重新编译扩展。

### 11.5 符号未找到

**错误：**
```
Error: Symbol not found: neomind_extension_abi_version
```

**解决方案：**
确保使用 `neomind_export!` 宏导出 FFI 函数：

```rust
neomind_extension_sdk::neomind_export!(MyExtension);
```

---

## 12. 附录

### 12.1 完整扩展示例

参考 [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) 仓库：

| 扩展 | 类型 | 描述 |
|------|------|------|
| `weather-forecast-v2` | Native | 天气预报 API 集成 |
| `image-analyzer-v2` | Native | YOLOv8 图像分析 |
| `yolo-video-v2` | Native | 实时视频流处理 |

### 12.2 相关文档

- [核心模块文档](01-core.md)
- [API 文档](14-api.md)
- [Web 前端文档](15-web.md)

### 12.3 版本历史

| 版本 | 日期 | 变更 |
|------|------|------|
| 2.1.0 | 2026-03-12 | 添加完整能力系统文档 |
| 2.0.0 | 2026-03-09 | V2 统一 SDK，ABI 版本 3 |
| 1.0.0 | 2025-12-01 | 初始版本 |

### 12.4 贡献指南

1. Fork 仓库
2. 创建功能分支
3. 提交 Pull Request
4. 确保所有测试通过

### 12.5 许可证

NeoMind 扩展 SDK 采用 MIT 许可证。

---

**文档维护者**: CamThink Team  
**最后更新**: 2026-03-12