//! Test data generation utilities
//!
//! Provides utilities for generating various types of test data
//! for AI Agent testing scenarios.

use chrono::{Datelike, Timelike, Utc};
use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generator for test data
pub struct TestDataGenerator {
    seed: u64,
}

impl TestDataGenerator {
    /// Create a new generator with a random seed
    pub fn new() -> Self {
        Self {
            seed: rand::random(),
        }
    }

    /// Create a generator with a specific seed (for reproducible tests)
    pub fn with_seed(seed: u64) -> Self {
        Self { seed }
    }

    /// Generate time series data with a pattern
    pub fn generate_time_series(
        &self,
        pattern: DataPattern,
        points: usize,
        interval_seconds: u64,
    ) -> Vec<TimeSeriesPoint> {
        let mut result = Vec::new();
        let now = Utc::now();
        let mut rng = StdRng::seed_from_u64(self.seed);

        for i in 0..points {
            let timestamp =
                now - chrono::Duration::seconds((points - i) as i64 * interval_seconds as i64);
            let value = match &pattern {
                DataPattern::Constant { base } => *base,
                DataPattern::Linear { start, slope } => start + slope * i as f64,
                DataPattern::SineWave {
                    amplitude,
                    frequency,
                    offset,
                } => offset + amplitude * (i as f64 * frequency).sin(),
                DataPattern::Random { min, max } => rng.gen_range(*min..=*max),
                DataPattern::DailyCycle { base, amplitude } => {
                    let hour = (timestamp.hour() as f64 - 14.0) * std::f64::consts::PI / 12.0;
                    base + amplitude * hour.cos()
                }
                DataPattern::WeeklyCycle { base, amplitude } => {
                    let dow = timestamp.weekday().num_days_from_monday() as f64;
                    let angle = (dow - 3.0) * 2.0 * std::f64::consts::PI / 7.0;
                    base + amplitude * angle.cos()
                }
                DataPattern::TrendWithNoise { base, trend, noise } => {
                    base + trend * i as f64 + rng.gen_range(-*noise..=*noise)
                }
                DataPattern::Anomaly {
                    normal,
                    anomaly_value,
                    anomaly_rate,
                } => {
                    if rng.gen_bool(*anomaly_rate) {
                        *anomaly_value
                    } else {
                        *normal + rng.gen_range(-1.0..1.0)
                    }
                }
            };

            result.push(TimeSeriesPoint {
                timestamp: timestamp.timestamp(),
                value,
            });
        }

        result
    }

    /// Generate multi-device time series data
    pub fn generate_multi_device_data(
        &self,
        device_count: usize,
        pattern: DataPattern,
        points_per_device: usize,
    ) -> HashMap<String, Vec<TimeSeriesPoint>> {
        let mut result = HashMap::new();
        let mut rng = StdRng::seed_from_u64(self.seed);

        for i in 0..device_count {
            let device_id = format!("device-{}", i + 1);
            let adjusted_pattern = match &pattern {
                DataPattern::TrendWithNoise { base, trend, noise } => DataPattern::TrendWithNoise {
                    base: base + i as f64 * 2.0,
                    trend: *trend,
                    noise: *noise,
                },
                _ => pattern.clone(),
            };

            let mut points = self.generate_time_series(adjusted_pattern, points_per_device, 300);

            // Add some device-specific noise
            for point in &mut points {
                point.value += rng.gen_range(-2.0..2.0);
            }

            result.insert(device_id, points);
        }

        result
    }

    /// Generate a complete scenario with multiple devices and metrics
    pub fn generate_scenario_data(&self, scenario: TestScenario) -> ScenarioData {
        match scenario {
            TestScenario::WarehouseMonitoring => {
                let mut devices = HashMap::new();

                // Temperature sensors (5 warehouses, 3 sensors each)
                for w in 1..=5 {
                    for s in 1..=3 {
                        let device_id = format!("warehouse{}-temp-{}", w, s);
                        let pattern = DataPattern::DailyCycle {
                            base: 22.0,
                            amplitude: 5.0,
                        };
                        let points = self.generate_time_series(pattern, 144, 600); // 5 min intervals, 12 hours
                        devices.insert(
                            device_id,
                            MetricSeries {
                                metric_name: "temperature".to_string(),
                                unit: "°C".to_string(),
                                data: points,
                            },
                        );
                    }

                    // Humidity sensors
                    for s in 1..=3 {
                        let device_id = format!("warehouse{}-hum-{}", w, s);
                        let pattern = DataPattern::DailyCycle {
                            base: 55.0,
                            amplitude: -10.0,
                        };
                        let points = self.generate_time_series(pattern, 144, 600);
                        devices.insert(
                            device_id,
                            MetricSeries {
                                metric_name: "humidity".to_string(),
                                unit: "%".to_string(),
                                data: points,
                            },
                        );
                    }
                }

                ScenarioData {
                    scenario_name: "Warehouse Monitoring".to_string(),
                    devices,
                    start_time: Utc::now() - chrono::Duration::hours(12),
                    end_time: Utc::now(),
                }
            }

            TestScenario::EnergyConsumption => {
                let mut devices = HashMap::new();

                // Energy meters for 10 devices
                for i in 1..=10 {
                    let device_id = format!("energy-meter-{}", i);
                    let pattern = DataPattern::TrendWithNoise {
                        base: 100.0 + i as f64 * 20.0,
                        trend: 0.01,
                        noise: 15.0,
                    };
                    let points = self.generate_time_series(pattern, 336, 300); // 7 days, 5 min intervals
                    devices.insert(
                        device_id,
                        MetricSeries {
                            metric_name: "power".to_string(),
                            unit: "W".to_string(),
                            data: points,
                        },
                    );
                }

                ScenarioData {
                    scenario_name: "Energy Consumption".to_string(),
                    devices,
                    start_time: Utc::now() - chrono::Duration::days(7),
                    end_time: Utc::now(),
                }
            }

            TestScenario::AnomalyDetection => {
                let mut devices = HashMap::new();

                // Temperature sensor with anomalies
                let device_id = "temp-sensor-1".to_string();
                let pattern = DataPattern::Anomaly {
                    normal: 25.0,
                    anomaly_value: 40.0,
                    anomaly_rate: 0.05, // 5% anomaly rate
                };
                let points = self.generate_time_series(pattern, 288, 300); // 24 hours
                devices.insert(
                    device_id,
                    MetricSeries {
                        metric_name: "temperature".to_string(),
                        unit: "°C".to_string(),
                        data: points,
                    },
                );

                ScenarioData {
                    scenario_name: "Anomaly Detection".to_string(),
                    devices,
                    start_time: Utc::now() - chrono::Duration::hours(24),
                    end_time: Utc::now(),
                }
            }
        }
    }
}

impl Default for TestDataGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Data generation patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataPattern {
    /// Constant value
    Constant { base: f64 },

    /// Linear trend
    Linear { start: f64, slope: f64 },

    /// Sine wave oscillation
    SineWave {
        amplitude: f64,
        frequency: f64,
        offset: f64,
    },

    /// Random values within range
    Random { min: f64, max: f64 },

    /// Daily temperature cycle (peaks at 14:00)
    DailyCycle { base: f64, amplitude: f64 },

    /// Weekly cycle (peaks mid-week)
    WeeklyCycle { base: f64, amplitude: f64 },

    /// Trend with noise
    TrendWithNoise { base: f64, trend: f64, noise: f64 },

    /// Normal data with occasional anomalies
    Anomaly {
        normal: f64,
        anomaly_value: f64,
        anomaly_rate: f64,
    },
}

/// A single time series data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: i64,
    pub value: f64,
}

/// Series of metric data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSeries {
    pub metric_name: String,
    pub unit: String,
    pub data: Vec<TimeSeriesPoint>,
}

/// Complete scenario data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioData {
    pub scenario_name: String,
    pub devices: HashMap<String, MetricSeries>,
    pub start_time: chrono::DateTime<Utc>,
    pub end_time: chrono::DateTime<Utc>,
}

/// Predefined test scenarios
#[derive(Debug, Clone, Copy)]
pub enum TestScenario {
    /// Multiple warehouses with temperature and humidity sensors
    WarehouseMonitoring,

    /// Energy consumption meters
    EnergyConsumption,

    /// Data with anomalies for testing detection
    AnomalyDetection,
}

/// Helper to create anomaly data for testing
pub struct AnomalyBuilder {
    normal_value: f64,
    anomaly_value: f64,
    anomaly_indices: Vec<usize>,
}

impl AnomalyBuilder {
    /// Create a new anomaly builder
    pub fn new(normal_value: f64, anomaly_value: f64) -> Self {
        Self {
            normal_value,
            anomaly_value,
            anomaly_indices: Vec::new(),
        }
    }

    /// Add anomaly at specific index
    pub fn add_anomaly_at(mut self, index: usize) -> Self {
        self.anomaly_indices.push(index);
        self
    }

    /// Add random anomalies
    pub fn add_random_anomalies(mut self, count: usize, total_points: usize) -> Self {
        let mut rng = rand::thread_rng();
        for _ in 0..count {
            self.anomaly_indices.push(rng.gen_range(0..total_points));
        }
        self
    }

    /// Generate the data series
    pub fn generate(&self, total_points: usize, interval_seconds: u64) -> Vec<TimeSeriesPoint> {
        let mut result = Vec::new();
        let now = Utc::now();
        let anomaly_set: std::collections::HashSet<_> =
            self.anomaly_indices.iter().cloned().collect();

        for i in 0..total_points {
            let timestamp = now
                - chrono::Duration::seconds((total_points - i) as i64 * interval_seconds as i64);
            let value = if anomaly_set.contains(&i) {
                self.anomaly_value
            } else {
                self.normal_value + (rand::random::<f64>() - 0.5) * 2.0
            };

            result.push(TimeSeriesPoint {
                timestamp: timestamp.timestamp(),
                value,
            });
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_constant_pattern() {
        let data_gen = TestDataGenerator::with_seed(42);
        let data = data_gen.generate_time_series(DataPattern::Constant { base: 25.0 }, 10, 60);

        assert_eq!(data.len(), 10);
        for point in &data {
            assert_eq!(point.value, 25.0);
        }
    }

    #[test]
    fn test_generate_sine_wave() {
        let data_gen = TestDataGenerator::with_seed(42);
        let data = data_gen.generate_time_series(
            DataPattern::SineWave {
                amplitude: 10.0,
                frequency: 0.1,
                offset: 20.0,
            },
            100,
            60,
        );

        assert_eq!(data.len(), 100);
        // Check that values oscillate
        let min_val = data.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
        let max_val = data
            .iter()
            .map(|p| p.value)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(min_val < 15.0); // offset - amplitude
        assert!(max_val > 25.0); // offset + amplitude
    }

    #[test]
    fn test_anomaly_builder() {
        let builder = AnomalyBuilder::new(25.0, 40.0)
            .add_anomaly_at(5)
            .add_anomaly_at(15);

        let data = builder.generate(20, 60);

        assert_eq!(data.len(), 20);
        assert!((data[5].value - 40.0).abs() < 1.0); // Anomaly
        assert!((data[15].value - 40.0).abs() < 1.0); // Anomaly
        assert!((data[0].value - 25.0).abs() < 3.0); // Normal with noise
    }
}
