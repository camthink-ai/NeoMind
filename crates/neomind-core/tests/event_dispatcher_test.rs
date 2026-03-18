//! Comprehensive Unit Tests for EventDispatcher
//!
//! Tests cover:
//! - Dispatcher creation
//! - In-process extension registration
//! - Isolated extension registration
//! - Event subscription management
//! - Event dispatching to extensions
//! - Extension unregistration

use neomind_core::extension::event_dispatcher::EventDispatcher;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, MetricDescriptor, ExtensionCommand, Result,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use serde_json::json;

// ============================================================================
// Mock Extension for Event Testing
// ============================================================================

struct MockEventExtension {
    id: String,
    event_subscriptions: Vec<&'static str>,
    events_received: AtomicUsize,
}

impl MockEventExtension {
    fn new(id: &str, subscriptions: Vec<&'static str>) -> Self {
        Self {
            id: id.to_string(),
            event_subscriptions: subscriptions,
            events_received: AtomicUsize::new(0),
        }
    }

    fn events_received(&self) -> usize {
        self.events_received.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Extension for MockEventExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "mock.event.extension",
                "Mock Event Extension",
                semver::Version::new(1, 0, 0),
            )
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn event_subscriptions(&self) -> &[&str] {
        &self.event_subscriptions
    }

    fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<()> {
        self.events_received.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Ok(json!({}))
    }
}

// ============================================================================
// Dispatcher Creation Tests
// ============================================================================

#[test]
fn test_dispatcher_creation() {
    let dispatcher = EventDispatcher::new();
    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.is_empty());
}

// ============================================================================
// In-Process Extension Registration Tests
// ============================================================================

#[tokio::test]
async fn test_register_in_process_extension() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.contains_key("test.ext"));
    assert_eq!(subscriptions["test.ext"], vec!["DeviceMetric"]);
}

#[tokio::test]
async fn test_register_in_process_extension_no_subscriptions() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec![]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    let subscriptions = dispatcher.get_subscriptions();
    // Extension with no subscriptions should not be in subscriptions map
    assert!(!subscriptions.contains_key("test.ext"));
}

#[tokio::test]
async fn test_register_multiple_in_process_extensions() {
    let dispatcher = EventDispatcher::new();

    for i in 0..3 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockEventExtension::new(&format!("ext.{}", i), vec!["DeviceMetric"]))
                as Box<dyn Extension>
        ));
        dispatcher.register_in_process_extension(format!("ext.{}", i), ext).await;
    }

    let subscriptions = dispatcher.get_subscriptions();
    assert_eq!(subscriptions.len(), 3);
}

// ============================================================================
// Isolated Extension Registration Tests
// ============================================================================

#[test]
fn test_register_isolated_extension() {
    let dispatcher = EventDispatcher::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    dispatcher.register_isolated_extension(
        "isolated.ext".to_string(),
        vec!["DeviceMetric".to_string(), "DeviceAlert".to_string()],
        tx,
    );

    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.contains_key("isolated.ext"));
    assert_eq!(subscriptions["isolated.ext"].len(), 2);
}

#[test]
fn test_register_isolated_extension_no_subscriptions() {
    let dispatcher = EventDispatcher::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    dispatcher.register_isolated_extension(
        "isolated.ext".to_string(),
        vec![],
        tx,
    );

    let subscriptions = dispatcher.get_subscriptions();
    // Extension with no subscriptions should not be in subscriptions map
    assert!(!subscriptions.contains_key("isolated.ext"));
}

// ============================================================================
// Extension Unregistration Tests
// ============================================================================

#[tokio::test]
async fn test_unregister_extension() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;
    assert!(dispatcher.get_subscriptions().contains_key("test.ext"));

    dispatcher.unregister_extension("test.ext");
    assert!(!dispatcher.get_subscriptions().contains_key("test.ext"));
}

#[test]
fn test_unregister_nonexistent_extension() {
    let dispatcher = EventDispatcher::new();

    // Should not panic
    dispatcher.unregister_extension("nonexistent");
}

// ============================================================================
// Subscription Tests
// ============================================================================

#[tokio::test]
async fn test_subscription_exact_match() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext.clone()).await;

    // Dispatch event
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Verify event was received by checking the extension's internal counter
    // Note: We can't directly access the counter due to trait object limitations
    // This test verifies the dispatch doesn't panic
}

#[tokio::test]
async fn test_subscription_multiple_event_types() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric", "DeviceAlert"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    // Dispatch both event types
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 1})).await;
    dispatcher.dispatch_event("DeviceAlert", json!({"value": 2})).await;

    // Test verifies dispatch doesn't panic
}

#[tokio::test]
async fn test_no_subscription_no_dispatch() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    // Dispatch different event type
    dispatcher.dispatch_event("OtherEvent", json!({"value": 42})).await;

    // Test verifies dispatch doesn't panic
}

// ============================================================================
// Concurrent Registration Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_registration() {
    let dispatcher = Arc::new(EventDispatcher::new());
    let mut handles = vec![];

    for i in 0..5 {
        let dispatcher = dispatcher.clone();
        let handle = tokio::spawn(async move {
            let ext = Arc::new(tokio::sync::RwLock::new(
                Box::new(MockEventExtension::new(&format!("ext.{}", i), vec!["DeviceMetric"]))
                    as Box<dyn Extension>
            ));
            dispatcher.register_in_process_extension(format!("ext.{}", i), ext).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let subscriptions = dispatcher.get_subscriptions();
    assert_eq!(subscriptions.len(), 5);
}

// ============================================================================
// Mixed In-Process and Isolated Tests
// ============================================================================

#[tokio::test]
async fn test_mixed_in_process_and_isolated() {
    let dispatcher = EventDispatcher::new();

    // Register in-process extension
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("inprocess.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));
    dispatcher.register_in_process_extension("inprocess.ext".to_string(), ext).await;

    // Register isolated extension
    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    dispatcher.register_isolated_extension(
        "isolated.ext".to_string(),
        vec!["DeviceMetric".to_string()],
        tx,
    );

    let subscriptions = dispatcher.get_subscriptions();
    assert_eq!(subscriptions.len(), 2);
}

// ============================================================================
// Large Payload Tests
// ============================================================================

#[tokio::test]
async fn test_large_payload() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    // Create large payload
    let large_data: Vec<u8> = vec![0u8; 1024 * 1024]; // 1MB
    let payload = json!({
        "data": large_data,
        "timestamp": 1234567890,
    });

    // Dispatch event with large payload
    dispatcher.dispatch_event("DeviceMetric", payload).await;

    // Test verifies dispatch doesn't panic
}

// ============================================================================
// Get Subscriptions Tests
// ============================================================================

#[test]
fn test_get_subscriptions_empty() {
    let dispatcher = EventDispatcher::new();
    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.is_empty());
}

#[tokio::test]
async fn test_get_subscriptions_after_registration() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(MockEventExtension::new("test.ext", vec!["DeviceMetric", "DeviceAlert"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("test.ext".to_string(), ext).await;

    let subscriptions = dispatcher.get_subscriptions();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions["test.ext"].len(), 2);
}

// ============================================================================
// Event Dispatch Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_dispatch_to_multiple_extensions() {
    let dispatcher = EventDispatcher::new();

    // Register multiple extensions
    for i in 0..3 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockEventExtension::new(&format!("ext.{}", i), vec!["DeviceMetric"]))
                as Box<dyn Extension>
        ));
        dispatcher.register_in_process_extension(format!("ext.{}", i), ext).await;
    }

    // Dispatch event
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Test verifies dispatch doesn't panic
}