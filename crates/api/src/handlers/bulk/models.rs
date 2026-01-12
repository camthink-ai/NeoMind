//! Bulk operation DTOs and request structures.

use serde::{Deserialize, Serialize};

/// Result of a single bulk operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkOperationResult {
    /// Index of the item in the request (for correlation)
    pub index: usize,
    /// Whether the operation succeeded
    pub success: bool,
    /// The resource ID (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Bulk operation response containing results for all items.
#[derive(Debug, Clone, Serialize)]
pub struct BulkResponse {
    /// Total number of items in the request
    pub total: usize,
    /// Number of successful operations
    pub succeeded: usize,
    /// Number of failed operations
    pub failed: usize,
    /// Individual operation results
    pub results: Vec<BulkOperationResult>,
}

/// Request for creating multiple alerts.
#[derive(Debug, Deserialize)]
pub struct BulkCreateAlertsRequest {
    pub alerts: Vec<AlertCreateItem>,
}

/// Single alert creation item.
#[derive(Debug, Deserialize)]
pub struct AlertCreateItem {
    pub title: String,
    pub message: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub source: String,
}

fn default_severity() -> String { "info".to_string() }

/// Request for resolving multiple alerts.
#[derive(Debug, Deserialize)]
pub struct BulkResolveAlertsRequest {
    /// Alert IDs to resolve
    pub alert_ids: Vec<String>,
}

/// Request for deleting multiple alerts.
#[derive(Debug, Deserialize)]
pub struct BulkDeleteAlertsRequest {
    /// Alert IDs to delete
    pub alert_ids: Vec<String>,
}

/// Request for acknowledging multiple alerts.
#[derive(Debug, Deserialize)]
pub struct BulkAcknowledgeAlertsRequest {
    /// Alert IDs to acknowledge
    pub alert_ids: Vec<String>,
}

/// Request for deleting multiple sessions.
#[derive(Debug, Deserialize)]
pub struct BulkDeleteSessionsRequest {
    /// Session IDs to delete
    pub session_ids: Vec<String>,
}

/// Request for deleting multiple devices.
#[derive(Debug, Deserialize)]
pub struct BulkDeleteDevicesRequest {
    /// Device IDs to delete
    pub device_ids: Vec<String>,
}

/// Request for deleting multiple device types.
#[derive(Debug, Deserialize)]
pub struct BulkDeleteDeviceTypesRequest {
    /// Device type IDs to delete
    pub type_ids: Vec<String>,
}

/// Request for executing commands on multiple devices.
#[derive(Debug, Deserialize)]
pub struct BulkDeviceCommandRequest {
    /// Device IDs to target
    pub device_ids: Vec<String>,
    /// Command to execute
    pub command: String,
    /// Optional parameters
    #[serde(default)]
    pub parameters: serde_json::Value,
}
