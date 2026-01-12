//! System context collection for autonomous agent analysis.
//!
//! This module provides data collection capabilities for gathering
//! system state information to support autonomous decision making.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::NeoTalkEvent;
use edge_ai_storage::TimeSeriesStore;
use edge_ai_rules::RuleHistoryStorage;
use edge_ai_alerts::AlertManager;

use super::review::{DeviceStatus, RuleStatistics, AlertSummary, SystemMetrics};

/// System context containing all collected data for analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemContext {
    /// Collection timestamp
    pub collected_at: DateTime<Utc>,
    /// Time range for data collection
    pub time_range: TimeRange,
    /// Device status snapshot
    pub device_status: HashMap<String, DeviceStatus>,
    /// Rule execution statistics
    pub rule_stats: RuleStatistics,
    /// Alert summary
    pub alert_summary: AlertSummary,
    /// System metrics
    pub system_metrics: SystemMetrics,
    /// Energy consumption data
    pub energy_data: Option<EnergyData>,
}

/// Time range for data collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// Start timestamp
    pub start: DateTime<Utc>,
    /// End timestamp
    pub end: DateTime<Utc>,
}

impl TimeRange {
    /// Create a new time range with the last N hours.
    pub fn last_hours(hours: i64) -> Self {
        let end = Utc::now();
        let start = end - chrono::Duration::hours(hours);
        Self { start, end }
    }

    /// Create a new time range with the last N days.
    pub fn last_days(days: i64) -> Self {
        Self::last_hours(days * 24)
    }

    /// Get the duration in seconds.
    pub fn duration_secs(&self) -> i64 {
        (self.end - self.start).num_seconds()
    }
}

/// Energy consumption data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyData {
    /// Total energy consumption in kWh
    pub total_kwh: f64,
    /// Average power consumption in kW
    pub avg_kw: f64,
    /// Peak power consumption in kW
    pub peak_kw: f64,
    /// Per-device energy consumption
    pub per_device: HashMap<String, DeviceEnergy>,
}

/// Energy data for a single device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEnergy {
    /// Device ID
    pub device_id: String,
    /// Energy consumption in kWh
    pub kwh: f64,
    /// Average power in kW
    pub avg_kw: f64,
    /// Peak power in kW
    pub peak_kw: f64,
}

/// Context collector for gathering system data.
pub struct ContextCollector {
    /// Event bus for subscribing to events
    event_bus: Arc<EventBus>,
    /// Time series storage for metrics
    storage: Option<Arc<TimeSeriesStore>>,
    /// Rule history storage
    rule_history: Option<Arc<RuleHistoryStorage>>,
    /// Alert manager
    alert_manager: Option<Arc<AlertManager>>,
}

impl ContextCollector {
    /// Create a new context collector with minimal dependencies.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            storage: None,
            rule_history: None,
            alert_manager: None,
        }
    }

    /// Create a new context collector with all dependencies.
    pub fn with_all(
        event_bus: Arc<EventBus>,
        storage: Arc<TimeSeriesStore>,
        rule_history: Arc<RuleHistoryStorage>,
        alert_manager: Arc<AlertManager>,
    ) -> Self {
        Self {
            event_bus,
            storage: Some(storage),
            rule_history: Some(rule_history),
            alert_manager: Some(alert_manager),
        }
    }

    /// Set the time series storage.
    pub fn with_storage(mut self, storage: Arc<TimeSeriesStore>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set the rule history storage.
    pub fn with_rule_history(mut self, rule_history: Arc<RuleHistoryStorage>) -> Self {
        self.rule_history = Some(rule_history);
        self
    }

    /// Set the alert manager.
    pub fn with_alert_manager(mut self, alert_manager: Arc<AlertManager>) -> Self {
        self.alert_manager = Some(alert_manager);
        self
    }

    /// Collect system context for the given time range.
    pub async fn collect_context(&self, time_range: TimeRange) -> SystemContext {
        let device_status = self.collect_device_status().await;
        let rule_stats = self.collect_rule_stats(&time_range).await;
        let alert_summary = self.collect_alert_summary(&time_range).await;
        let system_metrics = self.collect_system_metrics().await;
        let energy_data = self.collect_energy_data(&time_range).await;

        SystemContext {
            collected_at: Utc::now(),
            time_range,
            device_status,
            rule_stats,
            alert_summary,
            system_metrics,
            energy_data,
        }
    }

    /// Collect device status information.
    async fn collect_device_status(&self) -> HashMap<String, DeviceStatus> {
        let mut status_map = HashMap::new();

        // Try to get device status from recent events on the event bus
        // We subscribe briefly to collect current state
        let mut rx = self.event_bus.subscribe();

        // Collect events with a timeout
        let timeout_duration = Duration::from_millis(500);
        let start = std::time::Instant::now();

        while start.elapsed() < timeout_duration {
            match tokio::time::timeout(timeout_duration - start.elapsed(), rx.recv()).await {
                Ok(Some((event, _))) => {
                    match event {
                        NeoTalkEvent::DeviceOnline { device_id, device_type, timestamp } => {
                            let status = DeviceStatus {
                                device_id: device_id.clone(),
                                name: device_id.clone(), // Use ID as name for now
                                device_type,
                                online: true,
                                last_seen: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now),
                                metrics: HashMap::new(),
                                health_score: Some(100.0), // Online = healthy
                            };
                            status_map.insert(device_id, status);
                        }
                        NeoTalkEvent::DeviceOffline { device_id, reason: _, timestamp } => {
                            // Update existing or create offline status
                            if let Some(status) = status_map.get_mut(&device_id) {
                                status.online = false;
                                status.last_seen = DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now);
                                status.health_score = Some(0.0);
                            } else {
                                let status = DeviceStatus {
                                    device_id: device_id.clone(),
                                    name: device_id.clone(),
                                    device_type: "unknown".to_string(),
                                    online: false,
                                    last_seen: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now),
                                    metrics: HashMap::new(),
                                    health_score: Some(0.0),
                                };
                                status_map.insert(device_id, status);
                            }
                        }
                        NeoTalkEvent::DeviceMetric { device_id, metric, value, quality: _, timestamp } => {
                            // Update metrics for the device
                            if let Some(status) = status_map.get_mut(&device_id) {
                                let num_value = match value {
                                    edge_ai_core::event::MetricValue::Float(v) => v,
                                    edge_ai_core::event::MetricValue::Integer(v) => v as f64,
                                    edge_ai_core::event::MetricValue::Boolean(v) => if v { 1.0 } else { 0.0 },
                                    _ => 0.0,
                                };
                                status.metrics.insert(metric, num_value);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(None) => {
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // Timeout, stop collecting
                    break;
                }
            }
        }

        status_map
    }

    /// Collect rule execution statistics.
    async fn collect_rule_stats(&self, time_range: &TimeRange) -> RuleStatistics {
        if let Some(history) = &self.rule_history {
            use edge_ai_rules::HistoryFilter;

            let filter = HistoryFilter::new()
                .with_time_range(time_range.start, time_range.end);

            let entries = history.query(&filter).await.unwrap_or_default();

            let total_executions = entries.len() as u64;
            let successful_executions = entries.iter().filter(|e| e.success).count() as u64;
            let failed_executions = total_executions - successful_executions;

            // Count triggers per rule
            let mut trigger_counts: HashMap<String, u64> = HashMap::new();
            for entry in &entries {
                *trigger_counts.entry(entry.rule_id.clone()).or_insert(0) += 1;
            }

            let top_triggered = trigger_counts
                .into_iter()
                .map(|(rule_id, count)| {
                    use super::review::RuleTriggerStats;
                    RuleTriggerStats {
                        rule_id: rule_id.clone(),
                        rule_name: rule_id.clone(), // TODO: Get actual name
                        trigger_count: count,
                        success_count: count, // TODO: Calculate per-rule success
                        failure_count: 0,
                    }
                })
                .take(10)
                .collect();

            RuleStatistics {
                total_rules: 0, // TODO: Get from rule engine
                active_rules: 0,
                total_executions,
                successful_executions,
                failed_executions,
                top_triggered,
            }
        } else {
            RuleStatistics::default()
        }
    }

    /// Collect alert summary.
    async fn collect_alert_summary(&self, time_range: &TimeRange) -> AlertSummary {
        use edge_ai_alerts::{AlertSeverity, AlertStatus};

        if let Some(alert_manager) = &self.alert_manager {
            let all_alerts = alert_manager.list_alerts().await;

            let mut summary = AlertSummary::default();
            summary.total_alerts = all_alerts.len();

            // Count alerts by status and severity
            for alert in all_alerts {
                // Check if alert was created within time range
                let in_range = alert.timestamp >= time_range.start && alert.timestamp <= time_range.end;

                if in_range || matches!(alert.status, AlertStatus::Active) {
                    match alert.status {
                        AlertStatus::Active => {
                            summary.active_alerts += 1;
                        }
                        AlertStatus::Acknowledged | AlertStatus::Resolved => {
                            summary.resolved_alerts += 1;
                        }
                        _ => {}
                    }

                    // Count by severity (Emergency and Critical are "critical", Warning is "warning")
                    match alert.severity {
                        AlertSeverity::Emergency | AlertSeverity::Critical => summary.critical_alerts += 1,
                        AlertSeverity::Warning => summary.warning_alerts += 1,
                        _ => {}
                    }
                }
            }

            summary
        } else {
            // Fallback: try to get alert info from event bus
            let mut summary = AlertSummary::default();
            let mut rx = self.event_bus.subscribe();
            let timeout_duration = Duration::from_millis(500);
            let start = std::time::Instant::now();

            while start.elapsed() < timeout_duration {
                match tokio::time::timeout(timeout_duration - start.elapsed(), rx.recv()).await {
                    Ok(Some((NeoTalkEvent::AlertCreated { severity, .. }, _))) => {
                        summary.total_alerts += 1;
                        match severity.as_str() {
                            "critical" | "emergency" => summary.critical_alerts += 1,
                            "warning" => summary.warning_alerts += 1,
                            _ => {}
                        }
                    }
                    Ok(Some(_)) | Ok(None) | Err(_) => break,
                }
            }

            summary
        }
    }

    /// Collect system metrics.
    async fn collect_system_metrics(&self) -> SystemMetrics {
        let mut metrics = SystemMetrics::default();

        // Get device counts from event bus
        let mut online_count = 0;
        let mut offline_count = 0;
        let mut rx = self.event_bus.subscribe();
        let timeout_duration = Duration::from_millis(200);
        let start = std::time::Instant::now();

        while start.elapsed() < timeout_duration {
            match tokio::time::timeout(timeout_duration - start.elapsed(), rx.recv()).await {
                Ok(Some((NeoTalkEvent::DeviceOnline { .. }, _))) => {
                    online_count += 1;
                }
                Ok(Some((NeoTalkEvent::DeviceOffline { .. }, _))) => {
                    offline_count += 1;
                }
                Ok(Some(_)) | Ok(None) | Err(_) => break,
            }
        }

        metrics.online_devices = online_count;
        metrics.offline_devices = offline_count;
        metrics.total_devices = online_count + offline_count;

        // Collect system resource metrics
        // Try to get CPU and memory usage using sysinfo
        #[cfg(feature = "sysinfo")]
        {
            if let Ok(mut sys) = sysinfo::System::new_with_specifics(
                sysinfo::RefreshKind::new()
                    .with_cpu(sysinfo::CpuRefreshKind::new())
                    .with_memory(sysinfo::MemoryRefreshKind::new())
            ) {
                sys.refresh_cpu();
                sys.refresh_memory();

                if let Some(cpu) = sys.global_cpu_usage().get(0) {
                    metrics.cpu_usage = Some(cpu as f64);
                }

                let total_memory = sys.total_memory();
                let used_memory = sys.used_memory();
                if total_memory > 0 {
                    metrics.memory_usage = Some((used_memory as f64 / total_memory as f64) * 100.0);
                }

                // Disk usage
                sys.refresh_disks_list();
                if let Some(disk) = sys.disks().first() {
                    let total_disk = disk.total_space();
                    let available_disk = disk.available_space();
                    if total_disk > 0 {
                        metrics.disk_usage = Some(((total_disk - available_disk) as f64 / total_disk as f64) * 100.0);
                    }
                }
            }
        }

        // If sysinfo feature not available, use /proc on Linux or default values
        #[cfg(not(feature = "sysinfo"))]
        {
            #[cfg(target_os = "linux")]
            {
                // Try to read memory info from /proc/meminfo
                if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                    let mut total_mem = 0;
                    let mut free_mem = 0;
                    for line in meminfo.lines() {
                        if line.starts_with("MemTotal:") {
                            if let Some(val) = line.split_whitespace().nth(1) {
                                total_mem = val.parse().unwrap_or(0);
                            }
                        } else if line.starts_with("MemAvailable:") || line.starts_with("MemFree:") {
                            if let Some(val) = line.split_whitespace().nth(1) {
                                free_mem += val.parse().unwrap_or(0);
                            }
                        }
                    }
                    if total_mem > 0 {
                        metrics.memory_usage = Some(((total_mem - free_mem) as f64 / total_mem as f64) * 100.0);
                    }
                }

                // Try to read load average for CPU estimation
                if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
                    if let Some(load) = loadavg.split_whitespace().next() {
                        if let Ok(load_val) = load.parse::<f64>() {
                            metrics.cpu_usage = Some((load_val * 100.0).min(100.0));
                        }
                    }
                }
            }
        }

        metrics
    }

    /// Collect energy consumption data.
    async fn collect_energy_data(&self, time_range: &TimeRange) -> Option<EnergyData> {
        if let Some(storage) = &self.storage {
            // TODO: Query energy metrics from storage
            Some(EnergyData {
                total_kwh: 0.0,
                avg_kw: 0.0,
                peak_kw: 0.0,
                per_device: HashMap::new(),
            })
        } else {
            None
        }
    }

    /// Aggregate metrics across multiple devices.
    pub fn aggregate_device_metrics(
        &self,
        device_metrics: &HashMap<String, HashMap<String, Vec<f64>>>,
        metric_name: &str,
    ) -> MetricAggregation {
        let mut values = Vec::new();

        for device_metrics in device_metrics.values() {
            if let Some(metric_values) = device_metrics.get(metric_name) {
                values.extend(metric_values);
            }
        }

        if values.is_empty() {
            return MetricAggregation::default();
        }

        let count = values.len();
        let sum: f64 = values.iter().sum();
        let avg = sum / count as f64;

        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

        // Calculate standard deviation
        let variance = values.iter()
            .map(|&x| (x - avg).powi(2))
            .sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        MetricAggregation {
            count,
            sum,
            avg,
            min,
            max,
            std_dev,
        }
    }
}

/// Aggregated metric statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAggregation {
    /// Number of data points
    pub count: usize,
    /// Sum of all values
    pub sum: f64,
    /// Average (mean) value
    pub avg: f64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Standard deviation
    pub std_dev: f64,
}

impl Default for MetricAggregation {
    fn default() -> Self {
        Self {
            count: 0,
            sum: 0.0,
            avg: 0.0,
            min: 0.0,
            max: 0.0,
            std_dev: 0.0,
        }
    }
}

impl MetricAggregation {
    /// Calculate the coefficient of variation (relative variability).
    pub fn coefficient_of_variation(&self) -> f64 {
        if self.avg == 0.0 {
            0.0
        } else {
            (self.std_dev / self.avg).abs()
        }
    }

    /// Check if the values are relatively stable (low variance).
    pub fn is_stable(&self, threshold: f64) -> bool {
        self.coefficient_of_variation() < threshold
    }

    /// Get the range (max - min).
    pub fn range(&self) -> f64 {
        self.max - self.min
    }

    /// Get the median value (requires sorting, so not pre-calculated).
    pub fn median(&self, mut values: Vec<f64>) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let len = values.len();
        if len % 2 == 0 {
            (values[len / 2 - 1] + values[len / 2]) / 2.0
        } else {
            values[len / 2]
        }
    }
}

impl Default for SystemContext {
    fn default() -> Self {
        Self {
            collected_at: Utc::now(),
            time_range: TimeRange::last_hours(24),
            device_status: HashMap::new(),
            rule_stats: RuleStatistics::default(),
            alert_summary: AlertSummary::default(),
            system_metrics: SystemMetrics::default(),
            energy_data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_range_last_hours() {
        let range = TimeRange::last_hours(24);
        assert!(range.duration_secs() > 0);
        assert_eq!(range.duration_secs(), 24 * 3600);
    }

    #[test]
    fn test_time_range_last_days() {
        let range = TimeRange::last_days(2);
        assert_eq!(range.duration_secs(), 2 * 24 * 3600);
    }

    #[test]
    fn test_metric_aggregation_default() {
        let agg = MetricAggregation::default();
        assert_eq!(agg.count, 0);
        assert_eq!(agg.sum, 0.0);
        assert_eq!(agg.avg, 0.0);
    }

    #[test]
    fn test_metric_aggregation_calculations() {
        let values = vec
![1.0, 2.0, 3.0, 4.0, 5.0];
        let count = values.len()
;
        let sum: f64 = values.iter().sum();
        let avg = sum / count as f64;
        let min = 1.0;
        let max = 5.0;

        let variance = values.iter()
            .map(|&x| (x - avg).powi(2))
            .sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        assert_eq!(avg, 3.0);
        assert_eq!(min, 1.0);
        assert_eq!(max, 5.0);
        assert!(std_dev > 0.0);
    }

    #[test]
    fn test_coefficient_of_variation() {
        let mut agg = MetricAggregation {
            count: 4,
            sum: 20.0,
            avg: 5.0,
            min: 4.0,
            max: 6.0,
            std_dev: 1.0,
        };

        let cv = agg.coefficient_of_variation();
        assert_eq!(cv, 0.2); // 1.0 / 5.0 = 0.2
    }

    #[test]
    fn test_is_stable() {
        let agg = MetricAggregation {
            count: 4,
            sum: 20.0,
            avg: 5.0,
            min: 4.0,
            max: 6.0,
            std_dev: 0.5,
        };

        assert!(agg.is_stable(0.2)); // CV = 0.1 < 0.2
        assert!(!agg.is_stable(0.05)); // CV = 0.1 > 0.05
    }

    #[test]
    fn test_median() {
        let agg = MetricAggregation::default();
        let odd_values = vec
![1.0, 2.0, 3.0, 4.0, 5.0];
        let even_values = vec
![1.0, 2.0, 3.0, 4.0];

        assert_eq!(agg.median(odd_values)
, 3.0);
        assert_eq!(agg.median(even_values)
, 2.5);
    }

    #[tokio::test]
    async fn test_context_collector_creation() {
        let event_bus = Arc::new(EventBus::new());
        let collector = ContextCollector::new(event_bus);

        let time_range = TimeRange::last_hours(1);
        let context = collector.collect_context(time_range).await;

        assert_eq!(context.device_status.len(), 0);
    }

    #[tokio::test]
    async fn test_system_context_default() {
        let context = SystemContext::default();
        assert!(context.time_range.duration_secs() > 0);
        assert!(context.energy_data.is_none());
    }
}
