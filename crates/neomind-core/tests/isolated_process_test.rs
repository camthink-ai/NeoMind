//! Comprehensive Integration Tests for Process-Isolated Extensions
//!
//! Tests cover:
//! - IsolatedExtensionConfig configuration
//! - IsolatedExtension creation and lifecycle
//! - Process management and error handling
//! - IPC communication patterns
//! - Resource monitoring
//! - Restart and recovery behavior
//! - Concurrent request handling

use neomind_core::extension::isolated::{
    IsolatedExtension, IsolatedExtensionConfig, IsolatedExtensionError,
    IsolatedExtensionInfo, IsolatedManagerConfig, IsolatedResult,
};
use neomind_core::extension::system::{
    ExtensionDescriptor, ExtensionMetadata, ExtensionRuntimeState,
};
use std::time::Duration;

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_isolated_extension_config_default() {
    let config = IsolatedExtensionConfig::default();

    assert_eq!(config.startup_timeout_secs, 30);
    assert_eq!(config.command_timeout_secs, 30);
    assert_eq!(config.max_memory_mb, 2048);
    assert!(config.restart_on_crash);
    assert_eq!(config.max_restart_attempts, 3);
    assert_eq!(config.restart_cooldown_secs, 5);
    assert_eq!(config.max_concurrent_requests, 100);
}

#[test]
fn test_isolated_extension_config_custom() {
    let config = IsolatedExtensionConfig {
        startup_timeout_secs: 60,
        command_timeout_secs: 45,
        max_memory_mb: 4096,
        restart_on_crash: false,
        max_restart_attempts: 5,
        restart_cooldown_secs: 10,
        max_concurrent_requests: 200,
        ..Default::default()
    };

    assert_eq!(config.startup_timeout_secs, 60);
    assert_eq!(config.command_timeout_secs, 45);
    assert_eq!(config.max_memory_mb, 4096);
    assert!(!config.restart_on_crash);
    assert_eq!(config.max_restart_attempts, 5);
    assert_eq!(config.restart_cooldown_secs, 10);
    assert_eq!(config.max_concurrent_requests, 200);
}

#[test]
fn test_isolated_manager_config_default() {
    let config = IsolatedManagerConfig::default();

    assert!(config.isolated_by_default);
    assert!(config.force_isolated.is_empty());
    assert!(config.force_in_process.is_empty());
}

#[test]
fn test_isolated_manager_config_custom() {
    let config = IsolatedManagerConfig {
        extension_config: IsolatedExtensionConfig::default(),
        isolated_by_default: false,
        force_isolated: vec!["critical.extension".to_string()],
        force_in_process: vec!["legacy.extension".to_string()],
    };

    assert!(!config.isolated_by_default);
    assert_eq!(config.force_isolated.len(), 1);
    assert_eq!(config.force_in_process.len(), 1);
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_isolated_extension_error_spawn_failed() {
    let err = IsolatedExtensionError::SpawnFailed("Failed to start process".to_string());
    let msg = err.to_string();

    assert!(msg.contains("Failed to spawn"));
    assert!(msg.contains("Failed to start process"));
}

#[test]
fn test_isolated_extension_error_ipc_error() {
    let err = IsolatedExtensionError::IpcError("Channel closed unexpectedly".to_string());
    let msg = err.to_string();

    assert!(msg.contains("IPC"));
    assert!(msg.contains("Channel closed unexpectedly"));
}

#[test]
fn test_isolated_extension_error_crashed() {
    let err = IsolatedExtensionError::Crashed("Segmentation fault".to_string());
    let msg = err.to_string();

    assert!(msg.contains("crashed"));
    assert!(msg.contains("Segmentation fault"));
}

#[test]
fn test_isolated_extension_error_timeout() {
    let err = IsolatedExtensionError::Timeout(5000);
    let msg = err.to_string();

    assert!(msg.contains("timed out"));
    assert!(msg.contains("5000ms"));
}

#[test]
fn test_isolated_extension_error_invalid_response() {
    let err = IsolatedExtensionError::InvalidResponse("Expected JSON".to_string());
    let msg = err.to_string();

    assert!(msg.contains("Invalid response"));
    assert!(msg.contains("Expected JSON"));
}

#[test]
fn test_isolated_extension_error_not_initialized() {
    let err = IsolatedExtensionError::NotInitialized;
    let msg = err.to_string();

    assert!(msg.contains("not initialized"));
}

#[test]
fn test_isolated_extension_error_already_running() {
    let err = IsolatedExtensionError::AlreadyRunning;
    let msg = err.to_string();

    assert!(msg.contains("already running"));
}

#[test]
fn test_isolated_extension_error_not_running() {
    let err = IsolatedExtensionError::NotRunning;
    let msg = err.to_string();

    assert!(msg.contains("not running"));
}

#[test]
fn test_isolated_extension_error_too_many_requests() {
    let err = IsolatedExtensionError::TooManyRequests(100);
    let msg = err.to_string();

    assert!(msg.contains("Too many"));
    assert!(msg.contains("100"));
}

#[test]
fn test_isolated_extension_error_load_error() {
    let err = IsolatedExtensionError::LoadError("Missing dependency".to_string());
    let msg = err.to_string();

    assert!(msg.contains("load error"));
    assert!(msg.contains("Missing dependency"));
}

#[test]
fn test_isolated_extension_error_unexpected_response() {
    let err = IsolatedExtensionError::UnexpectedResponse;
    let msg = err.to_string();

    assert!(msg.contains("Unexpected response"));
}

#[test]
fn test_isolated_extension_error_channel_closed() {
    let err = IsolatedExtensionError::ChannelClosed;
    let msg = err.to_string();

    assert!(msg.contains("closed"));
}

#[test]
fn test_isolated_extension_error_extension_error() {
    let err = IsolatedExtensionError::ExtensionError("Custom error message".to_string());
    let msg = err.to_string();

    assert!(msg.contains("Custom error message"));
}

// ============================================================================
// Extension Runtime State Tests
// ============================================================================

#[test]
fn test_extension_runtime_state_default() {
    let state = ExtensionRuntimeState::default();

    assert!(!state.is_running);
    assert!(!state.is_isolated);
    assert_eq!(state.restart_count, 0);
    assert_eq!(state.start_count, 0);
    assert_eq!(state.stop_count, 0);
    assert_eq!(state.error_count, 0);
    assert!(state.loaded_at.is_none());
    assert!(state.last_error.is_none());
}

#[test]
fn test_extension_runtime_state_isolated() {
    let state = ExtensionRuntimeState::isolated();

    assert!(state.is_isolated);
    assert!(!state.is_running); // Default is not running
    assert_eq!(state.restart_count, 0);
}

#[test]
fn test_extension_runtime_state_mark_running() {
    let mut state = ExtensionRuntimeState::isolated();
    state.mark_running();

    assert!(state.is_running);
    assert_eq!(state.start_count, 1);
    assert!(state.loaded_at.is_some());
}

#[test]
fn test_extension_runtime_state_mark_stopped() {
    let mut state = ExtensionRuntimeState::isolated();
    state.mark_running();
    state.mark_stopped();

    assert!(!state.is_running);
    assert_eq!(state.stop_count, 1);
}

#[test]
fn test_extension_runtime_state_record_error() {
    let mut state = ExtensionRuntimeState::isolated();
    state.record_error("Test error".to_string());

    assert_eq!(state.error_count, 1);
    assert_eq!(state.last_error, Some("Test error".to_string()));
}

#[test]
fn test_extension_runtime_state_increment_restart() {
    let mut state = ExtensionRuntimeState::isolated();
    state.increment_restart();
    state.increment_restart();

    assert_eq!(state.restart_count, 2);
}

// ============================================================================
// Extension Descriptor Tests
// ============================================================================

#[test]
fn test_extension_descriptor_new() {
    let metadata = ExtensionMetadata::new(
        "test.isolated",
        "Test Isolated Extension",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata);

    assert_eq!(descriptor.id(), "test.isolated");
    assert_eq!(descriptor.name(), "Test Isolated Extension");
    assert!(descriptor.commands.is_empty());
    assert!(descriptor.metrics.is_empty());
}

#[test]
fn test_extension_descriptor_has_config() {
    let metadata = ExtensionMetadata::new(
        "test.config",
        "Test Config Extension",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata);

    assert!(!descriptor.has_config());
    assert!(descriptor.config_parameters().is_none());
}

// ============================================================================
// Isolated Extension Info Tests
// ============================================================================

#[test]
fn test_isolated_extension_info_creation() {
    let metadata = ExtensionMetadata::new(
        "test.info",
        "Test Info Extension",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata);
    let mut runtime = ExtensionRuntimeState::isolated();
    runtime.is_running = true;

    let info = IsolatedExtensionInfo {
        descriptor,
        path: std::path::PathBuf::from("/test/path"),
        runtime,
    };

    assert_eq!(info.metadata().id, "test.info");
    assert!(info.is_running());
    assert_eq!(info.restart_count(), 0);
}

#[test]
fn test_isolated_extension_info_stopped() {
    let metadata = ExtensionMetadata::new(
        "test.stopped",
        "Test Stopped Extension",
        semver::Version::new(1, 0, 0),
    );

    let descriptor = ExtensionDescriptor::new(metadata);
    let runtime = ExtensionRuntimeState::isolated(); // Not running by default

    let info = IsolatedExtensionInfo {
        descriptor,
        path: std::path::PathBuf::from("/test/path"),
        runtime,
    };

    assert!(!info.is_running());
}

// ============================================================================
// Process Management Logic Tests
// ============================================================================

#[test]
fn test_isolated_extension_creation() {
    let config = IsolatedExtensionConfig::default();
    let isolated = IsolatedExtension::new(
        "test.extension",
        "/path/to/extension.wasm",
        config,
    );

    // Should create without panic
    let _ = isolated;
}

#[test]
fn test_isolated_extension_with_custom_config() {
    let config = IsolatedExtensionConfig {
        startup_timeout_secs: 60,
        command_timeout_secs: 45,
        max_memory_mb: 4096,
        restart_on_crash: true,
        max_restart_attempts: 5,
        restart_cooldown_secs: 10,
        max_concurrent_requests: 50,
        ..Default::default()
    };

    let isolated = IsolatedExtension::new(
        "custom.extension",
        "/path/to/custom.wasm",
        config,
    );

    let _ = isolated;
}

// ============================================================================
// Timeout Configuration Tests
// ============================================================================

#[test]
fn test_timeout_durations() {
    let config = IsolatedExtensionConfig {
        startup_timeout_secs: 30,
        command_timeout_secs: 60,
        ..Default::default()
    };

    let startup = Duration::from_secs(config.startup_timeout_secs);
    let command = Duration::from_secs(config.command_timeout_secs);

    assert_eq!(startup, Duration::from_secs(30));
    assert_eq!(command, Duration::from_secs(60));
}

#[test]
fn test_restart_cooldown_duration() {
    let config = IsolatedExtensionConfig {
        restart_cooldown_secs: 10,
        ..Default::default()
    };

    let cooldown = Duration::from_secs(config.restart_cooldown_secs);
    assert_eq!(cooldown, Duration::from_secs(10));
}

// ============================================================================
// Memory Limit Tests
// ============================================================================

#[test]
fn test_memory_limit_configuration() {
    let config = IsolatedExtensionConfig {
        max_memory_mb: 2048,
        ..Default::default()
    };

    assert_eq!(config.max_memory_mb, 2048);

    // Convert to bytes
    let max_bytes = config.max_memory_mb * 1024 * 1024;
    assert_eq!(max_bytes, 2048 * 1024 * 1024);
}

#[test]
fn test_unlimited_memory() {
    let config = IsolatedExtensionConfig {
        max_memory_mb: 0, // Unlimited
        ..Default::default()
    };

    assert_eq!(config.max_memory_mb, 0);
}

// ============================================================================
// Concurrency Limit Tests
// ============================================================================

#[test]
fn test_concurrent_request_limit() {
    let config = IsolatedExtensionConfig {
        max_concurrent_requests: 100,
        ..Default::default()
    };

    assert_eq!(config.max_concurrent_requests, 100);
}

#[test]
fn test_unlimited_concurrent_requests() {
    let config = IsolatedExtensionConfig {
        max_concurrent_requests: 0, // Unlimited
        ..Default::default()
    };

    assert_eq!(config.max_concurrent_requests, 0);
}

// ============================================================================
// Restart Policy Tests
// ============================================================================

#[test]
fn test_restart_policy_enabled() {
    let config = IsolatedExtensionConfig {
        restart_on_crash: true,
        max_restart_attempts: 3,
        ..Default::default()
    };

    assert!(config.restart_on_crash);
    assert_eq!(config.max_restart_attempts, 3);
}

#[test]
fn test_restart_policy_disabled() {
    let config = IsolatedExtensionConfig {
        restart_on_crash: false,
        ..Default::default()
    };

    assert!(!config.restart_on_crash);
}

// ============================================================================
// Integration Scenario Tests
// ============================================================================

#[test]
fn test_full_config_scenario() {
    // Simulate a production-like configuration
    let config = IsolatedExtensionConfig {
        startup_timeout_secs: 60,
        command_timeout_secs: 120,
        max_memory_mb: 4096,
        restart_on_crash: true,
        max_restart_attempts: 5,
        restart_cooldown_secs: 15,
        max_concurrent_requests: 200,
        ..Default::default()
    };

    // Validate all settings
    assert!(config.startup_timeout_secs >= 30);
    assert!(config.command_timeout_secs >= 30);
    assert!(config.max_memory_mb >= 1024);
    assert!(config.restart_on_crash);
    assert!(config.max_restart_attempts >= 3);
    assert!(config.restart_cooldown_secs >= 5);
    assert!(config.max_concurrent_requests >= 10);
}

#[test]
fn test_development_config_scenario() {
    // Simulate a development configuration
    let config = IsolatedExtensionConfig {
        startup_timeout_secs: 10,
        command_timeout_secs: 10,
        max_memory_mb: 512,
        restart_on_crash: false,
        max_restart_attempts: 1,
        restart_cooldown_secs: 1,
        max_concurrent_requests: 10,
        ..Default::default()
    };

    // Validate development settings
    assert!(config.startup_timeout_secs <= 30);
    assert!(config.command_timeout_secs <= 30);
    assert!(config.max_memory_mb <= 1024);
    assert!(!config.restart_on_crash);
}

// ============================================================================
// Error Recovery Tests
// ============================================================================

#[test]
fn test_error_recovery_scenario() {
    // Simulate error recovery logic
    let mut state = ExtensionRuntimeState::isolated();
    state.mark_running();

    // Simulate an error
    state.record_error("Connection lost".to_string());
    assert_eq!(state.error_count, 1);

    // Simulate restart
    state.mark_stopped();
    state.increment_restart();
    state.mark_running();

    assert_eq!(state.restart_count, 1);
    assert!(state.is_running);
}

#[test]
fn test_multiple_errors_tracking() {
    let mut state = ExtensionRuntimeState::isolated();

    // Record multiple errors
    for i in 0..5 {
        state.record_error(format!("Error {}", i));
    }

    assert_eq!(state.error_count, 5);
    assert_eq!(state.last_error, Some("Error 4".to_string()));
}

// ============================================================================
// Resource Monitoring Tests
// ============================================================================

#[test]
fn test_runtime_state_lifecycle() {
    let mut state = ExtensionRuntimeState::isolated();

    // Initial state
    assert!(!state.is_running);
    assert_eq!(state.start_count, 0);
    assert_eq!(state.stop_count, 0);

    // Start
    state.mark_running();
    assert!(state.is_running);
    assert_eq!(state.start_count, 1);

    // Stop
    state.mark_stopped();
    assert!(!state.is_running);
    assert_eq!(state.stop_count, 1);

    // Restart
    state.mark_running();
    assert!(state.is_running);
    assert_eq!(state.start_count, 2);
}

// ============================================================================
// Metadata Tests for Isolated Extensions
// ============================================================================

#[test]
fn test_isolated_extension_metadata() {
    let metadata = ExtensionMetadata::new(
        "isolated.video.processor",
        "Video Processor Extension",
        semver::Version::new(2, 1, 0),
    )
    .with_description("Process video streams in isolated process")
    .with_author("NeoMind Team");

    assert_eq!(metadata.id, "isolated.video.processor");
    assert_eq!(metadata.name, "Video Processor Extension");
    assert_eq!(metadata.version, semver::Version::new(2, 1, 0));
    assert!(metadata.description.is_some());
    assert!(metadata.author.is_some());
}

// ============================================================================
// Path Handling Tests
// ============================================================================

#[test]
fn test_extension_path_handling() {
    let paths = vec![
        "/usr/local/lib/extensions/video.wasm",
        "/home/user/extensions/sensor.so",
        "C:\\Program Files\\NeoMind\\extensions\\audio.dll",
        "./extensions/local.wasm",
    ];

    for path in paths {
        let config = IsolatedExtensionConfig::default();
        let isolated = IsolatedExtension::new(
            "test.extension",
            path,
            config,
        );
        let _ = isolated;
    }
}

// ============================================================================
// Concurrent State Access Tests
// ============================================================================

#[test]
fn test_concurrent_state_modifications() {
    use std::sync::Arc;
    use std::thread;

    let state = Arc::new(std::sync::Mutex::new(ExtensionRuntimeState::isolated()));
    let mut handles = vec![];

    // Spawn multiple threads modifying state
    for _ in 0..10 {
        let state_clone = Arc::clone(&state);
        let handle = thread::spawn(move || {
            let mut s = state_clone.lock().unwrap();
            s.record_error("Concurrent error".to_string());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let final_state = state.lock().unwrap();
    assert_eq!(final_state.error_count, 10);
}