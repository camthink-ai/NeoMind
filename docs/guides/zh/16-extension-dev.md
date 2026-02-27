# 扩展开发指南 V2

**版本**: 2.0.0
**SDK 版本**: 2.0.0
**ABI 版本**: 3
**难度**: 中等
**预计时间**: 30 分钟

## 概述

本指南将带你使用 **NeoMind Extension SDK V2** 创建扩展，体验简化的开发流程。

## 什么是扩展？

NeoMind 扩展是可以动态加载到 NeoMind 中的模块，提供：

- **数据源** - 从外部系统获取数据（如天气API）
- **设备适配器** - 支持新的设备协议（如 Modbus）
- **AI 工具** - 为 Agent 提供新能力
- **视频处理** - 实时视频分析和流处理
- **图像分析** - YOLO 目标检测等

## SDK V2 特性

| 特性 | 说明 |
|------|------|
| **统一 SDK** | 单一依赖，同时支持 Native 和 WASM |
| **简化 FFI** | 一行宏导出所有 FFI 函数 |
| **ABI 版本 3** | 新的扩展接口，改进的安全性 |
| **类型安全** | 完整的类型定义和辅助宏 |

## 快速开始

### 1. 创建项目

```bash
# 创建新的库项目
cargo new --lib my_extension
cd my_extension
```

### 2. 配置 Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
# 只需要 SDK 依赖！
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

**重要**:
- `crate-type = ["cdylib"]` 用于生成动态库
- `panic = "unwind"` 是安全性必需的

### 3. 编写扩展

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================================
// 类型定义
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyResult {
    pub value: i64,
    pub message: String,
}

// ============================================================================
// 扩展实现
// ============================================================================

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

impl Default for MyExtension {
    fn default() -> Self {
        Self::new()
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
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add".to_string(),
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
                    llm_hints: "Increment the counter".to_string(),
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
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// 导出 FFI - 只需要这一行！
// ============================================================================

neomind_extension_sdk::neomind_export!(MyExtension);

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata() {
        let ext = MyExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "my-extension");
    }

    #[test]
    fn test_metrics() {
        let ext = MyExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 1);
    }
}
```

### 4. 编译

```bash
cargo build --release
```

输出:
- **macOS**: `target/release/libneomind_extension_my_extension.dylib`
- **Linux**: `target/release/libneomind_extension_my_extension.so`
- **Windows**: `target/release/neomind_extension_my_extension.dll`

### 5. 安装

```bash
# 复制到扩展目录
mkdir -p ~/.neomind/extensions/my-extension
cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/my-extension/

# 通过 API 发现
curl -X POST http://localhost:9375/api/extensions/discover
```

## Extension Trait

所有扩展必须实现 `Extension` trait：

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    /// 获取扩展元数据（必需）
    fn metadata(&self) -> &ExtensionMetadata;

    /// 声明扩展提供的指标（可选）
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// 声明扩展支持的命令（可选）
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// 执行命令（必需）
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// 生成指标数据（可选）
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// 健康检查（可选）
    async fn health_check(&self) -> Result<bool> { Ok(true) }
}
```

## SDK 提供的宏

### `neomind_export!`

一行导出所有 FFI 函数：

```rust
// 基本用法
neomind_extension_sdk::neomind_export!(MyExtension);

// 自定义构造函数
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, with_config);
```

### `static_metadata!`

创建静态元数据：

```rust
fn metadata(&self) -> &ExtensionMetadata {
    static_metadata!("my-extension", "My Extension", "1.0.0")
}
```

### 度量宏

```rust
// 创建度量值
metric_int!("counter", 42);
metric_float!("temperature", 23.5);
metric_bool!("active", true);
metric_string!("status", "running");
```

### 日志宏

```rust
ext_info!("Extension started");
ext_debug!("Processing item {}", id);
ext_warn!("Rate limit approaching");
ext_error!("Failed to connect: {}", err);
```

## 完整扩展示例

参考 [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) 仓库中的 V2 扩展：

| 扩展 | 类型 | 说明 |
|------|------|------|
| `weather-forecast-v2` | Native | 天气预报 API 集成 |
| `image-analyzer-v2` | Native | YOLOv8 图像分析 |
| `yolo-video-v2` | Native | 实时视频流处理 |

## 最佳实践

### 1. 使用静态存储

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

### 2. 命名规范

```
扩展 ID: {category}-{name}-v{major}

示例:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2

库文件: libneomind_extension_{name}_v{major}.{ext}
```

### 3. 线程安全

```rust
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,  // 使用原子类型
}
```

### 4. 错误处理

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    // ...
}
```

## Dashboard 组件

扩展可以提供自定义 Dashboard 组件。详见 [extension-package.md](extension-package.md)。

### 目录结构

```
my-extension/
├── Cargo.toml
├── src/lib.rs
├── metadata.json
└── frontend/
    ├── src/index.tsx
    ├── package.json
    └── vite.config.ts
```

## API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/extensions` | GET | 列出所有扩展 |
| `/api/extensions/:id` | GET | 获取扩展详情 |
| `/api/extensions/:id/command` | POST | 执行扩展命令 |
| `/api/extensions/:id/health` | GET | 健康检查 |
| `/api/extensions/discover` | POST | 发现新扩展 |

## 官方仓库

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - 扩展示例
- **[NeoMind](https://github.com/camthink-ai/NeoMind)** - 主项目

## 参考

- [核心模块文档](01-core.md)
- [扩展打包指南](extension-package.md)
- [主项目文档](../../CLAUDE.md)
