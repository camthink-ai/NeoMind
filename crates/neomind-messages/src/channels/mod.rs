//! Notification channels for sending messages.

pub mod filter;

#[cfg(feature = "webhook")]
pub mod webhook;

#[cfg(feature = "email")]
pub mod email;

#[cfg(feature = "telegram")]
pub mod telegram;

#[cfg(feature = "wecom")]
pub mod wecom;

#[cfg(feature = "dingtalk")]
pub mod dingtalk;

#[cfg(feature = "slack")]
pub mod slack;

#[cfg(feature = "feishu")]
pub mod feishu;

pub use filter::ChannelFilter;

use async_trait::async_trait;
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{Error, Message, Result};

#[cfg(feature = "webhook")]
pub use webhook::{WebhookChannel, WebhookChannelFactory};

#[cfg(feature = "email")]
pub use email::{EmailChannel, EmailChannelFactory};

#[cfg(feature = "telegram")]
pub use telegram::{TelegramChannel, TelegramChannelFactory};

#[cfg(feature = "wecom")]
pub use wecom::{WeComChannel, WeComChannelFactory};

#[cfg(feature = "dingtalk")]
pub use dingtalk::{DingTalkChannel, DingTalkChannelFactory};

#[cfg(feature = "slack")]
pub use slack::{SlackChannel, SlackChannelFactory};

#[cfg(feature = "feishu")]
pub use feishu::{FeishuChannel, FeishuChannelFactory};

/// Trait for message channels.
#[async_trait]
pub trait MessageChannel: Send + Sync {
    /// Get the channel name.
    fn name(&self) -> &str;

    /// Get the channel type.
    fn channel_type(&self) -> &str;

    /// Check if the channel is enabled.
    fn is_enabled(&self) -> bool;

    /// Send a message through this channel.
    async fn send(&self, message: &Message) -> Result<()>;

    /// Get the channel configuration as JSON.
    fn get_config(&self) -> Option<serde_json::Value> {
        None
    }

    /// Set recipients for email channels (no-op for other channel types).
    fn set_recipients(&mut self, _recipients: Vec<String>) {}

    /// Get recipients for email channels (empty for other channel types).
    fn get_recipients(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Factory trait for creating message channels from configuration.
pub trait ChannelFactory: Send + Sync {
    /// Get the channel type this factory creates.
    fn channel_type(&self) -> &str;

    /// Create a channel from configuration.
    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>>;
}

/// Persistent channel configuration for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredChannelConfig {
    /// Channel name
    pub name: String,
    /// Channel type (webhook, email, etc.)
    pub channel_type: String,
    /// Channel configuration
    pub config: serde_json::Value,
    /// Whether the channel is enabled
    pub enabled: bool,
    /// Filter for message routing
    #[serde(default)]
    pub filter: ChannelFilter,
}

/// Combined in-memory channel state, protected by a single RwLock to avoid
/// lock contention when multiple maps must be accessed together.
struct ChannelState {
    channels: HashMap<String, Arc<dyn MessageChannel>>,
    configs: HashMap<String, serde_json::Value>,
    enabled_states: HashMap<String, bool>,
    recipients: HashMap<String, Vec<String>>,
}

/// Channel registry for managing notification channels.
pub struct ChannelRegistry {
    /// All in-memory channel data behind one lock (B12 fix).
    state: RwLock<ChannelState>,
    /// Persistent storage backend (redb).
    /// Only used to clone out the `Arc<Database>` and then dropped before I/O.
    storage: RwLock<Option<Arc<redb::Database>>>,
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelRegistry {
    /// Create a new in-memory channel registry (no persistence).
    pub fn new() -> Self {
        Self {
            state: RwLock::new(ChannelState {
                channels: HashMap::new(),
                configs: HashMap::new(),
                enabled_states: HashMap::new(),
                recipients: HashMap::new(),
            }),
            storage: RwLock::new(None),
        }
    }

    /// Create a channel registry with persistent storage.
    pub fn with_storage<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)
            .map_err(|e| Error::Storage(format!("Failed to create data directory: {}", e)))?;

        let db_path = data_dir.join("channels.redb");
        let db = redb::Database::create(&db_path)
            .map_err(|e| Error::Storage(format!("Failed to open channels database: {}", e)))?;

        // Create tables
        let write_txn = db
            .begin_write()
            .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;
        {
            write_txn
                .open_table(redb::TableDefinition::<&str, &str>::new("channels"))
                .map_err(|e| Error::Storage(format!("Failed to open channels table: {}", e)))?;
            write_txn
                .open_table(redb::TableDefinition::<&str, &str>::new("recipients"))
                .map_err(|e| Error::Storage(format!("Failed to open recipients table: {}", e)))?;
        }
        write_txn
            .commit()
            .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

        Ok(Self {
            state: RwLock::new(ChannelState {
                channels: HashMap::new(),
                configs: HashMap::new(),
                enabled_states: HashMap::new(),
                recipients: HashMap::new(),
            }),
            storage: RwLock::new(Some(Arc::new(db))),
        })
    }

    /// Load persisted channel configurations.
    /// Returns a list of stored channel configs that need to be recreated.
    pub async fn load_persisted(&self) -> Vec<StoredChannelConfig> {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::warn!("Failed to read channels from storage: {}", e);
                    return Vec::new();
                }
            };

            let table =
                match read_txn.open_table(redb::TableDefinition::<&str, &str>::new("channels")) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Failed to open channels table: {}", e);
                        return Vec::new();
                    }
                };

            let mut configs = Vec::new();
            let iter = match table.iter() {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("Failed to iterate channels: {}", e);
                    return Vec::new();
                }
            };

            for result in iter {
                let (_key, value) = match result {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("Failed to read channel entry: {}", e);
                        continue;
                    }
                };
                if let Ok(stored) = serde_json::from_str::<StoredChannelConfig>(value.value()) {
                    // Also load into memory
                    self.state
                        .write()
                        .await
                        .configs
                        .insert(stored.name.clone(), stored.config.clone());
                    configs.push(stored);
                }
            }

            tracing::info!("Loaded {} persisted channel configurations", configs.len());
            return configs;
        }
        Vec::new()
    }

    /// Save a channel configuration to persistent storage.
    /// Clones the `Arc<Database>` and drops the read lock before performing
    /// I/O, so the storage lock is not held across a write transaction (B13 fix).
    async fn save_channel(&self, stored: &StoredChannelConfig) -> Result<()> {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let json = serde_json::to_string(stored).map_err(|e| {
                Error::Storage(format!("Failed to serialize channel config: {}", e))
            })?;

            let write_txn = db
                .begin_write()
                .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

            {
                let mut table = write_txn
                    .open_table(redb::TableDefinition::<&str, &str>::new("channels"))
                    .map_err(|e| Error::Storage(format!("Failed to open channels table: {}", e)))?;
                table
                    .insert(stored.name.as_str(), json.as_str())
                    .map_err(|e| Error::Storage(format!("Failed to save channel config: {}", e)))?;
            }

            write_txn
                .commit()
                .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

            tracing::debug!("Saved channel configuration: {}", stored.name);
        }
        Ok(())
    }

    /// Delete a channel configuration from persistent storage.
    async fn delete_channel(&self, name: &str) -> Result<()> {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let write_txn = db
                .begin_write()
                .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

            {
                let mut table = write_txn
                    .open_table(redb::TableDefinition::<&str, &str>::new("channels"))
                    .map_err(|e| Error::Storage(format!("Failed to open channels table: {}", e)))?;
                table.remove(name).map_err(|e| {
                    Error::Storage(format!("Failed to delete channel config: {}", e))
                })?;
            }

            write_txn
                .commit()
                .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

            tracing::debug!("Deleted channel configuration: {}", name);
        }
        Ok(())
    }

    /// Register a channel instance.
    pub async fn register(&self, channel: Arc<dyn MessageChannel>) {
        let name = channel.name().to_string();
        self.state.write().await.channels.insert(name, channel);
    }

    /// Register a channel with its configuration (persists to storage).
    pub async fn register_with_config(
        &self,
        name: String,
        channel: Arc<dyn MessageChannel>,
        config: serde_json::Value,
    ) {
        let channel_type = channel.channel_type().to_string();
        let enabled = channel.is_enabled();

        // Save to memory
        {
            let mut state = self.state.write().await;
            state.channels.insert(name.clone(), channel);
            state.configs.insert(name.clone(), config.clone());
        }

        // Persist to storage
        let stored = StoredChannelConfig {
            name: name.clone(),
            channel_type,
            config,
            enabled,
            filter: ChannelFilter::default(),
        };
        if let Err(e) = self.save_channel(&stored).await {
            tracing::warn!("Failed to persist channel config: {}", e);
        }
    }

    /// Unregister a channel by name (also removes from persistent storage).
    pub async fn unregister(&self, name: &str) -> bool {
        let removed = {
            let mut state = self.state.write().await;
            state.channels.remove(name).is_some() || state.configs.remove(name).is_some()
        };

        if removed {
            // Remove from persistent storage
            if let Err(e) = self.delete_channel(name).await {
                tracing::warn!("Failed to delete channel from storage: {}", e);
            }
        }

        removed
    }

    /// Get a channel by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn MessageChannel>> {
        self.state.read().await.channels.get(name).cloned()
    }

    /// List all channel names.
    pub async fn list_names(&self) -> Vec<String> {
        self.state.read().await.channels.keys().cloned().collect()
    }

    /// Get the number of channels.
    pub async fn len(&self) -> usize {
        self.state.read().await.channels.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.state.read().await.channels.is_empty()
    }

    /// Get detailed information about a channel.
    pub async fn get_info(&self, name: &str) -> Option<ChannelInfo> {
        let state = self.state.read().await;
        state.channels.get(name).map(|channel| {
            // Check if there's an override in enabled_states, otherwise use channel's internal state
            let enabled = state
                .enabled_states
                .get(name)
                .copied()
                .unwrap_or_else(|| channel.is_enabled());
            let channel_type = channel.channel_type().to_string();
            // Include recipients for email channels
            let channel_recipients = if channel_type == "email" {
                state.recipients.get(name).cloned()
            } else {
                None
            };
            ChannelInfo {
                name: name.to_string(),
                channel_type,
                enabled,
                config: state.configs.get(name).cloned(),
                recipients: channel_recipients,
            }
        })
    }

    /// List all channels with info.
    pub async fn list_info(&self) -> Vec<ChannelInfo> {
        let state = self.state.read().await;
        state
            .channels
            .keys()
            .map(|name| {
                let channel = state.channels.get(name);
                let channel_type = channel
                    .map(|c| c.channel_type().to_string())
                    .unwrap_or_default();
                // Include recipients for email channels
                let channel_recipients = if channel_type == "email" {
                    state.recipients.get(name).cloned()
                } else {
                    None
                };
                ChannelInfo {
                    name: name.clone(),
                    channel_type: channel_type.clone(),
                    enabled: state
                        .enabled_states
                        .get(name)
                        .copied()
                        .unwrap_or_else(|| channel.map(|c| c.is_enabled()).unwrap_or(false)),
                    config: state.configs.get(name).cloned(),
                    recipients: channel_recipients,
                }
            })
            .collect()
    }

    /// Get channel statistics.
    pub async fn get_stats(&self) -> ChannelStats {
        let state = self.state.read().await;
        let mut by_type = HashMap::new();
        let mut enabled_count = 0;

        for (name, channel) in state.channels.iter() {
            let ct = channel.channel_type().to_string();
            *by_type.entry(ct).or_insert(0) += 1;
            // Check override first, then channel's internal state
            let is_enabled = state
                .enabled_states
                .get(name)
                .copied()
                .unwrap_or_else(|| channel.is_enabled());
            if is_enabled {
                enabled_count += 1;
            }
        }

        ChannelStats {
            total: state.channels.len(),
            enabled: enabled_count,
            disabled: state.channels.len() - enabled_count,
            by_type,
        }
    }

    /// Set the enabled state of a channel.
    /// This updates both the in-memory state and persists to storage.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        // Check if channel exists & gather data for persistence in one lock scope
        let persist_data = {
            let state = self.state.read().await;
            if !state.channels.contains_key(name) {
                return Err(Error::NotFound(format!("Channel not found: {}", name)));
            }
            // Collect what we need for persistence, then drop the lock before I/O
            let config = state.configs.get(name).cloned();
            let channel = state.channels.get(name).cloned();
            (config, channel)
        };

        // Update in-memory state
        {
            let mut state = self.state.write().await;
            state.enabled_states.insert(name.to_string(), enabled);
        }

        // Persist the enabled-state change.
        // IMPORTANT: we must preserve the existing filter — reading the
        // current stored config first and only overwriting the `enabled`
        // field. Earlier this constructed a fresh `StoredChannelConfig`
        // with `filter: ChannelFilter::default()`, which silently wiped
        // any user-configured filter every time the operator toggled the
        // channel's enabled flag. Since `get_filter` reads from disk on
        // every send, the loss was immediate and silent.
        if let (Some(config), Some(channel)) = persist_data {
            let existing_filter = self.get_filter(name).await;
            let stored = StoredChannelConfig {
                name: name.to_string(),
                channel_type: channel.channel_type().to_string(),
                config,
                enabled,
                filter: existing_filter,
            };
            self.save_channel(&stored).await?;
        }

        tracing::info!("Channel '{}' enabled state set to: {}", name, enabled);
        Ok(())
    }

    /// Effective enabled state for delivery routing.
    ///
    /// This consults the registry's `enabled_states` override FIRST (set by
    /// `set_enabled` / `PUT /channels/:name/enabled`), falling back to the
    /// channel object's own `is_enabled()` only when no override exists.
    ///
    /// `MessageManager::create_message` MUST route through this method
    /// instead of `channel.is_enabled()`. The channel struct's internal
    /// `enabled` field is set once at factory create time and is never
    /// mutated by `set_enabled`, so consulting it directly bypasses the
    /// operator's disable override — the UI would show "disabled" while
    /// alerts continued to fire.
    pub async fn is_enabled_effective(&self, name: &str) -> bool {
        let state = self.state.read().await;
        if let Some(enabled) = state.enabled_states.get(name) {
            return *enabled;
        }
        // No override registered: fall back to channel's own field.
        // This only happens for channels that were never toggled after
        // server start (e.g. registered without going through set_enabled).
        match state.channels.get(name) {
            Some(channel) => channel.is_enabled(),
            None => false,
        }
    }

    /// Test a channel by sending a test message.
    pub async fn test(&self, name: &str) -> Result<TestResult> {
        let channel = {
            let state = self.state.read().await;
            state
                .channels
                .get(name)
                .cloned()
                .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", name)))?
        };

        let test_message = Message::system_with_severity(
            crate::MessageSeverity::Info,
            "Test Message".to_string(),
            "This is a test message to verify the channel is working.".to_string(),
        );

        let start = std::time::Instant::now();
        match channel.send(&test_message).await {
            Ok(()) => Ok(TestResult {
                success: true,
                message: "Test message sent successfully".to_string(),
                message_zh: "测试消息发送成功".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(TestResult {
                success: false,
                message: format!("Failed to send test message: {}", e),
                message_zh: format!("发送测试消息失败: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }

    // ========== Recipient Management (for Email Channels) ==========

    /// Get recipients for a channel.
    pub async fn get_recipients(&self, channel_name: &str) -> Vec<String> {
        let state = self.state.read().await;
        state
            .recipients
            .get(channel_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Add a recipient to a channel.
    pub async fn add_recipient(&self, channel_name: &str, email: &str) -> Result<()> {
        // Check if channel exists and get its type
        let channel_type = {
            let state = self.state.read().await;
            let channel = state
                .channels
                .get(channel_name)
                .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_name)))?;
            channel.channel_type().to_string()
        };

        // Validate email format (basic check)
        if !email.contains('@') || email.is_empty() {
            return Err(Error::InvalidConfiguration(
                "Invalid email address".to_string(),
            ));
        }

        // Add to memory
        {
            let mut state = self.state.write().await;
            let channel_recipients = state
                .recipients
                .entry(channel_name.to_string())
                .or_default();
            if channel_recipients.contains(&email.to_string()) {
                return Err(Error::InvalidConfiguration(
                    "Recipient already exists".to_string(),
                ));
            }
            channel_recipients.push(email.to_string());
        }

        // Update the channel with new recipients first (recreate for email channels)
        // Do this before persisting so we can rollback on failure
        if channel_type == "email" {
            if let Err(e) = self.recreate_email_channel(channel_name).await {
                // Rollback: remove from memory
                let mut state = self.state.write().await;
                if let Some(channel_recipients) = state.recipients.get_mut(channel_name) {
                    channel_recipients.retain(|r| r != email);
                }
                return Err(e);
            }
        }

        // Persist to storage only after channel recreation succeeds
        self.save_recipients(channel_name).await?;

        tracing::info!("Added recipient '{}' to channel '{}'", email, channel_name);
        Ok(())
    }

    /// Remove a recipient from a channel.
    pub async fn remove_recipient(&self, channel_name: &str, email: &str) -> Result<()> {
        // Check if channel exists and get its type
        let channel_type = {
            let state = self.state.read().await;
            let channel = state
                .channels
                .get(channel_name)
                .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_name)))?;
            channel.channel_type().to_string()
        };

        // Remove from memory
        let removed = {
            let mut state = self.state.write().await;
            if let Some(channel_recipients) = state.recipients.get_mut(channel_name) {
                let initial_len = channel_recipients.len();
                channel_recipients.retain(|r| r != email);
                channel_recipients.len() < initial_len
            } else {
                false
            }
        };

        if !removed {
            return Err(Error::NotFound(format!("Recipient not found: {}", email)));
        }

        // Update the channel with new recipients first (recreate for email channels)
        if channel_type == "email" {
            if let Err(e) = self.recreate_email_channel(channel_name).await {
                // Rollback: add back to memory
                let mut state = self.state.write().await;
                let channel_recipients = state
                    .recipients
                    .entry(channel_name.to_string())
                    .or_default();
                channel_recipients.push(email.to_string());
                return Err(e);
            }
        }

        // Persist to storage only after channel recreation succeeds
        self.save_recipients(channel_name).await?;

        tracing::info!(
            "Removed recipient '{}' from channel '{}'",
            email,
            channel_name
        );
        Ok(())
    }

    /// Recreate an email channel with updated recipients.
    async fn recreate_email_channel(&self, channel_name: &str) -> Result<()> {
        let (config, enabled, recipients) = {
            let state = self.state.read().await;
            let channel = state
                .channels
                .get(channel_name)
                .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_name)))?;

            let config = state.configs.get(channel_name).cloned().ok_or_else(|| {
                Error::InvalidConfiguration("Channel config not found".to_string())
            })?;

            let enabled = state
                .enabled_states
                .get(channel_name)
                .copied()
                .unwrap_or_else(|| channel.is_enabled());

            let recipients = state
                .recipients
                .get(channel_name)
                .cloned()
                .unwrap_or_default();

            (config, enabled, recipients)
        };

        // Build new config with recipients
        let mut new_config = config.clone();
        if let Some(obj) = new_config.as_object_mut() {
            obj.insert("recipients".to_string(), serde_json::json!(recipients));
            obj.insert("enabled".to_string(), serde_json::json!(enabled));
            obj.insert("name".to_string(), serde_json::json!(channel_name));
        }

        // Create new channel using factory
        #[cfg(feature = "email")]
        {
            let factory = EmailChannelFactory;
            let new_channel = factory.create(&new_config)?;

            // Replace the old channel
            let mut state = self.state.write().await;
            state.channels.insert(channel_name.to_string(), new_channel);
        }

        tracing::debug!(
            "Recreated email channel '{}' with {} recipients",
            channel_name,
            recipients.len()
        );
        Ok(())
    }

    /// Save recipients for a channel to persistent storage.
    async fn save_recipients(&self, channel_name: &str) -> Result<()> {
        // Clone out the data we need, then drop all locks before I/O (B13 fix).
        let (db, channel_recipients) = {
            let db = {
                let storage = self.storage.read().await;
                storage.as_ref().cloned()
            };
            let channel_recipients = {
                let state = self.state.read().await;
                state
                    .recipients
                    .get(channel_name)
                    .cloned()
                    .unwrap_or_default()
            };
            (db, channel_recipients)
        };

        if let Some(db) = db {
            let json = serde_json::to_string(&channel_recipients)
                .map_err(|e| Error::Storage(format!("Failed to serialize recipients: {}", e)))?;

            let write_txn = db
                .begin_write()
                .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

            {
                let mut table = write_txn
                    .open_table(redb::TableDefinition::<&str, &str>::new("recipients"))
                    .map_err(|e| {
                        Error::Storage(format!("Failed to open recipients table: {}", e))
                    })?;
                table
                    .insert(channel_name, json.as_str())
                    .map_err(|e| Error::Storage(format!("Failed to save recipients: {}", e)))?;
            }

            write_txn
                .commit()
                .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;
        }
        Ok(())
    }

    /// Load recipients for a channel from persistent storage.
    pub async fn load_recipients(&self, channel_name: &str) {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::warn!("Failed to read recipients: {}", e);
                    return;
                }
            };

            let table =
                match read_txn.open_table(redb::TableDefinition::<&str, &str>::new("recipients")) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::debug!("Recipients table not found or empty: {}", e);
                        return;
                    }
                };

            if let Ok(Some(value)) = table.get(channel_name) {
                if let Ok(loaded) = serde_json::from_str::<Vec<String>>(value.value()) {
                    let mut state = self.state.write().await;
                    state.recipients.insert(channel_name.to_string(), loaded);
                    tracing::debug!("Loaded recipients for channel '{}'", channel_name);
                }
            }
        }
    }

    /// Load all recipients from storage.
    pub async fn load_all_recipients(&self) {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::warn!("Failed to read recipients: {}", e);
                    return;
                }
            };

            let table =
                match read_txn.open_table(redb::TableDefinition::<&str, &str>::new("recipients")) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::debug!("Recipients table not found: {}", e);
                        return;
                    }
                };

            let iter = match table.iter() {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("Failed to iterate recipients: {}", e);
                    return;
                }
            };

            let mut count = 0;
            let mut state = self.state.write().await;
            for (key, value) in iter.flatten() {
                if let Ok(loaded) = serde_json::from_str::<Vec<String>>(value.value()) {
                    state.recipients.insert(key.value().to_string(), loaded);
                    count += 1;
                }
            }

            tracing::info!("Loaded recipients for {} channels", count);
        }
    }

    // ========== Filter Management ==========

    /// Get the filter for a channel (returns default filter if not found).
    pub async fn get_filter(&self, channel_name: &str) -> ChannelFilter {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::debug!("Failed to read channel filter: {}", e);
                    return ChannelFilter::default();
                }
            };

            let table =
                match read_txn.open_table(redb::TableDefinition::<&str, &str>::new("channels")) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::debug!("Channels table not found: {}", e);
                        return ChannelFilter::default();
                    }
                };

            if let Ok(Some(value)) = table.get(channel_name) {
                if let Ok(stored) = serde_json::from_str::<StoredChannelConfig>(value.value()) {
                    return stored.filter;
                }
            }
        }
        ChannelFilter::default()
    }

    /// Set the filter for a channel.
    pub async fn set_filter(&self, channel_name: &str, filter: ChannelFilter) -> Result<()> {
        let db = {
            let storage = self.storage.read().await;
            storage.as_ref().cloned()
        };
        if let Some(db) = db {
            // Read existing config
            let read_txn = db
                .begin_read()
                .map_err(|e| Error::Storage(format!("Failed to begin read: {}", e)))?;

            let table = read_txn
                .open_table(redb::TableDefinition::<&str, &str>::new("channels"))
                .map_err(|e| Error::Storage(format!("Failed to open channels table: {}", e)))?;

            let existing = table
                .get(channel_name)
                .map_err(|e| Error::Storage(format!("Failed to read channel: {}", e)))?;

            if let Some(value) = existing {
                let mut stored: StoredChannelConfig =
                    serde_json::from_str(value.value()).map_err(|e| {
                        Error::Storage(format!("Failed to deserialize channel config: {}", e))
                    })?;

                // Update filter
                stored.filter = filter;

                // Save updated config
                let json = serde_json::to_string(&stored).map_err(|e| {
                    Error::Storage(format!("Failed to serialize channel config: {}", e))
                })?;

                let write_txn = db
                    .begin_write()
                    .map_err(|e| Error::Storage(format!("Failed to begin write: {}", e)))?;

                {
                    let mut table = write_txn
                        .open_table(redb::TableDefinition::<&str, &str>::new("channels"))
                        .map_err(|e| {
                            Error::Storage(format!("Failed to open channels table: {}", e))
                        })?;
                    table.insert(channel_name, json.as_str()).map_err(|e| {
                        Error::Storage(format!("Failed to save channel filter: {}", e))
                    })?;
                }

                write_txn
                    .commit()
                    .map_err(|e| Error::Storage(format!("Failed to commit: {}", e)))?;

                tracing::info!("Updated filter for channel '{}'", channel_name);
            }
        }
        Ok(())
    }
}

/// Information about a registered channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Channel name (unique identifier)
    pub name: String,
    /// Channel type (console, memory, webhook, email)
    pub channel_type: String,
    /// Whether the channel is enabled
    pub enabled: bool,
    /// Channel configuration (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    /// Recipients for email channels (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipients: Option<Vec<String>>,
}

/// Channel statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStats {
    /// Total number of channels
    pub total: usize,
    /// Number of enabled channels
    pub enabled: usize,
    /// Number of disabled channels
    pub disabled: usize,
    /// Channels grouped by type
    pub by_type: HashMap<String, usize>,
}

/// Result of a channel test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Whether the test was successful
    pub success: bool,
    /// Result message (English)
    pub message: String,
    /// Result message (Chinese)
    pub message_zh: String,
    /// Time taken for the test in milliseconds
    pub duration_ms: u64,
}

/// Channel type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelTypeInfo {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub description: String,
    pub description_zh: String,
    pub icon: String,
    pub category: String,
}

/// List all available channel types.
pub fn list_channel_types() -> Vec<ChannelTypeInfo> {
    vec![
        #[cfg(feature = "webhook")]
        ChannelTypeInfo {
            id: "webhook".to_string(),
            name: "Webhook".to_string(),
            name_zh: "Webhook".to_string(),
            description: "Send messages via HTTP POST to a webhook URL".to_string(),
            description_zh: "通过 HTTP POST 将消息发送到 Webhook URL".to_string(),
            icon: "webhook".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "email")]
        ChannelTypeInfo {
            id: "email".to_string(),
            name: "Email".to_string(),
            name_zh: "邮件".to_string(),
            description: "Send messages via email".to_string(),
            description_zh: "通过邮件发送消息".to_string(),
            icon: "mail".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "telegram")]
        ChannelTypeInfo {
            id: "telegram".to_string(),
            name: "Telegram".to_string(),
            name_zh: "Telegram".to_string(),
            description: "Send messages via Telegram Bot API".to_string(),
            description_zh: "通过 Telegram 机器人发送消息".to_string(),
            icon: "send".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "wecom")]
        ChannelTypeInfo {
            id: "wecom".to_string(),
            name: "WeCom".to_string(),
            name_zh: "企业微信".to_string(),
            description: "Send messages via WeCom robot webhook".to_string(),
            description_zh: "通过企业微信机器人发送消息".to_string(),
            icon: "message-square".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "dingtalk")]
        ChannelTypeInfo {
            id: "dingtalk".to_string(),
            name: "DingTalk".to_string(),
            name_zh: "钉钉".to_string(),
            description: "Send messages via DingTalk custom robot".to_string(),
            description_zh: "通过钉钉自定义机器人发送消息".to_string(),
            icon: "message-circle".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "slack")]
        ChannelTypeInfo {
            id: "slack".to_string(),
            name: "Slack".to_string(),
            name_zh: "Slack".to_string(),
            description: "Send messages via Slack Incoming Webhook".to_string(),
            description_zh: "通过 Slack Incoming Webhook 发送消息".to_string(),
            icon: "hash".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "feishu")]
        ChannelTypeInfo {
            id: "feishu".to_string(),
            name: "Feishu".to_string(),
            name_zh: "飞书".to_string(),
            description: "Send messages via Feishu custom bot".to_string(),
            description_zh: "通过飞书自定义机器人发送消息".to_string(),
            icon: "messages-square".to_string(),
            category: "external".to_string(),
        },
    ]
}

/// Get channel type configuration schema.
pub fn get_channel_schema(channel_type: &str) -> Option<serde_json::Value> {
    match channel_type {
        #[cfg(feature = "webhook")]
        "webhook" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "url": {"type": "string"},
                "headers": {"type": "object"},
                "timeout_secs": {"type": "integer", "description": "Request timeout in seconds (default: 30)"}
            },
            "required": ["url"]
        })),
        #[cfg(feature = "email")]
        "email" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "smtp_server": {"type": "string"},
                "smtp_port": {"type": "integer"},
                "username": {"type": "string"},
                "password": {"type": "string"},
                "from_address": {"type": "string"},
                "use_tls": {"type": "boolean"}
            },
            "required": ["smtp_server", "smtp_port", "username", "password", "from_address"]
        })),
        #[cfg(feature = "telegram")]
        "telegram" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "token": {"type": "string", "description": "Telegram Bot API token"},
                "chat_id": {"type": "string", "description": "Target chat ID"}
            },
            "required": ["token", "chat_id"]
        })),
        #[cfg(feature = "wecom")]
        "wecom" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "key": {"type": "string", "description": "WeCom robot webhook key"}
            },
            "required": ["key"]
        })),
        #[cfg(feature = "dingtalk")]
        "dingtalk" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "access_token": {"type": "string", "description": "DingTalk robot access token"},
                "secret": {"type": "string", "description": "Secret for sign verification (optional)"}
            },
            "required": ["access_token"]
        })),
        #[cfg(feature = "slack")]
        "slack" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "webhook_url": {"type": "string", "description": "Slack Incoming Webhook URL"}
            },
            "required": ["webhook_url"]
        })),
        #[cfg(feature = "feishu")]
        "feishu" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "hook_id": {"type": "string", "description": "Feishu bot hook ID"},
                "secret": {"type": "string", "description": "Secret for sign verification (optional)"}
            },
            "required": ["hook_id"]
        })),
        _ => None,
    }
}

/// Detect an error signal in a webhook/channel response body.
///
/// Many webhook APIs (Feishu, DingTalk, WeCom, …) return HTTP 200 with an error
/// code in the JSON body for semantic errors (invalid payload, disabled bot, bad
/// token). Checking HTTP status alone hides these as false "success" — the
/// channel-test reports success but no message actually arrives. Returns
/// `Some(error description)` when the body signals an error, `None` otherwise.
pub(crate) fn detect_error_body(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    let obj = v.as_object()?;
    // Feishu/DingTalk/WeCom style: {"code": N, "msg": "..."} — code != 0 is an error.
    if let Some(code) = obj.get("code").and_then(|c| c.as_i64()) {
        if code != 0 {
            let msg = obj.get("msg").and_then(|m| m.as_str()).unwrap_or("");
            return Some(format!("code {code}: {msg}"));
        }
    }
    // Generic style: {"success": false} / {"ok": false}
    if matches!(obj.get("success").and_then(|s| s.as_bool()), Some(false)) {
        return Some("body reports success=false".to_string());
    }
    if matches!(obj.get("ok").and_then(|s| s.as_bool()), Some(false)) {
        return Some("body reports ok=false".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_error_body_flags_feishu_code() {
        assert_eq!(detect_error_body(r#"{"code":0,"msg":"success"}"#), None);
        let e = detect_error_body(r#"{"code":19001,"msg":"invalid msg_type"}"#).unwrap();
        assert!(e.contains("19001") && e.contains("invalid msg_type"), "{e}");
    }

    #[test]
    fn detect_error_body_flags_generic_false() {
        assert!(detect_error_body(r#"{"success":false}"#).is_some());
        assert!(detect_error_body(r#"{"ok":false}"#).is_some());
        assert_eq!(detect_error_body(r#"{"success":true}"#), None);
    }

    #[test]
    fn detect_error_body_ignores_non_error_bodies() {
        assert_eq!(detect_error_body(""), None);
        assert_eq!(detect_error_body("not json"), None);
        assert_eq!(detect_error_body(r#"{"data":{"x":1}}"#), None);
    }

    /// Mock channel for testing purposes only.
    struct MockChannel {
        name: String,
        enabled: bool,
    }

    impl MockChannel {
        fn new(name: String) -> Self {
            Self {
                name,
                enabled: true,
            }
        }
    }

    #[async_trait]
    impl MessageChannel for MockChannel {
        fn name(&self) -> &str {
            &self.name
        }

        fn channel_type(&self) -> &str {
            "mock"
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }

        async fn send(&self, _message: &Message) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ChannelRegistry::new();
        assert!(registry.is_empty().await);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_register_channel() {
        let registry = ChannelRegistry::new();
        let channel = Arc::new(MockChannel::new("test".to_string()));

        registry.register(channel).await;

        assert_eq!(registry.len().await, 1);
        assert!(registry.get("test").await.is_some());
    }

    #[tokio::test]
    async fn test_unregister_channel() {
        let registry = ChannelRegistry::new();
        let channel = Arc::new(MockChannel::new("test".to_string()));

        registry.register(channel).await;
        assert_eq!(registry.len().await, 1);

        let removed = registry.unregister("test").await;
        assert!(removed);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_list_names() {
        let registry = ChannelRegistry::new();

        registry
            .register(Arc::new(MockChannel::new("ch1".to_string())))
            .await;
        registry
            .register(Arc::new(MockChannel::new("ch2".to_string())))
            .await;

        let names = registry.list_names().await;
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ch1".to_string()));
        assert!(names.contains(&"ch2".to_string()));
    }

    #[tokio::test]
    async fn test_channel_stats() {
        let registry = ChannelRegistry::new();

        registry
            .register(Arc::new(MockChannel::new("ch1".to_string())))
            .await;
        registry
            .register(Arc::new(MockChannel::new("ch2".to_string())))
            .await;

        let stats = registry.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.enabled, 2);
    }

    #[test]
    fn test_list_channel_types() {
        let types = list_channel_types();
        // Only webhook and email should be available (when features enabled)
        #[cfg(feature = "webhook")]
        assert!(types.iter().any(|t| t.id == "webhook"));
        #[cfg(feature = "email")]
        assert!(types.iter().any(|t| t.id == "email"));
        // console and memory should NOT be available
        assert!(!types.iter().any(|t| t.id == "console"));
        assert!(!types.iter().any(|t| t.id == "memory"));
    }

    #[test]
    fn test_get_channel_schema() {
        #[cfg(feature = "webhook")]
        {
            let schema = get_channel_schema("webhook");
            assert!(schema.is_some());
        }
        #[cfg(feature = "email")]
        {
            let schema = get_channel_schema("email");
            assert!(schema.is_some());
        }

        let schema = get_channel_schema("invalid");
        assert!(schema.is_none());

        // console and memory should NOT have schemas
        let schema = get_channel_schema("console");
        assert!(schema.is_none());
        let schema = get_channel_schema("memory");
        assert!(schema.is_none());
    }
}
