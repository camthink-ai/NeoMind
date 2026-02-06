//! Device management state.
//!
//! Contains all device-related services:
//! - DeviceRegistry for device templates and configurations
//! - DeviceService for unified device operations
//! - TimeSeriesStorage for device metrics/telemetry
//! - EmbeddedBroker (optional) for MQTT
//! - Device status broadcast channel

use std::sync::Arc;
use tokio::sync::broadcast;

use neomind_devices::{DeviceRegistry, DeviceService, TimeSeriesStorage};

#[cfg(feature = "embedded-broker")]
use neomind_devices::EmbeddedBroker;

/// Device management state.
///
/// Provides access to all device-related services and storage.
#[derive(Clone)]
pub struct DeviceState {
    /// Device registry for templates and configurations.
    pub registry: Arc<DeviceRegistry>,

    /// Device service for unified device operations.
    pub service: Arc<DeviceService>,

    /// Time series storage for device metrics/telemetry.
    pub telemetry: Arc<TimeSeriesStorage>,

    /// Embedded MQTT broker (only used in embedded mode).
    #[cfg(feature = "embedded-broker")]
    pub embedded_broker: Option<Arc<EmbeddedBroker>>,

    /// Device status update broadcast sender.
    pub update_tx: broadcast::Sender<DeviceStatusUpdate>,
}

impl DeviceState {
    /// Create a new device state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Arc<DeviceRegistry>,
        service: Arc<DeviceService>,
        telemetry: Arc<TimeSeriesStorage>,
        update_tx: broadcast::Sender<DeviceStatusUpdate>,
    ) -> Self {
        Self {
            registry,
            service,
            telemetry,
            update_tx,
            #[cfg(feature = "embedded-broker")]
            embedded_broker: None,
        }
    }
}

/// Device status update for WebSocket broadcast.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceStatusUpdate {
    /// Update type
    pub update_type: String,
    /// Device ID
    pub device_id: String,
    /// Device status (online/offline/etc)
    pub status: Option<String>,
    /// Last seen timestamp
    pub last_seen: Option<i64>,
}
