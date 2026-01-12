# NeoTalk 插件开发端到端教程

本教程将一步步指导你完成第一个 NeoTalk 插件的开发、测试和部署。

## 教程概述

我们将创建一个 **天气查询工具插件**，功能包括：
- 根据城市名称查询天气
- 支持多个天气 API 提供商
- 缓存查询结果
- 错误处理和重试

**预计时间**: 30-45 分钟
**难度**: 初学者友好
**语言**: Rust (Native 插件)

---

## 准备工作

### 环境检查

```bash
# 检查 Rust 版本
rustc --version  # 应该 >= 1.70

# 检查 NeoTalk
edge-ai --version || echo "需要先安装 NeoTalk"

# 检查插件目录
ls -la ~/.neotalk/plugins/ || mkdir -p ~/.neotalk/plugins
```

### 安装依赖

```bash
# 如果还没安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 添加 WASM 目标 (本教程后续需要)
rustup target add wasm32-unknown-unknown
```

---

## 第一步：创建插件项目

### 1.1 创建项目目录

```bash
cd ~
mkdir -p neotalk-plugins
cd neotalk-plugins

# 创建插件项目
cargo new --lib weather-tool
cd weather-tool
```

### 1.2 配置 Cargo.toml

```bash
cat > Cargo.toml << 'EOF'
[package]
name = "weather-tool"
version = "0.1.0"
edition = "2021"
authors = ["Your Name <you@example.com>"]
description = "Weather query tool for NeoTalk"
license = "MIT"

[lib]
crate-type = ["cdylib"]

[dependencies]
edge-ai-core = { path = "/Users/shenmingming/NeoTalk/crates/core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["rt-multi-thread"] }
reqwest = { version = "0.12", features = ["json"] }
anyhow = "1.0"
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
EOF
```

### 1.3 项目结构

```
weather-tool/
├── Cargo.toml              # 配置文件
├── src/
│   └── lib.rs              # 主要代码
└── tests/
    └── integration_test.rs # 集成测试
```

---

## 第二步：实现插件核心功能

### 2.1 创建基础结构

```bash
cat > src/lib.rs << 'EOF'
//! Weather Tool Plugin for NeoTalk
//!
//! This plugin provides weather information for cities worldwide.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// ============================================================================
// 数据结构
// ============================================================================

/// 天气响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherResponse {
    pub city: String,
    pub temperature: f32,
    pub humidity: u32,
    pub description: String,
    pub timestamp: i64,
}

/// 插件配置
#[derive(Debug, Clone, Deserialize)]
pub struct WeatherConfig {
    pub api_key: Option<String>,
    pub cache_ttl_secs: Option<u64>,
    pub timeout_ms: Option<u64>,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            cache_ttl_secs: Some(300), // 5 分钟
            timeout_ms: Some(5000),
        }
    }
}

/// 插件状态
pub struct WeatherPlugin {
    config: WeatherConfig,
    cache: Arc<RwLock<HashMap<String, (WeatherResponse, Instant)>>>,
    initialized: bool,
}

// ============================================================================
// 插件实现
// ============================================================================

impl WeatherPlugin {
    /// 创建新插件实例
    pub fn new() -> Self {
        Self {
            config: WeatherConfig::default(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            initialized: false,
        }
    }

    /// 初始化插件
    pub fn initialize(&mut self, config: &Value) -> Result<()> {
        // 解析配置
        if let Ok(cfg) = serde_json::from_value::<WeatherConfig>(config.clone()) {
            self.config = cfg;
        }

        self.initialized = true;
        tracing::info!("Weather plugin initialized");
        Ok(())
    }

    /// 检查初始化状态
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// 查询天气
    pub async fn get_weather(&self, city: &str) -> Result<WeatherResponse> {
        // 检查缓存
        {
            let cache = self.cache.read().await;
            if let Some((response, timestamp)) = cache.get(city) {
                let ttl = Duration::from_secs(self.config.cache_ttl_secs.unwrap_or(300));
                if timestamp.elapsed() < ttl {
                    tracing::debug!("Cache hit for city: {}", city);
                    return Ok(response.clone());
                }
            }
        }

        // 调用 API 获取天气
        tracing::info!("Fetching weather for: {}", city);
        let response = self.fetch_weather(city).await?;

        // 更新缓存
        {
            let mut cache = self.cache.write().await;
            cache.insert(city.to_string(), (response.clone(), Instant::now()));
        }

        Ok(response)
    }

    /// 从 API 获取天气数据
    async fn fetch_weather(&self, city: &str) -> Result<WeatherResponse> {
        // 模拟 API 调用 (实际项目中调用真实天气 API)
        // 这里使用模拟数据返回

        let response = WeatherResponse {
            city: city.to_string(),
            temperature: 22.5 + (city.len() as f32),  // 模拟不同城市不同温度
            humidity: 65,
            description: "Partly cloudy".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        Ok(response)
    }

    /// 清除缓存
    pub async fn clear_cache(&self) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.clear();
        tracing::info!("Weather cache cleared");
        Ok(())
    }

    /// 获取缓存统计
    pub async fn cache_stats(&self) -> Value {
        let cache = self.cache.read().await;
        json!({
            "cached_cities": cache.len(),
            "ttl_seconds": self.config.cache_ttl_secs
        })
    }
}

impl Default for WeatherPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FFI 导出
// ============================================================================

use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};

#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut WeatherPlugin {
    Box::into_raw(Box::new(WeatherPlugin::new()))
}

#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut WeatherPlugin) {
    if !plugin.is_null() {
        unsafe {
            let _ = Box::from_raw(plugin);
        }
    }
}

// ============================================================================
// 插件描述符
// ============================================================================

const PLUGIN_ID: &str = "weather-tool\0";
const PLUGIN_NAME: &str = "Weather Tool\0";
const PLUGIN_VERSION: &str = "0.1.0\0";
const PLUGIN_DESC: &str = "Query weather information for cities\0";
const REQUIRED_VERSION: &str = ">=0.1.0\0";
const PLUGIN_AUTHOR: &str = "Your Name\0";

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

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = WeatherPlugin::new();
        assert!(!plugin.is_initialized());

        let config = json!({
            "cache_ttl_secs": 600,
            "timeout_ms": 3000
        });

        plugin.initialize(&config).unwrap();
        assert!(plugin.is_initialized());
    }

    #[tokio::test]
    async fn test_get_weather() {
        let plugin = WeatherPlugin::new();
        let weather = plugin.get_weather("Tokyo").await.unwrap();

        assert_eq!(weather.city, "Tokyo");
        assert!(weather.temperature > 0.0);
        assert!(!weather.description.is_empty());
    }

    #[tokio::test]
    async fn test_cache() {
        let plugin = WeatherPlugin::new();

        // 第一次查询
        let _ = plugin.get_weather("London").await;

        // 检查缓存统计
        let stats = plugin.cache_stats().await;
        assert_eq!(stats["cached_cities"], 1);

        // 清除缓存
        plugin.clear_cache().await.unwrap();

        let stats = plugin.cache_stats().await;
        assert_eq!(stats["cached_cities"], 0);
    }
}
EOF
```

### 2.2 添加 chrono 依赖

```bash
# 更新 Cargo.toml 添加 chrono
cat >> Cargo.toml << 'EOF'

chrono = { version = "0.4", features = ["serde"], optional = true }
EOF
```

---

## 第三步：编译插件

### 3.1 构建插件

```bash
cargo build --release
```

### 3.2 查找编译输出

编译后的插件位置：

| 平台 | 位置 |
|------|------|
| Linux | `target/release/libweather_tool.so` |
| macOS | `target/release/libweather_tool.dylib` |
| Windows | `target/release/weather_tool.dll` |

```bash
# macOS/Linux
ls -lh target/release/libweather_tool.*

# 或者使用 find
find target/release -name "*weather_tool*" -type f
```

---

## 第四步：验证插件

### 4.1 使用 CLI 工具验证

```bash
# 验证插件
edge-ai plugin validate target/release/libweather_tool.dylib
```

预期输出：
```
✓ Plugin validation passed

Checks:
  ✓ Native library format valid
  ✓ Plugin descriptor found
  ✓ API version compatible
  ✓ Required exports present

Details:
  Plugin: weather-tool v0.1.0
  Name: Weather Tool
  Description: Query weather information for cities
  Author: Your Name
```

### 4.2 查看插件信息

```bash
edge-ai plugin info target/release/libweather_tool.dylib
```

---

## 第五步：部署插件

### 5.1 复制到插件目录

```bash
# 创建插件目录
mkdir -p ~/.neotalk/plugins

# 复制插件
cp target/release/libweather_tool.dylib ~/.neotalk/plugins/

# 或者使用符号链接（开发时推荐）
ln -s "$(pwd)/target/release/libweather_tool.dylib" ~/.neotalk/plugins/
```

### 5.2 启动 NeoTalk

```bash
# 在新终端窗口
cd /Users/shenmingming/NeoTalk
cargo run -p edge-ai-cli -- serve
```

---

## 第六步：测试插件

### 6.1 列出插件

```bash
curl http://localhost:3000/api/plugins
```

预期响应：
```json
{
  "success": true,
  "plugins": [
    {
      "id": "weather-tool",
      "name": "Weather Tool",
      "plugin_type": "Tool",
      "state": "Loaded",
      "enabled": false,
      "version": "0.1.0"
    }
  ],
  "count": 1
}
```

### 6.2 启用插件

```bash
curl -X POST http://localhost:3000/api/plugins/weather-tool/enable
```

### 6.3 启动插件

```bash
curl -X POST http://localhost:3000/api/plugins/weather-tool/start \
  -H "Content-Type: application/json" \
  -d '{}'
```

### 6.4 执行插件命令

```bash
# 查询天气
curl -X POST http://localhost:3000/api/plugins/weather-tool/command \
  -H "Content-Type: application/json" \
  -d '{
    "command": "get_weather",
    "args": {"city": "Tokyo"}
  }'
```

预期响应：
```json
{
  "success": true,
  "result": {
    "city": "Tokyo",
    "temperature": 28.5,
    "humidity": 65,
    "description": "Partly cloudy",
    "timestamp": 1234567890
  }
}
```

---

## 第七步：编写集成测试

### 7.1 创建测试文件

```bash
mkdir -p tests
cat > tests/integration_test.rs << 'EOF'
//! Integration tests for weather-tool plugin

use std::path::PathBuf;

fn get_plugin_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test executable name
    path.pop(); // remove deps
    path.push("libweather_tool.dylib");
    path
}

#[test]
fn test_plugin_exists() {
    let path = get_plugin_path();
    assert!(path.exists(), "Plugin file not found at {:?}", path);
}

#[test]
fn test_plugin_file_size() {
    let path = get_plugin_path();
    let metadata = std::fs::metadata(&path).unwrap();
    assert!(metadata.len() > 1000, "Plugin file too small");
}
EOF
```

### 7.2 运行测试

```bash
cargo test
```

---

## 第八步：打包发布

### 8.1 创建发布包

```bash
# 创建发布目录
mkdir -p dist/weather-tool

# 复制文件
cp target/release/libweather_tool.dylib dist/weather-tool/
cat > dist/weather-tool/README.md << 'EOF'
# Weather Tool Plugin

A NeoTalk plugin for querying weather information.

## Installation

Copy the plugin file to your NeoTalk plugins directory:

```bash
cp libweather_tool.dylib ~/.neotalk/plugins/
```

## Usage

```bash
# Enable the plugin
edge-ai plugin enable weather-tool

# Query weather
curl -X POST http://localhost:3000/api/plugins/weather-tool/command \
  -H "Content-Type: application/json" \
  -d '{"command": "get_weather", "args": {"city": "Tokyo"}}'
```

## Configuration

```json
{
  "cache_ttl_secs": 300,
  "timeout_ms": 5000
}
```
EOF

# 打包
tar czf weather-plugin-0.1.0.tar.gz -C dist weather-tool/
```

---

## 进阶：将插件转换为 WASM

### 将 Native 插件移植到 WASM

```bash
# 1. 修改 Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "weather-tool-wasm"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wasm-bindgen = "0.2"

[dependencies.chrono]
version = "0.4"
features = ["serde", "wasmbind"]
default-features = false
EOF

# 2. 编译为 WASM
cargo build --release --target wasm32-unknown-unknown

# 3. 优化 WASM
wasm-opt target/wasm32-unknown-unknown/release/weather_tool_wasm.wasm \
  -O3 -o weather-tool.wasm

# 4. 创建元数据
cat > weather-tool.json << 'EOF'
{
  "id": "weather-tool",
  "name": "Weather Tool",
  "version": "0.1.0",
  "plugin_type": "tool",
  "required_neotalk_version": ">=0.1.0",
  "description": "Query weather information (WASM version)",
  "author": "Your Name",
  "memory_pages": 16,
  "permissions": ["network"]
}
EOF

# 5. 验证
edge-ai plugin validate weather-tool.wasm
```

---

## 故障排除

### 问题 1: 编译失败

```
error: linker `cc` not found
```

**解决**:
```bash
# macOS
xcode-select --install

# Ubuntu/Debian
sudo apt install build-essential
```

### 问题 2: 插件加载失败

```
Error: Failed to load library: image not found
```

**解决**:
```bash
# 检查依赖
otool -L target/release/libweather_tool.dylib  # macOS
ldd target/release/libweather_tool.so          # Linux

# 设置 rpath
install_name_tool -add_rpath @loader_path target/release/libweather_tool.dylib
```

### 问题 3: API 版本不匹配

```
Error: Plugin API version mismatch
```

**解决**: 更新 `NEOTALK_PLUGIN_API_VERSION` 或重新编译插件。

---

## 总结

恭喜！你已经完成了：

- ✅ 创建了一个完整的 NeoTalk 插件项目
- ✅ 实现了插件的核心功能
- ✅ 编译并验证了插件
- ✅ 部署并测试了插件
- ✅ 编写了集成测试
- ✅ 打包了发布版本

### 下一步

- 为插件添加更多功能
- 集成真实的天气 API
- 编写更完整的测试
- 发布到 NeoTalk 插件市场

### 相关资源

- [WASM 插件开发指南](./WASM_PLUGIN_GUIDE.md)
- [Native 插件 API 参考](./NATIVE_PLUGIN_API.md)
- [插件开发指南](./PLUGIN_DEVELOPMENT.md)
