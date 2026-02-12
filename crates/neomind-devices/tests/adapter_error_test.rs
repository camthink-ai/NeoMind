//! Device Adapter Error Handling Tests
//!
//! Tests error handling in device adapters including:
//! - Connection failures
//! - Invalid configurations
//! - State transitions

use neomind_devices::adapter::{Adapter, AdapterError};
use anyhow::anyhow;

#[tokio::test]
async fn test_error_display_configuration() {
    // Test AdapterError Configuration variant
    use neomind_devices::adapter::{Adapter, AdapterError};

    let error = AdapterError::Configuration("test error".to_string());

    // Verify error message contains the error text
    let error_msg = format!("{}", error);
    assert!(error_msg.contains("Configuration error: test error"));
}

#[tokio::test]
async fn test_error_display_connection() {
    // Test AdapterError Connection variant
    use neomind_devices::adapter::{Adapter, AdapterError};

    let error = AdapterError::Connection("connection failed".to_string());

    // Verify error message
    let error_msg = format!("{}", error);
    assert!(error_msg.contains("connection failed"));
}

#[tokio::test]
async fn test_error_display_communication() {
    // Test AdapterError Communication variant
    use neomind_devices::adapter::{Adapter, AdapterError};

    let error = AdapterError::Communication("send failed".to_string());

    // Verify error message
    let error_msg = format!("{}", error);
    assert!(error_msg.contains("send failed"));
}

#[tokio::test]
async fn test_error_display_stopped() {
    // Test AdapterError Stopped variant
    use neomind_devices::adapter::{Adapter, AdapterError};

    let error = AdapterError::Stopped("Adapter stopped".to_string());

    // Verify error message
    let error_msg = format!("{}", error);
    assert!(error_msg.contains("Adapter stopped"));
}

#[tokio::test]
async fn test_error_display_other() {
    // Test AdapterError Other variant with anyhow
    use neomind_devices::adapter::{Adapter, AdapterError};
    use anyhow::anyhow;

    let inner_error = std::io::Error::new(std::io::ErrorKind::NotFound, "test".to_string());
    let error = AdapterError::Other(inner_error);

    // Verify error message
    let error_msg = format!("{:?}", error);
    assert!(error_msg.contains("test") || error_msg.contains("NotFound"));
}
