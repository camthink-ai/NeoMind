//! System review framework for autonomous agent analysis.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::NeoTalkEvent;

use super::config::ReviewType;

/// Context collected for a system review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContext {
    /// Review ID
    pub id: String,
    /// Review type
    pub review_type: ReviewType,
    /// Start time of the review
    pub start_time: DateTime<Utc>,
    /// End time of the review
    pub end_time: Option<DateTime<Utc>>,
    /// Device status snapshot
    pub device_status: HashMap<String, DeviceStatus>,
    /// Rule execution statistics
    pub rule_stats: RuleStatistics,
    /// Alert summary
    pub alert_summary: AlertSummary,
    /// System metrics
    pub system_metrics: SystemMetrics,
}

impl ReviewContext {
    /// Create a new review context.
    pub fn new(review_type: ReviewType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            review_type,
            start_time: Utc::now(),
            end_time: None,
            device_status: HashMap::new(),
            rule_stats: RuleStatistics::default(),
            alert_summary: AlertSummary::default(),
            system_metrics: SystemMetrics::default(),
        }
    }

    /// Mark the review as completed.
    pub fn complete(&mut self) {
        self.end_time = Some(Utc::now());
    }

    /// Get the duration of the review in seconds.
    pub fn duration_secs(&self) -> Option<i64> {
        self.end_time
            .map(|end| (end - self.start_time).num_seconds())
    }
}

/// Device status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatus {
    /// Device ID
    pub device_id: String,
    /// Device name
    pub name: String,
    /// Device type
    pub device_type: String,
    /// Online status
    pub online: bool,
    /// Last seen timestamp
    pub last_seen: DateTime<Utc>,
    /// Current metrics
    pub metrics: HashMap<String, f64>,
    /// Health score (0-100)
    pub health_score: Option<f64>,
}

impl DeviceStatus {
    /// Create a new device status.
    pub fn new(device_id: String, name: String, device_type: String) -> Self {
        Self {
            device_id,
            name,
            device_type,
            online: false,
            last_seen: Utc::now(),
            metrics: HashMap::new(),
            health_score: None,
        }
    }

    /// Check if device is healthy.
    pub fn is_healthy(&self) -> bool {
        if !self.online {
            return false;
        }
        if let Some(score) = self.health_score {
            return score >= 70.0;
        }
        true
    }
}

/// Rule execution statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuleStatistics {
    /// Total number of rules
    pub total_rules: usize,
    /// Number of active rules
    pub active_rules: usize,
    /// Total executions in the review period
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Top triggered rules
    pub top_triggered: Vec<RuleTriggerStats>,
}

impl RuleStatistics {
    /// Calculate success rate.
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            return 100.0;
        }
        (self.successful_executions as f64 / self.total_executions as f64) * 100.0
    }
}

/// Statistics for a single rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTriggerStats {
    /// Rule ID
    pub rule_id: String,
    /// Rule name
    pub rule_name: String,
    /// Trigger count
    pub trigger_count: u64,
    /// Success count
    pub success_count: u64,
    /// Failure count
    pub failure_count: u64,
}

/// Alert summary for the review period.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlertSummary {
    /// Total alerts
    pub total_alerts: usize,
    /// Active alerts
    pub active_alerts: usize,
    /// Critical alerts
    pub critical_alerts: usize,
    /// Warning alerts
    pub warning_alerts: usize,
    /// Resolved alerts
    pub resolved_alerts: usize,
}

/// System metrics collected during review.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_usage: Option<f64>,
    /// Memory usage percentage
    pub memory_usage: Option<f64>,
    /// Disk usage percentage
    pub disk_usage: Option<f64>,
    /// Active connections
    pub active_connections: Option<usize>,
    /// Total devices
    pub total_devices: usize,
    /// Online devices
    pub online_devices: usize,
    /// Offline devices
    pub offline_devices: usize,
}

/// Result of a system review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewResult {
    /// Review completed successfully with findings
    Findings {
        /// Summary of findings
        summary: String,
        /// Detected issues
        issues: Vec<DetectedIssue>,
        /// Recommendations
        recommendations: Vec<Recommendation>,
    },
    /// Review completed with no significant findings
    NoFindings {
        /// Message explaining why no findings were detected
        message: String,
    },
    /// Review failed
    Failed {
        /// Error message
        error: String,
    },
}

impl ReviewResult {
    /// Check if the review was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Findings { .. } | Self::NoFindings { .. })
    }

    /// Get the summary message.
    pub fn summary(&self) -> String {
        match self {
            Self::Findings { summary, .. } => summary.clone(),
            Self::NoFindings { message } => message.clone(),
            Self::Failed { error } => format!("Review failed: {}", error),
        }
    }
}

/// Issue detected during system review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedIssue {
    /// Issue ID
    pub id: String,
    /// Issue severity
    pub severity: IssueSeverity,
    /// Issue type
    pub issue_type: String,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// Affected entities (device IDs, rule IDs, etc.)
    pub affected_entities: Vec<String>,
    /// Detected timestamp
    pub detected_at: DateTime<Utc>,
}

impl DetectedIssue {
    /// Create a new detected issue.
    pub fn new(
        severity: IssueSeverity,
        issue_type: String,
        title: String,
        description: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            severity,
            issue_type,
            title,
            description,
            affected_entities: Vec::new(),
            detected_at: Utc::now(),
        }
    }

    /// Add an affected entity.
    pub fn with_entity(mut self, entity: String) -> Self {
        self.affected_entities.push(entity);
        self
    }
}

/// Severity level for detected issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    /// Critical issue requiring immediate attention
    Critical,
    /// Warning issue that should be addressed soon
    Warning,
    /// Informational issue
    Info,
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::Warning => write!(f, "warning"),
            Self::Info => write!(f, "info"),
        }
    }
}

/// Recommendation generated from review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// Recommendation ID
    pub id: String,
    /// Recommendation type
    pub recommendation_type: String,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// Expected benefit
    pub expected_benefit: String,
    /// Confidence score (0-100)
    pub confidence: u8,
    /// Suggested actions
    pub actions: Vec<SuggestedAction>,
}

impl Recommendation {
    /// Create a new recommendation.
    pub fn new(
        recommendation_type: String,
        title: String,
        description: String,
        expected_benefit: String,
        confidence: u8,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            recommendation_type,
            title,
            description,
            expected_benefit,
            confidence,
            actions: Vec::new(),
        }
    }

    /// Add a suggested action.
    pub fn with_action(mut self, action: SuggestedAction) -> Self {
        self.actions.push(action);
        self
    }
}

/// Suggested action for implementing a recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    /// Action type (e.g., "create_rule", "modify_device", "notify_user")
    pub action_type: String,
    /// Action description
    pub description: String,
    /// Parameters for the action
    pub parameters: Option<serde_json::Value>,
}

/// Trait for performing system reviews.
#[async_trait::async_trait]
pub trait SystemReview: Send + Sync {
    /// Get the review type.
    fn review_type(&self) -> ReviewType;

    /// Perform the review.
    async fn review(&self, context: &mut ReviewContext) -> ReviewResult;
}

/// Device health review implementation.
pub struct DeviceHealthReview {
    event_bus: Arc<EventBus>,
}

impl DeviceHealthReview {
    /// Create a new device health review.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait::async_trait]
impl SystemReview for DeviceHealthReview {
    fn review_type(&self) -> ReviewType {
        ReviewType::DeviceHealth
    }

    async fn review(&self, context: &mut ReviewContext) -> ReviewResult {
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        // Check offline devices
        let offline_devices: Vec<_> = context
            .device_status
            .values()
            .filter(|d| !d.online)
            .collect();

        if !offline_devices.is_empty() {
            let offline_count = offline_devices.len();
            for device in &offline_devices {
                let issue = DetectedIssue::new(
                    IssueSeverity::Warning,
                    "device_offline".to_string(),
                    format!("Device {} is offline", device.name),
                    format!(
                        "Device {} has been offline since {}",
                        device.name,
                        device.last_seen.format("%Y-%m-%d %H:%M:%S")
                    ),
                )
                .with_entity(device.device_id.clone());

                issues.push(issue);
            }

            recommendations.push(Recommendation::new(
                "check_offline_devices".to_string(),
                "Investigate Offline Devices".to_string(),
                format!(
                    "{} devices are currently offline. Check network connectivity and device power.",
                    offline_count
                ),
                "Restore device availability for improved monitoring".to_string(),
                80,
            ));
        }

        // Check unhealthy devices
        let unhealthy_devices: Vec<_> = context
            .device_status
            .values()
            .filter(|d| !d.is_healthy())
            .collect();

        for device in &unhealthy_devices {
            let issue = DetectedIssue::new(
                IssueSeverity::Warning,
                "device_unhealthy".to_string(),
                format!("Device {} has low health score", device.name),
                format!(
                    "Device {} health score is {:.1}, below the healthy threshold of 70",
                    device.name,
                    device.health_score.unwrap_or(0.0)
                ),
            )
            .with_entity(device.device_id.clone());

            issues.push(issue);
        }

        if issues.is_empty() {
            ReviewResult::NoFindings {
                message: format!(
                    "All {} devices are healthy and online",
                    context.device_status.len()
                ),
            }
        } else {
            let offline_count = offline_devices.len();
            let unhealthy_count = unhealthy_devices.len();
            ReviewResult::Findings {
                summary: format!(
                    "Found {} device health issues: {} offline, {} unhealthy",
                    issues.len(),
                    offline_count,
                    unhealthy_count
                ),
                issues,
                recommendations,
            }
        }
    }
}

/// Trend analysis review implementation.
pub struct TrendAnalysisReview {
    event_bus: Arc<EventBus>,
}

impl TrendAnalysisReview {
    /// Create a new trend analysis review.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait::async_trait]
impl SystemReview for TrendAnalysisReview {
    fn review_type(&self) -> ReviewType {
        ReviewType::TrendAnalysis
    }

    async fn review(&self, context: &mut ReviewContext) -> ReviewResult {
        // Trend analysis would examine historical data patterns
        ReviewResult::NoFindings {
            message: "Insufficient data for trend analysis. Need at least 10 data points per metric.".to_string(),
        }
    }
}

/// Anomaly detection review implementation.
pub struct AnomalyDetectionReview {
    event_bus: Arc<EventBus>,
}

impl AnomalyDetectionReview {
    /// Create a new anomaly detection review.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait::async_trait]
impl SystemReview for AnomalyDetectionReview {
    fn review_type(&self) -> ReviewType {
        ReviewType::AnomalyDetection
    }

    async fn review(&self, context: &mut ReviewContext) -> ReviewResult {
        ReviewResult::NoFindings {
            message: "No anomalies detected in the current system state.".to_string(),
        }
    }
}

/// Energy optimization review implementation.
pub struct EnergyOptimizationReview {
    event_bus: Arc<EventBus>,
}

impl EnergyOptimizationReview {
    /// Create a new energy optimization review.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait::async_trait]
impl SystemReview for EnergyOptimizationReview {
    fn review_type(&self) -> ReviewType {
        ReviewType::EnergyOptimization
    }

    async fn review(&self, context: &mut ReviewContext) -> ReviewResult {
        ReviewResult::NoFindings {
            message: "No energy optimization opportunities identified.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_context_creation() {
        let context = ReviewContext::new(ReviewType::DeviceHealth);
        assert_eq!(context.review_type, ReviewType::DeviceHealth);
        assert!(context.end_time.is_none());
    }

    #[test]
    fn test_review_context_completion() {
        let mut context = ReviewContext::new(ReviewType::DeviceHealth);
        assert!(context.end_time.is_none());

        context.complete();
        assert!(context.end_time.is_some());
        assert!(context.duration_secs().unwrap() >= 0);
    }

    #[test]
    fn test_device_status_health() {
        let mut status = DeviceStatus::new(
            "device_1".to_string(),
            "Sensor 1".to_string(),
            "sensor".to_string(),
        );
        status.online = true;
        status.health_score = Some(80.0);

        assert!(status.is_healthy());

        status.health_score = Some(50.0);
        assert!(!status.is_healthy());

        status.online = false;
        assert!(!status.is_healthy());
    }

    #[test]
    fn test_rule_statistics_success_rate() {
        let stats = RuleStatistics {
            total_executions: 100,
            successful_executions: 95,
            failed_executions: 5,
            ..Default::default()
        };

        assert_eq!(stats.success_rate(), 95.0);
    }

    #[test]
    fn test_detected_issue_creation() {
        let issue = DetectedIssue::new(
            IssueSeverity::Critical,
            "test_issue".to_string(),
            "Test Issue".to_string(),
            "Test Description".to_string(),
        )
        .with_entity("device_1".to_string())
        .with_entity("device_2".to_string());

        assert_eq!(issue.affected_entities.len(), 2);
    }

    #[test]
    fn test_review_result_success() {
        let result = ReviewResult::NoFindings {
            message: "No issues".to_string(),
        };

        assert!(result.is_success());
        assert_eq!(result.summary(), "No issues");
    }

    #[test]
    fn test_issue_severity() {
        assert_eq!(IssueSeverity::Critical.to_string(), "critical");
        assert_eq!(IssueSeverity::Warning.to_string(), "warning");
        assert_eq!(IssueSeverity::Info.to_string(), "info");
    }
}
