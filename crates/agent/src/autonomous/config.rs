//! Configuration for the autonomous agent.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Review type for autonomous analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewType {
    /// Device health check
    DeviceHealth,
    /// Trend analysis
    TrendAnalysis,
    /// Anomaly detection
    AnomalyDetection,
    /// Energy optimization
    EnergyOptimization,
    /// Rule performance analysis
    RulePerformance,
    /// Workflow optimization
    WorkflowOptimization,
}

impl ReviewType {
    /// Get all review types.
    pub fn all() -> Vec<ReviewType> {
        vec![
            ReviewType::DeviceHealth,
            ReviewType::TrendAnalysis,
            ReviewType::AnomalyDetection,
            ReviewType::EnergyOptimization,
            ReviewType::RulePerformance,
            ReviewType::WorkflowOptimization,
        ]
    }

    /// Get the review type name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeviceHealth => "device_health",
            Self::TrendAnalysis => "trend_analysis",
            Self::AnomalyDetection => "anomaly_detection",
            Self::EnergyOptimization => "energy_optimization",
            Self::RulePerformance => "rule_performance",
            Self::WorkflowOptimization => "workflow_optimization",
        }
    }

    /// Get the review type display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::DeviceHealth => "Device Health Check",
            Self::TrendAnalysis => "Trend Analysis",
            Self::AnomalyDetection => "Anomaly Detection",
            Self::EnergyOptimization => "Energy Optimization",
            Self::RulePerformance => "Rule Performance",
            Self::WorkflowOptimization => "Workflow Optimization",
        }
    }

    /// Get the description for this review type.
    pub fn description(&self) -> &'static str {
        match self {
            Self::DeviceHealth => "Check device connectivity, status, and health metrics",
            Self::TrendAnalysis => "Analyze trends in device metrics over time",
            Self::AnomalyDetection => "Detect anomalies and unusual patterns in system behavior",
            Self::EnergyOptimization => "Identify opportunities for energy efficiency improvements",
            Self::RulePerformance => "Analyze rule execution performance and trigger rates",
            Self::WorkflowOptimization => "Review workflow execution and suggest optimizations",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "device_health" => Some(Self::DeviceHealth),
            "trend_analysis" => Some(Self::TrendAnalysis),
            "anomaly_detection" => Some(Self::AnomalyDetection),
            "energy_optimization" => Some(Self::EnergyOptimization),
            "rule_performance" => Some(Self::RulePerformance),
            "workflow_optimization" => Some(Self::WorkflowOptimization),
            _ => None,
        }
    }
}

impl std::fmt::Display for ReviewType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for the autonomous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousConfig {
    /// Review interval in seconds
    pub review_interval_secs: u64,
    /// Types of reviews to perform
    pub review_types: Vec<ReviewType>,
    /// Whether the agent is enabled
    pub enabled: bool,
    /// Maximum concurrent reviews
    pub max_concurrent_reviews: usize,
    /// Review timeout in seconds
    pub review_timeout_secs: u64,
    /// Minimum data points required for trend analysis
    pub min_trend_data_points: usize,
    /// Anomaly detection threshold (standard deviations)
    pub anomaly_threshold_std: f64,
}

impl Default for AutonomousConfig {
    fn default() -> Self {
        Self {
            review_interval_secs: 300, // 5 minutes
            review_types: ReviewType::all(),
            enabled: false,
            max_concurrent_reviews: 3,
            review_timeout_secs: 60,
            min_trend_data_points: 10,
            anomaly_threshold_std: 2.0,
        }
    }
}

impl AutonomousConfig {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom review interval.
    pub fn with_interval(mut self, secs: u64) -> Self {
        self.review_interval_secs = secs;
        self
    }

    /// Create with specific review types.
    pub fn with_review_types(mut self, types: Vec<ReviewType>) -> Self {
        self.review_types = types;
        self
    }

    /// Create with enabled flag.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Get the review interval as Duration.
    pub fn interval_duration(&self) -> Duration {
        Duration::from_secs(self.review_interval_secs)
    }

    /// Get the review timeout as Duration.
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.review_timeout_secs)
    }

    /// Check if a review type is enabled.
    pub fn is_review_enabled(&self, review_type: ReviewType) -> bool {
        self.review_types.contains(&review_type)
    }

    /// Get the set of enabled review types.
    pub fn enabled_reviews(&self) -> HashSet<ReviewType> {
        self.review_types.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_type_from_str() {
        assert_eq!(
            ReviewType::from_str("device_health"),
            Some(ReviewType::DeviceHealth)
        );
        assert_eq!(
            ReviewType::from_str("DEVICE_HEALTH"),
            Some(ReviewType::DeviceHealth)
        );
        assert_eq!(ReviewType::from_str("invalid"), None);
    }

    #[test]
    fn test_review_type_display() {
        assert_eq!(ReviewType::DeviceHealth.as_str(), "device_health");
        assert_eq!(
            ReviewType::DeviceHealth.display_name(),
            "Device Health Check"
        );
    }

    #[test]
    fn test_config_default() {
        let config = AutonomousConfig::default();
        assert_eq!(config.review_interval_secs, 300);
        assert!(!config.enabled);
        assert_eq!(config.review_types.len(), 6);
    }

    #[test]
    fn test_config_builder() {
        let config = AutonomousConfig::new()
            .with_interval(600)
            .with_enabled(true)
            .with_review_types(vec![ReviewType::DeviceHealth]);

        assert_eq!(config.review_interval_secs, 600);
        assert!(config.enabled);
        assert_eq!(config.review_types.len(), 1);
    }

    #[test]
    fn test_is_review_enabled() {
        let config = AutonomousConfig::new()
            .with_review_types(vec![ReviewType::DeviceHealth, ReviewType::TrendAnalysis]);

        assert!(config.is_review_enabled(ReviewType::DeviceHealth));
        assert!(config.is_review_enabled(ReviewType::TrendAnalysis));
        assert!(!config.is_review_enabled(ReviewType::AnomalyDetection));
    }
}
