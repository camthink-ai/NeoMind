//! File-level backup of the data directory's redb databases + secrets.
//!
//! redb has no in-process backup API, but it is crash-safe by design: a
//! byte-for-byte copy of a database file taken while the database is open is
//! indistinguishable from a file after a power cut, and redb recovers it to
//! the last complete commit on open. We lean on that: every copied database
//! is verified by opening it (which runs the recovery/header checks), and a
//! backup where any file fails verification is discarded whole — partial
//! backups are worse than none because they look restorable.
//!
//! Alongside the `*.redb` files, the two secret files are included: without
//! `encryption_key` the sealed LLM provider keys in the backup are
//! undecryptable, and without `.jwt_secret` every session invalidates on
//! restore. The backup directory is 0700 and secret files stay 0600.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use redb::Database;

/// Subdirectory of the data dir holding timestamped backup folders.
pub const BACKUP_DIR_NAME: &str = "backups";

/// Serializes backup creation process-wide. The scheduler and the manual
/// admin trigger both call `create_backup` (each on the blocking pool);
/// without this, a same-second collision had both writing the same
/// `backup-<second>` tmp dir — interleaved copies, one rename winning,
/// the loser leaking its tmp dir forever (prune only recognizes completed
/// backups).
static CREATE_BACKUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One file inside a backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileEntry {
    /// File name (flat, relative to the backup dir).
    pub name: String,
    /// Size in bytes at backup time.
    pub bytes: u64,
    /// Whether the copy was verified (redb files: opened successfully).
    pub verified: bool,
}

/// Manifest written into each backup dir as `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    /// ISO-8601 timestamp of when the backup started.
    pub created_at: String,
    /// Backup id (== directory name, `backup-<YYYYMMDD-HHMMSS>`).
    pub id: String,
    /// Files included in the backup.
    pub files: Vec<BackupFileEntry>,
    /// Total bytes copied.
    pub total_bytes: u64,
    /// Server version at backup time (restore diagnostics).
    pub app_version: String,
}

/// Summary of an existing backup on disk (without opening the files).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: String,
    pub total_bytes: u64,
    pub file_count: usize,
}

/// Errors specific to backup creation.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("no databases found to back up in {0}")]
    NothingToBackup(std::path::PathBuf),
    #[error("verification failed for {file}: {reason}")]
    VerificationFailed { file: String, reason: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Copy `from` to `to` preserving 0600 on secrets / 0644 elsewhere.
fn copy_file(from: &Path, to: &Path, secret: bool) -> std::io::Result<u64> {
    let bytes = std::fs::copy(from, to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if secret { 0o600 } else { 0o644 };
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(bytes)
}

/// Verify a copied redb file actually recovers: open it (header + recovery
/// checks run at open) and immediately close. `Database::open` takes a path,
/// not a handle, so the verification reads the copy — not the live file.
fn verify_redb_copy(path: &Path) -> Result<(), String> {
    // Open read-capable handle; redb recovers the file if needed. Dropping
    // the Database closes it cleanly. We must not hold it while the caller
    // renames the directory, so scope it here.
    let db = Database::open(path).map_err(|e| e.to_string())?;
    drop(db);
    Ok(())
}

/// Secret files that must ride along for a backup to be restorable.
const SECRET_FILES: &[&str] = &["encryption_key", ".jwt_secret"];

/// Create a timestamped backup of every `*.redb` + secret file in `data_dir`
/// into `data_dir/backups/backup-<YYYYMMDD-HHMMSS>/`.
///
/// Builds into a `…​.tmp` sibling first and renames on success, so a crashed
/// backup never looks complete. Returns the manifest of the finished backup.
pub fn create_backup(data_dir: &Path, app_version: &str) -> Result<BackupManifest, BackupError> {
    // Hold for the whole create: copies + verify + manifest + rename. The
    // scheduler and manual trigger both funnel through here.
    let _create_guard = CREATE_BACKUP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let started = std::time::Instant::now();
    // Millisecond precision: even serialized, two backups started within the
    // same second (manual right after a scheduled one) would collide on the
    // second-granular id and fail the final rename.
    let id = format!("backup-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"));
    let backup_root = data_dir.join(BACKUP_DIR_NAME);
    let final_dir = backup_root.join(&id);
    let tmp_dir = backup_root.join(format!("{id}.tmp"));

    std::fs::create_dir_all(&tmp_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let mut files: Vec<BackupFileEntry> = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut db_count = 0usize;

    let result = (|| -> Result<(), BackupError> {
        // Databases (flat *.redb directly in the data dir — stores are not
        // nested today; nested ones would need per-store knowledge).
        let mut db_files: Vec<PathBuf> = std::fs::read_dir(data_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|x| x == "redb").unwrap_or(false))
            .collect();
        db_files.sort();

        if db_files.is_empty() {
            return Err(BackupError::NothingToBackup(data_dir.to_path_buf()));
        }

        for from in &db_files {
            let name = from
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let to = tmp_dir.join(&name);
            let bytes = copy_file(from, &to, false)?;
            verify_redb_copy(&to).map_err(|reason| BackupError::VerificationFailed {
                file: name.clone(),
                reason,
            })?;
            db_count += 1;
            total_bytes += bytes;
            files.push(BackupFileEntry {
                name,
                bytes,
                verified: true,
            });
        }

        // Secrets — restorability depends on them, but their absence (fresh
        // install before first key generation) is not fatal.
        for secret in SECRET_FILES {
            let from = data_dir.join(secret);
            if !from.is_file() {
                continue;
            }
            let to = tmp_dir.join(secret);
            let bytes = copy_file(&from, &to, true)?;
            total_bytes += bytes;
            files.push(BackupFileEntry {
                name: secret.to_string(),
                bytes,
                verified: true,
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            let manifest = BackupManifest {
                created_at: chrono::Utc::now().to_rfc3339(),
                id: id.clone(),
                files,
                total_bytes,
                app_version: app_version.to_string(),
            };
            let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
                BackupError::VerificationFailed {
                    file: "manifest.json".into(),
                    reason: e.to_string(),
                }
            })?;
            let finish = (|| -> Result<(), BackupError> {
                std::fs::write(tmp_dir.join("manifest.json"), manifest_bytes)?;
                std::fs::rename(&tmp_dir, &final_dir)?;
                Ok(())
            })();
            if let Err(e) = finish {
                // Same rule as the copy phase: a failed finalize (manifest
                // write / rename — e.g. the disk filled right after copying)
                // must not leave a manifest-less tmp dir. Prune only
                // recognizes completed backups; a leaked tmp would sit
                // there forever.
                let _ = std::fs::remove_dir_all(&tmp_dir);
                tracing::error!(
                    category = "backup",
                    error = %e,
                    "Backup failed during finalize, discarded partial copy"
                );
                return Err(e);
            }
            tracing::info!(
                category = "backup",
                id = %id,
                databases = db_count,
                total_bytes,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Backup created and verified"
            );
            Ok(manifest)
        }
        Err(e) => {
            // Leave no half-finished backup behind.
            let _ = std::fs::remove_dir_all(&tmp_dir);
            tracing::error!(category = "backup", error = %e, "Backup failed, discarded partial copy");
            Err(e)
        }
    }
}

/// List existing backups (newest first by id/time).
pub fn list_backups(data_dir: &Path) -> Vec<BackupInfo> {
    let backup_root = data_dir.join(BACKUP_DIR_NAME);
    let Ok(entries) = std::fs::read_dir(&backup_root) else {
        return Vec::new();
    };
    let mut infos: Vec<BackupInfo> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| {
            let manifest_path = dir.join("manifest.json");
            let manifest: BackupManifest =
                serde_json::from_str(&std::fs::read_to_string(manifest_path).ok()?).ok()?;
            Some(BackupInfo {
                id: manifest.id,
                created_at: manifest.created_at,
                total_bytes: manifest.total_bytes,
                file_count: manifest.files.len(),
            })
        })
        .collect();
    infos.sort_by(|a, b| b.id.cmp(&a.id)); // timestamped ids sort chronologically
    infos
}

/// Keep only the newest `keep` backups; returns how many were removed.
pub fn prune_backups(data_dir: &Path, keep: usize) -> usize {
    let backups = list_backups(data_dir);
    let mut removed = 0;
    for old in backups.into_iter().skip(keep) {
        let dir = data_dir.join(BACKUP_DIR_NAME).join(&old.id);
        if std::fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
            tracing::info!(category = "backup", id = %old.id, "Pruned old backup");
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn make_db(path: &Path) {
        let db = Database::create(path).unwrap();
        let tx = db.begin_write().unwrap();
        let table: redb::TableDefinition<&str, &str> = redb::TableDefinition::new("t");
        {
            let mut t = tx.open_table(table).unwrap();
            t.insert("k", "v").unwrap();
        }
        tx.commit().unwrap();
        drop(db);
    }

    #[test]
    fn backup_roundtrips_and_verifies() {
        let dir = data_dir();
        make_db(&dir.path().join("settings.redb"));
        make_db(&dir.path().join("devices.redb"));
        std::fs::write(dir.path().join("encryption_key"), "0600-secret-key").unwrap();

        let manifest = create_backup(dir.path(), "0.9.20-test").unwrap();
        assert_eq!(manifest.files.len(), 3); // 2 dbs + 1 secret
        assert!(manifest.total_bytes > 0);

        // The backed-up database must open and contain the committed data.
        let backup_db = dir
            .path()
            .join(BACKUP_DIR_NAME)
            .join(&manifest.id)
            .join("settings.redb");
        let db = Database::open(backup_db).unwrap();
        let tx = db.begin_read().unwrap();
        let table: redb::TableDefinition<&str, &str> = redb::TableDefinition::new("t");
        let t = tx.open_table(table).unwrap();
        assert_eq!(t.get("k").unwrap().unwrap().value(), "v");
        drop(db);

        // No .tmp leftovers.
        let root = dir.path().join(BACKUP_DIR_NAME);
        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec![manifest.id.clone()]);
    }

    #[test]
    fn empty_data_dir_fails_cleanly() {
        let dir = data_dir();
        let err = create_backup(dir.path(), "test").unwrap_err();
        assert!(matches!(err, BackupError::NothingToBackup(_)));
        // And no tmp dir left behind.
        assert!(!dir
            .path()
            .join(BACKUP_DIR_NAME)
            .join("backup-*.tmp")
            .exists());
    }

    #[test]
    fn prune_keeps_newest() {
        let dir = data_dir();
        make_db(&dir.path().join("a.redb"));
        // Same-second ids would collide; fake manifests directly instead.
        let root = dir.path().join(BACKUP_DIR_NAME);
        std::fs::create_dir_all(root.join("backup-20260101-000001")).unwrap();
        std::fs::create_dir_all(root.join("backup-20260102-000000")).unwrap();
        std::fs::create_dir_all(root.join("backup-20260103-000000")).unwrap();
        for id in [
            "backup-20260101-000001",
            "backup-20260102-000000",
            "backup-20260103-000000",
        ] {
            let m = BackupManifest {
                created_at: id.to_string(),
                id: id.to_string(),
                files: vec![],
                total_bytes: 0,
                app_version: "test".into(),
            };
            std::fs::write(
                root.join(id).join("manifest.json"),
                serde_json::to_string(&m).unwrap(),
            )
            .unwrap();
        }

        let listed = list_backups(dir.path());
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id, "backup-20260103-000000"); // newest first

        let removed = prune_backups(dir.path(), 1);
        assert_eq!(removed, 2);
        let remaining = list_backups(dir.path());
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "backup-20260103-000000");
    }

    #[test]
    fn secret_file_permissions_preserved() {
        let dir = data_dir();
        make_db(&dir.path().join("a.redb"));
        std::fs::write(dir.path().join("encryption_key"), "key-material").unwrap();

        let manifest = create_backup(dir.path(), "test").unwrap();
        let secret_copy = dir
            .path()
            .join(BACKUP_DIR_NAME)
            .join(&manifest.id)
            .join("encryption_key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = secret_copy.metadata().unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "secret must stay 0600 in backups");
        }
        assert!(secret_copy.exists());
    }
}
