//! Testing utilities for NeoMind
//!
//! This crate provides testing tools including:
//! - Device simulator for generating IoT device data
//! - Test data generators
//! - Scenario builders
//! - Production-grade test utilities for isolated testing

pub mod device_simulator;
pub mod scenarios;
pub mod test_data;
pub mod test_utils;

pub use device_simulator::{DeviceSimulator, MetricData, SimulatedDevice, SimulatedDeviceType};
pub use scenarios::{Scenario, ScenarioBuilder};
pub use test_data::{DataPattern, TestDataGenerator};
pub use test_utils::{retry, test_id, test_temp_dir, TestDbConfig};

// The assert_eventually macro is already available at the crate root
// due to #[macro_export] in test_utils.rs
