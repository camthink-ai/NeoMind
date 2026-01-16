//! Edge AI Alerts Crate
//!
//! This crate provides alert management and notification channels for the NeoTalk platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `webhook` | ❌ | Webhook notification channel |
//! | `email` | ❌ | Email notification channel via SMTP |
//!
//! ## Alert Management
//!
//! - **Alert Management**: Create, track, and resolve alerts
//! - **Severity Levels**: Info, Warning, Critical, Emergency
//! - **Notification Channels**: Console, Memory, Webhook (with feature), Email (with feature)
//! - **Plugin System**: Extensible channel architecture with trait-based plugins
//! - **Alert Rules**: Automatic alert generation based on conditions
//! - **Alert History**: Track all alerts with configurable retention
//!
//! ## Plugin System
//!
//! The notification system uses a trait-based plugin architecture:
//!
//! ```rust,no_run
//! use edge_ai_alerts::{AlertManager, plugin::ChannelPluginRegistry, channels::ConsoleChannel};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let registry = ChannelPluginRegistry::new();
//!
//!     // Register a channel instance directly
//!     let channel = std::sync::Arc::new(ConsoleChannel::new("console".to_string()));
//!     registry.register_channel("console".to_string(), channel).await;
//!
//!     // Or create a channel from configuration using the factory
//!     use edge_ai_alerts::channels::ConsoleChannelFactory;
//!     use edge_ai_core::alerts::ChannelFactory;
//!
//!     let factory = ConsoleChannelFactory;
//!     let config = serde_json::json!({
//!         "name": "console2",
//!         "include_details": true
//!     });
//!     let channel2 = factory.create(&config)?;
//!     registry.register_channel("console2".to_string(), channel2).await;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_alerts::{AlertManager, AlertSeverity, channels::ConsoleChannel};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = AlertManager::new();
//!
//!     // Add a notification channel
//!     let channel = std::sync::Arc::new(ConsoleChannel::new("console".to_string()));
//!     manager.add_channel(channel).await;
//!
//!     // Create an alert
//!     let alert = manager
//!         .device_alert(
//!             AlertSeverity::Critical,
//!             "High Temperature".to_string(),
//!             "Temperature exceeded 80°C".to_string(),
//!             "sensor_1".to_string(),
//!         )
//!         .await?;
//!
//!     println!("Alert created: {}", alert.id);
//!
//!     // Later, resolve the alert
//!     manager.resolve(&alert.id).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod alert;
pub mod channel_schema;
pub mod channels;
pub mod error;
pub mod manager;
pub mod plugin;
pub mod plugin_adapter;

pub use alert::{Alert, AlertId, AlertSeverity, AlertStatus};
pub use channels::{ConsoleChannel, ConsoleChannelFactory, MemoryChannel, MemoryChannelFactory};
#[cfg(feature = "email")]
pub use channels::EmailChannelFactory;
#[cfg(feature = "webhook")]
pub use channels::WebhookChannelFactory;
pub use error::{Error, NeoTalkError, Result};
pub use manager::{
    AlertManager, AlertRule, AlertStats, AlwaysFalseRule, AlwaysTrueRule, CustomRule,
};
pub use channel_schema::{list_channel_types as list_channel_types, ChannelTypeInfo};
pub use plugin::{ChannelInfo, ChannelPluginRegistry, ChannelStats, DynAlertChannel, IntoDynChannel, TestResult};
pub use plugin_adapter::{
    alert_channel_to_unified_plugin, get_builtin_channel_types, AlertChannelPluginFactory,
    AlertChannelTypeDefinition, AlertChannelUnifiedPlugin, DynAlertChannelPlugin, TestResult as PluginTestResult,
};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::alerts::{
    Alert as CoreAlert, AlertChannel, AlertError, AlertSeverity as CoreSeverity,
    AlertStatus as CoreStatus, ChannelFactory,
};

// Re-export legacy types for backward compatibility
#[cfg(feature = "email")]
pub use channels::EmailAttachment;

#[cfg(feature = "webhook")]
pub use channels::WebhookChannel;

#[cfg(feature = "email")]
pub use channels::EmailChannel;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
