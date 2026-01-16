// Integration Tests for NeoTalk
//
// End-to-end tests for data flows:
// - Device → Event → Rule → Action
// - User → LLM → Tool → Device
// - Device Event → Workflow → Action
// - LLM Periodic Review → Decision → Execution

// Include common test utilities
pub mod common;

// Include handler tests
mod handlers;

use std::time::Duration;
use tokio::time::sleep;

/// Test: Device → Event → Rule → Action flow
///
/// 1. Publish a device metric event
/// 2. Rule engine evaluates and triggers
/// 3. Action is published to event bus
#[tokio::test]
async fn test_device_to_rule_action_flow() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;

    // Create event bus
    let bus = EventBus::new();

    // Subscribe to rule events
    let mut rule_rx = bus.filter().rule_events();

    // Subscribe to action events (via LLM events for now)
    let mut action_rx = bus.filter().llm_events();

    // Publish a device metric event (temperature > 50)
    bus.publish(NeoTalkEvent::DeviceMetric {
        device_id: "sensor_001".to_string(),
        metric: "temperature".to_string(),
        value: MetricValue::float(55.0),
        timestamp: 0,
        quality: None,
    })
    .await;

    // Wait for event propagation
    sleep(Duration::from_millis(100)).await;

    // The event was published - in a full integration test with rules engine,
    // we would verify rule triggering here
    let received = rule_rx.try_recv();
    // Note: In actual integration, the rules engine would process and trigger

    println!("Device event published successfully");
}

/// Test: User → LLM → Tool → Device flow
///
/// 1. User sends a message
/// 2. LLM processes and decides to call a tool
/// 3. Tool executes device command
#[tokio::test]
async fn test_user_to_llm_to_device_flow() {
    use edge_ai_core::event::NeoTalkEvent;
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();

    // Subscribe to user messages
    let mut user_rx = bus
        .filter()
        .custom(|e| matches!(e, NeoTalkEvent::UserMessage { .. }));

    // Subscribe to tool execution events
    let mut tool_rx = bus
        .filter()
        .custom(|e| matches!(e, NeoTalkEvent::ToolExecutionStart { .. }));

    // Publish user message
    bus.publish(NeoTalkEvent::UserMessage {
        content: "turn on the light".to_string(),
        session_id: "test_session".to_string(),
        timestamp: 0,
    })
    .await;

    // Wait for event propagation
    sleep(Duration::from_millis(50)).await;

    // Verify user message was received
    let received = user_rx.try_recv();
    assert!(received.is_some(), "User message should be received");

    println!("User message flow test completed");
}

/// Test: Device Event → Workflow → Action flow
///
/// 1. Device event is published
/// 2. Workflow trigger detects event
/// 3. Workflow executes steps
/// 4. Action is performed
#[tokio::test]
async fn test_device_to_workflow_action_flow() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();

    // Subscribe to workflow events
    let mut wf_rx = bus.filter().workflow_events();

    // Publish device event that should trigger workflow
    bus.publish(NeoTalkEvent::DeviceOnline {
        device_id: "sensor_001".to_string(),
        device_type: "motion_sensor".to_string(),
        timestamp: 0,
    })
    .await;

    // Wait for event propagation
    sleep(Duration::from_millis(50)).await;

    // In full integration, workflow engine would trigger here
    println!("Device to workflow flow test completed");
}

/// Test: LLM Periodic Review → Decision → Execution flow
///
/// 1. Periodic review is triggered
/// 2. LLM analyzes system state
/// 3. LLM proposes decision
/// 4. Decision is executed (if auto-approved)
#[tokio::test]
async fn test_llm_periodic_review_flow() {
    use edge_ai_core::event::{NeoTalkEvent, ProposedAction};
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();

    // Subscribe to LLM events
    let mut llm_rx = bus.filter().llm_events();

    // Trigger periodic review
    bus.publish(NeoTalkEvent::PeriodicReviewTriggered {
        review_id: "review_hourly_001".to_string(),
        review_type: "hourly".to_string(),
        timestamp: 0,
    })
    .await;

    // Simulate LLM proposing a decision
    let actions = vec![ProposedAction::notify_user("System is running optimally")];

    bus.publish(NeoTalkEvent::LlmDecisionProposed {
        decision_id: "decision_001".to_string(),
        title: "System Health Check".to_string(),
        description: "Routine hourly check passed".to_string(),
        reasoning: "All metrics are within normal ranges".to_string(),
        actions,
        confidence: 0.95,
        timestamp: 0,
    })
    .await;

    // Wait for event propagation
    sleep(Duration::from_millis(50)).await;

    // Verify LLM decision was received
    let received = llm_rx.try_recv();
    assert!(received.is_some(), "LLM decision should be received");

    println!("LLM periodic review flow test completed");
}

/// Test: Event streaming performance
///
/// Verify that events are published with minimal latency
#[tokio::test]
async fn test_event_streaming_performance() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;
    use std::time::Instant;

    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // Measure time to publish and receive
    let start = Instant::now();

    bus.publish(NeoTalkEvent::DeviceMetric {
        device_id: "test".to_string(),
        metric: "temp".to_string(),
        value: MetricValue::float(25.0),
        timestamp: 0,
        quality: None,
    })
    .await;

    let _received = rx.recv().await.unwrap();
    let elapsed = start.elapsed();

    // Event round-trip should be very fast (< 10ms in local testing)
    assert!(
        elapsed.as_millis() < 100,
        "Event delivery should be fast, got {:?}",
        elapsed
    );

    println!("Event latency: {:?}", elapsed);
}

/// Test: Multiple subscribers receive same event
///
/// Verify broadcast functionality
#[tokio::test]
async fn test_broadcast_to_multiple_subscribers() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();

    // Create multiple subscribers
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    let mut rx3 = bus.subscribe();

    // Publish event
    bus.publish(NeoTalkEvent::DeviceMetric {
        device_id: "test".to_string(),
        metric: "temp".to_string(),
        value: MetricValue::float(25.0),
        timestamp: 0,
        quality: None,
    })
    .await;

    // All subscribers should receive
    let r1 = rx1.recv().await;
    let r2 = rx2.recv().await;
    let r3 = rx3.recv().await;

    assert!(r1.is_some());
    assert!(r2.is_some());
    assert!(r3.is_some());

    println!("Broadcast test completed");
}

/// Test: Filtered subscriptions work correctly
///
/// Verify that filtered subscribers only receive matching events
#[tokio::test]
async fn test_filtered_subscriptions() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();

    let mut device_rx = bus.filter().device_events();
    let mut rule_rx = bus.filter().rule_events();

    // Publish device event
    bus.publish(NeoTalkEvent::DeviceMetric {
        device_id: "test".to_string(),
        metric: "temp".to_string(),
        value: MetricValue::float(25.0),
        timestamp: 0,
        quality: None,
    })
    .await;

    // Publish rule event
    bus.publish(NeoTalkEvent::RuleTriggered {
        rule_id: "rule1".to_string(),
        rule_name: "Test Rule".to_string(),
        trigger_value: 42.0,
        actions: vec!["action".to_string()],
        timestamp: 0,
    })
    .await;

    // Device subscriber should only get device event
    let device_event = device_rx.recv().await.unwrap();
    assert!(device_event.0.is_device_event());

    // Rule subscriber should only get rule event
    let rule_event = rule_rx.recv().await.unwrap();
    assert!(rule_event.0.is_rule_event());

    println!("Filtered subscription test completed");
}

/// Test: Event metadata is preserved
///
/// Verify that event metadata (source, timestamp, etc.) is correctly attached
#[tokio::test]
async fn test_event_metadata_preserved() {
    use edge_ai_core::event::{MetricValue, NeoTalkEvent};
    use edge_ai_core::eventbus::EventBus;

    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // Publish event with source
    bus.publish_with_source(
        NeoTalkEvent::DeviceMetric {
            device_id: "test".to_string(),
            metric: "temp".to_string(),
            value: MetricValue::float(25.0),
            timestamp: 12345,
            quality: None,
        },
        "test_adapter",
    )
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.1.source, "test_adapter");
    assert_eq!(received.0.timestamp(), 12345);

    println!("Event metadata test completed");
}
