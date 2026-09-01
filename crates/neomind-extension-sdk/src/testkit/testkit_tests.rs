//! Tests for the testkit itself — validates that the infrastructure
//! correctly detects the classes of bugs it was built to catch.

#![cfg(feature = "testkit")]

use crate::testkit::*;
use crate::*;
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

/// A minimal extension for testing the testkit.
struct TestExtension {
    command_count: AtomicU64,
    event_count: AtomicU64,
    // Simulate slow processing for timeout tests
    processing_delay_ms: u64,
}

impl TestExtension {
    fn new() -> Self {
        Self {
            command_count: AtomicU64::new(0),
            event_count: AtomicU64::new(0),
            processing_delay_ms: 0,
        }
    }

    fn with_delay(ms: u64) -> Self {
        Self {
            processing_delay_ms: ms,
            ..Self::new()
        }
    }
}

#[async_trait]
impl Extension for TestExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("test-ext", "Test Extension", "1.0.0")
                .with_description("Internal test extension")
        })
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.command_count.fetch_add(1, Ordering::SeqCst);

        if self.processing_delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.processing_delay_ms)).await;
        }

        match command {
            "echo" => Ok(json!({"echo": args})),
            "fail" => Err(ExtensionError::ExecutionFailed("intentional failure".into())),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn handle_event(&self, event_type: &str, _payload: &serde_json::Value) -> Result<()> {
        self.event_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![ExtensionMetricValue {
            name: "commands".to_string(),
            value: ParamMetricValue::Integer(self.command_count.load(Ordering::SeqCst) as i64),
            timestamp: 0,
        }])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn test_command_execution() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    let result = kit
        .execute_command("echo", &json!({"msg": "hello"}))
        .await
        .expect("echo should succeed");
    assert_eq!(result["echo"]["msg"], "hello");
}

#[tokio::test]
async fn test_command_error_propagation() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    let err = kit.execute_command("fail", &json!({})).await.unwrap_err();
    assert!(err.to_string().contains("intentional failure"));
}

#[tokio::test]
async fn test_unknown_command() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    let err = kit.execute_command("nope", &json!({})).await.unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("CommandNotFound"));
}

#[tokio::test]
async fn test_event_injection() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    let duration = kit
        .inject_device_metric("device-1", "temperature", json!({"Float": 25.5}))
        .await
        .expect("event should process");

    assert!(duration.as_millis() < 100, "event processing should be <100ms, got {:?}", duration);
}

#[tokio::test]
async fn test_timeout_detection() {
    // Extension that takes 100ms — should complete within 1s budget
    let mut kit = TestKit::with_config(
        TestExtension::with_delay(100),
        TestKitConfig {
            command_timeout: std::time::Duration::from_secs(1),
            ..Default::default()
        },
    );
    kit.start().await;

    let result = kit.execute_command("echo", &json!({})).await;
    assert!(result.is_ok(), "100ms should complete within 1s");
}

#[tokio::test]
async fn test_timeout_actually_times_out() {
    // Extension that takes 5s — should timeout with 1s budget
    let mut kit = TestKit::with_config(
        TestExtension::with_delay(5000),
        TestKitConfig {
            command_timeout: std::time::Duration::from_secs(1),
            ..Default::default()
        },
    );
    kit.start().await;

    let err = kit.execute_command("echo", &json!({})).await.unwrap_err();
    assert!(
        err.to_string().contains("timed out") || err.to_string().contains("Timeout"),
        "should detect timeout, got: {err}"
    );
}

#[tokio::test]
async fn test_capability_recording() {
    let recorder = CapabilityRecorder::new();

    recorder.record("device_metrics_write", &json!({"device_id": "d1", "metric": "temp", "value": 25}));
    recorder.record("device_metrics_write", &json!({"device_id": "d1", "metric": "hum", "value": 60}));
    recorder.record("event_publish", &json!({"event_type": "test"}));

    assert_eq!(recorder.call_count(), 3);
    assert_eq!(recorder.count_for("device_metrics_write"), 2);
    assert_eq!(recorder.count_for("event_publish"), 1);
    assert_eq!(recorder.count_for("nonexistent"), 0);

    let writes = recorder.calls_for("device_metrics_write");
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0]["metric"], "temp");
    assert_eq!(writes[1]["metric"], "hum");
}

#[tokio::test]
async fn test_capability_mock_programming() {
    use std::sync::Arc;

    let recorder = Arc::new(CapabilityRecorder::new());
    let provider = MockCapabilityProvider::new(recorder.clone());

    // Default: echo response
    let result = provider.invoke("some_capability", &json!({"x": 1}));
    assert!(result.is_ok());

    // Programmed success
    provider.set_response("some_capability", json!({"custom": true}));
    let result = provider.invoke("some_capability", &json!({"x": 2})).unwrap();
    assert_eq!(result["custom"], true);

    // Programmed failure
    provider.set_error("some_capability", "simulated failure");
    let err = provider.invoke("some_capability", &json!({"x": 3})).unwrap_err();
    assert!(err.contains("simulated failure"));

    // Recording happened for all calls
    assert_eq!(recorder.count_for("some_capability"), 3);
}

#[tokio::test]
async fn test_produce_metrics_through_kit() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    // Execute a command first so the counter is non-zero
    kit.execute_command("echo", &json!({})).await.unwrap();

    let metrics = kit.produce_metrics().await.expect("metrics should produce");
    assert!(!metrics.is_empty());
}

#[tokio::test]
async fn test_concurrent_event_injection() {
    let mut kit = TestKit::new(TestExtension::new());
    kit.start().await;

    // Inject events while running a command — should not deadlock
    let result = kit
        .with_concurrent_events(
            "test-device",
            std::time::Duration::from_millis(10),
            async {
                // Simulate concurrent work
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                "concurrent work done"
            },
        )
        .await
        .expect("should not deadlock");

    assert_eq!(result, "concurrent work done");
}

#[tokio::test]
async fn test_metadata_access() {
    let mut kit = TestKit::new(TestExtension::new());
    assert_eq!(kit.metadata().id, "test-ext");
    assert_eq!(kit.metadata().name, "Test Extension");
    assert_eq!(kit.metadata().version, "1.0.0");
}
