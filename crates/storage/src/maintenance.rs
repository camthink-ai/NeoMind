//! Maintenance and cleanup tasks for storage.
//!
//! Provides:
//! - Scheduled cleanup of expired data
//! - Retention policy enforcement
//! - Automatic cleanup of old history entries
//! - Maintenance task scheduler

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::Result;
use crate::backend::UnifiedStorage;

/// Maintenance task configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceConfig {
    /// Enable automatic maintenance.
    pub enabled: bool,

    /// Interval between maintenance runs (in seconds).
    pub interval_secs: u64,

    /// Time series retention policy (hours, None = keep forever).
    pub timeseries_retention_hours: Option<u64>,

    /// Memory entry retention (hours, None = keep forever).
    pub memory_retention_hours: Option<u64>,

    /// Maximum number of history entries to keep per item.
    pub max_history_entries: usize,
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 3600,                      // 1 hour
            timeseries_retention_hours: Some(24 * 7), // 7 days
            memory_retention_hours: Some(24 * 30),    // 30 days
            max_history_entries: 1000,
        }
    }
}

/// Result of a maintenance run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceResult {
    /// When the maintenance run started.
    pub started_at: i64,
    /// When the maintenance run completed.
    pub completed_at: i64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Number of timeseries points deleted.
    pub timeseries_deleted: usize,
    /// Number of memory entries deleted.
    pub memory_deleted: usize,
    /// Number of config history entries deleted.
    pub config_history_deleted: usize,
    /// Any errors that occurred.
    pub errors: Vec<String>,
}

/// Maintenance scheduler for automatic cleanup tasks.
pub struct MaintenanceScheduler {
    config: MaintenanceConfig,
    storage: Option<Arc<UnifiedStorage>>,
    running: Arc<RwLock<bool>>,
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl MaintenanceScheduler {
    /// Create a new maintenance scheduler.
    pub fn new(config: MaintenanceConfig) -> Self {
        Self {
            config,
            storage: None,
            running: Arc::new(RwLock::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the storage backend.
    pub fn with_storage(mut self, storage: Arc<UnifiedStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Start the maintenance scheduler.
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(()); // Already running
        }
        *running = true;
        drop(running);

        let interval = Duration::from_secs(self.config.interval_secs);
        let running_flag = self.running.clone();
        let storage = self.storage.clone();
        let config = self.config.clone();

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;

                // Check if still running
                {
                    let r = running_flag.read().await;
                    if !*r {
                        break;
                    }
                }

                // Run maintenance
                if let Some(storage) = &storage {
                    let _ = Self::run_maintenance(storage, &config).await;
                }
            }
        });

        let mut task_handle = self.task_handle.write().await;
        *task_handle = Some(handle);

        Ok(())
    }

    /// Stop the maintenance scheduler.
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        // Wait for the task to complete
        let mut task_handle = self.task_handle.write().await;
        if let Some(handle) = task_handle.take() {
            drop(task_handle);
            handle.await.ok();
        }

        Ok(())
    }

    /// Check if the scheduler is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Run a single maintenance pass.
    pub async fn run_maintenance(
        storage: &UnifiedStorage,
        config: &MaintenanceConfig,
    ) -> Result<MaintenanceResult> {
        let started_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        let mut result = MaintenanceResult {
            started_at,
            completed_at: 0,
            duration_ms: 0,
            timeseries_deleted: 0,
            memory_deleted: 0,
            config_history_deleted: 0,
            errors: Vec::new(),
        };

        // Cleanup timeseries data based on retention policy
        if let Some(retention_hours) = config.timeseries_retention_hours {
            match Self::cleanup_timeseries(storage, retention_hours).await {
                Ok(count) => result.timeseries_deleted = count,
                Err(e) => result.errors.push(format!("Timeseries cleanup: {}", e)),
            }
        }

        // Cleanup expired memory entries
        if let Some(retention_hours) = config.memory_retention_hours {
            match Self::cleanup_memories(storage, retention_hours).await {
                Ok(count) => result.memory_deleted = count,
                Err(e) => result.errors.push(format!("Memory cleanup: {}", e)),
            }
        }

        // Cleanup old config history
        match Self::cleanup_config_history(storage, config.max_history_entries).await {
            Ok(count) => result.config_history_deleted = count,
            Err(e) => result.errors.push(format!("Config history cleanup: {}", e)),
        }

        result.completed_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        result.duration_ms = if result.completed_at > result.started_at {
            ((result.completed_at - result.started_at) * 1000) as u64
        } else {
            0
        };

        Ok(result)
    }

    /// Cleanup old timeseries data.
    async fn cleanup_timeseries(_storage: &UnifiedStorage, _retention_hours: u64) -> Result<usize> {
        // This would integrate with TimeSeriesStore's retention cleanup
        // For now, return a placeholder
        Ok(0)
    }

    /// Cleanup expired memory entries.
    async fn cleanup_memories(_storage: &UnifiedStorage, _retention_hours: u64) -> Result<usize> {
        // This would integrate with LongTermMemoryStore's cleanup
        // For now, return a placeholder
        Ok(0)
    }

    /// Cleanup old config history entries.
    async fn cleanup_config_history(
        _storage: &UnifiedStorage,
        _keep_count: usize,
    ) -> Result<usize> {
        // This would clean up old config history
        Ok(0)
    }
}

/// Manual cleanup utilities.
pub struct CleanupUtils;

impl CleanupUtils {
    /// Calculate the cutoff timestamp for a retention period.
    pub fn calculate_cutoff(retention_hours: u64) -> i64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64
            - (retention_hours * 3600) as i64
    }

    /// Check if a timestamp is within the retention period.
    pub fn is_within_retention(timestamp: i64, retention_hours: u64) -> bool {
        let cutoff = Self::calculate_cutoff(retention_hours);
        timestamp > cutoff
    }

    /// Calculate the age of a timestamp in hours.
    pub fn age_in_hours(timestamp: i64) -> u64 {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        if now > timestamp {
            ((now - timestamp) / 3600) as u64
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maintenance_config_default() {
        let config = MaintenanceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.interval_secs, 3600);
        assert_eq!(config.timeseries_retention_hours, Some(24 * 7));
        assert_eq!(config.memory_retention_hours, Some(24 * 30));
    }

    #[test]
    fn test_cleanup_utils_calculate_cutoff() {
        let cutoff = CleanupUtils::calculate_cutoff(24);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        // Cutoff should be approximately 24 hours ago
        assert!(cutoff < now);
        assert!(cutoff > now - 86400 - 10); // Allow some margin
    }

    #[test]
    fn test_cleanup_utils_is_within_retention() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        // Recent timestamp should be within retention
        assert!(CleanupUtils::is_within_retention(now, 24));

        // Old timestamp (48 hours ago) should not be within 24h retention
        let old = now - (48 * 3600);
        assert!(!CleanupUtils::is_within_retention(old, 24));
    }

    #[test]
    fn test_cleanup_utils_age_in_hours() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;

        // Current timestamp should have age 0
        assert_eq!(CleanupUtils::age_in_hours(now), 0);

        // 1 hour ago should have age 1
        assert_eq!(CleanupUtils::age_in_hours(now - 3600), 1);

        // 24 hours ago should have age 24
        assert_eq!(CleanupUtils::age_in_hours(now - 86400), 24);
    }
}
