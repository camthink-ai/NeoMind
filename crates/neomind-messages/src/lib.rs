//! Message system for NeoMind.
//!
//! Messages are notifications that can be alerts, system messages,
//! or business events.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `webhook` | ❌ | Webhook notification channel |
//! | `email` | ❌ | Email notification channel via SMTP |
//!
//! ## Message Management
//!
//! - **Message Management**: Create, track, acknowledge, and resolve messages
//! - **Categories**: Alert, System, Business
//! - **Severity Levels**: Info, Warning, Critical, Emergency
//! - **Notification Channels**: Console, Memory, Webhook, Email
//! - **Plugin System**: Extensible channel architecture
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_messages::{MessageManager, MessageSeverity};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = MessageManager::new();
//!
//!     // Create an alert message
//!     let message = neomind_messages::Message::alert(
//!         MessageSeverity::Critical,
//!         "High Temperature".to_string(),
//!         "Temperature exceeded 80°C".to_string(),
//!         "sensor_1".to_string(),
//!     );
//!
//!     manager.create_message(message).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod message;
pub mod manager;
pub mod channels;
pub mod category;
pub mod error;

pub use message::{Message, MessageId, MessageSeverity, MessageStatus};
pub use manager::{MessageManager, MessageStats};
pub use category::MessageCategory;
pub use channels::{MessageChannel, ChannelRegistry, ConsoleChannel, MemoryChannel};
pub use error::{Error, Result};

// Channel factory exports
pub use channels::{ConsoleChannelFactory, MemoryChannelFactory};

// Conditional exports for feature-gated channels
#[cfg(feature = "webhook")]
pub use channels::{WebhookChannel, WebhookChannelFactory};

#[cfg(feature = "email")]
pub use channels::{EmailChannel, EmailChannelFactory};

// Channel info exports (always available)
pub use channels::{ChannelInfo, ChannelStats, ChannelTypeInfo, TestResult, ChannelFactory};

// Re-export channel helper functions
pub use channels::{list_channel_types, get_channel_schema};

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
