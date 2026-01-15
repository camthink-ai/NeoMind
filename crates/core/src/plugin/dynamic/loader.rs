//! Dynamic plugin loader.
//!
//! This module provides functionality to load plugins from dynamic library files.

use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};

use super::{
    descriptor::{PluginDescriptor, PLUGIN_ABI_VERSION},
    security::SecurityContext,
    wrapper::DynamicPluginWrapper,
    ParsedPluginDescriptor,
};
use crate::plugin::{DynUnifiedPlugin, PluginError, Result};

/// Result of loading a plugin.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// The parsed descriptor
    pub descriptor: ParsedPluginDescriptor,

    /// Path to the plugin file
    pub path: PathBuf,

    /// When the plugin was loaded
    pub loaded_at: chrono::DateTime<chrono::Utc>,
}

/// Dynamic plugin loader.
pub struct DynamicPluginLoader {
    /// Security context for validation
    security: SecurityContext,

    /// Search paths for plugins
    search_paths: Vec<PathBuf>,

    /// Loaded plugins
    loaded_plugins: Vec<LoadedPlugin>,
}

impl DynamicPluginLoader {
    /// Create a new loader with default security settings.
    pub fn new() -> Self {
        let security = SecurityContext::default();
        let mut search_paths = security.allowed_paths.clone();

        // Add current directory for development
        search_paths.push(PathBuf::from("."));

        Self {
            security,
            search_paths,
            loaded_plugins: Vec::new(),
        }
    }

    /// Create a loader with a custom security context.
    pub fn with_security(security: SecurityContext) -> Self {
        let search_paths = security.allowed_paths.clone();
        Self {
            security,
            search_paths,
            loaded_plugins: Vec::new(),
        }
    }

    /// Add a search path for plugins.
    pub fn add_search_path(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.search_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Get all search paths.
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Get all loaded plugins.
    pub fn loaded_plugins(&self) -> &[LoadedPlugin] {
        &self.loaded_plugins
    }

    /// Load a plugin from a specific file path.
    pub fn load_from_path(&mut self, path: impl AsRef<Path>) -> Result<(DynamicPluginWrapper, Library)> {
        let path = path.as_ref();

        // Validate path against security rules
        self.security.validate_path(path)?;

        // Verify signature if enabled
        self.security.verify_signature(path)?;

        // Load the library
        let library = unsafe {
            Library::new(path)
                .map_err(|e| PluginError::LoadFailed(format!("Failed to load library: {}", e)))?
        };

        // Get the plugin descriptor
        let descriptor: Symbol<PluginDescriptor> = unsafe {
            library
                .get(b"neotalk_plugin_descriptor")
                .map_err(|e| PluginError::LoadFailed(format!("Missing plugin descriptor: {}", e)))?
        };

        let descriptor = unsafe { &*descriptor };

        // Validate the descriptor
        self.security.validate_descriptor(descriptor)
            .map_err(|e| PluginError::InvalidPlugin(format!("Descriptor validation failed: {}", e)))?;

        // Parse the descriptor
        let parsed = unsafe {
            ParsedPluginDescriptor::from_raw(descriptor)
                .map_err(|e| PluginError::InvalidPlugin(format!("Invalid descriptor: {}", e)))?
        };

        // Create the wrapper
        let wrapper = DynamicPluginWrapper::new(parsed.clone())?;

        // Track the loaded plugin
        self.loaded_plugins.push(LoadedPlugin {
            descriptor: parsed,
            path: path.to_path_buf(),
            loaded_at: chrono::Utc::now(),
        });

        Ok((wrapper, library))
    }

    /// Discover and load all plugins from search paths.
    pub fn discover(&mut self) -> Vec<LoadedPlugin> {
        let mut discovered = Vec::new();

        for search_path in &self.search_paths {
            if !search_path.exists() {
                continue;
            }

            // Read directory entries
            if let Ok(entries) = std::fs::read_dir(search_path) {
                for entry in entries.flatten() {
                    let path = entry.path();

                    // Check if it's a plugin file
                    if self.is_plugin_file(&path) {
                        match self.peek(&path) {
                            Ok(descriptor) => {
                                discovered.push(LoadedPlugin {
                                    descriptor,
                                    path: path.clone(),
                                    loaded_at: chrono::Utc::now(),
                                });
                                tracing::info!("Discovered plugin: {}", path.display());
                            }
                            Err(e) => {
                                tracing::warn!("Failed to peek plugin {:?}: {}", path, e);
                            }
                        }
                    }
                }
            }
        }

        discovered
    }

    /// Reload a previously loaded plugin.
    pub fn reload(&mut self, path: &Path) -> Result<(DynamicPluginWrapper, Library)> {
        // Unload existing plugin with the same path
        self.loaded_plugins.retain(|p| &p.path != path);
        self.load_from_path(path)
    }

    /// Unload a plugin by path.
    pub fn unload(&mut self, path: &Path) -> Result<()> {
        let original_len = self.loaded_plugins.len();
        self.loaded_plugins.retain(|p| &p.path != path);

        if self.loaded_plugins.len() < original_len {
            Ok(())
        } else {
            Err(PluginError::NotFound(format!("Plugin at {:?}", path)))
        }
    }

    /// Check if a file is a plugin library based on extension.
    fn is_plugin_file(&self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str());
        match std::env::consts::OS {
            "macos" => ext == Some("dylib"),
            "linux" => ext == Some("so"),
            "windows" => ext == Some("dll"),
            _ => false,
        }
    }

    /// Get info about a plugin without loading it.
    pub fn peek(&self, path: &Path) -> Result<ParsedPluginDescriptor> {
        // Validate path
        self.security.validate_path(path)?;

        // Load the library temporarily
        let library = unsafe {
            Library::new(path)
                .map_err(|e| PluginError::LoadFailed(format!("Failed to load library: {}", e)))?
        };

        // Get the descriptor
        let descriptor: Symbol<PluginDescriptor> = unsafe {
            library
                .get(b"neotalk_plugin_descriptor")
                .map_err(|e| PluginError::LoadFailed(format!("Missing descriptor: {}", e)))?
        };

        let descriptor = unsafe { &*descriptor };

        // Validate and parse
        self.security.validate_descriptor(descriptor)
            .map_err(|e| PluginError::InvalidPlugin(format!("Descriptor validation failed: {}", e)))?;

        unsafe {
            ParsedPluginDescriptor::from_raw(descriptor)
                .map_err(|e| PluginError::InvalidPlugin(format!("Invalid descriptor: {}", e)))
        }
    }
}

impl Default for DynamicPluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = DynamicPluginLoader::new();
        assert!(!loader.search_paths.is_empty());
    }

    #[test]
    fn test_is_plugin_file() {
        let loader = DynamicPluginLoader::new();

        #[cfg(target_os = "macos")]
        {
            assert!(loader.is_plugin_file(Path::new("test.dylib")));
            assert!(!loader.is_plugin_file(Path::new("test.so")));
        }

        #[cfg(target_os = "linux")]
        {
            assert!(loader.is_plugin_file(Path::new("test.so")));
            assert!(!loader.is_plugin_file(Path::new("test.dylib")));
        }

        #[cfg(windows)]
        {
            assert!(loader.is_plugin_file(Path::new("test.dll")));
            assert!(!loader.is_plugin_file(Path::new("test.so")));
        }
    }
}
