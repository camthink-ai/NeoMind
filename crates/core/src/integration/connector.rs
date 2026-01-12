//! Connector trait for handling low-level communication.
//!
//! Connectors are responsible for the physical/transport layer of communication
//! with external systems. They handle:
//!
//! - Connection establishment and management
//! - Data transmission (raw bytes)
//! - Data reception (as a stream)
//! - Connection state tracking
//!
//! ## Architecture
//!
//! ```text
//! Integration                 Connector               Transport
//! ┌─────────────┐            ┌─────────────┐         ┌──────────┐
//! │             │  bytes     │             │  TCP/   │          │
//! │  Protocol   │───────────▶│   MQTT/     │─────────▶│ Network  │
//! │   Adapter   │            │  HTTP/WS    │         │          │
//! │             │◀───────────│  Connector  │◀─────────│          │
//! └─────────────┘            └─────────────┘         └──────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use edge_ai_core::integration::connector::{Connector, ConnectorError};
//!
//! struct MyConnector {
//!     connected: std::sync::Arc<std::sync::atomic::AtomicBool>,
//! }
//!
//! #[async_trait::async_trait]
//! impl Connector for MyConnector {
//!     fn connector_type(&self) -> &str {
//!         "my-protocol"
//!     }
//!
//!     fn is_connected(&self) -> bool {
//!         self.connected.load(std::sync::atomic::Ordering::Relaxed)
//!     }
//!
//!     async fn connect(&self) -> Result<(), ConnectorError> {
//!         // Establish connection
//!         self.connected.store(true, std::sync::atomic::Ordering::Relaxed);
//!         Ok(())
//!     }
//!
//!     async fn disconnect(&self) -> Result<(), ConnectorError> {
//!         self.connected.store(false, std::sync::atomic::Ordering::Relaxed);
//!         Ok(())
//!     }
//!
//!     fn subscribe(&self) -> Pin<Box<dyn Stream<Item = Vec<u8>> + Send + '_>> {
//!         // Return data stream
//!         // ...
//!     }
//!
//!     async fn send(&self, data: Vec<u8>) -> Result<(), ConnectorError> {
//!         if !self.is_connected() {
//!             return Err(ConnectorError::NotConnected);
//!         }
//!         // Send data
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Result type for connector operations.
pub type Result<T> = std::result::Result<T, ConnectorError>;

/// Connector error types.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    /// Connector is not connected.
    #[error("Connector not connected")]
    NotConnected,

    /// Connection failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Disconnection failed.
    #[error("Disconnection failed: {0}")]
    DisconnectionFailed(String),

    /// Send failed.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Receive failed.
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Timeout occurred.
    #[error("Operation timeout after {0}ms")]
    Timeout(u64),

    /// DNS resolution failed.
    #[error("DNS resolution failed: {0}")]
    DnsFailed(String),

    /// TLS/SSL error.
    #[error("TLS error: {0}")]
    TlsError(String),

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Other error.
    #[error("Connector error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Connection quality metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectionMetrics {
    /// Bytes sent.
    pub bytes_sent: u64,

    /// Bytes received.
    pub bytes_received: u64,

    /// Number of reconnects.
    pub reconnect_count: u64,

    /// Last activity timestamp.
    pub last_activity: i64,

    /// Connection latency in milliseconds.
    pub latency_ms: Option<u64>,

    /// Packet loss count.
    pub packet_loss: u64,
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            reconnect_count: 0,
            last_activity: chrono::Utc::now().timestamp(),
            latency_ms: None,
            packet_loss: 0,
        }
    }
}

/// Connector configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectorConfig {
    /// Host address.
    pub host: String,

    /// Port number.
    pub port: u16,

    /// Connection timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Whether to use TLS/SSL.
    #[serde(default)]
    pub use_tls: bool,

    /// Keep-alive interval in seconds.
    #[serde(default = "default_keep_alive")]
    pub keep_alive_secs: u64,

    /// Maximum reconnect attempts.
    #[serde(default = "default_max_reconnect")]
    pub max_reconnect: u32,

    /// Reconnect delay in milliseconds.
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_ms: u64,

    /// Additional configuration parameters.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

fn default_timeout() -> u64 {
    30000 // 30 seconds
}

fn default_keep_alive() -> u64 {
    60 // 1 minute
}

fn default_max_reconnect() -> u32 {
    5
}

fn default_reconnect_delay() -> u64 {
    5000 // 5 seconds
}

impl ConnectorConfig {
    /// Create a new connector configuration.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            timeout_ms: default_timeout(),
            use_tls: false,
            keep_alive_secs: default_keep_alive(),
            max_reconnect: default_max_reconnect(),
            reconnect_delay_ms: default_reconnect_delay(),
            extra: serde_json::json!({}),
        }
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set whether to use TLS.
    pub fn with_tls(mut self, use_tls: bool) -> Self {
        self.use_tls = use_tls;
        self
    }

    /// Set the keep-alive interval.
    pub fn with_keep_alive(mut self, keep_alive_secs: u64) -> Self {
        self.keep_alive_secs = keep_alive_secs;
        self
    }

    /// Set the maximum reconnect attempts.
    pub fn with_max_reconnect(mut self, max_reconnect: u32) -> Self {
        self.max_reconnect = max_reconnect;
        self
    }

    /// Get the full address (host:port).
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Connector trait - handles low-level communication.
///
/// Connectors are responsible for the transport layer only. They don't
/// understand protocol semantics - they just send and receive raw bytes.
#[async_trait]
pub trait Connector: Send + Sync {
    /// Get the connector type identifier.
    fn connector_type(&self) -> &str;

    /// Check if the connector is currently connected.
    fn is_connected(&self) -> bool;

    /// Get connection metrics if available.
    fn metrics(&self) -> Option<ConnectionMetrics> {
        None
    }

    /// Establish the connection.
    async fn connect(&self) -> Result<()>;

    /// Disconnect and cleanup resources.
    async fn disconnect(&self) -> Result<()>;

    /// Subscribe to incoming data stream.
    ///
    /// Returns a stream of raw byte payloads received from the external system.
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = Vec<u8>> + Send + '_>>;

    /// Send raw data to the external system.
    async fn send(&self, data: Vec<u8>) -> Result<()>;

    /// Send data with a timeout.
    async fn send_timeout(&self, data: Vec<u8>, timeout_ms: u64) -> Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.send(data),
        )
        .await
        .map_err(|_| ConnectorError::Timeout(timeout_ms))?
    }

    /// Check connection health.
    async fn health_check(&self) -> Result<bool> {
        Ok(self.is_connected())
    }
}

/// Dynamic connector wrapper for trait objects.
pub type DynConnector = Arc<dyn Connector>;

/// Base connector implementation with common functionality.
///
/// This struct provides a foundation for connector implementations
/// with connection state tracking and metrics.
pub struct BaseConnector {
    /// Connector type.
    connector_type: String,

    /// Connection state.
    connected: Arc<AtomicBool>,

    /// Connection metrics.
    metrics: Arc<parking_lot::Mutex<ConnectionMetrics>>,
}

impl BaseConnector {
    /// Create a new base connector.
    pub fn new(connector_type: impl Into<String>) -> Self {
        Self {
            connector_type: connector_type.into(),
            connected: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(parking_lot::Mutex::new(ConnectionMetrics::default())),
        }
    }

    /// Get the connector type.
    pub fn connector_type(&self) -> &str {
        &self.connector_type
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Set connection state.
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
        self.metrics.lock().last_activity = chrono::Utc::now().timestamp();
    }

    /// Get the connection metrics.
    pub fn metrics(&self) -> ConnectionMetrics {
        self.metrics.lock().clone()
    }

    /// Record sent bytes.
    pub fn record_sent(&self, bytes: u64) {
        let mut metrics = self.metrics.lock();
        metrics.bytes_sent += bytes;
        metrics.last_activity = chrono::Utc::now().timestamp();
    }

    /// Record received bytes.
    pub fn record_received(&self, bytes: u64) {
        let mut metrics = self.metrics.lock();
        metrics.bytes_received += bytes;
        metrics.last_activity = chrono::Utc::now().timestamp();
    }

    /// Increment reconnect count.
    pub fn increment_reconnect(&self) {
        self.metrics.lock().reconnect_count += 1;
    }

    /// Get a clone of the connection state Arc.
    pub fn connected_state(&self) -> Arc<AtomicBool> {
        self.connected.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_config() {
        let config = ConnectorConfig::new("localhost", 1883)
            .with_timeout(5000)
            .with_tls(true)
            .with_keep_alive(30);

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.timeout_ms, 5000);
        assert!(config.use_tls);
        assert_eq!(config.keep_alive_secs, 30);
        assert_eq!(config.address(), "localhost:1883");
    }

    #[test]
    fn test_base_connector() {
        let connector = BaseConnector::new("test");

        assert_eq!(connector.connector_type(), "test");
        assert!(!connector.is_connected());

        connector.set_connected(true);
        assert!(connector.is_connected());

        connector.record_sent(100);
        connector.record_received(200);

        let metrics = connector.metrics();
        assert_eq!(metrics.bytes_sent, 100);
        assert_eq!(metrics.bytes_received, 200);
    }

    #[test]
    fn test_connection_metrics_default() {
        let metrics = ConnectionMetrics::default();
        assert_eq!(metrics.bytes_sent, 0);
        assert_eq!(metrics.bytes_received, 0);
        assert_eq!(metrics.reconnect_count, 0);
    }

    #[test]
    fn test_reconnect_count_increment() {
        let connector = BaseConnector::new("test");
        assert_eq!(connector.metrics().reconnect_count, 0);

        connector.increment_reconnect();
        assert_eq!(connector.metrics().reconnect_count, 1);

        connector.increment_reconnect();
        connector.increment_reconnect();
        assert_eq!(connector.metrics().reconnect_count, 3);
    }
}
