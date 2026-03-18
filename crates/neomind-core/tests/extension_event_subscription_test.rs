//! Comprehensive integration tests for extension event subscription.
//!
//! Tests include:
//! - Extension event subscription registration
//! - Event dispatching to subscribed extensions
//! - Subscription matching (exact, prefix, wildcard)
//! - Extension event handling
//! - Isolated extension event pushing
//! - Automatic event conversion for all NeoMindEvent types
//! - Event filtering and routing

use async_trait::async_trait;
use neomind_core::{
    event::{MetricValue, NeoMindEvent},
    eventbus::EventBus,
    extension::{
        event_dispatcher::EventDispatcher,
        extension_event_subscription::ExtensionEventSubscriptionService,
        system::{Extension, ExtensionError, ExtensionMetadata, ExtensionMetricValue},
        types::DynExtension,
    },
};
use serde_json::json;
use semver::Version;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ============================================================================
// Test Extensions
// ============================================================================

/// Test extension that subscribes to device events
struct DeviceEventExtension {
    event_count: AtomicI64,
    last_event: std::sync::RwLock<Option<serde_json::Value>>,
}

impl DeviceEventExtension {
    fn new() -> Self {
        Self {
            event_count: AtomicI64::new(0),
            last_event: std::sync::RwLock::new(None),
        }
    }

    fn get_event_count(&self) -> i64 {
        self.event_count.load(Ordering::SeqCst)
    }

    async fn get_last_event(&self) -> Option<serde_json::Value> {
        self.last_event.read().unwrap().clone()
    }
}

#[async_trait]
impl Extension for DeviceEventExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "device-event-test",
                "Device Event Test",
                Version::parse("1.0.0").unwrap()
            )
        })
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        Ok(json!({}))
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
        Ok(vec![])
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["DeviceMetric", "DeviceOnline", "DeviceOffline"]
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), ExtensionError> {
        self.event_count.fetch_add(1, Ordering::SeqCst);
        let mut guard = self.last_event.write().unwrap();
        *guard = Some(payload.clone());
        tracing::info!(
            event_type = %event_type,
            payload = ?payload,
            "DeviceEventExtension received event"
        );
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Test extension that subscribes to all events
struct AllEventsExtension {
    event_count: AtomicI64,
    received_events: std::sync::RwLock<Vec<String>>,
}

impl AllEventsExtension {
    fn new() -> Self {
        Self {
            event_count: AtomicI64::new(0),
            received_events: std::sync::RwLock::new(Vec::new()),
        }
    }

    fn get_event_count(&self) -> i64 {
        self.event_count.load(Ordering::SeqCst)
    }

    async fn get_received_events(&self) -> Vec<String> {
        self.received_events.read().unwrap().clone()
    }
}

#[async_trait]
impl Extension for AllEventsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "all-events-test",
                "All Events Test",
                Version::parse("1.0.0").unwrap()
            )
        })
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        Ok(json!({}))
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
        Ok(vec![])
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["all"]
    }

    fn handle_event(&self, event_type: &str, _payload: &serde_json::Value) -> Result<(), ExtensionError> {
        self.event_count.fetch_add(1, Ordering::SeqCst);
        let mut guard = self.received_events.write().unwrap();
        guard.push(event_type.to_string());
        tracing::debug!(
            event_type = %event_type,
            "AllEventsExtension received event"
        );
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Test extension that subscribes to events with prefix matching
struct PrefixMatchExtension {
    event_count: AtomicI64,
}

impl PrefixMatchExtension {
    fn new() -> Self {
        Self {
            event_count: AtomicI64::new(0),
        }
    }

    fn get_event_count(&self) -> i64 {
        self.event_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Extension for PrefixMatchExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "prefix-match-test",
                "Prefix Match Test",
                Version::parse("1.0.0").unwrap()
            )
        })
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        Ok(json!({}))
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
        Ok(vec![])
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["Agent", "Device"]
    }

    fn handle_event(&self, event_type: &str, _payload: &serde_json::Value) -> Result<(), ExtensionError> {
        self.event_count.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(event_type = %event_type, "PrefixMatchExtension received event");
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Test extension that subscribes to no events
struct NoEventsExtension;

#[async_trait]
impl Extension for NoEventsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "no-events-test",
                "No Events Test",
                Version::parse("1.0.0").unwrap()
            )
        })
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        Ok(json!({}))
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
        Ok(vec![])
    }

    fn event_subscriptions(&self) -> &[&str] {
        &[]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_extension_event_subscription_service_start_stop() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus, event_dispatcher);

    let running = service.start();
    assert!(running.load(std::sync::atomic::Ordering::SeqCst));

    service.stop();
    assert!(!running.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn test_extension_event_subscription_exact_match() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(DeviceEventExtension::new())));
    event_dispatcher.register_in_process_extension("device-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();
    
    // Wait for service to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Publish device metric event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 1234567890,
            quality: Some(0.95),
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check that extension received the event
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<DeviceEventExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 1);

    let last_event = ext_ref.get_last_event().await;
    assert!(last_event.is_some());
    let event = last_event.unwrap();
    assert_eq!(event["event_type"], "DeviceMetric");
    assert_eq!(event["payload"]["device_id"], "sensor1");
    assert_eq!(event["payload"]["metric"], "temperature");
}

#[tokio::test]
async fn test_extension_event_subscription_wildcard() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension that subscribes to all events
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(AllEventsExtension::new())));
    event_dispatcher.register_in_process_extension("all-events-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish different types of events
    event_bus
        .publish(NeoMindEvent::DeviceOnline {
            device_id: "device1".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

    event_bus
        .publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test Rule".to_string(),
            trigger_value: 1.0,
            actions: vec![],
            timestamp: 0,
        })
        .await;

    event_bus
        .publish(NeoMindEvent::AgentExecutionStarted {
            agent_id: "agent1".to_string(),
            agent_name: "Test Agent".to_string(),
            execution_id: "exec1".to_string(),
            trigger_type: "manual".to_string(),
            timestamp: 0,
        })
        .await;

    // Wait for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check that extension received all events
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<AllEventsExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 3);

    let received_events = ext_ref.get_received_events().await;
    assert!(received_events.contains(&"DeviceOnline".to_string()));
    assert!(received_events.contains(&"RuleTriggered".to_string()));
    assert!(received_events.contains(&"AgentExecutionStarted".to_string()));
}

#[tokio::test]
async fn test_extension_event_subscription_prefix_match() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension with prefix subscriptions
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(PrefixMatchExtension::new())));
    event_dispatcher.register_in_process_extension("prefix-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish events with different prefixes
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 0,
            quality: None,
        })
        .await;

    event_bus
        .publish(NeoMindEvent::DeviceOnline {
            device_id: "device1".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

    event_bus
        .publish(NeoMindEvent::AgentExecutionStarted {
            agent_id: "agent1".to_string(),
            agent_name: "Test Agent".to_string(),
            execution_id: "exec1".to_string(),
            trigger_type: "manual".to_string(),
            timestamp: 0,
        })
        .await;

    event_bus
        .publish(NeoMindEvent::AgentThinking {
            agent_id: "agent1".to_string(),
            execution_id: "exec1".to_string(),
            step_number: 1,
            step_type: "analysis".to_string(),
            description: "Analyzing data".to_string(),
            details: None,
            timestamp: 0,
        })
        .await;

    // Publish non-matching event
    event_bus
        .publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test Rule".to_string(),
            trigger_value: 1.0,
            actions: vec![],
            timestamp: 0,
        })
        .await;

    // Wait for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check that extension received only matching events
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<PrefixMatchExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 4); // 2 Device + 2 Agent events
}

#[tokio::test]
async fn test_extension_event_subscription_no_events() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension that subscribes to no events
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(NoEventsExtension)));
    event_dispatcher.register_in_process_extension("no-events-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish events
    event_bus
        .publish(NeoMindEvent::DeviceOnline {
            device_id: "device1".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        })
        .await;

    // Wait for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Extension should not have received any events (no way to verify directly,
    // but the test should complete without errors)
}

#[tokio::test]
async fn test_extension_event_subscription_multiple_extensions() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register multiple extensions
    let device_ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(DeviceEventExtension::new())));
    let all_ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(AllEventsExtension::new())));

    event_dispatcher.register_in_process_extension("device-ext".to_string(), device_ext.clone()).await;
    event_dispatcher.register_in_process_extension("all-events-ext".to_string(), all_ext.clone()).await;

    // Start service
    service.start();

    // Publish device event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 0,
            quality: None,
        })
        .await;

    // Publish non-device event
    event_bus
        .publish(NeoMindEvent::RuleTriggered {
            rule_id: "rule1".to_string(),
            rule_name: "Test Rule".to_string(),
            trigger_value: 1.0,
            actions: vec![],
            timestamp: 0,
        })
        .await;

    // Wait for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check device extension received only device event
    let device_guard = device_ext.read().await;
    let device_ref = device_guard.as_ref().as_any().downcast_ref::<DeviceEventExtension>().unwrap();
    assert_eq!(device_ref.get_event_count(), 1);

    // Check all-events extension received both events
    let all_guard = all_ext.read().await;
    let all_ref = all_guard.as_ref().as_any().downcast_ref::<AllEventsExtension>().unwrap();
    assert_eq!(all_ref.get_event_count(), 2);
}

#[tokio::test]
async fn test_extension_event_subscription_isolated_extension() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Create event channel for isolated extension
    let (event_tx, mut event_rx) = mpsc::channel::<(String, serde_json::Value)>(100);

    // Register isolated extension
    event_dispatcher.register_isolated_extension(
        "isolated-ext".to_string(),
        vec!["DeviceMetric".to_string(), "AgentExecutionStarted".to_string()],
        event_tx,
    );

    // Start service
    service.start();
    
    // Wait for service to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Publish device metric event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 0,
            quality: None,
        })
        .await;

    // Wait for event to be pushed
    tokio::time::timeout(tokio::time::Duration::from_millis(500), async {
        let (event_type, payload) = event_rx.recv().await.unwrap();
        assert_eq!(event_type, "DeviceMetric");
        assert_eq!(payload["event_type"], "DeviceMetric");
        assert_eq!(payload["payload"]["device_id"], "sensor1");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_extension_event_subscription_automatic_conversion() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension that subscribes to all events
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(AllEventsExtension::new())));
    event_dispatcher.register_in_process_extension("auto-conv-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Test various event types to ensure automatic conversion
    let test_events = vec![
        NeoMindEvent::DeviceOnline {
            device_id: "device1".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        },
        NeoMindEvent::RuleEvaluated {
            rule_id: "rule1".to_string(),
            rule_name: "Test Rule".to_string(),
            condition_met: true,
            timestamp: 0,
        },
        NeoMindEvent::WorkflowTriggered {
            workflow_id: "workflow1".to_string(),
            trigger_type: "manual".to_string(),
            trigger_data: None,
            execution_id: "exec1".to_string(),
            timestamp: 0,
        },
        NeoMindEvent::AlertCreated {
            alert_id: "alert1".to_string(),
            title: "Test Alert".to_string(),
            severity: "info".to_string(),
            message: "Test message".to_string(),
            timestamp: 0,
        },
        NeoMindEvent::AgentDecision {
            agent_id: "agent1".to_string(),
            execution_id: "exec1".to_string(),
            description: "Test decision".to_string(),
            rationale: "Test rationale".to_string(),
            action: "test_action".to_string(),
            confidence: 0.95,
            timestamp: 0,
        },
        NeoMindEvent::LlmDecisionProposed {
            decision_id: "decision1".to_string(),
            title: "Test Decision".to_string(),
            description: "Test description".to_string(),
            reasoning: "Test reasoning".to_string(),
            actions: vec![],
            confidence: 0.9,
            timestamp: 0,
        },
        NeoMindEvent::ToolExecutionStart {
            tool_name: "test_tool".to_string(),
            arguments: json!({"key": "value"}),
            session_id: None,
            timestamp: 0,
        },
        NeoMindEvent::ExtensionOutput {
            extension_id: "test-ext".to_string(),
            output_name: "test_output".to_string(),
            value: MetricValue::Float(42.0),
            timestamp: 0,
            labels: None,
            quality: None,
        },
        NeoMindEvent::UserMessage {
            session_id: "session1".to_string(),
            content: "Test message".to_string(),
            timestamp: 0,
        },
        NeoMindEvent::Custom {
            event_type: "my_custom_event".to_string(),
            data: json!({"custom": "data"}),
        },
    ];

    // Publish all test events
    for event in test_events {
        event_bus.publish(event).await;
    }

    // Wait for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Check that extension received all events
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<AllEventsExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 10);

    let received_events = ext_ref.get_received_events().await;
    assert!(received_events.contains(&"DeviceOnline".to_string()));
    assert!(received_events.contains(&"RuleEvaluated".to_string()));
    assert!(received_events.contains(&"WorkflowTriggered".to_string()));
    assert!(received_events.contains(&"AlertCreated".to_string()));
    assert!(received_events.contains(&"AgentDecision".to_string()));
    assert!(received_events.contains(&"LlmDecisionProposed".to_string()));
    assert!(received_events.contains(&"ToolExecutionStart".to_string()));
    assert!(received_events.contains(&"ExtensionOutput".to_string()));
    assert!(received_events.contains(&"UserMessage".to_string()));
    assert!(received_events.contains(&"my_custom_event".to_string()));
}

#[tokio::test]
async fn test_extension_event_subscription_event_format() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(DeviceEventExtension::new())));
    event_dispatcher.register_in_process_extension("format-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish event with timestamp
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 1234567890,
            quality: Some(0.95),
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check event format
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<DeviceEventExtension>().unwrap();
    let last_event = ext_ref.get_last_event().await;
    assert!(last_event.is_some());

    let event = last_event.unwrap();
    assert_eq!(event["event_type"], "DeviceMetric");
    assert!(event["payload"].is_object());
    assert!(event["timestamp"].is_i64());
    assert_eq!(event["timestamp"], 1234567890);

    // Check payload structure
    let payload = &event["payload"];
    assert_eq!(payload["device_id"], "sensor1");
    assert_eq!(payload["metric"], "temperature");
    assert_eq!(payload["value"], serde_json::json!(25.5));
    // Quality is a float, so we need to compare with tolerance
    let quality = payload["quality"].as_f64().unwrap();
    assert!((quality - 0.95).abs() < 0.01, "Quality mismatch: {} vs 0.95", quality);
}

#[tokio::test]
async fn test_extension_event_subscription_unregister() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(DeviceEventExtension::new())));
    event_dispatcher.register_in_process_extension("unregister-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 0,
            quality: None,
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check extension received event
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<DeviceEventExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 1);

    // Unregister extension
    event_dispatcher.unregister_extension("unregister-ext");

    // Publish another event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor2".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(26.0),
            timestamp: 0,
            quality: None,
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Extension should not have received the second event
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<DeviceEventExtension>().unwrap();
    assert_eq!(ext_ref.get_event_count(), 1); // Still 1, not 2
}

#[tokio::test]
async fn test_extension_event_subscription_get_subscriptions() {
    let event_dispatcher = Arc::new(EventDispatcher::new());

    // Register extensions
    let device_ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(DeviceEventExtension::new())));
    let all_ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(AllEventsExtension::new())));

    event_dispatcher.register_in_process_extension("device-ext".to_string(), device_ext).await;
    event_dispatcher.register_in_process_extension("all-events-ext".to_string(), all_ext).await;

    // Get subscriptions
    let subscriptions = event_dispatcher.get_subscriptions();

    assert_eq!(subscriptions.len(), 2);
    assert!(subscriptions.contains_key("device-ext"));
    assert!(subscriptions.contains_key("all-events-ext"));

    let device_subs = subscriptions.get("device-ext").unwrap();
    assert_eq!(device_subs.len(), 3);
    assert!(device_subs.contains(&"DeviceMetric".to_string()));
    assert!(device_subs.contains(&"DeviceOnline".to_string()));
    assert!(device_subs.contains(&"DeviceOffline".to_string()));

    let all_subs = subscriptions.get("all-events-ext").unwrap();
    assert_eq!(all_subs.len(), 1);
    assert!(all_subs.contains(&"all".to_string()));
}

#[tokio::test]
async fn test_extension_event_subscription_error_handling() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Create extension that returns error
    struct ErrorExtension;
    #[async_trait]
    impl Extension for ErrorExtension {
        fn metadata(&self) -> &ExtensionMetadata {
            static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
            META.get_or_init(|| {
                ExtensionMetadata::new(
                    "error-ext",
                    "Error Extension",
                    Version::parse("1.0.0").unwrap()
                )
            })
        }

        fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
            vec![]
        }

        fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
            vec![]
        }

        async fn execute_command(
            &self,
            _command: &str,
            _args: &serde_json::Value,
        ) -> Result<serde_json::Value, ExtensionError> {
            Ok(json!({}))
        }

        fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
            Ok(vec![])
        }

        fn event_subscriptions(&self) -> &[&str] {
            &["DeviceMetric"]
        }

        fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<(), ExtensionError> {
            Err(ExtensionError::ExecutionFailed("Test error".to_string()))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // Register extension
    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(ErrorExtension)));
    event_dispatcher.register_in_process_extension("error-ext".to_string(), extension).await;

    // Start service
    service.start();

    // Publish event
    event_bus
        .publish(NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::Float(25.5),
            timestamp: 0,
            quality: None,
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Test should complete without panicking (error is logged but doesn't crash)
}

#[tokio::test]
async fn test_extension_event_subscription_custom_event() {
    let event_bus = Arc::new(EventBus::new());
    let event_dispatcher = Arc::new(EventDispatcher::new());
    let service = ExtensionEventSubscriptionService::new(event_bus.clone(), event_dispatcher.clone());

    // Register extension that subscribes to custom event
    struct CustomEventExtension {
        received_custom: std::sync::RwLock<bool>,
    }

    impl CustomEventExtension {
        fn new() -> Self {
            Self {
                received_custom: std::sync::RwLock::new(false),
            }
        }

        fn received_custom(&self) -> bool {
            *self.received_custom.read().unwrap()
        }
    }

    #[async_trait]
    impl Extension for CustomEventExtension {
        fn metadata(&self) -> &ExtensionMetadata {
            static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
            META.get_or_init(|| {
                ExtensionMetadata::new(
                    "custom-event-ext",
                    "Custom Event Extension",
                    Version::parse("1.0.0").unwrap()
                )
            })
        }

        fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDescriptor> {
            vec![]
        }

        fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
            vec![]
        }

        async fn execute_command(
            &self,
            _command: &str,
            _args: &serde_json::Value,
        ) -> Result<serde_json::Value, ExtensionError> {
            Ok(json!({}))
        }

        fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>, ExtensionError> {
            Ok(vec![])
        }

        fn event_subscriptions(&self) -> &[&str] {
            &["my_custom_event"]
        }

        fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), ExtensionError> {
            if event_type == "my_custom_event" {
                let mut guard = self.received_custom.write().unwrap();
                *guard = true;
                assert_eq!(payload["event_type"], "my_custom_event");
                assert_eq!(payload["payload"]["data"]["key"], "value");
            }
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let extension: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(CustomEventExtension::new())));
    event_dispatcher.register_in_process_extension("custom-ext".to_string(), extension.clone()).await;

    // Start service
    service.start();

    // Publish custom event
    event_bus
        .publish(NeoMindEvent::Custom {
            event_type: "my_custom_event".to_string(),
            data: json!({"key": "value"}),
        })
        .await;

    // Wait for event to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check that extension received custom event
    let ext_guard = extension.read().await;
    let ext_ref = ext_guard.as_ref().as_any().downcast_ref::<CustomEventExtension>().unwrap();
    assert!(ext_ref.received_custom());
}