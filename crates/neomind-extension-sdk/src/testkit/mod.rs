//! Extension Test Kit — protocol-level testing infrastructure.
//!
//! Closes the gap between "unit test that constructs a struct and calls a
//! method" and "e2e test with a real server". The test kit routes messages
//! through the SAME IPC protocol (IpcMessage serialization, command
//! dispatch, event push, capability invocation) as the real
//! neomind-extension-runner, but over in-memory channels instead of
//! stdin/stdout pipes.
//!
//! The bugs this catches that plain unit tests cannot:
//! - Deadlocks in handle_event / execute_command (the mock runner runs on
//!   the same tokio runtime with timing assertions)
//! - Capability calls with wrong parameters (the mock records everything)
//! - Event dispatch routing errors (inject events, assert they arrive)
//! - Stream session lifecycle leaks (open → push → close, check state)
//! - Lock ordering violations (concurrent event injection)
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use neomind_extension_sdk::testkit::*;
//!
//! #[tokio::test]
//! async fn test_analyze_command() {
//!     let mut kit = TestKit::new(MyExtension::new());
//!     kit.start().await;
//!
//!     let result = kit.execute_command("analyze", json!({"image": "..."})).await
//!         .expect("command should complete within 5s");
//!     assert!(result["success"].as_bool().unwrap());
//!
//!     // Verify capability calls
//!     let calls = kit.capability_calls("device_metrics_write");
//!     assert_eq!(calls.len(), 3); // detections, inference_ms, labels
//! }
//! ```
//!
//! # Feature Flag
//!
//! Enable via `features = ["testkit"]` in `[dev-dependencies]`.

#[cfg(feature = "testkit")]
pub mod mock_runner;
#[cfg(feature = "testkit")]
pub mod mock_capability;
#[cfg(feature = "testkit")]
pub mod event_injector;
#[cfg(feature = "testkit")]
pub mod assertions;

#[cfg(feature = "testkit")]
pub use mock_runner::{TestKit, TestKitConfig};
#[cfg(feature = "testkit")]
pub use mock_capability::{CapabilityRecorder, RecordedCall, MockCapabilityProvider};
#[cfg(feature = "testkit")]
pub use event_injector::{EventInjector, DeviceMetricEvent as InjectedMetric};
#[cfg(feature = "testkit")]
pub use assertions::{assert_event_processed_within, TimingViolation};

#[cfg(all(feature = "testkit", test))]
mod testkit_tests;
