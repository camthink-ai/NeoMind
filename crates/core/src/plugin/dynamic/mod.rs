//! Dynamic plugin loading system.
//!
//! This module provides functionality for loading plugins from dynamic library files
//! (.so on Linux, .dylib on macOS, .dll on Windows) at runtime.

pub mod descriptor;
pub mod factory;
pub mod loader;
pub mod security;
pub mod wrapper;

// Re-exports for convenience
pub use descriptor::{
    DescriptorError, PluginCapabilities, PluginDescriptor, PluginCreateFn,
    PluginDestroyFn, ParsedPluginDescriptor, PLUGIN_ABI_VERSION,
};
pub use factory::{DynamicPluginFactory, PluginFactoryEvent, wrapper_to_unified};
pub use loader::{DynamicPluginLoader, LoadedPlugin};
pub use security::SecurityContext;
pub use wrapper::DynamicPluginWrapper;
