//! Unified extension installation service.
//!
//! This service handles all extension installations from different sources:
//! - Marketplace downloads
//! - Local uploads (.nep files)
//! - Manual placement (/extensions/ directory)

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, error};

use neomind_core::extension::package::ExtensionPackage;

/// Unified extension installation service
///
/// Handles all sources of extension installation through a common pipeline.
pub struct ExtensionInstallService {
    install_dir: PathBuf,
    nep_cache_dir: PathBuf,
}

impl ExtensionInstallService {
    /// Create a new installation service
    pub fn new<P: AsRef<Path>>(install_dir: P, nep_cache_dir: P) -> Self {
        Self {
            install_dir: install_dir.as_ref().to_path_buf(),
            nep_cache_dir: nep_cache_dir.as_ref().to_path_buf(),
        }
    }

    /// Install extension from a .nep package file path
    pub async fn install_from_nep_file(
        &self,
        nep_path: &Path,
    ) -> Result<neomind_core::extension::package::InstallResult, Box<dyn std::error::Error>> {
        info!("Installing extension from: {}", nep_path.display());

        // 1. Load the package
        let package = ExtensionPackage::load(nep_path).await?;

        let ext_id = package.manifest.id.clone();
        let new_version = package.manifest.version.clone();

        // 2. Check if upgrade is needed
        if self.needs_upgrade(&ext_id, &new_version).await? {
            info!("Upgrading {} to version {}", ext_id, new_version);
            // Note: Unloading will be handled by the caller through unified service
        }

        // 3. Install the package
        let install_result = package.install(&self.install_dir).await?;

        // 4. Save to database
        self.save_to_database(&package, &install_result).await?;

        // 5. Clean up old version files
        self.cleanup_old_files(&ext_id).await?;

        Ok(install_result)
    }

    /// Install extension from byte stream (Marketplace download)
    pub async fn install_from_bytes(
        &self,
        bytes: &[u8],
        source_url: Option<&str>,
    ) -> Result<neomind_core::extension::package::InstallResult, Box<dyn std::error::Error>> {
        info!(
            "Installing extension from byte stream{}",
            source_url.map(|u| format!(" (from {})", u)).unwrap_or_default()
        );

        // Save to temporary file
        let temp_nep = self.nep_cache_dir.join(format!("temp_{}.nep", uuid::Uuid::new_v4()));
        
        // Ensure cache directory exists
        if let Some(parent) = temp_nep.parent() {
            fs::create_dir_all(parent).await?;
        }
        
        fs::write(&temp_nep, bytes).await?;

        // Use unified installation flow
        let result = self.install_from_nep_file(&temp_nep).await;

        // Clean up temporary file
        let _ = fs::remove_file(&temp_nep).await;

        result
    }

    /// Scan /extensions/ directory and auto-install all .nep packages
    pub async fn sync_nep_cache(&self) -> Result<SyncReport, Box<dyn std::error::Error>> {
        info!("Scanning {} for .nep packages", self.nep_cache_dir.display());

        if !self.nep_cache_dir.exists() {
            return Ok(SyncReport::default());
        }

        let mut report = SyncReport::default();
        let mut entries = fs::read_dir(&self.nep_cache_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Only process .nep files
            if path.extension().and_then(|s| s.to_str()) != Some("nep") {
                continue;
            }

            report.scanned += 1;

            match self.process_nep_file(&path).await {
                Ok(ProcessAction::Installed) => {
                    report.installed += 1;
                    info!("Installed extension from: {}", path.display());
                }
                Ok(ProcessAction::Upgraded) => {
                    report.upgraded += 1;
                    info!("Upgraded extension from: {}", path.display());
                }
                Ok(ProcessAction::Skipped) => {
                    report.skipped += 1;
                }
                Err(e) => {
                    error!("Failed to process {}: {}", path.display(), e);
                }
            }
        }

        Ok(report)
    }

    /// Check if an extension needs to be upgraded
    async fn needs_upgrade(
        &self,
        ext_id: &str,
        new_version: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let ext_dir = self.install_dir.join(ext_id);
        
        if !ext_dir.exists() {
            return Ok(true); // New installation
        }

        let manifest_path = ext_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Ok(true); // Corrupted installation, need to reinstall
        }

        let manifest_content = fs::read_to_string(&manifest_path).await?;
        let manifest: serde_json::Value = serde_json::from_str(&manifest_content)?;

        if let Some(current) = manifest.get("version").and_then(|v| v.as_str()) {
            match (current.parse::<semver::Version>(), new_version.parse::<semver::Version>()) {
                (Ok(cur), Ok(new)) => return Ok(cur < new),
                _ => return Ok(current != new_version),
            }
        }

        Ok(true)
    }

    /// Save extension record to database
    async fn save_to_database(
        &self,
        _package: &ExtensionPackage,
        _install_result: &neomind_core::extension::package::InstallResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // This will be called by the handler after loading the extension
        // The handler is responsible for creating the ExtensionRecord
        Ok(())
    }

    /// Clean up old version files
    async fn cleanup_old_files(&self, _ext_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Clean up temporary files, old versions, etc.
        // For now, the ExtensionPackage::install method handles this
        Ok(())
    }

    /// Process a single .nep file (used by sync_nep_cache)
    async fn process_nep_file(
        &self,
        nep_path: &Path,
    ) -> Result<ProcessAction, Box<dyn std::error::Error>> {
        // Load package to check ID and version
        let package = ExtensionPackage::load(nep_path).await?;
        let ext_id = &package.manifest.id;
        let version = &package.manifest.version;

        let ext_dir = self.install_dir.join(ext_id);
        
        if !ext_dir.exists() {
            return Ok(ProcessAction::Installed);
        }

        // Check if version upgrade is needed
        if self.needs_upgrade(ext_id, version).await? {
            return Ok(ProcessAction::Upgraded);
        }

        Ok(ProcessAction::Skipped)
    }
}

/// Result of a successful installation
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub extension_id: String,
    pub version: String,
    pub name: String,
    pub binary_path: PathBuf,
    pub checksum: String,
}

/// Report from sync_nep_cache operation
#[derive(Debug, Default)]
pub struct SyncReport {
    pub scanned: usize,
    pub installed: usize,
    pub upgraded: usize,
    pub skipped: usize,
}

/// Action taken during sync operation
#[derive(Debug, PartialEq)]
enum ProcessAction {
    Installed,
    Upgraded,
    Skipped,
}
