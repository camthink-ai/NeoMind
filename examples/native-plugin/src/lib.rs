//! NeoTalk 示例 Native 插件
//!
//! 这是一个示例插件，展示如何创建一个 NeoTalk native 插件。
//! 此插件实现了一个简单的"问候工具"功能。

use edge_ai_core::plugin::native::{NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
use edge_ai_core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

/// 插件实现
pub struct GreetingPlugin {
    /// 自定义问候语
    greeting: String,
    /// 是否已初始化
    initialized: bool,
}

impl GreetingPlugin {
    /// 创建新的插件实例
    pub fn new() -> Self {
        Self {
            greeting: "Hello from NeoTalk!".to_string(),
            initialized: false,
        }
    }

    /// 处理问候请求
    pub fn greet(&self, name: Option<&str>) -> String {
        match name {
            Some(n) => format!("{}, {}!", self.greeting, n),
            None => self.greeting.clone(),
        }
    }

    /// 更新问候语
    pub fn set_greeting(&mut self, greeting: String) {
        self.greeting = greeting;
    }
}

impl Default for GreetingPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FFI 导出函数
// ============================================================================

/// 创建插件实例
///
/// # Safety
/// 调用者必须确保返回的指针最终被 `neotalk_plugin_destroy` 销毁
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut GreetingPlugin {
    let plugin = Box::new(GreetingPlugin::new());
    Box::into_raw(plugin)
}

/// 销毁插件实例
///
/// # Safety
/// `plugin` 必须是由 `neotalk_plugin_create` 创建的有效指针
#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut GreetingPlugin) {
    if !plugin.is_null() {
        unsafe {
            let _ = Box::from_raw(plugin);
        }
    }
}

// ============================================================================
// 插件元数据导出
// ============================================================================

/// 插件 ID
const PLUGIN_ID: &str = "neotalk-example-greeting\0";
/// 插件名称
const PLUGIN_NAME: &str = "Greeting Plugin\0";
/// 插件版本
const PLUGIN_VERSION: &str = "0.1.0\0";
/// 插件描述
const PLUGIN_DESCRIPTION: &str = "A simple greeting plugin for NeoTalk\0";
/// 需要的 NeoTalk 版本
const REQUIRED_VERSION: &str = ">=0.1.0\0";

/// 导出插件描述符
///
/// 这个静态符号由 NeoTalk 的 `NativePluginLoader` 在加载时查找
#[no_mangle]
pub static neotalk_plugin_descriptor: NativePluginDescriptor = NativePluginDescriptor {
    api_version: NEOTALK_PLUGIN_API_VERSION,
    id: PLUGIN_ID.as_ptr(),
    id_len: PLUGIN_ID.len() - 1, // 不包含 null 终止符
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
// 可选：实现 Plugin trait (用于 Rust 内联测试)
// ============================================================================

impl Plugin for GreetingPlugin {
    fn metadata(&self) -> &PluginMetadata {
        static METADATA: PluginMetadata = {
            let mut meta = PluginMetadata::new(
                "neotalk-example-greeting",
                "Greeting Plugin",
                "0.1.0",
                ">=0.1.0",
            );
            meta.description = "A simple greeting plugin for NeoTalk".to_string();
            meta.author = Some("NeoTalk Team".to_string());
            meta.types = vec!["tool".to_string()];
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
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = GreetingPlugin::new();
        assert_eq!(plugin.greet(None), "Hello from NeoTalk!");
        assert_eq!(plugin.greet(Some("World")), "Hello from NeoTalk!, World!");
    }

    #[test]
    fn test_set_greeting() {
        let mut plugin = GreetingPlugin::new();
        plugin.set_greeting("Hi there".to_string());
        assert_eq!(plugin.greet(None), "Hi there");
    }

    #[test]
    fn test_plugin_metadata() {
        let plugin = GreetingPlugin::new();
        let metadata = plugin.metadata();
        assert_eq!(metadata.id, "neotalk-example-greeting");
        assert_eq!(metadata.name, "Greeting Plugin");
        assert_eq!(metadata.version, "0.1.0");
    }

    #[test]
    fn test_plugin_initialize() {
        let mut plugin = GreetingPlugin::new();
        assert!(!plugin.is_initialized());
        plugin.initialize(&serde_json::json!({})).unwrap();
        assert!(plugin.is_initialized());
    }
}
