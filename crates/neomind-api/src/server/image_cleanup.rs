//! Image file retention cleanup.
//!
//! Provides cleanup functionality for expired image files stored in
//! `data/images/<device>/<metric>/<timestamp>.<ext>`. This prevents
//! disk space leaks from accumulating image data over time.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Cleanup expired image files from the images directory.
///
/// This function recursively scans the `images_dir` and deletes files
/// whose timestamp (extracted from the filename) is older than the
/// specified retention period. It also cleans up empty directories.
///
/// # Arguments
///
/// * `images_dir` - Path to the images directory (e.g., `data/images`)
/// * `retention_hours` - Retention period in hours (files older than this are deleted)
///
/// # File Format
///
/// Images must be stored as: `<device>/<metric>/<timestamp>.<ext>`
/// where `timestamp` is **seconds** since Unix epoch — matching what
/// `save_image_binary` writes (ingest adapters `mqtt.rs` / `webhook.rs` pass
/// `now.timestamp()`, i.e. seconds). Comparing the filename ts in a different
/// unit than it was written in makes every image parse as 1970 and get deleted.
///
/// # Returns
///
/// Returns the number of files deleted and the number of directories cleaned up.
pub async fn cleanup_expired_images(
    images_dir: &Path,
    retention_hours: u64,
) -> anyhow::Result<(usize, usize)> {
    let retention_duration = Duration::from_secs(retention_hours * 3600);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("Failed to get current time: {}", e))?;

    let cutoff_timestamp_secs = now
        .checked_sub(retention_duration)
        .ok_or_else(|| anyhow::anyhow!("Invalid retention duration: would underflow"))?
        .as_secs() as i64;

    tracing::debug!(
        retention_hours = retention_hours,
        cutoff_timestamp_secs = cutoff_timestamp_secs,
        "Starting image cleanup"
    );

    if !images_dir.exists() {
        tracing::debug!("Images directory does not exist, skipping cleanup");
        return Ok((0, 0));
    }

    let mut files_deleted = 0usize;

    // Collect all files first to avoid borrowing issues
    let files_to_delete: Vec<(PathBuf, i64)> =
        collect_expired_files(images_dir, cutoff_timestamp_secs)?;

    // Delete files
    for (file_path, timestamp_secs) in &files_to_delete {
        match fs::remove_file(file_path) {
            Ok(_) => {
                files_deleted += 1;
                tracing::debug!(
                    file = %file_path.display(),
                    timestamp_secs = timestamp_secs,
                    "Deleted expired image file"
                );
            }
            Err(e) => {
                tracing::warn!(
                    file = %file_path.display(),
                    error = %e,
                    "Failed to delete expired image file"
                );
            }
        }
    }

    // Clean up empty directories
    let dirs_cleaned = cleanup_empty_directories(images_dir)?;

    if files_deleted > 0 || dirs_cleaned > 0 {
        tracing::info!(
            files_deleted = files_deleted,
            dirs_cleaned = dirs_cleaned,
            retention_hours = retention_hours,
            "Image cleanup completed"
        );
    } else {
        tracing::debug!("No expired images found to clean up");
    }

    Ok((files_deleted, dirs_cleaned))
}

/// Collect all expired image files from the images directory.
///
/// This function recursively scans the directory and collects files
/// whose timestamp is older than the cutoff.
fn collect_expired_files(
    images_dir: &Path,
    cutoff_timestamp_secs: i64,
) -> anyhow::Result<Vec<(PathBuf, i64)>> {
    let mut expired_files = Vec::new();

    let entries = match fs::read_dir(images_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                dir = %images_dir.display(),
                error = %e,
                "Failed to read images directory"
            );
            return Ok(expired_files);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            // Recursively process subdirectories
            let sub_files = collect_expired_files(&path, cutoff_timestamp_secs)?;
            expired_files.extend(sub_files);
        } else if file_type.is_file() {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename.starts_with(".tmp.") {
                // Stale temp file from a crashed save_image_binary — a
                // successful save renames .tmp -> final immediately, so a
                // lingering .tmp.* means a crash mid-write. Expire by mtime
                // (the temp name has no parseable timestamp) and only when
                // older than the cutoff, so an in-flight write is never touched.
                if let Some(mtime_secs) = file_mtime_secs(&path) {
                    if mtime_secs < cutoff_timestamp_secs {
                        expired_files.push((path, mtime_secs));
                    }
                }
            } else {
                // Timestamp-named files (save_image_binary: `<ts>[_<n>].<ext>`)
                // expire by their filename timestamp. Files whose name carries
                // no parseable timestamp — e.g. CLI-materialized images named by
                // content hash (`<hash>.<ext>`, from `device get` on a base64
                // image metric) — fall back to mtime so they don't leak on disk
                // forever. Without this, every agent `device get` of a changing
                // camera image would leak one never-cleaned file (edge boxes
                // fill over days/weeks). Same cutoff; a dedup-skip leaves mtime
                // at first materialization, which is the correct expiry point.
                let file_ts =
                    extract_timestamp_from_filename(&path).or_else(|| file_mtime_secs(&path));
                if let Some(timestamp_secs) = file_ts {
                    if timestamp_secs < cutoff_timestamp_secs {
                        expired_files.push((path, timestamp_secs));
                    }
                }
            }
        }
    }

    Ok(expired_files)
}

/// Extract timestamp from filename.
///
/// Expects filename format: `<timestamp>.<ext>` where timestamp is
/// seconds since Unix epoch.
fn extract_timestamp_from_filename(path: &Path) -> Option<i64> {
    let filename = path.file_name()?.to_str()?;

    // Split by extension
    let parts: Vec<&str> = filename.rsplitn(2, '.').collect();
    if parts.len() != 2 || parts.is_empty() {
        return None;
    }

    // The timestamp is the part before the extension. Primary format is
    // `<timestamp>.<ext>`; save_image_binary also emits `<timestamp>_<n>.<ext>`
    // on same-(device,metric,ts) write collisions. Strip the `_<n>` uniquifier
    // so those files are still expired by retention — otherwise they parse as
    // non-numeric and leak on disk forever (collect_expired_files skips them).
    let timestamp_str = parts.get(1)?;
    let ts_segment = timestamp_str.split('_').next()?;

    // Parse as i64 (seconds)
    ts_segment.parse::<i64>().ok()
}

/// File mtime in seconds since Unix epoch (for temp files that have no
/// parseable timestamp in their name).
fn file_mtime_secs(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let age = mtime.duration_since(UNIX_EPOCH).ok()?;
    Some(age.as_secs() as i64)
}

/// Clean up empty directories in the images directory.
///
/// Returns the number of empty directories removed.
fn cleanup_empty_directories(images_dir: &Path) -> anyhow::Result<usize> {
    let mut dirs_cleaned = 0usize;

    // Collect all directories first (bottom-up)
    let mut all_dirs = Vec::new();

    fn collect_all_dirs(dir: &Path, all_dirs: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if file_type.is_dir() {
                all_dirs.push(path.clone());
                collect_all_dirs(&path, all_dirs)?;
            }
        }

        Ok(())
    }

    collect_all_dirs(images_dir, &mut all_dirs)?;

    // Sort by depth (deepest first)
    all_dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    // Remove empty directories
    for dir in all_dirs {
        // Check if directory is empty
        let is_empty = match fs::read_dir(&dir) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => false,
        };

        if is_empty {
            match fs::remove_dir(&dir) {
                Ok(_) => {
                    dirs_cleaned += 1;
                    tracing::debug!(dir = %dir.display(), "Removed empty directory");
                }
                Err(e) => {
                    tracing::warn!(
                        dir = %dir.display(),
                        error = %e,
                        "Failed to remove empty directory"
                    );
                }
            }
        }
    }

    Ok(dirs_cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    fn create_test_image_file(dir: &Path, filename: &str) -> PathBuf {
        let file_path = dir.join(filename);
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"test image data").unwrap();
        file_path
    }

    fn create_test_dir_structure(base_dir: &Path) -> PathBuf {
        let device_dir = base_dir.join("device-001");
        let metric_dir = device_dir.join("image");
        fs::create_dir_all(&metric_dir).unwrap();
        metric_dir
    }

    #[test]
    fn test_extract_timestamp_from_filename() {
        // Valid timestamps
        assert_eq!(
            extract_timestamp_from_filename(Path::new("1634567890000.jpg")),
            Some(1634567890000)
        );
        assert_eq!(
            extract_timestamp_from_filename(Path::new("123456789.png")),
            Some(123456789)
        );
        assert_eq!(
            extract_timestamp_from_filename(Path::new("0.webp")),
            Some(0)
        );

        // Fallback collision format `<ts>_<n>.<ext>` emitted by save_image_binary
        // on same-(device,metric,ts) writes — must still parse so retention can
        // expire them (otherwise they leak forever).
        assert_eq!(
            extract_timestamp_from_filename(Path::new("1700000000_5.jpg")),
            Some(1700000000)
        );
        assert_eq!(
            extract_timestamp_from_filename(Path::new("1700000000_12.png")),
            Some(1700000000)
        );

        // Invalid filenames
        assert_eq!(
            extract_timestamp_from_filename(Path::new("notimestamp.jpg")),
            None
        );
        assert_eq!(extract_timestamp_from_filename(Path::new("text.txt")), None);
        assert_eq!(
            extract_timestamp_from_filename(Path::new("163abc.jpg")),
            None
        );
    }

    #[tokio::test]
    async fn test_cleanup_expired_images() {
        // Create temporary directory
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");
        let metric_dir = create_test_dir_structure(&images_dir);

        // Create test files with different timestamps
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Recent file (within retention period) - 1 hour ago
        let recent_file = create_test_image_file(&metric_dir, &format!("{}.jpg", now - 3600));

        // Expired file (older than retention period) - 5 hours ago
        let expired_file = create_test_image_file(&metric_dir, &format!("{}.jpg", now - 18000));

        // Run cleanup with 2 hours retention
        let result = cleanup_expired_images(&images_dir, 2).await.unwrap();

        // Assert: recent file should exist, expired file should be deleted
        assert!(recent_file.exists());
        assert!(!expired_file.exists());
        assert_eq!(result, (1, 0)); // 1 file deleted, 0 dirs cleaned
    }

    #[tokio::test]
    async fn test_cleanup_with_empty_directories() {
        // Create temporary directory
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");

        // Create nested directory structure with only expired files
        let device_dir = images_dir.join("device-002");
        let metric_dir = device_dir.join("temperature");
        fs::create_dir_all(&metric_dir).unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let expired_file = create_test_image_file(&metric_dir, &format!("{}.jpg", now - 5_000)); // ~1.4 hours ago

        // Run cleanup with 1 hour retention to ensure file is deleted
        let result = cleanup_expired_images(&images_dir, 1).await.unwrap();

        // Assert: file should be deleted and empty directories should be cleaned
        assert!(!expired_file.exists());
        assert!(!metric_dir.exists()); // empty metric dir should be removed
        assert!(!device_dir.exists()); // empty device dir should be removed
        assert_eq!(result.0, 1); // 1 file deleted
        assert!(result.1 > 0); // some dirs cleaned
    }

    #[tokio::test]
    async fn test_cleanup_nonexistent_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let nonexistent_dir = temp_dir.path().join("nonexistent");

        let result = cleanup_expired_images(&nonexistent_dir, 72).await.unwrap();
        assert_eq!(result, (0, 0));
    }

    #[tokio::test]
    async fn test_cleanup_no_expired_files() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");
        let metric_dir = create_test_dir_structure(&images_dir);

        // Create only recent files
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let recent_file = create_test_image_file(&metric_dir, &format!("{}.jpg", now));

        // Run cleanup with long retention
        let result = cleanup_expired_images(&images_dir, 720).await.unwrap(); // 30 days

        // Assert: no files should be deleted
        assert!(recent_file.exists());
        assert_eq!(result, (0, 0));
    }

    #[tokio::test]
    async fn test_cleanup_multiple_devices() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Create multiple devices with mixed file ages
        for device_id in 1..=3 {
            let device_dir = images_dir.join(format!("device-{:03}", device_id));
            let metric_dir = device_dir.join("image");
            fs::create_dir_all(&metric_dir).unwrap();

            // Recent file (1 hour ago)
            create_test_image_file(&metric_dir, &format!("{}.jpg", now - 3600));

            // Expired file (5 hours ago)
            create_test_image_file(&metric_dir, &format!("{}.jpg", now - 18000));
        }

        // Run cleanup with 2 hours retention
        let result = cleanup_expired_images(&images_dir, 2).await.unwrap();

        // Assert: 3 expired files deleted, 3 recent files remain
        assert_eq!(result.0, 3); // 3 files deleted
    }

    #[tokio::test]
    async fn test_cleanup_removes_stale_tmp_files() {
        // A .tmp.* file left by a crashed save_image_binary has no parseable
        // timestamp; cleanup must still expire it by mtime. A recent .tmp
        // (in-flight write) must be left alone.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");
        let metric_dir = images_dir.join("cam").join("image");
        std::fs::create_dir_all(&metric_dir).unwrap();

        // Stale .tmp (old mtime) — should be deleted.
        let stale_tmp = metric_dir.join(".tmp.1700000000");
        std::fs::write(&stale_tmp, b"stale").unwrap();
        let old = SystemTime::now() - Duration::from_secs(100_000); // ~28h ago
        let f = std::fs::File::open(&stale_tmp).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        drop(f);

        // Regular expired image — should also be deleted.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let expired = create_test_image_file(&metric_dir, &format!("{}.jpg", now - 18000));

        // Recent .tmp (in-flight write) — must NOT be deleted.
        let fresh_tmp = metric_dir.join(".tmp.9999");
        std::fs::write(&fresh_tmp, b"fresh").unwrap();

        let result = cleanup_expired_images(&images_dir, 2).await.unwrap();
        // 2 deleted: stale .tmp + expired image. fresh .tmp kept.
        assert!(!stale_tmp.exists(), "stale .tmp should be deleted");
        assert!(!expired.exists(), "expired image should be deleted");
        assert!(fresh_tmp.exists(), "fresh .tmp (in-flight) must be kept");
        assert_eq!(result.0, 2);
    }

    #[tokio::test]
    async fn test_cleanup_removes_hash_named_files_by_mtime() {
        // Regression: the CLI's materialize_data_url (agent `device get` on a
        // base64 image metric) writes files named by content hash
        // (`<hex>.<ext>`) — no parseable timestamp. extract_timestamp_from_filename
        // returns None for them, so pre-fix they were skipped forever (disk leak
        // on edge boxes that query changing camera images). collect_expired_files
        // must fall back to mtime for such files. A fresh one must be kept.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");
        let metric_dir = images_dir.join("cam").join("image");
        std::fs::create_dir_all(&metric_dir).unwrap();

        // Old hash-named file — must be deleted via mtime fallback.
        let stale_hash = create_test_image_file(&metric_dir, "a1b2c3d4e5f6a7b8.jpg");
        let old = SystemTime::now() - Duration::from_secs(100_000); // ~28h ago
        let f = std::fs::File::open(&stale_hash).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        drop(f);

        // Fresh hash-named file — must be kept (within retention).
        let fresh_hash = create_test_image_file(&metric_dir, "0fedcba987654321.jpg");

        let result = cleanup_expired_images(&images_dir, 2).await.unwrap();
        assert!(!stale_hash.exists(), "old hash-named file should expire by mtime");
        assert!(fresh_hash.exists(), "fresh hash-named file must be kept");
        assert_eq!(result.0, 1);
    }

    #[tokio::test]
    async fn test_cleanup_keeps_new_image_stamped_in_seconds() {
        // Regression (real production bug): ingest adapters stamp image
        // filenames with a SECOND-granularity `now.timestamp()` (mqtt.rs /
        // webhook.rs), NOT milliseconds. Pre-fix, cleanup treated the filename
        // ts as milliseconds, so a brand-new image (ts ~= 1.75e9 seconds) parsed
        // as 1970-01-21 and was ALWAYS deleted → every recent image returned
        // "image not found" on download. Cleanup must interpret the filename ts
        // as SECONDS to match what save_image_binary actually writes.
        let temp_dir = tempfile::TempDir::new().unwrap();
        let images_dir = temp_dir.path().join("images");
        let metric_dir = create_test_dir_structure(&images_dir);

        // Brand-new image, filename ts in SECONDS (exactly as production writes).
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let fresh = create_test_image_file(&metric_dir, &format!("{}.jpg", now_secs));

        // 72h retention — a seconds-old image MUST survive.
        let (deleted, _) = cleanup_expired_images(&images_dir, 72).await.unwrap();

        assert!(
            fresh.exists(),
            "new second-granularity image must NOT be deleted"
        );
        assert_eq!(
            deleted, 0,
            "nothing should be deleted for a brand-new image"
        );
    }
}
