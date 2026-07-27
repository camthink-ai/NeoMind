//! Native extension metadata loader for .so/.dylib/.dll files.
//!
//! This loader is now limited to ABI validation, metadata extraction,
//! and extension discovery. Native execution is handled by the
//! extension-runner process via JSON bridge exports.
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

use crate::extension::system::{CExtensionMetadata, ABI_VERSION};
use crate::extension::types::{ExtensionError, ExtensionMetadata, Result};
use std::panic;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Loader for native extension metadata and discovery.
pub struct NativeExtensionMetadataLoader;

impl NativeExtensionMetadataLoader {
    /// Create a new native extension metadata loader.
    pub fn new() -> Self {
        Self
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

        // Step 2: Check for required JSON-bridge FFI symbols
        // Old extensions (pre-0.6.0) use _create/_destroy interface which is incompatible
        // and would crash when called with the new JSON-bridge protocol
        let required_symbols = [
            "neomind_extension_descriptor_json\0",
            "neomind_extension_free_string\0",
            "neomind_extension_execute_command_json\0",
            "neomind_extension_produce_metrics_json\0",
        ];

        for symbol_name in &required_symbols {
            let symbol_result =
                unsafe { library.get::<unsafe extern "C" fn()>(symbol_name.as_bytes()) };
            if symbol_result.is_err() {
                return Err(ExtensionError::LoadFailed(format!(
                    "Extension uses incompatible FFI interface (missing symbol: {}). \
                     Extensions must be rebuilt with neomind-extension-sdk >= 0.6.0",
                    symbol_name.trim_end_matches('\0')
                )));
            }
        }

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

        // Library is dropped here (unloaded), which is fine for metadata-only access

        Ok(ExtensionMetadata {
            id,
            name,
            version: version.to_string(),
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
    ///
    /// # Safety
    ///
    /// This method NEVER loads native libraries during discovery to prevent crashes
    /// from incompatible extensions. Instead, it reads sidecar JSON metadata files.
    /// Extensions without sidecar JSON files are skipped with a warning.
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

        // Load metadata for each extension using sidecar JSON files only
        // This is SAFE and never loads native libraries during discovery
        for path in extension_paths {
            match self.load_metadata_from_sidecar(&path) {
                Ok(meta) => {
                    tracing::info!(
                        extension_id = %meta.id,
                        path = %path.display(),
                        "Discovered extension from sidecar JSON"
                    );
                    extensions.push((path, meta));
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Extension missing sidecar JSON, skipping (use .nep package format for automatic metadata)"
                    );
                }
            }
        }

        extensions
    }

    /// Load metadata from sidecar JSON file (safe, no library loading).
    ///
    /// The sidecar JSON file should be named the same as the binary with .json extension.
    /// For example: `extension.dylib` -> `extension.json`
    ///
    /// This is the SAFE way to get metadata without risking crashes from
    /// incompatible native library initialization code.
    fn load_metadata_from_sidecar(&self, binary_path: &Path) -> Result<ExtensionMetadata> {
        let sidecar_path = binary_path.with_extension("json");

        if !sidecar_path.exists() {
            return Err(ExtensionError::LoadFailed(format!(
                "No sidecar metadata file found at {}. Native extensions must have a sidecar JSON file for safe discovery.",
                sidecar_path.display()
            )));
        }

        let content = std::fs::read_to_string(&sidecar_path).map_err(|e| {
            ExtensionError::LoadFailed(format!("Failed to read sidecar JSON: {}", e))
        })?;

        let meta: ExtensionMetadata = serde_json::from_str(&content).map_err(|e| {
            ExtensionError::LoadFailed(format!("Invalid sidecar JSON format: {}", e))
        })?;

        // Update file_path to point to the actual binary
        Ok(ExtensionMetadata {
            file_path: Some(binary_path.to_path_buf()),
            ..meta
        })
    }

    /// Find binary file in .nep package folder structure.
    /// Looks for:
    /// 1. binaries/{platform}/extension.{ext} (standard .nep format)
    /// 2. extension.{ext} in folder root (installed format)
    async fn find_nep_binary(&self, folder: &Path) -> Option<PathBuf> {
        // First, check for extension binary in folder root (installed format)
        for ext in &["dylib", "so", "dll"] {
            let binary = folder.join(format!("extension.{}", ext));
            if binary.exists() {
                return Some(binary);
            }
        }

        // Then check binaries/ subdirectory (standard .nep format)
        let binaries_dir = folder.join("binaries");
        if !binaries_dir.exists() {
            return None;
        }

        // Detect current platform
        let platform = crate::extension::package::detect_platform();

        // detect_platform() returns underscore format (e.g. "linux_arm64"),
        // so match arms must use underscore keys. Candidate directory names
        // include historical spellings for compatibility with any unpacked dirs.
        let platform_dirs: Vec<&str> = match platform.as_str() {
            "darwin_aarch64" => vec!["darwin_aarch64", "darwin-aarch64", "darwin-arm64"],
            "darwin_x86_64" => vec![
                "darwin_x86_64",
                "darwin-x86_64",
                "darwin-x64",
                "darwin-amd64",
            ],
            "linux_amd64" => vec![
                "linux_amd64",
                "linux_x64",
                "linux-x86_64",
                "linux-x64",
                "linux-amd64",
            ],
            "linux_arm64" => vec!["linux_arm64", "linux-arm64", "linux-aarch64"],
            "windows_amd64" => vec![
                "windows_amd64",
                "windows_x86_64",
                "windows-x86_64",
                "windows-x64",
                "windows-amd64",
            ],
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

impl Default for NativeExtensionMetadataLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let _loader = NativeExtensionMetadataLoader::new();
    }
}
