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

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

use serde::{Deserialize, Serialize};

use crate::extension::types::ExtensionError;

/// Extension package format identifier
pub const PACKAGE_FORMAT: &str = "neomind-extension-package";
/// Current supported ABI version (determines package format compatibility)
/// This single version number controls:
/// - Package format compatibility
/// - FFI interface compatibility
/// - Extension loading compatibility
pub const CURRENT_ABI_VERSION: u32 = 3;
/// Minimum supported ABI version
pub const MIN_ABI_VERSION: u32 = 3;

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
    /// Package format version (deprecated, use abi_version)
    /// Still accepted for backward compatibility
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_version: Option<String>,

    /// ABI version - determines binary compatibility
    /// This is the primary version check for extension loading
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,

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
    /// Supports both string (e.g., "frontend/") and struct format for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_frontend_opt", default)]
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

    /// Opt-in env vars the runner injects at spawn, for extensions that
    /// need runtime-specific library paths (e.g. `ORT_DYLIB_PATH` for
    /// load-dynamic ONNX Runtime). Value is a path template with
    /// `{binaries}` (this spawn's platform binaries dir) and
    /// `{extension_dir}` placeholders. A hint applies only when the env var
    /// is unset and the resolved file exists — absent entries are skipped,
    /// so variant builds (jetson/cuda) can ship conditional libraries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_hints: Option<HashMap<String, String>>,
}

fn default_abi_version() -> u32 {
    CURRENT_ABI_VERSION
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FrontendConfig {
    /// Dashboard components provided by this extension
    #[serde(default)]
    pub components: Vec<DashboardComponentDef>,
}

/// Intermediate type for deserializing frontend field that accepts both string and struct formats
/// This provides backward compatibility with older extension packages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrontendField {
    /// String format (e.g., "frontend/") - used in older extension packages
    String(String),
    /// Struct format with components
    Struct(FrontendConfig),
}

impl FrontendField {
    /// Convert to FrontendConfig, handling both formats
    pub fn into_config(self) -> FrontendConfig {
        match self {
            FrontendField::String(_) => FrontendConfig {
                components: Vec::new(),
            },
            FrontendField::Struct(config) => config,
        }
    }
}

/// Helper function to deserialize optional frontend field
fn deserialize_frontend_opt<'de, D>(deserializer: D) -> Result<Option<FrontendConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<FrontendField> = Option::deserialize(deserializer)?;
    Ok(opt.map(|f| f.into_config()))
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
    /// Global variable name for the bundle (used for script tag loading)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_name: Option<String>,
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
    /// Resources directory (if any) - contains models, configs, etc.
    pub resources_dir: Option<PathBuf>,
    /// Models directory (if any) - contains AI model files
    pub models_dir: Option<PathBuf>,
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
    /// Stream-hash a file's contents for the checksum without buffering the
    /// whole file in memory. Used by [`load`](Self::load) so opening a large
    /// `.nep` (e.g. ML model bundle) for upload doesn't OOM.
    async fn stream_hash_file(path: &Path) -> Result<String, PackageError> {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncReadExt;
        let mut f = tokio::fs::File::open(path).await?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Load a package from a file
    pub async fn load(path: &Path) -> Result<Self, PackageError> {
        let size = tokio::fs::metadata(path).await?.len();

        // Stream-hash the file for the checksum — do NOT read_to_end, large
        // packages would OOM.
        let checksum = Self::stream_hash_file(path).await?;

        // Parse the manifest only, via a File-backed archive (no full buffer).
        let std_file = std::fs::File::open(path)?;
        let mut archive =
            ZipArchive::new(std_file).map_err(|e| PackageError::Zip(e.to_string()))?;

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

        // Check ABI version compatibility (primary version check)
        if manifest.abi_version < MIN_ABI_VERSION || manifest.abi_version > CURRENT_ABI_VERSION {
            return Err(PackageError::IncompatibleVersion {
                required: format!("{}-{}", MIN_ABI_VERSION, CURRENT_ABI_VERSION),
                got: manifest.abi_version.to_string(),
            });
        }

        // Validate extension ID
        if manifest.id.is_empty() {
            return Err(PackageError::InvalidManifest(
                "Extension ID is required".to_string(),
            ));
        }
        // Reject path-traversal characters — the id is joined directly into the
        // install path (`target_dir.join(ext_id)`), so an id like
        // "../../etc/cron.d/x" would write files outside the install dir
        // (RCE-equivalent once the marketplace opens to third-party packages).
        if manifest.id.contains("..")
            || manifest.id.contains('/')
            || manifest.id.contains('\\')
            || manifest.id.contains('\0')
        {
            return Err(PackageError::InvalidManifest(format!(
                "Extension ID contains invalid path characters: {:?}",
                manifest.id
            )));
        }

        // Validate version
        if manifest.version.is_empty() {
            return Err(PackageError::InvalidManifest(
                "Version is required".to_string(),
            ));
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
        let mut file = archive
            .by_name(path)
            .map_err(|e| PackageError::MissingFile(format!("{}: {}", path, e)))?;

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

        // Load ZIP archive from file — do NOT tokio::fs::read (would buffer
        // the whole package in memory). Open async, hand the std File to
        // ZipArchive; entries are then streamed by extract_file/extract_directory.
        let path = self.path.clone().ok_or_else(|| {
            PackageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Package has no file path",
            ))
        })?;
        // Open the file synchronously (brief, just the open — not a full read)
        // and hand the std File to ZipArchive. Entries are then streamed by
        // extract_file/extract_directory, so the package is never buffered
        // whole. std File is used because ZipArchive needs std Read+Seek.
        let file = std::fs::File::open(&path)?;
        let mut archive = ZipArchive::new(file).map_err(|e| PackageError::Zip(e.to_string()))?;

        // Extract manifest.json
        let manifest_path = ext_dir.join("manifest.json");
        self.extract_file(&mut archive, "manifest.json", &manifest_path)
            .await?;

        // Extract binary for current platform
        let binary_path = if let Some(rel_path) = self.get_binary_path() {
            let binary_file =
                ext_dir.join(PathBuf::from(&rel_path).file_name().unwrap_or_default());
            self.extract_file(&mut archive, &rel_path, &binary_file)
                .await?;

            // Create sidecar JSON file for all extension types (WASM and native)
            // This allows safe discovery without loading native libraries
            let sidecar_json = binary_file.with_extension("json");
            self.create_sidecar_json(&sidecar_json).await?;

            binary_file
        } else {
            let available_platforms: Vec<String> = self.manifest.binaries.keys().cloned().collect();
            return Err(PackageError::UnsupportedPlatform(format!(
                "{}. Available platforms: {}",
                detect_platform(),
                available_platforms.join(", ")
            )));
        };

        // Extract frontend directory if exists
        let frontend_dir = if self.manifest.frontend.is_some() {
            let frontend_path = ext_dir.join("frontend");
            self.extract_directory(&mut archive, "frontend/", &frontend_path)
                .await?;
            Some(frontend_path)
        } else {
            None
        };

        // ✨ Extract models directory if exists (AI model files)
        let models_path = ext_dir.join("models");
        let models_dir = if self
            .extract_directory(&mut archive, "models/", &models_path)
            .await
            .is_ok()
        {
            Some(models_path)
        } else {
            None
        };

        // ✨ Extract resources directory if exists (configs, assets, etc.)
        let resources_path = ext_dir.join("resources");
        let resources_dir = if self
            .extract_directory(&mut archive, "resources/", &resources_path)
            .await
            .is_ok()
        {
            Some(resources_path)
        } else {
            None
        };

        // 🔧 macOS: Re-sign all extracted dylibs after installation
        #[cfg(target_os = "macos")]
        {
            if let Some(binary_dir) = binary_path.parent() {
                Self::resign_dylibs_macos(binary_dir);
            }
        }

        // Get component definitions
        let components = self
            .manifest
            .frontend
            .as_ref()
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
            resources_dir,
            models_dir,
        })
    }

    /// Install the package synchronously (for use in spawn_blocking)
    /// Takes raw package bytes since from_bytes() doesn't store them
    /// Install the package synchronously (for use in spawn_blocking).
    /// Takes raw package bytes since from_bytes() doesn't store them.
    /// In-memory variant — used by the upload path where the body limit
    /// already bounds the size. The download path should use
    /// [`install_from_file`](Self::install_from_file) to avoid buffering
    /// large packages in memory.
    pub fn install_sync(data: &[u8], target_dir: &Path) -> Result<InstallResult, PackageError> {
        tracing::info!(
            "install_sync: {} bytes, target={}",
            data.len(),
            target_dir.display()
        );
        let checksum = Self::calculate_checksum(data);
        let cursor = Cursor::new(data.to_vec());
        let mut archive = ZipArchive::new(cursor).map_err(|e| PackageError::Zip(e.to_string()))?;
        Self::install_from_archive(&mut archive, target_dir, checksum)
    }

    /// Install a package from a file path — streams the ZIP from the file
    /// instead of buffering it in memory. Used by the marketplace download
    /// path so that large `.nep` packages (ML model bundles + CUDA ORT) do
    /// not OOM. Only the file header + individual entries are held in memory
    /// at a time, never the whole archive.
    pub fn install_from_file(
        data_path: &Path,
        target_dir: &Path,
    ) -> Result<InstallResult, PackageError> {
        tracing::info!(
            "install_from_file: {}, target={}",
            data_path.display(),
            target_dir.display()
        );

        // Stream-hash the file for the checksum (do not buffer it all).
        let checksum = {
            use std::io::Read;
            let mut hasher = Sha256::new();
            let mut f = std::fs::File::open(data_path)?;
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            format!("{:x}", hasher.finalize())
        };

        let file = std::fs::File::open(data_path)?;
        let mut archive = ZipArchive::new(file).map_err(|e| PackageError::Zip(e.to_string()))?;
        Self::install_from_archive(&mut archive, target_dir, checksum)
    }

    /// Core install logic over an already-opened ZIP archive. Generic over
    /// the reader so the in-memory path ([`install_sync`](Self::install_sync),
    /// via `Cursor`) and the file-backed path
    /// ([`install_from_file`](Self::install_from_file), via `File`) share one
    /// implementation. `checksum` is computed by the caller — it requires the
    /// raw package bytes/stream, which the archive abstraction does not expose.
    fn install_from_archive<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        target_dir: &Path,
        checksum: String,
    ) -> Result<InstallResult, PackageError> {
        // Read manifest from archive
        let manifest_content = Self::read_file_from_zip(archive, "manifest.json")?;
        let manifest: ExtensionPackageManifest = serde_json::from_str(&manifest_content)?;

        Self::validate_manifest(&manifest)?;

        let ext_id = &manifest.id;
        let version = &manifest.version;
        tracing::info!("install: manifest OK ({} v{})", ext_id, version);

        // Create extension directory
        let ext_dir = target_dir.join(ext_id);
        std::fs::create_dir_all(&ext_dir)?;

        // Extract manifest.json
        let manifest_path = ext_dir.join("manifest.json");
        let mut budget = InstallBudget::new();
        Self::extract_file_sync(archive, "manifest.json", &manifest_path, &mut budget)?;

        // Get binary path for current platform
        let platform = detect_platform();
        let binary_rel_path = manifest
            .binaries
            .get(&platform)
            .or_else(|| manifest.binaries.get("wasm"))
            .cloned()
            .ok_or_else(|| {
                let available_platforms: Vec<String> = manifest.binaries.keys().cloned().collect();
                tracing::error!(
                    "install FAILED: no binary for platform '{}' (have: {})",
                    platform,
                    available_platforms.join(", ")
                );
                PackageError::UnsupportedPlatform(format!(
                    "{}. Available platforms: {}",
                    platform,
                    available_platforms.join(", ")
                ))
            })?;

        tracing::info!("install: platform {} -> {}", platform, binary_rel_path);

        // Extract binary and preserve directory structure.
        // Route through safe_join_within to reject `..` / absolute paths
        // coming from the manifest — this is the central zip-slip defense
        // (the async install path uses `file_name()` to flatten; this path
        // preserves subdirs for shared libraries, so we must guard instead).
        let binary_file = Self::safe_join_within(&ext_dir, &binary_rel_path)?;
        Self::extract_file_sync(archive, &binary_rel_path, &binary_file, &mut budget)?;
        tracing::info!("install: binary extracted");

        // Extract all sibling files in the same directory as the binary
        // These are bundled shared libraries (e.g. libonnxruntime.dylib)
        if let Some(binary_dir) = std::path::Path::new(&binary_rel_path).parent() {
            let dir_prefix = if binary_dir.as_os_str().is_empty() {
                "".to_string()
            } else {
                format!("{}/", binary_dir.to_string_lossy())
            };
            let dest_dir = ext_dir.join(binary_dir);

            for i in 0..archive.len() {
                if let Ok(mut file) = archive.by_index(i) {
                    let name = file.name().to_string();
                    // Same directory, not the binary itself, not a directory entry
                    if name.starts_with(&dir_prefix)
                        && !name.ends_with('/')
                        && name != binary_rel_path
                        // data/ is extension-private runtime state — packages
                        // must never write there (upgrade-preservation contract)
                        && !name.starts_with("data/")
                        && !name[name.len() - 1..].starts_with('/')
                        && name.matches('/').count() == binary_rel_path.matches('/').count()
                    {
                        if file.is_symlink() {
                            return Err(PackageError::Zip(format!(
                                "symlink entry rejected: '{}'",
                                name
                            )));
                        }
                        if file.size() > Self::MAX_EXTRACT_FILE_SIZE {
                            return Err(PackageError::Zip(format!(
                                "Bundled library '{}' is {} bytes (exceeds limit)",
                                name,
                                file.size()
                            )));
                        }
                        budget.charge(file.size())?;
                        let dest = Self::safe_join_within(
                            &dest_dir,
                            name.trim_start_matches(&dir_prefix),
                        )?;
                        if let Some(parent) = dest.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        // macOS: if the destination already exists (re-install),
                        // code-signed files carry `com.apple.provenance` /
                        // embedded signatures that block File::create with
                        // EACCES. Remove first to clear xattrs + inode.
                        if dest.exists() {
                            let _ = std::fs::remove_file(&dest);
                        }
                        let mut out = std::fs::File::create(&dest)?;
                        std::io::copy(&mut file, &mut out)?;
                        tracing::info!("Extracted bundled library: {}", name);
                    }
                }
            }
        }

        // Create sidecar JSON file for all extension types (WASM and native)
        // This allows safe discovery without loading native libraries
        let sidecar_json = binary_file.with_extension("json");
        Self::create_sidecar_json_sync(&manifest, &sidecar_json)?;
        tracing::info!("install: sidecar json + sibling libs done");

        // Extract frontend directory if exists
        let frontend_dir = if manifest.frontend.is_some() {
            let frontend_path = ext_dir.join("frontend");
            Self::extract_directory_sync(archive, "frontend/", &frontend_path, &mut budget)?;
            Some(frontend_path)
        } else {
            None
        };

        // Extract models directory if exists (for AI/ML extensions)
        let models_path = ext_dir.join("models");
        Self::extract_directory_sync(archive, "models/", &models_path, &mut budget)?;

        // Extract assets directory if exists (for static assets)
        let assets_path = ext_dir.join("assets");
        Self::extract_directory_sync(archive, "assets/", &assets_path, &mut budget)?;

        // Extract config directory if exists (for configuration files)
        let config_path = ext_dir.join("config");
        Self::extract_directory_sync(archive, "config/", &config_path, &mut budget)?;
        tracing::info!("install: resource dirs done");

        // 🔧 macOS: Re-sign all extracted dylibs after installation.
        // When a .nep replaces an existing extension, macOS may cache the old code signature
        // for the file path/inode. Re-signing forces a fresh CDHash so the kernel accepts
        // the new binary when the extension-runner loads it via dlopen.
        // Without this, the runner gets SIGKILL (Code Signature Invalid) on launch.
        #[cfg(target_os = "macos")]
        {
            if let Some(binary_dir) = binary_file.parent() {
                tracing::info!("install: re-signing dylibs (macOS)");
                Self::resign_dylibs_macos(binary_dir);
                tracing::info!("install: resign done");
            }
        }

        // Get component definitions
        let components = manifest
            .frontend
            .as_ref()
            .map(|f| f.components.clone())
            .unwrap_or_default();

        // ✨ Determine which resource directories were extracted
        let models_dir = if models_path.exists() {
            Some(models_path)
        } else {
            None
        };

        let resources_dir = if assets_path.exists() || config_path.exists() {
            // Return assets as resources if either exists
            Some(assets_path)
        } else {
            None
        };

        tracing::info!("install: success ({} v{})", ext_id, version);
        Ok(InstallResult {
            extension_id: ext_id.clone(),
            version: version.clone(),
            binary_path: binary_file,
            manifest_path,
            frontend_dir,
            components,
            checksum,
            models_dir,
            resources_dir,
        })
    }

    /// Re-sign all .dylib files in the binary directory on macOS.
    /// This ensures macOS kernel code-signing cache is updated after
    /// overwriting existing dylibs during .nep installation.
    #[cfg(target_os = "macos")]
    fn resign_dylibs_macos(dir: &Path) {
        // Collect dylib paths first so we can dispatch them in parallel.
        // codesign is an external process — no shared mutable state, so
        // parallel execution is safe. This matters for bundles like
        // stream-player which ship 60+ FFmpeg dylibs: serial signing
        // takes 30-130s and blows past HTTP upload timeouts.
        let dylibs: Vec<std::path::PathBuf> = match std::fs::read_dir(dir) {
            Ok(e) => e
                .flatten()
                .map(|entry| entry.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("dylib"))
                .collect(),
            Err(_) => return,
        };

        if dylibs.is_empty() {
            return;
        }

        let start = std::time::Instant::now();
        let total = dylibs.len();

        // Spawn one thread per dylib. codesign is I/O-bound (fork + exec +
        // Mach IPC to the codesign daemon), so threads stay cheap even for
        // 60+ files. Capped implicitly by the OS scheduler.
        let successes = std::thread::scope(|s| {
            let handles: Vec<_> = dylibs
                .into_iter()
                .map(|path| {
                    s.spawn(move || {
                        match std::process::Command::new("codesign")
                            .args(["--force", "--sign", "-"])
                            .arg(&path)
                            .output()
                        {
                            Ok(output) if output.status.success() => true,
                            Ok(output) => {
                                tracing::warn!(
                                    "codesign failed for {}: {}",
                                    path.display(),
                                    String::from_utf8_lossy(&output.stderr).trim()
                                );
                                false
                            }
                            Err(e) => {
                                tracing::warn!("codesign failed for {}: {}", path.display(), e);
                                false
                            }
                        }
                    })
                })
                .collect();

            handles
                .into_iter()
                .map(|h| h.join().unwrap_or(false))
                .filter(|&ok| ok)
                .count()
        });

        tracing::info!(
            "Re-signed {}/{} dylib(s) in {}ms (parallel)",
            successes,
            total,
            start.elapsed().as_millis()
        );
    }

    /// Extract a single file from the archive (synchronous)
    fn extract_file_sync<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        src_path: &str,
        dst_path: &Path,
        budget: &mut InstallBudget,
    ) -> Result<(), PackageError> {
        let mut file = archive
            .by_name(src_path)
            .map_err(|e| PackageError::MissingFile(format!("{}: {}", src_path, e)))?;

        if file.is_symlink() {
            return Err(PackageError::Zip(format!(
                "symlink entry rejected: '{}'",
                src_path
            )));
        }
        // Zip-bomb defense: reject oversized entries.
        if file.size() > Self::MAX_EXTRACT_FILE_SIZE {
            return Err(PackageError::Zip(format!(
                "File '{}' is {} bytes (exceeds {} byte limit)",
                src_path,
                file.size(),
                Self::MAX_EXTRACT_FILE_SIZE
            )));
        }
        budget.charge(file.size())?;

        // Create parent directory
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // macOS: clear any existing file first — code-signed dylibs with
        // `com.apple.provenance` xattr reject File::create with EACCES on
        // re-install. remove_file clears xattrs + inode.
        if dst_path.exists() {
            let _ = std::fs::remove_file(dst_path);
        }

        // Stream the entry to disk — never read_to_end, since large model or
        // binary entries would OOM. std::io::copy uses an 8 KB buffer.
        let mut out = std::fs::File::create(dst_path)?;
        std::io::copy(&mut file, &mut out)?;
        Ok(())
    }

    /// Zip-bomb defense caps — shared by BOTH extraction paths. The async
    /// path (`extract_file`/`extract_directory`, used by `install()`) had NO
    /// caps while the sync path did; one definition so they can't drift again.
    pub(crate) const MAX_EXTRACT_FILE_SIZE: u64 = 200 * 1024 * 1024;
    pub(crate) const MAX_EXTRACT_TOTAL_SIZE: u64 = 500 * 1024 * 1024;
    pub(crate) const MAX_EXTRACT_FILE_COUNT: usize = 10_000;

    /// Resolve `rel_path` against `dst_dir`, rejecting any entry that would
    /// escape `dst_dir` via `..` traversal or absolute paths.
    ///
    /// This is the central zip-slip defense — both `extract_directory_sync`
    /// and `extract_directory` MUST route every zip entry through this helper.
    /// A malicious `.nep` package could otherwise include an entry such as
    /// `frontend/../../etc/cron.d/backdoor` and write outside the install dir.
    fn safe_join_within(
        dst_dir: &Path,
        rel_path: &str,
    ) -> Result<std::path::PathBuf, PackageError> {
        let mut resolved = dst_dir.to_path_buf();
        for component in std::path::Path::new(rel_path).components() {
            match component {
                std::path::Component::Normal(p) => resolved.push(p),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    return Err(PackageError::Zip(format!(
                        "path traversal rejected ( '..' in '{}')",
                        rel_path
                    )));
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err(PackageError::Zip(format!(
                        "absolute path rejected in '{}'",
                        rel_path
                    )));
                }
            }
        }
        Ok(resolved)
    }

    /// Extract a directory from the archive (synchronous)
    fn extract_directory_sync<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        src_prefix: &str,
        dst_dir: &Path,
        budget: &mut InstallBudget,
    ) -> Result<(), PackageError> {
        // Zip-bomb defense caps (shared with the async path).

        std::fs::create_dir_all(dst_dir)?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| PackageError::Zip(format!("Failed to access file {}: {}", i, e)))?;

            let name = file.name().to_string();

            // Check if file starts with prefix
            if name.starts_with(src_prefix) && !name.ends_with('/') {
                // Remove prefix to get relative path
                let rel_path = name[src_prefix.len()..].to_string();
                let dst_path = Self::safe_join_within(dst_dir, &rel_path)?;

                // Per-file + cumulative size caps.
                let entry_size = file.size();
                if file.is_symlink() {
                    return Err(PackageError::Zip(format!(
                        "symlink entry rejected: '{}'",
                        name
                    )));
                }
                if entry_size > Self::MAX_EXTRACT_FILE_SIZE {
                    return Err(PackageError::Zip(format!(
                        "File '{}' is {} bytes (exceeds {} byte limit)",
                        name,
                        entry_size,
                        Self::MAX_EXTRACT_FILE_SIZE
                    )));
                }
                budget.charge(entry_size)?;

                // Create parent directory
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                // Extract file — streamed (std::io::copy, 8 KB buffer) so a
                // large entry doesn't get buffered in memory.
                if dst_path.exists() {
                    let _ = std::fs::remove_file(&dst_path);
                }
                let mut out = std::fs::File::create(&dst_path)?;
                std::io::copy(&mut file, &mut out)?;
            }
        }

        Ok(())
    }

    /// Create a sidecar JSON file for WASM extensions (synchronous)
    fn create_sidecar_json_sync(
        manifest: &ExtensionPackageManifest,
        json_path: &Path,
    ) -> Result<(), PackageError> {
        use serde_json::json;

        // Build metrics array if capabilities exist
        let metrics = manifest
            .capabilities
            .as_ref()
            .map(|cap| {
                cap.metrics
                    .iter()
                    .map(|m| {
                        json!({
                            "name": m.name,
                            "display_name": m.display_name,
                            "data_type": m.data_type,
                            "unit": m.unit,
                            "min": m.min,
                            "max": m.max,
                            "required": false
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Build commands array if capabilities exist
        let commands = manifest.capabilities.as_ref().map(|cap| {
            cap.commands.iter().map(|c| {
                json!({
                    "name": c.name,
                    "display_name": c.display_name,
                    "payload_template": "",
                    "parameters": c.parameters.as_ref().map(|params| {
                        if let serde_json::Value::Object(props) = params {
                            if let Some(serde_json::Value::Object(props_map)) = props.get("properties") {
                                return props_map.iter().map(|(name, _)| {
                                    json!({
                                        "name": name,
                                        "display_name": name,
                                        "description": "",
                                        "param_type": "String",
                                        "required": false,
                                        "default_value": null,
                                        "min": null,
                                        "max": null,
                                        "options": []
                                    })
                                }).collect::<Vec<_>>();
                            }
                        }
                        Vec::<serde_json::Value>::new()
                    }).unwrap_or_default(),
                    "fixed_values": {},
                    "samples": [],
                    "description": ""
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let sidecar_data = json!({
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "description": manifest.description,
            "author": manifest.author,
            "homepage": manifest.homepage,
            "license": manifest.license,
            "file_path": None::<String>,
            "metrics": metrics,
            "commands": commands
        });

        let content = serde_json::to_string_pretty(&sidecar_data).map_err(|e| {
            PackageError::InvalidManifest(format!("Failed to serialize sidecar JSON: {}", e))
        })?;

        std::fs::write(json_path, content).map_err(|e| {
            PackageError::Io(std::io::Error::other(format!(
                "Failed to write sidecar JSON: {}",
                e
            )))
        })?;

        Ok(())
    }

    /// Extract a single file from the archive
    async fn extract_file<R: Read + std::io::Seek>(
        &self,
        archive: &mut ZipArchive<R>,
        src_path: &str,
        dst_path: &Path,
    ) -> Result<(), PackageError> {
        let mut file = archive
            .by_name(src_path)
            .map_err(|e| PackageError::MissingFile(format!("{}: {}", src_path, e)))?;

        if file.is_symlink() {
            return Err(PackageError::Zip(format!(
                "symlink entry rejected: '{}'",
                src_path
            )));
        }
        if file.size() > Self::MAX_EXTRACT_FILE_SIZE {
            return Err(PackageError::Zip(format!(
                "File '{}' is {} bytes (exceeds {} byte limit)",
                src_path,
                file.size(),
                Self::MAX_EXTRACT_FILE_SIZE
            )));
        }

        // Create parent directory
        if let Some(parent) = dst_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // macOS: clear existing (code-signed xattr blocks create on re-install)
        if dst_path.exists() {
            let _ = tokio::fs::remove_file(dst_path).await;
        }

        // Stream the entry to disk in chunks — never read_to_end, since large
        // model/binary entries would OOM. ZipFile is a blocking reader, so we
        // pull 64 KB at a time through std Read + tokio AsyncWrite.
        use std::io::Read;
        use tokio::io::AsyncWriteExt;
        let mut out = tokio::fs::File::create(dst_path).await?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).await?;
        }
        out.flush().await?;
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
        let mut total_bytes: u64 = 0;

        for i in 0..archive.len() {
            if i + 1 > Self::MAX_EXTRACT_FILE_COUNT {
                return Err(PackageError::Zip(format!(
                    "Archive contains more than {} files (zip-bomb suspected)",
                    Self::MAX_EXTRACT_FILE_COUNT
                )));
            }
            let mut file = archive
                .by_index(i)
                .map_err(|e| PackageError::Zip(format!("Failed to access file {}: {}", i, e)))?;

            let name = file.name().to_string();

            // Check if file starts with prefix
            if name.starts_with(src_prefix) && !name.ends_with('/') {
                // Remove prefix to get relative path
                let rel_path = name[src_prefix.len()..].to_string();
                let dst_path = Self::safe_join_within(dst_dir, &rel_path)?;

                if file.is_symlink() {
                    return Err(PackageError::Zip(format!(
                        "symlink entry rejected: '{}'",
                        name
                    )));
                }
                let entry_size = file.size();
                if entry_size > Self::MAX_EXTRACT_FILE_SIZE {
                    return Err(PackageError::Zip(format!(
                        "File '{}' is {} bytes (exceeds {} byte limit)",
                        name,
                        entry_size,
                        Self::MAX_EXTRACT_FILE_SIZE
                    )));
                }
                total_bytes += entry_size;
                if total_bytes > Self::MAX_EXTRACT_TOTAL_SIZE {
                    return Err(PackageError::Zip(format!(
                        "Total extracted size exceeds {} bytes (zip-bomb suspected)",
                        Self::MAX_EXTRACT_TOTAL_SIZE
                    )));
                }

                // Create parent directory
                if let Some(parent) = dst_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                // Stream-extract (chunked) — never read_to_end large entries.
                use std::io::Read as _;
                use tokio::io::AsyncWriteExt;
                if dst_path.exists() {
                    let _ = tokio::fs::remove_file(&dst_path).await;
                }
                let mut out = tokio::fs::File::create(&dst_path).await?;
                let mut buf = [0u8; 64 * 1024];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buf[..n]).await?;
                }
                out.flush().await?;
            }
        }

        Ok(())
    }

    /// Create a sidecar JSON file for WASM extensions
    /// This allows the WASM loader to find the metadata
    async fn create_sidecar_json(&self, json_path: &Path) -> Result<(), PackageError> {
        use serde_json::json;

        // Build metrics array if capabilities exist
        let metrics = self
            .manifest
            .capabilities
            .as_ref()
            .map(|cap| {
                cap.metrics
                    .iter()
                    .map(|m| {
                        json!({
                            "name": m.name,
                            "display_name": m.display_name,
                            "data_type": m.data_type,
                            "unit": m.unit,
                            "min": m.min,
                            "max": m.max,
                            "required": false
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Build commands array if capabilities exist
        let commands = self.manifest.capabilities.as_ref().map(|cap| {
            cap.commands.iter().map(|c| {
                json!({
                    "name": c.name,
                    "display_name": c.display_name,
                    "payload_template": "",
                    "parameters": c.parameters.as_ref().map(|params| {
                        if let serde_json::Value::Object(props) = params {
                            if let Some(serde_json::Value::Object(props_map)) = props.get("properties") {
                                return props_map.iter().map(|(name, _)| {
                                    json!({
                                        "name": name,
                                        "display_name": name,
                                        "description": "",
                                        "param_type": "String",
                                        "required": false,
                                        "default_value": null,
                                        "min": null,
                                        "max": null,
                                        "options": []
                                    })
                                }).collect::<Vec<_>>();
                            }
                        }
                        Vec::<serde_json::Value>::new()
                    }).unwrap_or_default(),
                    "fixed_values": {},
                    "samples": [],
                    "description": ""
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let sidecar_data = json!({
            "id": self.manifest.id,
            "name": self.manifest.name,
            "version": self.manifest.version,
            "description": self.manifest.description,
            "author": self.manifest.author,
            "homepage": self.manifest.homepage,
            "license": self.manifest.license,
            "file_path": self.path,
            "metrics": metrics,
            "commands": commands
        });

        let content = serde_json::to_string_pretty(&sidecar_data).map_err(|e| {
            PackageError::InvalidManifest(format!("Failed to serialize sidecar JSON: {}", e))
        })?;

        tokio::fs::write(json_path, content).await.map_err(|e| {
            PackageError::Io(std::io::Error::other(format!(
                "Failed to write sidecar JSON: {}",
                e
            )))
        })?;

        Ok(())
    }

    /// Uninstall an extension — removes its directory EXCEPT the
    /// extension-private `data/` subdir, which the platform contract
    /// guarantees survives upgrades AND uninstall (pipelines, face
    /// libraries, licenses, capture history live there). Operators wipe
    /// it explicitly if they truly want a clean removal.
    pub async fn uninstall(install_dir: &Path, extension_id: &str) -> Result<(), PackageError> {
        let ext_dir = install_dir.join(extension_id);

        if ext_dir.exists() {
            let data_dir = ext_dir.join("data");
            let mut entries = tokio::fs::read_dir(&ext_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path == data_dir {
                    tracing::info!(
                        "uninstall: preserving extension data dir: {}",
                        data_dir.display()
                    );
                    continue;
                }
                if path.is_dir() {
                    tokio::fs::remove_dir_all(&path).await?;
                } else {
                    tokio::fs::remove_file(&path).await?;
                }
            }
        }

        Ok(())
    }
}

/// Platform format types used in the extension system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformFormat {
    /// Hyphen format: "windows-x86_64", "darwin-aarch64"
    /// Used in marketplace metadata `builds` keys
    Hyphen,
    /// Underscore format: "windows_amd64", "darwin_aarch64"
    /// Used in .nep package `binaries` keys and filenames
    Underscore,
}

/// Detect the current platform
/// Returns platform string in underscore format (e.g., "darwin_aarch64")
/// to match the format used in extension packages
pub fn detect_platform() -> String {
    detect_platform_with_format(PlatformFormat::Underscore)
}

/// Detect the current platform with specified format
pub fn detect_platform_with_format(format: PlatformFormat) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch, format) {
        // macOS
        ("macos", "aarch64", PlatformFormat::Hyphen) => "darwin-aarch64".to_string(),
        ("macos", "aarch64", PlatformFormat::Underscore) => "darwin_aarch64".to_string(),
        ("macos", "x86_64", PlatformFormat::Hyphen) => "darwin-x86_64".to_string(),
        ("macos", "x86_64", PlatformFormat::Underscore) => "darwin_x86_64".to_string(),
        // Linux
        ("linux", "x86_64", PlatformFormat::Hyphen) => "linux-x86_64".to_string(),
        ("linux", "x86_64", PlatformFormat::Underscore) => "linux_amd64".to_string(),
        ("linux", "aarch64", PlatformFormat::Hyphen) => "linux-aarch64".to_string(),
        ("linux", "aarch64", PlatformFormat::Underscore) => "linux_arm64".to_string(),
        // Windows
        ("windows", "x86_64", PlatformFormat::Hyphen) => "windows-x86_64".to_string(),
        ("windows", "x86_64", PlatformFormat::Underscore) => "windows_amd64".to_string(),
        ("windows", "aarch64", PlatformFormat::Hyphen) => "windows-aarch64".to_string(),
        ("windows", "aarch64", PlatformFormat::Underscore) => "windows_arm64".to_string(),
        // Fallback
        _ => match format {
            PlatformFormat::Hyphen => format!("{}-{}", os, arch),
            PlatformFormat::Underscore => format!("{}_{}", os, arch),
        },
    }
}

/// Convert platform format between hyphen and underscore
pub fn convert_platform_format(platform: &str, target_format: PlatformFormat) -> String {
    // Detect source format
    let (os, arch) = if platform.contains('-') {
        // Hyphen format: "windows-x86_64"
        let parts: Vec<&str> = platform.splitn(2, '-').collect();
        if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            return platform.to_string();
        }
    } else if platform.contains('_') {
        // Underscore format: "windows_amd64"
        let parts: Vec<&str> = platform.splitn(2, '_').collect();
        if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            return platform.to_string();
        }
    } else {
        return platform.to_string();
    };

    // Map arch names: x86_64 <-> amd64, aarch64 <-> arm64
    let normalized_arch = match arch {
        "x86_64" | "amd64" => ("x86_64", "amd64"),
        "aarch64" | "arm64" => ("aarch64", "arm64"),
        _ => (arch, arch),
    };

    match target_format {
        PlatformFormat::Hyphen => format!("{}-{}", os, normalized_arch.0),
        PlatformFormat::Underscore => format!("{}_{}", os, normalized_arch.1),
    }
}

/// Per-INSTALL extraction budget, shared across every phase (manifest,
/// binary, bundled libs, frontend/models/assets/config directories).
/// Each phase used to carry its own 500MB budget — a crafted package
/// could legally extract ~2.7GB total. One budget, charged by all.
#[derive(Debug)]
pub(crate) struct InstallBudget {
    total_bytes: u64,
    file_count: usize,
}

impl InstallBudget {
    pub(crate) fn new() -> Self {
        Self {
            total_bytes: 0,
            file_count: 0,
        }
    }

    /// Charge one extracted file; error when cumulative caps blow.
    pub(crate) fn charge(&mut self, bytes: u64) -> Result<(), PackageError> {
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.file_count += 1;
        if self.total_bytes > ExtensionPackage::MAX_EXTRACT_TOTAL_SIZE {
            return Err(PackageError::Zip(format!(
                "Total extracted size exceeds {} bytes across the whole install (zip-bomb suspected)",
                ExtensionPackage::MAX_EXTRACT_TOTAL_SIZE
            )));
        }
        if self.file_count > ExtensionPackage::MAX_EXTRACT_FILE_COUNT {
            return Err(PackageError::Zip(format!(
                "More than {} files extracted across the whole install (zip-bomb suspected)",
                ExtensionPackage::MAX_EXTRACT_FILE_COUNT
            )));
        }
        Ok(())
    }
}

impl Default for InstallBudget {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod data_dir_tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nep-datadir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn uninstall_preserves_extension_data_dir() {
        let install_dir = scratch("un");
        let ext_dir = install_dir.join("vision-hub");
        std::fs::create_dir_all(ext_dir.join("data")).unwrap();
        std::fs::write(ext_dir.join("data/pipelines.json"), r#"[{"id":"gate"}]"#).unwrap();
        std::fs::write(ext_dir.join("data/license.key"), "NP1.x.y").unwrap();
        std::fs::write(ext_dir.join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(ext_dir.join("binaries")).unwrap();
        std::fs::write(ext_dir.join("binaries/extension.dylib"), b"bin").unwrap();

        ExtensionPackage::uninstall(&install_dir, "vision-hub").await.unwrap();

        // Package files gone…
        assert!(!ext_dir.join("manifest.json").exists());
        assert!(!ext_dir.join("binaries").exists());
        // …private data survives
        assert!(ext_dir.join("data/pipelines.json").exists());
        assert!(ext_dir.join("data/license.key").exists());
        let _ = std::fs::remove_dir_all(&install_dir);
    }
}

mod install_budget_tests {
    use super::*;

    #[test]
    fn budget_is_cumulative_across_phases() {
        // Each phase used to carry its own 500MB budget; the shared budget
        // must trip on the CUMULATIVE total, which is the whole point.
        let mut b = InstallBudget::new();
        let per = 200 * 1024 * 1024 + 1; // exceeds nothing alone? 200MB+1 each
                                         // 500MB cap / (200MB+1) → third charge must blow the total.
        assert!(b.charge(200 * 1024 * 1024).is_ok());
        assert!(b.charge(200 * 1024 * 1024).is_ok());
        let err = b.charge(per).unwrap_err().to_string();
        assert!(err.contains("Total extracted size"), "got: {err}");
        let _ = per;
    }

    #[test]
    fn budget_counts_files_cumulatively() {
        let mut b = InstallBudget::new();
        for _ in 0..ExtensionPackage::MAX_EXTRACT_FILE_COUNT {
            b.charge(1).unwrap();
        }
        let err = b.charge(1).unwrap_err().to_string();
        assert!(err.contains("files"), "got: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_join_within_rejects_traversal() {
        use ExtensionPackage;
        let dst = std::path::Path::new("/tmp/ext-install");

        // Normal relative path resolves within dst_dir
        let ok = ExtensionPackage::safe_join_within(dst, "frontend/dist/bundle.js").unwrap();
        assert!(
            ok.starts_with(dst),
            "normal path should stay within dst_dir"
        );

        // '..' in rel_path must be rejected (zip slip)
        assert!(
            ExtensionPackage::safe_join_within(dst, "../../etc/cron.d/backdoor").is_err(),
            "traversal via .. must be rejected"
        );
        // Mid-path '..' also rejected
        assert!(
            ExtensionPackage::safe_join_within(dst, "frontend/../../../etc/passwd").is_err(),
            "mid-path traversal must be rejected"
        );
        // Absolute path rejected
        assert!(
            ExtensionPackage::safe_join_within(dst, "/etc/passwd").is_err(),
            "absolute path must be rejected"
        );
        // '.' is allowed (no-op)
        assert!(
            ExtensionPackage::safe_join_within(dst, "frontend/./bundle.js").is_ok(),
            "current-dir component should be allowed"
        );
    }

    #[test]
    fn test_detect_platform() {
        let platform = detect_platform();
        println!("Detected platform: {}", platform);
        assert!(!platform.is_empty());
    }

    #[test]
    fn test_format_constants() {
        assert_eq!(PACKAGE_FORMAT, "neomind-extension-package");
        assert_eq!(CURRENT_ABI_VERSION, 3);
        assert_eq!(MIN_ABI_VERSION, 3);
    }

    #[test]
    fn test_frontend_string_format() {
        // Test that frontend field accepts string format (backward compatibility)
        let json = r#"{
            "format": "neomind-extension-package",
            "abi_version": 3,
            "id": "test-extension",
            "name": "Test Extension",
            "version": "1.0.0",
            "frontend": "frontend/"
        }"#;

        let manifest: ExtensionPackageManifest =
            serde_json::from_str(json).expect("Failed to parse manifest with string frontend");

        assert_eq!(manifest.id, "test-extension");
        assert!(manifest.frontend.is_some());
        assert!(manifest.frontend.as_ref().unwrap().components.is_empty());
    }

    #[test]
    fn test_frontend_struct_format() {
        // Test that frontend field accepts struct format
        let json = r#"{
            "format": "neomind-extension-package",
            "abi_version": 3,
            "id": "test-extension",
            "name": "Test Extension",
            "version": "1.0.0",
            "frontend": {
                "components": [
                    {
                        "type": "card",
                        "name": "Weather Card",
                        "description": "A weather display card",
                        "category": "widget",
                        "bundle_path": "dist/bundle.js",
                        "export_name": "WeatherCard"
                    }
                ]
            }
        }"#;

        let manifest: ExtensionPackageManifest =
            serde_json::from_str(json).expect("Failed to parse manifest with struct frontend");

        assert_eq!(manifest.id, "test-extension");
        assert!(manifest.frontend.is_some());
        assert_eq!(manifest.frontend.as_ref().unwrap().components.len(), 1);
        assert_eq!(
            manifest.frontend.as_ref().unwrap().components[0].name,
            "Weather Card"
        );
    }

    #[test]
    fn test_frontend_null_format() {
        // Test that frontend field accepts null/missing
        let json = r#"{
            "format": "neomind-extension-package",
            "abi_version": 3,
            "id": "test-extension",
            "name": "Test Extension",
            "version": "1.0.0"
        }"#;

        let manifest: ExtensionPackageManifest =
            serde_json::from_str(json).expect("Failed to parse manifest without frontend");

        assert_eq!(manifest.id, "test-extension");
        assert!(manifest.frontend.is_none());
    }

    fn unique_nonce() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }

    /// Build a minimal valid .nep (in-memory) for the current platform, so
    /// install tests don't depend on a real extension package.
    fn build_test_nep() -> Vec<u8> {
        use std::io::Write;
        let platform = detect_platform();
        let lib_ext = if platform.starts_with("windows") {
            "dll"
        } else if platform.starts_with("darwin") {
            "dylib"
        } else {
            "so"
        };
        let binary_entry = format!("binaries/{}/extension.{}", platform, lib_ext);
        let manifest = format!(
            r#"{{"format":"neomind-extension-package","abi_version":3,"id":"test-install-file","name":"Test","version":"9.9.9","binaries":{{"{}":"{}"}},"type":"native"}}"#,
            platform, binary_entry
        );
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("manifest.json", opts).unwrap();
        zw.write_all(manifest.as_bytes()).unwrap();
        zw.start_file(&binary_entry, opts).unwrap();
        zw.write_all(b"fake binary").unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn test_install_from_file_streams_archive() {
        // install_from_file must match install_sync for identical package
        // data (both share install_from_archive) — proves the file-backed
        // path works end-to-end without buffering the archive in memory.
        let zip_bytes = build_test_nep();
        let tmp = std::env::temp_dir().join(format!("neomind-test-install-{}.nep", unique_nonce()));
        std::fs::write(&tmp, &zip_bytes).unwrap();

        let target_sync =
            std::env::temp_dir().join(format!("neomind-test-sync-{}", unique_nonce()));
        let target_file =
            std::env::temp_dir().join(format!("neomind-test-file-{}", unique_nonce()));
        std::fs::create_dir_all(&target_sync).unwrap();
        std::fs::create_dir_all(&target_file).unwrap();

        let via_sync = ExtensionPackage::install_sync(&zip_bytes, &target_sync);
        let via_file = ExtensionPackage::install_from_file(&tmp, &target_file);

        // cleanup
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&target_sync);
        let _ = std::fs::remove_dir_all(&target_file);

        let via_sync = via_sync.expect("install_sync should succeed on test package");
        let via_file = via_file.expect("install_from_file should succeed on test package");

        // Same package → same metadata + checksum (computed via &[u8] vs a
        // streamed file, but the underlying bytes are identical).
        assert_eq!(via_file.extension_id, via_sync.extension_id);
        assert_eq!(via_file.version, via_sync.version);
        assert_eq!(via_file.checksum, via_sync.checksum);
        assert_eq!(via_file.extension_id, "test-install-file");
        assert!(!via_file.checksum.is_empty());
    }

    /// Current process RSS in KB (ps-based; works on macOS + Linux).
    fn current_rss_kb() -> u64 {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .expect("ps failed");
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    }

    /// Proves install_from_file does NOT buffer the whole package in memory.
    /// Builds a ~150 MB package streaming to disk (1 MB write buffer, so the
    /// build itself stays low-memory), then measures RSS before/after install.
    /// A streaming install's RSS delta is far below the package size; an
    /// in-memory install would spike by ~3x the package size and fail.
    #[test]
    #[ignore]
    fn test_install_from_file_memory_footprint() {
        use std::io::Write;
        let platform = detect_platform();
        let lib_ext = if platform.starts_with("windows") {
            "dll"
        } else if platform.starts_with("darwin") {
            "dylib"
        } else {
            "so"
        };
        let binary_entry = format!("binaries/{}/extension.{}", platform, lib_ext);
        let manifest = format!(
            r#"{{"format":"neomind-extension-package","abi_version":3,"id":"test-mem","name":"T","version":"1.0.0","binaries":{{"{}":"{}"}},"type":"native"}}"#,
            platform, binary_entry
        );

        // Stream-build a ~150 MB Stored (uncompressed) zip: 1 MB buffer in a
        // loop, so the package bytes are never all in memory during build.
        let target_pkg_mb: u64 = 150;
        let tmp = std::env::temp_dir().join(format!("neomind-memtest-{}.nep", unique_nonce()));
        {
            let file = std::fs::File::create(&tmp).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file("manifest.json", opts).unwrap();
            zw.write_all(manifest.as_bytes()).unwrap();
            zw.start_file(&binary_entry, opts).unwrap();
            let chunk = vec![0xABu8; 1024 * 1024]; // 1 MB
            for _ in 0..target_pkg_mb {
                zw.write_all(&chunk).unwrap();
            }
            zw.finish().unwrap();
        }
        let pkg_size = std::fs::metadata(&tmp).unwrap().len();
        let pkg_mb = pkg_size as f64 / 1024.0 / 1024.0;
        println!("package size: {:.0} MB", pkg_mb);

        let target =
            std::env::temp_dir().join(format!("neomind-memtest-target-{}", unique_nonce()));
        std::fs::create_dir_all(&target).unwrap();

        let rss_before = current_rss_kb();
        let result = ExtensionPackage::install_from_file(&tmp, &target);
        let rss_after = current_rss_kb();

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&target);

        result.expect("install_from_file should succeed on a large package");

        let delta_mb = (rss_after as i64 - rss_before as i64) as f64 / 1024.0;
        println!(
            "RSS before: {} KB, after: {} KB, delta: {:.1} MB (package {:.0} MB)",
            rss_before, rss_after, delta_mb, pkg_mb
        );
        // Streaming: delta must be far below the package size — only chunk
        // buffers + zip metadata are held. An in-memory install would spike
        // by ~3x package size. Allow 30% headroom for allocator/page artifacts.
        assert!(
            delta_mb < pkg_mb * 0.3,
            "RSS grew {:.1} MB for a {:.0} MB package — install_from_file is buffering the whole package in memory!",
            delta_mb,
            pkg_mb
        );
    }

    /// Upload path = load (stream hash + manifest) + install (file-backed
    /// extract). Proves the async path also streams — a 150 MB upload must
    /// not spike RSS the way the old read_to_end + Cursor did.
    #[tokio::test]
    #[ignore]
    async fn test_upload_path_memory_footprint() {
        use std::io::Write;
        let platform = detect_platform();
        let lib_ext = if platform.starts_with("windows") {
            "dll"
        } else if platform.starts_with("darwin") {
            "dylib"
        } else {
            "so"
        };
        let binary_entry = format!("binaries/{}/extension.{}", platform, lib_ext);
        let manifest = format!(
            r#"{{"format":"neomind-extension-package","abi_version":3,"id":"test-upload-mem","name":"T","version":"1.0.0","binaries":{{"{}":"{}"}},"type":"native"}}"#,
            platform, binary_entry
        );

        let target_pkg_mb: u64 = 150;
        let tmp =
            std::env::temp_dir().join(format!("neomind-upload-memtest-{}.nep", unique_nonce()));
        {
            let file = std::fs::File::create(&tmp).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file("manifest.json", opts).unwrap();
            zw.write_all(manifest.as_bytes()).unwrap();
            zw.start_file(&binary_entry, opts).unwrap();
            let chunk = vec![0xABu8; 1024 * 1024];
            for _ in 0..target_pkg_mb {
                zw.write_all(&chunk).unwrap();
            }
            zw.finish().unwrap();
        }
        let pkg_size = std::fs::metadata(&tmp).unwrap().len();
        let pkg_mb = pkg_size as f64 / 1024.0 / 1024.0;

        let target =
            std::env::temp_dir().join(format!("neomind-upload-memtest-target-{}", unique_nonce()));
        std::fs::create_dir_all(&target).unwrap();

        let rss_before = current_rss_kb();
        let package = ExtensionPackage::load(&tmp)
            .await
            .expect("load should succeed");
        let _result = package
            .install(&target)
            .await
            .expect("install should succeed");
        let rss_after = current_rss_kb();

        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&target);

        let delta_mb = (rss_after as i64 - rss_before as i64) as f64 / 1024.0;
        println!(
            "upload path RSS before: {} KB, after: {} KB, delta: {:.1} MB (package {:.0} MB)",
            rss_before, rss_after, delta_mb, pkg_mb
        );
        assert!(
            delta_mb < pkg_mb * 0.3,
            "upload path RSS grew {:.1} MB for {:.0} MB package — load/install not streaming!",
            delta_mb,
            pkg_mb
        );
    }
}
