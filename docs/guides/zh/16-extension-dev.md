# 扩展开发指南

**版本**: 0.5.8
**难度**: 中等
**预计时间**: 1-2 小时

## 概述

本指南将带你创建一个 NeoMind 扩展，从基础设置到完整实现。

## 什么是扩展？

NeoMind 扩展是可以动态加载到 NeoMind 中的模块，提供：

- **数据源** - 从外部系统获取数据（如天气API）
- **设备适配器** - 支持新的设备协议（如 Modbus）
- **AI 工具** - 为 Agent 提供新能力
- **告警通道** - 发送通知到外部服务
- **LLM 后端** - 添加自定义 LLM 提供者

## 快速开始

### 1. 创建项目

```bash
# 使用 cargo 创建新的库项目
cargo new --lib my_neomind_extension

cd my_neomind_extension

# 添加 NeoMind SDK 依赖
cargo add neomind-extension-sdk --path /path/to/neomind/crates/neomind-extension-sdk
```

### 2. 配置 Cargo.toml

```toml
[package]
name = "my-neomind-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-extension-sdk = "0.5.8"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

**重要**: `crate-type = ["cdylib"] 是必需的，用于生成动态库。

### 3. 编写扩展

```rust
use neomind_extension_sdk::prelude::*;

struct MyExtension;

declare_extension!(
    MyExtension,
    metadata: ExtensionMetadata {
        name: "my.extension".to_string(),
        version: "0.1.0".to_string(),
        author: "Your Name".to_string(),
        description: "My first extension".to_string(),
    },
);

impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static METADATA: ExtensionMetadata = ExtensionMetadata {
            id: "my.extension".to_string(),
            name: "My Extension".to_string(),
            version: "0.1.0".to_string(),
            description: Some("My first NeoMind extension".to_string()),
            author: Some("Your Name".to_string()),
            homepage: Some("https://example.com".to_string()),
            license: Some("MIT".to_string()),
            file_path: None,
        };
        &METADATA
    }
}
```

### 4. 编译

```bash
# macOS/Linux
cargo build --release

# 输出位置:
# macOS: target/release/libmy_neomind_extension.dylib
# Linux: target/release/libmy_neomind_extension.so
# Windows: target/release/my_neomind_extension.dll
```

### 5. 安装

```bash
# 复制到扩展目录
mkdir -p ~/.neomind/extensions
cp target/release/libmy_neomind_extension.* ~/.neomind/extensions/

# 或使用 API 注册
curl -X POST http://localhost:9375/api/extensions \
  -H "Content-Type: application/json" \
  -d '{
    "file_path": "/path/to/libmy_neomind_extension.dylib"
  }'
```

## 完整示例：天气数据源

### 项目结构

```
weather-extension/
├── Cargo.toml
└── src/
    └── lib.rs
```

### 完整代码

```rust
use neomind_extension_sdk::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 天气扩展状态
struct WeatherExtension {
    api_key: Arc<Mutex<Option<String>>>,
    city: Arc<Mutex<String>>,
    last_update: Arc<Mutex<Option<i64>>>,
}

/// 声明扩展
declare_extension!(
    WeatherExtension,
    metadata: ExtensionMetadata {
        name: "weather.extension".to_string(),
        version: "1.0.0".to_string(),
        author: "NeoMind Team".to_string(),
        description: "Weather data provider extension".to_string(),
    },
);

/// 指标定义
const WEATHER_METRICS: &[MetricDefinition] = &[
    MetricDefinition {
        name: "temperature".to_string(),
        display_name: "Temperature".to_string(),
        data_type: MetricDataType::Float,
        unit: "°C".to_string(),
        min: Some(-50.0),
        max: Some(50.0),
        required: true,
    },
    MetricDefinition {
        name: "humidity".to_string(),
        display_name: "Humidity".to_string(),
        data_type: MetricDataType::Integer,
        unit: "%".to_string(),
        min: Some(0.0),
        max: Some(100.0),
        required: true,
    },
    MetricDefinition {
        name: "condition".to_string(),
        display_name: "Condition".to_string(),
        data_type: MetricDataType::String,
        unit: "".to_string(),
        min: None,
        max: None,
        required: true,
    },
];

/// 命令定义
const WEATHER_COMMANDS: &[ExtensionCommand] = &[
    ExtensionCommand {
        name: "set_city".to_string(),
        display_name: "Set City".to_string(),
        payload_template: "{ \"city\": \"{{city}}\" }".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "city".to_string(),
                display_name: "City".to_string(),
                description: "City name".to_string(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            },
        ],
        fixed_values: serde_json::Map::new(),
        llm_hints: "Set the city for weather data".to_string(),
        parameter_groups: vec![],
    },
    ExtensionCommand {
        name: "refresh".to_string(),
        display_name: "Refresh Weather".to_string(),
        payload_template: "{}".to_string(),
        parameters: vec![],
        fixed_values: serde_json::Map::new(),
        llm_hints: "Force refresh weather data".to_string(),
        parameter_groups: vec![],
    },
];

impl Extension for WeatherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static METADATA: ExtensionMetadata = ExtensionMetadata {
            id: "weather.extension".to_string(),
            name: "Weather Extension".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Provides weather data for any city".to_string()),
            author: Some("NeoMind Team".to_string()),
            homepage: Some("https://github.com/neomind-platform".to_string()),
            license: Some("MIT".to_string()),
            file_path: None,
        };
        &METADATA
    }

    fn metrics(&self) -> &[MetricDefinition] {
        WEATHER_METRICS
    }

    fn commands(&self) -> &[ExtensionCommand] {
        WEATHER_COMMANDS
    }

    fn execute_command(&self, command: &str, args: &Value) -> Result<Value, ExtensionError> {
        match command {
            "set_city" => {
                if let Some(city) = args.get("city").and_then(|v| v.as_str()) {
                    *self.city.lock().unwrap() = city.to_string();
                    // 触发数据刷新
                    Ok(json!({
                        "status": "success",
                        "message": format!("City set to {}", city)
                    }))
                } else {
                    Err(ExtensionError::InvalidArguments(
                        "city parameter is required".to_string()
                    ))
                }
            }
            "refresh" => {
                // 触发数据刷新
                let city = self.city.lock().unwrap().clone();
                Ok(json!({
                    "status": "refreshing",
                    "city": city
                }))
            }
            _ => Err(ExtensionError::UnsupportedCommand {
                command: command.to_string(),
            })
        }
    }

    fn health_check(&self) -> Result<bool, ExtensionError> {
        // 检查 API key 是否配置
        let api_key = self.api_key.lock().unwrap();
        Ok(api_key.is_some())
    }
}
```

## 扩展类型详解

### 数据源扩展 (DataSource)

提供外部数据，供 Agent 和规则使用：

```rust
impl Extension for MyDataSource {
    fn metrics(&self) -> &[MetricDefinition] {
        &[
            MetricDefinition {
                name: "price".to_string(),
                display_name: "Stock Price".to_string(),
                data_type: MetricDataType::Float,
                unit: "USD".to_string(),
                min: Some(0.0),
                max: None,
                required: true,
            },
        ]
    }

    // 在后台更新数据
    // 通过 ExtensionMetricsStorage 上报
}
```

### 设备适配器扩展 (DeviceAdapter)

支持新的设备协议：

```rust
impl Extension for MyDeviceAdapter {
    fn commands(&self) -> &[ExtensionCommand] {
        &[
            ExtensionCommand {
                name: "connect".to_string(),
                display_name: "Connect Device".to_string(),
                payload_template: "{ \"address\": \"{{address}}\" }".to_string(),
                parameters: vec![/* ... */],
                fixed_values: serde_json::Map::new(),
                llm_hints: "Connect to a device".to_string(),
                parameter_groups: vec![],
            },
        ]
    }
}
```

### AI 工具扩展 (Tool)

为 Agent 提供新能力：

```rust
impl Extension for MyToolExtension {
    fn commands(&self) -> &[ExtensionCommand] {
        &[
            ExtensionCommand {
                name: "calculate".to_string(),
                display_name: "Calculate".to_string(),
                payload_template: "{ \"expression\": \"{{expr}}\" }".to_string(),
                parameters: vec![/* ... */],
                fixed_values: serde_json::Map::new(),
                llm_hints: "Performs mathematical calculations".to_string(),
                parameter_groups: vec![],
            },
        ]
    }
}
```

### 告警通道扩展 (AlertChannel)

发送通知到外部服务：

```rust
use neomind_extension_sdk::types::ChannelDescriptor;

impl Extension for MyAlertChannel {
    // 返回通道描述
    // 通过配置或其他方式提供
}
```

## 调试扩展

### 本地测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata() {
        let ext = MyExtension;
        let meta = ext.metadata();
        assert_eq!(meta.id, "weather.extension");
    }

    #[test]
    fn test_execute_command() {
        let ext = MyExtension::new();
        let result = ext.execute_command("set_city", &json!({"city": "Tokyo"}));
        assert!(result.is_ok());
    }
}
```

### 运行测试

```bash
cargo test

# 使用 NeoMind 测试框架
cargo test --package neomind-api --test extension_loader
```

## 部署

### 手动安装

```bash
# 1. 编译
cargo build --release

# 2. 复制到扩展目录
cp target/release/libmy_extension.dylib ~/.neomind/extensions/

# 3. 重启 NeoMind 或使用 API 发现
curl -X POST http://localhost:9375/api/extensions/discover
```

### API 注册

```bash
# 注册扩展
curl -X POST http://localhost:9375/api/extensions \
  -H "Content-Type: application/json" \
  -d '{
    "file_path": "/absolute/path/to/libmy_extension.dylib"
  }'

# 启动扩展
curl -X POST http://localhost:9375/api/extensions/weather.extension/start

# 检查健康状态
curl http://localhost:9375/api/extensions/weather.extension/health
```

## 最佳实践

### 1. 错误处理

始终返回有意义的错误信息：

```rust
fn execute_command(&self, command: &str, args: &Value) -> Result<Value, ExtensionError> {
    match command {
        "fetch" => {
            let url = args.get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtensionError::InvalidArguments(
                    "url parameter is required".to_string()
                ))?;

            // ... 执行逻辑
        }
        _ => Err(ExtensionError::UnsupportedCommand {
            command: command.to_string(),
        }),
    }
}
```

### 2. 资源管理

使用 Arc<Mutex<>> 管理共享状态：

```rust
struct MyExtension {
    state: Arc<Mutex<ExtensionState>>,
}

impl Extension for MyExtension {
    // 可以安全地跨线程访问
}
```

### 3. 配置管理

支持运行时配置：

```rust
impl Extension for MyExtension {
    fn execute_command(&self, command: &str, args: &Value) -> Result<Value, ExtensionError> {
        if command == "configure" {
            // 更新配置
            // 保存到内部状态
        }
    }
}
```

### 4. 数据上报

定期上报指标数据：

```rust
// 在扩展的异步任务中
loop {
    tokio::time::sleep(Duration::from_secs(60)).await;

    // 获取数据
    let data = fetch_weather_data().await?;

    // 上报到 NeoMind
    // 通过 ExtensionMetricsStorage API
    // 或使用 WebSocket 上报
}
```

## 扩展模板

### 简单模板项目

```bash
# 克隆模板（如果存在）
git clone https://github.com/neomind-platform/extension-template.git my-extension

# 或使用 cargo-generate
cargo install cargo-generate
cargo generate --git https://github.com/neomind-platform/extension-template \
  --name my-extension
```

### 最小 Cargo.toml

```toml
[package]
name = "my-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-extension-sdk = { version = "0.5" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
strip = true
lto = true
codegen-units = 1
opt-level = "z"
```

## 故障排除

### 编译错误

**"undefined symbol" 错误**:
- 确保使用 `#[no_mangle]` 导出符号
- 检查函数签名是否正确

**"wrong type" 错误**:
- 确保使用 FFI 安全类型（`repr(C)`）
- 避免使用 Rust 特定类型

### 加载错误

**"ABI version mismatch"**:
- 检查 `NEO_EXT_ABI_VERSION` 是否与服务器匹配
- 确保使用最新版本的 SDK

**"symbol not found"**:
- 确保导出正确的符号名称
- 使用 `nm` 或 `objdump` 检查导出符号

### 运行时错误

**"command not found"**:
- 检查 `execute_command` 实现是否处理该命令
- 确保命令名称拼写正确

**"health check failed"**:
- 实现或修复 `health_check` 方法
- 确保所有依赖已正确配置

## 参考资料

- [Extension SDK API 文档](13-extension-sdk.md)
- [核心模块文档](01-core.md)
- [迁移指南](../../architecture/plugin-migration.md)
- [主项目文档](../../CLAUDE.md)

## 官方仓库

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - 官方扩展市场、示例和模板
- **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** - 支持的设备类型定义
- **[示例扩展](https://github.com/camthink-ai/NeoMind-Extensions)** - 扩展示例项目

## 下一步

- 创建你的第一个扩展
- 查看 [示例扩展](https://github.com/neomind-platform/example-extensions)
- 加入 NeoMind 社区讨论
