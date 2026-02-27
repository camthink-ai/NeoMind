//! IPC (Inter-Process Communication) protocol for isolated extensions
//!
//! This module defines the message protocol used for communication between
//! the main NeoMind process and isolated extension processes.

use serde::{Deserialize, Serialize};

/// IPC message sent from host to extension process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    /// Initialize extension with config
    Init {
        /// Configuration JSON
        config: serde_json::Value,
    },

    /// Execute a command
    ExecuteCommand {
        /// Command name
        command: String,
        /// Command arguments
        args: serde_json::Value,
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get metrics
    ProduceMetrics {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Health check
    HealthCheck {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get metadata
    GetMetadata {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Graceful shutdown
    Shutdown,

    /// Ping (keep-alive)
    Ping {
        /// Timestamp
        timestamp: i64,
    },
}

/// IPC response sent from extension process to host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    /// Extension is ready
    Ready {
        /// Extension metadata
        metadata: super::super::system::ExtensionMetadata,
    },

    /// Command execution success
    Success {
        /// Request ID
        request_id: u64,
        /// Result data
        data: serde_json::Value,
    },

    /// Error response
    Error {
        /// Request ID (0 if not applicable)
        request_id: u64,
        /// Error message
        error: String,
        /// Error kind
        kind: ErrorKind,
    },

    /// Metrics response
    Metrics {
        /// Request ID
        request_id: u64,
        /// Metric values
        metrics: Vec<super::super::system::ExtensionMetricValue>,
    },

    /// Health check response
    Health {
        /// Request ID
        request_id: u64,
        /// Is healthy
        healthy: bool,
    },

    /// Metadata response
    Metadata {
        /// Request ID
        request_id: u64,
        /// Extension metadata
        metadata: super::super::system::ExtensionMetadata,
    },

    /// Pong response
    Pong {
        /// Original timestamp
        timestamp: i64,
    },
}

/// Error kind classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// Command not found
    CommandNotFound,
    /// Invalid arguments
    InvalidArguments,
    /// Execution failed
    ExecutionFailed,
    /// Timeout
    Timeout,
    /// Not found
    NotFound,
    /// Invalid format
    InvalidFormat,
    /// Not initialized
    NotInitialized,
    /// Internal error
    Internal,
    /// Security error
    Security,
}

impl IpcMessage {
    /// Serialize message to JSON bytes
    pub fn to_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    /// Deserialize message from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

impl IpcResponse {
    /// Serialize response to JSON bytes
    pub fn to_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    /// Deserialize response from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }

    /// Check if this response is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Get request ID if applicable
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Ready { .. } => None,
            Self::Success { request_id, .. } => Some(*request_id),
            Self::Error { request_id, .. } => Some(*request_id),
            Self::Metrics { request_id, .. } => Some(*request_id),
            Self::Health { request_id, .. } => Some(*request_id),
            Self::Metadata { request_id, .. } => Some(*request_id),
            Self::Pong { .. } => None,
        }
    }
}

/// Frame format for IPC communication
///
/// Frame format:
/// - 4 bytes: length (little-endian u32)
/// - N bytes: JSON payload
#[derive(Debug, Clone)]
pub struct IpcFrame {
    /// Payload bytes
    pub payload: Vec<u8>,
}

impl IpcFrame {
    /// Create a new frame from payload
    pub fn new(payload: Vec<u8>) -> Self {
        Self { payload }
    }

    /// Encode frame to bytes (length prefix + payload)
    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len() as u32;
        let mut bytes = Vec::with_capacity(4 + self.payload.len());
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Decode frame from bytes
    /// Returns (frame, remaining_bytes) or error message
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), &'static str> {
        if bytes.len() < 4 {
            return Err("Not enough bytes for length prefix");
        }

        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

        if bytes.len() < 4 + len {
            return Err("Not enough bytes for payload");
        }

        let payload = bytes[4..4 + len].to_vec();
        Ok((Self { payload }, 4 + len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = IpcMessage::ExecuteCommand {
            command: "test".to_string(),
            args: serde_json::json!({"arg": 1}),
            request_id: 1,
        };

        let bytes = msg.to_bytes().unwrap();
        let decoded = IpcMessage::from_bytes(&bytes).unwrap();

        match decoded {
            IpcMessage::ExecuteCommand { command, args, request_id } => {
                assert_eq!(command, "test");
                assert_eq!(request_id, 1);
                assert_eq!(args, serde_json::json!({"arg": 1}));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_frame_encoding() {
        let payload = b"hello world";
        let frame = IpcFrame::new(payload.to_vec());
        let encoded = frame.encode();

        assert_eq!(encoded.len(), 4 + payload.len());
        assert_eq!(&encoded[0..4], &(payload.len() as u32).to_le_bytes());
        assert_eq!(&encoded[4..], payload);

        let (decoded, consumed) = IpcFrame::decode(&encoded).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded.payload, payload);
    }
}
