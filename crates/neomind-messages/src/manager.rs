//! Message manager with persistent storage.
//!
//! Provides a unified interface for creating, tracking, and managing messages
//! with persistent storage using redb.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::channels::{ChannelFactory, ChannelFilter, ChannelRegistry};
use super::error::{Error, Result};
use super::{Message, MessageId, MessageSeverity, MessageStatus};

/// Minimum interval (in seconds) between duplicate messages with the same
/// title + source + severity key. Prevents message bombing from rules engine.
const DEDUP_INTERVAL_SECS: i64 = 60;

/// Persistent message manager with storage backend.
#[derive(Clone)]
pub struct MessageManager {
    /// In-memory cache for fast access
    messages: Arc<RwLock<HashMap<MessageId, Message>>>,
    /// Persistent storage backend
    storage: Arc<RwLock<Option<Arc<neomind_storage::MessageStore>>>>,
    /// Notification channels
    channels: Arc<RwLock<ChannelRegistry>>,
    /// Optional event bus for publishing message events
    event_bus: Arc<RwLock<Option<Arc<neomind_core::EventBus>>>>,
    /// Deduplication cache: (title, source, severity) -> last send timestamp
    dedup_cache: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
}

impl Default for MessageManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageManager {
    /// Create a new in-memory message manager (no persistence).
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(HashMap::new())),
            storage: Arc::new(RwLock::new(None)),
            channels: Arc::new(RwLock::new(ChannelRegistry::new())),
            event_bus: Arc::new(RwLock::new(None)),
            dedup_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new message manager with persistent storage.
    ///
    /// # Arguments
    /// * `data_dir` - Directory path where the database file will be stored.
    ///   The actual database file will be `{data_dir}/messages.redb`
    pub fn with_storage<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)
            .map_err(|e| Error::Storage(format!("Failed to create data directory: {}", e)))?;

        // Construct the database file path
        let db_path = data_dir.join("messages.redb");

        let store = Arc::new(neomind_storage::MessageStore::open(&db_path).map_err(|e| {
            Error::Storage(format!(
                "Failed to open message store at {:?}: {}",
                db_path, e
            ))
        })?);

        // Load existing messages into memory
        let mut messages = HashMap::new();
        match store.list() {
            Ok(stored_msgs) => {
                tracing::info!("Loading {} messages from storage", stored_msgs.len());
                for stored_msg in stored_msgs {
                    match MessageId::from_string(&stored_msg.id) {
                        Ok(id) => {
                            let msg = Self::stored_to_message(stored_msg);
                            messages.insert(id, msg);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse message ID '{}': {}", stored_msg.id, e);
                        }
                    }
                }
                tracing::info!(
                    "Successfully loaded {} messages into memory",
                    messages.len()
                );
            }
            Err(e) => {
                tracing::error!("Failed to load messages from storage: {}", e);
            }
        }

        // Create persistent channel registry
        let channels = ChannelRegistry::with_storage(data_dir)
            .map_err(|e| Error::Storage(format!("Failed to create channel registry: {}", e)))?;

        Ok(Self {
            messages: Arc::new(RwLock::new(messages)),
            storage: Arc::new(RwLock::new(Some(store))),
            channels: Arc::new(RwLock::new(channels)),
            event_bus: Arc::new(RwLock::new(None)),
            dedup_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Load persisted channel configurations.
    /// This should be called after creating the MessageManager to restore
    /// previously saved channels.
    pub async fn load_persisted_channels(&self) {
        let channels = self.channels.read().await;
        let configs = channels.load_persisted().await;
        // Need to drop the read lock before acquiring write lock
        drop(channels);

        // First, load all recipients from storage
        {
            let registry = self.channels.read().await;
            registry.load_all_recipients().await;
        }

        // Recreate and register channels from persisted configs
        let mut loaded_count = 0;
        for stored in configs {
            // Add enabled state to config for factory
            let mut config = stored.config.clone();
            if let Some(obj) = config.as_object_mut() {
                obj.insert("enabled".to_string(), serde_json::json!(stored.enabled));
                obj.insert("name".to_string(), serde_json::json!(stored.name));
            }

            // Add recipients to config for email channels
            if stored.channel_type == "email" {
                let registry = self.channels.read().await;
                let channel_recipients = registry.get_recipients(&stored.name).await;
                drop(registry);
                if !channel_recipients.is_empty() {
                    if let Some(obj) = config.as_object_mut() {
                        obj.insert(
                            "recipients".to_string(),
                            serde_json::json!(channel_recipients),
                        );
                    }
                }
            }

            // Create channel using factory based on type
            let channel_result = match stored.channel_type.as_str() {
                #[cfg(feature = "webhook")]
                "webhook" => {
                    let factory = crate::WebhookChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "email")]
                "email" => {
                    let factory = crate::EmailChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "telegram")]
                "telegram" => {
                    let factory = crate::TelegramChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "wecom")]
                "wecom" => {
                    let factory = crate::WeComChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "dingtalk")]
                "dingtalk" => {
                    let factory = crate::DingTalkChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "slack")]
                "slack" => {
                    let factory = crate::SlackChannelFactory;
                    factory.create(&config).map(Some)
                }
                #[cfg(feature = "feishu")]
                "feishu" => {
                    let factory = crate::FeishuChannelFactory;
                    factory.create(&config).map(Some)
                }
                _ => {
                    tracing::warn!("Unknown channel type: {}, skipping", stored.channel_type);
                    Ok(None)
                }
            };

            match channel_result {
                Ok(Some(channel)) => {
                    let registry = self.channels.write().await;
                    registry
                        .register_with_config(stored.name.clone(), channel, stored.config)
                        .await;
                    // Set enabled state if different from default
                    if !stored.enabled {
                        let _ = registry.set_enabled(&stored.name, false).await;
                    }
                    // Restore filter configuration if not default
                    if stored.filter != ChannelFilter::default() {
                        if let Err(e) = registry
                            .set_filter(&stored.name, stored.filter.clone())
                            .await
                        {
                            tracing::warn!(
                                "Failed to restore filter for channel '{}': {}",
                                stored.name,
                                e
                            );
                        }
                    }
                    loaded_count += 1;
                    tracing::info!(
                        "Restored channel: {} (type: {}, enabled: {})",
                        stored.name,
                        stored.channel_type,
                        stored.enabled
                    );
                }
                Ok(None) => {
                    // Unknown channel type, already logged
                }
                Err(e) => {
                    tracing::warn!("Failed to recreate channel '{}': {}", stored.name, e);
                }
            }
        }

        if loaded_count > 0 {
            tracing::info!("Successfully restored {} persisted channels", loaded_count);
        }
    }

    /// Convert StoredMessage to Message.
    fn stored_to_message(stored: neomind_storage::StoredMessage) -> Message {
        Message {
            id: MessageId::from_string(&stored.id).unwrap_or_else(|_| MessageId::new()),
            category: stored.category,
            severity: MessageSeverity::from_string(&stored.severity)
                .unwrap_or(MessageSeverity::Info),
            title: stored.title,
            message: stored.message,
            source: stored.source,
            source_type: stored.source_type.unwrap_or_else(|| "system".to_string()),
            timestamp: chrono::DateTime::from_timestamp(stored.timestamp, 0)
                .unwrap_or_else(chrono::Utc::now),
            status: MessageStatus::from_string(&stored.status).unwrap_or(MessageStatus::Active),
            metadata: stored.metadata,
            tags: stored.tags.unwrap_or_default(),
        }
    }

    /// Convert Message to StoredMessage.
    fn message_to_stored(msg: &Message) -> neomind_storage::StoredMessage {
        neomind_storage::StoredMessage {
            id: msg.id.to_string(),
            category: msg.category.clone(),
            severity: msg.severity.as_str().to_string(),
            title: msg.title.clone(),
            message: msg.message.clone(),
            source: msg.source.clone(),
            source_type: Some(msg.source_type.clone()),
            status: msg.status.as_str().to_string(),
            tags: if msg.tags.is_empty() {
                None
            } else {
                Some(msg.tags.clone())
            },
            metadata: msg.metadata.clone(),
            timestamp: msg.timestamp.timestamp(),
            acknowledged_at: None,
            resolved_at: None,
            acknowledged_by: None,
        }
    }

    /// Set the event bus for publishing message events.
    pub async fn set_event_bus(&self, event_bus: Arc<neomind_core::EventBus>) {
        *self.event_bus.write().await = Some(event_bus);
    }

    /// Get the channel registry.
    pub async fn channels(&self) -> Arc<RwLock<ChannelRegistry>> {
        self.channels.clone()
    }

    /// Register default channels (currently none - users should create webhook/email channels manually).
    pub async fn register_default_channels(&self) {
        // No default channels registered.
        // Users should create webhook/email channels through the API or UI.
        tracing::info!("MessageManager initialized with no default channels");
    }

    /// Create and send a message.
    pub async fn create_message(&self, message: Message) -> Result<Message> {
        let id = message.id.clone();
        let _is_active = message.is_active();
        let severity = message.severity;

        // Deduplication: skip if same title+source+severity was sent recently
        let dedup_key = format!(
            "{}|{}|{}",
            message.title,
            message.source,
            message.severity.as_str()
        );
        {
            let cache = self.dedup_cache.read().await;
            if let Some(last_sent) = cache.get(&dedup_key) {
                let elapsed = (chrono::Utc::now() - *last_sent).num_seconds();
                if elapsed < DEDUP_INTERVAL_SECS {
                    tracing::debug!(
                        "Skipping duplicate message '{}' (same key sent {}s ago)",
                        message.title,
                        elapsed
                    );
                    // Still store the message, just don't send to channels
                    self.messages
                        .write()
                        .await
                        .insert(id.clone(), message.clone());
                    if let Some(store) = self.storage.read().await.as_ref() {
                        let stored = Self::message_to_stored(&message);
                        let _ = store.insert_async(stored).await;
                    }
                    return Ok(message);
                }
            }
        }

        // Store in memory for all message types (Notification and DataPush)
        self.messages
            .write()
            .await
            .insert(id.clone(), message.clone());

        // Persist to storage if available (for all message types)
        if let Some(store) = self.storage.read().await.as_ref() {
            let stored = Self::message_to_stored(&message);
            store
                .insert_async(stored)
                .await
                .map_err(|e| Error::Storage(format!("Failed to persist message: {}", e)))?;
        }

        // Send through channels (don't fail if channels fail - message is already stored)
        let channels = self.channels.read().await;
        let channel_names = channels.list_names().await;
        let mut send_results: Vec<(String, std::result::Result<(), String>)> = Vec::new();

        for channel_name in &channel_names {
            if let Some(channel) = channels.get(channel_name).await {
                // Use the registry's effective enabled state — this honors
                // the `set_enabled` override (`PUT /channels/:name/enabled`).
                // Calling `channel.is_enabled()` directly would bypass the
                // override and keep delivering to "disabled" channels.
                if channels.is_enabled_effective(channel_name).await {
                    // Apply filter before sending
                    let filter = channels.get_filter(channel_name).await;
                    if !filter.matches(&message) {
                        tracing::debug!(
                            "Channel '{}' filter rejected message '{}'",
                            channel_name,
                            message.title
                        );
                        continue;
                    }

                    tracing::info!(
                        "Sending message through channel '{}' (type: {})",
                        channel_name,
                        channel.channel_type()
                    );

                    match channel.send(&message).await {
                        Ok(()) => {
                            tracing::info!(
                                "Successfully sent message through channel '{}'",
                                channel_name
                            );
                            send_results.push((channel_name.clone(), Ok(())));
                        }
                        Err(e) => {
                            // Log channel failure but don't fail the entire operation
                            tracing::warn!(
                                "Failed to send message through channel '{}': {}",
                                channel_name,
                                e
                            );
                            send_results.push((channel_name.clone(), Err(e.to_string())));
                        }
                    }
                }
            }
        }

        // Log if all channels failed (but message was still created successfully)
        let any_success = send_results.iter().any(|r| r.1.is_ok());
        if !any_success && !send_results.is_empty() {
            tracing::warn!(
                "All channels failed for message '{}', but message was stored successfully",
                message.title
            );
        }

        // Publish MessageCreated event to EventBus if configured
        if let Some(event_bus) = self.event_bus.read().await.as_ref() {
            use neomind_core::NeoMindEvent;
            let severity_str = format!("{:?}", severity).to_lowercase();
            let _ = event_bus
                .publish(NeoMindEvent::MessageCreated {
                    message_id: id.to_string(),
                    title: message.title.clone(),
                    severity: severity_str,
                    message: message.message.clone(),
                    timestamp: message.timestamp.timestamp(),
                })
                .await;
            tracing::debug!("Published MessageCreated event for message {}", id);
        }

        // Update dedup cache
        {
            let mut cache = self.dedup_cache.write().await;
            cache.insert(dedup_key, chrono::Utc::now());
            // Prune old entries (keep only last 5 minutes)
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(DEDUP_INTERVAL_SECS * 5);
            cache.retain(|_, ts| *ts > cutoff);
        }

        tracing::info!(
            "Message created successfully: id={}, title={}, severity={:?}, category={}",
            id,
            message.title,
            severity,
            message.category
        );

        Ok(message)
    }

    /// Create a simple alert message.
    pub async fn alert(
        &self,
        severity: MessageSeverity,
        title: String,
        message: String,
        source: String,
    ) -> Result<Message> {
        let msg = Message::alert(severity, title, message, source);
        self.create_message(msg).await
    }

    /// Create a device alert.
    pub async fn device_alert(
        &self,
        severity: MessageSeverity,
        title: String,
        message: String,
        device_id: String,
    ) -> Result<Message> {
        let msg = Message::device(severity, title, message, device_id);
        self.create_message(msg).await
    }

    /// Create a rule alert.
    pub async fn rule_alert(
        &self,
        severity: MessageSeverity,
        title: String,
        message: String,
        rule_id: String,
    ) -> Result<Message> {
        let msg = Message::rule(severity, title, message, rule_id);
        self.create_message(msg).await
    }

    /// Create a system message.
    pub async fn system_message(&self, title: String, message: String) -> Result<Message> {
        let msg = Message::system(title, message);
        self.create_message(msg).await
    }

    /// Get a message by ID.
    pub async fn get_message(&self, id: &MessageId) -> Option<Message> {
        self.messages.read().await.get(id).cloned()
    }

    /// List all messages.
    pub async fn list_messages(&self) -> Vec<Message> {
        let msgs: Vec<Message> = self.messages.read().await.values().cloned().collect();
        msgs
    }

    /// List messages filtered by category.
    pub async fn list_messages_by_category(&self, category: &str) -> Vec<Message> {
        self.messages
            .read()
            .await
            .values()
            .filter(|m| m.category == category)
            .cloned()
            .collect()
    }

    /// List messages filtered by status.
    pub async fn list_messages_by_status(&self, status: MessageStatus) -> Vec<Message> {
        self.messages
            .read()
            .await
            .values()
            .filter(|m| m.status == status)
            .cloned()
            .collect()
    }

    /// List active messages.
    pub async fn list_active_messages(&self) -> Vec<Message> {
        self.list_messages_by_status(MessageStatus::Active).await
    }

    /// Acknowledge a message.
    pub async fn acknowledge(&self, id: &MessageId) -> Result<()> {
        // Mutate in-memory, then drop write lock before I/O
        let stored_msg = {
            let mut messages = self.messages.write().await;
            let message = messages
                .get_mut(id)
                .ok_or_else(|| Error::NotFound(format!("Message not found: {}", id)))?;
            message.acknowledge();
            Self::message_to_stored(message)
        };

        // Persist outside lock
        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .update_async(stored_msg)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
        }

        // Publish event outside lock
        if let Some(event_bus) = self.event_bus.read().await.as_ref() {
            use neomind_core::NeoMindEvent;
            let _ = event_bus
                .publish(NeoMindEvent::MessageAcknowledged {
                    message_id: id.to_string(),
                    acknowledged_by: "api".to_string(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;
        }

        Ok(())
    }

    /// Resolve a message.
    pub async fn resolve(&self, id: &MessageId) -> Result<()> {
        let stored_msg = {
            let mut messages = self.messages.write().await;
            let message = messages
                .get_mut(id)
                .ok_or_else(|| Error::NotFound(format!("Message not found: {}", id)))?;
            message.resolve();
            Self::message_to_stored(message)
        };

        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .update_async(stored_msg)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
        }

        if let Some(event_bus) = self.event_bus.read().await.as_ref() {
            use neomind_core::NeoMindEvent;
            let _ = event_bus
                .publish(NeoMindEvent::MessageResolved {
                    message_id: id.to_string(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;
        }

        Ok(())
    }

    /// Archive a message.
    pub async fn archive(&self, id: &MessageId) -> Result<()> {
        let stored_msg = {
            let mut messages = self.messages.write().await;
            let message = messages
                .get_mut(id)
                .ok_or_else(|| Error::NotFound(format!("Message not found: {}", id)))?;
            message.archive();
            Self::message_to_stored(message)
        };

        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .update_async(stored_msg)
                .await
                .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
        }

        Ok(())
    }

    /// Delete a message.
    pub async fn delete(&self, id: &MessageId) -> Result<()> {
        // Remove from in-memory map first, drop lock, then persist
        self.messages
            .write()
            .await
            .remove(id)
            .ok_or_else(|| Error::NotFound(format!("Message not found: {}", id)))?;

        // Delete from storage outside write lock
        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .delete_async(id.to_string())
                .await
                .map_err(|e| Error::Storage(format!("Failed to delete message: {}", e)))?;
        }

        Ok(())
    }

    /// Delete multiple messages.
    pub async fn delete_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        // Remove from in-memory, drop lock, then persist
        let (count, id_strings): (usize, Vec<String>) = {
            let mut messages = self.messages.write().await;
            let mut count = 0;
            let id_strings: Vec<String> = ids
                .iter()
                .map(|id| {
                    if messages.remove(id).is_some() {
                        count += 1;
                    }
                    id.to_string()
                })
                .collect();
            (count, id_strings)
        };

        // Delete from storage outside lock
        if let Some(store) = self.storage.read().await.as_ref() {
            for id in id_strings {
                let _ = store.delete_async(id).await;
            }
        }

        Ok(count)
    }

    /// Acknowledge multiple messages.
    pub async fn acknowledge_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        let (count, to_persist): (usize, Vec<neomind_storage::StoredMessage>) = {
            let mut messages = self.messages.write().await;
            let mut count = 0;
            let mut to_persist = Vec::new();

            for id in ids {
                if let Some(message) = messages.get_mut(id) {
                    message.acknowledge();
                    count += 1;
                    to_persist.push(Self::message_to_stored(message));
                }
            }
            (count, to_persist)
        };

        // Persist outside lock
        if let Some(store) = self.storage.read().await.as_ref() {
            for stored in to_persist {
                let _ = store.update_async(stored).await;
            }
        }

        Ok(count)
    }

    /// Resolve multiple messages.
    pub async fn resolve_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        let (count, to_persist): (usize, Vec<neomind_storage::StoredMessage>) = {
            let mut messages = self.messages.write().await;
            let mut count = 0;
            let mut to_persist = Vec::new();

            for id in ids {
                if let Some(message) = messages.get_mut(id) {
                    message.resolve();
                    count += 1;
                    to_persist.push(Self::message_to_stored(message));
                }
            }
            (count, to_persist)
        };

        // Persist outside lock
        if let Some(store) = self.storage.read().await.as_ref() {
            for stored in to_persist {
                let _ = store.update_async(stored).await;
            }
        }

        Ok(count)
    }

    /// Get message statistics.
    pub async fn get_stats(&self) -> MessageStats {
        let messages = self.messages.read().await;
        let total = messages.len();
        let active = messages.values().filter(|m| m.is_active()).count();

        let mut by_category = HashMap::new();
        let mut by_severity = HashMap::new();
        let mut by_status = HashMap::new();

        for message in messages.values() {
            *by_category.entry(message.category.clone()).or_insert(0) += 1;
            *by_severity
                .entry(message.severity.as_str().to_string())
                .or_insert(0) += 1;
            *by_status
                .entry(message.status.as_str().to_string())
                .or_insert(0) += 1;
        }

        MessageStats {
            total,
            active,
            by_category,
            by_severity,
            by_status,
        }
    }

    /// Clear all messages (use with caution).
    pub async fn clear(&self) -> Result<()> {
        self.messages.write().await.clear();

        // Clear storage
        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .clear()
                .map_err(|e| Error::Storage(format!("Failed to clear messages: {}", e)))?;
        }

        Ok(())
    }

    /// Cleanup old messages.
    pub async fn cleanup_old(&self, older_than_days: i64) -> Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days);
        let mut messages = self.messages.write().await;
        let mut count = 0;

        messages.retain(|_, msg| {
            if msg.timestamp < cutoff {
                count += 1;
                false
            } else {
                true
            }
        });

        // Cleanup from storage
        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .cleanup_old(older_than_days)
                .map_err(|e| Error::Storage(format!("Failed to cleanup messages: {}", e)))?;
        }

        Ok(count)
    }
}

/// Message statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageStats {
    pub total: usize,
    pub active: usize,
    pub by_category: HashMap<String, usize>,
    pub by_severity: HashMap<String, usize>,
    pub by_status: HashMap<String, usize>,
}

/// Message rule for automatic message generation.
pub trait MessageRule: Send + Sync {
    /// Evaluate if the rule should trigger a message.
    fn evaluate(&self) -> bool;
    /// Generate the message if the rule triggers.
    fn generate_message(&self) -> Message;
}

/// Always-true rule for testing.
pub struct AlwaysTrueRule {
    pub message: Message,
}

impl MessageRule for AlwaysTrueRule {
    fn evaluate(&self) -> bool {
        true
    }

    fn generate_message(&self) -> Message {
        self.message.clone()
    }
}

/// Always-false rule for testing.
pub struct AlwaysFalseRule;

impl MessageRule for AlwaysFalseRule {
    fn evaluate(&self) -> bool {
        false
    }

    fn generate_message(&self) -> Message {
        Message::system(
            "Should not appear".to_string(),
            "This message should never be generated".to_string(),
        )
    }
}

/// Custom rule with predicate and message generator.
pub struct CustomRule<F, G>
where
    F: Fn() -> bool + Send + Sync,
    G: Fn() -> Message + Send + Sync,
{
    pub predicate: F,
    pub generator: G,
}

impl<F, G> MessageRule for CustomRule<F, G>
where
    F: Fn() -> bool + Send + Sync,
    G: Fn() -> Message + Send + Sync,
{
    fn evaluate(&self) -> bool {
        (self.predicate)()
    }

    fn generate_message(&self) -> Message {
        (self.generator)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (0.9.20): channels persisted via the create/update API must
    /// come back on restart. `load_persisted_channels` used to only restore
    /// webhook/email — telegram/wecom/dingtalk/slack/feishu configs sat in
    /// storage while the registry came up empty, silently disabling alerts.
    #[tokio::test]
    async fn test_persisted_channels_restored_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");

        {
            let manager = MessageManager::with_storage(&data_dir).unwrap();
            let registry = manager.channels.read().await;
            let cases: Vec<(
                &str,
                serde_json::Value,
                Box<dyn crate::channels::ChannelFactory>,
            )> = vec![
                (
                    "telegram",
                    serde_json::json!({"token": "t", "chat_id": "c"}),
                    Box::new(crate::TelegramChannelFactory),
                ),
                (
                    "feishu",
                    serde_json::json!({"hook_id": "h", "secret": "s"}),
                    Box::new(crate::FeishuChannelFactory),
                ),
                (
                    "webhook",
                    serde_json::json!({"url": "https://example.com/wh"}),
                    Box::new(crate::WebhookChannelFactory),
                ),
            ];
            for (i, (ty, cfg, factory)) in cases.into_iter().enumerate() {
                let _ = ty;
                let mut with_name = cfg.clone();
                with_name["name"] = serde_json::json!(format!("ch{i}"));
                let channel = factory
                    .create(&with_name)
                    .expect("factory must accept the same config the create handler does");
                registry
                    .register_with_config(format!("ch{i}"), channel, cfg.clone())
                    .await;
            }
        }

        // Fresh manager over the same storage == process restart.
        let manager = MessageManager::with_storage(&data_dir).unwrap();
        manager.load_persisted_channels().await;
        let registry = manager.channels.read().await;
        for i in 0..3 {
            assert!(
                registry.get(&format!("ch{i}")).await.is_some(),
                "persisted channel ch{i} must be restored after restart"
            );
        }
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = MessageManager::new();
        assert_eq!(manager.list_messages().await.len(), 0);
        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 0);
    }

    #[tokio::test]
    async fn test_create_message() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        assert_eq!(created.title, "Test");

        let retrieved = manager.get_message(&created.id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test");
    }

    #[tokio::test]
    async fn test_alert_message() {
        let manager = MessageManager::new();

        let created = manager
            .alert(
                MessageSeverity::Warning,
                "Test Alert".to_string(),
                "This is a test".to_string(),
                "test_source".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(created.title, "Test Alert");
        assert_eq!(created.category, "alert");
    }

    #[tokio::test]
    async fn test_acknowledge_message() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        assert!(created.is_active());

        manager.acknowledge(&created.id).await.unwrap();

        let retrieved = manager.get_message(&created.id).await.unwrap();
        assert_eq!(retrieved.status, MessageStatus::Acknowledged);
    }

    #[tokio::test]
    async fn test_resolve_message() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        manager.resolve(&created.id).await.unwrap();

        let retrieved = manager.get_message(&created.id).await.unwrap();
        assert_eq!(retrieved.status, MessageStatus::Resolved);
    }

    #[tokio::test]
    async fn test_delete_message() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        assert!(manager.get_message(&created.id).await.is_some());

        manager.delete(&created.id).await.unwrap();
        assert!(manager.get_message(&created.id).await.is_none());
    }

    #[tokio::test]
    async fn test_multiple_operations() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();

        let count = manager
            .acknowledge_multiple(&[msg1.id.clone(), msg2.id.clone()])
            .await
            .unwrap();
        assert_eq!(count, 2);

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 0);
    }

    #[tokio::test]
    async fn test_stats() {
        let manager = MessageManager::new();

        manager
            .create_message(Message::alert(
                MessageSeverity::Critical,
                "Alert1".to_string(),
                "Alert message".to_string(),
                "sensor1".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::system(
                "System1".to_string(),
                "System message".to_string(),
            ))
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 2);
        assert_eq!(*stats.by_category.get("alert").unwrap_or(&0), 1);
        assert_eq!(*stats.by_category.get("system").unwrap_or(&0), 1);
    }

    #[test]
    fn test_always_true_rule() {
        let msg = Message::system("Test".to_string(), "Test".to_string());
        let rule = AlwaysTrueRule { message: msg };

        assert!(rule.evaluate());
        let generated = rule.generate_message();
        assert_eq!(generated.title, "Test");
    }

    #[test]
    fn test_always_false_rule() {
        let rule = AlwaysFalseRule;
        assert!(!rule.evaluate());
    }

    #[test]
    fn test_custom_rule() {
        let rule = CustomRule {
            predicate: || true,
            generator: || Message::system("Generated".to_string(), "Generated message".to_string()),
        };

        assert!(rule.evaluate());
        let msg = rule.generate_message();
        assert_eq!(msg.title, "Generated");
    }

    #[tokio::test]
    async fn test_message_filtering_by_source_type() {
        use crate::channels::ChannelFilter;

        let filter = ChannelFilter {
            source_types: vec!["device".to_string()],
            ..Default::default()
        };

        let device_msg = Message::device(
            MessageSeverity::Warning,
            "Device Alert".to_string(),
            "Test".to_string(),
            "sensor_1".to_string(),
        );

        let system_msg = Message::system("System".to_string(), "Test".to_string());

        assert!(filter.matches(&device_msg));
        assert!(!filter.matches(&system_msg));
    }

    // =========================================================================
    // CRUD Tests
    // =========================================================================

    #[tokio::test]
    async fn test_create_and_retrieve_message() {
        let manager = MessageManager::new();
        let msg = Message::alert(
            MessageSeverity::Critical,
            "High Temperature".to_string(),
            "Temperature exceeded 80°C".to_string(),
            "sensor_1".to_string(),
        );

        let created = manager.create_message(msg).await.unwrap();
        assert_eq!(created.title, "High Temperature");
        assert_eq!(created.severity, MessageSeverity::Critical);
        assert!(created.is_active());

        let retrieved = manager.get_message(&created.id).await;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, created.id);
        assert_eq!(retrieved.title, "High Temperature");
    }

    #[tokio::test]
    async fn test_create_message_with_tags() {
        let manager = MessageManager::new();
        let msg = Message::alert(
            MessageSeverity::Warning,
            "Test".to_string(),
            "Test message".to_string(),
            "test_source".to_string(),
        )
        .with_tags(vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "tag3".to_string(),
        ]);

        let created = manager.create_message(msg).await.unwrap();
        assert_eq!(created.tags.len(), 3);
        assert!(created.tags.contains(&"tag1".to_string()));
        assert!(created.tags.contains(&"tag2".to_string()));
        assert!(created.tags.contains(&"tag3".to_string()));
    }

    #[tokio::test]
    async fn test_create_message_with_metadata() {
        let manager = MessageManager::new();
        let metadata = serde_json::json!({
            "temperature": 85.5,
            "unit": "celsius",
            "location": "server_room"
        });

        let msg = Message::alert(
            MessageSeverity::Critical,
            "High Temp".to_string(),
            "Temperature alert".to_string(),
            "sensor_1".to_string(),
        )
        .with_metadata(metadata.clone());

        let created = manager.create_message(msg).await.unwrap();
        assert!(created.metadata.is_some());
        assert_eq!(created.metadata.unwrap(), metadata);
    }

    #[tokio::test]
    async fn test_update_message_status() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        assert_eq!(created.status, MessageStatus::Active);

        // Acknowledge
        manager.acknowledge(&created.id).await.unwrap();
        let retrieved = manager.get_message(&created.id).await.unwrap();
        assert_eq!(retrieved.status, MessageStatus::Acknowledged);

        // Resolve
        manager.resolve(&created.id).await.unwrap();
        let retrieved = manager.get_message(&created.id).await.unwrap();
        assert_eq!(retrieved.status, MessageStatus::Resolved);

        // Archive
        manager.archive(&created.id).await.unwrap();
        let retrieved = manager.get_message(&created.id).await.unwrap();
        assert_eq!(retrieved.status, MessageStatus::Archived);
    }

    #[tokio::test]
    async fn test_update_nonexistent_message() {
        let manager = MessageManager::new();
        let fake_id = MessageId::new();

        let result = manager.acknowledge(&fake_id).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(Error::NotFound(_))));

        let result = manager.resolve(&fake_id).await;
        assert!(result.is_err());

        let result = manager.archive(&fake_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_single_message() {
        let manager = MessageManager::new();
        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let created = manager.create_message(msg).await.unwrap();
        assert!(manager.get_message(&created.id).await.is_some());

        manager.delete(&created.id).await.unwrap();
        assert!(manager.get_message(&created.id).await.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_message() {
        let manager = MessageManager::new();
        let fake_id = MessageId::new();

        let result = manager.delete(&fake_id).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn test_delete_multiple_messages() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();
        let msg3 = manager
            .create_message(Message::system("Test3".to_string(), "Message3".to_string()))
            .await
            .unwrap();

        assert_eq!(manager.list_messages().await.len(), 3);

        let count = manager
            .delete_multiple(&[msg1.id.clone(), msg2.id.clone()])
            .await
            .unwrap();
        assert_eq!(count, 2);

        assert_eq!(manager.list_messages().await.len(), 1);
        assert!(manager.get_message(&msg1.id).await.is_none());
        assert!(manager.get_message(&msg2.id).await.is_none());
        assert!(manager.get_message(&msg3.id).await.is_some());
    }

    #[tokio::test]
    async fn test_acknowledge_multiple_messages() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();
        let msg3 = manager
            .create_message(Message::system("Test3".to_string(), "Message3".to_string()))
            .await
            .unwrap();

        let count = manager
            .acknowledge_multiple(&[msg1.id.clone(), msg2.id.clone(), msg3.id.clone()])
            .await
            .unwrap();
        assert_eq!(count, 3);

        let stats = manager.get_stats().await;
        assert_eq!(stats.active, 0);
        assert_eq!(stats.by_status.get("acknowledged"), Some(&3));
    }

    #[tokio::test]
    async fn test_resolve_multiple_messages() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();

        let count = manager
            .resolve_multiple(&[msg1.id.clone(), msg2.id.clone()])
            .await
            .unwrap();
        assert_eq!(count, 2);

        let stats = manager.get_stats().await;
        assert_eq!(stats.by_status.get("resolved"), Some(&2));
    }

    // =========================================================================
    // Message Filtering Tests
    // =========================================================================

    #[tokio::test]
    async fn test_list_messages_by_category() {
        let manager = MessageManager::new();

        manager
            .create_message(Message::alert(
                MessageSeverity::Critical,
                "Alert1".to_string(),
                "Alert message".to_string(),
                "sensor1".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::system(
                "System1".to_string(),
                "System message".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::alert(
                MessageSeverity::Warning,
                "Alert2".to_string(),
                "Another alert".to_string(),
                "sensor2".to_string(),
            ))
            .await
            .unwrap();

        let alerts = manager.list_messages_by_category("alert").await;
        assert_eq!(alerts.len(), 2);

        let system = manager.list_messages_by_category("system").await;
        assert_eq!(system.len(), 1);

        let empty = manager.list_messages_by_category("nonexistent").await;
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_list_messages_by_status() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let _msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();

        manager.acknowledge(&msg1.id).await.unwrap();

        let active = manager.list_messages_by_status(MessageStatus::Active).await;
        assert_eq!(active.len(), 1);

        let acknowledged = manager
            .list_messages_by_status(MessageStatus::Acknowledged)
            .await;
        assert_eq!(acknowledged.len(), 1);
    }

    #[tokio::test]
    async fn test_list_active_messages() {
        let manager = MessageManager::new();

        let msg1 = manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        let msg2 = manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();

        manager.acknowledge(&msg1.id).await.unwrap();

        let active = manager.list_active_messages().await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, msg2.id);
    }

    #[tokio::test]
    async fn test_message_filtering_by_severity() {
        let manager = MessageManager::new();

        manager
            .create_message(Message::alert(
                MessageSeverity::Emergency,
                "Emergency".to_string(),
                "Emergency message".to_string(),
                "sensor1".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::alert(
                MessageSeverity::Critical,
                "Critical".to_string(),
                "Critical message".to_string(),
                "sensor2".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::alert(
                MessageSeverity::Info,
                "Info".to_string(),
                "Info message".to_string(),
                "sensor3".to_string(),
            ))
            .await
            .unwrap();

        let all_messages = manager.list_messages().await;
        assert_eq!(all_messages.len(), 3);

        let high_severity: Vec<_> = all_messages
            .iter()
            .filter(|m| m.severity >= MessageSeverity::Critical)
            .collect();
        assert_eq!(high_severity.len(), 2);
    }

    #[tokio::test]
    async fn test_message_filtering_by_source() {
        let manager = MessageManager::new();

        manager
            .create_message(Message::device(
                MessageSeverity::Warning,
                "Device1".to_string(),
                "Device message".to_string(),
                "sensor_1".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::device(
                MessageSeverity::Warning,
                "Device2".to_string(),
                "Another device".to_string(),
                "sensor_2".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::rule(
                MessageSeverity::Critical,
                "Rule1".to_string(),
                "Rule message".to_string(),
                "rule_1".to_string(),
            ))
            .await
            .unwrap();

        let all_messages = manager.list_messages().await;

        let device_messages: Vec<_> = all_messages
            .iter()
            .filter(|m| m.source_type == "device")
            .collect();
        assert_eq!(device_messages.len(), 2);

        let rule_messages: Vec<_> = all_messages
            .iter()
            .filter(|m| m.source_type == "rule")
            .collect();
        assert_eq!(rule_messages.len(), 1);
    }

    // =========================================================================
    // Edge Cases and Error Handling
    // =========================================================================

    #[tokio::test]
    async fn test_empty_message_fields() {
        let manager = MessageManager::new();

        // Test with empty strings
        let msg = Message::new(
            "",
            MessageSeverity::Info,
            "".to_string(),
            "".to_string(),
            "".to_string(),
        );

        let created = manager.create_message(msg).await.unwrap();
        assert_eq!(created.category, "");
        assert_eq!(created.title, "");
        assert_eq!(created.message, "");
        assert_eq!(created.source, "");
    }

    #[tokio::test]
    async fn test_message_with_special_characters() {
        let manager = MessageManager::new();

        let msg = Message::alert(
            MessageSeverity::Warning,
            "Temperature: 100°C & Humidity: 90%".to_string(),
            "Test <script>alert('xss')</script> & \"quotes\"".to_string(),
            "sensor-with-dash_1".to_string(),
        );

        let created = manager.create_message(msg).await.unwrap();
        assert!(created.title.contains("°C"));
        assert!(created.title.contains("&"));
        assert!(created.message.contains("<script>"));
    }

    #[tokio::test]
    async fn test_concurrent_message_creation() {
        let manager = MessageManager::new();
        let manager = Arc::new(manager);

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let mgr = manager.clone();
                tokio::spawn(async move {
                    mgr.create_message(Message::system(
                        format!("Concurrent {}", i),
                        format!("Message {}", i),
                    ))
                    .await
                })
            })
            .collect();

        // Wait for all tasks to complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 10);
    }

    #[tokio::test]
    async fn test_message_persistence_in_memory() {
        let manager = MessageManager::new();

        let msg = Message::system("Test".to_string(), "Test message".to_string());
        let created = manager.create_message(msg).await.unwrap();

        // Retrieve multiple times
        let retrieved1 = manager.get_message(&created.id).await.unwrap();
        let retrieved2 = manager.get_message(&created.id).await.unwrap();

        assert_eq!(retrieved1.id, retrieved2.id);
        assert_eq!(retrieved1.title, retrieved2.title);
    }

    #[tokio::test]
    async fn test_clear_all_messages() {
        let manager = MessageManager::new();

        manager
            .create_message(Message::system("Test1".to_string(), "Message1".to_string()))
            .await
            .unwrap();
        manager
            .create_message(Message::system("Test2".to_string(), "Message2".to_string()))
            .await
            .unwrap();

        assert_eq!(manager.list_messages().await.len(), 2);

        manager.clear().await.unwrap();

        assert_eq!(manager.list_messages().await.len(), 0);
        assert_eq!(manager.get_stats().await.total, 0);
    }

    #[tokio::test]
    async fn test_cleanup_old_messages() {
        let manager = MessageManager::new();

        // Create a message and manually age it
        let mut msg = Message::system(
            "Old Message".to_string(),
            "This should be cleaned up".to_string(),
        );
        msg.timestamp = chrono::Utc::now() - chrono::Duration::days(10);

        let old_msg = manager.create_message(msg).await.unwrap();

        // Create a recent message
        manager
            .create_message(Message::system(
                "Recent".to_string(),
                "This should stay".to_string(),
            ))
            .await
            .unwrap();

        assert_eq!(manager.list_messages().await.len(), 2);

        // Clean up messages older than 5 days
        let cleaned = manager.cleanup_old(5).await.unwrap();
        assert_eq!(cleaned, 1);

        assert_eq!(manager.list_messages().await.len(), 1);
        assert!(manager.get_message(&old_msg.id).await.is_none());
    }

    #[tokio::test]
    async fn test_message_stats_accuracy() {
        let manager = MessageManager::new();

        // Create messages with different properties
        manager
            .create_message(Message::alert(
                MessageSeverity::Emergency,
                "Emergency".to_string(),
                "Emergency".to_string(),
                "sensor1".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::alert(
                MessageSeverity::Critical,
                "Critical".to_string(),
                "Critical".to_string(),
                "sensor2".to_string(),
            ))
            .await
            .unwrap();

        manager
            .create_message(Message::alert(
                MessageSeverity::Warning,
                "Warning".to_string(),
                "Warning".to_string(),
                "sensor3".to_string(),
            ))
            .await
            .unwrap();

        let msg = manager
            .create_message(Message::alert(
                MessageSeverity::Info,
                "Info".to_string(),
                "Info".to_string(),
                "sensor4".to_string(),
            ))
            .await
            .unwrap();

        manager.acknowledge(&msg.id).await.unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 4);
        assert_eq!(stats.active, 3);
        assert_eq!(*stats.by_severity.get("emergency").unwrap_or(&0), 1);
        assert_eq!(*stats.by_severity.get("critical").unwrap_or(&0), 1);
        assert_eq!(*stats.by_severity.get("warning").unwrap_or(&0), 1);
        assert_eq!(*stats.by_severity.get("info").unwrap_or(&0), 1);
        assert_eq!(*stats.by_status.get("active").unwrap_or(&0), 3);
        assert_eq!(*stats.by_status.get("acknowledged").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_device_alert_creation() {
        let manager = MessageManager::new();

        let msg = manager
            .device_alert(
                MessageSeverity::Critical,
                "Device Offline".to_string(),
                "Sensor stopped responding".to_string(),
                "sensor_123".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(msg.source_type, "device");
        assert_eq!(msg.source, "sensor_123");
        assert!(msg.tags.contains(&"device".to_string()));
        assert_eq!(msg.severity, MessageSeverity::Critical);
    }

    #[tokio::test]
    async fn test_rule_alert_creation() {
        let manager = MessageManager::new();

        let msg = manager
            .rule_alert(
                MessageSeverity::Warning,
                "Rule Triggered".to_string(),
                "Temperature threshold exceeded".to_string(),
                "rule_temp_check".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(msg.source_type, "rule");
        assert_eq!(msg.source, "rule_temp_check");
        assert!(msg.tags.contains(&"rule".to_string()));
        assert_eq!(msg.severity, MessageSeverity::Warning);
    }

    #[tokio::test]
    async fn test_system_message_creation() {
        let manager = MessageManager::new();

        let msg = manager
            .system_message(
                "System Started".to_string(),
                "NeoMind system initialized successfully".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(msg.category, "system");
        assert_eq!(msg.title, "System Started");
        assert_eq!(msg.source_type, "system");
        assert_eq!(msg.severity, MessageSeverity::Info);
    }
}
