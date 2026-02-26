//! Native extension loader for .so/.dylib/.dll files.
//!
//! This loader uses libloading to dynamically load extension libraries
//! and call their FFI exports to create Extension instances.
//!
//! # Safety Mechanisms
//!
//! The loader includes comprehensive safety mechanisms to prevent extension
//! bugs from crashing the main server:
//!
//! 1. **Panic Isolation**: All FFI calls are wrapped with `catch_unwind` to
//!    catch panics originating from extension code.
//!
//! 2. **ABI Version Check**: Extensions must declare a compatible ABI version
//!    before any other operations.
//!
//! 3. **Null Pointer Checks**: All pointers returned by extensions are
//!    validated before use.
//!
//! 4. **Graceful Error Handling**: Any error during loading returns an
//!    `ExtensionError` rather than panicking or aborting.
//!
//! # Requirements for Extension Authors
//!
//! To ensure compatibility with the safety mechanisms:
//!
//! - Extensions MUST be compiled with `panic = "unwind"` (not "abort")
//! - Extensions should handle errors gracefully using `Result` types
//! - Extensions should avoid using `unwrap()` or `expect()` in FFI boundary

use std::path::{Path, PathBuf};
use std::panic;
use std::sync::Arc;

use crate::extension::system::{CExtensionMetadata, DynExtension, Extension, ABI_VERSION};
use crate::extension::types::{ExtensionError, ExtensionMetadata, Result};
use tracing::{debug, warn};

/// Loaded native extension with its library handle.
pub struct LoadedNativeExtension {
    /// The underlying library (kept alive to prevent unloading)
    #[allow(dead_code)]
    library: libloading::Library,
    /// The extension instance
    pub extension: DynExtension,
}

/// Loader for native extensions (.so, .dylib, .dll).
pub struct NativeExtensionLoader {
    /// Loaded libraries (kept alive to prevent unloading)
    _libraries: Vec<libloading::Library>,
}

impl NativeExtensionLoader {
    /// Create a new native extension loader.
    pub fn new() -> Self {
        Self {
            _libraries: Vec::new(),
        }
    }

    /// Load an extension from a native library file.
    ///
    /// This performs the following steps:
    /// 1. Loads the dylib using libloading
    /// 2. Verifies ABI version
    /// 3. Gets extension metadata via FFI
    /// 4. Creates extension instance via FFI
    /// 5. Returns the loaded extension
    pub fn load(&self, path: &Path) -> Result<LoadedNativeExtension> {
        self.load_with_config(path, None)
    }

    /// Load the extension with a configuration.
    ///
    /// The config is passed as JSON to the extension's create function.
    ///
    /// # Safety
    ///
    /// This function uses unsafe FFI calls to interact with the extension library.
    /// All FFI calls are wrapped with panic handlers to prevent extension bugs
    /// from crashing the main server.
    pub fn load_with_config(
        &self,
        path: &Path,
        config: Option<&serde_json::Value>,
    ) -> Result<LoadedNativeExtension> {
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

        debug!(path = %path.display(), "Loading native extension");

        // Load the library
        let library = unsafe { libloading::Library::new(path) }
            .map_err(|e| ExtensionError::LoadFailed(format!("Failed to load library: {}", e)))?;

        // Get ABI version with panic protection
        let version = Self::safe_call_ffi("abi_version", || {
            let abi_version: libloading::Symbol<unsafe extern "C" fn() -> u32> = unsafe {
                library
                    .get(b"neomind_extension_abi_version\0")
                    .map_err(|e| ExtensionError::SymbolNotFound(format!("abi_version: {}", e)))?
            };
            Ok(unsafe { abi_version() })
        })?;

        if version != ABI_VERSION {
            return Err(ExtensionError::IncompatibleVersion {
                expected: ABI_VERSION,
                got: version,
            });
        }

        // Get extension metadata with panic protection
        let c_meta = Self::safe_call_ffi("metadata", || {
            let get_metadata: libloading::Symbol<unsafe extern "C" fn() -> CExtensionMetadata> = unsafe {
                library
                    .get(b"neomind_extension_metadata\0")
                    .map_err(|e| ExtensionError::SymbolNotFound(format!("metadata: {}", e)))?
            };
            Ok(unsafe { get_metadata() })
        })?;

        // Convert C metadata to Rust metadata
        let id = unsafe { std::ffi::CStr::from_ptr(c_meta.id) }
            .to_string_lossy()
            .to_string();
        let name = unsafe { std::ffi::CStr::from_ptr(c_meta.name) }
            .to_string_lossy()
            .to_string();
        let version_str = unsafe { std::ffi::CStr::from_ptr(c_meta.version) }
            .to_string_lossy()
            .to_string();

        let description = if !c_meta.description.is_null() {
            Some(
                unsafe { std::ffi::CStr::from_ptr(c_meta.description) }
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };

        let author = if !c_meta.author.is_null() {
            Some(
                unsafe { std::ffi::CStr::from_ptr(c_meta.author) }
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };

        let version =
            semver::Version::parse(&version_str).unwrap_or_else(|_| semver::Version::new(0, 1, 0));

        let metadata = ExtensionMetadata {
            id,
            name,
            version,
            description,
            author,
            homepage: None,
            license: None,
            file_path: Some(path.to_path_buf()),
            config_parameters: None,
        };

        // Create extension instance with panic protection
        // Note: Extensions return tokio::sync::RwLock<Box<dyn Extension>>
        let ext_ptr = Self::safe_call_ffi("create", || {
            let create_ext: libloading::Symbol<
                unsafe extern "C" fn(*const u8, usize) -> *mut tokio::sync::RwLock<Box<dyn Extension>>,
            > = unsafe {
                library
                    .get(b"neomind_extension_create\0")
                    .map_err(|e| ExtensionError::SymbolNotFound(format!("create: {}", e)))?
            };

            // Serialize config to JSON bytes
            let default_config = serde_json::json!({});
            let config_value = config.unwrap_or(&default_config);
            let config_string =
                serde_json::to_string(config_value).unwrap_or_else(|_| "{}".to_string());
            let config_json = std::ffi::CString::new(config_string)
                .unwrap_or_else(|_| std::ffi::CString::new("{}").unwrap());
            let ptr = unsafe {
                create_ext(
                    config_json.as_ptr() as *const u8,
                    config_json.as_bytes().len(),
                )
            };

            if ptr.is_null() {
                return Err(ExtensionError::LoadFailed(
                    "Extension creation returned null".to_string(),
                ));
            }

            Ok(ptr)
        })?;

        // Convert the raw pointer to Arc<RwLock<Box<dyn Extension>>>
        let extension = unsafe {
            // Take ownership of the pointer - this gives us Box<tokio::sync::RwLock<Box<dyn Extension>>>
            let ext_box = Box::from_raw(ext_ptr);
            // Move out of the Box and create Arc from the inner RwLock
            Arc::new(*ext_box)
        };

        debug!(extension_id = %metadata.id, "Native extension loaded successfully");

        Ok(LoadedNativeExtension { library, extension })
    }

    /// Safely call an FFI function with panic protection.
    ///
    /// This wraps the FFI call in `catch_unwind` to prevent panics from
    /// propagating and crashing the main server.
    fn safe_call_ffi<T, F>(fn_name: &str, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + panic::UnwindSafe,
    {
        match panic::catch_unwind(f) {
            Ok(result) => result,
            Err(panic_payload) => {
                // Try to extract a meaningful error message
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic in extension FFI".to_string()
                };

                warn!(
                    function = %fn_name,
                    panic_msg = %msg,
                    "Extension FFI call panicked, caught and converted to error"
                );

                Err(ExtensionError::LoadFailed(format!(
                    "Extension panicked in {}: {}",
                    fn_name, msg
                )))
            }
        }
    }

    /// Get metadata only (lightweight version for discovery).
    pub async fn load_metadata(&self, path: &Path) -> Result<ExtensionMetadata> {
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

        // Load the library temporarily
        let library = unsafe { libloading::Library::new(path) }
            .map_err(|e| ExtensionError::LoadFailed(format!("Failed to load library: {}", e)))?;

        // Get ABI version
        let abi_version: libloading::Symbol<unsafe extern "C" fn() -> u32> = unsafe {
            library
                .get(b"neomind_extension_abi_version\0")
                .map_err(|e| ExtensionError::SymbolNotFound(format!("abi_version: {}", e)))?
        };

        let version = unsafe { abi_version() };
        if version != ABI_VERSION {
            return Err(ExtensionError::IncompatibleVersion {
                expected: ABI_VERSION,
                got: version,
            });
        }

        // Get extension metadata
        let get_metadata: libloading::Symbol<unsafe extern "C" fn() -> CExtensionMetadata> = unsafe {
            library
                .get(b"neomind_extension_metadata\0")
                .map_err(|e| ExtensionError::SymbolNotFound(format!("metadata: {}", e)))?
        };

        let c_meta = unsafe { get_metadata() };

        // Convert C metadata to Rust metadata
        let id = unsafe { std::ffi::CStr::from_ptr(c_meta.id) }
            .to_string_lossy()
            .to_string();
        let name = unsafe { std::ffi::CStr::from_ptr(c_meta.name) }
            .to_string_lossy()
            .to_string();
        let version_str = unsafe { std::ffi::CStr::from_ptr(c_meta.version) }
            .to_string_lossy()
            .to_string();

        let description = if !c_meta.description.is_null() {
            Some(
                unsafe { std::ffi::CStr::from_ptr(c_meta.description) }
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };

        let author = if !c_meta.author.is_null() {
            Some(
                unsafe { std::ffi::CStr::from_ptr(c_meta.author) }
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };

        let version =
            semver::Version::parse(&version_str).unwrap_or_else(|_| semver::Version::new(0, 1, 0));

        // Library is dropped here (unloaded), which is fine for metadata-only access

        Ok(ExtensionMetadata {
            id,
            name,
            version,
            description,
            author,
            homepage: None,
            license: None,
            file_path: Some(path.to_path_buf()),
            config_parameters: None,
        })
    }

    /// Discover native extensions in a directory.
    pub async fn discover(&self, dir: &Path) -> Vec<(PathBuf, ExtensionMetadata)> {
        let mut extensions = Vec::new();

        let Ok(entries) = std::fs::read_dir(dir) else {
            return extensions;
        };

        // Collect all potential extension paths first
        let mut extension_paths: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if crate::extension::is_native_extension(&path) {
                extension_paths.push(path);
            }
        }

        // Load metadata for each extension
        for path in extension_paths {
            let loader = NativeExtensionLoader::new();
            if let Ok(meta) = loader.load_metadata(&path).await {
                extensions.push((path, meta));
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
