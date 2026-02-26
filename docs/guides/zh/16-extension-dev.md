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

# 添加 NeoMind core 依赖
cargo add neomind-core --path /path/to/NeoMind/crates/neomind-core
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
neomind-core = { path = "../NeoMind/crates/neomind-core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt-multi-thread", "macros"] }
semver = "1.0"
once_cell = "1.19"
```

**重要**: `crate-type = ["cdylib"]` 是必需的，用于生成动态库。

### 3. 编写扩展

```rust
use std::sync::OnceLock;
use async_trait::async_trait;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDefinition,
    ExtensionCommand, ExtensionMetricValue, ParamMetricValue, MetricDataType,
    ParameterDefinition, ABI_VERSION, Result,
};
use serde_json::Value;
use semver::Version;

struct MyExtension;

impl MyExtension {
    fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "com.example.my-extension",
                "My Extension",
                Version::new(1, 0, 0),
            )
            .with_description("我的第一个 NeoMind 扩展")
            .with_author("Your Name")
        })
    }

    fn metrics(&self) -> &[MetricDefinition] {
        &[]
    }

    fn commands(&self) -> &[ExtensionCommand] {
        &[]
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        Err(ExtensionError::CommandNotFound(command.to_string()))
    }
}

// FFI 导出
use tokio::sync::RwLock;

#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> neomind_core::extension::system::CExtensionMetadata {
    use std::ffi::CStr;

    let id = CStr::from_bytes_with_nul(b"com.example.my-extension\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"My Extension\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"1.0.0\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"My first extension\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"Your Name\0").unwrap();

    neomind_core::extension::system::CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 0,
        command_count: 0,
    }
}

#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    let _config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    let extension: Box<dyn Extension> = Box::new(MyExtension::new());
    Box::into_raw(Box::new(RwLock::new(extension)))
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Extension>>) {
    if !ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ptr);
        }
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

## Extension Trait (V2 API)

所有扩展必须实现 `Extension` trait：

```rust
#[async_trait::async_trait]
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

## FFI 导出

每个扩展必须导出以下符号：

| 符号 | 返回类型 | 说明 |
|------|----------|------|
| `neomind_extension_abi_version` | `u32` | 返回 `ABI_VERSION`（当前为 2） |
| `neomind_extension_metadata` | `CExtensionMetadata` | 返回 C 兼容的元数据 |
| `neomind_extension_create` | `*mut RwLock<Box<dyn Extension>>` | 创建扩展实例 |
| `neomind_extension_destroy` | `void` | 销毁扩展实例 |

## 流式扩展

NeoMind 支持三种流处理模式：

| 模式 | 描述 | 用例 |
|------|------|------|
| **Stateless** | 独立处理每个数据块 | 图像分析 |
| **Stateful** | 基于会话保持上下文 | 视频处理 |
| **Push** | 扩展主动推送数据 | 实时监控 |

完整流式扩展示例请参考 [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) 仓库。

## 最佳实践

### 1. 使用静态存储保存元数据

```rust
// ✅ 正确：使用 OnceLock 获取静态引用
fn metadata(&self) -> &ExtensionMetadata {
    static META: OnceLock<ExtensionMetadata> = OnceLock::new();
    META.get_or_init(|| { /* ... */ })
}

// ❌ 错误：每次调用创建新实例
fn metadata(&self) -> &ExtensionMetadata {
    &ExtensionMetadata::new("id", "name", Version::new(1, 0, 0))
}
```

### 2. Panic 设置

**扩展必须使用 `panic = "unwind"` 编译（不是 `"abort"`）**

```toml
# Cargo.toml
[profile.release]
panic = "unwind"  # 安全性必需！
```

### 3. 线程安全

使用 `Arc<Mutex<T>>` 或 `Arc<RwLock<T>>` 管理共享状态：

```rust
struct MyExtension {
    state: Arc<Mutex<MyState>>,
}
```

### 4. 错误处理

返回有意义的错误信息：

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    // ...
}
```

## 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metadata() {
        let ext = MyExtension::new();
        assert_eq!(ext.metadata().id, "com.example.my-extension");
    }

    #[tokio::test]
    async fn test_command() {
        let ext = MyExtension::new();
        let result = ext.execute_command("test", &serde_json::json!({})).await;
        assert!(result.is_ok());
    }
}
```

## 部署

```bash
# 编译
cargo build --release

# 安装
cp target/release/libmy_extension.dylib ~/.neomind/extensions/

# 通过 API 发现
curl -X POST http://localhost:9375/api/extensions/discover
```

## Dashboard 组件

扩展可以提供自定义 Dashboard 组件，在 NeoMind 仪表板构建器中使用。这允许扩展以自定义方式可视化其数据。

### 概述

Dashboard 组件是：
- **React 组件**，打包为 IIFE（立即调用函数表达式）
- **在 `manifest.json` 中定义**，与扩展放在一起
- **通过 API 提供**，路径为 `/api/extensions/{id}/assets/`
- **运行时动态加载**

### 目录结构

```
my-extension/
├── libmy_extension.dylib      # 编译后的扩展库
├── manifest.json              # 包含组件定义的扩展清单
└── assets/
    └── dashboard/
        └── my-component.js    # 打包后的 React 组件
```

### 清单定义

在扩展库所在目录创建 `manifest.json` 文件：

```json
{
  "id": "com.example.my-extension",
  "name": "My Extension",
  "version": "1.0.0",
  "description": "带有 Dashboard 组件的扩展",
  "author": "Your Name",
  "dashboard_components": [
    {
      "type": "my-custom-card",
      "name": "My Custom Card",
      "description": "自定义 Dashboard 卡片组件",
      "category": "indicators",
      "icon": "Activity",
      "bundle_path": "/assets/dashboard/my-component.js",
      "export_name": "MyCustomCard",
      "size_constraints": {
        "min_w": 2,
        "min_h": 2,
        "default_w": 4,
        "default_h": 3,
        "max_w": 8,
        "max_h": 6
      },
      "has_data_source": true,
      "max_data_sources": 1,
      "has_display_config": true,
      "has_actions": false,
      "config_schema": {
        "type": "object",
        "properties": {
          "title": {
            "type": "string",
            "title": "标题",
            "default": "我的卡片"
          },
          "showValue": {
            "type": "boolean",
            "title": "显示数值",
            "default": true
          }
        }
      },
      "data_source_schema": {
        "type": "object",
        "properties": {
          "extensionMetric": {
            "type": "string",
            "title": "指标"
          }
        }
      },
      "default_config": {
        "title": "我的卡片",
        "showValue": true
      },
      "variants": ["default", "compact"],
      "data_binding": {
        "extension_metric": "temperature",
        "extension_command": null,
        "required_fields": []
      }
    }
  ]
}
```

### 组件类别

| 类别 | 描述 | 示例 |
|------|------|------|
| `indicators` | 数值显示、指标 | 卡片、仪表、状态指示器 |
| `charts` | 可视化数据表示 | 折线图、柱状图、饼图 |
| `controls` | 交互式输入 | 按钮、开关、滑块 |
| `display` | 内容显示 | 图片、网页视图、Markdown |
| `spatial` | 空间与媒体 | 地图、视频流、图层 |
| `business` | 业务相关 | Agent 监控、工作流 |
| `custom` | 扩展自定义 | 任何自定义组件 |

### 组件属性

组件从 Dashboard 接收以下 props：

```typescript
interface DashboardComponentProps {
  // 唯一组件实例 ID
  id: string

  // 组件类型
  type: string

  // 网格位置和大小
  x: number
  y: number
  w: number
  h: number

  // 来自 config_schema 的用户配置
  config: Record<string, unknown>

  // 绑定到此组件的数据源
  dataSources: DataSource[]

  // 当前数据值
  data: Record<string, unknown>

  // 显示/样式配置
  displayConfig?: DisplayConfig

  // 动作配置
  actions?: ActionConfig[]

  // 当前变体
  variant?: string

  // 编辑模式标志
  isEditing?: boolean

  // 更新配置的回调
  onConfigChange?: (config: Record<string, unknown>) => void
}
```

### 构建组件

创建 React 组件并打包为 IIFE：

**组件代码 (`src/MyCustomCard.tsx`):**

```typescript
import React from 'react'

interface MyCustomCardProps {
  id: string
  config: {
    title?: string
    showValue?: boolean
  }
  data: {
    value?: number
    timestamp?: string
  }
  isEditing?: boolean
  onConfigChange?: (config: Record<string, unknown>) => void
}

export const MyCustomCard: React.FC<MyCustomCardProps> = ({
  id,
  config,
  data,
  isEditing,
  onConfigChange,
}) => {
  const { title = '我的卡片', showValue = true } = config
  const { value, timestamp } = data

  return (
    <div className="flex flex-col h-full p-4 bg-white rounded-lg shadow">
      <h3 className="text-lg font-semibold mb-2">{title}</h3>
      {showValue && (
        <div className="text-3xl font-bold text-blue-600">
          {value !== undefined ? value.toFixed(2) : '--'}
        </div>
      )}
      {timestamp && (
        <div className="text-xs text-gray-500 mt-auto">
          更新时间: {new Date(timestamp).toLocaleString()}
        </div>
      )}
    </div>
  )
}

export default MyCustomCard
```

**Vite 配置 (`vite.config.ts`):**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: 'src/MyCustomCard.tsx',
      name: 'MyCustomCard',
      fileName: () => 'my-component.js',
      formats: ['iife'],
    },
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
        },
      },
    },
  },
})
```

**构建:**

```bash
npm run build
# 输出: dist/my-component.js
```

### 数据绑定

组件可以绑定到扩展指标或命令：

**指标绑定:**

```json
{
  "data_binding": {
    "extension_metric": "temperature",
    "required_fields": ["value"]
  }
}
```

组件通过 `data` prop 接收数据：

```typescript
// 数据从扩展的 produce_metrics() 获取
// 指标名称匹配 extension_metric
const { value, unit, timestamp } = data
```

**命令绑定:**

```json
{
  "data_binding": {
    "extension_command": "get_status",
    "required_fields": ["status", "count"]
  }
}
```

当指定命令绑定时，Dashboard 会定期执行命令：

```typescript
// 数据来自执行扩展命令
const result = await api.executeExtensionCommand(extensionId, 'get_status', {})
// 结果作为 data 传递给组件
```

### 图标

使用 [lucide-react](https://lucide.dev/) 图标。常用图标：

| 图标 | 用途 |
|------|------|
| `Activity` | 指标、监控 |
| `Thermometer` | 温度 |
| `Gauge` | 仪表、测量 |
| `LineChart` | 图表 |
| `Image` | 图片显示 |
| `Video` | 视频流 |
| `Map` | 地图 |
| `Lightbulb` | 状态、指示器 |
| `Settings` | 配置 |
| `Zap` | 动作、快速控制 |

### 流式扩展与 Dashboard 组件

对于实时数据，结合流式扩展和 Dashboard 组件：

```typescript
// 使用流式数据的组件
import React, { useEffect, useState } from 'react'
import { useExtensionStream } from '@/hooks/useExtensionStream'

export const StreamingCard: React.FC<Props> = ({ config, id }) => {
  const { extensionId } = config

  const { isConnected, sendChunk, results } = useExtensionStream({
    extensionId,
    mode: 'stateless',
    onResult: (result) => {
      console.log('流结果:', result)
    },
  })

  return (
    <div>
      <div>状态: {isConnected ? '已连接' : '已断开'}</div>
      <div>结果数: {results.length}</div>
    </div>
  )
}
```

### 测试 Dashboard 组件

1. **构建扩展**: `cargo build --release`
2. **构建 Dashboard 组件**: `npm run build`
3. **复制文件到扩展目录**:
   ```bash
   mkdir -p ~/.neomind/extensions/my-extension/assets/dashboard
   cp target/release/libmy_extension.* ~/.neomind/extensions/my-extension/
   cp manifest.json ~/.neomind/extensions/my-extension/
   cp dist/my-component.js ~/.neomind/extensions/my-extension/assets/dashboard/
   ```
4. **重启 NeoMind** 或调用 `/api/extensions/discover`
5. **通过 Dashboard 构建器添加组件**

### API 端点

| 端点 | 描述 |
|------|------|
| `GET /api/extensions/:id/components` | 获取扩展的组件 |
| `GET /api/extensions/dashboard-components` | 获取所有 Dashboard 组件 |
| `GET /api/extensions/:id/assets/*` | 提供静态资源 |

## 官方仓库

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - 扩展示例，包含流式处理支持
- **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** - 设备类型定义

## 参考

- [核心模块文档](01-core.md)
- [主项目文档](../../CLAUDE.md)