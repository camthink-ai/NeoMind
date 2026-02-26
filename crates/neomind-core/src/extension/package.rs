//! NeoMind Extension Package (.nep) parser and installer
//!
//! A .nep (NeoMind Extension Package) is a ZIP archive containing:
//! - manifest.json - Extension metadata
//! - binaries/ - Platform-specific extension binaries
//! - frontend/ - Frontend components and assets
//!
//! # Package Structure
//!
//! ```text
//! {extension-id}-{version}.nep
//! ├── manifest.json
//! ├── binaries/
//! │   ├── darwin_aarch64/
//! │   │   └── extension.dylib
//! │   ├── darwin_x86_64/
//! │   │   └── extension.dylib
//! │   ├── linux_amd64/
//! │   │   └── extension.so
//! │   ├── windows_amd64/
//! │   │   └── extension.dll
//! │   └── wasm/
//! │       ├── extension.wasm
//! │       └── extension.json
//! └── frontend/
//!     ├── dist/
//!     │   ├── bundle.js
//!     │   └── bundle.css
//!     └── assets/
//!         └── icons/
//! ```

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::extension::types::ExtensionError;

/// Extension package format identifier
pub const PACKAGE_FORMAT: &str = "neomind-extension-package";
/// Current package format version
pub const PACKAGE_FORMAT_VERSION: &str = "1.0";

/// Parsed extension package
#[derive(Debug, Clone)]
pub struct ExtensionPackage {
    /// Package file path (if loaded from file)
    pub path: Option<PathBuf>,
    /// Parsed manifest
    pub manifest: ExtensionPackageManifest,
    /// Package file SHA256 checksum
    pub checksum: String,
    /// Package file size in bytes
    pub size: u64,
}

/// Extension package manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionPackageManifest {
    /// Package format identifier
    pub format: String,
    /// Package format version
    pub format_version: String,

    /// Extension metadata
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// Minimum NeoMind version required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neomind: Option<NeomindRequirements>,

    /// Binary files for different platforms
    #[serde(default)]
    pub binaries: HashMap<String, String>,

    /// Frontend components
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontend: Option<FrontendConfig>,

    /// Extension capabilities (metrics, commands)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,

    /// Permissions required
    #[serde(default)]
    pub permissions: Vec<String>,

    /// Extension type (native, wasm, frontend-only)
    #[serde(default = "default_extension_type")]
    #[serde(rename = "type")]
    pub extension_type: String,
}

fn default_extension_type() -> String {
    "native".to_string()
}

/// NeoMind compatibility requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeomindRequirements {
    /// Minimum NeoMind version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
}

/// Frontend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfig {
    /// Dashboard components provided by this extension
    #[serde(default)]
    pub components: Vec<DashboardComponentDef>,
}

/// Dashboard component definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponentDef {
    /// Component type ID
    #[serde(rename = "type")]
    pub component_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Component category
    pub category: String,
    /// Icon name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Path to bundled JS file (relative to frontend/)
    pub bundle_path: String,
    /// Exported component name
    pub export_name: String,
    /// Size constraints
    #[serde(default)]
    pub size_constraints: SizeConstraints,
    /// Has data source configuration
    #[serde(default)]
    pub has_data_source: bool,
    /// Has display configuration
    #[serde(default)]
    pub has_display_config: bool,
    /// Has actions configuration
    #[serde(default)]
    pub has_actions: bool,
    /// Maximum data sources
    #[serde(default)]
    pub max_data_sources: usize,
    /// Configuration schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    /// Data source schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_schema: Option<serde_json::Value>,
    /// Default configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_config: Option<serde_json::Value>,
    /// Component variants
    #[serde(default)]
    pub variants: Vec<String>,
    /// Data binding configuration
    #[serde(default)]
    pub data_binding: DataBindingConfig,
}

/// Size constraints for dashboard components
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SizeConstraints {
    #[serde(default = "default_min")]
    pub min_w: usize,
    #[serde(default = "default_min")]
    pub min_h: usize,
    #[serde(default = "default_2")]
    pub default_w: usize,
    #[serde(default = "default_4")]
    pub default_h: usize,
    #[serde(default)]
    pub max_w: Option<usize>,
    #[serde(default)]
    pub max_h: Option<usize>,
    #[serde(default)]
    pub preserve_aspect: bool,
}

fn default_min() -> usize {
    1
}

fn default_2() -> usize {
    2
}

fn default_4() -> usize {
    4
}

/// Data binding configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataBindingConfig {
    /// Extension metric to bind to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_metric: Option<String>,
    /// Extension command to execute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_command: Option<String>,
    /// Required fields from command result
    #[serde(default)]
    pub required_fields: Vec<String>,
}

/// Extension capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub metrics: Vec<MetricDescriptor>,
    #[serde(default)]
    pub commands: Vec<CommandDescriptor>,
}

/// Metric descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDescriptor {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// Command descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptor {
    pub name: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// Installation result
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Extension ID
    pub extension_id: String,
    /// Installed version
    pub version: String,
    /// Binary file path
    pub binary_path: PathBuf,
    /// Manifest file path
    pub manifest_path: PathBuf,
    /// Frontend directory (if any)
    pub frontend_dir: Option<PathBuf>,
    /// Installed dashboard components
    pub components: Vec<DashboardComponentDef>,
    /// Package checksum
    pub checksum: String,
}

/// Extension package error
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Invalid package format: {0}")]
    InvalidFormat(String),

    #[error("Missing required file: {0}")]
    MissingFile(String),

    #[error("Unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Incompatible version: required {required}, got {got}")]
    IncompatibleVersion { required: String, got: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<PackageError> for ExtensionError {
    fn from(err: PackageError) -> Self {
        ExtensionError::LoadFailed(err.to_string())
    }
}

impl ExtensionPackage {
    /// Load a package from a file
    pub async fn load(path: &Path) -> Result<Self, PackageError> {
        // Read file
        let mut file = tokio::fs::File::open(path).await?;
        let metadata = file.metadata().await?;
        let size = metadata.len();

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        // Calculate checksum
        let checksum = Self::calculate_checksum(&buffer);

        // Parse ZIP archive
        let cursor = Cursor::new(buffer);
        let mut archive = ZipArchive::new(cursor).map_err(|e| PackageError::Zip(e.to_string()))?;

        // Read manifest.json
        let manifest_content = Self::read_file_from_zip(&mut archive, "manifest.json")?;
        let manifest: ExtensionPackageManifest = serde_json::from_str(&manifest_content)?;

        // Validate manifest
        Self::validate_manifest(&manifest)?;

        Ok(Self {
            path: Some(path.to_path_buf()),
            manifest,
            checksum,
            size,
        })
    }

    /// Load a package from bytes
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PackageError> {
        let checksum = Self::calculate_checksum(&data);
        let size = data.len() as u64;

        let cursor = Cursor::new(data);
        let mut archive = ZipArchive::new(cursor).map_err(|e| PackageError::Zip(e.to_string()))?;

        let manifest_content = Self::read_file_from_zip(&mut archive, "manifest.json")?;
        let manifest: ExtensionPackageManifest = serde_json::from_str(&manifest_content)?;

        Self::validate_manifest(&manifest)?;

        Ok(Self {
            path: None,
            manifest,
            checksum,
            size,
        })
    }

    /// Validate the manifest
    fn validate_manifest(manifest: &ExtensionPackageManifest) -> Result<(), PackageError> {
        if manifest.format != PACKAGE_FORMAT {
            return Err(PackageError::InvalidFormat(format!(
                "Expected format '{}', got '{}'",
                PACKAGE_FORMAT, manifest.format
            )));
        }

        // Check format version compatibility
        if manifest.format_version != PACKAGE_FORMAT_VERSION {
            return Err(PackageError::IncompatibleVersion {
                required: PACKAGE_FORMAT_VERSION.to_string(),
                got: manifest.format_version.clone(),
            });
        }

        // Validate extension ID
        if manifest.id.is_empty() {
            return Err(PackageError::InvalidManifest("Extension ID is required".to_string()));
        }

        // Validate version
        if manifest.version.is_empty() {
            return Err(PackageError::InvalidManifest("Version is required".to_string()));
        }

        Ok(())
    }

    /// Calculate SHA256 checksum of data
    fn calculate_checksum(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Read a file from the ZIP archive
    fn read_file_from_zip<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        path: &str,
    ) -> Result<String, PackageError> {
        let mut file = archive.by_name(path).map_err(|e| {
            PackageError::MissingFile(format!("{}: {}", path, e))
        })?;

        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(content)
    }

    /// Get the binary path for the current platform
    /// Falls back to wasm if no native binary is available
    pub fn get_binary_path(&self) -> Option<String> {
        let platform = detect_platform();
        // First try native binary for current platform
        if let Some(path) = self.manifest.binaries.get(&platform) {
            return Some(path.clone());
        }
        // Fall back to wasm (universal platform)
        self.manifest.binaries.get("wasm").cloned()
    }

    /// Install the package to a target directory
    pub async fn install(&self, target_dir: &Path) -> Result<InstallResult, PackageError> {
        let ext_id = &self.manifest.id;
        let version = &self.manifest.version;

        // Create extension directory
        let ext_dir = target_dir.join(ext_id);
        tokio::fs::create_dir_all(&ext_dir).await?;

        // Load ZIP archive
        let data = if let Some(path) = &self.path {
            tokio::fs::read(path).await?
        } else {
            return Err(PackageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Package has no file path",
            )));
        };

        let cursor = Cursor::new(data);
        let mut archive = ZipArchive::new(cursor).map_err(|e| PackageError::Zip(e.to_string()))?;

        // Extract manifest.json
        let manifest_path = ext_dir.join("manifest.json");
        self.extract_file(&mut archive, "manifest.json", &manifest_path).await?;

        // Extract binary for current platform
        let binary_path = if let Some(rel_path) = self.get_binary_path() {
            let binary_file = ext_dir.join(
                PathBuf::from(&rel_path).file_name().unwrap_or_default()
            );
            self.extract_file(&mut archive, &rel_path, &binary_file).await?;

            // For WASM binaries, create a sidecar JSON file for the loader
            if binary_file.extension().and_then(|e| e.to_str()) == Some("wasm") {
                let sidecar_json = binary_file.with_extension("json");
                self.create_wasm_sidecar_json(&sidecar_json).await?;
            }

            binary_file
        } else {
            return Err(PackageError::UnsupportedPlatform(detect_platform()));
        };

        // Extract frontend directory if exists
        let frontend_dir = if self.manifest.frontend.is_some() {
            let frontend_path = ext_dir.join("frontend");
            self.extract_directory(&mut archive, "frontend/", &frontend_path).await?;
            Some(frontend_path)
        } else {
            None
        };

        // Get component definitions
        let components = self.manifest.frontend.as_ref()
            .map(|f| f.components.clone())
            .unwrap_or_default();

        Ok(InstallResult {
            extension_id: ext_id.clone(),
            version: version.clone(),
            binary_path,
            manifest_path,
            frontend_dir,
            components,
            checksum: self.checksum.clone(),
        })
    }

    /// Extract a single file from the archive
    async fn extract_file<R: Read + std::io::Seek>(
        &self,
        archive: &mut ZipArchive<R>,
        src_path: &str,
        dst_path: &Path,
    ) -> Result<(), PackageError> {
        let mut file = archive.by_name(src_path).map_err(|e| {
            PackageError::MissingFile(format!("{}: {}", src_path, e))
        })?;

        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        // Create parent directory
        if let Some(parent) = dst_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(dst_path, content).await?;
        Ok(())
    }

    /// Extract a directory from the archive
    async fn extract_directory<R: Read + std::io::Seek>(
        &self,
        archive: &mut ZipArchive<R>,
        src_prefix: &str,
        dst_dir: &Path,
    ) -> Result<(), PackageError> {
        tokio::fs::create_dir_all(dst_dir).await?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                PackageError::Zip(format!("Failed to access file {}: {}", i, e))
            })?;

            let name = file.name().to_string();

            // Check if file starts with prefix
            if name.starts_with(src_prefix) && !name.ends_with('/') {
                // Remove prefix to get relative path
                let rel_path = name[src_prefix.len()..].to_string();
                let dst_path = dst_dir.join(&rel_path);

                // Create parent directory
                if let Some(parent) = dst_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                // Extract file
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;
                tokio::fs::write(dst_path, content).await?;
            }
        }

        Ok(())
    }

    /// Create a sidecar JSON file for WASM extensions
    /// This allows the WASM loader to find the metadata
    async fn create_wasm_sidecar_json(&self, json_path: &Path) -> Result<(), PackageError> {
        use serde_json::json;

        let sidecar_data = json!({
            "id": self.manifest.id,
            "name": self.manifest.name,
            "version": self.manifest.version,
            "description": self.manifest.description,
            "author": self.manifest.author,
            "homepage": self.manifest.homepage,
            "license": self.manifest.license,
            "file_path": self.path,
        });

        let content = serde_json::to_string_pretty(&sidecar_data)
            .map_err(|e| PackageError::InvalidManifest(format!("Failed to serialize sidecar JSON: {}", e)))?;

        tokio::fs::write(json_path, content).await
            .map_err(|e| PackageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to write sidecar JSON: {}", e)
            )))?;

        Ok(())
    }

    /// Uninstall an extension (remove its directory)
    pub async fn uninstall(install_dir: &Path, extension_id: &str) -> Result<(), PackageError> {
        let ext_dir = install_dir.join(extension_id);

        if ext_dir.exists() {
            tokio::fs::remove_dir_all(&ext_dir).await?;
        }

        Ok(())
    }
}

/// Detect the current platform
pub fn detect_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("macos", "aarch64") => "darwin_aarch64".to_string(),
        ("macos", "x86_64") => "darwin_x86_64".to_string(),
        ("linux", "x86_64") => "linux_amd64".to_string(),
        ("linux", "aarch64") => "linux_arm64".to_string(),
        ("windows", "x86_64") => "windows_amd64".to_string(),
        _ => format!("{}_{}", os, arch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_platform() {
        let platform = detect_platform();
        println!("Detected platform: {}", platform);
        assert!(!platform.is_empty());
    }

    #[test]
    fn test_format_constants() {
        assert_eq!(PACKAGE_FORMAT, "neomind-extension-package");
        assert_eq!(PACKAGE_FORMAT_VERSION, "1.0");
    }
}
