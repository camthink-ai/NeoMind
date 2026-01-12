//! Modbus device adapter.
//!
//! Implements the Device trait for devices communicating via Modbus protocol.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::mdl::{
    Device, Command, ConnectionStatus, DeviceCapability, DeviceError, DeviceId,
    DeviceInfo, DeviceState, DeviceType, MetricDefinition, MetricDataType, MetricValue,
};

/// Modbus register type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegisterType {
    /// Coil (read-write, 1 bit)
    Coil,
    /// Discrete Input (read-only, 1 bit)
    DiscreteInput,
    /// Input Register (read-only, 16 bits)
    InputRegister,
    /// Holding Register (read-write, 16 bits)
    HoldingRegister,
}

/// Definition of a Modbus register mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDefinition {
    /// Metric name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Register address
    pub address: u16,
    /// Register type
    pub register_type: RegisterType,
    /// Data type for interpretation
    pub data_type: MetricDataType,
    /// Scaling factor (for numeric conversion)
    pub scale: Option<f64>,
    /// Unit of measurement
    pub unit: Option<String>,
    /// Number of registers for this value (for 32-bit values)
    pub count: u16,
}

impl RegisterDefinition {
    /// Create a new coil definition.
    pub fn coil(name: impl Into<String>, address: u16) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            address,
            register_type: RegisterType::Coil,
            data_type: MetricDataType::Boolean,
            scale: None,
            unit: None,
            count: 1,
        }
    }

    /// Create a new input register definition.
    pub fn input(name: impl Into<String>, address: u16) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            address,
            register_type: RegisterType::InputRegister,
            data_type: MetricDataType::Integer,
            scale: None,
            unit: None,
            count: 1,
        }
    }

    /// Create a new holding register definition.
    pub fn holding(name: impl Into<String>, address: u16) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            address,
            register_type: RegisterType::HoldingRegister,
            data_type: MetricDataType::Integer,
            scale: None,
            unit: None,
            count: 1,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the data type.
    pub fn with_data_type(mut self, data_type: MetricDataType) -> Self {
        self.data_type = data_type;
        self
    }

    /// Set the scaling factor.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = Some(scale);
        self
    }

    /// Set the unit.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set register count (for multi-register values).
    pub fn with_count(mut self, count: u16) -> Self {
        self.count = count;
        self
    }

    /// Convert to MetricDefinition.
    pub fn to_metric_definition(&self) -> MetricDefinition {
        MetricDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            data_type: self.data_type.clone(),
            unit: self.unit.clone(),
            read_only: matches!(
                self.register_type,
                RegisterType::DiscreteInput | RegisterType::InputRegister
            ),
            min: None,
            max: None,
        }
    }
}

/// Configuration for a Modbus device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusConfig {
    /// Device IP address or hostname
    pub host: String,

    /// Modbus TCP port
    #[serde(default = "default_modbus_port")]
    pub port: u16,

    /// Slave/Unit ID
    #[serde(default = "default_slave_id")]
    pub slave_id: u8,

    /// Connection timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Polling interval in milliseconds (for continuous monitoring)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
}

fn default_modbus_port() -> u16 { 502 }
fn default_slave_id() -> u8 { 1 }
fn default_timeout() -> u64 { 5000 }
fn default_poll_interval() -> u64 { 1000 }

impl ModbusConfig {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 502,
            slave_id: 1,
            timeout_ms: 5000,
            poll_interval_ms: 1000,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_slave_id(mut self, slave_id: u8) -> Self {
        self.slave_id = slave_id;
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn full_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Device that communicates via Modbus TCP.
pub struct ModbusDevice {
    /// Unique device identifier
    id: DeviceId,
    /// Human-readable name
    name: String,
    /// Modbus configuration
    config: ModbusConfig,
    /// Register mappings
    registers: HashMap<String, RegisterDefinition>,
    /// Cached register values
    cached_values: Arc<RwLock<HashMap<String, MetricValue>>>,
    /// Device state
    state: Arc<RwLock<DeviceState>>,
    /// Device type
    device_type: DeviceType,
    /// Device location (optional)
    location: Option<String>,
}

impl ModbusDevice {
    /// Create a new Modbus device.
    pub fn new(
        name: impl Into<String>,
        config: ModbusConfig,
        registers: Vec<RegisterDefinition>,
    ) -> Self {
        let id = DeviceId::new();
        let name = name.into();

        let registers_map: HashMap<String, RegisterDefinition> = registers
            .into_iter()
            .map(|r| (r.name.clone(), r))
            .collect();

        // Determine device type from register types
        let has_read_only = registers_map.values().any(|r| {
            matches!(
                r.register_type,
                RegisterType::DiscreteInput | RegisterType::InputRegister
            )
        });
        let has_read_write = registers_map.values().any(|r| {
            matches!(r.register_type, RegisterType::Coil | RegisterType::HoldingRegister)
        });

        let device_type = match (has_read_only, has_read_write) {
            (true, true) => DeviceType::Controller,
            (true, false) => DeviceType::Sensor,
            (false, true) => DeviceType::Actuator,
            (false, false) => DeviceType::Controller,
        };

        let state = DeviceState {
            status: ConnectionStatus::Disconnected,
            last_seen: None,
            error: None,
        };

        Self {
            id,
            name,
            config,
            registers: registers_map,
            cached_values: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(state)),
            device_type,
            location: None,
        }
    }

    /// Create a sensor device (read-only input registers).
    pub fn sensor(
        name: impl Into<String>,
        host: impl Into<String>,
        registers: Vec<RegisterDefinition>,
    ) -> Self {
        let config = ModbusConfig::new(host);
        Self::new(name, config, registers)
    }

    /// Set the device location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Read a raw Modbus register.
    async fn read_register(&self, reg_def: &RegisterDefinition) -> Result<MetricValue, DeviceError> {
        // Simulate reading from Modbus device
        // In a real implementation, this would use tokio_modbus client
        match reg_def.register_type {
            RegisterType::Coil => Ok(MetricValue::Boolean(false)),
            RegisterType::DiscreteInput => Ok(MetricValue::Boolean(false)),
            RegisterType::InputRegister => {
                // Apply scaling factor if present
                let raw = 100i64; // Simulated value
                let value = if let Some(scale) = reg_def.scale {
                    MetricValue::Float(raw as f64 * scale)
                } else {
                    MetricValue::Integer(raw)
                };
                Ok(value)
            }
            RegisterType::HoldingRegister => {
                let raw = 100i64;
                let value = if let Some(scale) = reg_def.scale {
                    MetricValue::Float(raw as f64 * scale)
                } else {
                    MetricValue::Integer(raw)
                };
                Ok(value)
            }
        }
    }

    /// Write to a Modbus register.
    async fn write_register(&self, reg_def: &RegisterDefinition, value: &MetricValue) -> Result<(), DeviceError> {
        if matches!(
            reg_def.register_type,
            RegisterType::DiscreteInput | RegisterType::InputRegister
        ) {
            return Err(DeviceError::InvalidCommand(
                "Cannot write to read-only register".to_string(),
            ));
        }

        // Simulate writing to Modbus device
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Update cached value
        let mut values = self.cached_values.write().await;
        values.insert(reg_def.name.clone(), value.clone());

        Ok(())
    }

    /// Get register definition by name.
    pub fn register(&self, name: &str) -> Option<&RegisterDefinition> {
        self.registers.get(name)
    }
}

#[async_trait]
impl Device for ModbusDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn capabilities(&self) -> Vec<DeviceCapability> {
        let mut caps = vec![DeviceCapability::ReadNumeric];

        let has_writable = self.registers.values().any(|r| {
            matches!(r.register_type, RegisterType::Coil | RegisterType::HoldingRegister)
        });

        if has_writable {
            caps.push(DeviceCapability::WriteNumeric);
        }

        caps
    }

    fn metrics(&self) -> Vec<MetricDefinition> {
        self.registers
            .values()
            .map(|r| r.to_metric_definition())
            .collect()
    }

    fn commands(&self) -> Vec<String> {
        // Modbus devices typically don't have named commands
        // Writing to registers is done via write_metric or direct register write
        Vec::new()
    }

    async fn read_metric(&self, metric: &str) -> Result<MetricValue, DeviceError> {
        let reg_def = self
            .registers
            .get(metric)
            .ok_or_else(|| DeviceError::InvalidMetric(format!("Unknown metric: {}", metric)))?;

        // Check cache first
        {
            let values = self.cached_values.read().await;
            if let Some(cached) = values.get(metric) {
                return Ok(cached.clone());
            }
        }

        // Read from device
        let value = self.read_register(reg_def).await?;

        // Cache the value
        {
            let mut values = self.cached_values.write().await;
            values.insert(metric.to_string(), value.clone());
        }

        // Update last seen
        let mut state = self.state.write().await;
        state.last_seen = Some(chrono::Utc::now());

        Ok(value)
    }

    async fn write_command(&self, command: &Command) -> Result<Option<MetricValue>, DeviceError> {
        // For Modbus, commands are writes to specific registers
        // The command name should be the register name
        if let Some(reg_def) = self.registers.get(&command.name) {
            if let Some((_, param)) = command.parameters.iter().next() {
                self.write_register(reg_def, &param.value).await?;
            }
            Ok(None)
        } else {
            Err(DeviceError::InvalidCommand(format!("Unknown register: {}", command.name)))
        }
    }

    async fn connect(&mut self) -> Result<(), DeviceError> {
        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Connecting;
        drop(state);

        // Simulate connection (in real implementation, would connect to Modbus device)
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Connected;
        state.last_seen = Some(chrono::Utc::now());
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), DeviceError> {
        let mut state = self.state.write().await;
        state.status = ConnectionStatus::Disconnected;
        state.last_seen = Some(chrono::Utc::now());
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        matches!(state.status, ConnectionStatus::Connected)
    }

    async fn state(&self) -> DeviceState {
        let state = self.state.read().await;
        state.clone()
    }

    fn info(&self) -> DeviceInfo {
        // Use try_read to avoid blocking; fall back to default state if locked
        let current_state = self.state.try_read()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_else(|| DeviceState {
                status: ConnectionStatus::Disconnected,
                last_seen: None,
                error: None,
            });

        DeviceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            device_type: self.device_type,
            protocol: "modbus".to_string(),
            status: current_state.status,
            capabilities: self.capabilities(),
            metrics: self.metrics(),
            commands: self.commands(),
            location: self.location.clone(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("host".to_string(), self.config.host.clone());
                meta.insert("port".to_string(), self.config.port.to_string());
                meta.insert("slave_id".to_string(), self.config.slave_id.to_string());
                meta
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_definition() {
        let reg = RegisterDefinition::input("temperature", 0)
            .with_description("Temperature sensor")
            .with_scale(0.1)
            .with_unit("°C");

        assert_eq!(reg.name, "temperature");
        assert_eq!(reg.address, 0);
        assert_eq!(reg.register_type, RegisterType::InputRegister);
        assert_eq!(reg.scale, Some(0.1));
        assert_eq!(reg.unit, Some("°C".to_string()));
    }

    #[test]
    fn test_modbus_config() {
        let config = ModbusConfig::new("192.168.1.100")
            .with_port(502)
            .with_slave_id(2)
            .with_timeout(10000);

        assert_eq!(config.host, "192.168.1.100");
        assert_eq!(config.port, 502);
        assert_eq!(config.slave_id, 2);
        assert_eq!(config.timeout_ms, 10000);
    }

    #[tokio::test]
    async fn test_modbus_device() {
        let registers = vec![
            RegisterDefinition::input("temperature", 0)
                .with_scale(0.1)
                .with_unit("°C"),
            RegisterDefinition::input("humidity", 1)
                .with_scale(0.1)
                .with_unit("%"),
        ];

        let device = ModbusDevice::sensor("WeatherStation", "192.168.1.100", registers);

        assert_eq!(device.name(), "WeatherStation");
        assert_eq!(device.device_type(), DeviceType::Sensor);
        assert_eq!(device.metrics().len(), 2);
    }

    #[tokio::test]
    async fn test_modbus_read_metric() {
        let registers = vec![
            RegisterDefinition::input("temperature", 0)
                .with_scale(0.1)
                .with_unit("°C"),
        ];

        let device = ModbusDevice::sensor("TempSensor", "192.168.1.100", registers);

        let value = device.read_metric("temperature").await.unwrap();
        // Simulated value is 100, scaled by 0.1 = 10.0
        assert_eq!(value, MetricValue::Float(10.0));
    }
}
