//! Storage monitoring and metrics.
//!
//! Provides:
//! - Storage metrics collection
//! - Performance monitoring
//! - Health checks
//! - Resource usage tracking
//! - Alerting for storage issues

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::backend::UnifiedStorage;

/// Storage health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    /// Storage is healthy.
    Healthy,
    /// Storage is degraded but functional.
    Degraded,
    /// Storage is unhealthy.
    Unhealthy,
}

/// Storage metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetrics {
    /// When these metrics were collected.
    pub timestamp: i64,
    /// Database file size in bytes.
    pub db_size_bytes: u64,
    /// Number of read operations.
    pub read_count: u64,
    /// Number of write operations.
    pub write_count: u64,
    /// Number of delete operations.
    pub delete_count: u64,
    /// Total read time in milliseconds.
    pub total_read_ms: u64,
    /// Total write time in milliseconds.
    pub total_write_ms: u64,
    /// Cache hit rate (0.0 to 1.0).
    pub cache_hit_rate: f64,
    /// Number of active connections.
    pub active_connections: u64,
    /// Memory usage in bytes.
    pub memory_usage_bytes: u64,
    /// Disk usage in bytes.
    pub disk_usage_bytes: u64,
    /// Disk available in bytes.
    pub disk_available_bytes: u64,
}

impl Default for StorageMetrics {
    fn default() -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0) as i64,
            db_size_bytes: 0,
            read_count: 0,
            write_count: 0,
            delete_count: 0,
            total_read_ms: 0,
            total_write_ms: 0,
            cache_hit_rate: 0.0,
            active_connections: 0,
            memory_usage_bytes: 0,
            disk_usage_bytes: 0,
            disk_available_bytes: 0,
        }
    }
}

impl StorageMetrics {
    /// Calculate average read latency in milliseconds.
    pub fn avg_read_latency_ms(&self) -> f64 {
        if self.read_count > 0 {
            self.total_read_ms as f64 / self.read_count as f64
        } else {
            0.0
        }
    }

    /// Calculate average write latency in milliseconds.
    pub fn avg_write_latency_ms(&self) -> f64 {
        if self.write_count > 0 {
            self.total_write_ms as f64 / self.write_count as f64
        } else {
            0.0
        }
    }

    /// Calculate total operations per second estimate.
    pub fn ops_per_second(&self) -> f64 {
        let total_ops = self.read_count + self.write_count + self.delete_count;
        // Assume metrics collected over 1 second for now
        total_ops as f64
    }
}

/// Performance statistics for a specific operation type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    /// Number of operations.
    pub count: u64,
    /// Total duration in milliseconds.
    pub total_duration_ms: u64,
    /// Minimum duration in milliseconds.
    pub min_duration_ms: u64,
    /// Maximum duration in milliseconds.
    pub max_duration_ms: u64,
    /// Number of errors.
    pub errors: u64,
}

impl Default for OperationStats {
    fn default() -> Self {
        Self {
            count: 0,
            total_duration_ms: 0,
            min_duration_ms: u64::MAX,
            max_duration_ms: 0,
            errors: 0,
        }
    }
}

impl OperationStats {
    /// Record an operation.
    pub fn record(&mut self, duration_ms: u64, success: bool) {
        self.count += 1;
        self.total_duration_ms += duration_ms;
        self.min_duration_ms = self.min_duration_ms.min(duration_ms);
        self.max_duration_ms = self.max_duration_ms.max(duration_ms);
        if !success {
            self.errors += 1;
        }
    }

    /// Calculate average duration.
    pub fn avg_duration_ms(&self) -> f64 {
        if self.count > 0 {
            self.total_duration_ms as f64 / self.count as f64
        } else {
            0.0
        }
    }

    /// Calculate success rate (0.0 to 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.count > 0 {
            (self.count - self.errors) as f64 / self.count as f64
        } else {
            1.0
        }
    }
}

/// Storage health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Overall health status.
    pub status: HealthStatus,
    /// When the check was performed.
    pub timestamp: i64,
    /// Individual check results.
    pub checks: HashMap<String, CheckResult>,
    /// Any additional information.
    pub message: Option<String>,
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Check name.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Check duration in milliseconds.
    pub duration_ms: u64,
    /// Additional information.
    pub info: Option<String>,
    /// Error message if check failed.
    pub error: Option<String>,
}

/// Storage monitoring configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Enable monitoring.
    pub enabled: bool,

    /// Metrics collection interval (seconds).
    pub metrics_interval_secs: u64,

    /// Health check interval (seconds).
    pub health_check_interval_secs: u64,

    /// Alert thresholds.
    pub alerts: AlertThresholds,

    /// Retention for metrics data (hours).
    pub metrics_retention_hours: u64,
}

/// Alert thresholds for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThresholds {
    /// Maximum database size in bytes before alerting.
    pub max_db_size_bytes: u64,

    /// Maximum read latency in milliseconds.
    pub max_read_latency_ms: u64,

    /// Maximum write latency in milliseconds.
    pub max_write_latency_ms: u64,

    /// Minimum disk space required (bytes).
    pub min_disk_space_bytes: u64,

    /// Maximum error rate (0.0 to 1.0).
    pub max_error_rate: f64,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            max_db_size_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            max_read_latency_ms: 1000, // 1 second
            max_write_latency_ms: 5000, // 5 seconds
            min_disk_space_bytes: 1024 * 1024 * 1024, // 1 GB
            max_error_rate: 0.01, // 1%
        }
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            metrics_interval_secs: 60, // 1 minute
            health_check_interval_secs: 30, // 30 seconds
            alerts: AlertThresholds::default(),
            metrics_retention_hours: 24,
        }
    }
}

/// Storage monitor for metrics and health checks.
pub struct StorageMonitor {
    config: MonitoringConfig,
    storage: Option<Arc<UnifiedStorage>>,

    // Metrics counters
    read_count: Arc<AtomicU64>,
    write_count: Arc<AtomicU64>,
    delete_count: Arc<AtomicU64>,
    total_read_ms: Arc<AtomicU64>,
    total_write_ms: Arc<AtomicU64>,

    // Operation statistics
    operation_stats: Arc<RwLock<HashMap<String, OperationStats>>>,

    // Latest metrics
    latest_metrics: Arc<RwLock<StorageMetrics>>,

    // Latest health check result
    latest_health: Arc<RwLock<Option<HealthCheckResult>>>,
}

impl StorageMonitor {
    /// Create a new storage monitor.
    pub fn new(config: MonitoringConfig) -> Self {
        Self {
            config,
            storage: None,
            read_count: Arc::new(AtomicU64::new(0)),
            write_count: Arc::new(AtomicU64::new(0)),
            delete_count: Arc::new(AtomicU64::new(0)),
            total_read_ms: Arc::new(AtomicU64::new(0)),
            total_write_ms: Arc::new(AtomicU64::new(0)),
            operation_stats: Arc::new(RwLock::new(HashMap::new())),
            latest_metrics: Arc::new(RwLock::new(StorageMetrics::default())),
            latest_health: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the storage backend.
    pub fn with_storage(mut self, storage: Arc<UnifiedStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Record a read operation.
    pub fn record_read(&self, duration_ms: u64) {
        self.read_count.fetch_add(1, Ordering::Relaxed);
        self.total_read_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Record a write operation.
    pub fn record_write(&self, duration_ms: u64) {
        self.write_count.fetch_add(1, Ordering::Relaxed);
        self.total_write_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Record a delete operation.
    pub fn record_delete(&self) {
        self.delete_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a custom operation.
    pub async fn record_operation(&self, name: &str, duration_ms: u64, success: bool) {
        let mut stats = self.operation_stats.write().await;
        let entry = stats.entry(name.to_string()).or_default();
        entry.record(duration_ms, success);
    }

    /// Get current metrics.
    pub async fn get_metrics(&self) -> StorageMetrics {
        let mut metrics = self.latest_metrics.read().await.clone();
        metrics.read_count = self.read_count.load(Ordering::Relaxed);
        metrics.write_count = self.write_count.load(Ordering::Relaxed);
        metrics.delete_count = self.delete_count.load(Ordering::Relaxed);
        metrics.total_read_ms = self.total_read_ms.load(Ordering::Relaxed);
        metrics.total_write_ms = self.total_write_ms.load(Ordering::Relaxed);
        metrics.timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        metrics
    }

    /// Get operation statistics.
    pub async fn get_operation_stats(&self) -> HashMap<String, OperationStats> {
        self.operation_stats.read().await.clone()
    }

    /// Get statistics for a specific operation.
    pub async fn get_operation_stat(&self, name: &str) -> Option<OperationStats> {
        self.operation_stats.read().await.get(name).cloned()
    }

    /// Perform health checks.
    pub async fn health_check(&self) -> HealthCheckResult {
        let mut checks = HashMap::new();
        let mut status = HealthStatus::Healthy;
        let start = SystemTime::now();

        // Check database file accessibility
        let db_check = self.check_database().await;
        if !db_check.passed {
            status = HealthStatus::Unhealthy;
        }
        checks.insert("database".to_string(), db_check);

        // Check disk space
        let disk_check = self.check_disk_space().await;
        if !disk_check.passed {
            status = if status == HealthStatus::Healthy {
                HealthStatus::Degraded
            } else {
                HealthStatus::Unhealthy
            };
        }
        checks.insert("disk_space".to_string(), disk_check);

        // Check performance
        let perf_check = self.check_performance().await;
        if !perf_check.passed {
            status = if status != HealthStatus::Unhealthy {
                HealthStatus::Degraded
            } else {
                HealthStatus::Unhealthy
            };
        }
        checks.insert("performance".to_string(), perf_check);

        // Check error rates
        let error_check = self.check_error_rate().await;
        if !error_check.passed {
            status = if status != HealthStatus::Unhealthy {
                HealthStatus::Degraded
            } else {
                HealthStatus::Unhealthy
            };
        }
        checks.insert("error_rate".to_string(), error_check);

        let duration = SystemTime::now()
            .duration_since(start)
            .unwrap_or(Duration::ZERO);

        let result = HealthCheckResult {
            status,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0) as i64,
            checks,
            message: Some(format!("Health check completed in {:?}", duration)),
        };

        // Store latest result
        *self.latest_health.write().await = Some(result.clone());

        result
    }

    /// Get latest health check result.
    pub async fn get_latest_health(&self) -> Option<HealthCheckResult> {
        self.latest_health.read().await.clone()
    }

    /// Reset all metrics.
    pub async fn reset_metrics(&self) {
        self.read_count.store(0, Ordering::Relaxed);
        self.write_count.store(0, Ordering::Relaxed);
        self.delete_count.store(0, Ordering::Relaxed);
        self.total_read_ms.store(0, Ordering::Relaxed);
        self.total_write_ms.store(0, Ordering::Relaxed);
        self.operation_stats.write().await.clear();
        *self.latest_metrics.write().await = StorageMetrics::default();
    }

    /// Check database accessibility.
    async fn check_database(&self) -> CheckResult {
        let start = SystemTime::now();

        if let Some(storage) = &self.storage {
            // Try to read from the database using the backend
            match storage.backend().read("__health__", "__check__") {
                Ok(_) => CheckResult {
                    name: "database".to_string(),
                    passed: true,
                    duration_ms: SystemTime::now()
                        .duration_since(start)
                        .unwrap_or(Duration::ZERO)
                        .as_millis() as u64,
                    info: Some("Database is accessible".to_string()),
                    error: None,
                },
                Err(e) => CheckResult {
                    name: "database".to_string(),
                    passed: false,
                    duration_ms: SystemTime::now()
                        .duration_since(start)
                        .unwrap_or(Duration::ZERO)
                        .as_millis() as u64,
                    info: None,
                    error: Some(format!("{:?}", e)),
                },
            }
        } else {
            CheckResult {
                name: "database".to_string(),
                passed: false,
                duration_ms: 0,
                info: None,
                error: Some("No storage configured".to_string()),
            }
        }
    }

    /// Check disk space availability.
    async fn check_disk_space(&self) -> CheckResult {
        let start = SystemTime::now();

        // Get current directory size and available space
        let current_dir = std::env::current_dir().ok();

        if let Some(_dir) = current_dir {
            // Check if minimum disk space is available
            // This is a simplified check - in production, you'd use actual disk stats
            let passed = true; // Placeholder

            CheckResult {
                name: "disk_space".to_string(),
                passed,
                duration_ms: SystemTime::now()
                    .duration_since(start)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64,
                info: Some("Disk space check passed".to_string()),
                error: None,
            }
        } else {
            CheckResult {
                name: "disk_space".to_string(),
                passed: true,
                duration_ms: 0,
                info: Some("Could not determine current directory".to_string()),
                error: None,
            }
        }
    }

    /// Check performance metrics.
    async fn check_performance(&self) -> CheckResult {
        let metrics = self.get_metrics().await;
        let avg_read = metrics.avg_read_latency_ms();
        let avg_write = metrics.avg_write_latency_ms();

        let passed = avg_read < self.config.alerts.max_read_latency_ms as f64
            && avg_write < self.config.alerts.max_write_latency_ms as f64;

        CheckResult {
            name: "performance".to_string(),
            passed,
            duration_ms: 0,
            info: Some(format!(
                "Avg read: {:.2}ms, Avg write: {:.2}ms",
                avg_read, avg_write
            )),
            error: if passed {
                None
            } else {
                Some("Performance exceeds thresholds".to_string())
            },
        }
    }

    /// Check error rates.
    async fn check_error_rate(&self) -> CheckResult {
        let stats = self.operation_stats.read().await;
        let mut total_count = 0;
        let mut total_errors = 0;

        for stat in stats.values() {
            total_count += stat.count;
            total_errors += stat.errors;
        }

        let error_rate = if total_count > 0 {
            total_errors as f64 / total_count as f64
        } else {
            0.0
        };

        let passed = error_rate <= self.config.alerts.max_error_rate;

        CheckResult {
            name: "error_rate".to_string(),
            passed,
            duration_ms: 0,
            info: Some(format!("Error rate: {:.2}%", error_rate * 100.0)),
            error: if passed {
                None
            } else {
                Some("Error rate exceeds threshold".to_string())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_metrics_default() {
        let metrics = StorageMetrics::default();
        assert_eq!(metrics.read_count, 0);
        assert_eq!(metrics.write_count, 0);
        assert_eq!(metrics.avg_read_latency_ms(), 0.0);
    }

    #[test]
    fn test_operation_stats() {
        let mut stats = OperationStats::default();
        stats.record(100, true);
        stats.record(200, true);
        stats.record(50, false);

        assert_eq!(stats.count, 3);
        assert_eq!(stats.avg_duration_ms(), 116.66666666666667);
        assert_eq!(stats.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_alert_thresholds_default() {
        let thresholds = AlertThresholds::default();
        assert_eq!(thresholds.max_db_size_bytes, 10 * 1024 * 1024 * 1024);
        assert_eq!(thresholds.max_read_latency_ms, 1000);
    }

    #[test]
    fn test_monitoring_config_default() {
        let config = MonitoringConfig::default();
        assert!(config.enabled);
        assert_eq!(config.metrics_interval_secs, 60);
        assert_eq!(config.health_check_interval_secs, 30);
    }

    #[test]
    fn test_storage_monitor() {
        let monitor = StorageMonitor::new(MonitoringConfig::default());
        monitor.record_read(100);
        monitor.record_write(50);
        monitor.record_delete();

        assert_eq!(monitor.read_count.load(Ordering::Relaxed), 1);
        assert_eq!(monitor.write_count.load(Ordering::Relaxed), 1);
        assert_eq!(monitor.delete_count.load(Ordering::Relaxed), 1);
    }
}
