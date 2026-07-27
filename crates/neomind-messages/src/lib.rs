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
//! - **Notification Channels**: Webhook, Email (extensible)
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

pub mod channels;
pub mod error;
pub mod manager;
pub mod message;

// Re-exports (only types used externally via crate-root shortcut path)
pub use channels::{
    get_channel_schema, list_channel_types, ChannelFactory, ChannelInfo, ChannelStats,
    MessageChannel,
};
pub use error::{Error, Result};
pub use manager::MessageManager;
pub use message::{Message, MessageId, MessageSeverity, MessageStatus};

// Feature-gated channel factories (used by API handler for channel registration)
#[cfg(feature = "dingtalk")]
pub use channels::DingTalkChannelFactory;
#[cfg(feature = "email")]
pub use channels::EmailChannelFactory;
#[cfg(feature = "feishu")]
pub use channels::FeishuChannelFactory;
#[cfg(feature = "slack")]
pub use channels::SlackChannelFactory;
#[cfg(feature = "telegram")]
pub use channels::TelegramChannelFactory;
#[cfg(feature = "wecom")]
pub use channels::WeComChannelFactory;
#[cfg(feature = "webhook")]
pub use channels::WebhookChannelFactory;

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
