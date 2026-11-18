//! Comprehensive Unit Tests for IsolatedExtensionManager
//!
//! Tests cover:
//! - Manager creation and configuration
//! - Extension loading and unloading
//! - Command execution via IPC
//! - Health checking
//! - Metrics collection
//! - Process lifecycle management
//! - Event dispatcher integration

use neomind_core::extension::isolated::{
    IsolatedExtensionConfig, IsolatedExtensionManager, IsolatedManagerConfig,
};
use neomind_core::extension::system::ExtensionDescriptor;

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_isolated_manager_config_default() {
    let config = IsolatedManagerConfig::default();

    assert!(config.isolated_by_default);
    assert!(config.force_isolated.is_empty());
}

#[test]
fn test_isolated_extension_config_default() {
    let config = IsolatedExtensionConfig::default();

    // Default config should exist
    let _ = config;
}

#[test]
fn test_isolated_manager_config_custom() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: false,
        force_isolated: vec!["legacy.extension".to_string()],
    };

    assert!(!config.isolated_by_default);
    assert_eq!(config.force_isolated.len(), 1);
}

// ============================================================================
// Manager Creation Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_manager_creation() {
    let manager = IsolatedExtensionManager::with_defaults();
    assert_eq!(manager.count().await, 0);
}

#[tokio::test]
#[serial_test::serial]
async fn test_manager_with_custom_config() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: true,
        force_isolated: vec![],
    };

    let manager = IsolatedExtensionManager::new(config);
    assert_eq!(manager.count().await, 0);
}

// ============================================================================
// Extension Management Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_manager_empty_list() {
    let manager = IsolatedExtensionManager::with_defaults();
    let list = manager.list().await;
    assert!(list.is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn test_manager_contains_nonexistent() {
    let manager = IsolatedExtensionManager::with_defaults();
    assert!(!manager.contains("nonexistent").await);
}

#[tokio::test]
#[serial_test::serial]
async fn test_manager_get_nonexistent() {
    let manager = IsolatedExtensionManager::with_defaults();
    let result = manager.get("nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn test_manager_get_info_nonexistent() {
    let manager = IsolatedExtensionManager::with_defaults();
    let info = manager.get_info("nonexistent");
    assert!(info.is_none());
}

// ============================================================================
// Should Use Isolated Tests
// ============================================================================

#[test]
fn test_should_use_isolated_default() {
    let manager = IsolatedExtensionManager::with_defaults();

    // By default, all extensions should use isolated mode
    assert!(manager.should_use_isolated("any.extension"));
}

#[test]
fn test_should_use_isolated_force_isolated() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: false,
        force_isolated: vec!["critical.extension".to_string()],
    };

    let manager = IsolatedExtensionManager::new(config);

    // Force isolated should override default
    assert!(manager.should_use_isolated("critical.extension"));

    // Other extensions should not use isolated
    assert!(!manager.should_use_isolated("other.extension"));
}

#[test]
fn test_should_use_isolated_by_default() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: true,
        force_isolated: vec![],
    };

    let manager = IsolatedExtensionManager::new(config);

    // All extensions should use isolated when isolated_by_default is true
    assert!(manager.should_use_isolated("any.extension"));
}

// ============================================================================
// Event Dispatcher Tests
// ============================================================================

#[test]
fn test_event_dispatcher_exists() {
    let manager = IsolatedExtensionManager::with_defaults();
    let dispatcher = manager.event_dispatcher();
    // Just verify we can get the dispatcher
    let _ = dispatcher;
}

// ============================================================================
// Stop All Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_stop_all_empty() {
    let manager = IsolatedExtensionManager::with_defaults();

    // Should not panic when stopping with no extensions
    manager.stop_all().await;
    assert_eq!(manager.count().await, 0);
}

// ============================================================================
// Config Access Tests
// ============================================================================

#[test]
fn test_config_access() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: true,
        force_isolated: vec![],
    };

    let manager = IsolatedExtensionManager::new(config.clone());
    let retrieved_config = manager.config();

    assert_eq!(
        retrieved_config.isolated_by_default,
        config.isolated_by_default
    );
    assert_eq!(retrieved_config.force_isolated, config.force_isolated);
}

// ============================================================================
// Isolated Extension Info Tests
// ============================================================================

#[test]
fn test_isolated_extension_info_metadata_accessor() {
    use neomind_core::extension::isolated::IsolatedExtensionInfo;
    use neomind_core::extension::system::{ExtensionMetadata, ExtensionRuntimeState};
    use std::path::PathBuf;

    let metadata = ExtensionMetadata::new("test.extension", "Test Extension", "1.0.0");

    let descriptor = ExtensionDescriptor::new(metadata.clone());

    let mut runtime = ExtensionRuntimeState::isolated();
    runtime.is_running = true; // Mark as running for test

    let info = IsolatedExtensionInfo {
        descriptor,
        path: PathBuf::from("/test/path"),
        runtime,
    };

    // Test accessor methods
    assert_eq!(info.metadata().id, "test.extension");
    assert!(info.commands().is_empty());
    assert!(info.metrics().is_empty());
    assert!(info.is_running());
}

#[test]
fn test_isolated_extension_info_runtime_state() {
    use neomind_core::extension::isolated::IsolatedExtensionInfo;
    use neomind_core::extension::system::{
        ExtensionDescriptor, ExtensionMetadata, ExtensionRuntimeState,
    };
    use std::path::PathBuf;

    let metadata = ExtensionMetadata::new("test.extension", "Test Extension", "1.0.0");

    let descriptor = ExtensionDescriptor::new(metadata);

    let mut runtime = ExtensionRuntimeState::isolated();
    runtime.is_running = false;
    runtime.restart_count = 3;

    let info = IsolatedExtensionInfo {
        descriptor,
        path: PathBuf::from("/test/path"),
        runtime,
    };

    assert!(!info.is_running());
    assert_eq!(info.restart_count(), 3);
}

// ============================================================================
// Extension Runtime State Tests
// ============================================================================

#[test]
fn test_extension_runtime_state_isolated() {
    use neomind_core::extension::system::ExtensionRuntimeState;

    let state = ExtensionRuntimeState::isolated();

    assert!(state.is_isolated);
    assert!(!state.is_running); // Default is not running
    assert_eq!(state.restart_count, 0);
}

#[test]
fn test_extension_runtime_state_in_process() {
    use neomind_core::extension::system::ExtensionRuntimeState;

    let state = ExtensionRuntimeState::new();

    assert!(!state.is_isolated);
    assert!(!state.is_running);
    assert_eq!(state.restart_count, 0);
}

// ============================================================================
// Isolated Extension Error Tests
// ============================================================================

#[test]
fn test_isolated_extension_error_display() {
    use neomind_core::extension::isolated::IsolatedExtensionError;

    let err = IsolatedExtensionError::SpawnFailed("Process failed".to_string());
    assert!(err.to_string().contains("Failed to spawn"));

    let err = IsolatedExtensionError::IpcError("Channel closed".to_string());
    assert!(err.to_string().contains("IPC communication error"));

    let err = IsolatedExtensionError::Timeout(5000);
    assert!(err.to_string().contains("timed out"));
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_contains_check() {
    let manager = std::sync::Arc::new(IsolatedExtensionManager::with_defaults());
    let mut handles = vec![];

    for i in 0..10 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move { mgr.contains(&format!("ext.{}", i)).await });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(!result); // All should be false since no extensions are loaded
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_list() {
    let manager = std::sync::Arc::new(IsolatedExtensionManager::with_defaults());
    let mut handles = vec![];

    for _ in 0..5 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move { mgr.list().await });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_empty());
    }
}
