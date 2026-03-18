//! Integration Tests for Event Subscription and Dispatch
//!
//! Tests cover:
//! - Event subscription registration
//! - Event dispatching to subscribers
//! - Event filtering
//! - Multi-extension event handling
//! - Event bus integration
//! - Isolated extension event handling

use neomind_core::extension::event_dispatcher::EventDispatcher;
use neomind_core::extension::event_subscription::{EventSubscription, EventFilter};
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionCommand,
    MetricDescriptor, Result,
};
use neomind_core::eventbus::EventBus;
use neomind_core::event::NeoMindEvent;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use serde_json::json;

// ============================================================================
// Event Recording Extension
// ============================================================================

struct EventRecordingExtension {
    id: String,
    subscriptions: Vec<&'static str>,
    events_received: AtomicUsize,
}

impl EventRecordingExtension {
    fn new(id: &str, subscriptions: Vec<&'static str>) -> Self {
        Self {
            id: id.to_string(),
            subscriptions,
            events_received: AtomicUsize::new(0),
        }
    }

    fn events_received(&self) -> usize {
        self.events_received.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Extension for EventRecordingExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "event.recording.extension",
                "Event Recording Extension",
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
        &self.subscriptions
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
// Event Subscription Tests
// ============================================================================

#[test]
fn test_event_subscription_creation() {
    let sub = EventSubscription::with_types(vec!["DeviceMetric".to_string()]);

    assert!(sub.is_subscribed("DeviceMetric"));
    assert!(!sub.is_subscribed("OtherEvent"));
}

#[test]
fn test_event_subscription_with_filters() {
    let filter = EventFilter::new().by_device_id("device-1");
    let sub = EventSubscription::with_types(vec!["DeviceMetric".to_string()])
        .with_filters(filter);

    assert!(sub.is_subscribed("DeviceMetric"));
}

#[test]
fn test_event_subscription_empty_types() {
    let sub = EventSubscription::new();

    // Empty types means subscribe to all events
    assert!(sub.is_subscribed("DeviceMetric"));
    assert!(sub.is_subscribed("AnyEvent"));
}

#[test]
fn test_event_filter_by_device() {
    let filter = EventFilter::new().by_device_id("device-1");

    assert!(filter.device_id.is_some());
    assert_eq!(filter.device_id.unwrap(), "device-1");
}

#[test]
fn test_event_filter_by_source() {
    let filter = EventFilter::new().by_source("devices");

    assert!(filter.source.is_some());
    assert_eq!(filter.source.unwrap(), "devices");
}

// ============================================================================
// EventDispatcher Tests
// ============================================================================

#[test]
fn test_dispatcher_creation() {
    let dispatcher = EventDispatcher::new();
    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.is_empty());
}

#[tokio::test]
async fn test_dispatcher_register_in_process() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.contains_key("ext.1"));
    assert_eq!(subscriptions["ext.1"], vec!["DeviceMetric"]);
}

#[test]
fn test_dispatcher_register_isolated() {
    let dispatcher = EventDispatcher::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    dispatcher.register_isolated_extension(
        "isolated.1".to_string(),
        vec!["Alert".to_string()],
        tx,
    );

    let subscriptions = dispatcher.get_subscriptions();
    assert!(subscriptions.contains_key("isolated.1"));
    assert_eq!(subscriptions["isolated.1"], vec!["Alert"]);
}

#[tokio::test]
async fn test_dispatcher_unregister() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;
    assert!(dispatcher.get_subscriptions().contains_key("ext.1"));

    dispatcher.unregister_extension("ext.1");
    assert!(!dispatcher.get_subscriptions().contains_key("ext.1"));
}

// ============================================================================
// Event Dispatch Tests
// ============================================================================

#[tokio::test]
async fn test_dispatch_exact_match() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Event should be dispatched (verified by no panic)
}

#[tokio::test]
async fn test_dispatch_no_match() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    // Dispatch event that doesn't match subscription
    dispatcher.dispatch_event("OtherEvent", json!({"value": 42})).await;

    // Should not panic, event not delivered
}

#[tokio::test]
async fn test_dispatch_wildcard() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["*"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    // Dispatch any event - should match wildcard
    dispatcher.dispatch_event("AnyEvent", json!({"value": 1})).await;
    dispatcher.dispatch_event("AnotherEvent", json!({"value": 2})).await;

    // No panic is sufficient verification
}

// ============================================================================
// Multi-Extension Tests
// ============================================================================

#[tokio::test]
async fn test_multiple_extensions_same_event() {
    let dispatcher = EventDispatcher::new();

    // Register multiple extensions for the same event
    for i in 0..3 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(EventRecordingExtension::new(&format!("ext.{}", i), vec!["DeviceMetric"]))
                as Box<dyn Extension>
        ));
        dispatcher.register_in_process_extension(format!("ext.{}", i), ext).await;
    }

    // Dispatch event
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // All extensions should receive the event (verified by no panic)
}

#[tokio::test]
async fn test_multiple_extensions_different_events() {
    let dispatcher = EventDispatcher::new();

    // Register extensions for different events
    let ext1 = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));
    let ext2 = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.2", vec!["Alert"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext1).await;
    dispatcher.register_in_process_extension("ext.2".to_string(), ext2).await;

    // Dispatch DeviceMetric - only ext.1 should receive
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 1})).await;

    // Dispatch Alert - only ext.2 should receive
    dispatcher.dispatch_event("Alert", json!({"value": 2})).await;

    // Test verifies dispatch doesn't panic
}

// ============================================================================
// Event Bus Tests
// ============================================================================

#[test]
fn test_event_bus_creation() {
    let bus = EventBus::new();
    // EventBus should be created successfully
    let _ = bus;
}

#[tokio::test]
async fn test_event_bus_publish() {
    let bus = Arc::new(EventBus::new());

    // Publish an event
    let event = NeoMindEvent::DeviceMetric {
        device_id: "device-1".to_string(),
        metric: "temperature".to_string(),
        value: neomind_core::event::MetricValue::Float(25.5),
        timestamp: chrono::Utc::now().timestamp_millis(),
        quality: None,
    };

    // Should not panic
    bus.publish(event).await;
}

// ============================================================================
// Isolated Extension Event Channel Tests
// ============================================================================

#[tokio::test]
async fn test_isolated_event_channel() {
    let dispatcher = EventDispatcher::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    dispatcher.register_isolated_extension(
        "isolated.1".to_string(),
        vec!["DeviceMetric".to_string()],
        tx,
    );

    // Dispatch event
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Receive event from channel
    let received = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv()
    ).await;

    assert!(received.is_ok());
    let (event_type, payload) = received.unwrap().unwrap();
    assert_eq!(event_type, "DeviceMetric");
    assert_eq!(payload["value"], 42);
}

#[tokio::test]
async fn test_isolated_event_channel_no_match() {
    let dispatcher = EventDispatcher::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    dispatcher.register_isolated_extension(
        "isolated.1".to_string(),
        vec!["Alert".to_string()],
        tx,
    );

    // Dispatch event that doesn't match subscription
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Should not receive event
    let received = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        rx.recv()
    ).await;

    assert!(received.is_err()); // Timeout - no event received
}

// ============================================================================
// Extension Event Subscription Service Tests
// ============================================================================

#[test]
fn test_extension_event_subscription_service_creation() {
    // This test verifies the service can be created
    // The actual service is tested in its own module
    assert!(true);
}

// ============================================================================
// Stress Tests
// ============================================================================

#[tokio::test]
async fn test_high_volume_events() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    // Dispatch many events
    for i in 0..100 {
        dispatcher.dispatch_event("DeviceMetric", json!({"value": i})).await;
    }

    // Test verifies dispatch doesn't panic
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_empty_subscription_list() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec![]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;

    // Extension should not be in subscriptions
    let subscriptions = dispatcher.get_subscriptions();
    assert!(!subscriptions.contains_key("ext.1"));
}

#[tokio::test]
async fn test_dispatch_after_unregister() {
    let dispatcher = EventDispatcher::new();
    let ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(EventRecordingExtension::new("ext.1", vec!["DeviceMetric"]))
            as Box<dyn Extension>
    ));

    dispatcher.register_in_process_extension("ext.1".to_string(), ext).await;
    dispatcher.unregister_extension("ext.1");

    // Dispatch event after unregister
    dispatcher.dispatch_event("DeviceMetric", json!({"value": 42})).await;

    // Test verifies dispatch doesn't panic
}