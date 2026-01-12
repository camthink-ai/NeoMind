//! Home Assistant integration for NeoTalk.
//!
//! This module provides functionality to discover, map, and synchronize devices
//! from Home Assistant into NeoTalk's device management system.
//!
//! ## Features
//!
//! - **REST API Client**: Interact with Home Assistant's REST API
//! - **WebSocket Client**: Real-time state synchronization via WebSocket
//! - **Entity Mapping**: Automatic mapping of HASS entities to NeoTalk devices
//! - **Device Templates**: Built-in templates for common device types
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_devices::hass::{HassClient, HassConnectionConfig, HassEntityMapper};
//! use edge_ai_storage::HassSettings;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create connection config
//!     let config = HassConnectionConfig::with_bearer_token(
//!         "http://localhost:8123".to_string(),
//!         "your_token_here".to_string(),
//!     );
//!
//!     // Create REST client
//!     let client = HassClient::new(config)?;
//!
//!     // Test connection
//!     let connected = client.test_connection().await?;
//!     println!("Connected: {}", connected);
//!
//!     // Get all entities
//!     let states = client.get_states().await?;
//!     println!("Found {} entities", states.len());
//!
//!     // Map entities to NeoTalk devices
//!     let mapper = HassEntityMapper::new();
//!     let mapped_devices = mapper.map_entities(states)?;
//!
//!     for device in mapped_devices {
//!         println!("Mapped device: {} ({})", device.device.name, device.device.id);
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod entities;
pub mod client;
pub mod websocket;
pub mod mapper;
pub mod templates;

// Re-exports for convenience
pub use entities::{
    HassAuth, HassConnectionConfig, HassEntityState, HassEntityAttributes,
    HassDeviceInfo, HassDomain, HassDeviceClass, HassServiceCall, HassEvent,
};
pub use client::{HassClient, HassClientError, HassResult};
pub use websocket::{
    HassWebSocketClient, HassWsError, HassWsResult, HassWsEventData,
    HassSubscription,
};
pub use mapper::{
    HassEntityMapper, EntityMapping, MetricMapping, CommandMapping,
    MappedDevice, MappedDeviceInfo, MappingError, MappingResult,
};
pub use templates::{HassDeviceTemplate, TemplateMetric, TemplateCommand, builtin_templates};

use serde::{Deserialize, Serialize};

// Re-export HassSettings from storage crate
pub use edge_ai_storage::HassSettings;
