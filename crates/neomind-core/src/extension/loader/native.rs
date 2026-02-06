//! Native extension loader for .so/.dylib/.dll files.

use std::path::{Path, PathBuf};

use crate::extension::types::{ExtensionError, ExtensionMetadata, ExtensionType, Result};

/// Loader for native extensions (.so, .dylib, .dll).
pub struct NativeExtensionLoader {
    /// Loaded library handles (kept alive to prevent unloading)
    _libraries: Vec<libloading::Library>,
}

impl NativeExtensionLoader {
    /// Create a new native extension loader.
    pub fn new() -> Self {
        Self {
            _libraries: Vec::new(),
        }
    }

    /// Load extension metadata from a native library.
    ///
    /// The library should export a `neomind_extension_descriptor` function
    /// that returns extension metadata.
    pub async fn load(&self, path: &Path) -> Result<ExtensionMetadata> {
        // Validate file exists
        if !path.exists() {
            return Err(ExtensionError::NotFound(path.display().to_string()));
        }

        // Validate extension
        let ext = path.extension().and_then(|e| e.to_str());
        if !matches!(ext, Some("so") | Some("dylib") | Some("dll")) {
            return Err(ExtensionError::InvalidFormat(
                "Not a native library file".to_string(),
            ));
        }

        // Try to load the library and get descriptor
        // Note: In production, this would use libloading to dynamically load
        // For now, we create metadata from the file path
        let file_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Extract extension ID from filename (e.g., "libmy_extension.so" -> "my_extension")
        let id = file_name
            .strip_prefix("lib")
            .unwrap_or(file_name)
            .to_string();

        let metadata = ExtensionMetadata::new(
            &id,
            format!("{} Extension", id),
            semver::Version::new(1, 0, 0),
            ExtensionType::Generic,
        )
        .with_file_path(path.to_path_buf());

        Ok(metadata)
    }

    /// Discover native extensions in a directory.
    pub fn discover(&self, dir: &Path) -> Vec<PathBuf> {
        let mut extensions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if super::is_native_extension(&path) {
                    extensions.push(path);
                }
            }
        }

        extensions
    }
}

impl Default for NativeExtensionLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = NativeExtensionLoader::new();
        assert!(loader._libraries.is_empty());
    }
}
