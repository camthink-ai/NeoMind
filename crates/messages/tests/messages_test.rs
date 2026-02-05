//! Comprehensive tests for the Messages system.
//!
//! Tests include:
//! - Message creation and management
//! - Message categories
//! - Severity levels
//! - Channel operations
//! - Message lifecycle

use edge_ai_messages::{
    category::MessageCategory,
    channels::{ConsoleChannel, MemoryChannel, MemoryChannelFactory, ConsoleChannelFactory, MessageChannel, ChannelFactory},
    manager::MessageManager,
    message::{Message, MessageSeverity, MessageStatus, MessageId},
};
use std::sync::Arc;

#[tokio::test]
async fn test_message_creation() {
    let message = Message::new(
        MessageCategory::System.as_str(),
        MessageSeverity::Info,
        "Test Alert".to_string(),
        "Test message".to_string(),
        "test_source".to_string(),
    );

    assert_eq!(message.title, "Test Alert");
    assert_eq!(message.severity, MessageSeverity::Info);
    assert_eq!(message.category, MessageCategory::System.as_str());
    assert_eq!(message.status, MessageStatus::Active);
}

#[tokio::test]
async fn test_message_alert_helper() {
    let message = Message::alert(
        MessageSeverity::Warning,
        "High Temperature".to_string(),
        "Temperature exceeded 30°C".to_string(),
        "sensor1".to_string(),
    );

    assert_eq!(message.title, "High Temperature");
    assert_eq!(message.severity, MessageSeverity::Warning);
    assert_eq!(message.category, MessageCategory::Alert.as_str());
}

#[tokio::test]
async fn test_message_system_helper() {
    let message = Message::system(
        "System Started".to_string(),
        "System has started successfully".to_string(),
    );

    assert_eq!(message.title, "System Started");
    assert_eq!(message.severity, MessageSeverity::Info);
    assert_eq!(message.category, MessageCategory::System.as_str());
}

#[tokio::test]
async fn test_message_with_metadata() {
    let mut message = Message::alert(
        MessageSeverity::Critical,
        "Device Offline".to_string(),
        "Device stopped responding".to_string(),
        "sensor1".to_string(),
    );
    message.metadata = Some(serde_json::json!({
        "device_id": "sensor1",
        "location": "room1"
    }));

    assert!(message.metadata.is_some());
    let metadata = message.metadata.unwrap();
    assert_eq!(metadata["device_id"], "sensor1");
    assert_eq!(metadata["location"], "room1");
}

#[tokio::test]
async fn test_message_status_transitions() {
    let mut message = Message::system(
        "Test".to_string(),
        "Test message".to_string(),
    );

    assert_eq!(message.status, MessageStatus::Active);

    message.status = MessageStatus::Acknowledged;
    assert_eq!(message.status, MessageStatus::Acknowledged);

    message.status = MessageStatus::Resolved;
    assert_eq!(message.status, MessageStatus::Resolved);

    message.status = MessageStatus::Archived;
    assert_eq!(message.status, MessageStatus::Archived);
}

#[tokio::test]
async fn test_message_manager_create() {
    let manager = MessageManager::new();

    let message = Message::new(
        MessageCategory::System.as_str(),
        MessageSeverity::Info,
        "Test Alert".to_string(),
        "Test message".to_string(),
        "test".to_string(),
    );

    let created = manager.create_message(message).await.unwrap();
    assert!(!created.id.to_string().is_empty());

    let retrieved = manager.get_message(&created.id).await.unwrap();
    assert_eq!(retrieved.title, "Test Alert");
}

#[tokio::test]
async fn test_message_manager_list() {
    let manager = MessageManager::new();

    manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 1".to_string(),
            "Message 1".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    manager
        .create_message(Message::alert(
            MessageSeverity::Warning,
            "Alert 2".to_string(),
            "Message 2".to_string(),
            "sensor1".to_string(),
        ))
        .await
        .unwrap();

    let messages = manager.list_messages().await;
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_message_manager_filter_by_status() {
    let manager = MessageManager::new();

    let _msg1 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Info Alert".to_string(),
            "Info".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg2 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Warning,
            "Warning Alert".to_string(),
            "Warning".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg3 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Critical,
            "Critical Alert".to_string(),
            "Critical".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    // Acknowledge one
    manager.acknowledge(&msg2.id).await.unwrap();

    // Resolve another
    manager.resolve(&msg3.id).await.unwrap();

    let active = manager.list_messages_by_status(MessageStatus::Active).await;
    assert_eq!(active.len(), 1);

    let acknowledged = manager.list_messages_by_status(MessageStatus::Acknowledged).await;
    assert_eq!(acknowledged.len(), 1);

    let resolved = manager.list_messages_by_status(MessageStatus::Resolved).await;
    assert_eq!(resolved.len(), 1);
}

#[tokio::test]
async fn test_message_manager_filter_by_category() {
    let manager = MessageManager::new();

    manager
        .create_message(Message::system(
            "System Alert".to_string(),
            "System message".to_string(),
        ))
        .await
        .unwrap();

    manager
        .create_message(Message::alert(
            MessageSeverity::Warning,
            "Device Alert".to_string(),
            "Device message".to_string(),
            "sensor1".to_string(),
        ))
        .await
        .unwrap();

    let system_alerts = manager.list_messages_by_category(MessageCategory::System.as_str()).await;
    assert_eq!(system_alerts.len(), 1);
    assert_eq!(system_alerts[0].title, "System Alert");

    let device_alerts = manager.list_messages_by_category(MessageCategory::Alert.as_str()).await;
    assert_eq!(device_alerts.len(), 1);
    assert_eq!(device_alerts[0].title, "Device Alert");
}

#[tokio::test]
async fn test_message_manager_acknowledge() {
    let manager = MessageManager::new();

    let message = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Test Alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    manager.acknowledge(&message.id).await.unwrap();

    let retrieved = manager.get_message(&message.id).await.unwrap();
    assert_eq!(retrieved.status, MessageStatus::Acknowledged);
}

#[tokio::test]
async fn test_message_manager_resolve() {
    let manager = MessageManager::new();

    let message = manager
        .create_message(Message::alert(
            MessageSeverity::Warning,
            "Test Alert".to_string(),
            "Test".to_string(),
            "sensor1".to_string(),
        ))
        .await
        .unwrap();

    manager.resolve(&message.id).await.unwrap();

    let retrieved = manager.get_message(&message.id).await.unwrap();
    assert_eq!(retrieved.status, MessageStatus::Resolved);
}

#[tokio::test]
async fn test_message_manager_delete() {
    let manager = MessageManager::new();

    let message = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Test Alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    manager.delete(&message.id).await.unwrap();

    let result = manager.get_message(&message.id).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_message_manager_stats() {
    let manager = MessageManager::new();

    // Create multiple messages
    let msg1 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 0".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg2 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 1".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg3 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 2".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    // Acknowledge all three
    manager.acknowledge(&msg1.id).await.unwrap();
    manager.acknowledge(&msg2.id).await.unwrap();
    manager.acknowledge(&msg3.id).await.unwrap();

    let stats = manager.get_stats().await;

    assert_eq!(stats.total, 3);
    assert_eq!(stats.active, 0);
    assert_eq!(*stats.by_status.get("acknowledged").unwrap_or(&0), 3);
}

#[tokio::test]
async fn test_console_channel() {
    let channel = ConsoleChannel::new("console".to_string());
    let message = Message::system(
        "Test".to_string(),
        "Test message".to_string(),
    );

    // Console channel should send without error
    let result = channel.send(&message).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_memory_channel() {
    let channel = MemoryChannel::new("test_channel".to_string());
    let message = Message::system(
        "Test".to_string(),
        "Test message".to_string(),
    );

    channel.send(&message).await.unwrap();

    let messages = channel.get_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].title, "Test");
}

#[tokio::test]
async fn test_channel_factory_console() {
    let factory = ConsoleChannelFactory;
    let channel = factory.create(&serde_json::json!({})).unwrap();

    let message = Message::system(
        "Test".to_string(),
        "Test message".to_string(),
    );
    let result = channel.send(&message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_channel_factory_memory() {
    // Just test that the factory creates a channel that works
    let factory = MemoryChannelFactory;
    let channel = factory.create(&serde_json::json!({"name": "test"})).unwrap();

    let message = Message::alert(
        MessageSeverity::Warning,
        "Test Alert".to_string(),
        "Test".to_string(),
        "sensor1".to_string(),
    );
    let result = channel.send(&message).await;

    assert!(result.is_ok());
    assert_eq!(channel.name(), "test");
    assert_eq!(channel.channel_type(), "memory");
}

#[tokio::test]
async fn test_message_categories() {
    let categories = vec![
        MessageCategory::Alert,
        MessageCategory::System,
        MessageCategory::Business,
    ];

    for category in categories {
        let message = Message::new(
            category.as_str(),
            MessageSeverity::Info,
            format!("{:?} Alert", category),
            "Test".to_string(),
            "test".to_string(),
        );
        assert_eq!(message.category, category.as_str());
    }
}

#[tokio::test]
async fn test_message_severities() {
    let severities = vec![
        MessageSeverity::Info,
        MessageSeverity::Warning,
        MessageSeverity::Critical,
        MessageSeverity::Emergency,
    ];

    for severity in severities {
        let message = Message::new(
            MessageCategory::System.as_str(),
            severity,
            format!("{:?} Alert", severity),
            "Test".to_string(),
            "test".to_string(),
        );
        assert_eq!(message.severity, severity);
    }
}

#[tokio::test]
async fn test_message_serialization() {
    let mut message = Message::alert(
        MessageSeverity::Warning,
        "Serialized Alert".to_string(),
        "Test description".to_string(),
        "sensor1".to_string(),
    );
    message.metadata = Some(serde_json::json!({"key": "value"}));

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("Serialized Alert"));

    let deserialized: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.title, "Serialized Alert");
    assert_eq!(deserialized.severity, MessageSeverity::Warning);
    assert_eq!(deserialized.metadata.unwrap()["key"], "value");
}

#[tokio::test]
async fn test_message_manager_bulk_operations() {
    let manager = MessageManager::new();

    let msg1 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 0".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg2 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 1".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg3 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Alert 2".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    // Bulk acknowledge
    let count = manager
        .acknowledge_multiple(&[msg1.id.clone(), msg2.id.clone()])
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Bulk resolve
    let count = manager
        .resolve_multiple(&[msg1.id.clone(), msg3.id.clone()])
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_message_manager_clear() {
    let manager = MessageManager::new();

    for i in 0..5 {
        manager
            .create_message(Message::new(
                MessageCategory::System.as_str(),
                MessageSeverity::Info,
                format!("Alert {}", i),
                "Test".to_string(),
                "test".to_string(),
            ))
            .await
            .unwrap();
    }

    assert_eq!(manager.list_messages().await.len(), 5);

    manager.clear().await.unwrap();

    assert_eq!(manager.list_messages().await.len(), 0);
}

#[tokio::test]
async fn test_message_manager_cleanup_old() {
    let manager = MessageManager::new();

    // Create some messages
    for i in 0..3 {
        manager
            .create_message(Message::new(
                MessageCategory::System.as_str(),
                MessageSeverity::Info,
                format!("Alert {}", i),
                "Test".to_string(),
                "test".to_string(),
            ))
            .await
            .unwrap();
    }

    // Cleanup messages older than 100 days (should not affect recent messages)
    let cleaned = manager.cleanup_old(100).await.unwrap();
    assert_eq!(cleaned, 0);

    // Cleanup messages older than 0 days (should clean all)
    let cleaned = manager.cleanup_old(0).await.unwrap();
    assert_eq!(cleaned, 3);
}

#[tokio::test]
async fn test_message_manager_active_messages() {
    let manager = MessageManager::new();

    let msg1 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Active 1".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    let msg2 = manager
        .create_message(Message::new(
            MessageCategory::System.as_str(),
            MessageSeverity::Info,
            "Active 2".to_string(),
            "Test".to_string(),
            "test".to_string(),
        ))
        .await
        .unwrap();

    // Acknowledge one
    manager.acknowledge(&msg1.id).await.unwrap();

    let active = manager.list_active_messages().await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, msg2.id);
}

#[tokio::test]
async fn test_memory_channel_clear() {
    let channel = MemoryChannel::new("test_channel".to_string());

    use edge_ai_messages::MessageChannel;

    channel
        .send(&Message::system(
            "Test 1".to_string(),
            "Message 1".to_string(),
        ))
        .await
        .unwrap();

    channel
        .send(&Message::system(
            "Test 2".to_string(),
            "Message 2".to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(channel.get_messages().await.len(), 2);

    channel.clear().await;

    assert_eq!(channel.get_messages().await.len(), 0);
}

#[tokio::test]
async fn test_message_clone() {
    let message = Message::new(
        MessageCategory::System.as_str(),
        MessageSeverity::Info,
        "Original".to_string(),
        "Test".to_string(),
        "test".to_string(),
    );

    let cloned = message.clone();

    assert_eq!(cloned.title, "Original");
    assert_eq!(cloned.id, message.id);
}

#[tokio::test]
async fn test_message_id() {
    let id1 = MessageId::new();
    let id2 = MessageId::new();

    assert_ne!(id1, id2);

    let id_str = id1.to_string();
    let parsed = MessageId::from_string(&id_str).unwrap();
    assert_eq!(id1, parsed);
}

#[tokio::test]
async fn test_message_severity_from_string() {
    assert_eq!(
        MessageSeverity::from_string("info"),
        Some(MessageSeverity::Info)
    );
    assert_eq!(
        MessageSeverity::from_string("warning"),
        Some(MessageSeverity::Warning)
    );
    assert_eq!(
        MessageSeverity::from_string("critical"),
        Some(MessageSeverity::Critical)
    );
    assert_eq!(
        MessageSeverity::from_string("emergency"),
        Some(MessageSeverity::Emergency)
    );
    assert_eq!(MessageSeverity::from_string("invalid"), None);
}

#[tokio::test]
async fn test_message_status_from_string() {
    assert_eq!(
        MessageStatus::from_string("active"),
        Some(MessageStatus::Active)
    );
    assert_eq!(
        MessageStatus::from_string("acknowledged"),
        Some(MessageStatus::Acknowledged)
    );
    assert_eq!(
        MessageStatus::from_string("resolved"),
        Some(MessageStatus::Resolved)
    );
    assert_eq!(
        MessageStatus::from_string("archived"),
        Some(MessageStatus::Archived)
    );
    assert_eq!(MessageStatus::from_string("invalid"), None);
}

#[tokio::test]
async fn test_category_from_string() {
    assert_eq!(
        MessageCategory::from_string("alert"),
        Some(MessageCategory::Alert)
    );
    assert_eq!(
        MessageCategory::from_string("system"),
        Some(MessageCategory::System)
    );
    assert_eq!(
        MessageCategory::from_string("business"),
        Some(MessageCategory::Business)
    );
    assert_eq!(MessageCategory::from_string("invalid"), None);
}

#[tokio::test]
async fn test_manager_alert_helper() {
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
    assert_eq!(created.category, MessageCategory::Alert.as_str());
}

#[tokio::test]
async fn test_manager_system_message_helper() {
    let manager = MessageManager::new();

    let created = manager
        .system_message(
            "System Info".to_string(),
            "System is running".to_string(),
        )
        .await
        .unwrap();

    assert_eq!(created.title, "System Info");
    assert_eq!(created.category, MessageCategory::System.as_str());
}

#[tokio::test]
async fn test_message_is_active() {
    let message = Message::system(
        "Test".to_string(),
        "Test message".to_string(),
    );

    assert!(message.is_active());
}

#[tokio::test]
async fn test_message_device_helper() {
    let message = Message::device(
        MessageSeverity::Warning,
        "Device Alert".to_string(),
        "Device offline".to_string(),
        "sensor1".to_string(),
    );

    assert_eq!(message.title, "Device Alert");
    assert_eq!(message.source, "sensor1");
    assert_eq!(message.source_type, "device");
}
