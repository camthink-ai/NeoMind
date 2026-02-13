//! Device Simulator for Testing
//!
//! Simulates IoT devices with realistic telemetry data generation.
//! Supports multiple device types and configurable data patterns.

use chrono::{DateTime, Timelike, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use neomind_core::{MetricValue, NeoMindEvent};

/// Device simulator that generates realistic telemetry data
pub struct DeviceSimulator {
    devices: Arc<RwLock<Vec<SimulatedDevice>>>,
    running: Arc<RwLock<bool>>,
}

impl DeviceSimulator {
    /// Create a new device simulator
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Add a simulated device
    pub async fn add_device(&self, device: SimulatedDevice) {
        self.devices.write().await.push(device);
    }

    /// Add multiple devices at once
    pub async fn add_devices(&self, devices: Vec<SimulatedDevice>) {
        self.devices.write().await.extend(devices);
    }

    /// Get all devices
    pub async fn get_devices(&self) -> Vec<SimulatedDevice> {
        self.devices.read().await.clone()
    }

    /// Get device by ID
    pub async fn get_device(&self, device_id: &str) -> Option<SimulatedDevice> {
        self.devices
            .read()
            .await
            .iter()
            .find(|d| d.id == device_id)
            .cloned()
    }

    /// Start the simulator
    pub async fn start(
        &self,
        event_bus: Arc<neomind_core::eventbus::EventBus>,
    ) -> anyhow::Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        let devices = self.devices.clone();
        let is_running = self.running.clone();

        // Spawn the simulation task
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(5)); // Emit data every 5 seconds

            loop {
                tick.tick().await;

                // Check if still running
                {
                    let running_guard = is_running.read().await;
                    if !*running_guard {
                        break;
                    }
                }

                // Generate telemetry for each device
                let mut devices_guard = devices.write().await;
                for device in devices_guard.iter_mut() {
                    if device.enabled {
                        let telemetry = device.generate_telemetry();

                        // Publish device metric events
                        for (metric_name, value) in telemetry.clone() {
                            let event = NeoMindEvent::DeviceMetric {
                                device_id: device.id.clone(),
                                metric: metric_name,
                                value: MetricValue::Float(value),
                                timestamp: Utc::now().timestamp(),
                                quality: None,
                            };
                            let _ = event_bus.publish(event).await;
                        }

                        // Update device state
                        device.current_telemetry = Some(telemetry);
                        device.last_update = Some(Utc::now());
                    }
                }
            }

            tracing::info!("Device simulator stopped");
        });

        tracing::info!(
            "Device simulator started with {} devices",
            self.devices.read().await.len()
        );
        Ok(())
    }

    /// Stop the simulator
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// Inject a specific event for testing
    pub async fn inject_event(
        &self,
        event_bus: Arc<neomind_core::eventbus::EventBus>,
        device_id: &str,
        metric_name: &str,
        value: f64,
    ) -> anyhow::Result<()> {
        let event = NeoMindEvent::DeviceMetric {
            device_id: device_id.to_string(),
            metric: metric_name.to_string(),
            value: MetricValue::Float(value),
            timestamp: Utc::now().timestamp(),
            quality: None,
        };
        event_bus.publish(event).await;
        Ok(())
    }

    /// Get historical telemetry data for a device
    pub async fn get_telemetry_history(&self, device_id: &str, hours: u64) -> Vec<MetricData> {
        let devices = self.devices.read().await;
        if let Some(device) = devices.iter().find(|d| d.id == device_id) {
            device.generate_historical_data(hours)
        } else {
            Vec::new()
        }
    }
}

impl Default for DeviceSimulator {
    fn default() -> Self {
        Self::new()
    }
}

/// A simulated device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedDevice {
    /// Device ID
    pub id: String,

    /// Device name
    pub name: String,

    /// Device type
    #[serde(rename = "type")]
    pub device_type: SimulatedDeviceType,

    /// Device location (optional)
    pub location: Option<String>,

    /// Whether this device is enabled
    pub enabled: bool,

    /// Base value for metrics (used for generating realistic data)
    pub base_values: HashMap<String, f64>,

    /// Variance for random fluctuation
    pub variance: f64,

    /// Current telemetry data
    #[serde(skip)]
    pub current_telemetry: Option<HashMap<String, f64>>,

    /// Last update timestamp
    #[serde(skip)]
    pub last_update: Option<DateTime<Utc>>,

    /// Historical data (kept in memory for testing)
    #[serde(skip)]
    pub history: Vec<MetricData>,
}

/// Types of simulated devices
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SimulatedDeviceType {
    /// Temperature sensor
    TemperatureSensor,

    /// Humidity sensor
    HumiditySensor,

    /// Temperature and humidity sensor (combined)
    TempHumiditySensor,

    /// Energy meter
    EnergyMeter,

    /// Smart switch
    SmartSwitch,

    /// Motion sensor
    MotionSensor,

    /// Door/window sensor
    DoorSensor,

    /// Air quality sensor
    AirQualitySensor,

    /// Light sensor
    LightSensor,

    /// Custom device type
    Custom(String),
}

impl SimulatedDevice {
    /// Create a new simulated device
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        device_type: SimulatedDeviceType,
    ) -> Self {
        let id = id.into();
        let name = name.into();

        // Set default base values based on device type
        let mut base_values = HashMap::new();
        let variance = match &device_type {
            SimulatedDeviceType::TemperatureSensor => {
                base_values.insert("temperature".to_string(), 25.0);
                2.0
            }
            SimulatedDeviceType::HumiditySensor => {
                base_values.insert("humidity".to_string(), 50.0);
                5.0
            }
            SimulatedDeviceType::TempHumiditySensor => {
                base_values.insert("temperature".to_string(), 25.0);
                base_values.insert("humidity".to_string(), 50.0);
                2.0
            }
            SimulatedDeviceType::EnergyMeter => {
                base_values.insert("power".to_string(), 100.0);
                base_values.insert("energy".to_string(), 0.0);
                20.0
            }
            SimulatedDeviceType::SmartSwitch => {
                base_values.insert("state".to_string(), 0.0);
                0.1
            }
            SimulatedDeviceType::MotionSensor => {
                base_values.insert("motion".to_string(), 0.0);
                0.1
            }
            SimulatedDeviceType::DoorSensor => {
                base_values.insert("open".to_string(), 0.0);
                0.1
            }
            SimulatedDeviceType::AirQualitySensor => {
                base_values.insert("aqi".to_string(), 50.0);
                base_values.insert("co2".to_string(), 400.0);
                10.0
            }
            SimulatedDeviceType::LightSensor => {
                base_values.insert("illuminance".to_string(), 500.0);
                50.0
            }
            SimulatedDeviceType::Custom(_) => 1.0,
        };

        Self {
            id,
            name,
            device_type,
            location: None,
            enabled: true,
            base_values,
            variance,
            current_telemetry: None,
            last_update: None,
            history: Vec::new(),
        }
    }

    /// Set the location
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Set a custom base value
    pub fn with_base_value(mut self, metric: impl Into<String>, value: f64) -> Self {
        self.base_values.insert(metric.into(), value);
        self
    }

    /// Set variance
    pub fn with_variance(mut self, variance: f64) -> Self {
        self.variance = variance;
        self
    }

    /// Generate telemetry data
    pub fn generate_telemetry(&self) -> HashMap<String, f64> {
        let mut rng = rand::thread_rng();
        let mut result = HashMap::new();

        for (metric, base_value) in &self.base_values {
            let value = if *metric == "state" || *metric == "motion" || *metric == "open" {
                // Binary values
                if rng.gen_bool(0.1) {
                    1.0
                } else {
                    0.0
                }
            } else {
                // Continuous values with variance
                let noise = (rand::random::<f64>() - 0.5) * 2.0 * self.variance;
                base_value + noise
            };
            result.insert(metric.clone(), value);
        }

        result
    }

    /// Generate historical data for testing
    pub fn generate_historical_data(&self, hours: u64) -> Vec<MetricData> {
        let mut result = Vec::new();
        let now = Utc::now();
        let points_per_hour = 12; // One data point every 5 minutes
        let total_points = hours * points_per_hour;

        for i in 0..total_points {
            let minutes_ago = (total_points - i) * 5;
            let timestamp = now - chrono::Duration::minutes(minutes_ago as i64);
            let mut values = HashMap::new();

            // Add time-based patterns (diurnal cycle)
            let hour = timestamp.hour() as f64;
            let day_factor = ((hour - 14.0) * std::f64::consts::PI / 12.0).cos(); // Peak at 14:00

            for (metric, base_value) in &self.base_values {
                let value = if *metric == "temperature" {
                    // Temperature follows daily pattern
                    base_value + day_factor * 5.0 + (rand::random::<f64>() - 0.5) * self.variance
                } else if *metric == "humidity" {
                    // Humidity inversely related to temperature
                    base_value - day_factor * 10.0 + (rand::random::<f64>() - 0.5) * self.variance
                } else if *metric == "illuminance" {
                    // Light follows day/night cycle
                    if (6.0..=18.0).contains(&hour) {
                        let day_progress = (hour - 6.0) / 12.0;
                        let peak = (day_progress * (1.0 - day_progress) * 4.0).max(0.0);
                        base_value * peak + (rand::random::<f64>() - 0.5) * self.variance
                    } else {
                        10.0 + (rand::random::<f64>() - 0.5) * 5.0
                    }
                } else {
                    base_value + (rand::random::<f64>() - 0.5) * self.variance
                };
                values.insert(metric.clone(), value.max(0.0));
            }

            result.push(MetricData {
                device_id: self.id.clone(),
                timestamp: timestamp.timestamp(),
                values,
            });
        }

        result
    }

    /// Get available metrics for this device
    pub fn get_metrics(&self) -> Vec<String> {
        self.base_values.keys().cloned().collect()
    }
}

/// Metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricData {
    pub device_id: String,
    pub timestamp: i64,
    pub values: HashMap<String, f64>,
}

/// Builder for creating multiple simulated devices
pub struct DeviceSimulatorBuilder {
    devices: Vec<SimulatedDevice>,
}

impl DeviceSimulatorBuilder {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Add temperature sensors for multiple locations
    pub fn add_temperature_sensors(mut self, count: usize, prefix: &str) -> Self {
        for i in 0..count {
            let device = SimulatedDevice::new(
                format!("{}-temp-{}", prefix, i + 1),
                format!("{} 温度传感器 {}", prefix, i + 1),
                SimulatedDeviceType::TemperatureSensor,
            )
            .with_location(format!("{} 区域{}", prefix, i + 1))
            .with_base_value("temperature", 20.0 + i as f64 * 2.0);
            self.devices.push(device);
        }
        self
    }

    /// Add humidity sensors for multiple locations
    pub fn add_humidity_sensors(mut self, count: usize, prefix: &str) -> Self {
        for i in 0..count {
            let device = SimulatedDevice::new(
                format!("{}-hum-{}", prefix, i + 1),
                format!("{} 湿度传感器 {}", prefix, i + 1),
                SimulatedDeviceType::HumiditySensor,
            )
            .with_location(format!("{} 区域{}", prefix, i + 1))
            .with_base_value("humidity", 45.0 + i as f64 * 3.0);
            self.devices.push(device);
        }
        self
    }

    /// Add combined temp/humidity sensors
    pub fn add_temp_humidity_sensors(mut self, count: usize, prefix: &str) -> Self {
        for i in 0..count {
            let device = SimulatedDevice::new(
                format!("{}-env-{}", prefix, i + 1),
                format!("{} 环境传感器 {}", prefix, i + 1),
                SimulatedDeviceType::TempHumiditySensor,
            )
            .with_location(format!("{} 区域{}", prefix, i + 1))
            .with_base_value("temperature", 22.0 + i as f64)
            .with_base_value("humidity", 50.0 + i as f64 * 2.0);
            self.devices.push(device);
        }
        self
    }

    /// Add energy meters
    pub fn add_energy_meters(mut self, count: usize, prefix: &str) -> Self {
        for i in 0..count {
            let device = SimulatedDevice::new(
                format!("{}-energy-{}", prefix, i + 1),
                format!("{} 能耗表 {}", prefix, i + 1),
                SimulatedDeviceType::EnergyMeter,
            )
            .with_base_value("power", 50.0 + i as f64 * 20.0)
            .with_base_value("energy", 0.0)
            .with_variance(10.0);
            self.devices.push(device);
        }
        self
    }

    /// Add smart switches (with on/off control capability)
    pub fn add_smart_switches(mut self, count: usize, prefix: &str) -> Self {
        for i in 0..count {
            let device = SimulatedDevice::new(
                format!("{}-switch-{}", prefix, i + 1),
                format!("{} 智能开关 {}", prefix, i + 1),
                SimulatedDeviceType::SmartSwitch,
            )
            .with_base_value("state", 0.0);
            self.devices.push(device);
        }
        self
    }

    /// Add a warehouse environment setup (common IoT scenario)
    pub fn add_warehouse_environment(mut self, warehouse_count: usize) -> Self {
        for i in 0..warehouse_count {
            let warehouse_id = format!("warehouse{}", i + 1);
            self = self.add_temp_humidity_sensors(3, &warehouse_id); // 3 sensors per warehouse
            self = self.add_smart_switches(2, &warehouse_id); // 2 switches per warehouse
        }
        self
    }

    /// Build and return the devices
    pub fn build(self) -> Vec<SimulatedDevice> {
        self.devices
    }

    /// Create a simulator with these devices
    pub async fn create_simulator(self) -> DeviceSimulator {
        let simulator = DeviceSimulator::new();
        for device in self.devices {
            simulator.add_device(device).await;
        }
        simulator
    }
}

impl Default for DeviceSimulatorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_device() {
        let device = SimulatedDevice::new(
            "test-1",
            "Test Sensor",
            SimulatedDeviceType::TemperatureSensor,
        );

        assert_eq!(device.id, "test-1");
        assert_eq!(device.name, "Test Sensor");
        assert!(device.base_values.contains_key("temperature"));
    }

    #[tokio::test]
    async fn test_generate_telemetry() {
        let device = SimulatedDevice::new(
            "test-1",
            "Test Sensor",
            SimulatedDeviceType::TemperatureSensor,
        );

        let telemetry = device.generate_telemetry();
        assert!(telemetry.contains_key("temperature"));
        let temp = telemetry["temperature"];
        assert!(temp > 15.0 && temp < 35.0); // Should be around 25 ± variance
    }

    #[tokio::test]
    async fn test_simulator_builder() {
        let devices = DeviceSimulatorBuilder::new()
            .add_temperature_sensors(3, "test")
            .add_humidity_sensors(2, "test")
            .build();

        assert_eq!(devices.len(), 5); // 3 temp + 2 humidity
    }

    #[tokio::test]
    async fn test_historical_data_generation() {
        let device = SimulatedDevice::new(
            "test-1",
            "Test Sensor",
            SimulatedDeviceType::TemperatureSensor,
        );

        let history = device.generate_historical_data(24); // 24 hours
        assert_eq!(history.len(), 24 * 12); // 12 points per hour
    }
}
