//! Generic streaming support for extensions.
//!
//! This module provides a universal streaming interface that supports various data types:
//! - Image analysis (JPEG/PNG frames)
//! - Video streams (H264/H265)
//! - Audio processing (PCM/MP3/AAC)
//! - Sensor data (temperature, humidity, etc.)
//! - Log streams
//! - File transfers
//! - Custom data types

use serde::{Deserialize, Serialize};

/// Stream direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamDirection {
    /// Upload only (client -> extension)
    #[serde(rename = "upload")]
    Upload,
    /// Download only (extension -> client)
    #[serde(rename = "download")]
    Download,
    /// Bidirectional
    #[serde(rename = "bidirectional")]
    Bidirectional,
}

/// Stream mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamMode {
    /// Stateless - each request is processed independently
    #[serde(rename = "stateless")]
    Stateless,
    /// Stateful - maintains session context
    #[serde(rename = "stateful")]
    Stateful,
    /// Push - extension proactively pushes data
    #[serde(rename = "push")]
    Push,
}

/// Data type for streaming
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamDataType {
    /// Raw binary data
    #[serde(rename = "binary")]
    Binary,
    /// Text data
    #[serde(rename = "text")]
    Text,
    /// JSON data
    #[serde(rename = "json")]
    Json,
    /// Image data
    #[serde(rename = "image")]
    Image { format: String },
    /// Audio data
    #[serde(rename = "audio")]
    Audio {
        format: String,
        sample_rate: u32,
        channels: u16,
    },
    /// Video data
    #[serde(rename = "video")]
    Video {
        codec: String,
        width: u32,
        height: u32,
        fps: u32,
    },
    /// Sensor data
    #[serde(rename = "sensor")]
    Sensor { sensor_type: String },
    /// Custom data type with MIME type
    #[serde(rename = "custom")]
    Custom { mime_type: String },
}

impl StreamDataType {
    /// Get the MIME type for this data type
    pub fn mime_type(&self) -> String {
        match self {
            StreamDataType::Binary => "application/octet-stream".to_string(),
            StreamDataType::Text => "text/plain".to_string(),
            StreamDataType::Json => "application/json".to_string(),
            StreamDataType::Image { format } => match format.to_lowercase().as_str() {
                "jpeg" | "jpg" => "image/jpeg".to_string(),
                "png" => "image/png".to_string(),
                "gif" => "image/gif".to_string(),
                "webp" => "image/webp".to_string(),
                "bmp" => "image/bmp".to_string(),
                _ => format!("image/{}", format),
            },
            StreamDataType::Audio { format, .. } => match format.to_lowercase().as_str() {
                "pcm" => "audio/pcm".to_string(),
                "mp3" => "audio/mpeg".to_string(),
                "aac" => "audio/aac".to_string(),
                "wav" => "audio/wav".to_string(),
                "ogg" => "audio/ogg".to_string(),
                _ => format!("audio/{}", format),
            },
            StreamDataType::Video { codec, .. } => match codec.to_lowercase().as_str() {
                "h264" | "h.264" => "video/h264".to_string(),
                "h265" | "h.265" | "hevc" => "video/h265".to_string(),
                "vp8" => "video/vp8".to_string(),
                "vp9" => "video/vp9".to_string(),
                "av1" => "video/av1".to_string(),
                _ => format!("video/{}", codec),
            },
            StreamDataType::Sensor { sensor_type } => {
                format!("application/x-sensor.{}", sensor_type)
            }
            StreamDataType::Custom { mime_type } => mime_type.clone(),
        }
    }

    /// Parse from MIME type string
    pub fn from_mime_type(mime: &str) -> Option<Self> {
        match mime {
            "application/octet-stream" => Some(StreamDataType::Binary),
            "text/plain" => Some(StreamDataType::Text),
            "application/json" => Some(StreamDataType::Json),
            m if m.starts_with("image/") => {
                let format = m.strip_prefix("image/")?.to_string();
                Some(StreamDataType::Image { format })
            }
            m if m.starts_with("audio/") => {
                let format = m.strip_prefix("audio/")?.to_string();
                Some(StreamDataType::Audio {
                    format,
                    sample_rate: 48000,
                    channels: 2,
                })
            }
            m if m.starts_with("video/") => {
                let codec = m.strip_prefix("video/")?.to_string();
                Some(StreamDataType::Video {
                    codec,
                    width: 1920,
                    height: 1080,
                    fps: 30,
                })
            }
            m if m.starts_with("application/x-sensor.") => {
                let sensor_type = m.strip_prefix("application/x-sensor.")?.to_string();
                Some(StreamDataType::Sensor { sensor_type })
            }
            _ => Some(StreamDataType::Custom {
                mime_type: mime.to_string(),
            }),
        }
    }
}

/// Data chunk for streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataChunk {
    /// Chunk sequence number
    pub sequence: u64,
    /// Data type
    pub data_type: StreamDataType,
    /// Data content
    pub data: Vec<u8>,
    /// Timestamp (milliseconds)
    pub timestamp: i64,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Is this the last chunk?
    pub is_last: bool,
}

impl DataChunk {
    /// Create a new binary data chunk
    pub fn binary(sequence: u64, data: Vec<u8>) -> Self {
        Self {
            sequence,
            data_type: StreamDataType::Binary,
            data,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: None,
            is_last: false,
        }
    }

    /// Create a new text data chunk
    pub fn text(sequence: u64, text: String) -> Self {
        Self {
            sequence,
            data_type: StreamDataType::Text,
            data: text.into_bytes(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: None,
            is_last: false,
        }
    }

    /// Create a new JSON data chunk
    pub fn json(sequence: u64, value: serde_json::Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            sequence,
            data_type: StreamDataType::Json,
            data: serde_json::to_vec(&value)?,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: None,
            is_last: false,
        })
    }

    /// Create a new image data chunk
    pub fn image(sequence: u64, data: Vec<u8>, format: String) -> Self {
        Self {
            sequence,
            data_type: StreamDataType::Image { format },
            data,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: None,
            is_last: false,
        }
    }

    /// Mark as the last chunk
    pub fn with_last(mut self) -> Self {
        self.is_last = true;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Stream processing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResult {
    /// Corresponding input sequence number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_sequence: Option<u64>,
    /// Output sequence number
    pub output_sequence: u64,
    /// Result data
    pub data: Vec<u8>,
    /// Result data type
    pub data_type: StreamDataType,
    /// Processing time (milliseconds)
    pub processing_ms: f32,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Error information (if processing failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StreamError>,
}

impl StreamResult {
    /// Create a successful result
    pub fn success(
        input_sequence: Option<u64>,
        output_sequence: u64,
        data: Vec<u8>,
        data_type: StreamDataType,
        processing_ms: f32,
    ) -> Self {
        Self {
            input_sequence,
            output_sequence,
            data,
            data_type,
            processing_ms,
            metadata: None,
            error: None,
        }
    }

    /// Create a successful JSON result
    pub fn json(
        input_sequence: Option<u64>,
        output_sequence: u64,
        value: serde_json::Value,
        processing_ms: f32,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            input_sequence,
            output_sequence,
            data: serde_json::to_vec(&value)?,
            data_type: StreamDataType::Json,
            processing_ms,
            metadata: None,
            error: None,
        })
    }

    /// Create an error result
    pub fn error(input_sequence: Option<u64>, error: StreamError) -> Self {
        Self {
            input_sequence,
            output_sequence: 0,
            data: vec![],
            data_type: StreamDataType::Binary,
            processing_ms: 0.0,
            metadata: None,
            error: Some(error),
        }
    }

    /// Parse result as JSON
    pub fn as_json(&self) -> Option<serde_json::Value> {
        if self.data_type == StreamDataType::Json {
            serde_json::from_slice(&self.data).ok()
        } else {
            None
        }
    }

    /// Parse result as text
    pub fn as_text(&self) -> Option<String> {
        if self.data_type == StreamDataType::Text {
            String::from_utf8(self.data.clone()).ok()
        } else {
            None
        }
    }

    /// Add metadata to result
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Stream error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl StreamError {
    /// Create a new stream error
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    /// Create a fatal error
    pub fn fatal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message, false)
    }

    /// Create a retryable error
    pub fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message, true)
    }
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for StreamError {}

/// Stream capability description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCapability {
    /// Stream direction
    pub direction: StreamDirection,
    /// Stream mode
    pub mode: StreamMode,
    /// Supported data types
    pub supported_data_types: Vec<StreamDataType>,
    /// Maximum chunk size in bytes
    pub max_chunk_size: usize,
    /// Preferred chunk size in bytes
    pub preferred_chunk_size: usize,
    /// Maximum concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Flow control settings
    pub flow_control: FlowControl,
    /// Extension-specific configuration schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
}

impl Default for StreamCapability {
    fn default() -> Self {
        Self {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateless,
            supported_data_types: vec![StreamDataType::Binary],
            max_chunk_size: 4 * 1024 * 1024, // 4MB
            preferred_chunk_size: 64 * 1024,  // 64KB
            max_concurrent_sessions: 10,
            flow_control: FlowControl::default(),
            config_schema: None,
        }
    }
}

impl StreamCapability {
    /// Create a basic upload capability
    pub fn upload() -> Self {
        Self {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateless,
            supported_data_types: Vec::new(),
            ..Default::default()
        }
    }

    /// Create a basic download capability
    pub fn download() -> Self {
        Self {
            direction: StreamDirection::Download,
            mode: StreamMode::Stateless,
            supported_data_types: Vec::new(),
            ..Default::default()
        }
    }

    /// Create a stateful session capability
    pub fn stateful() -> Self {
        Self {
            direction: StreamDirection::Bidirectional,
            mode: StreamMode::Stateful,
            supported_data_types: Vec::new(),
            ..Default::default()
        }
    }

    /// Create a push capability
    pub fn push() -> Self {
        Self {
            direction: StreamDirection::Download,
            mode: StreamMode::Push,
            supported_data_types: Vec::new(),
            ..Default::default()
        }
    }

    /// Add supported data type
    pub fn with_data_type(mut self, data_type: StreamDataType) -> Self {
        self.supported_data_types.push(data_type);
        self
    }

    /// Set chunk size limits
    pub fn with_chunk_size(mut self, preferred: usize, max: usize) -> Self {
        self.preferred_chunk_size = preferred;
        self.max_chunk_size = max;
        self
    }
}

/// Flow control settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowControl {
    /// Supports backpressure
    pub supports_backpressure: bool,
    /// Window size (unacknowledged chunks)
    pub window_size: u32,
    /// Supports throttling
    pub supports_throttling: bool,
    /// Maximum rate (chunks per second, 0 = unlimited)
    pub max_rate: u32,
}

impl Default for FlowControl {
    fn default() -> Self {
        Self {
            supports_backpressure: false,
            window_size: 10,
            supports_throttling: false,
            max_rate: 0,
        }
    }
}

/// Stream session
#[derive(Debug, Clone)]
pub struct StreamSession {
    /// Session ID
    pub id: String,
    /// Extension ID
    pub extension_id: String,
    /// Creation time
    pub created_at: i64,
    /// Configuration
    pub config: serde_json::Value,
    /// Client info
    pub client_info: ClientInfo,
}

/// Client information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub input_chunks: u64,
    pub output_chunks: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub errors: u64,
    pub last_activity: i64,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            input_chunks: 0,
            output_chunks: 0,
            input_bytes: 0,
            output_bytes: 0,
            errors: 0,
            last_activity: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl SessionStats {
    /// Update with new input
    pub fn record_input(&mut self, bytes: usize) {
        self.input_chunks += 1;
        self.input_bytes += bytes as u64;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }

    /// Update with new output
    pub fn record_output(&mut self, bytes: usize) {
        self.output_chunks += 1;
        self.output_bytes += bytes as u64;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }

    /// Record an error
    pub fn record_error(&mut self) {
        self.errors += 1;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }
}

impl StreamSession {
    /// Create a new stream session
    pub fn new(
        id: String,
        extension_id: String,
        config: serde_json::Value,
        client_info: ClientInfo,
    ) -> Self {
        Self {
            id,
            extension_id,
            created_at: chrono::Utc::now().timestamp_millis(),
            config,
            client_info,
        }
    }

    /// Get session age in milliseconds
    pub fn age_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis() - self.created_at
    }

    /// Get session age in seconds
    pub fn age_secs(&self) -> i64 {
        self.age_ms() / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_data_type_mime() {
        assert_eq!(StreamDataType::Binary.mime_type(), "application/octet-stream");
        assert_eq!(StreamDataType::Text.mime_type(), "text/plain");
        assert_eq!(
            StreamDataType::Image { format: "jpeg".into() }.mime_type(),
            "image/jpeg"
        );
        assert_eq!(
            StreamDataType::Audio {
                format: "mp3".into(),
                sample_rate: 48000,
                channels: 2
            }
            .mime_type(),
            "audio/mpeg"
        );
    }

    #[test]
    fn test_stream_data_type_from_mime() {
        assert_eq!(
            StreamDataType::from_mime_type("application/octet-stream"),
            Some(StreamDataType::Binary)
        );
        assert_eq!(
            StreamDataType::from_mime_type("image/png"),
            Some(StreamDataType::Image { format: "png".into() })
        );
        assert_eq!(
            StreamDataType::from_mime_type("audio/mp3"),
            Some(StreamDataType::Audio {
                format: "mp3".into(),
                sample_rate: 48000,
                channels: 2
            })
        );
    }

    #[test]
    fn test_data_chunk_creation() {
        let chunk = DataChunk::binary(0, vec![1, 2, 3]);
        assert_eq!(chunk.sequence, 0);
        assert_eq!(chunk.data, vec![1, 2, 3]);
        assert!(!chunk.is_last);

        let chunk = chunk.with_last();
        assert!(chunk.is_last);
    }

    #[test]
    fn test_data_chunk_json() {
        let value = serde_json::json!({"test": "data"});
        let chunk = DataChunk::json(0, value.clone()).unwrap();
        assert_eq!(chunk.data_type, StreamDataType::Json);

        let parsed: serde_json::Value = serde_json::from_slice(&chunk.data).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_stream_result_json() {
        let value = serde_json::json!({"result": 42});
        let result = StreamResult::json(Some(0), 1, value.clone(), 10.0).unwrap();

        assert_eq!(result.input_sequence, Some(0));
        assert_eq!(result.output_sequence, 1);
        assert_eq!(result.processing_ms, 10.0);

        let parsed = result.as_json().unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_session_stats() {
        let mut stats = SessionStats::default();
        stats.record_input(100);
        stats.record_output(200);

        assert_eq!(stats.input_chunks, 1);
        assert_eq!(stats.input_bytes, 100);
        assert_eq!(stats.output_chunks, 1);
        assert_eq!(stats.output_bytes, 200);
    }

    #[test]
    fn test_stream_error() {
        let err = StreamError::fatal("TEST_ERROR", "Something went wrong");
        assert_eq!(err.code, "TEST_ERROR");
        assert_eq!(err.message, "Something went wrong");
        assert!(!err.retryable);

        let err = StreamError::retryable("TEMP_ERROR", "Try again");
        assert!(err.retryable);
    }

    #[test]
    fn test_stream_capability_builder() {
        let cap = StreamCapability::upload()
            .with_data_type(StreamDataType::Image { format: "jpeg".into() })
            .with_data_type(StreamDataType::Image { format: "png".into() })
            .with_chunk_size(1024, 10 * 1024);

        assert_eq!(cap.direction, StreamDirection::Upload);
        assert_eq!(cap.mode, StreamMode::Stateless);
        assert_eq!(cap.supported_data_types.len(), 2);
        assert_eq!(cap.preferred_chunk_size, 1024);
        assert_eq!(cap.max_chunk_size, 10 * 1024);
    }

    #[test]
    fn test_stream_session_age() {
        let session = StreamSession::new(
            "test-session".to_string(),
            "test-ext".to_string(),
            serde_json::json!({}),
            ClientInfo {
                client_id: "client-1".to_string(),
                ip_addr: None,
                user_agent: None,
            },
        );

        // Age should be very small
        assert!(session.age_ms() < 100);
        assert!(session.age_secs() == 0);
    }
}
