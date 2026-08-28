//! Memory scheduler for background maintenance tasks
//!
//! Runs periodic tasks for:
//! - Temp file cleanup (session directories)

use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use neomind_storage::{MarkdownMemoryStore, MemoryConfig};

/// Memory scheduler for background tasks
pub struct MemoryScheduler {
    store: Arc<RwLock<MarkdownMemoryStore>>,
    config: MemoryConfig,
    job_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MemoryScheduler {
    /// Create a new scheduler
    pub fn new(store: Arc<RwLock<MarkdownMemoryStore>>, config: MemoryConfig) -> Self {
        Self {
            store,
            config,
            job_handle: None,
        }
    }

    /// Start background jobs
    pub fn start(&mut self) {
        if !self.config.enabled {
            info!("Memory system disabled, not starting scheduler");
            return;
        }

        let store = self.store.clone();
        let config = self.config.clone();

        self.job_handle = Some(tokio::spawn(async move {
            // Run cleanup every 24 hours
            let mut cleanup_timer = interval(Duration::from_secs(86400));

            info!("Memory scheduler started (temp file cleanup every 24h)");

            // Trigger first cleanup immediately
            cleanup_timer.tick().await;

            loop {
                cleanup_timer.tick().await;
                if let Err(e) = Self::run_temp_cleanup_job(&store, &config).await {
                    error!(error = %e, "Temp cleanup job failed");
                }
            }
        }));
    }

    /// Temp File Cleanup: deletes session directories older than temp_file_ttl_days
    async fn run_temp_cleanup_job(
        store: &Arc<RwLock<MarkdownMemoryStore>>,
        config: &MemoryConfig,
    ) -> Result<(), String> {
        let sessions_dir = {
            let store_guard = store.read().await;
            store_guard.base_path().join("sessions")
        };

        if !sessions_dir.exists() {
            info!("Sessions directory does not exist, skipping cleanup");
            return Ok(());
        }

        let ttl_secs = config.temp_file_ttl_days * 86400;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Failed to get current time: {}", e))?
            .as_secs();

        let mut deleted_count = 0;
        let mut error_count = 0;

        let entries = fs::read_dir(&sessions_dir)
            .map_err(|e| format!("Failed to read sessions directory: {}", e))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    error_count += 1;
                    warn!(error = %e, "Failed to read session directory entry");
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    error_count += 1;
                    warn!(path = %path.display(), error = %e, "Failed to get metadata");
                    continue;
                }
            };

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .unwrap_or_default()
                .as_secs();

            let age_secs = now.saturating_sub(modified);

            if age_secs > ttl_secs {
                match fs::remove_dir_all(&path) {
                    Ok(_) => {
                        deleted_count += 1;
                        info!(path = %path.display(), age_days = age_secs / 86400, "Deleted old session directory");
                    }
                    Err(e) => {
                        error_count += 1;
                        warn!(path = %path.display(), error = %e, "Failed to delete session directory");
                    }
                }
            }
        }

        info!(
            deleted_count = deleted_count,
            error_count = error_count,
            ttl_days = config.temp_file_ttl_days,
            "Temp cleanup job completed"
        );

        Ok(())
    }

    /// Stop background jobs
    pub fn stop(&mut self) {
        if let Some(handle) = self.job_handle.take() {
            handle.abort();
            info!("Memory scheduler stopped");
        }
    }

    /// Check if scheduler is running
    pub fn is_running(&self) -> bool {
        self.job_handle.is_some()
    }
}

impl Drop for MemoryScheduler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scheduler_creation() {
        let temp = TempDir::new().unwrap();
        let config = MemoryConfig {
            storage_path: temp.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let store = Arc::new(RwLock::new(MarkdownMemoryStore::new(temp.path())));
        let scheduler = MemoryScheduler::new(store, config);

        assert!(!scheduler.is_running());
    }

    #[tokio::test]
    async fn test_scheduler_start_stop() {
        let temp = TempDir::new().unwrap();
        let config = MemoryConfig {
            storage_path: temp.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let store = Arc::new(RwLock::new(MarkdownMemoryStore::new(temp.path())));
        let mut scheduler = MemoryScheduler::new(store, config);

        scheduler.start();
        assert!(scheduler.is_running());

        scheduler.stop();
        assert!(!scheduler.is_running());
    }

    #[test]
    fn test_disabled_scheduler() {
        let temp = TempDir::new().unwrap();
        let config = MemoryConfig {
            storage_path: temp.path().to_string_lossy().to_string(),
            enabled: false,
            ..Default::default()
        };

        let store = Arc::new(RwLock::new(MarkdownMemoryStore::new(temp.path())));
        let mut scheduler = MemoryScheduler::new(store, config);

        scheduler.start();
        assert!(!scheduler.is_running()); // Should not start when disabled
    }
}
