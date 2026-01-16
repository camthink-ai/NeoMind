//! Device Service - Unified service layer for device operations
//!
//! This service provides a high-level API for:
//! - Device registration and management
//! - Command sending (automatically uses templates)
//! - Data querying
//! - Integration with adapters and telemetry storage

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::adapter::{ConnectionStatus, DeviceAdapter};
use super::mdl::{Command as MdlCommand, DeviceError, MetricValue};
use super::registry::{DeviceConfig, DeviceRegistry, DeviceTypeTemplate};
use super::telemetry::TimeSeriesStorage;
use edge_ai_core::EventBus;
use std::sync::atomic::{AtomicU64, Ordering};

// Import storage types for command history persistence
use edge_ai_storage::device_registry::{
    CommandHistoryRecord as StorageCommandRecord,
    CommandStatus as StorageCommandStatus,
    DeviceRegistryStore,
};

/// Command history record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandHistoryRecord {
    /// Unique command ID
    pub command_id: String,
    /// Device ID
    pub device_id: String,
    /// Command name
    pub command_name: String,
    /// Command parameters
    pub parameters: HashMap<String, serde_json::Value>,
    /// Command status
    pub status: CommandStatus,
    /// Result message (if available)
    pub result: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Timestamp when command was created
    pub created_at: i64,
    /// Timestamp when command completed (if applicable)
    pub completed_at: Option<i64>,
}

/// Command execution status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum CommandStatus {
    /// Command is pending execution
    Pending,
    /// Command is currently executing
    Executing,
    /// Command completed successfully
    Success,
    /// Command failed
    Failed,
    /// Command timed out
    Timeout,
}

impl CommandStatus {
    /// Check if command is a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Timeout)
    }

    /// Convert to storage command status
    fn to_storage(&self) -> StorageCommandStatus {
        match self {
            Self::Pending => StorageCommandStatus::Pending,
            Self::Executing => StorageCommandStatus::Sent,
            Self::Success => StorageCommandStatus::Completed,
            Self::Failed => StorageCommandStatus::Failed,
            Self::Timeout => StorageCommandStatus::Timeout,
        }
    }

    /// Convert from storage command status
    fn from_storage(status: StorageCommandStatus) -> Self {
        match status {
            StorageCommandStatus::Pending => Self::Pending,
            StorageCommandStatus::Sent => Self::Executing,
            StorageCommandStatus::Completed => Self::Success,
            StorageCommandStatus::Failed => Self::Failed,
            StorageCommandStatus::Timeout => Self::Timeout,
        }
    }
}

/// Convert local command record to storage format
fn command_to_storage(record: &CommandHistoryRecord) -> StorageCommandRecord {
    StorageCommandRecord {
        command_id: record.command_id.clone(),
        device_id: record.device_id.clone(),
        command_name: record.command_name.clone(),
        parameters: record.parameters.clone(),
        status: record.status.to_storage(),
        result: record.result.clone(),
        error: record.error.clone(),
        created_at: record.created_at,
        completed_at: record.completed_at,
    }
}

/// Convert storage command record to local format
fn command_from_storage(record: StorageCommandRecord) -> CommandHistoryRecord {
    CommandHistoryRecord {
        command_id: record.command_id,
        device_id: record.device_id,
        command_name: record.command_name,
        parameters: record.parameters,
        status: CommandStatus::from_storage(record.status),
        result: record.result,
        error: record.error,
        created_at: record.created_at,
        completed_at: record.completed_at,
    }
}

/// Device status information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceStatus {
    /// Current connection status
    pub status: ConnectionStatus,
    /// Last activity timestamp
    pub last_seen: i64,
    /// Adapter that manages this device
    pub adapter_id: Option<String>,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            last_seen: chrono::Utc::now().timestamp(),
            adapter_id: None,
        }
    }
}

impl DeviceStatus {
    /// Create a new device status
    pub fn new(status: ConnectionStatus) -> Self {
        Self {
            status,
            last_seen: chrono::Utc::now().timestamp(),
            adapter_id: None,
        }
    }

    /// Update the status and timestamp
    pub fn update(&mut self, status: ConnectionStatus) {
        self.status = status;
        self.last_seen = chrono::Utc::now().timestamp();
    }

    /// Check if device is currently connected
    /// Returns true only if status is Connected AND last_seen was within 5 minutes
    pub fn is_connected(&self) -> bool {
        if !matches!(self.status, ConnectionStatus::Connected) {
            return false;
        }
        // Check if device was seen in the last 5 minutes (300 seconds)
        let now = chrono::Utc::now().timestamp();
        let elapsed = now - self.last_seen;
        elapsed < 300 // 5 minutes = 300 seconds
    }
}

/// Device Service
/// Provides unified interface for device operations
pub struct DeviceService {
    /// Device registry for templates and configurations
    registry: Arc<DeviceRegistry>,
    /// Event bus for publishing events
    event_bus: EventBus,
    /// Telemetry storage (optional)
    telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Adapter registry for command sending (plugin registry will provide this)
    /// This will be populated by device adapters that register themselves
    adapters: Arc<RwLock<HashMap<String, Arc<dyn DeviceAdapter>>>>,
    /// Device status cache (device_id -> DeviceStatus)
    device_status: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// Command history (device_id -> Vec<CommandHistoryRecord>)
    command_history: Arc<RwLock<HashMap<String, Vec<CommandHistoryRecord>>>>,
    /// Command ID counter
    command_id_counter: Arc<AtomicU64>,
    /// Maximum history entries per device
    max_history_entries: usize,
}

impl DeviceService {
    /// Create a new device service
    pub fn new(registry: Arc<DeviceRegistry>, event_bus: EventBus) -> Self {
        Self {
            registry,
            event_bus,
            telemetry_storage: Arc::new(RwLock::new(None)),
            adapters: Arc::new(RwLock::new(HashMap::new())),
            device_status: Arc::new(RwLock::new(HashMap::new())),
            command_history: Arc::new(RwLock::new(HashMap::new())),
            command_id_counter: Arc::new(AtomicU64::new(1)),
            max_history_entries: 100, // Keep last 100 commands per device
        }
    }

    /// Start the device service - listens for device events and updates status
    /// Also loads command history from storage if available
    pub async fn start(&self) {
        tracing::info!("DeviceService::start() called - subscribing to EventBus events");

        // Load command history from storage if available
        self.load_command_history_from_storage()
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load command history from storage: {}", e);
            });

        let event_bus = self.event_bus.clone();
        let device_status = self.device_status.clone();
        let telemetry_storage = self.telemetry_storage.clone();

        tokio::spawn(async move {
            // Filter for device-related events
            let filter = |event: &edge_ai_core::NeoTalkEvent| -> bool {
                matches!(
                    event,
                    edge_ai_core::NeoTalkEvent::DeviceOnline { .. }
                        | edge_ai_core::NeoTalkEvent::DeviceOffline { .. }
                        | edge_ai_core::NeoTalkEvent::DeviceMetric { .. }
                )
            };
            let mut rx = event_bus.subscribe_filtered(filter);
            while let Some((event, _)) = rx.recv().await {
                match event {
                    edge_ai_core::NeoTalkEvent::DeviceOnline { device_id, .. } => {
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.update(ConnectionStatus::Connected);
                        tracing::debug!("Device {} marked as online", device_id);
                    }
                    edge_ai_core::NeoTalkEvent::DeviceOffline { device_id, .. } => {
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.update(ConnectionStatus::Disconnected);
                        tracing::debug!("Device {} marked as offline", device_id);
                    }
                    edge_ai_core::NeoTalkEvent::DeviceMetric { device_id, metric, value, timestamp: _, quality: _ } => {
                        // Update last_seen when receiving metrics
                        tracing::debug!("Received metric event: device_id={}, metric={}", device_id, metric);
                        let mut status = device_status.write().await;
                        let entry = status.entry(device_id.clone()).or_default();
                        entry.last_seen = chrono::Utc::now().timestamp();
                        // If status was disconnected, mark as connected
                        if entry.status == ConnectionStatus::Disconnected {
                            entry.status = ConnectionStatus::Connected;
                            tracing::info!("Device {} marked as connected due to metric activity", device_id);
                        }
                        drop(status);

                        // Write to telemetry storage if available
                        let ts_storage = telemetry_storage.read().await;
                        if let Some(storage) = ts_storage.as_ref() {
                            // Convert core MetricValue to devices MetricValue
                            let metric_value: MetricValue = match &value {
                                edge_ai_core::MetricValue::Integer(i) => MetricValue::Integer(*i),
                                edge_ai_core::MetricValue::Float(f) => MetricValue::Float(*f),
                                edge_ai_core::MetricValue::String(s) => MetricValue::String(s.clone()),
                                edge_ai_core::MetricValue::Boolean(b) => MetricValue::Boolean(*b),
                                edge_ai_core::MetricValue::Json(j) => {
                                    // Try to convert JSON to appropriate type
                                    if let Some(n) = j.as_i64() {
                                        MetricValue::Integer(n)
                                    } else if let Some(f) = j.as_f64() {
                                        MetricValue::Float(f)
                                    } else if let Some(s) = j.as_str() {
                                        MetricValue::String(s.to_string())
                                    } else if let Some(b) = j.as_bool() {
                                        MetricValue::Boolean(b)
                                    } else {
                                        MetricValue::String(j.to_string())
                                    }
                                }
                            };

                            let data_point = super::telemetry::DataPoint {
                                timestamp: chrono::Utc::now().timestamp(),
                                value: metric_value,
                                quality: None,
                            };

                            if let Err(e) = storage.write(&device_id, &metric, data_point).await {
                                tracing::warn!("Failed to write telemetry to storage: {}", e);
                            } else {
                                tracing::debug!("DeviceService wrote metric {} for device {}", metric, device_id);
                            }
                        } else {
                            tracing::warn!("DeviceService telemetry_storage is None, cannot write metric {} for device {}", metric, device_id);
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    /// Set telemetry storage
    pub async fn set_telemetry_storage(&self, storage: Arc<TimeSeriesStorage>) {
        *self.telemetry_storage.write().await = Some(storage);
    }

    /// Register an adapter for command sending
    pub async fn register_adapter(&self, adapter_id: String, adapter: Arc<dyn DeviceAdapter>) {
        // Set telemetry storage for the adapter if available
        let telemetry_storage = self.telemetry_storage.read().await;
        if let Some(storage) = telemetry_storage.as_ref() {
            tracing::info!("Setting telemetry storage for adapter '{}'", adapter_id);
            adapter.set_telemetry_storage(storage.clone());
        } else {
            tracing::warn!("Cannot set telemetry storage for adapter '{}': DeviceService telemetry_storage is None", adapter_id);
        }
        drop(telemetry_storage);

        let mut adapters = self.adapters.write().await;
        adapters.insert(adapter_id, adapter);
    }

    /// Unregister an adapter
    pub async fn unregister_adapter(&self, adapter_id: &str) {
        let mut adapters = self.adapters.write().await;
        adapters.remove(adapter_id);
    }

    /// Get an adapter by ID
    pub async fn get_adapter(&self, adapter_id: &str) -> Option<Arc<dyn DeviceAdapter>> {
        let adapters = self.adapters.read().await;
        adapters.get(adapter_id).cloned()
    }

    // ========== Template Management ==========

    /// Register a device type template
    pub async fn register_template(&self, template: DeviceTypeTemplate) -> Result<(), DeviceError> {
        self.registry.register_template(template).await
    }

    /// Get a device type template
    pub async fn get_template(&self, device_type: &str) -> Option<DeviceTypeTemplate> {
        self.registry.get_template(device_type).await
    }

    /// List all device type templates
    pub async fn list_templates(&self) -> Vec<DeviceTypeTemplate> {
        self.registry.list_templates().await
    }

    /// Unregister a device type template
    pub async fn unregister_template(&self, device_type: &str) -> Result<(), DeviceError> {
        self.registry.unregister_template(device_type).await
    }

    // ========== Device Configuration Management ==========

    /// Register a device configuration
    /// This will also notify the appropriate adapter to subscribe to the device's telemetry topic
    pub async fn register_device(&self, config: DeviceConfig) -> Result<(), DeviceError> {
        let device_id = config.device_id.clone();
        let adapter_type = config.adapter_type.clone();

        // First register the device in the registry
        self.registry.register_device(config).await?;

        // Then notify the adapter to subscribe to this device's telemetry topic
        // Find the adapter that handles this device type
        let adapters = self.adapters.read().await;
        for (adapter_id, adapter) in adapters.iter() {
            // Check if this adapter can handle the device type
            if adapter.adapter_type() == adapter_type {
                tracing::info!(
                    "Notifying adapter '{}' to subscribe to device '{}'",
                    adapter_id,
                    device_id
                );
                // Subscribe the adapter to this device
                let _ = adapter.subscribe_device(&device_id).await;
                break;
            }
        }

        Ok(())
    }

    /// Get a device configuration with its template
    pub async fn get_device_with_template(
        &self,
        device_id: &str,
    ) -> Result<(DeviceConfig, DeviceTypeTemplate), DeviceError> {
        let config = self
            .registry
            .get_device(device_id)
            .await
            .ok_or_else(|| DeviceError::NotFoundStr(device_id.to_string()))?;

        let template = self
            .registry
            .get_template(&config.device_type)
            .await
            .ok_or_else(|| {
                DeviceError::NotFoundStr(format!(
                    "Template '{}' not found for device '{}'",
                    config.device_type, device_id
                ))
            })?;

        Ok((config, template))
    }

    /// Get a device configuration
    pub async fn get_device(&self, device_id: &str) -> Option<DeviceConfig> {
        self.registry.get_device(device_id).await
    }

    /// List all device configurations
    pub async fn list_devices(&self) -> Vec<DeviceConfig> {
        self.registry.list_devices().await
    }

    /// List devices by type
    pub async fn list_devices_by_type(&self, device_type: &str) -> Vec<DeviceConfig> {
        self.registry.list_devices_by_type(device_type).await
    }

    /// Unregister a device configuration
    pub async fn unregister_device(&self, device_id: &str) -> Result<(), DeviceError> {
        self.registry.unregister_device(device_id).await
    }

    /// Update a device configuration
    pub async fn update_device(
        &self,
        device_id: &str,
        config: DeviceConfig,
    ) -> Result<(), DeviceError> {
        self.registry.update_device(device_id, config).await
    }

    // ========== Command Sending ==========

    /// Send a command to a device
    /// Automatically uses the device's template to validate and build the command payload
    pub async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        params: HashMap<String, serde_json::Value>,
    ) -> Result<Option<MetricValue>, DeviceError> {
        // Get device config and template
        let (config, template) = self.get_device_with_template(device_id).await?;

        // Find command definition in template
        let command_def = template
            .commands
            .iter()
            .find(|cmd| cmd.name == command_name)
            .ok_or_else(|| {
                DeviceError::InvalidCommand(format!(
                    "Command '{}' not found in template '{}'",
                    command_name, template.device_type
                ))
            })?;

        // Validate and convert parameters
        let validated_params = self.validate_command_params(command_def, params)?;

        // Build command payload from template
        let payload = self.build_command_payload(command_def, &validated_params)?;

        // Get adapter for sending command
        let adapter_id = config
            .adapter_id
            .as_deref()
            .ok_or_else(|| DeviceError::InvalidParameter("Device has no adapter_id set".into()))?;

        let adapter = {
            let adapters = self.adapters.read().await;
            adapters.get(adapter_id).cloned().ok_or_else(|| {
                DeviceError::NotFoundStr(format!("Adapter '{}' not found", adapter_id))
            })?
        };

        // Determine command topic from device connection config
        // For MQTT, this would be the command_topic field
        let command_topic = config.connection_config.command_topic.clone();

        // Send command via adapter
        adapter
            .send_command(device_id, command_name, payload, command_topic)
            .await
            .map_err(|e| {
                DeviceError::InvalidParameter(format!("Failed to send command via adapter: {}", e))
            })?;

        // Return None for now (could return command result in the future)
        Ok(None)
    }

    /// Validate command parameters against template definition
    fn validate_command_params(
        &self,
        command_def: &super::mdl_format::CommandDefinition,
        params: HashMap<String, serde_json::Value>,
    ) -> Result<HashMap<String, MetricValue>, DeviceError> {
        let mut validated = HashMap::new();

        for param_def in &command_def.parameters {
            // Check if parameter is provided or has default
            let value = if let Some(param_value) = params.get(&param_def.name) {
                // Convert JSON value to MetricValue
                self.json_to_metric_value(param_value, &param_def.data_type)?
            } else if let Some(default) = &param_def.default_value {
                // Use default value
                default.clone()
            } else {
                return Err(DeviceError::InvalidParameter(format!(
                    "Required parameter '{}' not provided and has no default",
                    param_def.name
                )));
            };

            // Validate min/max if applicable
            if let (Some(min), Some(max)) = (param_def.min, param_def.max) {
                match &value {
                    MetricValue::Integer(i) => {
                        let i_f64 = *i as f64;
                        if i_f64 < min || i_f64 > max {
                            return Err(DeviceError::InvalidParameter(format!(
                                "Parameter '{}' value {} out of range [{}, {}]",
                                param_def.name, i, min, max
                            )));
                        }
                    }
                    MetricValue::Float(f) => {
                        if *f < min || *f > max {
                            return Err(DeviceError::InvalidParameter(format!(
                                "Parameter '{}' value {} out of range [{}, {}]",
                                param_def.name, f, min, max
                            )));
                        }
                    }
                    _ => {}
                }
            }

            validated.insert(param_def.name.clone(), value);
        }

        Ok(validated)
    }

    /// Convert JSON value to MetricValue based on expected type
    fn json_to_metric_value(
        &self,
        json: &serde_json::Value,
        expected_type: &super::mdl::MetricDataType,
    ) -> Result<MetricValue, DeviceError> {
        match (json, expected_type) {
            (serde_json::Value::Number(n), super::mdl::MetricDataType::Integer) => {
                if let Some(i) = n.as_i64() {
                    Ok(MetricValue::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(MetricValue::Integer(f as i64))
                } else {
                    Err(DeviceError::InvalidParameter(
                        "Number out of range for Integer".into(),
                    ))
                }
            }
            (serde_json::Value::Number(n), super::mdl::MetricDataType::Float) => {
                if let Some(f) = n.as_f64() {
                    Ok(MetricValue::Float(f))
                } else {
                    Err(DeviceError::InvalidParameter(
                        "Number cannot be converted to Float".into(),
                    ))
                }
            }
            (serde_json::Value::Bool(b), super::mdl::MetricDataType::Boolean) => {
                Ok(MetricValue::Boolean(*b))
            }
            (serde_json::Value::String(s), super::mdl::MetricDataType::String) => {
                Ok(MetricValue::String(s.clone()))
            }
            (serde_json::Value::Null, _) => Ok(MetricValue::Null),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Integer) => s
                .trim()
                .parse::<i64>()
                .map(MetricValue::Integer)
                .map_err(|_| {
                    DeviceError::InvalidParameter(format!("Cannot convert '{}' to Integer", s))
                }),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Float) => s
                .trim()
                .parse::<f64>()
                .map(MetricValue::Float)
                .map_err(|_| {
                    DeviceError::InvalidParameter(format!("Cannot convert '{}' to Float", s))
                }),
            (serde_json::Value::String(s), super::mdl::MetricDataType::Boolean) => {
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "true" | "1" | "yes" | "on" => Ok(MetricValue::Boolean(true)),
                    "false" | "0" | "no" | "off" => Ok(MetricValue::Boolean(false)),
                    _ => Err(DeviceError::InvalidParameter(format!(
                        "Cannot convert '{}' to Boolean",
                        s
                    ))),
                }
            }
            (v, _) => {
                // Try to convert to string as fallback
                Ok(MetricValue::String(v.to_string()))
            }
        }
    }

    /// Build command payload from template
    fn build_command_payload(
        &self,
        command_def: &super::mdl_format::CommandDefinition,
        params: &HashMap<String, MetricValue>,
    ) -> Result<String, DeviceError> {
        let mut payload = command_def.payload_template.clone();

        // Replace ${param} placeholders with actual values
        for (param_name, value) in params {
            let placeholder = format!("${{{{{}}}}}", param_name);
            let value_str = match value {
                MetricValue::Integer(i) => i.to_string(),
                MetricValue::Float(f) => f.to_string(),
                MetricValue::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                MetricValue::Boolean(b) => b.to_string(),
                MetricValue::Binary(_) => {
                    return Err(DeviceError::InvalidParameter(
                        "Binary values not supported in command payloads".into(),
                    ));
                }
                MetricValue::Null => "null".to_string(),
            };
            payload = payload.replace(&placeholder, &value_str);
        }

        Ok(payload)
    }

    // ========== Data Querying ==========

    /// Query telemetry data for a device metric
    pub async fn query_telemetry(
        &self,
        device_id: &str,
        metric_name: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<(i64, MetricValue)>, DeviceError> {
        // Get device config and template
        let (config, template) = self.get_device_with_template(device_id).await?;

        // Validate metric exists in template (skip validation in simple mode where no metrics are defined)
        if !template.metrics.is_empty() && !template.metrics.iter().any(|m| m.name == metric_name) {
            return Err(DeviceError::InvalidMetric(format!(
                "Metric '{}' not found in template '{}'",
                metric_name, template.device_type
            )));
        }

        // Query from telemetry storage
        let storage_guard = self.telemetry_storage.read().await;
        if let Some(storage) = storage_guard.as_ref() {
            let start = start_time.unwrap_or(i64::MIN);
            let end = end_time.unwrap_or(i64::MAX);

            tracing::debug!("Querying telemetry for {}/{} from {} to {}", device_id, metric_name, start, end);

            let points = storage
                .query(device_id, metric_name, start, end)
                .await
                .map_err(|e| {
                    tracing::error!("Telemetry query failed for {}/{}: {}", device_id, metric_name, e);
                    DeviceError::Communication(format!("Telemetry query failed: {}", e))
                })?;

            tracing::debug!("Query returned {} points for {}/{}", points.len(), device_id, metric_name);

            Ok(points.into_iter().map(|p| (p.timestamp, p.value)).collect())
        } else {
            tracing::warn!("Telemetry storage not configured when querying {}/{}", device_id, metric_name);
            Err(DeviceError::InvalidParameter(
                "Telemetry storage not configured".into(),
            ))
        }
    }

    /// Get current metric values for a device (latest from cache)
    /// For devices with no defined metrics (simple mode), returns all available metrics from storage
    pub async fn get_current_metrics(
        &self,
        device_id: &str,
    ) -> Result<HashMap<String, MetricValue>, DeviceError> {
        // Get device config and template
        let (_, template) = self.get_device_with_template(device_id).await?;

        let mut result = HashMap::new();
        let now = chrono::Utc::now().timestamp();

        // If template has defined metrics, query those specifically
        if !template.metrics.is_empty() {
            for metric in &template.metrics {
                // Query latest value (last 1 hour)
                match self
                    .query_telemetry(device_id, &metric.name, Some(now - 3600), Some(now))
                    .await
                {
                    Ok(points) => {
                        if let Some((_, value)) = points.last() {
                            result.insert(metric.name.clone(), value.clone());
                        }
                    }
                    Err(_) => {
                        // Metric not available yet
                    }
                }
            }
        } else {
            // Simple mode: no metrics defined - return all available metrics from storage
            // Query all metrics for this device from the last hour
            // Use telemetry_storage to list all metrics for this device
            if let Some(storage) = self.telemetry_storage.read().await.as_ref() {
                if let Ok(all_metrics) = storage.list_metrics(device_id).await {
                    for metric_name in all_metrics {
                        if !metric_name.is_empty() {
                            // Query latest value for this metric
                            match self
                                .query_telemetry(device_id, &metric_name, Some(now - 3600), Some(now))
                                .await
                            {
                                Ok(points) => {
                                    if let Some((_, value)) = points.last() {
                                        result.insert(metric_name.clone(), value.clone());
                                    }
                                }
                                Err(_) => {
                                    // Metric not available yet, skip
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    // ========== Helper Methods ==========

    /// Get registry reference (for external use)
    pub fn registry(&self) -> &Arc<DeviceRegistry> {
        &self.registry
    }

    /// Get event bus reference
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Get metric definition from a device type template
    pub async fn get_metric_definition(
        &self,
        device_type: &str,
        metric_name: &str,
    ) -> Option<super::mdl_format::MetricDefinition> {
        let template = self.registry.get_template(device_type).await?;
        template.metrics.into_iter().find(|m| m.name == metric_name)
    }

    // ========== Device Status Management ==========

    /// Get the current status of a device
    pub async fn get_device_status(&self, device_id: &str) -> DeviceStatus {
        let status_map = self.device_status.read().await;
        status_map.get(device_id).cloned().unwrap_or_default()
    }

    /// Get the connection status for a device
    pub async fn get_device_connection_status(&self, device_id: &str) -> ConnectionStatus {
        self.get_device_status(device_id).await.status
    }

    /// Get the last seen timestamp for a device
    pub async fn get_device_last_seen(&self, device_id: &str) -> i64 {
        self.get_device_status(device_id).await.last_seen
    }

    /// Update device status (called by event listeners or adapters)
    pub async fn update_device_status(&self, device_id: &str, status: ConnectionStatus) {
        let mut status_map = self.device_status.write().await;
        let entry = status_map.entry(device_id.to_string()).or_default();
        entry.update(status);
    }

    /// Get all device statuses
    pub async fn get_all_device_statuses(&self) -> HashMap<String, DeviceStatus> {
        let status_map = self.device_status.read().await;
        status_map.clone()
    }

    /// Get devices filtered by status
    pub async fn get_devices_by_status(&self, status: ConnectionStatus) -> Vec<String> {
        let status_map = self.device_status.read().await;
        status_map
            .iter()
            .filter(|(_, s)| s.status == status)
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ========== Command History Management ==========

    /// Add a command to history
    pub async fn add_command_to_history(
        &self,
        device_id: &str,
        command_name: &str,
        parameters: HashMap<String, serde_json::Value>,
    ) -> String {
        let command_id = format!(
            "cmd_{}",
            self.command_id_counter.fetch_add(1, Ordering::Relaxed)
        );

        let record = CommandHistoryRecord {
            command_id: command_id.clone(),
            device_id: device_id.to_string(),
            command_name: command_name.to_string(),
            parameters,
            status: CommandStatus::Pending,
            result: None,
            error: None,
            created_at: chrono::Utc::now().timestamp(),
            completed_at: None,
        };

        // Clone for storage before moving into the HashMap
        let record_for_storage = record.clone();

        let mut history = self.command_history.write().await;
        let device_commands = history
            .entry(device_id.to_string())
            .or_insert_with(Vec::new);
        device_commands.push(record);

        // Trim history if exceeds max
        if device_commands.len() > self.max_history_entries {
            device_commands.remove(0);
        }
        drop(history);

        // Persist to storage
        self.save_command_to_storage(&record_for_storage).await;

        command_id
    }

    /// Update command status in history
    pub async fn update_command_status(
        &self,
        device_id: &str,
        command_id: &str,
        status: CommandStatus,
        result: Option<String>,
        error: Option<String>,
    ) {
        let is_terminal = status.is_terminal();
        let mut record_for_storage = None;

        {
            let mut history = self.command_history.write().await;
            if let Some(device_commands) = history.get_mut(device_id) {
                if let Some(record) = device_commands
                    .iter_mut()
                    .find(|r| r.command_id == command_id)
                {
                    record.status = status;
                    record.result = result;
                    record.error = error;
                    if is_terminal {
                        record.completed_at = Some(chrono::Utc::now().timestamp());
                    }
                    // Clone for storage after updating
                    record_for_storage = Some(record.clone());
                }
            }
        }

        // Persist to storage if record was found and updated
        if let Some(record) = record_for_storage {
            self.save_command_to_storage(&record).await;
        }
    }

    /// Get command history for a device
    pub async fn get_command_history(
        &self,
        device_id: &str,
        limit: Option<usize>,
    ) -> Vec<CommandHistoryRecord> {
        let history = self.command_history.read().await;
        if let Some(device_commands) = history.get(device_id) {
            let mut commands = device_commands.clone();
            // Sort by created_at descending (newest first)
            commands.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            if let Some(limit) = limit {
                commands.truncate(limit);
            }
            commands
        } else {
            vec![]
        }
    }

    /// Get a specific command by ID
    pub async fn get_command(
        &self,
        device_id: &str,
        command_id: &str,
    ) -> Option<CommandHistoryRecord> {
        let history = self.command_history.read().await;
        history
            .get(device_id)?
            .iter()
            .find(|r| r.command_id == command_id)
            .cloned()
    }

    /// Load command history from storage (called on startup)
    async fn load_command_history_from_storage(&self) -> Result<(), DeviceError> {
        let Some(store) = self.registry.storage() else {
            // No storage configured, that's fine
            return Ok(());
        };

        // List all commands from storage (limit to reasonable number)
        let storage_commands = store.list_all_commands(Some(1000)).map_err(|e| {
            DeviceError::Storage(format!("Failed to load command history: {}", e))
        })?;

        if storage_commands.is_empty() {
            return Ok(());
        }

        let mut history = self.command_history.write().await;
        let mut max_counter = 0;

        for storage_record in storage_commands {
            let device_id = storage_record.device_id.clone();
            let record = command_from_storage(storage_record);

            // Update command ID counter based on loaded records
            if let Some(suffix) = record.command_id.strip_prefix("cmd_") {
                if let Ok(num) = suffix.parse::<u64>() {
                    max_counter = max_counter.max(num);
                }
            }

            history
                .entry(device_id)
                .or_insert_with(Vec::new)
                .push(record);
        }

        // Update the command ID counter to avoid collisions
        self.command_id_counter
            .fetch_max(max_counter + 1, Ordering::Relaxed);

        tracing::info!(
            "Loaded {} command history entries from storage",
            history.values().map(|v| v.len()).sum::<usize>()
        );

        Ok(())
    }

    /// Save a command record to storage
    async fn save_command_to_storage(&self, record: &CommandHistoryRecord) {
        let Some(store) = self.registry.storage() else {
            return;
        };

        let storage_record = command_to_storage(record);
        if let Err(e) = store.save_command(&storage_record) {
            tracing::warn!("Failed to save command {} to storage: {}", record.command_id, e);
        }
    }

    /// Clear command history for a device
    pub async fn clear_command_history(&self, device_id: &str) {
        let mut history = self.command_history.write().await;
        history.remove(device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ConnectionConfig;

    #[tokio::test]
    async fn test_service_template_registration() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        service.register_template(template).await.unwrap();

        let retrieved = service.get_template("test_sensor").await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_service_device_registration() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        // Register template first
        let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor");
        service.register_template(template).await.unwrap();

        // Register device
        let config = DeviceConfig {
            device_id: "sensor1".to_string(),
            name: "Sensor 1".to_string(),
            device_type: "test_sensor".to_string(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig::new(),
            adapter_id: None,
        };

        service.register_device(config).await.unwrap();

        let retrieved = service.get_device("sensor1").await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_command_parameter_validation() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl::MetricDataType;
        use crate::mdl_format::CommandDefinition;
        use crate::mdl_format::ParameterDefinition;

        let command_def = CommandDefinition {
            name: "set_temperature".to_string(),
            display_name: "Set Temperature".to_string(),
            payload_template: r#"{"action": "set_temperature", "value": ${{value}}}"#.to_string(),
            parameters: vec![ParameterDefinition {
                name: "value".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                default_value: None,
                min: Some(0.0),
                max: Some(100.0),
                unit: "°C".to_string(),
                allowed_values: vec![],
            }],
            samples: vec![],
            llm_hints: String::new(),
        };

        // Valid parameter
        let mut params = HashMap::new();
        params.insert("value".to_string(), serde_json::json!(25.5));
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_ok());

        // Invalid parameter (out of range)
        let mut params = HashMap::new();
        params.insert("value".to_string(), serde_json::json!(150.0));
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_err());

        // Missing parameter
        let params = HashMap::new();
        let result = service.validate_command_params(&command_def, params);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_command_payload() {
        let event_bus = EventBus::new();
        let registry = Arc::new(DeviceRegistry::new());
        let service = DeviceService::new(registry.clone(), event_bus);

        use crate::mdl_format::CommandDefinition;

        let command_def = CommandDefinition {
            name: "set_temperature".to_string(),
            display_name: "Set Temperature".to_string(),
            payload_template: r#"{"action": "set_temperature", "value": ${{value}}}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            llm_hints: String::new(),
        };

        let mut params = HashMap::new();
        params.insert("value".to_string(), MetricValue::Float(25.5));

        let payload = service
            .build_command_payload(&command_def, &params)
            .unwrap();
        assert!(payload.contains("25.5"));
    }
}
