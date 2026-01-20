//! Statistics analyzer for value distribution and pattern detection.
//!
//! This module provides statistical analysis of metric values without LLM dependency.
//! It computes ranges, distributions, and detects patterns like units, boolean-like values.

use serde_json::Value;
use std::collections::HashMap;

/// Statistical analysis result for a single path
#[derive(Debug, Clone)]
pub struct StatisticsResult {
    /// Path being analyzed
    pub path: String,
    /// Value statistics
    pub stats: ValueStatistics,
    /// Detected unit (if any)
    pub unit: Option<String>,
    /// Detected value type pattern
    pub pattern: ValuePattern,
    /// Quality score (0.0 - 1.0)
    pub quality_score: f32,
}

/// Statistical measures for numeric values
#[derive(Debug, Clone)]
pub struct NumericStatistics {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Mean (average)
    pub mean: f64,
    /// Median value
    pub median: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Variance
    pub variance: f64,
    /// 25th percentile
    pub p25: f64,
    /// 75th percentile
    pub p75: f64,
    /// Total samples
    pub count: usize,
}

/// Statistical measures for string values
#[derive(Debug, Clone)]
pub struct StringStatistics {
    /// Minimum length
    pub min_length: usize,
    /// Maximum length
    pub max_length: usize,
    /// Average length
    pub avg_length: f64,
    /// Most common value
    pub mode: Option<String>,
    /// Frequency of most common value
    pub mode_frequency: usize,
    /// Number of unique values
    pub unique_count: usize,
    /// Total samples
    pub count: usize,
}

/// Combined statistics for any value type
#[derive(Debug, Clone)]
pub enum ValueStatistics {
    /// Numeric statistics
    Numeric(NumericStatistics),
    /// String statistics
    String(StringStatistics),
    /// Boolean statistics
    Boolean { true_count: usize, false_count: usize, null_count: usize },
    /// Unknown/mixed type
    Unknown,
}

/// Detected value pattern
#[derive(Debug, Clone, PartialEq)]
pub enum ValuePattern {
    /// Regular numeric value
    Numeric,
    /// Percentage (0-100 range)
    Percentage,
    /// Temperature in Celsius
    TemperatureCelsius,
    /// Temperature in Fahrenheit
    TemperatureFahrenheit,
    /// Boolean-like (0/1, true/false)
    BooleanLike,
    /// Hex-encoded value
    HexLike,
    /// Timestamp/unix time
    Timestamp,
    /// Enumeration (few distinct string values)
    Enumeration(Vec<String>),
    /// Identifier/UUID
    Identifier,
    /// Raw data
    RawData,
    /// Unknown pattern
    Unknown,
}

impl ValuePattern {
    /// Get display name for the pattern
    pub fn display_name(&self) -> &'static str {
        match self {
            ValuePattern::Numeric => "numeric",
            ValuePattern::Percentage => "percentage",
            ValuePattern::TemperatureCelsius => "temperature_celsius",
            ValuePattern::TemperatureFahrenheit => "temperature_fahrenheit",
            ValuePattern::BooleanLike => "boolean_like",
            ValuePattern::HexLike => "hex_like",
            ValuePattern::Timestamp => "timestamp",
            ValuePattern::Enumeration(_) => "enumeration",
            ValuePattern::Identifier => "identifier",
            ValuePattern::RawData => "raw_data",
            ValuePattern::Unknown => "unknown",
        }
    }

    /// Get suggested semantic type based on pattern
    pub fn suggested_semantic_type(&self) -> &'static str {
        match self {
            ValuePattern::Numeric => "measurement",
            ValuePattern::Percentage => "percentage",
            ValuePattern::TemperatureCelsius => "temperature",
            ValuePattern::TemperatureFahrenheit => "temperature",
            ValuePattern::BooleanLike => "status",
            ValuePattern::HexLike => "raw_value",
            ValuePattern::Timestamp => "timestamp",
            ValuePattern::Enumeration(_) => "state",
            ValuePattern::Identifier => "identifier",
            ValuePattern::RawData => "data",
            ValuePattern::Unknown => "value",
        }
    }
}

/// Statistics analyzer - computes value statistics and detects patterns
pub struct StatisticsAnalyzer {
    /// Minimum samples required for meaningful statistics
    min_samples: usize,
    /// Maximum samples to analyze (for performance)
    max_samples: usize,
}

impl StatisticsAnalyzer {
    /// Create a new statistics analyzer
    pub fn new() -> Self {
        Self {
            min_samples: 3,
            max_samples: 1000,
        }
    }

    /// Set minimum samples threshold
    pub fn with_min_samples(mut self, min: usize) -> Self {
        self.min_samples = min.min(1);
        self
    }

    /// Set maximum samples limit
    pub fn with_max_samples(mut self, max: usize) -> Self {
        self.max_samples = max.max(1);
        self
    }

    /// Analyze values for a single path
    pub fn analyze_path(&self, path: &str, values: &[Value]) -> StatisticsResult {
        let stats = self.compute_statistics(values);
        let unit = self.detect_unit(&stats, path);
        let pattern = self.detect_pattern(&stats, path, &unit);
        let quality_score = self.compute_quality_score(&stats, values.len());

        StatisticsResult {
            path: path.to_string(),
            stats,
            unit,
            pattern,
            quality_score,
        }
    }

    /// Compute statistics from values
    fn compute_statistics(&self, values: &[Value]) -> ValueStatistics {
        if values.is_empty() {
            return ValueStatistics::Unknown;
        }

        // Categorize values
        let mut numeric_values: Vec<f64> = Vec::new();
        let mut string_values: Vec<String> = Vec::new();
        let mut true_count = 0;
        let mut false_count = 0;
        let mut null_count = 0;

        for value in values {
            match value {
                Value::Number(n) => {
                    if let Some(f) = n.as_f64() {
                        numeric_values.push(f);
                    }
                }
                Value::String(s) => {
                    string_values.push(s.clone());
                }
                Value::Bool(b) => {
                    if *b {
                        true_count += 1;
                    } else {
                        false_count += 1;
                    }
                }
                Value::Null => {
                    null_count += 1;
                }
                Value::Array(_) | Value::Object(_) => {
                    // Complex types - skip for statistics
                }
            }
        }

        // Determine dominant type
        let total = values.len();
        let numeric_ratio = numeric_values.len() as f64 / total as f64;
        let boolean_ratio = (true_count + false_count) as f64 / total as f64;
        let string_ratio = string_values.len() as f64 / total as f64;

        if numeric_ratio > 0.7 {
            ValueStatistics::Numeric(self.compute_numeric_stats(&numeric_values))
        } else if boolean_ratio > 0.7 {
            ValueStatistics::Boolean { true_count, false_count, null_count }
        } else if string_ratio > 0.5 {
            ValueStatistics::String(self.compute_string_stats(&string_values))
        } else {
            ValueStatistics::Unknown
        }
    }

    /// Compute numeric statistics
    fn compute_numeric_stats(&self, values: &[f64]) -> NumericStatistics {
        if values.is_empty() {
            return NumericStatistics {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                median: 0.0,
                std_dev: 0.0,
                variance: 0.0,
                p25: 0.0,
                p75: 0.0,
                count: 0,
            };
        }

        let count = values.len();
        let sorted = {
            let mut v = values.to_vec();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v
        };

        let min = sorted[0];
        let max = sorted[count - 1];
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let median = if count % 2 == 0 {
            (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
        } else {
            sorted[count / 2]
        };

        let p25 = sorted[count / 4];
        let p75 = sorted[count * 3 / 4];

        let variance = values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / count as f64;

        let std_dev = variance.sqrt();

        NumericStatistics {
            min,
            max,
            mean,
            median,
            std_dev,
            variance,
            p25,
            p75,
            count,
        }
    }

    /// Compute string statistics
    fn compute_string_stats(&self, values: &[String]) -> StringStatistics {
        if values.is_empty() {
            return StringStatistics {
                min_length: 0,
                max_length: 0,
                avg_length: 0.0,
                mode: None,
                mode_frequency: 0,
                unique_count: 0,
                count: 0,
            };
        }

        let count = values.len();
        let lengths: Vec<usize> = values.iter().map(|s| s.len()).collect();
        let min_length = *lengths.iter().min().unwrap_or(&0);
        let max_length = *lengths.iter().max().unwrap_or(&0);
        let avg_length = lengths.iter().sum::<usize>() as f64 / count as f64;

        // Find mode
        let mut freq_map: HashMap<String, usize> = HashMap::new();
        for s in values {
            *freq_map.entry(s.clone()).or_insert(0) += 1;
        }

        let unique_count = freq_map.len();
        let (mode, mode_frequency) = freq_map
            .into_iter()
            .max_by_key(|(_, freq)| *freq)
            .unzip();

        StringStatistics {
            min_length,
            max_length,
            avg_length,
            mode,
            mode_frequency: mode_frequency.unwrap_or(0),
            unique_count,
            count,
        }
    }

    /// Detect unit from statistics and path
    fn detect_unit(&self, stats: &ValueStatistics, path: &str) -> Option<String> {
        // Check path hints first
        let path_lower = path.to_lowercase();

        if let ValueStatistics::Numeric(num_stats) = stats {
            // Temperature ranges
            if num_stats.min >= -50.0 && num_stats.max <= 60.0 {
                if path_lower.contains("temp") || path_lower.contains("temperature") {
                    return Some("°C".to_string());
                }
            }
            if num_stats.min >= 30.0 && num_stats.max <= 120.0 {
                if path_lower.contains("temp") || path_lower.contains("temperature") {
                    return Some("°F".to_string());
                }
            }

            // Percentage range (0-100)
            if num_stats.min >= 0.0 && num_stats.max <= 100.0 {
                if path_lower.contains("humidity") || path_lower.contains("level")
                    || path_lower.contains("percent") || path_lower.contains("ratio")
                {
                    return Some("%".to_string());
                }
            }

            // Pressure ranges
            if num_stats.min >= 900.0 && num_stats.max <= 1100.0 {
                if path_lower.contains("pressure") || path_lower.contains("bar") {
                    return Some("hPa".to_string());
                }
            }
            if num_stats.min >= 28.0 && num_stats.max <= 32.0 {
                if path_lower.contains("pressure") || path_lower.contains("in") {
                    return Some("inHg".to_string());
                }
            }
        }

        // Check for explicit unit indicators in path
        for (keyword, unit) in &[
            ("temp", "°C"),
            ("temperature", "°C"),
            ("humidity", "%"),
            ("pressure", "Pa"),
            ("power", "W"),
            ("voltage", "V"),
            ("current", "A"),
            ("energy", "kWh"),
            ("frequency", "Hz"),
            ("lux", "lx"),
            ("ppm", "ppm"),
            ("co2", "ppm"),
        ] {
            if path_lower.contains(keyword) {
                return Some(unit.to_string());
            }
        }

        None
    }

    /// Detect value pattern
    fn detect_pattern(&self, stats: &ValueStatistics, path: &str, unit: &Option<String>) -> ValuePattern {
        let path_lower = path.to_lowercase();

        match stats {
            ValueStatistics::Numeric(num) => {
                // Timestamp pattern (unix time in seconds or milliseconds)
                if num.min >= 1_000_000_000.0 && num.max < 2_000_000_000.0 {
                    return ValuePattern::Timestamp;
                }
                if num.min >= 1_000_000_000_000.0 && num.max < 2_000_000_000_000.0 {
                    return ValuePattern::Timestamp;
                }

                // Percentage pattern
                if unit.as_deref() == Some("%") {
                    return ValuePattern::Percentage;
                }
                if num.min >= 0.0 && num.max <= 100.0 {
                    if path_lower.contains("humidity") || path_lower.contains("level")
                        || path_lower.contains("percent") || path_lower.contains("ratio")
                    {
                        return ValuePattern::Percentage;
                    }
                }

                // Temperature patterns
                if unit.as_deref() == Some("°C") {
                    return ValuePattern::TemperatureCelsius;
                }
                if unit.as_deref() == Some("°F") {
                    return ValuePattern::TemperatureFahrenheit;
                }

                // Boolean-like (0/1 values)
                if num.min >= 0.0 && num.max <= 1.0 && num.std_dev < 0.6 {
                    if num.p25 == num.median && num.p75 == num.median {
                        // Most values are either 0 or 1
                        return ValuePattern::BooleanLike;
                    }
                }

                ValuePattern::Numeric
            }
            ValueStatistics::Boolean { .. } => ValuePattern::BooleanLike,
            ValueStatistics::String(str_stats) => {
                // Enumeration pattern (few unique values)
                if str_stats.unique_count <= 10 && str_stats.unique_count < str_stats.count / 2 {
                    let mut unique_values: Vec<String> = vec![];
                    // We'd need the original values to get this right
                    return ValuePattern::Enumeration(unique_values);
                }

                // UUID/Identifier pattern
                if str_stats.avg_length >= 32.0 && str_stats.avg_length <= 40.0 {
                    if str_stats.min_length == str_stats.max_length {
                        return ValuePattern::Identifier;
                    }
                }

                // Check for hex-like strings
                if str_stats.unique_count > 10 && str_stats.avg_length > 8.0 {
                    if path_lower.contains("id") || path_lower.contains("uuid") || path_lower.contains("key") {
                        return ValuePattern::Identifier;
                    }
                }

                ValuePattern::Unknown
            }
            ValueStatistics::Unknown => ValuePattern::Unknown,
        }
    }

    /// Compute quality score based on data characteristics
    fn compute_quality_score(&self, stats: &ValueStatistics, sample_count: usize) -> f32 {
        let mut score = 1.0;

        // Reduce score for low sample count
        if sample_count < self.min_samples {
            score *= 0.3;
        } else if sample_count < self.min_samples * 2 {
            score *= 0.6;
        }

        // Adjust based on statistics type
        match stats {
            ValueStatistics::Numeric(num) => {
                // High variance might indicate inconsistent data
                if num.std_dev > (num.max - num.min) / 2.0 {
                    score *= 0.8;
                }
            }
            ValueStatistics::String(str_stats) => {
                // Too many unique values might be IDs, not metrics
                if str_stats.unique_count as f64 > str_stats.count as f64 * 0.9 {
                    score *= 0.7;
                }
            }
            _ => {}
        }

        (score as f32).clamp(0.0, 1.0)
    }
}

impl Default for StatisticsAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Quick statistics computation helper
pub fn compute_quick_stats(values: &[Value]) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut nums: Vec<f64> = Vec::new();

    for v in values {
        if let Value::Number(n) = v {
            if let Some(f) = n.as_f64() {
                nums.push(f);
            }
        }
    }

    if nums.is_empty() {
        return (None, None, None);
    }

    let min = nums.iter().cloned().reduce(f64::min).unwrap();
    let max = nums.iter().cloned().reduce(f64::max).unwrap();
    let mean: f64 = nums.iter().sum::<f64>() / nums.len() as f64;

    (Some(min), Some(max), Some(mean))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_numeric_statistics() {
        let analyzer = StatisticsAnalyzer::new();
        let values = vec![json!(10.0), json!(20.0), json!(30.0), json!(40.0), json!(50.0)];

        let result = analyzer.analyze_path("$.test", &values);

        assert!(matches!(result.stats, ValueStatistics::Numeric(_)));
        if let ValueStatistics::Numeric(num) = result.stats {
            assert_eq!(num.min, 10.0);
            assert_eq!(num.max, 50.0);
            assert_eq!(num.mean, 30.0);
            assert_eq!(num.median, 30.0);
        }
    }

    #[test]
    fn test_percentage_detection() {
        let analyzer = StatisticsAnalyzer::new();
        let values: Vec<Value> = (0..=100).step_by(10).map(|i| json!(i)).collect();

        let result = analyzer.analyze_path("$.humidity", &values);

        assert_eq!(result.pattern, ValuePattern::Percentage);
        assert_eq!(result.unit, Some("%".to_string()));
    }

    #[test]
    fn test_temperature_detection() {
        let analyzer = StatisticsAnalyzer::new();
        let values = vec![json!(20.5), json!(21.0), json!(19.8), json!(22.1)];

        let result = analyzer.analyze_path("$.temperature", &values);

        assert_eq!(result.unit, Some("°C".to_string()));
    }

    #[test]
    fn test_boolean_like_detection() {
        let analyzer = StatisticsAnalyzer::new();
        let values = vec![json!(0), json!(1), json!(0), json!(1), json!(1)];

        let result = analyzer.analyze_path("$.status", &values);

        assert_eq!(result.pattern, ValuePattern::BooleanLike);
    }

    #[test]
    fn test_quick_stats() {
        let values = vec![json!(10.0), json!(20.0), json!(30.0)];

        let (min, max, mean) = compute_quick_stats(&values);

        assert_eq!(min, Some(10.0));
        assert_eq!(max, Some(30.0));
        assert_eq!(mean, Some(20.0));
    }
}
