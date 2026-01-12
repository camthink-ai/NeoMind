//! Modbus TCP integration.
//!
//! Provides integration with Modbus TCP networks for industrial device communication.

use crate::{Integration, IntegrationMetadata, IntegrationType, IntegrationState};
use crate::protocols::BaseIntegration;
use edge_ai_core::integration::{
    IntegrationEvent, IntegrationCommand, IntegrationResponse, IntegrationError,
    Result as IntegrationResult,
};
use async_trait::async_trait;
use futures::Stream;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Modbus function codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ModbusFunction {
    ReadCoils = 0x01,
    ReadDiscreteInputs = 0x02,
    ReadHoldingRegisters = 0x03,
    ReadInputRegisters = 0x04,
    WriteSingleCoil = 0x05,
    WriteSingleRegister = 0x06,
    WriteMultipleCoils = 0x0F,
    WriteMultipleRegisters = 0x10,
}

impl ModbusFunction {
    /// Get the function code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// Modbus register type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModbusRegisterType {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
}

/// Modbus integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusConfig {
    /// Slave/unit ID.
    pub slave_id: u8,

    /// Host address.
    pub host: String,

    /// Port number.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Connection timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Polling interval in milliseconds.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,

    /// Register definitions for polling.
    #[serde(default)]
    pub registers: Vec<ModbusRegisterConfig>,
}

fn default_port() -> u16 { 502 }
fn default_timeout() -> u64 { 5000 }
fn default_poll_interval() -> u64 { 1000 }

impl ModbusConfig {
    /// Create a new Modbus configuration.
    pub fn new(host: impl Into<String>, slave_id: u8) -> Self {
        Self {
            slave_id,
            host: host.into(),
            port: default_port(),
            timeout_ms: default_timeout(),
            poll_interval_ms: default_poll_interval(),
            registers: Vec::new(),
        }
    }

    /// Set the port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Add a register configuration.
    pub fn with_register(mut self, register: ModbusRegisterConfig) -> Self {
        self.registers.push(register);
        self
    }

    /// Get the socket address.
    pub fn socket_addr(&self) -> IntegrationResult<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .map_err(|e| IntegrationError::Configuration(format!("Invalid address: {}", e)))
    }
}

/// Modbus register configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusRegisterConfig {
    /// Register name.
    pub name: String,

    /// Register type.
    pub register_type: ModbusRegisterType,

    /// Starting address.
    pub address: u16,

    /// Count (number of coils/registers).
    pub count: u16,

    /// Data type for parsing.
    #[serde(default)]
    pub data_type: ModbusDataType,

    /// Scale factor for value conversion.
    #[serde(default = "default_scale")]
    pub scale: f64,

    /// Offset for value conversion.
    #[serde(default = "default_offset")]
    pub offset: f64,
}

fn default_scale() -> f64 { 1.0 }
fn default_offset() -> f64 { 0.0 }

/// Modbus data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModbusDataType {
    #[default]
    U16,
    I16,
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
}

/// Modbus TCP client.
pub struct ModbusClient {
    /// Slave ID.
    slave_id: u8,

    /// TCP stream.
    stream: Option<TcpStream>,

    /// Timeout.
    timeout: Duration,
}

impl ModbusClient {
    /// Create a new Modbus client.
    pub fn new(slave_id: u8, timeout: Duration) -> Self {
        Self {
            slave_id,
            stream: None,
            timeout,
        }
    }

    /// Connect to the Modbus server.
    pub async fn connect(&mut self, addr: SocketAddr) -> IntegrationResult<()> {
        let stream = timeout(self.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| IntegrationError::ConnectionFailed("Connection timeout".to_string()))?
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        self.stream = Some(stream);
        Ok(())
    }

    /// Disconnect.
    pub fn disconnect(&mut self) {
        self.stream = None;
    }

    /// Read holding registers.
    pub async fn read_holding_registers(&mut self, address: u16, count: u16) -> IntegrationResult<Vec<u16>> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| IntegrationError::Stopped)?;

        // Build Modbus request
        let request = vec![
            self.slave_id,           // Slave ID
            ModbusFunction::ReadHoldingRegisters.code(),
            (address >> 8) as u8,     // Address high
            (address & 0xFF) as u8,    // Address low
            (count >> 8) as u8,        // Count high
            (count & 0xFF) as u8,       // Count low
        ];

        // Calculate CRC
        let crc = calculate_crc(&request);
        let mut full_request = request;
        full_request.extend_from_slice(&crc.to_le_bytes());

        // Send request
        stream.write_all(&full_request)
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        // Read response
        let mut response = vec![0u8; 5 + count as usize * 2]; // Header + data + CRC
        stream.read_exact(&mut response)
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        // Parse response (skip slave ID, function code, byte count)
        let mut values = Vec::new();
        for i in 0..count as usize {
            let high = response[3 + i * 2] as u16;
            let low = response[3 + i * 2 + 1] as u16;
            values.push((high << 8) | low);
        }

        Ok(values)
    }

    /// Write single register.
    pub async fn write_single_register(&mut self, address: u16, value: u16) -> IntegrationResult<()> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| IntegrationError::Stopped)?;

        let request = vec![
            self.slave_id,
            ModbusFunction::WriteSingleRegister.code(),
            (address >> 8) as u8,
            (address & 0xFF) as u8,
            (value >> 8) as u8,
            (value & 0xFF) as u8,
        ];

        let crc = calculate_crc(&request);
        let mut full_request = request;
        full_request.extend_from_slice(&crc.to_le_bytes());

        stream.write_all(&full_request)
            .await
            .map_err(|e| IntegrationError::ConnectionFailed(e.to_string()))?;

        Ok(())
    }
}

/// Calculate CRC16 for Modbus.
fn calculate_crc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0xA001;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Modbus integration.
pub struct ModbusIntegration {
    /// Base integration.
    base: BaseIntegration,

    /// Configuration.
    config: ModbusConfig,

    /// Event sender.
    sender: Arc<mpsc::Sender<IntegrationEvent>>,

    /// Running flag.
    running: Arc<std::sync::atomic::AtomicBool>,

    /// Register values cache.
    cache: Arc<parking_lot::Mutex<HashMap<String, f64>>>,
}

impl ModbusIntegration {
    /// Create a new Modbus integration.
    pub fn new(config: ModbusConfig) -> Self {
        let (sender, _) = mpsc::channel(1024);

        Self {
            base: BaseIntegration::new(
                format!("modbus_{}", uuid::Uuid::new_v4()),
                "Modbus TCP",
                IntegrationType::Modbus,
            ),
            config,
            sender: Arc::new(sender),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cache: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &ModbusConfig {
        &self.config
    }

    /// Read a register value.
    pub async fn read_register(&self, name: &str) -> IntegrationResult<f64> {
        let config = self.config.registers.iter()
            .find(|r| r.name == name)
            .ok_or_else(|| IntegrationError::NotFound(name.to_string()))?;

        let mut client = ModbusClient::new(
            self.config.slave_id,
            Duration::from_millis(self.config.timeout_ms),
        );

        let addr = self.config.socket_addr()?;
        client.connect(addr).await?;

        let values: Vec<u16> = client.read_holding_registers(config.address, config.count).await?;
        client.disconnect();

        let value = if values.len() == 1 {
            apply_data_type(values[0], config.data_type)
        } else {
            // Multiple registers - for now just return first
            apply_data_type(values[0], config.data_type)
        };

        let scaled = value * config.scale + config.offset;

        // Update cache
        self.cache.lock().insert(name.to_string(), scaled);

        Ok(scaled)
    }

    /// Write a register value.
    pub async fn write_register(&self, name: &str, value: f64) -> IntegrationResult<()> {
        let config = self.config.registers.iter()
            .find(|r| r.name == name)
            .ok_or_else(|| IntegrationError::NotFound(name.to_string()))?;

        // Reverse the scale/offset
        let raw = (value - config.offset) / config.scale;
        let raw_value = raw as u16;

        let mut client = ModbusClient::new(
            self.config.slave_id,
            Duration::from_millis(self.config.timeout_ms),
        );

        let addr = self.config.socket_addr()?;
        client.connect(addr).await?;
        client.write_single_register(config.address, raw_value).await?;
        client.disconnect();

        Ok(())
    }
}

/// Apply data type conversion.
fn apply_data_type(raw: u16, data_type: ModbusDataType) -> f64 {
    match data_type {
        ModbusDataType::U16 => raw as f64,
        ModbusDataType::I16 => (raw as i16) as f64,
        ModbusDataType::U32 => (raw as u32) as f64,
        ModbusDataType::I32 => (raw as i32) as f64,
        ModbusDataType::F64 => f64::from_bits(raw as u64),
        _ => raw as f64,
    }
}

#[async_trait]
impl Integration for ModbusIntegration {
    fn metadata(&self) -> &IntegrationMetadata {
        &self.base.metadata
    }

    fn state(&self) -> IntegrationState {
        self.base.to_integration_state()
    }

    async fn start(&self) -> IntegrationResult<()> {
        // Verify connection
        let mut client = ModbusClient::new(
            self.config.slave_id,
            Duration::from_millis(self.config.timeout_ms),
        );

        let addr = self.config.socket_addr()?;
        client.connect(addr).await?;
        client.disconnect();

        self.running.store(true, std::sync::atomic::Ordering::Relaxed);
        self.base.set_running(true);

        // Start polling task
        let sender = self.sender.clone();
        let registers = self.config.registers.clone();
        let slave_id = self.config.slave_id;
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);
        let socket_addr = addr;
        let cache = self.cache.clone();
        let running = self.running.clone();

        if !registers.is_empty() {
            tokio::spawn(async move {
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut client = ModbusClient::new(slave_id, timeout);
                    if let Ok(()) = client.connect(socket_addr).await {
                        for reg in &registers {
                            if let Ok(values) = client.read_holding_registers(reg.address, reg.count).await {
                                let values_vec: Vec<u16> = values;
                                let raw: u16 = if !values_vec.is_empty() { values_vec[0] } else { 0 };
                                let value = apply_data_type(raw, reg.data_type) * reg.scale + reg.offset;

                                cache.lock().insert(reg.name.clone(), value);

                                let event = IntegrationEvent::Data {
                                    source: "modbus".to_string(),
                                    data_type: reg.name.clone(),
                                    payload: serde_json::to_vec(&value).unwrap_or_default(),
                                    metadata: serde_json::json!({
                                        "address": reg.address,
                                        "register_type": reg.register_type,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp(),
                                };

                                let _ = sender.send(event);
                            }
                        }
                    }
                    client.disconnect();

                    tokio::time::sleep(poll_interval).await;
                }
            });
        }

        Ok(())
    }

    async fn stop(&self) -> IntegrationResult<()> {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
        self.base.set_running(false);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>> {
        // Create a new channel for this subscriber
        let (_tx, rx) = mpsc::channel(1024);
        // We'd need to store the sender to broadcast events
        // For now, return empty stream as this is a simplified implementation
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    async fn send_command(&self, command: IntegrationCommand) -> IntegrationResult<IntegrationResponse> {
        match command {
            IntegrationCommand::Query { target, .. } => {
                let value = self.read_register(&target).await?;
                Ok(IntegrationResponse::success(serde_json::json!({ "value": value })))
            }
            IntegrationCommand::CallService { service, params, .. } => {
                // Parse service as "read:<register>" or "write:<register>"
                if let Some(rest) = service.strip_prefix("read:") {
                    let value = self.read_register(rest).await?;
                    Ok(IntegrationResponse::success(serde_json::json!({ "value": value })))
                } else if let Some(rest) = service.strip_prefix("write:") {
                    let value = params["value"].as_f64()
                        .ok_or_else(|| IntegrationError::TransformationFailed("Invalid value".to_string()))?;
                    self.write_register(rest, value).await?;
                    Ok(IntegrationResponse::success(serde_json::json!({})))
                } else {
                    Err(IntegrationError::CommandFailed(format!("Unknown service: {}", service)))
                }
            }
            IntegrationCommand::SendData { .. } => {
                Err(IntegrationError::CommandFailed(
                    "SendData not supported for Modbus".to_string()
                ))
            }
        }
    }
}

/// Create a Modbus integration from a config.
pub fn create_modbus_integration(
    id: impl Into<String>,
    config: ModbusConfig,
) -> IntegrationResult<ModbusIntegration> {
    let mut integration = ModbusIntegration::new(config);
    integration.base = BaseIntegration::new(id, "Modbus TCP", IntegrationType::Modbus);
    Ok(integration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modbus_config() {
        let config = ModbusConfig::new("192.168.1.100", 1)
            .with_port(502)
            .with_register(ModbusRegisterConfig {
                name: "temperature".to_string(),
                register_type: ModbusRegisterType::HoldingRegister,
                address: 0,
                count: 1,
                data_type: ModbusDataType::I16,
                scale: 0.1,
                offset: 0.0,
            });

        assert_eq!(config.slave_id, 1);
        assert_eq!(config.host, "192.168.1.100");
        assert_eq!(config.port, 502);
        assert_eq!(config.registers.len(), 1);
    }

    #[test]
    fn test_crc_calculation() {
        // Test that CRC is deterministic
        let data = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x01];
        let crc1 = calculate_crc(&data);
        let crc2 = calculate_crc(&data);
        assert_eq!(crc1, crc2);

        // Different inputs should produce different CRCs
        let data2 = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x02];
        let crc3 = calculate_crc(&data2);
        assert_ne!(crc1, crc3);
    }

    #[test]
    fn test_modbus_integration() {
        let config = ModbusConfig::new("localhost", 1);
        let integration = ModbusIntegration::new(config);
        assert_eq!(integration.metadata().integration_type, IntegrationType::Modbus);
        assert!(!integration.base.is_running());
    }
}
