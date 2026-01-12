//! Embedded MQTT Broker
//!
//! This module provides an embedded MQTT broker using rumqttd.
//! The broker runs in the same process as NeoTalk, eliminating the need
//! for an external MQTT broker installation.
//!
//! ## Configuration
//!
//! ```toml
//! [mqtt]
//! listen = "0.0.0.0"  # Listen address
//! port = 1883        # Broker listening port
//! ```
//!
//! External broker connections are managed via the data sources page.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Embedded MQTT broker error type
#[derive(Debug, Error)]
pub enum EmbeddedBrokerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Broker error: {0}")]
    Broker(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Broker mode configuration (deprecated, kept for compatibility)
///
/// Note: NeoTalk now always uses the embedded broker. External broker
/// connections are managed via the data sources page (ExternalBroker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrokerMode {
    /// Use external MQTT broker (deprecated)
    External,
    /// Use embedded broker (default)
    Embedded,
}

impl Default for BrokerMode {
    fn default() -> Self {
        Self::Embedded
    }
}

/// Configuration for the embedded broker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedBrokerConfig {
    /// Listening address for embedded broker
    #[serde(default = "default_listen_addr")]
    pub listen: String,

    /// Listening port for embedded broker
    #[serde(default = "default_port")]
    pub port: u16,

    /// Maximum number of concurrent connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Maximum payload size in bytes
    #[serde(default = "default_max_payload")]
    pub max_payload_size: usize,

    /// Connection timeout in milliseconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_ms: u16,

    /// Enable dynamic topic filters
    #[serde(default = "default_dynamic_filters")]
    pub dynamic_filters: bool,
}

fn default_listen_addr() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    1883
}

fn default_max_connections() -> usize {
    1000
}

fn default_max_payload() -> usize {
    268435456 // 256 MB
}

fn default_connection_timeout() -> u16 {
    60000 // 60 seconds
}

fn default_dynamic_filters() -> bool {
    true
}

impl Default for EmbeddedBrokerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen_addr(),
            port: default_port(),
            max_connections: default_max_connections(),
            max_payload_size: default_max_payload(),
            connection_timeout_ms: default_connection_timeout(),
            dynamic_filters: default_dynamic_filters(),
        }
    }
}

impl EmbeddedBrokerConfig {
    /// Create a new embedded broker config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the listening address
    pub fn with_listen(mut self, listen: impl Into<String>) -> Self {
        self.listen = listen.into();
        self
    }

    /// Set the listening port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set max connections
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Get the full socket address
    pub fn socket_addr(&self) -> Result<SocketAddr, EmbeddedBrokerError> {
        format!("{}:{}", self.listen, self.port)
            .parse()
            .map_err(|e| EmbeddedBrokerError::Config(format!("Invalid address: {}", e)))
    }
}

/// Embedded MQTT broker handle
///
/// This handle manages the lifecycle of the embedded broker.
pub struct EmbeddedBroker {
    config: EmbeddedBrokerConfig,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl EmbeddedBroker {
    /// Create a new embedded broker with the given configuration
    pub fn new(config: EmbeddedBrokerConfig) -> Self {
        Self {
            config,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Create with default configuration
    pub fn with_default() -> Self {
        Self::new(EmbeddedBrokerConfig::default())
    }

    /// Check if the broker is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Start the embedded broker in a background thread
    ///
    /// This method spawns a new thread that runs the broker.
    /// The broker will listen on the configured address and port.
    pub fn start(&self) -> Result<(), EmbeddedBrokerError> {
        if self.is_running() {
            tracing::warn!("Embedded broker is already running");
            return Ok(());
        }

        // Check if port is already in use (possibly by a previous instance)
        if is_broker_running(self.config.port) {
            tracing::info!("Embedded broker port {} already in use, assuming already running", self.config.port);
            self.running.store(true, std::sync::atomic::Ordering::Relaxed);
            return Ok(());
        }

        let addr = self.config.socket_addr()?;
        let running = self.running.clone();
        let max_connections = self.config.max_connections;
        let max_payload = self.config.max_payload_size;
        let connection_timeout = self.config.connection_timeout_ms;
        let dynamic_filters = self.config.dynamic_filters;

        running.store(true, std::sync::atomic::Ordering::Relaxed);

        let _handle = thread::Builder::new()
            .name("neotalk-broker".to_string())
            .spawn(move || {
                tracing::info!("Starting embedded MQTT broker on {}", addr);

                // Build minimal broker config
                let mut broker_config = rumqttd::Config {
                    id: 0,
                    router: rumqttd::RouterConfig {
                        max_connections,
                        max_outgoing_packet_count: 200,
                        max_segment_size: 1048576, // 1 MB
                        max_segment_count: 10,
                        custom_segment: None,
                        initialized_filters: None,
                        // Note: shared_subscriptions_strategy is private in rumqttd 0.19
                        // Using default value through serde default
                        ..Default::default()
                    },
                    v4: None,
                    v5: None,
                    ws: None,
                    cluster: None,
                    console: None,
                    bridge: None,
                    prometheus: None,
                    metrics: None,
                };

                // Configure v4 (MQTT 3.1.1) server
                let mut v4_config = HashMap::new();
                v4_config.insert(
                    "main".to_string(),
                    rumqttd::ServerSettings {
                        name: "neotalk-broker".to_string(),
                        listen: addr,
                        tls: None,
                        next_connection_delay_ms: 1,
                        connections: rumqttd::ConnectionSettings {
                            connection_timeout_ms: connection_timeout,
                            max_payload_size: max_payload,
                            max_inflight_count: 200,
                            auth: None,
                            external_auth: None,
                            dynamic_filters,
                        },
                    },
                );
                broker_config.v4 = Some(v4_config);

                // Create and start broker
                let mut broker = rumqttd::Broker::new(broker_config);

                // The broker.start() method blocks until broker stops
                match broker.start() {
                    Ok(_) => {
                        tracing::info!("Embedded MQTT broker stopped");
                    }
                    Err(e) => {
                        tracing::error!("Embedded MQTT broker error: {}", e);
                    }
                }

                running.store(false, std::sync::atomic::Ordering::Relaxed);
            })?;

        tracing::info!("Embedded MQTT broker thread started");

        // Give the broker a moment to start
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Check if we can connect to the broker port (quick health check)
        if !is_broker_running(self.config.port) {
            return Err(EmbeddedBrokerError::Broker(
                "Broker failed to start or port not available".to_string(),
            ));
        }

        tracing::info!("Embedded broker started successfully on port {}", self.config.port);

        // Detach the thread so it runs independently
        // The thread will stop when the program exits or broker fails
        Ok(())
    }

    /// Get the broker configuration
    pub fn config(&self) -> &EmbeddedBrokerConfig {
        &self.config
    }
}

/// Helper function to check if a port is available
pub fn is_port_available(port: u16) -> bool {
    use std::net::{TcpListener, IpAddr, Ipv4Addr};

    match TcpListener::bind((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Helper function to check if MQTT broker is already running on the specified port
///
/// Returns true if a service is listening on the port (port is NOT available)
pub fn is_broker_running(port: u16) -> bool {
    !is_port_available(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EmbeddedBrokerConfig::default();
        assert_eq!(config.listen, "0.0.0.0");
        assert_eq!(config.port, 1883);
        assert_eq!(config.max_connections, 1000);
    }

    #[test]
    fn test_config_builder() {
        let config = EmbeddedBrokerConfig::new()
            .with_port(8883)
            .with_listen("127.0.0.1")
            .with_max_connections(500);

        assert_eq!(config.port, 8883);
        assert_eq!(config.listen, "127.0.0.1");
        assert_eq!(config.max_connections, 500);
    }

    #[test]
    fn test_socket_addr() {
        let config = EmbeddedBrokerConfig::new()
            .with_port(1883)
            .with_listen("0.0.0.0");

        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 1883);
        assert_eq!(addr.ip(), std::net::Ipv4Addr::new(0, 0, 0, 0));
    }

    #[test]
    fn test_broker_mode_default() {
        assert_eq!(BrokerMode::default(), BrokerMode::Embedded);
    }

    #[test]
    fn test_broker_mode_deserialize() {
        let external: BrokerMode = serde_json::from_str("\"external\"").unwrap();
        let embedded: BrokerMode = serde_json::from_str("\"embedded\"").unwrap();

        assert_eq!(external, BrokerMode::External);
        assert_eq!(embedded, BrokerMode::Embedded);
    }
}
