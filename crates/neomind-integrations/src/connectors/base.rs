//! Base connector implementations using streams.
//!
//! Provides a foundation for building stream-based connectors.

use futures::{channel::mpsc, SinkExt, Stream, StreamExt};
use neomind_core::integration::connector::{
    BaseConnector as CoreBaseConnector, Connector, ConnectorError, Result,
};
use parking_lot::Mutex;
use std::pin::Pin;
use std::sync::Arc;

/// Configuration for stream-based connectors.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamConnectorConfig {
    /// Host address.
    pub host: String,

    /// Port number.
    pub port: u16,

    /// Buffer size for the stream.
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    /// Connection timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_buffer_size() -> usize {
    1000
}

fn default_timeout() -> u64 {
    30000
}

impl StreamConnectorConfig {
    /// Create a new stream connector configuration.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            buffer_size: default_buffer_size(),
            timeout_ms: default_timeout(),
        }
    }

    /// Get the full address.
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// A stream-based connector using channels.
///
/// This connector provides a simple implementation for testing
/// and can be used as a base for other connectors.
pub struct StreamConnector {
    /// Connector type (for identification).
    connector_type: String,

    /// Base connector functionality.
    base: CoreBaseConnector,

    /// Sender for outgoing data.
    sender: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,

    /// Receiver for incoming data.
    receiver: Arc<Mutex<Option<mpsc::Receiver<Vec<u8>>>>>,

    /// Configuration.
    config: StreamConnectorConfig,
}

impl StreamConnector {
    /// Create a new stream connector.
    pub fn new(connector_type: impl Into<String>, config: StreamConnectorConfig) -> Self {
        let (sender, receiver) = mpsc::channel(config.buffer_size);
        let connector_type = connector_type.into();

        Self {
            base: CoreBaseConnector::new(connector_type.clone()),
            connector_type,
            sender: Arc::new(Mutex::new(Some(sender))),
            receiver: Arc::new(Mutex::new(Some(receiver))),
            config,
        }
    }

    /// Get the connector type.
    pub fn connector_type(&self) -> &str {
        &self.connector_type
    }

    /// Get the configuration.
    pub fn config(&self) -> &StreamConnectorConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl Connector for StreamConnector {
    fn connector_type(&self) -> &str {
        &self.connector_type
    }

    fn is_connected(&self) -> bool {
        self.base.is_connected()
    }

    fn metrics(&self) -> Option<neomind_core::integration::connector::ConnectionMetrics> {
        Some(self.base.metrics())
    }

    async fn connect(&self) -> Result<()> {
        self.base.set_connected(true);
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        self.base.set_connected(false);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = Vec<u8>> + Send + '_>> {
        let receiver = self.receiver.lock().take();
        if let Some(rx) = receiver {
            Box::pin(rx.map(|data| data))
        } else {
            Box::pin(futures::stream::empty())
        }
    }

    async fn send(&self, data: Vec<u8>) -> Result<()> {
        self.base.record_sent(data.len() as u64);

        // Clone the sender to avoid holding the lock across await
        let sender = self.sender.lock().clone();
        if let Some(mut tx) = sender {
            tx.send(data)
                .await
                .map_err(|e| ConnectorError::SendFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(ConnectorError::NotConnected)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config() {
        let config = StreamConnectorConfig::new("localhost", 8080);
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8080);
        assert_eq!(config.address(), "localhost:8080");
        assert_eq!(config.buffer_size, 1000);
        assert_eq!(config.timeout_ms, 30000);
    }

    #[tokio::test]
    async fn test_stream_connector() {
        let config = StreamConnectorConfig::new("localhost", 8080);
        let connector = StreamConnector::new("test", config);

        assert_eq!(connector.connector_type(), "test");
        assert!(!connector.is_connected());

        connector.connect().await.unwrap();
        assert!(connector.is_connected());

        connector.disconnect().await.unwrap();
        assert!(!connector.is_connected());
    }

    #[tokio::test]
    async fn test_stream_send_receive() {
        let config = StreamConnectorConfig::new("localhost", 8080);
        let connector = StreamConnector::new("test", config);

        connector.connect().await.unwrap();

        // Send data
        connector.send(b"hello".to_vec()).await.unwrap();

        // Receive data
        let mut stream = connector.subscribe();
        let data = stream.next().await;
        assert_eq!(data, Some(b"hello".to_vec()));

        connector.disconnect().await.unwrap();
    }
}
