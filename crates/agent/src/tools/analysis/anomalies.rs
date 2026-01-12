//! Anomaly detection tool for time series data.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use edge_ai_storage::TimeSeriesStore;
use edge_ai_tools::{Tool, ToolOutput, ToolError, error::Result as ToolResult, tool::{object_schema, string_property, number_property, array_property}};

/// Anomaly detection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyDetectionResult {
    /// Metric name
    pub metric: String,
    /// Device ID
    pub device_id: String,
    /// Data points analyzed
    pub data_point_count: usize,
    /// Detected anomalies
    pub anomalies: Vec<Anomaly>,
    /// Statistical summary
    pub summary: AnomalySummary,
}

/// A detected anomaly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    /// Anomaly ID
    pub id: String,
    /// Timestamp of the anomaly
    pub timestamp: i64,
    /// Anomalous value
    pub value: f64,
    /// Expected value
    pub expected: f64,
    /// Deviation from expected (in standard deviations)
    pub deviation: f64,
    /// Severity level
    pub severity: AnomalySeverity,
    /// Possible causes
    pub causes: Vec<String>,
}

/// Severity of an anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnomalySeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Summary of anomaly detection statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalySummary {
    /// Total anomalies detected
    pub total_count: usize,
    /// Count by severity
    pub by_severity: AnomalySeverityBreakdown,
    /// Detection method used
    pub method: DetectionMethod,
    /// Threshold used (in standard deviations)
    pub threshold_std: f64,
}

/// Breakdown of anomalies by severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalySeverityBreakdown {
    pub low: usize,
    pub medium: usize,
    pub high: usize,
    pub critical: usize,
}

/// Method used for anomaly detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    ZScore,
    IQR,
    MovingAverage,
    IsolationForest,
}

/// Tool for detecting anomalies in time series data.
pub struct DetectAnomaliesTool {
    storage: Arc<TimeSeriesStore>,
    /// Default threshold in standard deviations
    default_threshold: f64,
}

impl DetectAnomaliesTool {
    /// Create a new anomaly detection tool.
    pub fn new(storage: Arc<TimeSeriesStore>) -> Self {
        Self {
            storage,
            default_threshold: 3.0, // 3 standard deviations
        }
    }

    /// Create with custom threshold.
    pub fn with_threshold(storage: Arc<TimeSeriesStore>, threshold: f64) -> Self {
        Self {
            storage,
            default_threshold: threshold,
        }
    }

    /// Detect anomalies using Z-score method.
    fn detect_z_score(&self, values: &[f64], timestamps: &[i64], threshold: f64) -> Vec<Anomaly> {
        if values.len() < 3 {
            return Vec::new();
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Vec::new();
        }

        let mut anomalies = Vec::new();

        for (i, (&value, &timestamp)) in values.iter().zip(timestamps.iter()).enumerate() {
            let z_score = (value - mean) / std_dev;

            if z_score.abs() > threshold {
                let severity = match z_score.abs() {
                    s if s >= 5.0 => AnomalySeverity::Critical,
                    s if s >= 4.0 => AnomalySeverity::High,
                    s if s >= 3.0 => AnomalySeverity::Medium,
                    _ => AnomalySeverity::Low,
                };

                anomalies.push(Anomaly {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp,
                    value,
                    expected: mean,
                    deviation: z_score,
                    severity,
                    causes: self.analyze_causes(value, mean, std_dev),
                });
            }
        }

        anomalies
    }

    /// Analyze possible causes for an anomaly.
    fn analyze_causes(&self, value: f64, mean: f64, std_dev: f64) -> Vec<String> {
        let mut causes = Vec::new();

        let diff = value - mean;

        if diff > 0.0 {
            causes.push("Unexpected increase in metric value".to_string());
            if std_dev / mean.abs() > 0.5 {
                causes.push("High variability in recent measurements".to_string());
            }
        } else {
            causes.push("Unexpected decrease in metric value".to_string());
            causes.push("Possible sensor malfunction or disconnection".to_string());
        }

        causes
    }
}

#[async_trait]
impl Tool for DetectAnomaliesTool {
    fn name(&self) -> &str {
        "detect_anomalies"
    }

    fn description(&self) -> &str {
        "Detect anomalies in time series data using statistical methods. Use this to identify unusual patterns, outliers, or potential issues."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("The ID of the device to analyze"),
                "metric": string_property("The metric name to analyze (e.g., 'temperature', 'humidity')"),
                "start_time": number_property("Start timestamp (Unix epoch). Optional, defaults to 24 hours ago."),
                "end_time": number_property("End timestamp (Unix epoch). Optional, defaults to now."),
                "threshold": number_property("Detection threshold in standard deviations. Optional, defaults to 3."),
                "method": string_property("Detection method: 'z_score' or 'iqr'. Optional, defaults to 'z_score'.")
            }),
            vec!["device_id".to_string(), "metric".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let device_id = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        let metric = args["metric"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("metric must be a string".to_string()))?;

        let end_time = args["end_time"]
            .as_i64()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let start_time = args["start_time"]
            .as_i64()
            .unwrap_or(end_time - 86400); // Default 24 hours

        let threshold = args["threshold"]
            .as_f64()
            .unwrap_or(self.default_threshold);

        let _method = args["method"]
            .as_str()
            .unwrap_or("z_score");

        // Query data from storage
        let result = self
            .storage
            .query_range(device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query data: {}", e)))?;

        if result.points.is_empty() {
            return Ok(ToolOutput::success_with_metadata(
                serde_json::json!({
                    "device_id": device_id,
                    "metric": metric,
                    "message": "No data available for the specified time range"
                }),
                serde_json::json!({"has_data": false})
            ));
        }

        // Extract values and timestamps
        let values: Vec<f64> = result.points.iter().map(|p| p.value.as_f64().unwrap_or(0.0)).collect();
        let timestamps: Vec<i64> = result.points.iter().map(|p| p.timestamp).collect();

        // Detect anomalies
        let anomalies = self.detect_z_score(&values, &timestamps, threshold);

        // Calculate severity breakdown
        let mut by_severity = AnomalySeverityBreakdown {
            low: 0,
            medium: 0,
            high: 0,
            critical: 0,
        };

        for anomaly in &anomalies {
            match anomaly.severity {
                AnomalySeverity::Low => by_severity.low += 1,
                AnomalySeverity::Medium => by_severity.medium += 1,
                AnomalySeverity::High => by_severity.high += 1,
                AnomalySeverity::Critical => by_severity.critical += 1,
            }
        }

        let summary = AnomalySummary {
            total_count: anomalies.len(),
            by_severity,
            method: DetectionMethod::ZScore,
            threshold_std: threshold,
        };

        let detection_result = AnomalyDetectionResult {
            metric: metric.to_string(),
            device_id: device_id.to_string(),
            data_point_count: result.points.len(),
            anomalies,
            summary,
        };

        Ok(ToolOutput::success_with_metadata(
            serde_json::to_value(&detection_result).unwrap(),
            serde_json::json!({
                "has_data": true,
                "anomalies_found": !detection_result.anomalies.is_empty()
            })
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_anomalies() {
        let tool = DetectAnomaliesTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        let values = vec
![10.0, 11.0, 10.5, 10.2, 10.8, // Normal
                           25.0, // Anomaly
                           10.3, 10.7, 10.1];
        let timestamps: Vec<i64> = (0..values.len() as i64).map(|i| 1000 + i * 100).collect();

        let anomalies = tool.detect_z_score(&values, &timestamps, 2.0);

        assert_eq!(anomalies.len(), 1);
        assert_eq!(anomalies[0].value, 25.0);
    }

    #[test]
    fn test_no_anomalies() {
        let tool = DetectAnomaliesTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        let values = vec
![10.0, 11.0, 10.5, 10.2, 10.8, 10.3, 10.7, 10.1];
        let timestamps: Vec<i64> = (0..values.len() as i64).map(|i| 1000 + i * 100).collect();

        let anomalies = tool.detect_z_score(&values, &timestamps, 2.0);

        assert_eq!(anomalies.len(), 0);
    }

    #[test]
    fn test_severity_classification() {
        let tool = DetectAnomaliesTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        // Many normal values with extreme outlier
        // With 30 values of 10.0 and one 60.0, z-score ≈ 5.3 (Critical)
        let values = vec
![10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
                           10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
                           10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
                           60.0, // Extreme outlier
        ];
        let timestamps: Vec<i64> = (0..values.len() as i64).map(|i| 1000 + i * 100).collect();

        // Detect anomalies
        let anomalies = tool.detect_z_score(&values, &timestamps, 3.0);

        // Should detect at least 1 anomaly
        assert!(anomalies.len() >= 1);
        // Anomaly should be critical severity (>5 SD)
        assert_eq!(anomalies[0].severity, AnomalySeverity::Critical);
    }
}
