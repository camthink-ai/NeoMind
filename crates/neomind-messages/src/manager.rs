//! Message manager with persistent storage.
//!
//! Provides a unified interface for creating, tracking, and managing messages
//! with persistent storage using redb.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::channels::ChannelRegistry;
use super::error::{Error, Result};
use super::{Message, MessageId, MessageSeverity, MessageStatus};

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
    /// Data directory for persistent storage (reserved for future use).
    #[allow(dead_code)]
    data_dir: Arc<RwLock<Option<String>>>,
}

impl MessageManager {
    /// Create a new in-memory message manager (no persistence).
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(HashMap::new())),
            storage: Arc::new(RwLock::new(None)),
            channels: Arc::new(RwLock::new(ChannelRegistry::new())),
            event_bus: Arc::new(RwLock::new(None)),
            data_dir: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new message manager with persistent storage.
    pub fn with_storage<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)
            .map_err(|e| Error::Storage(format!("Failed to create data directory: {}", e)))?;

        let store = Arc::new(
            neomind_storage::MessageStore::open(data_dir)
                .map_err(|e| Error::Storage(format!("Failed to open message store: {}", e)))?,
        );

        // Load existing messages into memory
        let mut messages = HashMap::new();
        if let Ok(stored_msgs) = store.list() {
            for stored_msg in stored_msgs {
                if let Ok(id) = MessageId::from_string(&stored_msg.id) {
                    let msg = Self::stored_to_message(stored_msg);
                    messages.insert(id, msg);
                }
            }
        }

        Ok(Self {
            messages: Arc::new(RwLock::new(messages)),
            storage: Arc::new(RwLock::new(Some(store))),
            channels: Arc::new(RwLock::new(ChannelRegistry::new())),
            event_bus: Arc::new(RwLock::new(None)),
            data_dir: Arc::new(RwLock::new(Some(data_dir.to_string_lossy().to_string()))),
        })
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

    /// Register default channels (console, memory).
    pub async fn register_default_channels(&self) {
        let channels = self.channels.read().await;
        channels
            .register_with_config(
                "console".to_string(),
                std::sync::Arc::new(super::channels::ConsoleChannel::new("console".to_string())),
                serde_json::json!({"include_details": true}),
            )
            .await;
        channels
            .register_with_config(
                "memory".to_string(),
                std::sync::Arc::new(super::channels::MemoryChannel::new("memory".to_string())),
                serde_json::json!({}),
            )
            .await;
    }

    /// Create and send a message.
    pub async fn create_message(&self, message: Message) -> Result<Message> {
        let id = message.id.clone();
        let _is_active = message.is_active();
        let severity = message.severity;

        // Store in memory
        self.messages
            .write()
            .await
            .insert(id.clone(), message.clone());

        // Persist to storage if available
        if let Some(store) = self.storage.read().await.as_ref() {
            let stored = Self::message_to_stored(&message);
            store
                .insert(&stored)
                .map_err(|e| Error::Storage(format!("Failed to persist message: {}", e)))?;
        }

        // Send through channels (don't fail if channels fail - message is already stored)
        let channels = self.channels.read().await;
        let channel_names = channels.list_names().await;
        let mut send_results = Vec::new();

        for channel_name in &channel_names {
            if let Some(channel) = channels.get(channel_name).await {
                if channel.is_enabled() {
                    match channel.send(&message).await {
                        Ok(()) => send_results.push((channel_name.clone(), Ok(()))),
                        Err(e) => {
                            // Log channel failure but don't fail the entire operation
                            tracing::warn!(
                                "Failed to send message through channel '{}': {}",
                                channel_name,
                                e
                            );
                            send_results.push((channel_name.clone(), Err(e)));
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
        let mut messages = self.messages.write().await;
        if let Some(message) = messages.get_mut(id) {
            message.acknowledge();

            // Persist update
            if let Some(store) = self.storage.read().await.as_ref() {
                let stored = Self::message_to_stored(message);
                store
                    .update(&stored)
                    .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
            }

            // Publish event
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
        } else {
            Err(Error::NotFound(format!("Message not found: {}", id)))
        }
    }

    /// Resolve a message.
    pub async fn resolve(&self, id: &MessageId) -> Result<()> {
        let mut messages = self.messages.write().await;
        if let Some(message) = messages.get_mut(id) {
            message.resolve();

            // Persist update
            if let Some(store) = self.storage.read().await.as_ref() {
                let stored = Self::message_to_stored(message);
                store
                    .update(&stored)
                    .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
            }

            // Publish event
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
        } else {
            Err(Error::NotFound(format!("Message not found: {}", id)))
        }
    }

    /// Archive a message.
    pub async fn archive(&self, id: &MessageId) -> Result<()> {
        let mut messages = self.messages.write().await;
        if let Some(message) = messages.get_mut(id) {
            message.archive();

            // Persist update
            if let Some(store) = self.storage.read().await.as_ref() {
                let stored = Self::message_to_stored(message);
                store
                    .update(&stored)
                    .map_err(|e| Error::Storage(format!("Failed to update message: {}", e)))?;
            }

            Ok(())
        } else {
            Err(Error::NotFound(format!("Message not found: {}", id)))
        }
    }

    /// Delete a message.
    pub async fn delete(&self, id: &MessageId) -> Result<()> {
        self.messages
            .write()
            .await
            .remove(id)
            .ok_or_else(|| Error::NotFound(format!("Message not found: {}", id)))?;

        // Delete from storage
        if let Some(store) = self.storage.read().await.as_ref() {
            store
                .delete(&id.to_string())
                .map_err(|e| Error::Storage(format!("Failed to delete message: {}", e)))?;
        }

        Ok(())
    }

    /// Delete multiple messages.
    pub async fn delete_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        let mut messages = self.messages.write().await;
        let mut count = 0;

        for id in ids {
            if messages.remove(id).is_some() {
                count += 1;
            }
        }

        // Delete from storage
        if let Some(store) = self.storage.read().await.as_ref() {
            for id in ids {
                let _ = store.delete(&id.to_string());
            }
        }

        Ok(count)
    }

    /// Acknowledge multiple messages.
    pub async fn acknowledge_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        let mut messages = self.messages.write().await;
        let mut count = 0;

        for id in ids {
            if let Some(message) = messages.get_mut(id) {
                message.acknowledge();
                count += 1;
            }
        }

        // Persist updates
        if let Some(store) = self.storage.read().await.as_ref() {
            for id in ids {
                if let Some(message) = messages.get(id) {
                    let stored = Self::message_to_stored(message);
                    let _ = store.update(&stored);
                }
            }
        }

        Ok(count)
    }

    /// Resolve multiple messages.
    pub async fn resolve_multiple(&self, ids: &[MessageId]) -> Result<usize> {
        let mut messages = self.messages.write().await;
        let mut count = 0;

        for id in ids {
            if let Some(message) = messages.get_mut(id) {
                message.resolve();
                count += 1;
            }
        }

        // Persist updates
        if let Some(store) = self.storage.read().await.as_ref() {
            for id in ids {
                if let Some(message) = messages.get(id) {
                    let stored = Self::message_to_stored(message);
                    let _ = store.update(&stored);
                }
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

    /// Reload messages from storage.
    pub async fn reload(&self) -> Result<()> {
        if let Some(store) = self.storage.read().await.as_ref() {
            let stored_msgs = store
                .list()
                .map_err(|e| Error::Storage(format!("Failed to load messages: {}", e)))?;

            let mut messages = HashMap::new();
            for stored_msg in stored_msgs {
                if let Ok(id) = MessageId::from_string(&stored_msg.id) {
                    let msg = Self::stored_to_message(stored_msg);
                    messages.insert(id, msg);
                }
            }

            *self.messages.write().await = messages;
        }

        Ok(())
    }
}

impl Default for MessageManager {
    fn default() -> Self {
        Self::new()
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
}
