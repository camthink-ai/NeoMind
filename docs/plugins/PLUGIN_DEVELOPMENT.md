# NeoTalk 插件开发指南

本指南介绍如何为 NeoTalk 开发插件，包括 Native 插件和配置热重载功能。

## 目录

1. [插件类型](#插件类型)
2. [快速开始](#快速开始)
3. [Native 插件开发](#native-插件开发)
4. [配置热重载](#配置热重载)
5. [插件管理 API](#插件管理-api)
6. [最佳实践](#最佳实践)
7. [故障排除](#故障排除)

---

## 插件类型

NeoTalk 支持三种插件方式：

| 类型 | 文件扩展名 | 优势 | 适用场景 |
|------|-----------|------|---------|
| **编译时插件** | N/A | 类型安全，零开销 | 核心功能扩展 |
| **Native 插件** | `.so`, `.dylib`, `.dll` | 动态加载，性能最佳 | 复杂业务逻辑 |
| **WASM 插件** | `.wasm` | 沙箱安全，跨平台 | 第三方插件 |

### 插件功能分类

- `LlmBackend` - LLM 后端 (OpenAI, Anthropic, Ollama)
- `StorageBackend` - 存储后端 (redb, sled, memory)
- `DeviceAdapter` - 设备适配器 (MQTT, Modbus, HASS)
- `Tool` - 函数调用工具
- `Integration` - 外部集成 (n8n, WhatsApp)
- `AlertChannel` - 告警通道 (Email, Webhook, SMS)
- `RuleEngine` - 规则引擎
- `WorkflowEngine` - 工作流引擎

---

## 快速开始

### 1. 创建新插件项目

```bash
cd /path/to/NeoTalk
mkdir -p plugins/my-plugin
cd plugins/my-plugin
cargo init --lib
```

### 2. 配置 Cargo.toml

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
edge-ai-core = { path = "../../crates/core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### 3. 实现插件

```rust
use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
use edge_ai_core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

pub struct MyPlugin {
    initialized: bool,
}

impl MyPlugin {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

// FFI 导出
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
    Box::into_raw(Box::new(MyPlugin::new()))
}

#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut MyPlugin) {
    if !plugin.is_null() {
        unsafe { let _ = Box::from_raw(plugin); }
    }
}

// 元数据
const PLUGIN_ID: &str = "my-plugin\0";
// ... 其他元数据

#[no_mangle]
pub static neotalk_plugin_descriptor: NativePluginDescriptor = NativePluginDescriptor {
    api_version: NEOTALK_PLUGIN_API_VERSION,
    id: PLUGIN_ID.as_ptr(),
    id_len: PLUGIN_ID.len() - 1,
    // ...
};
```

### 4. 构建和测试

```bash
# 构建
cargo build --release

# 测试
cargo test

# 输出: target/release/libmy_plugin.so (Linux)
#       target/release/libmy_plugin.dylib (macOS)
#       target/release/my_plugin.dll (Windows)
```

---

## Native 插件开发

### 核心概念

#### FFI 边界

Native 插件通过 C FFI 与 NeoTalk 主程序通信。必须导出三个符号：

1. **`neotalk_plugin_create`** - 创建插件实例
2. **`neotalk_plugin_destroy`** - 销毁插件实例
3. **`neotalk_plugin_descriptor`** - 插件元数据

#### 生命周期

```
加载 → 验证 API 版本 → 创建实例 → 初始化 → 启动 → 运行 → 停止 → 销毁
```

### 完整示例

```rust
//! My Awesome Plugin for NeoTalk

use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
use edge_ai_core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

/// 插件结构体
pub struct MyPlugin {
    config: Value,
    initialized: bool,
    running: bool,
}

impl MyPlugin {
    /// 创建新实例
    pub fn new() -> Self {
        Self {
            config: json!({}),
            initialized: false,
            running: false,
        }
    }

    /// 初始化插件
    pub fn initialize(&mut self, config: &Value) -> Result<(), PluginError> {
        self.config = config.clone();
        self.initialized = true;
        Ok(())
    }

    /// 启动插件
    pub fn start(&mut self) -> Result<(), PluginError> {
        if !self.initialized {
            return Err(PluginError::InitializationFailed(
                "Plugin not initialized".to_string()
            ));
        }
        self.running = true;
        Ok(())
    }

    /// 停止插件
    pub fn stop(&mut self) -> Result<(), PluginError> {
        self.running = false;
        Ok(())
    }

    /// 处理命令
    pub fn handle_command(&self, cmd: &str, args: &Value) -> Result<Value, PluginError> {
        match cmd {
            "status" => Ok(json!({
                "initialized": self.initialized,
                "running": self.running,
            })),
            _ => Err(PluginError::ExecutionFailed(
                format!("Unknown command: {}", cmd)
            )),
        }
    }
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ============ FFI 导出 ============

#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
    Box::into_raw(Box::new(MyPlugin::new()))
}

#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut MyPlugin) {
    if !plugin.is_null() {
        unsafe { let _ = Box::from_raw(plugin); }
    }
}

// ============ 插件元数据 ============

const PLUGIN_ID: &str = "my-awesome-plugin\0";
const PLUGIN_NAME: &str = "My Awesome Plugin\0";
const PLUGIN_VERSION: &str = "0.1.0\0";
const PLUGIN_DESC: &str = "An awesome plugin for NeoTalk\0";
const REQUIRED_VERSION: &str = ">=0.1.0\0";

#[no_mangle]
pub static neotalk_plugin_descriptor: NativePluginDescriptor = NativePluginDescriptor {
    api_version: NEOTALK_PLUGIN_API_VERSION,
    id: PLUGIN_ID.as_ptr(),
    id_len: PLUGIN_ID.len() - 1,
    name: PLUGIN_NAME.as_ptr(),
    name_len: PLUGIN_NAME.len() - 1,
    version: PLUGIN_VERSION.as_ptr(),
    version_len: PLUGIN_VERSION.len() - 1,
    description: PLUGIN_DESC.as_ptr(),
    description_len: PLUGIN_DESC.len() - 1,
    required_version: REQUIRED_VERSION.as_ptr(),
    required_version_len: REQUIRED_VERSION.len() - 1,
    create: neotalk_plugin_create as *const (),
    destroy: neotalk_plugin_destroy as *const () -> *const (),
};
```

### 字符串处理注意事项

所有导出给 C 的字符串必须以 `\0` 结尾：

```rust
// ✓ 正确
const PLUGIN_ID: &str = "my-plugin\0";
id_len: PLUGIN_ID.len() - 1,  // = 10

// ✗ 错误 - 没有 null 终止符
const PLUGIN_ID: &str = "my-plugin";

// ✗ 错误 - 长度包含 null 终止符
id_len: PLUGIN_ID.len(),  // = 11
```

### API 版本兼容性

NeoTalk 使用语义化版本控制。插件声明其兼容的 NeoTalk 版本：

| 要求格式 | 说明 | 示例 |
|---------|------|------|
| `*` | 任意版本 | `"*\0"` |
| `>=1.0.0` | 大于等于指定版本 | `">=1.0.0\0"` |
| `^1.0.0` | 兼容版本更新 | `"^1.0.0\0"` |
| `~1.0.0` | 补丁版本更新 | `"~1.0.0\0"` |

---

## 配置热重载

### ConfigWatcher 基础用法

```rust
use edge_ai_core::plugin::watcher::ConfigWatcher;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut watcher = ConfigWatcher::new().await?;

    // 监控配置文件
    watcher.watch("config.json", |path, new_config| {
        println!("Config reloaded: {:?}", path);
        println!("New config: {:?}", new_config);
    }).await?;

    // 保持运行
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

    Ok(())
}
```

### HotConfig 类型安全热重载

```rust
use edge_ai_core::plugin::watcher::HotConfig;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct MyConfig {
    max_connections: usize,
    timeout_ms: u64,
}

async fn use_hot_config() -> Result<(), Box<dyn std::error::Error>> {
    // 创建热重载配置
    let config = HotConfig::new(
        "config.toml",
        |v| Ok(serde_json::from_value(v.clone())?)
    ).await?;

    // 获取当前值
    let current = config.get().await;
    println!("Max connections: {}", current.max_connections);

    // 手动重载
    config.reload().await?;

    Ok(())
}
```

### 支持的配置格式

| 格式 | 文件扩展名 |
|------|-----------|
| JSON | `.json` |
| TOML | `.toml` |

---

## 插件管理 API

### 列出所有插件

```bash
curl http://localhost:3000/api/plugins
```

响应：
```json
{
  "success": true,
  "plugins": [
    {
      "id": "my-plugin",
      "name": "My Plugin",
      "plugin_type": "tool",
      "state": "Running",
      "enabled": true,
      "version": "0.1.0",
      "stats": { "start_count": 1, ... }
    }
  ],
  "count": 1
}
```

### 注册插件

```bash
curl -X POST http://localhost:3000/api/plugins \
  -H "Content-Type: application/json" \
  -d '{
    "path": "/path/to/libmy_plugin.so",
    "plugin_type": "tool",
    "auto_start": true,
    "config": {"greeting": "Hi"}
  }'
```

### 启动/停止插件

```bash
# 启动
curl -X POST http://localhost:3000/api/plugins/my-plugin/start

# 停止
curl -X POST http://localhost:3000/api/plugins/my-plugin/stop
```

### 健康检查

```bash
curl http://localhost:3000/api/plugins/my-plugin/health
```

### 执行插件命令

```bash
curl -X POST http://localhost:3000/api/plugins/my-plugin/command \
  -H "Content-Type: application/json" \
  -d '{
    "command": "status",
    "args": {}
  }'
```

---

## 最佳实践

### 1. 错误处理

始终使用 `PluginError` 而不是 `panic!`：

```rust
// ✓ 好的做法
pub fn do_work(&self) -> Result<(), PluginError> {
    if self.invalid_state {
        return Err(PluginError::ExecutionFailed(
            "Invalid state".to_string()
        ));
    }
    Ok(())
}

// ✗ 坏的做法
pub fn do_work(&self) -> Result<(), PluginError> {
    if self.invalid_state {
        panic!("Invalid state!");
    }
    Ok(())
}
```

### 2. 资源清理

使用 RAII 确保资源正确释放：

```rust
pub struct MyPlugin {
    _handle: Option<SomeResource>,
}

impl Drop for MyPlugin {
    fn drop(&mut self) {
        if let Some(handle) = self._handle.take() {
            // 清理资源
        }
    }
}
```

### 3. 线程安全

如果插件需要在多线程环境中使用，确保内部状态是线程安全的：

```rust
use std::sync::{Arc, Mutex};

pub struct MyPlugin {
    inner: Arc<Mutex<PluginState>>,
}
```

### 4. 版本命名

使用语义化版本：

```
MAJOR.MINOR.PATCH

0.1.0 - 初始版本
0.1.1 - Bug 修复
0.2.0 - 新功能（向后兼容）
1.0.0 - 破坏性更改
```

### 5. 文档

在插件中添加文档注释：

```rust
/// My Awesome Plugin
///
/// This plugin does awesome things.
///
/// # Configuration
///
/// ```json
/// {
///   "option": "value"
/// }
/// ```
///
/// # Commands
///
/// - `status` - Get plugin status
/// - `reset` - Reset internal state
pub struct MyPlugin;
```

---

## 故障排除

### 插件加载失败

**错误**: `Failed to load library: ...`

**解决方案**:
1. 检查文件扩展名是否正确
2. 确认编译目标平台与运行平台一致
3. 检查依赖的库版本是否匹配

### API 版本不匹配

**错误**: `Plugin version mismatch: expected 1, found 2`

**解决方案**:
更新插件中的 `NEOTALK_PLUGIN_API_VERSION` 常量。

### 配置热重载不工作

**检查**:
1. 文件路径是否正确
2. 文件是否存在
3. 是否有文件监听权限

### 调试技巧

```rust
// 在插件中添加日志
use tracing::{info, warn, error};

#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
    info!("Creating plugin instance");
    Box::into_raw(Box::new(MyPlugin::new()))
}
```

---

## 相关文档

- [Native 插件 API 参考](./NATIVE_PLUGIN_API.md) - API 详细说明
- [插件模板](./TEMPLATE.md) - 完整模板代码
