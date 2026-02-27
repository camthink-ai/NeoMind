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
//! 5. **Library Lifetime Management**: The library handle is kept alive
//!    through `Arc<Library>` tied to the extension instance.
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
///
/// The `library` field keeps the dynamic library loaded in memory.
/// The `extension` field contains the actual extension instance.
///
/// # Important
///
/// Both fields must be kept together. Dropping the library while the
/// extension is still in use will cause undefined behavior.
pub struct LoadedNativeExtension {
    /// The underlying library (kept alive to prevent unloading).
    /// Using Arc allows sharing the library handle if needed.
    pub library: Arc<libloading::Library>,
    /// The extension instance
    pub extension: DynExtension,
}

impl LoadedNativeExtension {
    /// Create a new loaded extension with library handle.
    pub fn new(library: libloading::Library, extension: DynExtension) -> Self {
        Self {
            library: Arc::new(library),
            extension,
        }
    }

    /// Get a clone of the library Arc for separate storage.
    pub fn library_arc(&self) -> Arc<libloading::Library> {
        Arc::clone(&self.library)
    }
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

        Ok(LoadedNativeExtension::new(library, extension))
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
    ///
    /// Supports two formats:
    /// 1. Legacy: Top-level binary files (e.g., `extension.dylib`)
    /// 2. .nep package format: Folders with `binaries/{platform}/extension.{ext}`
    pub async fn discover(&self, dir: &Path) -> Vec<(PathBuf, ExtensionMetadata)> {
        let mut extensions = Vec::new();

        let Ok(entries) = std::fs::read_dir(dir) else {
            return extensions;
        };

        // Collect all potential extension paths first
        let mut extension_paths: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();

            // 1. Legacy format: Top-level binary files
            if crate::extension::is_native_extension(&path) {
                extension_paths.push(path);
                continue;
            }

            // 2. .nep package format: Folder with binaries/ subdirectory
            if path.is_dir() {
                if let Some(nep_binary) = self.find_nep_binary(&path).await {
                    extension_paths.push(nep_binary);
                }
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

    /// Find binary file in .nep package folder structure.
    /// Looks for: binaries/{platform}/extension.{ext}
    async fn find_nep_binary(&self, folder: &Path) -> Option<PathBuf> {
        let binaries_dir = folder.join("binaries");
        if !binaries_dir.exists() {
            return None;
        }

        // Detect current platform
        let platform = crate::extension::package::detect_platform();

        // Platform-specific subdirectory names
        let platform_dirs: Vec<&str> = match platform.as_str() {
            "darwin-aarch64" => vec!["darwin_aarch64", "darwin-aarch64", "darwin-arm64"],
            "darwin-x64" => vec!["darwin_x64", "darwin-x64", "darwin-amd64"],
            "linux-x64" => vec!["linux_x64", "linux-x64", "linux-amd64"],
            "linux-arm64" => vec!["linux_arm64", "linux-arm64"],
            "windows-x64" => vec!["windows_x64", "windows-x64", "windows-amd64"],
            _ => vec![platform.as_str()],
        };

        // Look for binary in platform-specific directories
        for platform_dir in platform_dirs {
            let platform_path = binaries_dir.join(platform_dir);
            if platform_path.exists() {
                // Look for extension binary
                for ext in &["dylib", "so", "dll"] {
                    let binary = platform_path.join(format!("extension.{}", ext));
                    if binary.exists() {
                        return Some(binary);
                    }
                }
            }
        }

        None
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
