//! Core types for the data push module.

use serde::{Deserialize, Serialize};

/// Unique identifier for a push target.
pub type PushTargetId = String;

/// Scheduling configuration for a push target.
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PushSchedule {
    /// Event-driven: subscribe to EventBus, push immediately when data matches.
    EventDriven {
        /// Event type filters (e.g. "device_metric", "extension_output").
        event_types: Vec<String>,
    },
    /// Interval-based: periodically pull latest data from TimeSeriesStore.
    Interval {
        /// Polling interval in seconds.
        interval_secs: u64,
    },
}

/// Retry configuration for failed deliveries.
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    /// Exponential backoff base in seconds.
    pub backoff_secs: u64,
    /// Maximum backoff duration in seconds.
    pub max_backoff_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff_secs: 5,
            max_backoff_secs: 300,
        }
    }
}

/// Output shape for a batched payload.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchFormat {
    /// Flat list: `{ batch, count, items: [{source_id, value, timestamp}, ...] }`.
    #[default]
    Flat,
    /// Nested by source: `{ batch, format, count, timestamp, data: {type: {id: {field: value}}} }`.
    /// Field paths (e.g. `values.devName`) are split on `.` to rebuild nesting,
    /// reversing the flattening applied at ingestion (`unified_extractor`).
    Nested,
}

/// Batch/aggregation configuration for push delivery.
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Maximum number of events to aggregate before flushing.
    /// Default: 1 (no batching, send immediately).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Maximum time in milliseconds to wait before flushing a partial batch.
    /// Default: 1000ms.
    #[serde(default = "default_batch_interval_ms")]
    pub batch_interval_ms: u64,
    /// Output shape of the batch payload (flat list vs nested by source).
    #[serde(default)]
    pub format: BatchFormat,
}

fn default_batch_size() -> usize {
    1
}

fn default_batch_interval_ms() -> u64 {
    2000
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: default_batch_size(),
            batch_interval_ms: default_batch_interval_ms(),
            format: BatchFormat::default(),
        }
    }
}

impl BatchConfig {
    /// Returns true if batching is enabled (batch_size > 1).
    pub fn is_enabled(&self) -> bool {
        self.batch_size > 1
    }
}

/// The type of push destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushTargetType {
    Webhook,
    Mqtt,
}

impl std::fmt::Display for PushTargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Webhook => write!(f, "webhook"),
            Self::Mqtt => write!(f, "mqtt"),
        }
    }
}

/// Filter for selecting which data sources to push.
#[derive(utoipa::ToSchema, Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceFilter {
    /// Prefix patterns to match DataSourceId (e.g. "device:sensor1:" matches all fields).
    pub source_patterns: Vec<String>,
    /// Only push when the value changes.
    #[serde(default)]
    pub only_changes: bool,
}

impl DataSourceFilter {
    /// Check if a data source ID matches any of the configured patterns.
    pub fn matches(&self, source_id: &str) -> bool {
        if self.source_patterns.is_empty() {
            return true;
        }
        self.source_patterns.iter().any(|pattern| {
            if pattern == "*" {
                return true;
            }
            // Trailing wildcard: "device:sensor:*" matches "device:sensor:temperature".
            // Without this the literal '*' fails starts_with and the filter
            // silently matches nothing — a very common user/AI expectation.
            if let Some(prefix) = pattern.strip_suffix('*') {
                source_id.starts_with(prefix)
            } else {
                source_id.starts_with(pattern) || source_id == pattern
            }
        })
    }
}

/// A configured push target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushTarget {
    pub id: PushTargetId,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub target_type: PushTargetType,
    /// Target-specific configuration (URL, headers, broker, etc.).
    pub config: serde_json::Value,
    pub schedule: PushSchedule,
    pub data_filter: DataSourceFilter,
    /// Handlebars template for payload transformation.
    pub template: Option<String>,
    #[serde(default)]
    pub retry_config: RetryConfig,
    #[serde(default)]
    pub batch_config: BatchConfig,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Status of a delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Success,
    Failed,
    Retrying,
}

/// Log entry for a single delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryLog {
    pub id: String,
    pub target_id: PushTargetId,
    pub status: DeliveryStatus,
    pub data_source_id: String,
    pub payload_sent: String,
    pub response: Option<String>,
    pub attempts: u32,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

/// Template context available in Handlebars rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateContext {
    pub source_id: String,
    pub value: serde_json::Value,
    pub timestamp: i64,
}

/// Aggregated statistics for push targets.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PushStats {
    pub total_targets: usize,
    pub active_targets: usize,
    pub total_deliveries: u64,
    pub successful_deliveries: u64,
    pub failed_deliveries: u64,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_source_filter_prefix_match() {
        let filter = DataSourceFilter {
            source_patterns: vec!["device:sensor1:".to_string()],
            only_changes: false,
        };
        assert!(filter.matches("device:sensor1:temperature"));
        assert!(filter.matches("device:sensor1:humidity"));
        assert!(!filter.matches("device:sensor2:temperature"));
    }

    #[test]
    fn test_data_source_filter_exact_match() {
        let filter = DataSourceFilter {
            source_patterns: vec!["device:sensor1:temp".to_string()],
            only_changes: false,
        };
        assert!(filter.matches("device:sensor1:temp"));
        assert!(!filter.matches("device:sensor1:humidity"));
    }

    #[test]
    fn test_data_source_filter_empty_patterns() {
        let filter = DataSourceFilter {
            source_patterns: vec![],
            only_changes: false,
        };
        assert!(filter.matches("anything"));
    }

    #[test]
    fn test_data_source_filter_trailing_wildcard() {
        // "device:sensor1:*" must match all fields under that device — the '*'
        // is a wildcard, not a literal. Regression for the bug where push
        // silently matched nothing because starts_with("...:*") failed.
        let filter = DataSourceFilter {
            source_patterns: vec!["device:sensor1:*".to_string()],
            only_changes: false,
        };
        assert!(filter.matches("device:sensor1:temperature"));
        assert!(filter.matches("device:sensor1:humidity"));
        assert!(!filter.matches("device:sensor2:temperature"));
        // Bare "*" matches everything.
        let all = DataSourceFilter {
            source_patterns: vec!["*".to_string()],
            only_changes: false,
        };
        assert!(all.matches("anything"));
    }
}
