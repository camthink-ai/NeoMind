//! Trend analysis tool for time series data.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use edge_ai_storage::TimeSeriesStore;
use edge_ai_tools::{Tool, ToolOutput, ToolError, error::Result as ToolResult, tool::{object_schema, string_property, number_property, boolean_property}};

/// Trend analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    /// Metric name
    pub metric: String,
    /// Device ID
    pub device_id: String,
    /// Data points analyzed
    pub data_points: Vec<TrendDataPoint>,
    /// Trend direction
    pub trend: TrendDirection,
    /// Trend strength (-1 to 1)
    pub strength: f64,
    /// Statistical summary
    pub summary: TrendSummary,
    /// Predictions
    pub predictions: Option<TrendPredictions>,
}

/// A single data point in trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendDataPoint {
    /// Timestamp
    pub timestamp: i64,
    /// Value
    pub value: f64,
}

/// Direction of the trend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrendDirection {
    Increasing,
    Decreasing,
    Stable,
    Volatile,
}

/// Statistical summary of the trend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendSummary {
    /// Mean value
    pub mean: f64,
    /// Median value
    pub median: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Coefficient of variation
    pub cv: f64,
}

/// Predictions based on trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPredictions {
    /// Predicted value in 1 hour
    pub next_1h: Option<f64>,
    /// Predicted value in 6 hours
    pub next_6h: Option<f64>,
    /// Predicted value in 24 hours
    pub next_24h: Option<f64>,
    /// Confidence level (0-100)
    pub confidence: f32,
}

/// Tool for analyzing trends in time series data.
pub struct AnalyzeTrendsTool {
    storage: Arc<TimeSeriesStore>,
}

impl AnalyzeTrendsTool {
    /// Create a new trend analysis tool.
    pub fn new(storage: Arc<TimeSeriesStore>) -> Self {
        Self { storage }
    }

    /// Analyze trend from data points.
    fn analyze_trend(&self, data: &[TrendDataPoint]) -> (TrendDirection, f64) {
        if data.len() < 2 {
            return (TrendDirection::Stable, 0.0);
        }

        // Calculate linear regression slope
        let n = data.len() as f64;
        let sum_x: f64 = (0..data.len()).map(|i| i as f64).sum();
        let sum_y: f64 = data.iter().map(|d| d.value).sum();
        let sum_xy: f64 = data.iter().enumerate().map(|(i, d)| i as f64 * d.value).sum();
        let sum_x2: f64 = (0..data.len()).map(|i| (i as f64) * (i as f64)).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);

        // Calculate correlation coefficient (R)
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;
        let numerator: f64 = data.iter().enumerate()
            .map(|(i, d)| (i as f64 - mean_x) * (d.value - mean_y))
            .sum();
        let sum_xx: f64 = data.iter().enumerate()
            .map(|(i, _)| (i as f64 - mean_x).powi(2))
            .sum();
        let sum_yy: f64 = data.iter()
            .map(|d| (d.value - mean_y).powi(2))
            .sum();
        let r = numerator / (sum_xx * sum_yy).sqrt();

        // Determine trend direction
        let direction = if r.abs() < 0.3 {
            TrendDirection::Stable
        } else if slope > 0.0 {
            TrendDirection::Increasing
        } else {
            TrendDirection::Decreasing
        };

        (direction, r)
    }

    /// Calculate statistical summary.
    fn calculate_summary(&self, data: &[TrendDataPoint]) -> TrendSummary {
        if data.is_empty() {
            return TrendSummary {
                mean: 0.0,
                median: 0.0,
                std_dev: 0.0,
                min: 0.0,
                max: 0.0,
                cv: 0.0,
            };
        }

        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let n = values.len();

        let mean = values.iter().sum::<f64>() / n as f64;

        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = if n % 2 == 0 {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        } else {
            sorted[n / 2]
        };

        let variance = values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

        let cv = if mean != 0.0 { std_dev / mean.abs() } else { 0.0 };

        TrendSummary {
            mean,
            median,
            std_dev,
            min,
            max,
            cv,
        }
    }

    /// Generate predictions based on trend.
    fn generate_predictions(&self, data: &[TrendDataPoint], slope: f64, r: f64) -> TrendPredictions {
        if data.is_empty() || r.abs() < 0.3 {
            return TrendPredictions {
                next_1h: None,
                next_6h: None,
                next_24h: None,
                confidence: (r.abs() * 100.0) as f32,
            };
        }

        let last_value = data.last().unwrap().value;
        let last_timestamp = data.last().unwrap().timestamp;

        // Estimate time interval between points
        let interval = if data.len() > 1 {
            (data[1].timestamp - data[0].timestamp) as f64
        } else {
            3600.0 // Default to 1 hour
        };

        // Predict future values
        let next_1h = last_value + slope * (3600.0 / interval);
        let next_6h = last_value + slope * (6.0 * 3600.0 / interval);
        let next_24h = last_value + slope * (24.0 * 3600.0 / interval);

        TrendPredictions {
            next_1h: Some(next_1h),
            next_6h: Some(next_6h),
            next_24h: Some(next_24h),
            confidence: (r.abs() * 100.0) as f32,
        }
    }
}

#[async_trait]
impl Tool for AnalyzeTrendsTool {
    fn name(&self) -> &str {
        "analyze_trends"
    }

    fn description(&self) -> &str {
        "Analyze trends in time series data. Use this to identify patterns, predict future values, and detect changes in device metrics."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("The ID of the device to analyze"),
                "metric": string_property("The metric name to analyze (e.g., 'temperature', 'humidity')"),
                "start_time": number_property("Start timestamp (Unix epoch). Optional, defaults to 24 hours ago."),
                "end_time": number_property("End timestamp (Unix epoch). Optional, defaults to now."),
                "predict": boolean_property("Whether to generate predictions. Optional, defaults to true.")
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

        let predict = args["predict"].as_bool().unwrap_or(true);

        // Query data from storage
        let result = self
            .storage
            .query_range(device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query data: {}", e)))?;

        // Convert to trend data points
        let data_points: Vec<TrendDataPoint> = result.points.iter().map(|p| TrendDataPoint {
            timestamp: p.timestamp,
            value: p.value.as_f64().unwrap_or(0.0),
        }).collect();

        if data_points.is_empty() {
            return Ok(ToolOutput::success_with_metadata(
                serde_json::json!({
                    "device_id": device_id,
                    "metric": metric,
                    "message": "No data available for the specified time range"
                }),
                serde_json::json!({"has_data": false})
            ));
        }

        // Analyze trend
        let (trend, strength) = self.analyze_trend(&data_points);

        // Calculate summary
        let summary = self.calculate_summary(&data_points);

        // Generate predictions if requested
        let predictions = if predict {
            Some(self.generate_predictions(&data_points, 0.0, strength))
        } else {
            None
        };

        let analysis = TrendAnalysis {
            metric: metric.to_string(),
            device_id: device_id.to_string(),
            data_points,
            trend,
            strength: strength.abs(),
            summary,
            predictions,
        };

        Ok(ToolOutput::success_with_metadata(
            serde_json::to_value(&analysis).unwrap(),
            serde_json::json!({
                "has_data": true,
                "data_points_count": analysis.data_points.len()
            })
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trend_direction() {
        let tool = AnalyzeTrendsTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        let data = vec![
            TrendDataPoint { timestamp: 1000, value: 10.0 },
            TrendDataPoint { timestamp: 2000, value: 12.0 },
            TrendDataPoint { timestamp: 3000, value: 14.0 },
            TrendDataPoint { timestamp: 4000, value: 16.0 },
        ];

        let (direction, strength) = tool.analyze_trend(&data);
        assert_eq!(direction, TrendDirection::Increasing);
        assert!(strength > 0.5); // Strong positive correlation
    }

    #[test]
    fn test_trend_summary() {
        let tool = AnalyzeTrendsTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        let data = vec![
            TrendDataPoint { timestamp: 1000, value: 10.0 },
            TrendDataPoint { timestamp: 2000, value: 20.0 },
            TrendDataPoint { timestamp: 3000, value: 30.0 },
            TrendDataPoint { timestamp: 4000, value: 40.0 },
        ];

        let summary = tool.calculate_summary(&data);
        assert_eq!(summary.mean, 25.0);
        assert_eq!(summary.min, 10.0);
        assert_eq!(summary.max, 40.0);
    }

    #[test]
    fn test_trend_predict_stable() {
        let tool = AnalyzeTrendsTool::new(
            edge_ai_storage::TimeSeriesStore::memory().unwrap()
        );

        let data = vec![
            TrendDataPoint { timestamp: 1000, value: 20.0 },
            TrendDataPoint { timestamp: 2000, value: 21.0 },
            TrendDataPoint { timestamp: 3000, value: 19.0 },
            TrendDataPoint { timestamp: 4000, value: 20.5 },
        ];

        let predictions = tool.generate_predictions(&data, 0.0, 0.1);
        assert!(predictions.confidence < 50.0); // Low confidence for weak trend
    }
}
