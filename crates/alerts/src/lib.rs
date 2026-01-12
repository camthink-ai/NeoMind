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
//! - **Alert Rules**: Automatic alert generation based on conditions
//! - **Alert History**: Track all alerts with configurable retention
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_alerts::{AlertManager, AlertSeverity, channels::MemoryChannel, channels::NotificationChannel};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = AlertManager::new();
//!
//!     // Add a notification channel
//!     let channel = NotificationChannel::Memory(MemoryChannel::new("memory".to_string()));
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
//!     println!("Alert created: {}", alert.summary());
//!
//!     // Later, resolve the alert
//!     manager.resolve(&alert.id).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod alert;
pub mod channels;
pub mod error;
pub mod manager;

pub use alert::{Alert, AlertId, AlertSeverity, AlertStatus};
pub use channels::{ChannelType, NotificationChannel, ChannelRegistry, MemoryChannel, ConsoleChannel};
pub use error::{Error, Result, NeoTalkError};
pub use manager::{AlertManager, AlertStats, AlertRule, AlwaysTrueRule, AlwaysFalseRule, CustomRule};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::alerts::{
    Alert as CoreAlert, AlertChannel, ChannelFactory, AlertError, AlertSeverity as CoreSeverity,
    AlertStatus as CoreStatus,
};

#[cfg(feature = "email")]
pub use channels::{EmailChannel, EmailAttachment};

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
