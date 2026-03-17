//! Unified extension uninstallation service.
//!
//! This service handles complete extension removal including:
//! - Unloading from memory
//! - Deleting extension directory
//! - Removing database records
//! - Cleaning up .nep cache (optional)

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};

use neomind_core::extension::package::ExtensionPackage;
use neomind_storage::extensions::ExtensionStore;

/// Unified extension uninstallation service
pub struct ExtensionUninstallService {
    install_dir: PathBuf,
    nep_cache_dir: PathBuf,
}

impl ExtensionUninstallService {
    /// Create a new uninstallation service
    pub fn new<P: AsRef<Path>>(install_dir: P, nep_cache_dir: P) -> Self {
        Self {
            install_dir: install_dir.as_ref().to_path_buf(),
            nep_cache_dir: nep_cache_dir.as_ref().to_path_buf(),
        }
    }

    /// Completely uninstall an extension
    ///
    /// # Arguments
    /// * `ext_id` - Extension ID to uninstall
    /// * `cleanup_nep_cache` - If true, also remove the .nep package from cache
    ///
    /// # Returns
    /// Uninstall report showing what was cleaned up
    pub async fn uninstall(
        &self,
        ext_id: &str,
        cleanup_nep_cache: bool,
    ) -> Result<UninstallReport, Box<dyn std::error::Error>> {
        info!(
            "Uninstalling extension: {} (cleanup_nep_cache: {})",
            ext_id, cleanup_nep_cache
        );

        let mut report = UninstallReport::default();

        // 1. Delete extension directory
        let ext_dir = self.install_dir.join(ext_id);
        if ext_dir.exists() {
            fs::remove_dir_all(&ext_dir).await?;
            report.directory_removed = true;
            info!("Removed extension directory: {}", ext_dir.display());
        }

        // 2. Delete from database
        if let Ok(store) = ExtensionStore::open("data/extensions.redb") {
            if store.delete(ext_id)? {
                report.database_removed = true;
                info!("Removed database record for: {}", ext_id);
            }
        }

        // 3. Clean up .nep cache (optional)
        if cleanup_nep_cache {
            if let Some(nep_file) = self.find_nep_cache_file(ext_id).await? {
                fs::remove_file(&nep_file).await?;
                report.nep_cache_removed = true;
                info!("Removed .nep cache file: {}", nep_file.display());
            }
        }

        Ok(report)
    }

    /// Find the .nep cache file for an extension
    ///
    /// This searches the nep_cache_dir for a .nep package that matches
    /// the given extension ID.
    async fn find_nep_cache_file(
        &self,
        ext_id: &str,
    ) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        if !self.nep_cache_dir.exists() {
            return Ok(None);
        }

        let mut entries = fs::read_dir(&self.nep_cache_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Only check .nep files
            if path.extension().and_then(|s| s.to_str()) != Some("nep") {
                continue;
            }

            // Read manifest to check if it matches
            if let Ok(package) = ExtensionPackage::load(&path).await {
                if package.manifest.id == ext_id {
                    return Ok(Some(path));
                }
            }
        }

        Ok(None)
    }

    /// Clean up all .nep cache files for uninstalled extensions
    ///
    /// This is useful for maintenance to remove orphaned .nep packages
    /// whose extensions are no longer installed.
    pub async fn cleanup_orphaned_nep_cache(
        &self,
    ) -> Result<CleanupReport, Box<dyn std::error::Error>> {
        info!("Cleaning up orphaned .nep cache files");

        let mut report = CleanupReport::default();

        if !self.nep_cache_dir.exists() {
            return Ok(report);
        }

        // Get list of installed extensions
        let installed_exts: std::collections::HashSet<String> = if self.install_dir.exists() {
            let mut entries = fs::read_dir(&self.install_dir).await?;
            let mut exts = std::collections::HashSet::new();

            while let Some(entry) = entries.next_entry().await? {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        exts.insert(name.to_string());
                    }
                }
            }
            exts
        } else {
            std::collections::HashSet::new()
        };

        // Check each .nep file
        let mut entries = fs::read_dir(&self.nep_cache_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("nep") {
                continue;
            }

            report.scanned += 1;

            // Try to load the package to get its ID
            match ExtensionPackage::load(&path).await {
                Ok(package) => {
                    let ext_id = &package.manifest.id;

                    // If extension is not installed, remove the .nep file
                    if !installed_exts.contains(ext_id) {
                        fs::remove_file(&path).await?;
                        report.removed += 1;
                        info!(
                            "Removed orphaned .nep cache for extension: {} ({})",
                            ext_id,
                            path.display()
                        );
                    } else {
                        report.kept += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to load .nep package {}: {}, skipping",
                        path.display(),
                        e
                    );
                    report.failed += 1;
                }
            }
        }

        Ok(report)
    }
}

/// Report from uninstall operation
#[derive(Debug, Default)]
pub struct UninstallReport {
    pub directory_removed: bool,
    pub database_removed: bool,
    pub nep_cache_removed: bool,
}

/// Report from cleanup_orphaned_nep_cache operation
#[derive(Debug, Default)]
pub struct CleanupReport {
    pub scanned: usize,
    pub removed: usize,
    pub kept: usize,
    pub failed: usize,
}
