//! Simple standalone test for EventBus functionality
use neomind_core::{event::NeoMindEvent, EventBus, MetricValue};

#[tokio::main]
async fn main() {
    println!("=== EventBus ExtensionOutput Event Test ===\n");

    // Create event bus
    let event_bus = EventBus::new();
    let mut receiver = event_bus.subscribe();

    // Start listening for events
    let event_bus_clone = event_bus.clone();
    let handle = tokio::spawn(async move {
        let mut count = 0;
        while let Some((event, _meta)) = receiver.recv().await {
            count += 1;
            println!(
                "[{}] Event #{}: {}",
                std::any::type_name::<NeoMindEvent>(),
                count,
                event.type_name()
            );
        }
        println!("Total events received: {}", count);
    });

    // Give listener time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Test: Publish an ExtensionOutput event manually
    println!("\n--- Publishing ExtensionOutput event ---");
    let test_event = NeoMindEvent::ExtensionOutput {
        extension_id: "test_ext".to_string(),
        output_name: "temperature".to_string(),
        value: MetricValue::Float(23.5),
        timestamp: chrono::Utc::now().timestamp(),
        labels: None,
        quality: None,
    };

    let published = event_bus.publish(test_event).await;
    println!("Published successfully: {}", published);

    // Give time for event to be received
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Clean up
    drop(event_bus);
    handle.await.unwrap();

    println!("\n=== Test Complete ===");
}
