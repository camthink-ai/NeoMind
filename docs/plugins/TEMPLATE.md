# NeoTalk 插件模板

完整的 NeoTalk Native 插件模板代码。

## 快速开始

```bash
# 创建新插件
mkdir -p my-plugin/src
cd my-plugin
cargo init --lib
```

## Cargo.toml
[package]
name = "neotalk-plugin-template"
version = "0.1.0"
edition = "2021"
authors = ["Your Name <your.email@example.com>"]
description = "A NeoTalk plugin template"
license = "MIT OR Apache-2.0"

[lib]
crate-type = ["cdylib"]

[dependencies]
edge-ai-core = { path = "../../crates/core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }

[features]
default = []
test-utils = []
```

---

## src/lib.rs 模板

```rust
//! {{PLUGIN_NAME}} for NeoTalk
//!
//! {{PLUGIN_DESCRIPTION}}
//!
//! ## Configuration
//!
//! The plugin accepts the following configuration:
//!
//! ```json
//! {
//!   "option1": "value1",
//!   "option2": 42
//! }
//! ```
//!
//! ## Commands
//!
//! - `status` - Get plugin status
//! - `do_work` - Execute the main functionality

#![deny(missing_docs)]

use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
use edge_ai_core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

/// {{PLUGIN_NAME}} plugin
///
/// This plugin does {{PLUGIN_DESCRIPTION}}.
pub struct {{PLUGIN_STRUCT_NAME}} {
    /// Whether the plugin is initialized
    initialized: bool,

    /// Plugin configuration
    config: Value,

    /// Internal state
    state: Option<InternalState>,
}

/// Internal state for the plugin
struct InternalState {
    // Add your internal state here
    counter: usize,
}

impl {{PLUGIN_STRUCT_NAME}} {
    /// Create a new plugin instance
    pub fn new() -> Self {
        Self {
            initialized: false,
            config: json!({}),
            state: Some(InternalState {
                counter: 0,
            }),
        }
    }

    /// Initialize the plugin with configuration
    pub fn initialize(&mut self, config: &Value) -> Result<(), PluginError> {
        self.config = config.clone();

        // Parse configuration
        // let option1 = config.get("option1")
        //     .and_then(|v| v.as_str())
        //     .ok_or_else(|| PluginError::InvalidConfiguration(
        //         "Missing 'option1' in configuration".to_string()
        //     ))?;

        // Initialize internal state
        self.initialized = true;
        Ok(())
    }

    /// Start the plugin
    pub fn start(&mut self) -> Result<(), PluginError> {
        if !self.initialized {
            return Err(PluginError::InitializationFailed(
                "Plugin not initialized".to_string()
            ));
        }

        // Start any background tasks or connections
        tracing::info!("{{PLUGIN_STRUCT_NAME}} started");
        Ok(())
    }

    /// Stop the plugin
    pub fn stop(&mut self) -> Result<(), PluginError> {
        // Stop any background tasks or connections
        tracing::info!("{{PLUGIN_STRUCT_NAME}} stopped");
        Ok(())
    }

    /// Execute the main functionality
    pub fn do_work(&self) -> Result<Value, PluginError> {
        if !self.initialized {
            return Err(PluginError::ExecutionFailed(
                "Plugin not initialized".to_string()
            ));
        }

        // Your plugin logic here
        Ok(json!({
            "result": "success",
            "data": "work completed"
        }))
    }

    /// Handle a plugin command
    pub fn handle_command(&self, command: &str, args: &Value) -> Result<Value, PluginError> {
        match command {
            "status" => Ok(json!({
                "initialized": self.initialized,
                "config": self.config,
            })),
            "do_work" => self.do_work(),
            _ => Err(PluginError::ExecutionFailed(
                format!("Unknown command: {}", command)
            )),
        }
    }
}

impl Default for {{PLUGIN_STRUCT_NAME}} {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Plugin Trait Implementation (optional)
// ============================================================================

impl Plugin for {{PLUGIN_STRUCT_NAME}} {
    fn metadata(&self) -> &PluginMetadata {
        static METADATA: PluginMetadata = {
            let mut meta = PluginMetadata::new(
                "{{PLUGIN_ID}}",
                "{{PLUGIN_NAME}}",
                "{{PLUGIN_VERSION}}",
                "{{REQUIRED_VERSION}}",
            );
            meta.description = "{{PLUGIN_DESCRIPTION}}".to_string();
            meta.author = Some("{{PLUGIN_AUTHOR}}".to_string());
            meta.types = vec!["{{PLUGIN_TYPE}}".to_string()];
            meta
        };
        &METADATA
    }

    fn initialize(&mut self, config: &Value) -> edge_ai_core::plugin::Result<()> {
        Self::initialize(self, config)
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

/// Create a new plugin instance
///
/// # Safety
/// The returned pointer must be eventually freed using `neotalk_plugin_destroy`
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut {{PLUGIN_STRUCT_NAME}} {
    Box::into_raw(Box::new({{PLUGIN_STRUCT_NAME}}::new()))
}

/// Destroy a plugin instance
///
/// # Safety
/// `plugin` must be a pointer created by `neotalk_plugin_create`
#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut {{PLUGIN_STRUCT_NAME}}) {
    if !plugin.is_null() {
        unsafe {
            let _ = Box::from_raw(plugin);
        }
    }
}

// ============================================================================
// Plugin Descriptor
// ============================================================================

/// Plugin ID (must be null-terminated)
const PLUGIN_ID: &str = "{{PLUGIN_ID}}\0";
/// Plugin name (must be null-terminated)
const PLUGIN_NAME: &str = "{{PLUGIN_NAME}}\0";
/// Plugin version (must be null-terminated)
const PLUGIN_VERSION: &str = "{{PLUGIN_VERSION}}\0";
/// Plugin description (must be null-terminated)
const PLUGIN_DESCRIPTION: &str = "{{PLUGIN_DESCRIPTION}}\0";
/// Required NeoTalk version (must be null-terminated)
const REQUIRED_VERSION: &str = "{{REQUIRED_VERSION}}\0";

/// Plugin descriptor exported for NeoTalk
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = {{PLUGIN_STRUCT_NAME}}::new();
        assert!(!plugin.initialized);
    }

    #[test]
    fn test_initialization() {
        let mut plugin = {{PLUGIN_STRUCT_NAME}}::new();
        let config = json!({
            "option1": "value1"
        });
        assert!(plugin.initialize(&config).is_ok());
        assert!(plugin.initialized);
    }

    #[test]
    fn test_do_work() {
        let mut plugin = {{PLUGIN_STRUCT_NAME}}::new();
        plugin.initialize(&json!({})).unwrap();
        let result = plugin.do_work();
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_command() {
        let mut plugin = {{PLUGIN_STRUCT_NAME}}::new();
        plugin.initialize(&json!({})).unwrap();

        let result = plugin.handle_command("status", &json!({}));
        assert!(result.is_ok());

        let json = result.unwrap();
        assert_eq!(json["initialized"], true);
    }

    #[test]
    fn test_unknown_command() {
        let plugin = {{PLUGIN_STRUCT_NAME}}::new();
        let result = plugin.handle_command("unknown", &json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata() {
        let plugin = {{PLUGIN_STRUCT_NAME}}::new();
        let metadata = plugin.metadata();
        assert_eq!(metadata.id, "{{PLUGIN_ID}}");
        assert_eq!(metadata.name, "{{PLUGIN_NAME}}");
    }
}
```

---

## 占位符说明

使用以下占位符，在复制后需要替换：

| 占位符 | 说明 | 示例 |
|--------|------|------|
| `{{PLUGIN_NAME}}` | 插件显示名称 | `"My Awesome Plugin"` |
| `{{PLUGIN_STRUCT_NAME}}` | Rust 结构体名 | `"MyAwesomePlugin"` |
| `{{PLUGIN_DESCRIPTION}}` | 插件描述 | `"Does awesome things"` |
| `{{PLUGIN_ID}}` | 插件 ID (小写，连字符) | `"my-awesome-plugin"` |
| `{{PLUGIN_VERSION}}` | 语义化版本 | `"0.1.0"` |
| `{{PLUGIN_AUTHOR}}` | 作者信息 | `"Your Name"` |
| `{{PLUGIN_TYPE}}` | 插件类型 | `"tool"` |
| `{{REQUIRED_VERSION}}` | NeoTalk 版本要求 | `">=0.1.0"` |

---

## 相关文档

- [插件开发指南](./PLUGIN_DEVELOPMENT.md)
- [Native Plugin API](./NATIVE_PLUGIN_API.md)
