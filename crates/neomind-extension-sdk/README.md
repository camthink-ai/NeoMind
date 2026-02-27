# NeoMind Extension SDK V2

**版本**: 2.0.0 | **ABI 版本**: 3

统一的 NeoMind 扩展开发工具包，支持 Native 和 WASM 目标。

## 特性

- **统一 SDK** - Native 和 WASM 单一代码库
- **简化 FFI** - 一行宏导出所有 FFI 函数
- **ABI 版本 3** - 新的扩展接口，改进的安全性
- **类型安全** - 完整的类型定义和辅助宏
- **异步支持** - 基于 Tokio 的异步运行时

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

## 许可证

Apache-2.0
