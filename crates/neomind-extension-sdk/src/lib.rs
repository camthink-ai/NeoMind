//! NeoMind Plugin SDK
//!
//! This SDK provides tools and macros for building dynamic plugins for NeoMind.
//!
//! # Quick Start
//!
//! ```rust
//! use neomind_extension_sdk::prelude::*;
//!
//! export_plugin!(MyPlugin, "my-plugin", "1.0.0", PluginType::Tool);
//! ```

pub mod descriptor;
pub mod error;
#[macro_use]
pub mod macros;
pub mod types;

/// Prelude module with common imports
pub mod prelude {
    pub use crate::descriptor::{PluginDescriptor, PluginType};
    pub use crate::error::PluginResult;
    pub use crate::types::{PluginContext, PluginRequest, PluginResponse};
    pub use serde_json::Value;

    // Macros are automatically available due to #[macro_use]
}

// Re-exports for convenience
pub use descriptor::{PLUGIN_ABI_VERSION, PluginDescriptor, PluginType};
pub use error::{PluginError, PluginResult};
pub use types::{PluginContext, PluginRequest, PluginResponse};

/// Create a plugin instance from a JSON config string.
///
/// # Safety
/// The config_json pointer must point to valid UTF-8 data.
pub unsafe fn create_plugin<P>(config_json: *const u8, config_len: usize) -> *mut ()
where
    P: 'static,
{
    use std::ptr;

    // Parse the config
    let config_str = if config_json.is_null() || config_len == 0 {
        "{}"
    } else {
        // SAFETY: Caller guarantees config_json points to valid data
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            match std::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            }
        }
    };

    let config: serde_json::Value = match serde_json::from_str(config_str) {
        Ok(c) => c,
        Err(_) => return ptr::null_mut(),
    };

    // Create the plugin instance
    // For now, we just store the config as a placeholder
    Box::leak(Box::new(config)) as *mut serde_json::Value as *mut ()
}

/// Destroy a plugin instance.
///
/// # Safety
/// The instance pointer must have been created by `create_plugin`.
pub unsafe fn destroy_plugin<P>(instance: *mut ())
where
    P: 'static,
{
    // Reconstruct the box and drop it
    if !instance.is_null() {
        // SAFETY: Instance was created by create_plugin
        unsafe {
            let _ = Box::from_raw(instance as *mut serde_json::Value);
        }
    }
}
