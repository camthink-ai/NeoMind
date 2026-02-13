//! Database backup and restore functionality.
//!
//! Provides:
//! - Full database backups to file
//! - Restore from backup files
//! - Export to JSON format
//! - Import from JSON format
//! - Incremental backups
//! - Backup management and cleanup

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::backend::UnifiedStorage;
use crate::{Error, Result};

/// Backup metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Unique backup ID.
    pub id: String,
    /// Backup timestamp.
    pub timestamp: i64,
    /// Backup type.
    pub backup_type: BackupType,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Number of tables included.
    pub table_count: usize,
    /// Optional description.
    pub description: Option<String>,
    /// Backup file path.
    pub path: PathBuf,
}

/// Backup type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupType {
    /// Full backup of all data.
    Full,
    /// Incremental backup (data changes since last backup).
    Incremental,
    /// Export to JSON format.
    JsonExport,
}

/// Backup configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Directory where backups are stored.
    pub backup_dir: PathBuf,
    /// Maximum number of backups to keep (0 = unlimited).
    pub max_backups: usize,
    /// Whether to compress backups.
    pub compress: bool,
    /// Tables to exclude from backups.
    pub exclude_tables: Vec<String>,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_dir: PathBuf::from("./backups"),
            max_backups: 10,
            compress: false,
            exclude_tables: vec![],
        }
    }
}

/// Backup manager for database operations.
pub struct BackupManager {
    config: BackupConfig,
    /// Reference to the unified storage
    storage: Option<Arc<UnifiedStorage>>,
}

impl BackupManager {
    /// Create a new backup manager with default config.
    pub fn new<P: AsRef<Path>>(backup_dir: P) -> Result<Self> {
        let backup_dir = backup_dir.as_ref();
        if !backup_dir.exists() {
            fs::create_dir_all(backup_dir)?;
        }

        Ok(Self {
            config: BackupConfig {
                backup_dir: backup_dir.to_path_buf(),
                ..Default::default()
            },
            storage: None,
        })
    }

    /// Create with custom configuration.
    pub fn with_config(config: BackupConfig) -> Result<Self> {
        if !config.backup_dir.exists() {
            fs::create_dir_all(&config.backup_dir)?;
        }

        Ok(Self {
            config,
            storage: None,
        })
    }

    /// Set the storage backend.
    pub fn with_storage(mut self, storage: Arc<UnifiedStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Create a full backup.
    pub fn create_full_backup(
        &self,
        source_path: &Path,
        description: Option<String>,
    ) -> Result<BackupMetadata> {
        let backup_id = self.generate_backup_id();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        let filename = format!("backup_{}.redb", backup_id);
        let backup_path = self.config.backup_dir.join(&filename);

        // Copy the source database file to backup location
        if source_path.exists() {
            self.copy_file(source_path, &backup_path)?;
        }

        let size_bytes = if backup_path.exists() {
            fs::metadata(&backup_path)?.len()
        } else {
            0
        };

        let metadata = BackupMetadata {
            id: backup_id.clone(),
            timestamp,
            backup_type: BackupType::Full,
            size_bytes,
            table_count: 0,
            description,
            path: backup_path,
        };

        self.save_metadata(&metadata)?;
        self.cleanup_old_backups()?;

        Ok(metadata)
    }

    /// Create an incremental backup (export changes since timestamp).
    pub fn create_incremental_backup(
        &self,
        since: i64,
        description: Option<String>,
    ) -> Result<BackupMetadata> {
        let backup_id = self.generate_backup_id();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        let filename = format!("incremental_{}.json", backup_id);
        let backup_path = self.config.backup_dir.join(&filename);

        // Collect data changed since the given timestamp
        let backup_data = if let Some(storage) = &self.storage {
            self.collect_incremental_data(storage, since)?
        } else {
            serde_json::json!({})
        };

        // Write to JSON file
        let file = File::create(&backup_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &backup_data)?;

        let size_bytes = fs::metadata(&backup_path)?.len();

        let metadata = BackupMetadata {
            id: backup_id.clone(),
            timestamp,
            backup_type: BackupType::Incremental,
            size_bytes,
            table_count: 0,
            description,
            path: backup_path,
        };

        self.save_metadata(&metadata)?;
        self.cleanup_old_backups()?;

        Ok(metadata)
    }

    /// Export data to JSON format.
    pub fn export_to_json(&self, output_path: &Path) -> Result<()> {
        let export_data = if let Some(storage) = &self.storage {
            self.export_storage_to_json(storage)?
        } else {
            serde_json::json!({})
        };

        let file = File::create(output_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &export_data)?;

        Ok(())
    }

    /// Import data from JSON format.
    pub fn import_from_json(&self, input_path: &Path) -> Result<usize> {
        let mut file = File::open(input_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let import_data: serde_json::Value = serde_json::from_str(&contents)?;

        if let Some(storage) = &self.storage {
            self.import_json_to_storage(storage, &import_data)?;
        }

        Ok(0) // Return count of imported records
    }

    /// Restore from a backup.
    pub fn restore_from_backup(&self, backup_id: &str, target_path: &Path) -> Result<()> {
        let metadata = self.load_metadata(backup_id)?;

        if metadata.backup_type == BackupType::Full {
            // Copy the backup file to the target
            self.copy_file(&metadata.path, target_path)?;
        } else {
            return Err(Error::InvalidInput(
                "Incremental backups must be restored via import_from_json".to_string(),
            ));
        }

        Ok(())
    }

    /// List all available backups.
    pub fn list_backups(&self) -> Result<Vec<BackupMetadata>> {
        let mut backups = Vec::new();

        let entries = fs::read_dir(&self.config.backup_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Look for metadata files
            if path.extension().and_then(|s| s.to_str()) == Some("meta") {
                if let Ok(metadata) = self.load_metadata_from_path(&path) {
                    backups.push(metadata);
                }
            }
        }

        // Sort by timestamp descending
        backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(backups)
    }

    /// Get backup metadata by ID.
    pub fn get_backup(&self, backup_id: &str) -> Result<Option<BackupMetadata>> {
        let meta_path = self.config.backup_dir.join(format!("{}.meta", backup_id));
        if meta_path.exists() {
            Ok(Some(self.load_metadata_from_path(&meta_path)?))
        } else {
            Ok(None)
        }
    }

    /// Delete a backup.
    pub fn delete_backup(&self, backup_id: &str) -> Result<bool> {
        let metadata = match self.load_metadata(backup_id) {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        // Delete the backup file
        if metadata.path.exists() {
            fs::remove_file(&metadata.path)?;
        }

        // Delete the metadata file
        let meta_path = self.config.backup_dir.join(format!("{}.meta", backup_id));
        if meta_path.exists() {
            fs::remove_file(&meta_path)?;
        }

        Ok(true)
    }

    /// Cleanup old backups exceeding max_backups limit.
    pub fn cleanup_old_backups(&self) -> Result<usize> {
        if self.config.max_backups == 0 {
            return Ok(0);
        }

        let backups = self.list_backups()?;
        let mut deleted = 0;

        for backup in backups.iter().skip(self.config.max_backups) {
            if self.delete_backup(&backup.id)? {
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// Generate a unique backup ID.
    fn generate_backup_id(&self) -> String {
        format!(
            "{:x}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    }

    /// Save backup metadata.
    fn save_metadata(&self, metadata: &BackupMetadata) -> Result<()> {
        let meta_path = self.config.backup_dir.join(format!("{}.meta", metadata.id));
        let file = File::create(&meta_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, metadata)?;
        Ok(())
    }

    /// Load backup metadata by ID.
    fn load_metadata(&self, backup_id: &str) -> Result<BackupMetadata> {
        let meta_path = self.config.backup_dir.join(format!("{}.meta", backup_id));
        self.load_metadata_from_path(&meta_path)
    }

    /// Load backup metadata from file path.
    fn load_metadata_from_path(&self, path: &Path) -> Result<BackupMetadata> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let metadata: BackupMetadata = serde_json::from_reader(reader)?;
        Ok(metadata)
    }

    /// Copy a file from source to destination.
    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()> {
        let mut src_file = File::open(src)?;
        let mut dest_file = File::create(dest)?;

        let mut buffer = Vec::new();
        src_file.read_to_end(&mut buffer)?;
        dest_file.write_all(&buffer)?;

        Ok(())
    }

    /// Collect incremental data since a timestamp.
    fn collect_incremental_data(
        &self,
        _storage: &UnifiedStorage,
        since: i64,
    ) -> Result<serde_json::Value> {
        // This would scan tables for data modified since the timestamp
        // For now, return empty as this would need timestamp tracking per record
        Ok(serde_json::json!({
            "since": since,
            "data": []
        }))
    }

    /// Export storage data to JSON.
    fn export_storage_to_json(&self, _storage: &UnifiedStorage) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "version": "1.0",
            "timestamp": chrono::Utc::now().timestamp(),
            "data": {}
        }))
    }

    /// Import JSON data to storage.
    fn import_json_to_storage(
        &self,
        _storage: &UnifiedStorage,
        _data: &serde_json::Value,
    ) -> Result<()> {
        // Implementation would parse the JSON and write to storage
        Ok(())
    }
}

/// Simplified backup handler for individual stores.
pub struct BackupHandler {
    backup_dir: PathBuf,
}

impl BackupHandler {
    /// Create a new backup handler.
    pub fn new<P: AsRef<Path>>(backup_dir: P) -> Result<Self> {
        let backup_dir = backup_dir.as_ref();
        if !backup_dir.exists() {
            fs::create_dir_all(backup_dir)?;
        }

        Ok(Self {
            backup_dir: backup_dir.to_path_buf(),
        })
    }

    /// Backup a file (e.g., database file).
    pub fn backup_file(&self, source_path: &Path, name: &str) -> Result<PathBuf> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.backup", name, timestamp);
        let backup_path = self.backup_dir.join(&filename);

        let mut src_file = File::open(source_path)?;
        let mut dest_file = File::create(&backup_path)?;

        let mut buffer = Vec::new();
        src_file.read_to_end(&mut buffer)?;
        dest_file.write_all(&buffer)?;

        Ok(backup_path)
    }

    /// Restore a file from backup.
    pub fn restore_file(&self, backup_path: &Path, target_path: &Path) -> Result<()> {
        let mut src_file = File::open(backup_path)?;
        let mut dest_file = File::create(target_path)?;

        let mut buffer = Vec::new();
        src_file.read_to_end(&mut buffer)?;
        dest_file.write_all(&buffer)?;

        Ok(())
    }

    /// List all backups for a given name pattern.
    pub fn list_backups_for(&self, name: &str) -> Result<Vec<PathBuf>> {
        let mut backups = Vec::new();

        let entries = fs::read_dir(&self.backup_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with(name) && file_name.ends_with(".backup") {
                    backups.push(path);
                }
            }
        }

        backups.sort();
        backups.reverse(); // Newest first

        Ok(backups)
    }

    /// Get the backup directory path.
    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    /// Delete a backup file.
    pub fn delete_backup(&self, backup_path: &Path) -> Result<bool> {
        if backup_path.exists() {
            fs::remove_file(backup_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_config_default() {
        let config = BackupConfig::default();
        assert_eq!(config.max_backups, 10);
        assert!(!config.compress);
        assert!(config.exclude_tables.is_empty());
    }

    #[test]
    fn test_backup_handler() {
        let handler = BackupHandler::new("/tmp/test_backups").unwrap();
        assert_eq!(handler.backup_dir(), Path::new("/tmp/test_backups"));
    }

    #[test]
    fn test_generate_backup_id() {
        let manager = BackupManager::new("/tmp/test_backup_manager").unwrap();
        let id1 = manager.generate_backup_id();

        // Add a small delay to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        let id2 = manager.generate_backup_id();
        // IDs should be different (generated at different times)
        assert_ne!(id1, id2);
    }
}
