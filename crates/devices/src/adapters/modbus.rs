//! Modbus device adapter for NeoTalk event-driven architecture.
//!
//! This adapter polls Modbus TCP devices and publishes metric events.
//!
//! ## Protocol Mapping Integration
//!
//! The adapter can use a `ProtocolMapping` for flexible register and data handling:
//! ```text
//! Device Type Definition       Modbus Mapping
//! ├─ voltage capability      ──→ slave:1, register:0x0000, type:Float32
//! ├─ current capability      ──→ slave:1, register:0x0002, type:Float32
//! └─ reset command           ──→ slave:1, register:0x0200, type:Coil
//! ```

use crate::adapter::{DeviceAdapter, DeviceEvent, DiscoveredDeviceInfo, AdapterResult};
use crate::protocol::{ProtocolMapping, Address};
use edge_ai_core::EventBus;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Modbus device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusDeviceConfig {
    /// Unique device identifier
    pub device_id: String,

    /// Device name (human-readable)
    #[serde(default)]
    pub name: Option<String>,

    /// Modbus TCP host
    pub host: String,

    /// Modbus TCP port
    #[serde(default = "default_modbus_port")]
    pub port: u16,

    /// Slave/unit ID
    #[serde(default = "default_slave_id")]
    pub slave_id: u8,

    /// Polling interval in seconds
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,

    /// Register definitions for this device
    pub registers: Vec<RegisterDefinition>,
}

fn default_modbus_port() -> u16 {
    502
}

fn default_slave_id() -> u8 {
    1
}

fn default_poll_interval() -> u64 {
    60
}

impl ModbusDeviceConfig {
    /// Create a new Modbus device configuration.
    pub fn new(
        device_id: impl Into<String>,
        host: impl Into<String>,
        registers: Vec<RegisterDefinition>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            name: None,
            host: host.into(),
            port: default_modbus_port(),
            slave_id: default_slave_id(),
            poll_interval_secs: default_poll_interval(),
            registers,
        }
    }

    /// Set the device name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the slave ID.
    pub fn with_slave_id(mut self, slave_id: u8) -> Self {
        self.slave_id = slave_id;
        self
    }

    /// Set the polling interval.
    pub fn with_poll_interval(mut self, secs: u64) -> Self {
        self.poll_interval_secs = secs;
        self
    }
}

/// Modbus register definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDefinition {
    /// Metric name (will be used as the event metric name)
    pub name: String,

    /// Register address
    pub address: u16,

    /// Register type
    #[serde(default)]
    pub register_type: RegisterType,

    /// Data type
    #[serde(default)]
    pub data_type: ModbusDataType,

    /// Scale factor (value = raw * scale)
    #[serde(default = "default_scale")]
    pub scale: f64,

    /// Offset (value = raw * scale + offset)
    #[serde(default)]
    pub offset: f64,

    /// Number of registers to read (for 32-bit values)
    #[serde(default = "default_register_count")]
    pub count: u16,
}

fn default_scale() -> f64 {
    1.0
}

fn default_register_count() -> u16 {
    1
}

impl RegisterDefinition {
    /// Create a new holding register definition.
    pub fn holding_register(name: impl Into<String>, address: u16) -> Self {
        Self {
            name: name.into(),
            address,
            register_type: RegisterType::HoldingRegister,
            data_type: ModbusDataType::Int16,
            scale: 1.0,
            offset: 0.0,
            count: 1,
        }
    }

    /// Create a new input register definition.
    pub fn input_register(name: impl Into<String>, address: u16) -> Self {
        Self {
            name: name.into(),
            address,
            register_type: RegisterType::InputRegister,
            data_type: ModbusDataType::Int16,
            scale: 1.0,
            offset: 0.0,
            count: 1,
        }
    }

    /// Set the data type.
    pub fn with_data_type(mut self, data_type: ModbusDataType) -> Self {
        self.data_type = data_type;
        self
    }

    /// Set the scale factor.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Set the offset.
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    /// Set the register count.
    pub fn with_count(mut self, count: u16) -> Self {
        self.count = count;
        self
    }
}

/// Modbus register type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegisterType {
    /// Input register (read-only)
    InputRegister,
    /// Holding register (read-write)
    HoldingRegister,
    /// Coil (boolean, read-write)
    Coil,
    /// Discrete input (boolean, read-only)
    DiscreteInput,
}

impl Default for RegisterType {
    fn default() -> Self {
        Self::HoldingRegister
    }
}

/// Modbus data type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModbusDataType {
    /// 16-bit signed integer
    Int16,
    /// 16-bit unsigned integer
    Uint16,
    /// 32-bit signed integer
    Int32,
    /// 32-bit unsigned integer
    Uint32,
    /// 32-bit float
    Float32,
    /// Boolean
    Bool,
}

impl Default for ModbusDataType {
    fn default() -> Self {
        Self::Int16
    }
}

/// Convert to the protocol mapping's ModbusDataType.
impl From<ModbusDataType> for crate::protocol::modbus_mapping::ModbusDataType {
    fn from(value: ModbusDataType) -> Self {
        match value {
            ModbusDataType::Int16 => Self::Int16,
            ModbusDataType::Uint16 => Self::Uint16,
            ModbusDataType::Int32 => Self::Int32,
            ModbusDataType::Uint32 => Self::Uint32,
            ModbusDataType::Float32 => Self::Float32,
            ModbusDataType::Bool => Self::Bool,
        }
    }
}

/// Convert RegisterType to ModbusRegisterType.
impl From<RegisterType> for crate::protocol::mapping::ModbusRegisterType {
    fn from(value: RegisterType) -> Self {
        match value {
            RegisterType::InputRegister => Self::InputRegister,
            RegisterType::HoldingRegister => Self::HoldingRegister,
            RegisterType::Coil => Self::Coil,
            RegisterType::DiscreteInput => Self::DiscreteInput,
        }
    }
}

/// Modbus device adapter configuration.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ModbusAdapterConfig {
    /// Adapter name
    pub name: String,

    /// Modbus devices to poll
    pub devices: Vec<ModbusDeviceConfig>,
}

impl ModbusAdapterConfig {
    /// Create a new Modbus adapter configuration.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            devices: Vec::new(),
        }
    }

    /// Add a device configuration.
    pub fn with_device(mut self, device: ModbusDeviceConfig) -> Self {
        self.devices.push(device);
        self
    }

    /// Add multiple device configurations.
    pub fn with_devices(mut self, devices: Vec<ModbusDeviceConfig>) -> Self {
        self.devices = devices;
        self
    }
}

/// Modbus device adapter.
///
/// Polls Modbus TCP devices and publishes metric events.
/// Can optionally use a ProtocolMapping for flexible register and data handling.
pub struct ModbusAdapter {
    /// Adapter configuration
    config: ModbusAdapterConfig,
    /// Event channel sender
    event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Device IDs
    devices: Arc<tokio::sync::RwLock<Vec<String>>>,
    /// Optional protocol mapping for flexible register/data handling
    protocol_mapping: Option<Arc<dyn ProtocolMapping>>,
    /// Device ID to device type mapping (used with protocol mapping)
    device_types: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}

impl ModbusAdapter {
    /// Create a new Modbus adapter.
    pub fn new(config: ModbusAdapterConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        // Extract device IDs
        let device_ids: Vec<String> = config.devices.iter()
            .map(|d| d.device_id.clone())
            .collect();

        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(tokio::sync::RwLock::new(device_ids)),
            protocol_mapping: None,
            device_types: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create a new Modbus adapter with a protocol mapping.
    pub fn with_mapping(
        config: ModbusAdapterConfig,
        mapping: Arc<dyn ProtocolMapping>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        // Extract device IDs
        let device_ids: Vec<String> = config.devices.iter()
            .map(|d| d.device_id.clone())
            .collect();

        Self {
            config,
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            devices: Arc::new(tokio::sync::RwLock::new(device_ids)),
            protocol_mapping: Some(mapping),
            device_types: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Register a device type with its ID (for protocol mapping).
    pub async fn register_device_type(&self, device_id: String, device_type: String) {
        let mut types = self.device_types.write().await;
        types.insert(device_id, device_type);
    }

    /// Set the protocol mapping.
    pub fn set_mapping(&mut self, mapping: Arc<dyn ProtocolMapping>) {
        self.protocol_mapping = Some(mapping);
    }

    /// Get the current protocol mapping.
    pub fn mapping(&self) -> Option<&Arc<dyn ProtocolMapping>> {
        self.protocol_mapping.as_ref()
    }

    /// Get device configuration by ID.
    fn get_device_config(&self, device_id: &str) -> Option<&ModbusDeviceConfig> {
        self.config.devices.iter()
            .find(|d| d.device_id == device_id)
    }

    /// Parse register data using protocol mapping if available.
    fn parse_register_data(
        &self,
        device_id: &str,
        metric_name: &str,
        data: &[u8],
    ) -> crate::mdl::MetricValue {
        // Try protocol mapping first
        if let Some(ref mapping) = self.protocol_mapping {
            if let Ok(value) = mapping.parse_metric(metric_name, data) {
                return value;
            }
            // Fall through to default parsing on error
        }

        // Default parsing: use register definition
        if let Some(device) = self.get_device_config(device_id) {
            if let Some(reg) = device.registers.iter().find(|r| r.name == metric_name) {
                return Self::default_parse_register(data, reg);
            }
        }

        // Ultimate fallback: try to parse as basic types
        Self::default_parse_value(data)
    }

    /// Default register parsing using RegisterDefinition.
    fn default_parse_register(data: &[u8], reg: &RegisterDefinition) -> crate::mdl::MetricValue {
        use crate::mdl::MetricValue;

        let raw_value = match reg.data_type {
            ModbusDataType::Int16 => {
                if data.len() >= 2 {
                    let value = i16::from_be_bytes([data[0], data[1]]);
                    MetricValue::Integer(value as i64)
                } else {
                    MetricValue::Null
                }
            }
            ModbusDataType::Uint16 => {
                if data.len() >= 2 {
                    let value = u16::from_be_bytes([data[0], data[1]]);
                    MetricValue::Integer(value as i64)
                } else {
                    MetricValue::Null
                }
            }
            ModbusDataType::Int32 => {
                if data.len() >= 4 {
                    let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    MetricValue::Integer(value as i64)
                } else {
                    MetricValue::Null
                }
            }
            ModbusDataType::Uint32 => {
                if data.len() >= 4 {
                    let value = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    MetricValue::Integer(value as i64)
                } else {
                    MetricValue::Null
                }
            }
            ModbusDataType::Float32 => {
                if data.len() >= 4 {
                    let bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                    let value = f32::from_be_bytes(bytes);
                    MetricValue::Float(value as f64)
                } else {
                    MetricValue::Null
                }
            }
            ModbusDataType::Bool => {
                if !data.is_empty() {
                    MetricValue::Boolean(data[0] != 0)
                } else {
                    MetricValue::Null
                }
            }
        };

        // Apply scale and offset
        match raw_value {
            MetricValue::Integer(i) => {
                let scaled = (i as f64) * reg.scale + reg.offset;
                if (scaled - scaled.round()).abs() < 0.0001 {
                    MetricValue::Integer(scaled.round() as i64)
                } else {
                    MetricValue::Float(scaled)
                }
            }
            MetricValue::Float(f) => {
                MetricValue::Float(f * reg.scale + reg.offset)
            }
            other => other,
        }
    }

    /// Default value parsing (when no register definition is available).
    fn default_parse_value(data: &[u8]) -> crate::mdl::MetricValue {
        use crate::mdl::MetricValue;

        if data.len() >= 4 {
            // Try to parse as i32 first
            let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            MetricValue::Integer(value as i64)
        } else if data.len() >= 2 {
            // Try to parse as i16
            let value = i16::from_be_bytes([data[0], data[1]]);
            MetricValue::Integer(value as i64)
        } else if !data.is_empty() {
            // Single byte as boolean
            MetricValue::Boolean(data[0] != 0)
        } else {
            MetricValue::Null
        }
    }

    /// Read a register value from a device.
    pub async fn read_register(
        &self,
        device_id: &str,
        metric_name: &str,
    ) -> Result<crate::mdl::MetricValue, Box<dyn std::error::Error + Send + Sync>> {
        let device = self.get_device_config(device_id)
            .ok_or("Device not found")?;

        // Get register address from protocol mapping or device config
        let (address, register_type, count) = if let Some(ref mapping) = self.protocol_mapping {
            if let Some(Address::Modbus { slave, register, register_type: rt, count: ct }) =
                mapping.metric_address(metric_name) {
                (register, rt, ct)
            } else {
                // Fallback to device config
                let reg = device.registers.iter()
                    .find(|r| r.name == metric_name)
                    .ok_or("Register not found")?;
                (reg.address, reg.register_type.into(), Some(reg.count))
            }
        } else {
            let reg = device.registers.iter()
                .find(|r| r.name == metric_name)
                .ok_or("Register not found")?;
            (reg.address, reg.register_type.into(), Some(reg.count))
        };

        // In a real implementation, this would read from the actual Modbus device
        // For now, return a simulated value
        info!("Reading Modbus register: device={}, metric={}, address={:?}, type={:?}",
            device_id, metric_name, address, register_type);

        // Simulated data - in production, use tokio-modbus
        let simulated_data = vec![0x00, 0x19]; // 25 in big-endian

        Ok(self.parse_register_data(device_id, metric_name, &simulated_data))
    }

    /// Write a command to a Modbus device.
    pub async fn send_command(
        &self,
        device_id: &str,
        command: &str,
        params: &HashMap<String, crate::mdl::MetricValue>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let device = self.get_device_config(device_id)
            .ok_or("Device not found")?;

        if let Some(ref mapping) = self.protocol_mapping {
            // Serialize command using protocol mapping
            let payload = mapping.serialize_command(command, params)?;
            let address = mapping.command_address(command)
                .ok_or("Command not found in mapping")?;

            if let Address::Modbus { slave, register, register_type, .. } = address {
                info!("Sending Modbus command: device={}, command={}, slave={}, register={:?}, type={:?}",
                    device_id, command, slave, register, register_type);
                // In a real implementation, this would write to the Modbus device
                // For now, just log the payload
                debug!("Command payload: {:?}", payload);
                return Ok(());
            }

            Err("Command address is not Modbus type".into())
        } else {
            // Fallback: find command register from device config
            warn!("Sending command without protocol mapping (not fully implemented)");
            info!("Sending Modbus command to {} (no mapping)", device_id);
            // In a real implementation, this would write to the Modbus device
            Ok(())
        }
    }

    /// Get all registers for a device from protocol mapping or config.
    pub fn get_device_registers(&self, device_id: &str) -> Vec<(String, u16, RegisterType, ModbusDataType)> {
        let mut result = Vec::new();

        if let Some(ref mapping) = self.protocol_mapping {
            // Get registers from protocol mapping
            for capability in mapping.mapped_capabilities() {
                if let Some(Address::Modbus { register, register_type, .. }) =
                    mapping.metric_address(&capability) {
                    // Convert ModbusRegisterType to RegisterType
                    let rt = match register_type {
                        crate::protocol::mapping::ModbusRegisterType::InputRegister => RegisterType::InputRegister,
                        crate::protocol::mapping::ModbusRegisterType::HoldingRegister => RegisterType::HoldingRegister,
                        crate::protocol::mapping::ModbusRegisterType::Coil => RegisterType::Coil,
                        crate::protocol::mapping::ModbusRegisterType::DiscreteInput => RegisterType::DiscreteInput,
                    };
                    result.push((capability, register, rt, ModbusDataType::Int16));
                }
            }
        } else if let Some(device) = self.get_device_config(device_id) {
            // Get registers from device config
            for reg in &device.registers {
                result.push((reg.name.clone(), reg.address, reg.register_type, reg.data_type));
            }
        }

        result
    }
}

#[async_trait]
impl DeviceAdapter for ModbusAdapter {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn adapter_type(&self) -> &'static str {
        "modbus"
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn start(&self) -> AdapterResult<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting Modbus adapter: {}", self.config.name);

        self.running.store(true, std::sync::atomic::Ordering::Relaxed);

        // Spawn polling tasks for each device
        let running = self.running.clone();
        let adapter_name = self.config.name.clone();
        let devices: Vec<_> = self.config.devices.clone();
        let event_tx = self.event_tx.clone();
        let protocol_mapping = self.protocol_mapping.clone();
        let device_types = self.device_types.clone();

        tokio::spawn(async move {
            // Create polling tasks for each device
            let mut handles = Vec::new();

            for device in devices {
                let running = running.clone();
                let device_id = device.device_id.clone();
                let device_name = device.name.clone().unwrap_or_else(|| device_id.clone());
                let registers = device.registers.clone();
                let poll_interval = Duration::from_secs(device.poll_interval_secs);
                let event_tx = event_tx.clone();
                let protocol_mapping = protocol_mapping.clone();
                let device_types = device_types.clone();

                let handle = tokio::spawn(async move {
                    while running.load(std::sync::atomic::Ordering::Relaxed) {
                        // Poll each register
                        for reg in &registers {
                            debug!("Polling Modbus device: {}, register: {}",
                                device_id, reg.name);

                            // In production, actual polling would happen here
                            // For now, we simulate a metric event
                            let simulated_value = if reg.scale != 1.0 || reg.offset != 0.0 {
                                crate::mdl::MetricValue::Float(25.0 * reg.scale + reg.offset)
                            } else {
                                crate::mdl::MetricValue::Float(25.0)
                            };

                            // Try to use protocol mapping for parsing if available
                            let value = if let Some(ref mapping) = protocol_mapping {
                                let simulated_data = vec![0x00, 0x19]; // 25 in big-endian
                                if let Ok(v) = mapping.parse_metric(&reg.name, &simulated_data) {
                                    v
                                } else {
                                    simulated_value
                                }
                            } else {
                                simulated_value
                            };

                            let event = DeviceEvent::Metric {
                                device_id: device_id.clone(),
                                metric: reg.name.clone(),
                                value,
                                timestamp: chrono::Utc::now().timestamp(),
                            };

                            let _ = event_tx.send(event);
                        }

                        tokio::time::sleep(poll_interval).await;
                    }
                    debug!("Stopped polling device: {}", device_id);
                });

                handles.push(handle);
            }

            // Wait for all polling tasks to complete
            for handle in handles {
                let _ = handle.await;
            }

            debug!("Modbus adapter '{}' stopped", adapter_name);
        });

        info!("Modbus adapter '{}' started, polling {} devices",
              self.config.name, self.config.devices.len());
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        info!("Stopping Modbus adapter: {}", self.config.name);
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
        let rx = self.event_tx.subscribe();
        Box::pin(async_stream::stream! {
            let mut rx = rx;
            while let Ok(event) = rx.recv().await {
                yield event;
            }
        })
    }

    fn device_count(&self) -> usize {
        self.devices.try_read().map(|v| v.len()).unwrap_or(0)
    }

    fn list_devices(&self) -> Vec<String> {
        self.devices.try_read().map(|v| v.clone()).unwrap_or_default()
    }
}

/// Create a Modbus adapter connected to an event bus.
pub fn create_modbus_adapter(
    config: ModbusAdapterConfig,
    event_bus: &EventBus,
) -> Arc<ModbusAdapter> {
    let adapter = Arc::new(ModbusAdapter::new(config));
    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            let device_id = event.device_id().unwrap_or("unknown").to_string();
            let neotalk_event = event.to_neotalk_event();
            let source = format!("adapter:modbus:{}", device_id);
            event_bus.publish_with_source(neotalk_event, source).await;
        }
    });

    adapter
}

/// Create a Modbus adapter with protocol mapping.
pub fn create_modbus_adapter_with_mapping(
    config: ModbusAdapterConfig,
    mapping: Arc<dyn ProtocolMapping>,
    event_bus: &EventBus,
) -> Arc<ModbusAdapter> {
    let adapter = Arc::new(ModbusAdapter::with_mapping(config, mapping));
    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            let device_id = event.device_id().unwrap_or("unknown").to_string();
            let neotalk_event = event.to_neotalk_event();
            let source = format!("adapter:modbus:{}", device_id);
            event_bus.publish_with_source(neotalk_event, source).await;
        }
    });

    adapter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modbus_device_config() {
        let config = ModbusDeviceConfig::new(
            "temp_sensor",
            "192.168.1.100",
            vec![RegisterDefinition::holding_register("temperature", 100)],
        )
            .with_name("Temperature Sensor")
            .with_port(502)
            .with_slave_id(1)
            .with_poll_interval(30);

        assert_eq!(config.device_id, "temp_sensor");
        assert_eq!(config.host, "192.168.1.100");
        assert_eq!(config.port, 502);
        assert_eq!(config.slave_id, 1);
        assert_eq!(config.poll_interval_secs, 30);
        assert_eq!(config.registers.len(), 1);
    }

    #[test]
    fn test_register_definition() {
        let reg = RegisterDefinition::holding_register("temperature", 100)
            .with_data_type(ModbusDataType::Int16)
            .with_scale(0.1)
            .with_offset(-40.0);

        assert_eq!(reg.name, "temperature");
        assert_eq!(reg.address, 100);
        assert_eq!(reg.register_type, RegisterType::HoldingRegister);
        assert_eq!(reg.scale, 0.1);
        assert_eq!(reg.offset, -40.0);
    }

    #[test]
    fn test_input_register() {
        let reg = RegisterDefinition::input_register("humidity", 200);
        assert_eq!(reg.register_type, RegisterType::InputRegister);
    }

    #[test]
    fn test_adapter_config() {
        let device = ModbusDeviceConfig::new(
            "sensor1",
            "192.168.1.10",
            vec![RegisterDefinition::holding_register("value", 0)],
        );

        let config = ModbusAdapterConfig::new("test_adapter")
            .with_device(device);

        assert_eq!(config.name, "test_adapter");
        assert_eq!(config.devices.len(), 1);
    }

    #[tokio::test]
    async fn test_adapter_lifecycle() {
        let config = ModbusAdapterConfig::new("test");
        let adapter = ModbusAdapter::new(config);

        assert!(!adapter.is_running());
        adapter.start().await.unwrap();
        assert!(adapter.is_running());
        adapter.stop().await.unwrap();
        assert!(!adapter.is_running());
    }

    #[tokio::test]
    async fn test_name_and_type() {
        let config = ModbusAdapterConfig::new("my_modbus");
        let adapter = ModbusAdapter::new(config);

        assert_eq!(adapter.name(), "my_modbus");
        assert_eq!(adapter.adapter_type(), "modbus");
    }

    #[tokio::test]
    async fn test_device_tracking() {
        let device = ModbusDeviceConfig::new(
            "sensor1",
            "192.168.1.10",
            vec![RegisterDefinition::holding_register("value", 0)],
        );

        let config = ModbusAdapterConfig::new("test")
            .with_device(device);

        let adapter = ModbusAdapter::new(config);

        assert_eq!(adapter.device_count(), 1);
        let devices = adapter.list_devices();
        assert_eq!(devices[0], "sensor1");
    }

    #[test]
    fn test_register_type_default() {
        let reg = RegisterDefinition::holding_register("test", 100);
        assert_eq!(reg.register_type, RegisterType::HoldingRegister);
    }

    #[test]
    fn test_data_type_default() {
        let reg = RegisterDefinition::holding_register("test", 100);
        assert_eq!(reg.data_type, ModbusDataType::Int16);
    }

    #[test]
    fn test_default_values() {
        let config = ModbusDeviceConfig::new("test", "localhost", vec![]);
        assert_eq!(config.port, 502);
        assert_eq!(config.slave_id, 1);
        assert_eq!(config.poll_interval_secs, 60);
    }

    #[test]
    fn test_modbus_data_types() {
        assert_eq!(ModbusDataType::Int16, ModbusDataType::Int16);
        assert_eq!(ModbusDataType::Uint16, ModbusDataType::Uint16);
        assert_eq!(ModbusDataType::Int32, ModbusDataType::Int32);
        assert_eq!(ModbusDataType::Uint32, ModbusDataType::Uint32);
        assert_eq!(ModbusDataType::Float32, ModbusDataType::Float32);
        assert_eq!(ModbusDataType::Bool, ModbusDataType::Bool);
    }

    #[test]
    fn test_scale_and_offset() {
        let config = ModbusAdapterConfig::new("test");
        let adapter = ModbusAdapter::new(config);

        // Test scale and offset calculation
        // If raw = 250, scale = 0.1, offset = -40
        // Then scaled = 250 * 0.1 + (-40) = 25 - 40 = -15
        let raw = 250.0;
        let scale = 0.1;
        let offset = -40.0;
        let scaled = raw * scale + offset;
        assert_eq!(scaled, -15.0);
    }

    #[test]
    fn test_default_parse_register_int16() {
        let data: [u8; 2] = [0x00, 0x64]; // 100 in big-endian
        let reg = RegisterDefinition::holding_register("test", 0)
            .with_data_type(ModbusDataType::Int16);
        let result = ModbusAdapter::default_parse_register(&data, &reg);
        assert!(matches!(result, crate::mdl::MetricValue::Integer(100)));
    }

    #[test]
    fn test_default_parse_register_with_scale() {
        let data: [u8; 2] = [0x00, 0xF0]; // 240 in big-endian
        let reg = RegisterDefinition::holding_register("voltage", 0)
            .with_data_type(ModbusDataType::Int16)
            .with_scale(0.1);
        let result = ModbusAdapter::default_parse_register(&data, &reg);
        assert!(matches!(result, crate::mdl::MetricValue::Float(f) if (f - 24.0).abs() < 0.1));
    }

    #[test]
    fn test_default_parse_register_float32() {
        let data: [u8; 4] = [0x41, 0xBE, 0x00, 0x00]; // 23.75 in big-endian
        let reg = RegisterDefinition::holding_register("test", 0)
            .with_data_type(ModbusDataType::Float32);
        let result = ModbusAdapter::default_parse_register(&data, &reg);
        if let crate::mdl::MetricValue::Float(f) = result {
            assert!((f - 23.75).abs() < 0.01);
        } else {
            panic!("Expected Float value");
        }
    }
}
