//! Testing utilities for NeoMind
//!
//! This crate provides testing tools including:
//! - Device simulator for generating IoT device data
//! - Test data generators
//! - Scenario builders

pub mod device_simulator;
pub mod test_data;
pub mod scenarios;

pub use device_simulator::{
    DeviceSimulator, SimulatedDevice, SimulatedDeviceType, MetricData,
};
pub use test_data::{TestDataGenerator, DataPattern};
pub use scenarios::{Scenario, ScenarioBuilder};
