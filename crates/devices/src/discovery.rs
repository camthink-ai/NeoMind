//! Device discovery functionality.
//!
//! Supports various discovery methods:
//! - MQTT broker scanning
//! - HASS MQTT Discovery protocol (Tasmota, Shelly, ESPHome)
//! - Modbus network scanning
//! - mDNS/Bonjour service discovery
//! - IP range scanning

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, Ipv4Addr};
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use super::mdl::{DeviceId, DeviceType, MetricDefinition};
use super::mqtt::{MqttDevice, MqttConfig};
use super::modbus::{ModbusDevice, ModbusConfig, RegisterDefinition};

// Re-export HASS discovery types
pub use super::hass_discovery::{
    HassDiscoveryConfig, HassDiscoveryMessage, HassDiscoveryError, HassTopicParts,
    parse_discovery_message, is_discovery_topic, is_supported_component,
    discovery_subscription_pattern, component_to_device_type,
};
pub use super::hass_discovery_mapper::{map_hass_to_mdl, register_hass_device_type, generate_uplink_config};
pub use super::hass_discovery_listener::{HassDiscoveryListener, HassDiscoveryConfig as HassDiscoveryListenerConfig};

/// Discovery method type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryMethodType {
    /// MQTT subscription-based discovery
    Mqtt,
    /// HASS MQTT Discovery protocol
    HassDiscovery,
    /// Modbus TCP scanning
    Modbus,
    /// mDNS/Bonjour service discovery
    Mdns,
    /// IP range port scanning
    PortScan,
    /// Manual registration
    Manual,
}

/// Result of a device discovery operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    /// Suggested device ID
    pub id: DeviceId,
    /// Device type (if detected)
    pub device_type: Option<DeviceType>,
    /// Connection information
    pub connection: DeviceConnection,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,
    /// Additional information
    pub info: HashMap<String, String>,
}

/// Connection information for a discovered device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceConnection {
    Mqtt {
        broker: String,
        port: u16,
        topic_prefix: String,
    },
    Modbus {
        host: String,
        port: u16,
        slave_id: u8,
    },
    Tcp {
        host: String,
        port: u16,
    },
}

/// Configuration for MQTT-based discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttDiscoveryConfig {
    /// MQTT broker address
    pub broker: String,
    /// Broker port
    pub port: u16,
    /// Topics to subscribe to for discovery (supports wildcards)
    pub topics: Vec<String>,
    /// Subscription timeout in seconds
    #[serde(default = "default_discovery_timeout")]
    pub timeout_secs: u64,
}

fn default_discovery_timeout() -> u64 { 30 }

impl MqttDiscoveryConfig {
    pub fn new(broker: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            port: 1883,
            topics: vec!["#".to_string()],
            timeout_secs: 30,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_topics(mut self, topics: Vec<String>) -> Self {
        self.topics = topics;
        self
    }
}

/// Configuration for Modbus network scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusDiscoveryConfig {
    /// IP address range (e.g., "192.168.1.0/24")
    pub ip_range: String,
    /// Modbus TCP port to scan
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    /// Slave IDs to try
    #[serde(default = "default_slave_ids")]
    pub slave_ids: Vec<u8>,
    /// Connection timeout per host (ms)
    #[serde(default = "default_scan_timeout")]
    pub timeout_ms: u64,
    /// Number of concurrent scans
    #[serde(default = "default_concurrent_scans")]
    pub concurrent: usize,
}

fn default_modbus_port() -> u16 { 502 }
fn default_slave_ids() -> Vec<u8> { vec![1, 2, 3, 4, 5] }
fn default_scan_timeout() -> u64 { 500 }
fn default_concurrent_scans() -> usize { 50 }

impl ModbusDiscoveryConfig {
    pub fn new(ip_range: impl Into<String>) -> Self {
        Self {
            ip_range: ip_range.into(),
            port: 502,
            slave_ids: vec![1, 2, 3, 4, 5],
            timeout_ms: 500,
            concurrent: 50,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_slave_ids(mut self, slave_ids: Vec<u8>) -> Self {
        self.slave_ids = slave_ids;
        self
    }
}

/// Discovery error.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Invalid IP range: {0}")]
    InvalidIpRange(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Timeout")]
    Timeout,

    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// Result of a discovery operation.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Method used
    pub method: DiscoveryMethodType,
    /// Devices found
    pub devices: Vec<DiscoveredDevice>,
    /// Duration of discovery
    pub duration: Duration,
    /// Errors encountered (non-fatal)
    pub errors: Vec<String>,
}

/// Device discovery manager.
pub struct DeviceDiscovery;

impl Default for DeviceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceDiscovery {
    pub fn new() -> Self {
        Self
    }

    /// Discover devices using Modbus TCP scanning.
    pub async fn scan_modbus(&self, config: ModbusDiscoveryConfig) -> Result<DiscoveryResult, DiscoveryError> {
        let start = std::time::Instant::now();
        let mut devices = Vec::new();
        let mut errors = Vec::new();

        // Parse IP range
        let addrs = self.parse_ip_range(&config.ip_range)?;

        // Scan hosts concurrently
        let results = stream::iter(addrs)
            .map(|addr| {
                let port = config.port;
                let timeout = config.timeout_ms;
                let slave_ids = config.slave_ids.clone();
                async move {
                    self.scan_modbus_host(addr, port, &slave_ids, timeout).await
                }
            })
            .buffer_unordered(config.concurrent)
            .collect::<Vec<_>>()
            .await;

        for result in results {
            match result {
                Ok(Some(device)) => devices.push(device),
                Ok(None) => {},
                Err(e) => errors.push(e.to_string()),
            }
        }

        Ok(DiscoveryResult {
            method: DiscoveryMethodType::Modbus,
            devices,
            duration: start.elapsed(),
            errors,
        })
    }

    /// Scan a single Modbus host.
    async fn scan_modbus_host(
        &self,
        host: IpAddr,
        port: u16,
        slave_ids: &[u8],
        timeout_ms: u64,
    ) -> Result<Option<DiscoveredDevice>, DiscoveryError> {
        let addr = SocketAddr::new(host, port);

        // Try to connect
        let timeout_result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            TcpStream::connect(&addr),
        )
        .await;

        let _conn = match timeout_result {
            Ok(Ok(stream)) => stream,
            _ => return Ok(None), // Not an error, just no device there
        };

        // Connection successful - it's likely a Modbus device
        // Try to determine slave ID by reading basic registers
        let mut info = HashMap::new();
        info.insert("host".to_string(), host.to_string());
        info.insert("port".to_string(), port.to_string());

        // For simplicity, assume first slave ID works
        let slave_id = slave_ids.first().copied().unwrap_or(1);

        Ok(Some(DiscoveredDevice {
            id: DeviceId::new(),
            device_type: Some(DeviceType::Controller),
            connection: DeviceConnection::Modbus {
                host: host.to_string(),
                port,
                slave_id,
            },
            confidence: 0.8, // Modbus is pretty reliable if port is open
            info,
        }))
    }

    /// Parse an IP range string into a list of addresses.
    fn parse_ip_range(&self, range: &str) -> Result<Vec<IpAddr>, DiscoveryError> {
        // Support formats:
        // - 192.168.1.0/24 (CIDR)
        // - 192.168.1.1-100 (range)
        // - 192.168.1.1 (single)

        if range.contains('/') {
            // CIDR notation
            let parts: Vec<&str> = range.split('/').collect();
            if parts.len() != 2 {
                return Err(DiscoveryError::InvalidIpRange(range.to_string()));
            }

            let base: Ipv4Addr = parts[0]
                .parse()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;
            let prefix: u32 = parts[1]
                .parse()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;

            if prefix > 32 {
                return Err(DiscoveryError::InvalidIpRange(range.to_string()));
            }

            let addrs = (0..2u32.pow(32 - prefix))
                .map(|i| {
                    let base_u32 = u32::from(base);
                    let ip = Ipv4Addr::from(base_u32 + i);
                    IpAddr::V4(ip)
                })
                .collect();

            Ok(addrs)
        } else if range.contains('-') {
            // Range notation (e.g., 192.168.1.1-100)
            let parts: Vec<&str> = range.split('-').collect();
            if parts.len() != 2 {
                return Err(DiscoveryError::InvalidIpRange(range.to_string()));
            }

            let base_str = parts[0];
            let last_octet_str = parts[1];

            // Get the first three octets
            let octets: Vec<&str> = base_str.split('.').collect();
            if octets.len() != 4 {
                return Err(DiscoveryError::InvalidIpRange(range.to_string()));
            }

            let prefix = format!("{}.{}.{}", octets[0], octets[1], octets[2]);
            let start = octets[3]
                .parse::<u8>()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;
            let end = last_octet_str
                .parse::<u8>()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;

            let addrs = (start..=end)
                .map(|i| format!("{}.{}", prefix, i).parse::<IpAddr>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;

            Ok(addrs)
        } else {
            // Single IP
            let addr: IpAddr = range
                .parse()
                .map_err(|_| DiscoveryError::InvalidIpRange(range.to_string()))?;
            Ok(vec![addr])
        }
    }

    /// Create a Modbus device from a discovered device.
    pub fn create_modbus_device(
        &self,
        discovered: &DiscoveredDevice,
        name: String,
        registers: Vec<RegisterDefinition>,
    ) -> Result<ModbusDevice, DiscoveryError> {
        match &discovered.connection {
            DeviceConnection::Modbus { host, port, slave_id } => {
                let config = ModbusConfig::new(host)
                    .with_port(*port)
                    .with_slave_id(*slave_id);

                Ok(ModbusDevice::new(name, config, registers))
            }
            _ => Err(DiscoveryError::Protocol(
                "Not a Modbus device".to_string(),
            )),
        }
    }

    /// Create an MQTT device from a discovered device.
    pub fn create_mqtt_device(
        &self,
        discovered: &DiscoveredDevice,
        name: String,
        metrics: Vec<MetricDefinition>,
    ) -> Result<MqttDevice, DiscoveryError> {
        match &discovered.connection {
            DeviceConnection::Mqtt {
                broker,
                port,
                topic_prefix,
            } => {
                let config = MqttConfig::new(broker, topic_prefix).with_port(*port);
                Ok(MqttDevice::new(name, config, metrics))
            }
            _ => Err(DiscoveryError::Protocol(
                "Not an MQTT device".to_string(),
            )),
        }
    }

    /// Scan for open ports on a single host.
    pub async fn scan_ports(
        &self,
        host: &str,
        ports: Vec<u16>,
        timeout_ms: u64,
    ) -> Result<Vec<u16>, DiscoveryError> {
        let addr: IpAddr = host
            .parse()
            .map_err(|_| DiscoveryError::InvalidIpRange(host.to_string()))?;

        let results = stream::iter(ports)
            .map(|port| {
                let addr = addr;
                async move {
                    let socket_addr = SocketAddr::new(addr, port);
                    match tokio::time::timeout(
                        Duration::from_millis(timeout_ms),
                        TcpStream::connect(&socket_addr),
                    )
                    .await
                    {
                        Ok(Ok(_)) => Some(port),
                        _ => None,
                    }
                }
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;

        Ok(results.into_iter().filter_map(|p| p).collect())
    }

    /// Discover common service ports.
    pub async fn discover_services(&self, host: &str) -> Result<Vec<DiscoveredDevice>, DiscoveryError> {
        let common_ports = vec![
            21,   // FTP
            22,   // SSH
            23,   // Telnet
            80,   // HTTP
            443,  // HTTPS
            1883, // MQTT
            502,  // Modbus
            1883, // MQTT
            5683, // CoAP
            7896, // ThingsBoard
        ];

        let open_ports = self.scan_ports(host, common_ports, 500).await?;

        let mut devices = Vec::new();

        for port in open_ports {
            let device_type = match port {
                1883 => Some(DeviceType::Gateway),
                502 => Some(DeviceType::Controller),
                _ => None,
            };

            let mut info = HashMap::new();
            info.insert("host".to_string(), host.to_string());
            info.insert("port".to_string(), port.to_string());

            devices.push(DiscoveredDevice {
                id: DeviceId::new(),
                device_type,
                connection: DeviceConnection::Tcp {
                    host: host.to_string(),
                    port,
                },
                confidence: 0.7,
                info,
            });
        }

        Ok(devices)
    }

    /// Parse a HASS discovery message
    pub fn parse_hass_discovery(
        &self,
        topic: &str,
        payload: &[u8],
    ) -> Result<HassDiscoveryMessage, DiscoveryError> {
        parse_discovery_message(topic, payload)
            .map_err(|e| DiscoveryError::Protocol(e.to_string()))
    }

    /// Convert HASS discovery message to MDL device type definition
    pub fn hass_to_mdl(
        &self,
        msg: &HassDiscoveryMessage,
    ) -> Result<super::mdl_format::DeviceTypeDefinition, DiscoveryError> {
        map_hass_to_mdl(msg)
            .map_err(|e| DiscoveryError::Protocol(e.to_string()))
    }
}

/// HASS Discovery static methods (available without DeviceDiscovery instance)
impl DeviceDiscovery {
    /// Get HASS discovery subscription topic
    pub fn hass_discovery_topic(components: Option<Vec<String>>) -> String {
        if let Some(comps) = components {
            if comps.is_empty() {
                "homeassistant/+/config".to_string()
            } else {
                comps.iter()
                    .map(|c| format!("homeassistant/{}/+/config", c))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        } else {
            "homeassistant/+/config".to_string()
        }
    }

    /// Check if a topic is a HASS discovery topic
    pub fn is_hass_discovery_topic(topic: &str) -> bool {
        is_discovery_topic(topic)
    }

    /// Get supported HASS components
    pub fn hass_supported_components() -> Vec<&'static str> {
        vec![
            "sensor", "binary_sensor", "switch", "light", "cover",
            "climate", "fan", "lock", "camera", "vacuum", "media_player",
        ]
    }

    /// Get device type for a HASS component
    pub fn hass_component_to_device_type(component: &str) -> Option<&'static str> {
        component_to_device_type(component)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ip_range_single() {
        let discovery = DeviceDiscovery::new();
        let addrs = discovery.parse_ip_range("192.168.1.1").unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].to_string(), "192.168.1.1");
    }

    #[test]
    fn test_parse_ip_range_cidr() {
        let discovery = DeviceDiscovery::new();
        let addrs = discovery.parse_ip_range("192.168.1.0/30").unwrap();
        assert_eq!(addrs.len(), 4);
    }

    #[test]
    fn test_parse_ip_range_dash() {
        let discovery = DeviceDiscovery::new();
        let addrs = discovery.parse_ip_range("192.168.1.1-5").unwrap();
        assert_eq!(addrs.len(), 5);
        assert_eq!(addrs[0].to_string(), "192.168.1.1");
        assert_eq!(addrs[4].to_string(), "192.168.1.5");
    }

    #[tokio::test]
    async fn test_discover_services_localhost() {
        let discovery = DeviceDiscovery::new();
        // This test might fail if no services are running
        let result = discovery.discover_services("127.0.0.1").await;
        assert!(result.is_ok());
    }
}
