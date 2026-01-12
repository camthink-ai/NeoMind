# NeoTalk WASM 插件开发指南

本指南介绍如何使用 WebAssembly (WASM) 为 NeoTalk 开发跨语言插件。

## 目录

1. [为什么选择 WASM](#为什么选择-wasm)
2. [快速开始](#快速开始)
3. [WASM 插件结构](#wasm-插件结构)
4. [开发环境设置](#开发环境设置)
5. [使用 Rust 开发 WASM 插件](#使用-rust-开发-wasm-插件)
6. [使用 AssemblyScript 开发 WASM 插件](#使用-assemblyscript-开发-wasm-插件)
7. [使用 TinyGo 开发 WASM 插件](#使用-tinygo-开发-wasm-插件)
8. [插件元数据 (Sidecar JSON)](#插件元数据-sidecar-json)
9. [宿主函数 (Host Functions)](#宿主函数-host-functions)
10. [CLI 工具使用](#cli-工具使用)
11. [部署与测试](#部署与测试)
12. [故障排除](#故障排除)

---

## 为什么选择 WASM

### 优势

| 特性 | WASM | Native |
|------|------|--------|
| **沙箱安全** | ✓ 内存隔离，资源受限 | ✗ 直接访问系统资源 |
| **跨平台** | ✓ 一次编译，到处运行 | ✗ 需要为每个平台编译 |
| **语言支持** | ✓ Rust, C/C++, Go, AssemblyScript, JavaScript 等 | ✗ 仅支持 C ABI |
| **热加载** | ✓ 安全地加载/卸载 | ⚠️ 可能导致崩溃 |
| **性能** | ⚠️ 接近原生 (~90%) | ✓ 原生性能 |

### 适用场景

- **第三方插件**: 需要从社区安全加载
- **多语言开发**: 团队使用不同编程语言
- **频繁更新**: 需要热加载功能
- **云端分发**: 通过网络分发插件

---

## 快速开始

### 最简 WASM 插件

使用 Rust 创建一个简单的 WASM 插件：

```bash
# 1. 创建项目
cargo new --lib my-wasm-plugin
cd my-wasm-plugin

# 2. 添加依赖
cat >> Cargo.toml << 'EOF'
[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
EOF

# 3. 编译为 WASM
cargo build --release --target wasm32-unknown-unknown

# 4. 优化 wasm 文件
wasm-opt target/wasm32-unknown-unknown/release/my_wasm_plugin.wasm \
  -O3 -o my-plugin.wasm

# 5. 创建元数据
cat > my-plugin.json << 'EOF'
{
  "id": "my-wasm-plugin",
  "name": "My WASM Plugin",
  "version": "0.1.0",
  "description": "A simple WASM plugin",
  "plugin_type": "tool",
  "required_neotalk_version": ">=0.1.0",
  "author": "Your Name",
  "memory_pages": 16
}
EOF

# 6. 验证插件
edge-ai plugin validate my-plugin.wasm
```

---

## WASM 插件结构

### 文件组成

```
my-plugin/
├── src/
│   └── lib.rs          # 源代码
├── Cargo.toml          # Rust 配置
├── my-plugin.wasm      # 编译输出
└── my-plugin.json      # 元数据 (sidecar)
```

### 必需的导出函数

WASM 插件需要导出以下函数：

| 函数 | 描述 | 签名 |
|------|------|------|
| `neotalk_init` | 初始化插件 | `(ptr: u32, len: u32) -> u32` |
| `neotalk_command` | 执行命令 | `(cmd_ptr: u32, cmd_len: u32, args_ptr: u32, args_len: u32) -> u32` |
| `neotalk_get_result` | 获取结果 | `(ptr: u32, len_ptr: u32) -> u32` |
| `neotalk_cleanup` | 清理资源 | `()` |

---

## 开发环境设置

### 安装工具链

#### Rust + WASM 目标

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 添加 WASM 目标
rustup target add wasm32-unknown-unknown

# 安装 wasm-opt (可选，用于优化)
# macOS
brew install binaryen
# Linux
apt install binaryen
```

#### AssemblyScript

```bash
npm install -g assemblyscript
```

#### TinyGo

```bash
# macOS
brew install tinygo

# Linux
curl -sSL https://github.com/tinygo-org/tinygo/releases/download/v0.31.0/tinygo_0.31.0.linux_amd64.tar.gz | tar xz
export PATH=$PATH:tinygo/bin
```

---

## 使用 Rust 开发 WASM 插件

### 项目配置

```toml
# Cargo.toml
[package]
name = "my-wasm-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wasm-bindgen = "0.2"
```

### 基础插件实现

```rust
//! My WASM Plugin for NeoTalk

use serde_json::Value;

// 全局状态
static mut RESULT: Option<String> = None;

/// 初始化插件
#[no_mangle]
pub extern "C" fn neotalk_init(config_ptr: *const u8, config_len: usize) -> u32 {
    // 读取配置 (在实际实现中需要从 WASM 内存读取)
    // 这里简化处理
    1 // 成功
}

/// 处理命令
#[no_mangle]
pub extern "C" fn neotalk_command(
    cmd_ptr: *const u8,
    cmd_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> u32 {
    // 解析命令和参数
    let cmd = unsafe {
        std::slice::from_raw_parts(cmd_ptr, cmd_len)
    };
    let cmd_str = std::str::from_utf8(cmd).unwrap_or("");

    let result = match cmd_str {
        "ping" => Ok(json!({ "status": "pong" }).to_string()),
        "echo" => {
            // 解析参数并返回
            Ok(json!({ "echo": "received" }).to_string())
        }
        _ => Err(json!({ "error": "unknown command" }).to_string()),
    };

    // 存储结果
    unsafe {
        RESULT = Some(result.unwrap_or_else(|e| e));
    }

    0 // 成功
}

/// 获取结果
#[no_mangle]
pub extern "C" fn neotalk_get_result(ptr: *mut u32, len_ptr: *mut u32) -> u32 {
    unsafe {
        if let Some(ref result) = RESULT {
            // 在实际实现中需要分配 WASM 内存并返回指针
            let bytes = result.as_bytes();
            *ptr = bytes.as_ptr() as u32;
            *len_ptr = bytes.len();
            0 // 成功
        } else {
            1 // 无结果
        }
    }
}

/// 清理资源
#[no_mangle]
pub extern "C" fn neotalk_cleanup() {
    unsafe {
        RESULT = None;
    }
}
```

### 高级示例：带内存管理

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;

// 自定义分配器用于追踪内存
struct TrackedAllocator;

unsafe impl GlobalAlloc for TrackedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: TrackedAllocator = TrackedAllocator;

/// 分配内存并返回指针
#[no_mangle]
pub extern "C" fn neotalk_alloc(size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, 1).unwrap();
    unsafe { System.alloc(layout) }
}

/// 释放内存
#[no_mangle]
pub extern "C" fn neotalk_free(ptr: *mut u8, size: usize) {
    let layout = Layout::from_size_align(size, 1).unwrap();
    unsafe { System.dealloc(ptr, layout) }
}
```

---

## 使用 AssemblyScript 开发 WASM 插件

### 项目设置

```bash
npm init asconfig .
```

### 插件实现

```typescript
// assembly/index.ts

// 状态变量
let result: string | null = null;

// 导出的初始化函数
export function neotalk_init(configPtr: number, configLen: number): i32 {
    return 1; // 成功
}

// 导出的命令处理函数
export function neotalk_command(
    cmdPtr: number,
    cmdLen: number,
    argsPtr: number,
    argsLen: number
): i32 {
    // 从内存读取命令字符串
    const cmd = String.UTF8.decode(
        changetype<usize>(cmdPtr),
        cmdLen
    );

    switch (cmd) {
        case "ping":
            result = `{"status":"pong"}`;
            break;
        case "add":
            result = `{"result": 42}`;
            break;
        default:
            result = `{"error":"unknown command"}`;
    }

    return 0; // 成功
}

// 导出的结果获取函数
export function neotalk_get_result(ptrPtr: usize, lenPtr: usize): i32 {
    if (result === null) {
        return 1; // 无结果
    }

    const bytes = String.UTF8.encode(result);
    const memPtr = heap.alloc(bytes.length);
    memory.copy(memPtr, changetype<usize>(bytes), bytes.length);

    store<usize>(ptrPtr, memPtr);
    store<usize>(lenPtr, bytes.length);

    return 0; // 成功
}

// 清理函数
export function neotalk_cleanup(): void {
    result = null;
}
```

### 编译

```bash
asc assembly/index.ts -b my-plugin.wasm -O3z --runtime incremental
```

---

## 使用 TinyGo 开发 WASM 插件

### 插件实现

```go
// main.go
package main

import (
    "encoding/json"
    "unsafe"
)

//export neotalk_init
func neotalk_init(configPtr uint32, configLen uint32) uint32 {
    return 1 // 成功
}

//export neotalk_command
func neotalk_command(cmdPtr, cmdLen, argsPtr, argsLen uint32) uint32 {
    // 读取命令
    cmd := goString(cmdPtr, cmdLen)

    switch cmd {
    case "ping":
        setResult(`{"status":"pong"}`)
    case "add":
        setResult(`{"result":42}`)
    default:
        setResult(`{"error":"unknown command"}`)
    }

    return 0 // 成功
}

//export neotalk_cleanup
func neotalk_cleanup() {
    // 清理资源
}

// 辅助函数
func setResult(s string) {
    // 存储结果到内存
}

func goString(ptr uint32, len uint32) string {
    // 从 WASM 内存读取字符串
    bytes := (*[1 << 20]byte)(unsafe.Pointer(ptr))[:len:len]
    return string(bytes)
}

func main() {
    // TinyGo WASM 模块需要 main 函数
}
```

### 编译

```bash
tinygo build -o my-plugin.wasm -target wasm main.go
```

---

## 插件元数据 (Sidecar JSON)

### 元数据结构

WASM 插件需要一个配套的 JSON 文件来描述其元数据：

```json
{
  "$schema": "https://neotalk.dev/schemas/plugin-v1.json",

  // 基本标识
  "id": "my-wasm-plugin",
  "name": "My WASM Plugin",
  "version": "0.1.0",
  "description": "A plugin that does amazing things",

  // 类型与兼容性
  "plugin_type": "tool",
  "required_neotalk_version": ">=0.1.0",

  // 作者信息
  "author": "Your Name",
  "homepage": "https://github.com/user/my-plugin",
  "repository": "https://github.com/user/my-plugin.git",
  "license": "MIT",

  // 资源限制
  "memory_pages": 16,
  "max_fuel": 1000000,
  "timeout_ms": 5000,

  // 权限
  "permissions": [
    "network",
    "file_read",
    "file_write"
  ],

  // 配置架构
  "config_schema": {
    "type": "object",
    "properties": {
      "api_key": { "type": "string" },
      "endpoint": { "type": "string", "format": "uri" }
    },
    "required": ["api_key"]
  },

  // 导入的宿主函数
  "imports": {
    "neotalk": {
      "log": "function",
      "get_config": "function"
    }
  },

  // 导出的函数
  "exports": [
    "neotalk_init",
    "neotalk_command",
    "neotalk_get_result",
    "neotalk_cleanup"
  ]
}
```

### 字段说明

| 字段 | 类型 | 必需 | 说明 |
|------|------|------|------|
| `id` | string | ✓ | 插件唯一标识，小写字母、数字、连字符 |
| `name` | string | ✓ | 人类可读的名称 |
| `version` | string | ✓ | 语义化版本 |
| `plugin_type` | string | ✓ | tool, llm_backend, storage_backend 等 |
| `memory_pages` | number | - | WASM 内存页数 (每页 64KB) |
| `max_fuel` | number | - | 最大执行指令数 (fuel metering) |
| `permissions` | array | - | 所需权限列表 |

---

## 宿主函数 (Host Functions)

宿主函数是 NeoTalk 提供给 WASM 插件调用的 API。

### 可用的宿主函数

```rust
// 在 WASM 插件中可以调用这些函数

extern "C" {
    // 日志
    fn neotalk_log(level: u32, ptr: *const u8, len: usize);

    // 配置
    fn neotalk_get_config(key_ptr: *const u8, key_len: usize, out_ptr: *mut u8, out_len: *mut usize) -> i32;

    // HTTP 请求
    fn neotalk_http_get(url_ptr: *const u8, url_len: usize) -> i32;
    fn neotalk_http_post(url_ptr: *const u8, url_len: usize, body_ptr: *const u8, body_len: usize) -> i32;

    // 时间
    fn neotalk_now_ms() -> u64;

    // 随机数
    fn neotalk_random() -> u32;
}
```

### 在 Rust 中使用

```rust
#[no_mangle]
pub extern "C" fn neotalk_command(cmd_ptr: *const u8, cmd_len: usize, ...) -> u32 {
    // 调用宿主函数记录日志
    extern "C" {
        fn neotalk_log(level: u32, ptr: *const u8, len: usize);
    }

    let msg = b"Processing command\0";
    unsafe {
        neotalk_log(1, msg.as_ptr(), msg.len() - 1);
    }

    // ... 处理命令
}
```

---

## CLI 工具使用

### 验证插件

```bash
edge-ai plugin validate my-plugin.wasm
```

输出示例：
```
✓ Plugin validation passed

Checks:
  ✓ WASM format valid
  ✓ Required exports present
  ✓ Metadata file found
  ✓ Memory limits within range

Warnings:
  ! No permissions specified (plugin will run in strict sandbox)
  ! Large memory size: 32 pages (2048 KB)

Details:
  Plugin: my-wasm-plugin v0.1.0
  Type: tool
  Memory: 32 pages
```

### 详细验证输出

```bash
edge-ai plugin validate my-plugin.wasm --verbose
```

### 列出插件

```bash
# 列出当前目录的插件
edge-ai plugin list

# 列出指定目录
edge-ai plugin list --dir /path/to/plugins

# 按类型筛选
edge-ai plugin list --type tool
edge-ai plugin list --type llm_backend
```

### 查看插件信息

```bash
edge-ai plugin info my-plugin.wasm
```

输出：
```
Plugin Information
==================

ID:              my-wasm-plugin
Name:            My WASM Plugin
Version:         0.1.0
Type:            Tool
Description:     A plugin that does amazing things
Author:          Your Name
Homepage:        https://github.com/user/my-plugin
Repository:      https://github.com/user/my-plugin.git
License:         MIT

Requirements:
  NeoTalk:        >=0.1.0

Config Schema:
{
  "type": "object",
  "properties": {
    "api_key": { "type": "string" },
    "endpoint": { "type": "string", "format": "uri" }
  },
  "required": ["api_key"]
}

Module:          /path/to/my-plugin.wasm
Memory Size:     1024 KB
```

### 创建插件脚手架

```bash
edge-ai plugin create my-tool --type tool
edge-ai plugin create my-llm --type llm_backend --output ./plugins
```

---

## 部署与测试

### 本地测试

```bash
# 1. 将插件放到插件目录
cp my-plugin.wasm ~/.neotalk/plugins/
cp my-plugin.json ~/.neotalk/plugins/

# 2. 启动 NeoTalk
edge-ai serve

# 3. 通过 API 测试
curl http://localhost:3000/api/plugins
curl http://localhost:3000/api/plugins/my-wasm-plugin/health
```

### 插件 API 测试

```bash
# 获取插件状态
curl http://localhost:3000/api/plugins/my-wasm-plugin

# 执行插件命令
curl -X POST http://localhost:3000/api/plugins/my-wasm-plugin/command \
  -H "Content-Type: application/json" \
  -d '{"command": "ping", "args": {}}'

# 获取插件配置
curl http://localhost:3000/api/plugins/my-wasm-plugin/config

# 更新配置
curl -X PUT http://localhost:3000/api/plugins/my-wasm-plugin/config \
  -H "Content-Type: application/json" \
  -d '{"api_key": "new-key"}'
```

---

## 故障排除

### 常见错误

#### 1. WASM 加载失败

```
Error: Failed to load WASM module: magic header not found
```

**原因**: 文件不是有效的 WASM 格式

**解决**:
```bash
# 验证 WASM 文件
file my-plugin.wasm
# 应该输出: WebAssembly (wasm) binary module

# 重新编译
cargo build --release --target wasm32-unknown-unknown
```

#### 2. 缺少必需导出

```
Error: Required export 'neotalk_command' not found
```

**解决**: 确保插件导出所有必需函数：
- `neotalk_init`
- `neotalk_command`
- `neotalk_get_result`
- `neotalk_cleanup`

#### 3. 内存不足

```
Error: Out of memory: failed to allocate
```

**解决**: 在元数据中增加内存页数：
```json
{
  "memory_pages": 32  // 每页 64KB
}
```

#### 4. 燃料耗尽

```
Error: Fuel metering exhausted
```

**解决**: 增加燃料限制：
```json
{
  "max_fuel": 10000000
}
```

### 调试技巧

#### 使用 wasm-objdump

```bash
# 查看导出的函数
wasm-objdump -x my-plugin.wasm | grep Export

# 查看导入的函数
wasm-objdump -x my-plugin.wasm | grep Import
```

#### 使用 wasm2wat

```bash
# 转换为可读的 WAT 格式
wasm2wat my-plugin.wasm > my-plugin.wat
```

### 性能优化

#### 1. 使用 wasm-opt

```bash
wasm-opt my-plugin.wasm -O3 -o my-plugin-opt.wasm
```

#### 2. 减少代码大小

```toml
# Cargo.toml
[profile.release]
opt-level = "z"     # 优化大小
lto = true          # 链接时优化
codegen-units = 1   # 单个编译单元
strip = true        # 移除符号
```

#### 3. 使用 wasm-strip

```bash
wasm-strip my-plugin.wasm
```

---

## 相关文档

- [插件开发指南](./PLUGIN_DEVELOPMENT.md) - Native 插件开发
- [Native 插件 API 参考](./NATIVE_PLUGIN_API.md) - Native API 详细说明
- [端到端教程](./END_TO_END_TUTORIAL.md) - 从零创建插件
