//! Device discovery functionality.
//!
//! Supports various discovery methods:
//! - MQTT broker scanning
//! - mDNS/Bonjour service discovery
//! - IP range scanning

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use super::mdl::{DeviceId, DeviceType, MetricDefinition};

/// Discovery method type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryMethodType {
    /// MQTT subscription-based discovery
    Mqtt,
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

fn default_discovery_timeout() -> u64 {
    30
}

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

    /// Parse an IP range string into a list of addresses.
    pub fn parse_ip_range(&self, range: &str) -> Result<Vec<IpAddr>, DiscoveryError> {
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

    /// Create an MQTT device from a discovered device.
    ///
    /// **Note**: MQTT device creation should use DeviceService instead.
    /// This method is kept for API compatibility but always returns an error.
    pub fn create_mqtt_device(
        &self,
        _discovered: &DiscoveredDevice,
        _name: String,
        _metrics: Vec<MetricDefinition>,
    ) -> Result<serde_json::Value, DiscoveryError> {  // Changed from MqttDevice
        Err(DiscoveryError::Protocol(
            "MQTT device creation should use DeviceService with MqttAdapter. See adapters::mqtt module.".to_string()
        ))
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

        Ok(results.into_iter().flatten().collect())
    }

    /// Discover common service ports.
    pub async fn discover_services(
        &self,
        host: &str,
    ) -> Result<Vec<DiscoveredDevice>, DiscoveryError> {
        let common_ports = vec![
            21,   // FTP
            22,   // SSH
            23,   // Telnet
            80,   // HTTP
            443,  // HTTPS
            1883, // MQTT
            5683, // CoAP
            7896, // ThingsBoard
        ];

        let open_ports = self.scan_ports(host, common_ports, 500).await?;

        let mut devices = Vec::new();

        for port in open_ports {
            let device_type = match port {
                1883 => Some(DeviceType::Gateway),
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
