//! Unified extension installation service.
//!
//! Handles extension installation from .nep package files: the marketplace
//! download path, local uploads, and the `sync` scan of the extension cache
//! directory. Disk-side only — runtime registration (load + record + config
//! carry-forward) lives with the callers via
//! `handlers::extensions::register_installed_package`.

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{error, info};

use neomind_core::extension::package::ExtensionPackage;

/// Unified extension installation service.
pub struct ExtensionInstallService {
    install_dir: PathBuf,
    nep_cache_dir: PathBuf,
}

/// A package installed (or upgraded) by [`ExtensionInstallService::sync_nep_cache`].
/// Callers use this to register the extension with the runtime.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub extension_id: String,
    pub version: String,
    pub binary_path: PathBuf,
    pub frontend_dir: Option<PathBuf>,
    pub checksum: String,
    pub upgraded: bool,
}

impl ExtensionInstallService {
    /// Create a new installation service.
    pub fn new<P: AsRef<Path>>(install_dir: P, nep_cache_dir: P) -> Self {
        Self {
            install_dir: install_dir.as_ref().to_path_buf(),
            nep_cache_dir: nep_cache_dir.as_ref().to_path_buf(),
        }
    }

    /// Install extension from a .nep package file path.
    ///
    /// Runs the SYNC installer on the blocking pool: the async `install()`
    /// holds a non-Send `ZipFile` across awaits, which used to be invisible
    /// only because nothing ever called it from a spawned task. The sync
    /// path applies the exact same zip-bomb/traversal/symlink defenses.
    pub async fn install_from_nep_file(
        &self,
        nep_path: &Path,
    ) -> Result<
        neomind_core::extension::package::InstallResult,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        info!("Installing extension from: {}", nep_path.display());
        self.install_sync(nep_path).await
    }

    /// Blocking-pool install via the synchronous package installer.
    async fn install_sync(
        &self,
        nep_path: &Path,
    ) -> Result<
        neomind_core::extension::package::InstallResult,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let install_dir = self.install_dir.clone();
        let path = nep_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            ExtensionPackage::install_from_file(&path, &install_dir)
        })
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("install task join error: {e}").into()
        })?
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("package install failed: {e}").into()
        })
    }

    /// Install extension from byte stream (marketplace download).
    pub async fn install_from_bytes(
        &self,
        bytes: &[u8],
        source_url: Option<&str>,
    ) -> Result<
        neomind_core::extension::package::InstallResult,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        info!(
            "Installing extension from byte stream{}",
            source_url
                .map(|u| format!(" (from {})", u))
                .unwrap_or_default()
        );

        let temp_nep = self
            .nep_cache_dir
            .join(format!("temp_{}.nep", uuid::Uuid::new_v4()));
        if let Some(parent) = temp_nep.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&temp_nep, bytes).await?;

        let result = self.install_from_nep_file(&temp_nep).await;
        let _ = fs::remove_file(&temp_nep).await;
        result
    }

    /// Scan the .nep cache directory and install every package that is new or
    /// newer than what's on disk.
    ///
    /// This USED TO be a no-op reporter: `process_nep_file` decided
    /// Installed/Upgraded/Skipped and returned without touching the disk, so
    /// callers reported "installed: N" while nothing happened. Now it really
    /// installs and returns the details callers need to register the
    /// extensions with the runtime.
    pub async fn sync_nep_cache(
        &self,
    ) -> Result<SyncReport, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Scanning {} for .nep packages",
            self.nep_cache_dir.display()
        );

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
            // Marketplace download cache uses temp_*.nep for in-flight writes.
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("temp_"))
                .unwrap_or(false)
            {
                continue;
            }

            report.scanned += 1;

            match self.process_nep_file(&path).await {
                Ok(Some(installed)) => {
                    if installed.upgraded {
                        report.upgraded += 1;
                        info!(
                            extension_id = %installed.extension_id,
                            version = %installed.version,
                            "Upgraded extension from {}", path.display()
                        );
                    } else {
                        report.installed += 1;
                        info!(
                            extension_id = %installed.extension_id,
                            version = %installed.version,
                            "Installed extension from {}", path.display()
                        );
                    }
                    report.installed_packages.push(installed);
                }
                Ok(None) => {
                    report.skipped += 1;
                }
                Err(e) => {
                    error!("Failed to process {}: {}", path.display(), e);
                    report.failed += 1;
                }
            }
        }

        Ok(report)
    }

    /// Whether an on-disk extension is older than `new_version`.
    /// Missing/corrupt installation counts as "needs install".
    async fn needs_upgrade(
        &self,
        ext_id: &str,
        new_version: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
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
            match (
                current.parse::<semver::Version>(),
                new_version.parse::<semver::Version>(),
            ) {
                (Ok(cur), Ok(new)) => return Ok(cur < new),
                _ => return Ok(current != new_version),
            }
        }

        Ok(true)
    }

    /// Install a single .nep file if it is new or newer than what's on disk.
    /// `Ok(None)` = up to date, nothing done.
    async fn process_nep_file(
        &self,
        nep_path: &Path,
    ) -> Result<Option<InstalledPackage>, Box<dyn std::error::Error + Send + Sync>> {
        let package = ExtensionPackage::load(nep_path).await?;
        let ext_id = package.manifest.id.clone();
        let version = package.manifest.version.clone();

        let ext_dir = self.install_dir.join(&ext_id);
        let upgraded = ext_dir.exists();
        if upgraded && !self.needs_upgrade(&ext_id, &version).await? {
            return Ok(None);
        }

        let result = self.install_sync(nep_path).await?;
        Ok(Some(InstalledPackage {
            extension_id: result.extension_id,
            version: result.version,
            binary_path: result.binary_path,
            frontend_dir: result.frontend_dir,
            checksum: result.checksum,
            upgraded,
        }))
    }
}

/// Report from sync_nep_cache operation.
#[derive(Debug, Default)]
pub struct SyncReport {
    pub scanned: usize,
    pub installed: usize,
    pub upgraded: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Everything actually installed/upgraded — callers register these with
    /// the runtime (load + ExtensionRecord + config carry-forward).
    pub installed_packages: Vec<InstalledPackage>,
}
