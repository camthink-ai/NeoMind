//! Common test utilities for API tests.
//!
//! Tests should use unique usernames/IDs to avoid conflicts when running in parallel.

/// Create a mock server state for testing.
///
/// Note: Tests should use unique usernames/IDs to avoid conflicts when running in parallel.
/// Each call creates a completely isolated ServerState with in-memory storage.
pub async fn create_test_server_state() -> neomind_api::handlers::ServerState {
    neomind_api::handlers::ServerState::new_for_testing().await
}

/// Register an always-ACKing stub MQTT adapter into the test state.
///
/// The BindIncomplete strictness (2026-11-17 review) makes device
/// registration fail with "No MQTT adapter found" when the fixture has no
/// adapter — correct in production, but tests that only need devices to
/// EXIST (references/mentions/e2e) and onboarding tests that need a
/// subscribe SUBACK must provide a controllable adapter instead of relaxing
/// the production rule. Register this stub in those setups.
pub async fn register_mqtt_stub(state: &neomind_api::handlers::ServerState) {
    use std::pin::Pin;

    use async_trait::async_trait;
    use futures::Stream;
    use neomind_devices::adapter::{AdapterResult, DeviceAdapter, DeviceEvent};

    /// A minimal "mqtt"-typed adapter whose subscribe always ACKs.
    struct MqttStubAdapter;

    #[async_trait]
    impl DeviceAdapter for MqttStubAdapter {
        fn name(&self) -> &str {
            "test-internal-mqtt"
        }

        fn adapter_type(&self) -> &'static str {
            "mqtt"
        }

        fn is_running(&self) -> bool {
            true
        }

        async fn start(&self) -> AdapterResult<()> {
            Ok(())
        }

        async fn stop(&self) -> AdapterResult<()> {
            Ok(())
        }

        fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
            Box::pin(futures::stream::pending())
        }

        fn device_count(&self) -> usize {
            0
        }

        fn list_devices(&self) -> Vec<String> {
            Vec::new()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn connection_status(&self) -> neomind_devices::adapter::ConnectionStatus {
            neomind_devices::adapter::ConnectionStatus::Connected
        }

        async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
            // The controllable SUBACK: registration tests need subscribe to
            // succeed deterministically.
            Ok(())
        }

        async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
            Ok(())
        }

        async fn send_command(
            &self,
            _device_id: &str,
            _command: &str,
            _payload: String,
            _topic: Option<String>,
        ) -> AdapterResult<()> {
            Err(neomind_devices::adapter::AdapterError::Communication(
                "test stub does not deliver commands".to_string(),
            ))
        }
    }

    state
        .devices
        .service
        .register_adapter(
            "internal-mqtt".to_string(),
            std::sync::Arc::new(MqttStubAdapter),
        )
        .await;
}
