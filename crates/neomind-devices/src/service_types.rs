//! Types for device service operations
//!
//! This module contains common types used by the device service,
//! separated for better organization and reusability.

use crate::adapter::ConnectionStatus;
use std::collections::HashMap;
use tokio::time::Duration;

// Import storage types for command history persistence
use neomind_storage::device_registry::{
    CommandHistoryRecord as StorageCommandRecord, CommandStatus as StorageCommandStatus,
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
    pub fn to_storage(&self) -> StorageCommandStatus {
        match self {
            Self::Pending => StorageCommandStatus::Pending,
            Self::Executing => StorageCommandStatus::Sent,
            Self::Success => StorageCommandStatus::Completed,
            Self::Failed => StorageCommandStatus::Failed,
            Self::Timeout => StorageCommandStatus::Timeout,
        }
    }

    /// Convert from storage command status
    pub fn from_storage(status: StorageCommandStatus) -> Self {
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
pub fn command_to_storage(record: &CommandHistoryRecord) -> StorageCommandRecord {
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
pub fn command_from_storage(record: StorageCommandRecord) -> CommandHistoryRecord {
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

/// Adapter information for API responses.
///
/// This provides a simplified view of adapter state without the plugin system overhead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterInfo {
    /// Adapter ID
    pub id: String,
    /// Adapter name
    pub name: String,
    /// Adapter type (mqtt, http, webhook, etc.)
    pub adapter_type: String,
    /// Whether the adapter is running
    pub running: bool,
    /// Number of devices managed by this adapter
    pub device_count: usize,
    /// Connection status
    pub status: String,
    /// Last activity timestamp
    pub last_activity: i64,
}

/// Aggregated adapter statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterStats {
    /// Total number of adapters
    pub total_adapters: usize,
    /// Number of running adapters
    pub running_adapters: usize,
    /// Total number of devices across all adapters
    pub total_devices: usize,
    /// Per-adapter information
    pub adapters: Vec<AdapterInfo>,
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
            // Use 0 as default to indicate "never seen" - this ensures proper
            // offline detection for devices that haven't sent any metrics yet
            last_seen: 0,
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

/// Heartbeat configuration for device health monitoring
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Heartbeat interval in seconds (default: 60)
    pub heartbeat_interval: u64,
    /// Device offline timeout in seconds (default: 300 = 5 minutes)
    pub offline_timeout: u64,
    /// Whether to automatically mark stale devices as offline
    pub auto_mark_offline: bool,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: 60,
            offline_timeout: 300,
            auto_mark_offline: true,
        }
    }
}

impl HeartbeatConfig {
    /// Create a new heartbeat configuration
    pub fn new(interval_secs: u64, timeout_secs: u64) -> Self {
        Self {
            heartbeat_interval: interval_secs,
            offline_timeout: timeout_secs,
            auto_mark_offline: true,
        }
    }

    /// Get the interval as Duration
    pub fn interval_duration(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval)
    }

    /// Check if a device is stale based on last_seen timestamp
    pub fn is_stale(&self, last_seen: i64) -> bool {
        let now = chrono::Utc::now().timestamp();
        let elapsed = (now - last_seen) as u64;
        elapsed > self.offline_timeout
    }
}
