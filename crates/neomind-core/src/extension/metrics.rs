/**
 * Extension Metrics Module
 *
 * Prometheus metrics for tracking extension system performance and health.
 * Provides structured metrics for:
 * - Extension command execution
 * - Extension lifecycle events
 * - IPC communication
 * - Resource usage
 */

use prometheus::{
    IntCounter, Histogram, IntGauge, Registry,
    register_int_counter, register_histogram, register_int_gauge,
};
use once_cell::sync::Lazy;
use std::sync::Arc;
use tracing::{info, error, warn};

// =============================================================================
// Extension Command Metrics
// =============================================================================

/// Total number of extension commands executed
static EXTENSION_COMMANDS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "extension_commands_total",
        "Total number of extension commands executed"
    )
    .expect("Failed to register extension_commands_total")
});

/// Extension command execution duration in seconds
static EXTENSION_COMMAND_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram!(
        "extension_command_duration_seconds",
        "Extension command execution duration in seconds",
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .expect("Failed to register extension_command_duration_seconds")
});

/// Number of active extension command requests
static EXTENSION_ACTIVE_REQUESTS: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "extension_active_requests",
        "Number of active extension command requests"
    )
    .expect("Failed to register extension_active_requests")
});

/// Total number of extension command errors
static EXTENSION_COMMAND_ERRORS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "extension_command_errors_total",
        "Total number of extension command errors"
    )
    .expect("Failed to register extension_command_errors_total")
});

// =============================================================================
// Extension Lifecycle Metrics
// =============================================================================

/// Total number of extension loads
static EXTENSION_LOADS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "extension_loads_total",
        "Total number of extension loads attempted"
    )
    .expect("Failed to register extension_loads_total")
});

/// Extension load duration in seconds
static EXTENSION_LOAD_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram!(
        "extension_load_duration_seconds",
        "Extension load duration in seconds",
        vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]
    )
    .expect("Failed to register extension_load_duration_seconds")
});

/// Number of currently loaded extensions
static EXTENSION_LOADED_COUNT: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "extension_loaded_count",
        "Number of currently loaded extensions"
    )
    .expect("Failed to register extension_loaded_count")
});

/// Total number of extension unloads
static EXTENSION_UNLOADS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "extension_unloads_total",
        "Total number of extension unloads"
    )
    .expect("Failed to register extension_unloads_total")
});

// =============================================================================
// IPC Metrics
// =============================================================================

/// Total number of IPC messages sent
static IPC_MESSAGES_SENT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ipc_messages_sent_total",
        "Total number of IPC messages sent to isolated extensions"
    )
    .expect("Failed to register ipc_messages_sent_total")
});

/// Total number of IPC messages received
static IPC_MESSAGES_RECEIVED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ipc_messages_received_total",
        "Total number of IPC messages received from isolated extensions"
    )
    .expect("Failed to register ipc_messages_received_total")
});

/// IPC message round-trip duration in seconds
static IPC_ROUNDTRIP_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram!(
        "ipc_roundtrip_duration_seconds",
        "IPC message round-trip duration in seconds",
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.025, 0.05, 0.1]
    )
    .expect("Failed to register ipc_roundtrip_duration_seconds")
});

/// Total number of IPC errors
static IPC_ERRORS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "ipc_errors_total",
        "Total number of IPC communication errors"
    )
    .expect("Failed to register ipc_errors_total")
});

// =============================================================================
// Resource Usage Metrics
// =============================================================================

/// Extension process memory usage in bytes
static EXTENSION_MEMORY_USAGE_BYTES: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "extension_memory_usage_bytes",
        "Extension process memory usage in bytes"
    )
    .expect("Failed to register extension_memory_usage_bytes")
});

/// Extension process CPU usage percentage
static EXTENSION_CPU_USAGE_PERCENT: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "extension_cpu_usage_percent",
        "Extension process CPU usage as percentage"
    )
    .expect("Failed to register extension_cpu_usage_percent")
});

/// IPC buffer pool utilization
static IPC_BUFFER_POOL_UTILIZATION: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "ipc_buffer_pool_utilization",
        "Current IPC buffer pool utilization (number of buffers in use)"
    )
    .expect("Failed to register ipc_buffer_pool_utilization")
});

// =============================================================================
// Metrics Collection Functions
// =============================================================================

/// Start tracking an extension command execution
/// Returns a timer that will record the duration when dropped
pub fn track_extension_command(extension_id: &str, command: &str) -> ExtensionCommandTracker {
    EXTENSION_COMMANDS_TOTAL.inc();
    EXTENSION_ACTIVE_REQUESTS.inc();

    ExtensionCommandTracker {
        extension_id: extension_id.to_string(),
        command: command.to_string(),
        start: std::time::Instant::now(),
    }
}

/// Timer for extension command execution
pub struct ExtensionCommandTracker {
    extension_id: String,
    command: String,
    start: std::time::Instant,
}

impl ExtensionCommandTracker {
    /// Complete the command tracking successfully
    pub fn complete(self) {
        let duration = self.start.elapsed().as_secs_f64();
        EXTENSION_COMMAND_DURATION_SECONDS.observe(duration);
        EXTENSION_ACTIVE_REQUESTS.dec();

        info!(
            extension_id = %self.extension_id,
            command = %self.command,
            duration_seconds = %duration,
            status = "success",
            "Command executed"
        );
    }

    /// Complete the command tracking with an error
    pub fn error(self, error: &dyn std::error::Error) {
        let duration = self.start.elapsed().as_secs_f64();
        EXTENSION_COMMAND_DURATION_SECONDS.observe(duration);
        EXTENSION_ACTIVE_REQUESTS.dec();
        EXTENSION_COMMAND_ERRORS_TOTAL.inc();

        error!(
            extension_id = %self.extension_id,
            command = %self.command,
            duration_seconds = %duration,
            error = %error,
            status = "error",
            "Command failed"
        );
    }
}

impl Drop for ExtensionCommandTracker {
    fn drop(&mut self) {
        // If not explicitly completed or errored, record as incomplete
        if EXTENSION_ACTIVE_REQUESTS.get() > 0 {
            EXTENSION_ACTIVE_REQUESTS.dec();
        }
    }
}

/// Track an extension load operation
pub fn track_extension_load(extension_id: &str) -> ExtensionLoadTracker {
    EXTENSION_LOADS_TOTAL.inc();

    ExtensionLoadTracker {
        extension_id: extension_id.to_string(),
        start: std::time::Instant::now(),
    }
}

/// Timer for extension load operation
pub struct ExtensionLoadTracker {
    extension_id: String,
    start: std::time::Instant,
}

impl ExtensionLoadTracker {
    /// Complete the load tracking successfully
    pub fn complete(self) {
        let duration = self.start.elapsed().as_secs_f64();
        EXTENSION_LOAD_DURATION_SECONDS.observe(duration);
        EXTENSION_LOADED_COUNT.inc();

        info!(
            extension_id = %self.extension_id,
            duration_seconds = %duration,
            status = "success",
            "Extension loaded"
        );
    }

    /// Complete the load tracking with an error
    pub fn error(self, error: &dyn std::error::Error) {
        let duration = self.start.elapsed().as_secs_f64();
        EXTENSION_LOAD_DURATION_SECONDS.observe(duration);

        error!(
            extension_id = %self.extension_id,
            duration_seconds = %duration,
            error = %error,
            status = "error",
            "Extension load failed"
        );
    }
}

/// Record an extension unload
pub fn record_extension_unload(extension_id: &str) {
    EXTENSION_UNLOADS_TOTAL.inc();
    EXTENSION_LOADED_COUNT.dec();

    info!(
        extension_id = %extension_id,
        "Extension unloaded"
    );
}

/// Track an IPC message
pub fn track_ipc_message(extension_id: &str, message_type: &str) -> IpcMessageTracker {
    IPC_MESSAGES_SENT_TOTAL.inc();

    IpcMessageTracker {
        extension_id: extension_id.to_string(),
        message_type: message_type.to_string(),
        start: std::time::Instant::now(),
    }
}

/// Timer for IPC message round-trip
pub struct IpcMessageTracker {
    extension_id: String,
    message_type: String,
    start: std::time::Instant,
}

impl IpcMessageTracker {
    /// Complete the IPC tracking successfully
    pub fn complete(self) {
        let duration = self.start.elapsed().as_secs_f64();
        IPC_MESSAGES_RECEIVED_TOTAL.inc();
        IPC_ROUNDTRIP_DURATION_SECONDS.observe(duration);

        if duration > 0.1 {
            warn!(
                extension_id = %self.extension_id,
                message_type = %self.message_type,
                duration_seconds = %duration,
                "Slow IPC message"
            );
        }
    }

    /// Complete the IPC tracking with an error
    pub fn error(self, error: &dyn std::error::Error) {
        IPC_ERRORS_TOTAL.inc();

        error!(
            extension_id = %self.extension_id,
            message_type = %self.message_type,
            error = %error,
            "IPC message failed"
        );
    }
}

/// Update extension resource usage metrics
pub fn update_extension_resource_metrics(
    _extension_id: &str,
    memory_bytes: u64,
    cpu_percent: u64,
) {
    EXTENSION_MEMORY_USAGE_BYTES.set(memory_bytes as i64);
    EXTENSION_CPU_USAGE_PERCENT.set(cpu_percent as i64);
}

/// Update IPC buffer pool utilization
pub fn update_ipc_buffer_pool_utilization(buffers_in_use: usize) {
    IPC_BUFFER_POOL_UTILIZATION.set(buffers_in_use as i64);
}

/// Get the global Prometheus registry
pub fn get_registry() -> &'static Registry {
    // prometheus::default_registry() is not available in the prometheus crate
    // We need to collect our metrics manually or use a custom registry
    // For now, return a placeholder
    static REGISTRY: Lazy<Registry> = Lazy::new(|| {
        let registry = Registry::new();

        // Register all metrics
        registry.register(Box::new(EXTENSION_COMMANDS_TOTAL.clone())).unwrap();
        registry.register(Box::new(EXTENSION_COMMAND_DURATION_SECONDS.clone())).unwrap();
        registry.register(Box::new(EXTENSION_ACTIVE_REQUESTS.clone())).unwrap();
        registry.register(Box::new(EXTENSION_COMMAND_ERRORS_TOTAL.clone())).unwrap();

        registry.register(Box::new(EXTENSION_LOADS_TOTAL.clone())).unwrap();
        registry.register(Box::new(EXTENSION_LOAD_DURATION_SECONDS.clone())).unwrap();
        registry.register(Box::new(EXTENSION_LOADED_COUNT.clone())).unwrap();
        registry.register(Box::new(EXTENSION_UNLOADS_TOTAL.clone())).unwrap();

        registry.register(Box::new(IPC_MESSAGES_SENT_TOTAL.clone())).unwrap();
        registry.register(Box::new(IPC_MESSAGES_RECEIVED_TOTAL.clone())).unwrap();
        registry.register(Box::new(IPC_ROUNDTRIP_DURATION_SECONDS.clone())).unwrap();
        registry.register(Box::new(IPC_ERRORS_TOTAL.clone())).unwrap();

        registry.register(Box::new(EXTENSION_MEMORY_USAGE_BYTES.clone())).unwrap();
        registry.register(Box::new(EXTENSION_CPU_USAGE_PERCENT.clone())).unwrap();
        registry.register(Box::new(IPC_BUFFER_POOL_UTILIZATION.clone())).unwrap();

        registry
    });

    &REGISTRY
}

/// Gather all metrics in Prometheus text format
pub fn gather_metrics() -> String {
    use prometheus::Encoder;

    let registry = get_registry();
    let metric_families = registry.gather();

    let encoder = prometheus::TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    String::from_utf8(buffer).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_command_tracking() {
        let tracker = track_extension_command("test-ext", "test-command");
        tracker.complete();

        assert_eq!(EXTENSION_COMMANDS_TOTAL.get(), 1);
        assert_eq!(EXTENSION_ACTIVE_REQUESTS.get(), 0);
    }

    #[test]
    fn test_extension_load_tracking() {
        let tracker = track_extension_load("test-ext");
        tracker.complete();

        assert_eq!(EXTENSION_LOADS_TOTAL.get(), 1);
        assert_eq!(EXTENSION_LOADED_COUNT.get(), 1);
    }

    #[test]
    fn test_metrics_gathering() {
        let metrics = gather_metrics();
        assert!(metrics.contains("extension_commands_total"));
        assert!(metrics.contains("extension_loads_total"));
    }
}
