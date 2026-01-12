# Native Plugin API 参考

本文档详细描述了 NeoTalk Native 插件 API。

## 目录

1. [核心数据结构](#核心数据结构)
2. [FFI 导出函数](#ffi-导出函数)
3. [插件描述符](#插件描述符)
4. [内存管理](#内存管理)
5. [API 版本](#api-版本)

---

## 核心数据结构

### NativePluginDescriptor

插件描述符结构，必须在插件库中导出。

```rust
#[repr(C)]
pub struct NativePluginDescriptor {
    /// API 版本 - 必须等于 NEOTALK_PLUGIN_API_VERSION
    pub api_version: u32,

    /// 插件 ID 指针（null 终止的 UTF-8 字符串）
    pub id: *const u8,
    /// 插件 ID 长度（不包含 null 终止符）
    pub id_len: usize,

    /// 插件名称指针
    pub name: *const u8,
    /// 插件名称长度
    pub name_len: usize,

    /// 插件版本指针
    pub version: *const u8,
    /// 插件版本长度
    pub version_len: usize,

    /// 插件描述指针
    pub description: *const u8,
    /// 插件描述长度
    pub description_len: usize,

    /// 需要的 NeoTalk 版本指针
    pub required_version: *const u8,
    /// 需要的 NeoTalk 版本长度
    pub required_version_len: usize,

    /// 创建函数指针
    pub create: *const (),
    /// 销毁函数指针
    pub destroy: *const (),
}
```

### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `api_version` | `u32` | 必须等于 `NEOTALK_PLUGIN_API_VERSION` (当前为 1) |
| `id` | `*const u8` | 插件唯一标识符，仅限小写字母、数字、连字符 |
| `name` | `*const u8` | 人类可读的插件名称 |
| `version` | `*const u8` | 语义化版本字符串 |
| `description` | `*const u8` | 插件描述 |
| `required_version` | `*const u8` | 兼容的 NeoTalk 版本要求 |
| `create` | `*const ()` | 指向 `neotalk_plugin_create` 函数 |
| `destroy` | `*const ()` | 指向 `neotalk_plugin_destroy` 函数 |

---

## FFI 导出函数

### neotalk_plugin_create

创建插件实例。

```rust
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut YourPluginType;
```

**返回值**:
- 成功: 指向堆分配的插件实例的非空指针
- 失败: 空指针（不推荐，应该总是返回有效指针）

**示例**:
```rust
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
    Box::into_raw(Box::new(MyPlugin::new()))
}
```

**注意事项**:
- 返回的指针必须由 `neotalk_plugin_destroy` 释放
- 不要在此函数中执行耗时的初始化操作
- 如果初始化可能失败，考虑返回 Result 或使用延迟初始化

### neotalk_plugin_destroy

销毁插件实例并释放资源。

```rust
#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut YourPluginType);
```

**参数**:
- `plugin`: 由 `neotalk_plugin_create` 返回的指针

**示例**:
```rust
#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut MyPlugin) {
    if !plugin.is_null() {
        unsafe {
            let _ = Box::from_raw(plugin);
        }
    }
}
```

**注意事项**:
- 必须能够安全地处理空指针
- 不要重复调用（double-free）
- 在此函数中释放所有插件持有的资源

---

## 插件描述符

### 完整示例

```rust
use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};

// 字符串必须以 null 终止
const PLUGIN_ID: &str = "my-awesome-plugin\0";
const PLUGIN_NAME: &str = "My Awesome Plugin\0";
const PLUGIN_VERSION: &str = "0.1.0\0";
const PLUGIN_DESCRIPTION: &str = "Does awesome things\0";
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
    description: PLUGIN_DESCRIPTION.as_ptr(),
    description_len: PLUGIN_DESCRIPTION.len() - 1,
    required_version: REQUIRED_VERSION.as_ptr(),
    required_version_len: REQUIRED_VERSION.len() - 1,
    create: neotalk_plugin_create as *const (),
    destroy: neotalk_plugin_destroy as *const () -> *const (),
};
```

### 字符串长度计算

```rust
// 长度不包含 null 终止符
const TEXT: &str = "hello\0";
assert_eq!(TEXT.len(), 6);      // 包含 '\0'
assert_eq!(TEXT.len() - 1, 5); // 不包含 '\0'

// 在描述符中使用
id_len: TEXT.len() - 1,  // 正确 ✓
id_len: TEXT.len(),      // 错误 ✗
```

---

## 内存管理

### 所有权规则

1. **创建**: `neotalk_plugin_create` 返回的指针由调用者获得所有权
2. **销毁**: 调用者必须调用 `neotalk_plugin_destroy` 来释放内存
3. **唯一**: 每个创建的指针只能被销毁一次

### Box 模式

推荐使用 `Box` 来管理插件实例：

```rust
pub struct MyPlugin { /* ... */ }

#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
    // Box::into_raw 转移所有权给调用者
    Box::into_raw(Box::new(MyPlugin::new()))
}

#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut MyPlugin) {
    if !plugin.is_null() {
        // Box::from_raw 重新获得所有权并释放
        unsafe { Box::from_raw(plugin); }
    }
}
```

### 内部资源管理

如果插件持有需要清理的资源：

```rust
pub struct MyPlugin {
    connection: Option<DbConnection>,
}

impl MyPlugin {
    fn cleanup(&mut self) {
        if let Some(conn) = self.connection.take() {
            conn.close();
        }
    }
}

impl Drop for MyPlugin {
    fn drop(&mut self) {
        self.cleanup();
    }
}
```

---

## API 版本

### 当前版本

```rust
pub const NEOTALK_PLUGIN_API_VERSION: u32 = 1;
```

### 版本变更

| API 版本 | 更改内容 | 兼容性 |
|---------|---------|--------|
| 1 | 初始版本 | - |

### 版本检查

NeoTalk 会在加载时检查 API 版本：

```
if descriptor.api_version != NEOTALK_PLUGIN_API_VERSION {
    return Err(PluginError::VersionMismatch { ... });
}
```

---

## 类型定义

### PluginCreateFn

创建函数类型：

```rust
type PluginCreateFn = unsafe extern "C" fn() -> *mut ();
```

### PluginDestroyFn

销毁函数类型：

```rust
type PluginDestroyFn = unsafe extern "C" fn(*mut ());
```

---

## 完整插件模板

```rust
//! My Plugin for NeoTalk
//!
//! Description of what this plugin does.

use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
use edge_ai_core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

/// Plugin implementation
pub struct MyPlugin {
    initialized: bool,
}

impl MyPlugin {
    pub fn new() -> Self {
        Self {
            initialized: false,
        }
    }

    pub fn do_work(&self) -> Result<String, PluginError> {
        if !self.initialized {
            return Err(PluginError::ExecutionFailed(
                "Plugin not initialized".to_string()
            ));
        }
        Ok("Work done!".to_string())
    }
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// Optional: Implement Plugin trait for Rust-side usage
impl Plugin for MyPlugin {
    fn metadata(&self) -> &PluginMetadata {
        static METADATA: PluginMetadata = {
            let mut meta = PluginMetadata::new(
                "my-plugin",
                "My Plugin",
                "0.1.0",
                ">=0.1.0",
            );
            meta.description = "Description here".to_string();
            meta
        };
        &METADATA
    }

    fn initialize(&mut self, _config: &Value) -> edge_ai_core::plugin::Result<()> {
        self.initialized = true;
        Ok(())
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

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

// ============================================================================
// Plugin Descriptor
// ============================================================================

const PLUGIN_ID: &str = "my-plugin\0";
const PLUGIN_NAME: &str = "My Plugin\0";
const PLUGIN_VERSION: &str = "0.1.0\0";
const PLUGIN_DESC: &str = "Description\0";
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = MyPlugin::new();
        assert!(!plugin.initialized);
    }

    #[test]
    fn test_do_work() {
        let mut plugin = MyPlugin::new();
        plugin.initialize(&json!({})).unwrap();
        assert!(plugin.do_work().is_ok());
    }
}
```

---

## 相关文档

- [插件开发指南](./PLUGIN_DEVELOPMENT.md)
- [插件模板](./TEMPLATE.md)
