//! Comprehensive tests for the EventBus module.
//!
//! Tests include:
//! - Basic publish/subscribe
//! - Multiple subscribers
//! - Filtered subscriptions
//! - Event metadata
//! - Concurrent operations
//! - Priority event bus

use neomind_core::{
    event::{EventMetadata, MetricValue, NeoMindEvent, ProposedAction as Action},
    eventbus::{EventBus, FilterBuilder, SharedEventBus},
    priority_eventbus::{EventPriority, PriorityEventBus},
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_event_bus_basic_publish_subscribe() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "DeviceOnline");
}

#[tokio::test]
async fn test_event_bus_multiple_subscribers() {
    let bus = EventBus::new();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    let mut rx3 = bus.subscribe();

    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    // All subscribers should receive the event
    let event1 = rx1.recv().await.unwrap();
    let event2 = rx2.recv().await.unwrap();
    let event3 = rx3.recv().await.unwrap();

    assert_eq!(event1.0.type_name(), "DeviceOnline");
    assert_eq!(event2.0.type_name(), "DeviceOnline");
    assert_eq!(event3.0.type_name(), "DeviceOnline");
}

#[tokio::test]
async fn test_event_bus_filtered_device_events() {
    let bus = EventBus::new();
    let mut rx = bus.filter().device_events();

    // Publish device event
    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "sensor1".to_string(),
        device_type: "temperature".to_string(),
        timestamp: 1000,
    })
    .await;

    // Publish non-device event
    bus.publish(NeoMindEvent::RuleTriggered {
        rule_id: "rule1".to_string(),
        rule_name: "Test Rule".to_string(),
        trigger_value: 25.0,
        actions: vec![],
        timestamp: 1001,
    })
    .await;

    // Should only receive the device event
    let received = rx.recv().await.unwrap();
    assert!(matches!(received.0, NeoMindEvent::DeviceOnline { .. }));
}

#[tokio::test]
async fn test_event_bus_filtered_rule_events() {
    let bus = EventBus::new();
    let mut rx = bus.filter().rule_events();

    bus.publish(NeoMindEvent::RuleTriggered {
        rule_id: "rule1".to_string(),
        rule_name: "Temperature Alert".to_string(),
        trigger_value: 30.0,
        actions: vec!["High temperature".to_string()],
        timestamp: 1000,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "RuleTriggered");
}

#[tokio::test]
async fn test_event_bus_filtered_llm_events() {
    let bus = EventBus::new();
    let mut rx = bus.filter().llm_events();

    bus.publish(NeoMindEvent::LlmDecisionProposed {
        decision_id: "decision1".to_string(),
        title: "Adjust Thermostat".to_string(),
        description: "Temperature is too high".to_string(),
        reasoning: "Based on sensor data".to_string(),
        actions: vec![Action::control_device(
            "thermostat",
            "set_temp",
            serde_json::json!("22")
        )],
        confidence: 0.95,
        timestamp: 1000,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "LlmDecisionProposed");
}

#[tokio::test]
async fn test_event_bus_filtered_alert_events() {
    let bus = EventBus::new();
    let mut rx = bus.filter().alert_events();

    bus.publish(NeoMindEvent::AlertCreated {
        alert_id: "alert1".to_string(),
        title: "High Temperature".to_string(),
        severity: "warning".to_string(),
        message: "Temperature exceeded threshold".to_string(),
        timestamp: 1000,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "AlertCreated");
}

#[tokio::test]
async fn test_event_bus_custom_filter() {
    let bus = EventBus::new();
    let mut rx = bus
        .filter()
        .custom(|event| matches!(event, NeoMindEvent::DeviceMetric { .. }));

    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    bus.publish(NeoMindEvent::DeviceMetric {
        device_id: "sensor1".to_string(),
        metric: "temperature".to_string(),
        value: MetricValue::Float(25.5),
        timestamp: 1000,
        quality: None,
    })
    .await;

    // Should only receive the metric event
    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(received.0, NeoMindEvent::DeviceMetric { .. }));
}

#[tokio::test]
async fn test_event_bus_publish_with_source() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish_with_source(
        NeoMindEvent::DeviceOnline {
            device_id: "device1".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        },
        "mqtt_adapter",
    )
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.1.source, "mqtt_adapter");
}

#[tokio::test]
async fn test_event_bus_subscriber_count() {
    let bus = EventBus::new();

    assert_eq!(bus.subscriber_count(), 0);

    let _rx1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);

    let _rx2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    // Note: subscriber count may not immediately decrease due to Arc
}

#[tokio::test]
async fn test_event_bus_shared() {
    let bus: SharedEventBus = Arc::new(EventBus::new());
    let bus_clone = Arc::clone(&bus);

    let mut rx = bus.subscribe();

    tokio::spawn(async move {
        bus_clone
            .publish(NeoMindEvent::DeviceOnline {
                device_id: "device1".to_string(),
                device_type: "sensor".to_string(),
                timestamp: 0,
            })
            .await;
    });

    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received.0.type_name(), "DeviceOnline");
}

#[tokio::test]
async fn test_event_bus_concurrent_publish() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // Spawn multiple tasks publishing concurrently
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let bus_clone = bus.clone();
            tokio::spawn(async move {
                bus_clone
                    .publish(NeoMindEvent::DeviceOnline {
                        device_id: format!("device{}", i),
                        device_type: "sensor".to_string(),
                        timestamp: 0,
                    })
                    .await;
            })
        })
        .collect();

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Receive events
    let mut count = 0;
    for _ in 0..10 {
        match timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(_)) => count += 1,
            _ => break,
        }
    }

    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_event_bus_device_metric_event() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    bus.publish(NeoMindEvent::DeviceMetric {
        device_id: "sensor1".to_string(),
        metric: "temperature".to_string(),
        value: MetricValue::Float(25.5),
        timestamp: 1000,
        quality: Some(0.95),
    })
    .await;

    let received = rx.recv().await.unwrap();

    match received.0 {
        NeoMindEvent::DeviceMetric {
            device_id,
            metric,
            value,
            quality,
            ..
        } => {
            assert_eq!(device_id, "sensor1");
            assert_eq!(metric, "temperature");
            match value {
                MetricValue::Float(v) => assert_eq!(v, 25.5),
                _ => panic!("Expected Float value"),
            }
            assert_eq!(quality, Some(0.95));
        }
        _ => panic!("Expected DeviceMetric event"),
    }
}

#[tokio::test]
async fn test_event_bus_try_recv() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // No event yet
    assert!(rx.try_recv().is_none());

    // Publish an event
    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    // Should be able to try_recv now
    let received = rx.try_recv().unwrap();
    assert_eq!(received.0.type_name(), "DeviceOnline");

    // No more events
    assert!(rx.try_recv().is_none());
}

#[tokio::test]
async fn test_event_bus_with_capacity() {
    let bus = EventBus::with_capacity(100);
    let mut rx = bus.subscribe();

    assert_eq!(bus.subscriber_count(), 1);

    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "DeviceOnline");
}

#[tokio::test]
async fn test_event_bus_with_name() {
    let bus = EventBus::with_name("test_bus".to_string());
    assert_eq!(bus.name(), "test_bus");

    let default_bus = EventBus::new();
    assert_eq!(default_bus.name(), "default");
}

#[tokio::test]
async fn test_priority_event_bus() {
    let inner_bus = EventBus::new();
    let priority_bus = PriorityEventBus::new(inner_bus);

    // Publish events with different priorities
    priority_bus
        .publish_critical(NeoMindEvent::AlertCreated {
            alert_id: "alert1".to_string(),
            title: "Critical Alert".to_string(),
            severity: "critical".to_string(),
            message: "System failure".to_string(),
            timestamp: 1000,
        })
        .await;

    priority_bus
        .publish_with_priority(
            NeoMindEvent::DeviceOnline {
                device_id: "device1".to_string(),
                device_type: "sensor".to_string(),
                timestamp: 1001,
            },
            EventPriority::Normal,
        )
        .await;

    priority_bus
        .publish_with_priority(
            NeoMindEvent::DeviceMetric {
                device_id: "sensor1".to_string(),
                metric: "temperature".to_string(),
                value: MetricValue::Float(25.0),
                timestamp: 1002,
                quality: None,
            },
            EventPriority::Low,
        )
        .await;

    // Check pending count
    let pending = priority_bus.pending_count().await;
    assert_eq!(pending, 3);

    // Process the queue
    let processed = priority_bus.process_queue(10).await;
    assert_eq!(processed, 3);
}

#[tokio::test]
async fn test_event_bus_multiple_filter_types() {
    let bus = EventBus::new();

    let mut device_rx = bus.filter().device_events();
    let mut rule_rx = bus.filter().rule_events();
    let mut llm_rx = bus.filter().llm_events();
    let mut alert_rx = bus.filter().alert_events();

    // Publish one of each type
    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    bus.publish(NeoMindEvent::RuleTriggered {
        rule_id: "rule1".to_string(),
        rule_name: "Test".to_string(),
        trigger_value: 1.0,
        actions: vec![],
        timestamp: 0,
    })
    .await;

    bus.publish(NeoMindEvent::LlmDecisionProposed {
        decision_id: "decision1".to_string(),
        title: "Test".to_string(),
        description: "Test".to_string(),
        reasoning: "Test".to_string(),
        actions: vec![],
        confidence: 0.5,
        timestamp: 0,
    })
    .await;

    bus.publish(NeoMindEvent::AlertCreated {
        alert_id: "alert1".to_string(),
        title: "Test".to_string(),
        severity: "info".to_string(),
        message: "Test".to_string(),
        timestamp: 0,
    })
    .await;

    // Each receiver should get its respective event
    assert_eq!(
        timeout(Duration::from_millis(100), device_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .0
            .type_name(),
        "DeviceOnline"
    );
    assert_eq!(
        timeout(Duration::from_millis(100), rule_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .0
            .type_name(),
        "RuleTriggered"
    );
    assert_eq!(
        timeout(Duration::from_millis(100), llm_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .0
            .type_name(),
        "LlmDecisionProposed"
    );
    assert_eq!(
        timeout(Duration::from_millis(100), alert_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .0
            .type_name(),
        "AlertCreated"
    );
}

#[tokio::test]
async fn test_event_metadata_builder() {
    let metadata = EventMetadata::new("test_source");

    assert_eq!(metadata.source, "test_source");
}

#[tokio::test]
async fn test_metric_value_variants() {
    let float_val = MetricValue::Float(25.5);
    let int_val = MetricValue::Integer(100);
    let string_val = MetricValue::String("test".to_string());
    let bool_val = MetricValue::Boolean(true);

    match float_val {
        MetricValue::Float(v) => assert_eq!(v, 25.5),
        _ => panic!("Expected Float"),
    }

    match int_val {
        MetricValue::Integer(v) => assert_eq!(v, 100),
        _ => panic!("Expected Integer"),
    }

    match string_val {
        MetricValue::String(v) => assert_eq!(v, "test"),
        _ => panic!("Expected String"),
    }

    match bool_val {
        MetricValue::Boolean(v) => assert_eq!(v, true),
        _ => panic!("Expected Boolean"),
    }
}

#[tokio::test]
async fn test_priority_event_bus_max_queue() {
    let inner_bus = EventBus::new();
    let priority_bus = PriorityEventBus::new(inner_bus).with_max_queue_size(5);

    // Fill the queue
    for i in 0..5 {
        priority_bus
            .publish_with_priority(
                NeoMindEvent::DeviceOnline {
                    device_id: format!("device{}", i),
                    device_type: "sensor".to_string(),
                    timestamp: i as i64,
                },
                EventPriority::Normal,
            )
            .await;
    }

    assert_eq!(priority_bus.pending_count().await, 5);

    // Low priority event should be dropped when queue is full
    let result = priority_bus
        .publish_with_priority(
            NeoMindEvent::DeviceOnline {
                device_id: "overflow".to_string(),
                device_type: "sensor".to_string(),
                timestamp: 100,
            },
            EventPriority::Low,
        )
        .await;

    assert!(!result, "Low priority event should be dropped when queue is full");
}

#[tokio::test]
async fn test_event_bus_clone() {
    let bus = EventBus::new();
    let bus_clone = bus.clone();

    let mut rx = bus_clone.subscribe();

    bus.publish(NeoMindEvent::DeviceOnline {
        device_id: "device1".to_string(),
        device_type: "sensor".to_string(),
        timestamp: 0,
    })
    .await;

    let received = rx.recv().await.unwrap();
    assert_eq!(received.0.type_name(), "DeviceOnline");
}
