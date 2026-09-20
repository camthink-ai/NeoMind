//! MQTT device adapter for NeoMind event-driven architecture.
//!
//! This adapter connects to an MQTT broker, subscribes to device topics,
//! and publishes device events to the event bus.
//!
//! ## Topic Format
//!
//! Uplink telemetry: `device/{device_type}/{device_id}/uplink`
//! Downlink commands: `device/{device_type}/{device_id}/downlink`
//! Discovery: `{discovery_prefix}/announce`
//!
//! ## Protocol Mapping Integration
//!
//! The adapter can use a `ProtocolMapping` for flexible topic and payload handling:
//! ```text
//! Device Type Definition       MQTT Mapping
//! ├─ temperature capability  ──→ sensor/${id}/temperature
//! ├─ humidity capability     ──→ sensor/${id}/humidity
//! └─ set_interval command    ──→ sensor/${id}/command
//! ```

use crate::adapter::{ConnectionStatus, DeviceAdapter, DeviceEvent};
use crate::mdl::MetricValue;
use crate::protocol::ProtocolMapping;
use crate::registry::DeviceRegistry;
use crate::telemetry::TimeSeriesStorage;
use crate::unified_extractor::UnifiedExtractor;
use base64::Engine as _;
use futures::StreamExt;
use neomind_core::EventBus;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
// TLS support

/// Single MQTT broker connection
pub(crate) struct MqttClientInner {
    /// Unique broker identifier
    pub(crate) _broker_id: String,
    /// Broker address (host:port)
    pub(crate) _broker_addr: String,
    /// MQTT client
    pub(crate) client: rumqttc::AsyncClient,
    /// Running flag for the event loop task
    pub(crate) running: Arc<RwLock<bool>>,
    /// Subscribed topics for this broker
    pub(crate) subscribed_topics: Arc<RwLock<std::collections::HashSet<String>>>,
}

/// MQTT device adapter.
///
/// Manages multiple MQTT broker connections and automatically subscribes
/// to device topics on all connected brokers.
pub struct MqttAdapter {
    /// Adapter configuration
    pub(crate) config: MqttAdapterConfig,
    /// Event channel sender
    pub(crate) event_tx: broadcast::Sender<DeviceEvent>,
    /// Running state
    pub(crate) running: Arc<std::sync::atomic::AtomicBool>,
    /// Connected devices
    pub(crate) devices: Arc<RwLock<Vec<String>>>,
    /// Optional protocol mapping for flexible topic/payload handling
    pub(crate) protocol_mapping: Option<Arc<dyn ProtocolMapping>>,
    /// Device ID to device type mapping (used with protocol mapping)
    pub(crate) device_types: Arc<RwLock<HashMap<String, String>>>,
    /// Multiple MQTT broker connections (broker_id -> client)
    pub(crate) mqtt_clients: Arc<RwLock<HashMap<String, MqttClientInner>>>,
    /// Connection status (overall - true if at least one broker is connected)
    pub(crate) connection_status: Arc<RwLock<ConnectionStatus>>,
    /// Event bus for publishing system events
    pub(crate) event_bus: Option<Arc<EventBus>>,
    /// Device registry for template management (wrapped for interior mutability)
    pub(crate) device_registry: Arc<RwLock<Arc<DeviceRegistry>>>,
    /// Time series storage for telemetry
    pub(crate) telemetry_storage: Arc<RwLock<Option<Arc<TimeSeriesStorage>>>>,
    /// Metric cache (device_id -> metric_name -> (value, timestamp))
    pub(crate) metric_cache:
        Arc<RwLock<HashMap<String, HashMap<String, (MetricValue, chrono::DateTime<chrono::Utc>)>>>>,
    /// Topic to device ID mapping (for routing messages to registered devices)
    pub(crate) topic_to_device: Arc<RwLock<HashMap<String, String>>>,
    /// Topics this adapter has published OUTBOUND command payloads to.
    /// Used to suppress the broker self-echo — when the embedded broker
    /// reflects our own publish back through our wildcard subscription,
    /// the inbound handler must NOT route it through auto-onboarding
    /// (otherwise every `capture` command creates a phantom "discovered
    /// device" entry for the command topic).
    pub(crate) outbound_command_topics: Arc<RwLock<HashSet<String>>>,
    /// Unified data extractor
    pub(crate) extractor: Arc<UnifiedExtractor>,
    /// Data directory for image storage (runtime, not config)
    pub(crate) data_dir: Arc<RwLock<Option<PathBuf>>>,
}

/// Create an MQTT adapter connected to an event bus.
pub fn create_mqtt_adapter(
    config: MqttAdapterConfig,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<MqttAdapter> {
    // Convert &EventBus to Arc<EventBus> by creating a new Arc
    // Note: This assumes EventBus can be cloned safely
    let event_bus_arc = Arc::new(event_bus.clone());

    let adapter = Arc::new(
        MqttAdapter::new(config)
            .with_event_bus(event_bus_arc)
            .with_device_registry(device_registry),
    );

    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            if let Some(device_id) = event.device_id() {
                let source = format!("adapter:mqtt:{}", device_id);
                let neomind_event = event.clone().to_neomind_event();
                event_bus.publish_with_source(neomind_event, source).await;
            }
        }
    });

    adapter
}

/// Create an MQTT adapter with protocol mapping.
pub fn create_mqtt_adapter_with_mapping(
    config: MqttAdapterConfig,
    mapping: Arc<dyn ProtocolMapping>,
    event_bus: &EventBus,
    device_registry: Arc<DeviceRegistry>,
) -> Arc<MqttAdapter> {
    let event_bus_arc = Arc::new(event_bus.clone());

    let adapter = Arc::new(
        MqttAdapter::with_mapping(config, mapping, Some(event_bus_arc))
            .with_device_registry(device_registry),
    );

    let adapter_clone = adapter.clone();
    let event_bus = event_bus.clone();

    // Spawn event forwarding task
    tokio::spawn(async move {
        let mut rx = adapter_clone.subscribe();
        while let Some(event) = rx.next().await {
            if let Some(device_id) = event.device_id() {
                let source = format!("adapter:mqtt:{}", device_id);
                let neomind_event = event.clone().to_neomind_event();
                event_bus.publish_with_source(neomind_event, source).await;
            }
        }
    });

    adapter
}

/// Result of an MQTT connection test.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MqttTestResult {
    pub success: bool,
    pub message: String,
}

/// Test MQTT connectivity by performing a real CONNECT/CONNACK handshake.
///
/// This creates a temporary MQTT client, connects to the broker, and waits
/// for CONNACK. Returns success if the broker accepts the connection.
pub async fn test_mqtt_connection(
    host: &str,
    port: u16,
    username: Option<&str>,
    password: Option<&str>,
    tls: bool,
    ca_cert: Option<&str>,
    client_cert: Option<&str>,
    client_key: Option<&str>,
) -> MqttTestResult {
    let client_id = format!("neomind-test-{}", uuid::Uuid::new_v4());
    let mut mqttoptions = rumqttc::MqttOptions::new(&client_id, host, port);
    mqttoptions.set_max_packet_size(1024, 1024);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    if let (Some(user), Some(pass)) = (username, password) {
        mqttoptions.set_credentials(user, pass);
    }

    if tls {
        match MqttAdapter::build_tls_transport(ca_cert, client_cert, client_key) {
            Ok(transport) => {
                mqttoptions.set_transport(transport);
            }
            Err(e) => {
                return MqttTestResult {
                    success: false,
                    message: format!("TLS configuration error: {}", e),
                };
            }
        }
    }

    let (_client, mut eventloop) = rumqttc::AsyncClient::new(mqttoptions, 5);

    // Poll for CONNACK with a 10-second timeout
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match eventloop.poll().await {
                Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(ack))) => {
                    if ack.code == rumqttc::ConnectReturnCode::Success {
                        return Ok("MQTT CONNECT successful".to_string());
                    } else {
                        return Err(format!("CONNACK rejected: {:?}", ack.code));
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(format!("{}", e)),
            }
        }
    })
    .await;

    match result {
        Ok(Ok(msg)) => MqttTestResult {
            success: true,
            message: msg,
        },
        Ok(Err(msg)) => MqttTestResult {
            success: false,
            message: msg,
        },
        Err(_) => MqttTestResult {
            success: false,
            message: "Connection timeout after 10 seconds".to_string(),
        },
    }
}

pub use config::MqttAdapterConfig;

// Domain submodules — impl blocks live across these files.
mod adapter;
pub(crate) mod config;
mod connection;
mod handling;
pub(crate) mod topics;
pub(crate) use topics::*;

#[cfg(test)]
mod tests;
