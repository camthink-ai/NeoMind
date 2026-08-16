//! Host API for NeoMind Extensions
//!
//! This module contains the Extension trait and capability system that extensions
//! implement to integrate with the NeoMind platform.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::OnceLock;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::RwLock;

use crate::ipc_types::{
    CommandDescriptor, ExtensionDescriptor, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, ExtensionStats, MetricDescriptor, PushOutputMessage, Result,
};

// ============================================================================
// Capability System
// ============================================================================

macro_rules! define_capabilities {
    ($($variant:ident => $const_name:ident => $name:literal => $doc:literal),* $(,)?) => {
        /// Extension capabilities for accessing NeoMind platform features.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
        pub enum ExtensionCapability {
            $(
                #[doc = $doc]
                #[serde(rename = $name)]
                $variant,
            )*
            #[serde(rename = "custom")]
            Custom(String),
        }

        impl ExtensionCapability {
            pub fn is_custom(&self) -> bool {
                matches!(self, ExtensionCapability::Custom(_))
            }

            pub fn name(&self) -> String {
                match self {
                    $(ExtensionCapability::$variant => $name.to_string(),)*
                    ExtensionCapability::Custom(name) => name.clone(),
                }
            }

            pub fn all_capabilities() -> Vec<Self> {
                vec![$(ExtensionCapability::$variant,)*]
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(ExtensionCapability::$variant),)*
                    _ => Some(ExtensionCapability::Custom(name.to_string())),
                }
            }
        }

        /// Capability name constants.
        pub mod capabilities {
            $(pub const $const_name: &str = $name;)*
        }
    };
}

define_capabilities! {
    DeviceMetricsRead => DEVICE_METRICS_READ => "device_metrics_read" => "Access to device metrics (read current state)",
    DeviceMetricsWrite => DEVICE_METRICS_WRITE => "device_metrics_write" => "Access to write device metrics (including virtual metrics)",
    DeviceControl => DEVICE_CONTROL => "device_control" => "Access to control devices (send commands)",
    StorageQuery => STORAGE_QUERY => "storage_query" => "Access to storage queries (read telemetry)",
    EventPublish => EVENT_PUBLISH => "event_publish" => "Access to publish events",
    EventSubscribe => EVENT_SUBSCRIBE => "event_subscribe" => "Access to subscribe to events",
    TelemetryHistory => TELEMETRY_HISTORY => "telemetry_history" => "Access to query device telemetry history",
    MetricsAggregate => METRICS_AGGREGATE => "metrics_aggregate" => "Access to aggregate device metrics",
    ExtensionCall => EXTENSION_CALL => "extension_call" => "Access to call other extensions",
    AgentTrigger => AGENT_TRIGGER => "agent_trigger" => "Access to trigger agents",
    ChatStream => CHAT_STREAM => "chat_stream" => "Access to streaming chat (SessionManager) with token-level events pushed via EventPush",
    ChatStreamCancel => CHAT_STREAM_CANCEL => "chat_stream_cancel" => "Cancel an in-flight ChatStream session via SessionManager.cancel_session()",
    ChatSessionOpen => CHAT_SESSION_OPEN => "chat_session_open" => "Open a persistent chat session subscription (SessionManager); AgentEvents for this session are routed to the caller via AgentStreamChunk events tagged with session_id",
    ChatSessionSend => CHAT_SESSION_SEND => "chat_session_send" => "Send a message to an open chat session; returns turn_id immediately. LLM events arrive via AgentStreamChunk (chunk.turn_id == returned turn_id)",
    ChatSessionClose => CHAT_SESSION_CLOSE => "chat_session_close" => "Close a chat session subscription and cancel any in-flight turn",
    ChatStreamCancelTurn => CHAT_STREAM_CANCEL_TURN => "chat_stream_cancel_turn" => "Cancel a specific turn within an open chat session (does not close the session)",
    RuleTrigger => RULE_TRIGGER => "rule_trigger" => "Access to trigger rules",
    DeviceTemplateRegister => DEVICE_TEMPLATE_REGISTER => "device_template_register" => "Register device type templates",
    DeviceRegister => DEVICE_REGISTER => "device_register" => "Register device instances",
    DeviceUnregister => DEVICE_UNREGISTER => "device_unregister" => "Unregister device instances",
}

impl ExtensionCapability {
    pub fn display_name(&self) -> String {
        match self {
            ExtensionCapability::DeviceMetricsRead => "Device Metrics Read".to_string(),
            ExtensionCapability::DeviceMetricsWrite => "Device Metrics Write".to_string(),
            ExtensionCapability::DeviceControl => "Device Control".to_string(),
            ExtensionCapability::StorageQuery => "Storage Query".to_string(),
            ExtensionCapability::EventPublish => "Event Publish".to_string(),
            ExtensionCapability::EventSubscribe => "Event Subscribe".to_string(),
            ExtensionCapability::TelemetryHistory => "Telemetry History".to_string(),
            ExtensionCapability::MetricsAggregate => "Metrics Aggregate".to_string(),
            ExtensionCapability::ExtensionCall => "Extension Call".to_string(),
            ExtensionCapability::AgentTrigger => "Agent Trigger".to_string(),
            ExtensionCapability::ChatStream => "Chat Stream".to_string(),
            ExtensionCapability::ChatStreamCancel => "Chat Stream Cancel".to_string(),
            ExtensionCapability::ChatSessionOpen => "Chat Session Open".to_string(),
            ExtensionCapability::ChatSessionSend => "Chat Session Send".to_string(),
            ExtensionCapability::ChatSessionClose => "Chat Session Close".to_string(),
            ExtensionCapability::ChatStreamCancelTurn => "Chat Stream Cancel Turn".to_string(),
            ExtensionCapability::RuleTrigger => "Rule Trigger".to_string(),
            ExtensionCapability::DeviceTemplateRegister => "Device Template Register".to_string(),
            ExtensionCapability::DeviceRegister => "Device Register".to_string(),
            ExtensionCapability::DeviceUnregister => "Device Unregister".to_string(),
            ExtensionCapability::Custom(name) => format!("Custom: {}", name),
        }
    }

    pub fn description(&self) -> String {
        match self {
            ExtensionCapability::DeviceMetricsRead => {
                "Read current device metrics and state".to_string()
            }
            ExtensionCapability::DeviceMetricsWrite => {
                "Write device metrics including virtual metrics".to_string()
            }
            ExtensionCapability::DeviceControl => "Send commands to control devices".to_string(),
            ExtensionCapability::StorageQuery => "Query stored telemetry data".to_string(),
            ExtensionCapability::EventPublish => "Publish events".to_string(),
            ExtensionCapability::EventSubscribe => "Subscribe to events".to_string(),
            ExtensionCapability::TelemetryHistory => {
                "Query device telemetry history data".to_string()
            }
            ExtensionCapability::MetricsAggregate => {
                "Aggregate and calculate device metrics".to_string()
            }
            ExtensionCapability::ExtensionCall => "Call other extensions".to_string(),
            ExtensionCapability::AgentTrigger => "Trigger AI agent execution".to_string(),
            ExtensionCapability::ChatStream => {
                "Invoke streaming chat session (SessionManager); AgentEvent stream pushed via event bus"
                    .to_string()
            }
            ExtensionCapability::ChatStreamCancel => {
                "Cancel an in-flight ChatStream session (SessionManager.cancel_session)"
                    .to_string()
            }
            ExtensionCapability::ChatSessionOpen => {
                "Open a persistent chat session subscription. After this call, all AgentEvents on this session are routed to the caller via AgentStreamChunk events tagged with the session_id."
                    .to_string()
            }
            ExtensionCapability::ChatSessionSend => {
                "Send a message to an open chat session; returns turn_id immediately. LLM events arrive via AgentStreamChunk (chunk.turn_id == returned turn_id)."
                    .to_string()
            }
            ExtensionCapability::ChatSessionClose => {
                "Close a chat session subscription and cancel any in-flight turn (SessionManager.cancel_session)"
                    .to_string()
            }
            ExtensionCapability::ChatStreamCancelTurn => {
                "Cancel a specific turn within an open chat session without closing the session"
                    .to_string()
            }
            ExtensionCapability::RuleTrigger => "Trigger rule engine execution".to_string(),
            ExtensionCapability::DeviceTemplateRegister => {
                "Register device type templates".to_string()
            }
            ExtensionCapability::DeviceRegister => "Register device instances".to_string(),
            ExtensionCapability::DeviceUnregister => "Unregister device instances".to_string(),
            ExtensionCapability::Custom(_) => "Custom capability".to_string(),
        }
    }

    pub fn category(&self) -> String {
        match self {
            ExtensionCapability::DeviceMetricsRead
            | ExtensionCapability::DeviceMetricsWrite
            | ExtensionCapability::DeviceControl
            | ExtensionCapability::DeviceTemplateRegister
            | ExtensionCapability::DeviceRegister
            | ExtensionCapability::DeviceUnregister => "device".to_string(),
            ExtensionCapability::StorageQuery => "storage".to_string(),
            ExtensionCapability::EventPublish | ExtensionCapability::EventSubscribe => {
                "event".to_string()
            }
            ExtensionCapability::TelemetryHistory | ExtensionCapability::MetricsAggregate => {
                "telemetry".to_string()
            }
            ExtensionCapability::ExtensionCall => "extension".to_string(),
            ExtensionCapability::AgentTrigger
            | ExtensionCapability::ChatStream
            | ExtensionCapability::ChatStreamCancel
            | ExtensionCapability::ChatSessionOpen
            | ExtensionCapability::ChatSessionSend
            | ExtensionCapability::ChatSessionClose
            | ExtensionCapability::ChatStreamCancelTurn => "agent".to_string(),
            ExtensionCapability::RuleTrigger => "rule".to_string(),
            ExtensionCapability::Custom(_) => "custom".to_string(),
        }
    }
}

/// Capability manifest for extension capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub capabilities: Vec<ExtensionCapability>,
    pub api_version: String,
    pub min_core_version: String,
    pub package_name: String,
}

// ============================================================================
// Capability Provider
// ============================================================================

/// Error type for capability operations.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("Capability not available: {0:?}")]
    NotAvailable(ExtensionCapability),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
    #[error("Provider not found for capability: {0:?}")]
    ProviderNotFound(ExtensionCapability),
}

/// Trait for capability providers.
#[async_trait]
pub trait ExtensionCapabilityProvider: Send + Sync {
    fn capability_manifest(&self) -> CapabilityManifest;

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, CapabilityError>;
}

// ============================================================================
// Extension Context
// ============================================================================

/// Extension context configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionContextConfig {
    #[serde(default)]
    pub api_base_url: String,
    pub api_version: String,
    pub extension_id: String,
    #[serde(default)]
    pub rate_limit: Option<usize>,
}

impl Default for ExtensionContextConfig {
    fn default() -> Self {
        Self {
            api_base_url: String::new(),
            api_version: "v1".to_string(),
            extension_id: String::new(),
            rate_limit: None,
        }
    }
}

/// Available capabilities registry.
#[derive(Debug, Clone, Default)]
pub struct AvailableCapabilities {
    capabilities: HashMap<ExtensionCapability, (String, String)>,
}

impl AvailableCapabilities {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
        }
    }

    pub fn register_capability(
        &mut self,
        capability: ExtensionCapability,
        package_name: String,
        api_version: String,
    ) {
        self.capabilities
            .insert(capability, (package_name, api_version));
    }

    pub fn has_capability(&self, capability: &ExtensionCapability) -> bool {
        self.capabilities.contains_key(capability)
    }

    pub fn get_provider(&self, capability: &ExtensionCapability) -> Option<(String, String)> {
        self.capabilities.get(capability).cloned()
    }

    pub fn list(&self) -> Vec<(ExtensionCapability, String, String)> {
        self.capabilities
            .iter()
            .map(|(cap, (pkg, ver))| (cap.clone(), pkg.clone(), ver.clone()))
            .collect()
    }
}

/// Extension context for capability invocation.
#[derive(Clone)]
pub struct ExtensionContext {
    config: ExtensionContextConfig,
    available_capabilities: Arc<RwLock<AvailableCapabilities>>,
    providers: Arc<RwLock<HashMap<String, Arc<dyn ExtensionCapabilityProvider>>>>,
}

impl ExtensionContext {
    pub fn new(
        config: ExtensionContextConfig,
        providers: Arc<RwLock<HashMap<String, Arc<dyn ExtensionCapabilityProvider>>>>,
    ) -> Self {
        Self {
            config,
            available_capabilities: Arc::new(RwLock::new(AvailableCapabilities::new())),
            providers,
        }
    }

    pub fn with_defaults(
        extension_id: String,
        api_base_url: String,
        providers: Arc<RwLock<HashMap<String, Arc<dyn ExtensionCapabilityProvider>>>>,
    ) -> Self {
        Self::new(
            ExtensionContextConfig {
                extension_id,
                api_base_url,
                ..Default::default()
            },
            providers,
        )
    }

    pub fn extension_id(&self) -> &str {
        &self.config.extension_id
    }

    pub async fn register_provider(
        &self,
        package_name: String,
        provider: Arc<dyn ExtensionCapabilityProvider>,
    ) {
        let manifest = provider.capability_manifest();
        let mut available = self.available_capabilities.write().await;
        for capability in &manifest.capabilities {
            available.register_capability(
                capability.clone(),
                package_name.clone(),
                manifest.api_version.clone(),
            );
        }
        let mut providers = self.providers.write().await;
        providers.insert(package_name, provider);
    }

    pub async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, CapabilityError> {
        let available = self.available_capabilities.read().await;
        let (package_name, _) = available
            .get_provider(&capability)
            .ok_or_else(|| CapabilityError::ProviderNotFound(capability.clone()))?;

        let providers = self.providers.read().await;
        let provider = providers.get(&package_name).ok_or_else(|| {
            CapabilityError::ProviderError(format!("Provider '{}' not found", package_name))
        })?;

        // Inject extension_id into params for device_register
        let params = match capability {
            ExtensionCapability::DeviceRegister => {
                let mut p = params.as_object().cloned().unwrap_or_default();
                p.insert(
                    "_extension_id".to_string(),
                    serde_json::json!(self.config.extension_id),
                );
                serde_json::Value::Object(p)
            }
            _ => params.clone(),
        };
        provider.invoke_capability(capability, &params).await
    }

    pub async fn has_capability(&self, capability: &ExtensionCapability) -> bool {
        let available = self.available_capabilities.read().await;
        available.has_capability(capability)
    }

    pub async fn list_capabilities(&self) -> Vec<(ExtensionCapability, String, String)> {
        let available = self.available_capabilities.read().await;
        available.list()
    }

    pub fn config(&self) -> &ExtensionContextConfig {
        &self.config
    }
}

// ============================================================================
// Streaming Types
// ============================================================================

/// Stream direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum StreamDirection {
    #[serde(rename = "upload")]
    #[default]
    Upload,
    #[serde(rename = "download")]
    Download,
    #[serde(rename = "bidirectional")]
    Bidirectional,
}

/// Stream mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum StreamMode {
    #[serde(rename = "stateless")]
    #[default]
    Stateless,
    #[serde(rename = "stateful")]
    Stateful,
    #[serde(rename = "push")]
    Push,
}

/// Stream data type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamDataType {
    #[serde(rename = "binary")]
    Binary,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "image")]
    Image { format: String },
    #[serde(rename = "audio")]
    Audio {
        format: String,
        sample_rate: u32,
        channels: u16,
    },
    #[serde(rename = "video")]
    Video {
        codec: String,
        width: u32,
        height: u32,
        fps: u32,
    },
    #[serde(rename = "sensor")]
    Sensor { sensor_type: String },
    #[serde(rename = "custom")]
    Custom { mime_type: String },
}

impl StreamDataType {
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

    pub fn from_mime_type(mime: &str) -> Option<Self> {
        match mime {
            "application/octet-stream" => Some(StreamDataType::Binary),
            "text/plain" => Some(StreamDataType::Text),
            "application/json" => Some(StreamDataType::Json),
            m if m.starts_with("image/") => Some(StreamDataType::Image {
                format: m.strip_prefix("image/")?.to_string(),
            }),
            m if m.starts_with("audio/") => Some(StreamDataType::Audio {
                format: m.strip_prefix("audio/")?.to_string(),
                sample_rate: 48000,
                channels: 2,
            }),
            m if m.starts_with("video/") => Some(StreamDataType::Video {
                codec: m.strip_prefix("video/")?.to_string(),
                width: 1920,
                height: 1080,
                fps: 30,
            }),
            m if m.starts_with("application/x-sensor.") => Some(StreamDataType::Sensor {
                sensor_type: m.strip_prefix("application/x-sensor.")?.to_string(),
            }),
            _ => Some(StreamDataType::Custom {
                mime_type: mime.to_string(),
            }),
        }
    }
}

/// Data chunk for streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataChunk {
    pub sequence: u64,
    pub data_type: StreamDataType,
    pub data: Vec<u8>,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub is_last: bool,
}

impl DataChunk {
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

    pub fn json(
        sequence: u64,
        value: serde_json::Value,
    ) -> std::result::Result<Self, serde_json::Error> {
        Ok(Self {
            sequence,
            data_type: StreamDataType::Json,
            data: serde_json::to_vec(&value)?,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: None,
            is_last: false,
        })
    }

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

    pub fn with_last(mut self) -> Self {
        self.is_last = true;
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Stream error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl StreamError {
    /// Create a new stream error.
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    /// Create a fatal (non-retryable) error.
    pub fn fatal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }

    /// Create a retryable error.
    pub fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: true,
        }
    }
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Stream result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_sequence: Option<u64>,
    pub output_sequence: u64,
    pub data: Vec<u8>,
    pub data_type: StreamDataType,
    pub processing_ms: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StreamError>,
}

impl StreamResult {
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

    pub fn json(
        input_sequence: Option<u64>,
        output_sequence: u64,
        value: serde_json::Value,
        processing_ms: f32,
    ) -> std::result::Result<Self, serde_json::Error> {
        Ok(Self::success(
            input_sequence,
            output_sequence,
            serde_json::to_vec(&value)?,
            StreamDataType::Json,
            processing_ms,
        ))
    }

    /// Create an error result with minimal arguments.
    pub fn error(input_sequence: Option<u64>, error: StreamError) -> Self {
        Self {
            input_sequence,
            output_sequence: 0,
            data: Vec::new(),
            data_type: StreamDataType::Binary,
            processing_ms: 0.0,
            metadata: None,
            error: Some(error),
        }
    }

    /// Create an error result with full arguments.
    pub fn error_with_details(
        input_sequence: Option<u64>,
        output_sequence: u64,
        error: StreamError,
        processing_ms: f32,
    ) -> Self {
        Self {
            input_sequence,
            output_sequence,
            data: Vec::new(),
            data_type: StreamDataType::Binary,
            processing_ms,
            metadata: None,
            error: Some(error),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Parse the data as JSON.
    pub fn as_json(&self) -> std::result::Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.data)
    }

    /// Parse the data as text.
    pub fn as_text(&self) -> std::result::Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.data)
    }
}

/// Flow control configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlowControl {
    pub supports_backpressure: bool,
    pub window_size: u32,
    pub supports_throttling: bool,
    pub max_rate: u32,
}

impl FlowControl {
    pub fn default_stream() -> Self {
        Self {
            supports_backpressure: true,
            window_size: 64 * 1024,
            supports_throttling: false,
            max_rate: 0,
        }
    }
}

impl Default for FlowControl {
    fn default() -> Self {
        Self::default_stream()
    }
}

/// Stream capability descriptor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamCapability {
    pub supported_data_types: Vec<StreamDataType>,
    pub max_chunk_size: usize,
    pub preferred_chunk_size: usize,
    pub max_concurrent_sessions: usize,
    pub mode: StreamMode,
    pub direction: StreamDirection,
    pub flow_control: FlowControl,
    /// Optional JSON schema for stream configuration
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
}

impl StreamCapability {
    /// Create a push capability with common defaults.
    pub fn push() -> Self {
        Self {
            supported_data_types: vec![StreamDataType::Binary],
            max_chunk_size: 64 * 1024,
            preferred_chunk_size: 16 * 1024,
            max_concurrent_sessions: 5,
            mode: StreamMode::Push,
            direction: StreamDirection::Download,
            flow_control: FlowControl::default(),
            config_schema: None,
        }
    }

    /// Create an upload capability with common defaults.
    pub fn upload() -> Self {
        Self {
            supported_data_types: vec![StreamDataType::Binary],
            max_chunk_size: 1024 * 1024,
            preferred_chunk_size: 64 * 1024,
            max_concurrent_sessions: 5,
            mode: StreamMode::Stateless,
            direction: StreamDirection::Upload,
            flow_control: FlowControl::default(),
            config_schema: None,
        }
    }

    /// Create a download capability with common defaults.
    pub fn download() -> Self {
        Self {
            supported_data_types: vec![StreamDataType::Binary],
            max_chunk_size: 1024 * 1024,
            preferred_chunk_size: 64 * 1024,
            max_concurrent_sessions: 5,
            mode: StreamMode::Stateless,
            direction: StreamDirection::Download,
            flow_control: FlowControl::default(),
            config_schema: None,
        }
    }

    /// Create a stateful capability with common defaults.
    pub fn stateful() -> Self {
        Self {
            supported_data_types: vec![StreamDataType::Binary],
            max_chunk_size: 1024 * 1024,
            preferred_chunk_size: 64 * 1024,
            max_concurrent_sessions: 5,
            mode: StreamMode::Stateful,
            direction: StreamDirection::Bidirectional,
            flow_control: FlowControl::default(),
            config_schema: None,
        }
    }

    /// Add a supported data type.
    pub fn with_data_type(mut self, data_type: StreamDataType) -> Self {
        self.supported_data_types.push(data_type);
        self
    }

    /// Set chunk size constraints.
    pub fn with_chunk_size(mut self, preferred: usize, max: usize) -> Self {
        self.preferred_chunk_size = preferred;
        self.max_chunk_size = max;
        self
    }
}

/// Client information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_id: String,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
}

/// Stream session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSession {
    pub id: String,
    pub extension_id: String,
    pub config: serde_json::Value,
    pub started_at: i64,
    pub last_activity: i64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub chunks_in: u64,
    pub chunks_out: u64,
    pub client_info: Option<ClientInfo>,
    pub metadata: Option<serde_json::Value>,
}

impl StreamSession {
    pub fn new(
        id: String,
        extension_id: String,
        config: serde_json::Value,
        client_info: ClientInfo,
    ) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id,
            extension_id,
            config,
            started_at: now,
            last_activity: now,
            bytes_in: 0,
            bytes_out: 0,
            chunks_in: 0,
            chunks_out: 0,
            client_info: Some(client_info),
            metadata: None,
        }
    }

    /// Get the age of this session in seconds.
    /// [unit fix] started_at is MILLIS (set in new()); this subtracted it
    /// from SECONDS — always negative, clamped to 0, so every session
    /// reported age 0. age_ms had the inverse bug (×1000 on an
    /// already-millis value, inflating age 1000×) — both fixed.
    pub fn age_secs(&self) -> i64 {
        let now_ms = chrono::Utc::now().timestamp_millis();
        (now_ms - self.started_at).max(0) / 1000
    }

    /// Get session age in milliseconds.
    pub fn age_ms(&self) -> i64 {
        let now = chrono::Utc::now().timestamp_millis();
        (now - self.started_at).max(0)
    }
}

/// Session statistics.
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
            // [unit fix] every duration consumer computes now_millis - last_activity
            // (extension_stream.rs ×3) — this was SECONDS, so durations came
            // out ≈1.75e12 ms. MILLIS everywhere for this field.
            last_activity: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl SessionStats {
    /// Record an error, incrementing the error counter.
    pub fn record_error(&mut self) {
        self.errors += 1;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }

    /// Record input data.
    pub fn record_input(&mut self, bytes: u64) {
        self.input_chunks += 1;
        self.input_bytes += bytes;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }

    /// Record output data.
    pub fn record_output(&mut self, bytes: u64) {
        self.output_chunks += 1;
        self.output_bytes += bytes;
        self.last_activity = chrono::Utc::now().timestamp_millis();
    }
}

// ============================================================================
// Event System
// ============================================================================

/// Event filter for subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventFilter {
    pub source: Option<String>,
    pub device_id: Option<String>,
    pub extension_id: Option<String>,
    pub agent_id: Option<String>,
    pub rule_id: Option<String>,
    /// Deprecated: workflow engine was removed; field is retained for serde compatibility.
    #[deprecated(
        note = "NeoMind no longer ships a workflow engine; this field is unused.",
        since = "0.8.26"
    )]
    pub workflow_id: Option<String>,
    pub expression: Option<String>,
}

impl EventFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn by_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn by_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    pub fn by_extension_id(mut self, extension_id: impl Into<String>) -> Self {
        self.extension_id = Some(extension_id.into());
        self
    }

    pub fn matches(&self, _event_type: &str, event_value: &serde_json::Value) -> bool {
        if let Some(ref source) = self.source {
            if event_value.get("source").and_then(|v| v.as_str()) != Some(source.as_str()) {
                return false;
            }
        }
        if let Some(ref device_id) = self.device_id {
            if event_value.get("device_id").and_then(|v| v.as_str()) != Some(device_id.as_str()) {
                return false;
            }
        }
        true
    }
}

/// Event subscription configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    pub event_types: Vec<String>,
    pub filters: Option<EventFilter>,
    pub max_buffer_size: usize,
    pub enabled: bool,
}

impl Default for EventSubscription {
    fn default() -> Self {
        Self {
            event_types: Vec::new(),
            filters: None,
            max_buffer_size: 1000,
            enabled: true,
        }
    }
}

impl EventSubscription {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_types(event_types: Vec<String>) -> Self {
        Self {
            event_types,
            filters: None,
            max_buffer_size: 1000,
            enabled: true,
        }
    }

    pub fn with_filters(mut self, filters: EventFilter) -> Self {
        self.filters = Some(filters);
        self
    }

    pub fn is_subscribed(&self, event_type: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self.event_types.is_empty() {
            return true;
        }
        self.event_types.iter().any(|et| et == event_type)
    }
}

// ============================================================================
// Capability Context (for FFI)
// ============================================================================

/// Native capability function types.
pub type NativeCapabilityInvokeFn = unsafe extern "C" fn(*const u8, usize) -> *mut c_char;
pub type NativeCapabilityFreeFn = unsafe extern "C" fn(*mut c_char);

#[derive(Clone, Copy)]
struct NativeCapabilityBridge {
    invoke: NativeCapabilityInvokeFn,
    free: NativeCapabilityFreeFn,
}

static NATIVE_CAPABILITY_BRIDGE: OnceLock<NativeCapabilityBridge> = OnceLock::new();

/// Set the native capability bridge for FFI.
pub fn set_native_capability_bridge(
    invoke: NativeCapabilityInvokeFn,
    free: NativeCapabilityFreeFn,
) {
    let _ = NATIVE_CAPABILITY_BRIDGE.set(NativeCapabilityBridge { invoke, free });
}

// ============================================================================
// Push Output Writer (for Push mode data flow)
// ============================================================================

/// Function pointer type for writing push output from extension → runner.
/// The runner registers this callback so the extension can push data
/// without going through the JSON FFI round-trip.
pub type PushOutputWriterFn = unsafe extern "C" fn(*const u8, usize) -> i32;

static PUSH_WRITER: OnceLock<PushOutputWriterFn> = OnceLock::new();

/// Called by the generated FFI registration function to install the
/// push-output writer callback. Returns 0 on success.
pub fn set_push_output_writer(writer: PushOutputWriterFn) {
    let _ = PUSH_WRITER.set(writer);
}

/// Send a push-output message from the extension to the host.
///
/// The extension calls this during Push mode to emit data chunks.
/// Returns `Ok(())` on success or an error if no writer is registered.
pub fn send_push_output(msg: &PushOutputMessage) -> crate::ipc_types::Result<()> {
    let writer = PUSH_WRITER.get().ok_or_else(|| {
        crate::ipc_types::ExtensionError::InternalError("push output writer not registered".into())
    })?;
    let json = serde_json::to_vec(msg).map_err(|e| {
        crate::ipc_types::ExtensionError::InternalError(format!(
            "failed to serialize PushOutputMessage: {}",
            e
        ))
    })?;
    let rc = unsafe { writer(json.as_ptr(), json.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(crate::ipc_types::ExtensionError::InternalError(format!(
            "push_output_writer returned {}",
            rc
        )))
    }
}

/// Block on an async future synchronously.
/// Only available on native targets (requires tokio runtime).
#[cfg(not(target_arch = "wasm32"))]
fn block_on_sync<F, T>(future: F) -> std::result::Result<T, CapabilityError>
where
    F: std::future::Future<Output = std::result::Result<T, CapabilityError>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => {
            let runtime = tokio::runtime::Runtime::new().map_err(|e| {
                CapabilityError::ProviderError(format!("failed to create tokio runtime: {}", e))
            })?;
            runtime.block_on(future)
        }
    }
}

/// Capability context for invoking capabilities from extensions.
/// Only available on native targets (requires tokio runtime).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct CapabilityContext {
    ctx: Arc<RwLock<Option<ExtensionContext>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for CapabilityContext {
    fn default() -> Self {
        Self {
            ctx: Arc::new(RwLock::new(None)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CapabilityContext {
    pub fn from_context(context: ExtensionContext) -> Self {
        Self {
            ctx: Arc::new(RwLock::new(Some(context))),
        }
    }

    pub fn invoke_capability(
        &self,
        capability_name: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        let capability = match ExtensionCapability::from_name(capability_name) {
            Some(capability) => capability,
            None => {
                return serde_json::json!({
                    "success": false,
                    "error": format!("Unknown capability: {}", capability_name),
                });
            }
        };

        let context = match block_on_sync(async {
            Ok::<Option<ExtensionContext>, CapabilityError>(self.ctx.read().await.clone())
        }) {
            Ok(context) => context,
            Err(error) => {
                return serde_json::json!({
                    "success": false,
                    "error": error.to_string(),
                });
            }
        };

        if let Some(context) = context {
            return match block_on_sync(async {
                context.invoke_capability(capability, params).await
            }) {
                Ok(value) => value,
                Err(error) => serde_json::json!({
                    "success": false,
                    "error": error.to_string(),
                }),
            };
        }

        let Some(bridge) = NATIVE_CAPABILITY_BRIDGE.get().copied() else {
            return serde_json::json!({
                "success": false,
                "error": "native capability bridge is not initialized",
            });
        };

        let input = match serde_json::to_vec(&serde_json::json!({
            "capability": capability_name,
            "params": params,
        })) {
            Ok(input) => input,
            Err(error) => {
                return serde_json::json!({
                    "success": false,
                    "error": format!("failed to serialize capability request: {}", error),
                });
            }
        };

        let ptr = unsafe { (bridge.invoke)(input.as_ptr(), input.len()) };
        if ptr.is_null() {
            return serde_json::json!({
                "success": false,
                "error": "native capability bridge returned null",
            });
        }

        let response = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        unsafe { (bridge.free)(ptr) };

        serde_json::from_str(&response).unwrap_or_else(|error| {
            serde_json::json!({
                "success": false,
                "error": format!("failed to parse capability bridge response: {}", error),
            })
        })
    }
}

// ============================================================================
// Extension Trait
// ============================================================================

/// Core extension trait that all NeoMind extensions must implement.
#[async_trait]
pub trait Extension: Send + Sync {
    /// Returns the extension metadata.
    fn metadata(&self) -> &ExtensionMetadata;

    /// Returns an optional descriptor with commands and metrics.
    fn descriptor(&self) -> Option<ExtensionDescriptor> {
        None
    }

    /// Initialize the extension.
    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    /// Start the extension.
    fn start(&mut self) -> Result<()> {
        Ok(())
    }

    /// Stop the extension.
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    /// Get extension status.
    fn status(&self) -> String {
        "unknown".to_string()
    }

    /// Returns metrics provided by this extension.
    fn metrics(&self) -> Vec<MetricDescriptor> {
        Vec::new()
    }

    /// Returns commands provided by this extension.
    fn commands(&self) -> Vec<CommandDescriptor> {
        Vec::new()
    }

    /// Produce current metric values.
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(Vec::new())
    }

    /// Health check.
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    /// Configure the extension.
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    /// Get extension statistics.
    fn get_stats(&self) -> ExtensionStats {
        ExtensionStats::default()
    }

    /// Get latest output for push mode.
    fn latest_output(&self) -> Option<PushOutputMessage> {
        None
    }

    /// Get stream capability.
    fn stream_capability(&self) -> Option<StreamCapability> {
        None
    }

    /// Execute a command.
    async fn execute_command(
        &self,
        command_name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let _ = args;
        Err(ExtensionError::CommandNotFound(command_name.to_string()))
    }

    /// Initialize a streaming session.
    async fn init_session(&self, _session: &StreamSession) -> Result<()> {
        Ok(())
    }

    /// Process a chunk in a session.
    async fn process_session_chunk(
        &self,
        _session_id: &str,
        _chunk: DataChunk,
    ) -> Result<StreamResult> {
        Err(ExtensionError::ExecutionFailed(
            "Session streaming not supported".to_string(),
        ))
    }

    /// Close a streaming session.
    async fn close_session(&self, _session_id: &str) -> Result<SessionStats> {
        Ok(SessionStats::default())
    }

    /// Process a single chunk (stateless).
    async fn process_chunk(&self, _chunk: DataChunk) -> Result<StreamResult> {
        Err(ExtensionError::ExecutionFailed(
            "Streaming not supported".to_string(),
        ))
    }

    /// Start push mode.
    async fn start_push(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    /// Stop push mode.
    async fn stop_push(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    /// Set output sender for push mode.
    /// Not available on WASM target (requires tokio).
    #[cfg(not(target_arch = "wasm32"))]
    fn set_output_sender(&self, _sender: Arc<tokio::sync::mpsc::Sender<PushOutputMessage>>) {}

    /// Get event subscriptions.
    fn event_subscriptions(&self) -> &[&str] {
        &[]
    }

    /// Handle an event.
    fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    /// Called when extension is unloaded.
    async fn on_unload(&self) -> Result<()> {
        Ok(())
    }

    /// Get as Any for downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ============================================================================
// Re-exports for compatibility
// ============================================================================
