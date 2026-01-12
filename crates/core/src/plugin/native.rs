//! Native plugin loader using libloading.
//!
//! This module provides the ability to load Rust-compiled plugins
//! as dynamic libraries (.so, .dylib, .dll) at runtime.

use libloading::{Library, Symbol};
use crate::plugin::{PluginError, Result, PluginMetadata, Plugin};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

/// Native plugin descriptor that must be exported by the plugin library.
///
/// Every native plugin must export a function named `neotalk_plugin_descriptor`
/// that returns a pointer to this structure.
#[repr(C)]
pub struct NativePluginDescriptor {
    /// Plugin API version - should match NEOTALK_PLUGIN_API_VERSION
    pub api_version: u32,

    /// Plugin ID
    pub id: *const u8,

    /// Plugin ID length
    pub id_len: usize,

    /// Plugin name
    pub name: *const u8,

    /// Plugin name length
    pub name_len: usize,

    /// Plugin version
    pub version: *const u8,

    /// Plugin version length
    pub version_len: usize,

    /// Plugin description
    pub description: *const u8,

    /// Plugin description length
    pub description_len: usize,

    /// Required NeoTalk version
    pub required_version: *const u8,

    /// Required version length
    pub required_version_len: usize,

    /// Pointer to the create function
    pub create: *const (),

    /// Pointer to the destroy function
    pub destroy: *const (),
}

/// Current plugin API version.
pub const NEOTALK_PLUGIN_API_VERSION: u32 = 1;

/// Type for the plugin create function.
type PluginCreateFn = unsafe extern "C" fn() -> *mut ();
/// Type for the plugin destroy function.
type PluginDestroyFn = unsafe extern "C" fn(*mut ());

/// Result of loading a native plugin.
#[derive(Debug)]
pub struct LoadedNativePlugin {
    /// The loaded library (kept for cleanup)
    _library: Option<Library>,

    /// Plugin metadata
    pub metadata: PluginMetadata,

    /// Create function
    create_fn: PluginCreateFn,

    /// Destroy function
    destroy_fn: PluginDestroyFn,

    /// Library path for reloading
    library_path: PathBuf,
}

/// Native plugin loader.
pub struct NativePluginLoader {
    /// Directories to search for plugins
    search_paths: Vec<PathBuf>,

    /// Loaded plugins
    loaded_plugins: StdMutex<Vec<LoadedNativePlugin>>,
}

impl NativePluginLoader {
    /// Create a new native plugin loader.
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            loaded_plugins: StdMutex::new(Vec::new()),
        }
    }

    /// Add a search path for plugins.
    pub fn add_search_path(&mut self, path: impl AsRef<Path>) {
        self.search_paths.push(path.as_ref().to_path_buf());
    }

    /// Load a plugin from a specific file path.
    pub fn load_from_path(&self, path: impl AsRef<Path>) -> Result<LoadedNativePlugin> {
        let path = path.as_ref();

        // Security validation: Check if path exists and is a file
        if !path.exists() {
            return Err(PluginError::InitializationFailed(format!(
                "Plugin path does not exist: {:?}",
                path
            )));
        }

        if !path.is_file() {
            return Err(PluginError::InitializationFailed(format!(
                "Plugin path is not a file: {:?}",
                path
            )));
        }

        // Security validation: Check file extension
        let valid_extensions = if cfg!(target_os = "macos") {
            [".dylib"]
        } else if cfg!(target_os = "linux") {
            [".so"]
        } else if cfg!(target_os = "windows") {
            [".dll"]
        } else {
            return Err(PluginError::InitializationFailed(
                "Unsupported platform for native plugins".to_string()
            ));
        };

        let ext = path.extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| PluginError::InitializationFailed(
                "Plugin file has no extension".to_string()
            ))?;

        if !valid_extensions.contains(&ext) {
            return Err(PluginError::InitializationFailed(format!(
                "Invalid plugin extension: {}, expected one of: {:?}",
                ext, valid_extensions
            )));
        }

        // Security validation: Validate path is within allowed search paths
        let canonical_path = path.canonicalize()
            .map_err(|e| PluginError::InitializationFailed(format!("Cannot canonicalize path: {}", e)))?;

        let is_allowed = self.search_paths.iter().any(|search_path| {
            search_path.canonicalize()
                .ok()
                .map(|canonical_search| canonical_path.starts_with(canonical_search))
                .unwrap_or(false)
        });

        if !self.search_paths.is_empty() && !is_allowed {
            return Err(PluginError::InitializationFailed(format!(
                "Plugin path is not within allowed search paths: {:?}",
                path
            )));
        }

        // Load the library
        let library = unsafe {
            Library::new(&canonical_path)
                .map_err(|e| PluginError::InitializationFailed(format!("Failed to load library: {}", e)))?
        };

        // Get the plugin descriptor
        let descriptor: Symbol<NativePluginDescriptor> = unsafe {
            library.get(b"neotalk_plugin_descriptor")
                .map_err(|e| PluginError::InitializationFailed(format!("Missing plugin descriptor: {}", e)))?
        };

        let descriptor = unsafe { &*descriptor };

        // Check API version
        if descriptor.api_version != NEOTALK_PLUGIN_API_VERSION {
            return Err(PluginError::VersionMismatch {
                expected: NEOTALK_PLUGIN_API_VERSION.to_string(),
                found: descriptor.api_version.to_string(),
            });
        }

        // Extract strings
        let id = unsafe {
            std::slice::from_raw_parts(descriptor.id, descriptor.id_len)
        };
        let id = String::from_utf8(id.to_vec())
            .map_err(|e| PluginError::InitializationFailed(format!("Invalid plugin ID: {}", e)))?;

        let name = unsafe {
            std::slice::from_raw_parts(descriptor.name, descriptor.name_len)
        };
        let name = String::from_utf8(name.to_vec())
            .map_err(|e| PluginError::InitializationFailed(format!("Invalid plugin name: {}", e)))?;

        let version = unsafe {
            std::slice::from_raw_parts(descriptor.version, descriptor.version_len)
        };
        let version = String::from_utf8(version.to_vec())
            .map_err(|e| PluginError::InitializationFailed(format!("Invalid version: {}", e)))?;

        let description = unsafe {
            std::slice::from_raw_parts(descriptor.description, descriptor.description_len)
        };
        let description = String::from_utf8(description.to_vec())
            .unwrap_or_default();

        let required_version = unsafe {
            std::slice::from_raw_parts(descriptor.required_version, descriptor.required_version_len)
        };
        let required_version = String::from_utf8(required_version.to_vec())
            .unwrap_or_else(|_| "*".to_string());

        // Create metadata
        let metadata = PluginMetadata::new(&id, &name, &version, &required_version)
            .with_description(description);

        // Get function pointers - we need to extract them before moving library
        let create_fn: PluginCreateFn = unsafe {
            let sym: Symbol<PluginCreateFn> = library.get(b"neotalk_plugin_create")
                .map_err(|e| PluginError::InitializationFailed(format!("Missing create function: {}", e)))?;
            *sym
        };

        let destroy_fn: PluginDestroyFn = unsafe {
            let sym: Symbol<PluginDestroyFn> = library.get(b"neotalk_plugin_destroy")
                .map_err(|e| PluginError::InitializationFailed(format!("Missing destroy function: {}", e)))?;
            *sym
        };

        Ok(LoadedNativePlugin {
            _library: Some(library),
            metadata,
            create_fn,
            destroy_fn,
            library_path: path.to_path_buf(),
        })
    }

    /// Discover and load all plugins from search paths.
    pub fn discover_and_load(&self) -> Result<Vec<LoadedNativePlugin>> {
        let mut plugins = Vec::new();

        for search_path in &self.search_paths {
            if let Ok(entries) = std::fs::read_dir(search_path) {
                for entry in entries.flatten() {
                    let path = entry.path();

                    // Check file extension based on platform
                    let is_plugin = cfg!(all(target_os = "macos", target_os = "ios"))
                        .then(|| path.extension().and_then(|e| e.to_str()) == Some("dylib"))
                        .unwrap_or_else(||
                            cfg!(target_os = "linux")
                                .then(|| path.extension().and_then(|e| e.to_str()) == Some("so"))
                                .unwrap_or_else(||
                                    cfg!(target_os = "windows")
                                        .then(|| path.extension().and_then(|e| e.to_str()) == Some("dll"))
                                        .unwrap_or(false)
                                )
                        );

                    if is_plugin {
                        match self.load_from_path(&path) {
                            Ok(plugin) => {
                                tracing::info!("Loaded native plugin: {}", plugin.metadata.id);
                                plugins.push(plugin);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to load plugin {:?}: {}", path, e);
                            }
                        }
                    }
                }
            }
        }

        Ok(plugins)
    }

    /// Reload a previously loaded plugin.
    pub fn reload(&self, plugin: &LoadedNativePlugin) -> Result<LoadedNativePlugin> {
        self.load_from_path(&plugin.library_path)
    }
}

impl Default for NativePluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for a dynamically loaded plugin.
pub struct NativePluginWrapper {
    /// Plugin instance pointer
    instance: *mut (),

    /// Metadata
    pub metadata: PluginMetadata,

    /// Destroy function
    destroy_fn: PluginDestroyFn,

    /// Whether the plugin has been destroyed
    destroyed: StdMutex<bool>,
}

impl NativePluginWrapper {
    /// Create a wrapper from a loaded plugin.
    ///
    /// # Safety
    /// The create_fn must produce a valid plugin instance.
    pub unsafe fn from_loaded(plugin: &LoadedNativePlugin) -> Result<Self> {
        let instance = (plugin.create_fn)();

        if instance.is_null() {
            return Err(PluginError::InitializationFailed(
                "Plugin create function returned null".to_string()
            ));
        }

        Ok(Self {
            instance,
            metadata: plugin.metadata.clone(),
            destroy_fn: plugin.destroy_fn,
            destroyed: StdMutex::new(false),
        })
    }
}

impl Drop for NativePluginWrapper {
    fn drop(&mut self) {
        // Handle poisoned mutex gracefully
        match self.destroyed.lock() {
            Ok(mut destroyed) => {
                if !*destroyed {
                    unsafe {
                        (self.destroy_fn)(self.instance);
                    }
                    *destroyed = true;
                }
            }
            Err(e) => {
                // Mutex is poisoned, try to recover and clean up anyway
                let mut destroyed = e.into_inner();
                if !*destroyed {
                    unsafe {
                        (self.destroy_fn)(self.instance);
                    }
                    *destroyed = true;
                }
            }
        }
    }
}

/// Helper macro to export the plugin descriptor.
///
/// # Usage
/// ```ignore
/// use edge_ai_core::plugin::native::{export_plugin_descriptor, NEOTALK_PLUGIN_API_VERSION, NativePluginDescriptor};
///
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     // ...
/// }
///
/// extern "C" fn neotalk_plugin_create() -> *mut MyPlugin {
///     Box::into_raw(Box::new(MyPlugin::new()))
/// }
///
/// extern "C" fn neotalk_plugin_destroy(plugin: *mut MyPlugin) {
///     unsafe { Box::from_raw(plugin); }
/// }
///
/// // Export the descriptor
/// export_plugin_descriptor! {
///     id: "my-plugin",
///     name: "My Plugin",
///     version: "1.0.0",
///     description: "A sample native plugin",
///     required_version: ">=1.0.0",
/// }
/// ```
#[macro_export]
macro_rules! export_plugin_descriptor {
    (
        id: $id:expr,
        name: $name:expr,
        version: $version:expr,
        description: $description:expr,
        required_version: $required_version:expr,
    ) => {
        /// Export the plugin descriptor.
        #[no_mangle]
        pub static neotalk_plugin_descriptor: $crate::plugin::native::NativePluginDescriptor = {
            $crate::plugin::native::NativePluginDescriptor {
                api_version: $crate::plugin::native::NEOTALK_PLUGIN_API_VERSION,
                id: concat!($id, "\0").as_ptr(),
                id_len: concat!($id, "\0").len() - 1,
                name: concat!($name, "\0").as_ptr(),
                name_len: concat!($name, "\0").len() - 1,
                version: concat!($version, "\0").as_ptr(),
                version_len: concat!($version, "\0").len() - 1,
                description: concat!($description, "\0").as_ptr(),
                description_len: concat!($description, "\0").len() - 1,
                required_version: concat!($required_version, "\0").as_ptr(),
                required_version_len: concat!($required_version, "\0").len() - 1,
                create: $crate::plugin::native::neotalk_plugin_create as *const (),
                destroy: $crate::plugin::native::neotalk_plugin_destroy as *const () -> *const (),
            }
        };

        // Ensure the create/destroy functions exist
        extern "C" {
            fn neotalk_plugin_create() -> *mut ();
            fn neotalk_plugin_destroy(*mut ());
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = NativePluginLoader::new();
        assert!(loader.search_paths.is_empty());

        loader.add_search_path("/tmp/plugins");
        assert_eq!(loader.search_paths.len(), 1);
    }

    #[test]
    fn test_api_version() {
        assert_eq!(NEOTALK_PLUGIN_API_VERSION, 1);
    }
}
