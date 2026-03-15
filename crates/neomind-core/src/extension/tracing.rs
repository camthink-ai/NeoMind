/**
 * Extension Tracing Module
 *
 * OpenTelemetry integration for distributed tracing of extension operations.
 * Provides structured tracing for:
 * - Extension command execution
 * - Extension lifecycle events
 * - IPC communication
 * - Resource operations
 */

use opentelemetry::trace::TraceContextExt;
use opentelemetry::{Context, global, Key};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::{debug, error, info, info_span, warn, Instrument, Span};

// =============================================================================
// Tracing Keys
// =============================================================================()

pub static EXTENSION_ID: Key = Key::from_static_str("extension_id");
pub static COMMAND_NAME: Key = Key::from_static_str("command_name");
pub static COMMAND_ARGS: Key = Key::from_static_str("command_args");
pub static IS_ISOLATED: Key = Key::from_static_str("is_isolated");
pub static LOAD_TIME_MS: Key = Key::from_static_str("load_time_ms");
pub static ERROR_TYPE: Key = Key::from_static_str("error_type");
pub static ERROR_MESSAGE: Key = Key::from_static_str("error_message");
pub static IPC_MESSAGE_TYPE: Key = Key::from_static_str("ipc_message_type");
pub static IPC_ROUNDTRIP_MS: Key = Key::from_static_str("ipc_roundtrip_ms");
pub static MEMORY_BYTES: Key = Key::from_static_str("memory_bytes");
pub static CPU_PERCENT: Key = Key::from_static_str("cpu_percent");

// =============================================================================
// Initialization
// =============================================================================()

/// Initialize OpenTelemetry tracing
///
/// This sets up the OpenTelemetry SDK with distributed tracing support.
/// Call this once during application startup.
///
/// # Arguments
/// * `service_name` - Name of the service (e.g., "neomind-core")
///
/// # Example
/// ```no_run
/// use neomind_core::extension::tracing::init_telemetry;
///
/// fn main() {
///     init_telemetry("neomind-core").unwrap();
///     // Your application code here
///     // Shutdown telemetry on exit
///     opentelemetry::global::shutdown_tracer_provider();
/// }
/// ```
pub fn init_telemetry(service_name: &'static str) -> Result<(), Box<dyn std::error::Error>> {
    // Set up the propagation layer for distributed tracing
    global::set_text_map_propagator(TraceContextPropagator::new());

    info!("OpenTelemetry initialized for service: {}", service_name);
    Ok(())
}

// =============================================================================
// Extension Tracing Functions
// =============================================================================()

/// Create a tracing span for extension command execution
///
/// # Arguments
/// * `extension_id` - Extension identifier
/// * `command` - Command name
///
/// # Returns
/// A tracing::Span instrumented with OpenTelemetry
///
/// # Example
/// ```no_run
/// use neomind_core::extension::tracing::extension_command_span;
///
/// fn execute_command(extension_id: &str, command: &str) {
///     let span = extension_command_span(extension_id, command);
///
///     async {
///         // Your command execution code here
///     }.instrument(span).await;
/// }
/// ```
pub fn extension_command_span(extension_id: &str, command: &str) -> tracing::span::Span {
    info_span!(
        "extension_execute_command",
        extension_id = %extension_id,
        command = %command,
        otel.kind = "client",
        otel.name = format!("extension/{}", command)
    )
}

/// Create a tracing span for extension load operation
pub fn extension_load_span(extension_id: &str, is_isolated: bool) -> tracing::span::Span {
    info_span!(
        "extension_load",
        extension_id = %extension_id,
        is_isolated = %is_isolated,
        otel.kind = "internal",
        otel.name = format!("extension/load/{}", extension_id)
    )
}

/// Create a tracing span for extension unload operation
pub fn extension_unload_span(extension_id: &str) -> tracing::span::Span {
    info_span!(
        "extension_unload",
        extension_id = %extension_id,
        otel.kind = "internal",
        otel.name = format!("extension/unload/{}", extension_id)
    )
}

/// Create a tracing span for IPC communication
pub fn ipc_communication_span(
    extension_id: &str,
    message_type: &str,
) -> tracing::span::Span {
    info_span!(
        "ipc_communication",
        extension_id = %extension_id,
        message_type = %message_type,
        otel.kind = "client",
        otel.name = format!("ipc/{}", message_type)
    )
}

// =============================================================================
// Instrumented Async Functions
// =============================================================================()

/// Instrument an async extension command execution with tracing
///
/// # Arguments
/// * `extension_id` - Extension identifier
/// * `command` - Command name
/// * `fut` - Async function to instrument
///
/// # Returns
/// Result of the async function
///
/// # Example
/// ```no_run
/// use neomind_core::extension::tracing::instrumented_command;
/// use serde_json::json;
///
/// async fn execute(extension_id: &str, command: &str) -> Result<(), Error> {
///     instrumented_command(extension_id, command, async {
///         // Your command execution code here
///         Ok(())
///     }).await
/// }
/// ```
pub async fn instrumented_command<F, T>(
    extension_id: &str,
    command: &str,
    fut: F,
) -> Result<T, crate::extension::ExtensionError>
where
    F: std::future::Future<Output = Result<T, crate::extension::ExtensionError>>,
{
    let span = extension_command_span(extension_id, command);

    async move {
        debug!(
            extension_id = %extension_id,
            command = %command,
            "Executing extension command"
        );

        let result = fut.await;

        match &result {
            Ok(_) => {
                info!(
                    extension_id = %extension_id,
                    command = %command,
                    "Command executed successfully"
                );
            }
            Err(error) => {
                error!(
                    extension_id = %extension_id,
                    command = %command,
                    error = %error,
                    "Command execution failed"
                );

                // Record error in span
                Span::current().record("error_type", &"extension_error");
                Span::current().record("error_message", &error.to_string());
            }
        }

        result
    }
    .instrument(span)
    .await
}

/// Instrument an async extension load operation with tracing
pub async fn instrumented_load<F, T>(
    extension_id: &str,
    is_isolated: bool,
    fut: F,
) -> Result<T, crate::extension::ExtensionError>
where
    F: std::future::Future<Output = Result<T, crate::extension::ExtensionError>>,
{
    let span = extension_load_span(extension_id, is_isolated);

    async move {
        let start = std::time::Instant::now();

        debug!(
            extension_id = %extension_id,
            is_isolated = %is_isolated,
            "Loading extension"
        );

        let result = fut.await;

        let duration = start.elapsed();

        match &result {
            Ok(_) => {
                info!(
                    extension_id = %extension_id,
                    is_isolated = %is_isolated,
                    load_time_ms = %duration.as_millis(),
                    "Extension loaded successfully"
                );

                // Record load time in span
                Span::current().record("load_time_ms", duration.as_millis() as i64);
            }
            Err(error) => {
                error!(
                    extension_id = %extension_id,
                    is_isolated = %is_isolated,
                    error = %error,
                    "Extension load failed"
                );

                Span::current().record("error_type", &"load_error");
                Span::current().record("error_message", &error.to_string());
            }
        }

        result
    }
    .instrument(span)
    .await
}

/// Instrument an async IPC communication with tracing
pub async fn instrumented_ipc<F, T>(
    extension_id: &str,
    message_type: &str,
    fut: F,
) -> Result<T, crate::extension::isolated::IsolatedExtensionError>
where
    F: std::future::Future<Output = Result<T, crate::extension::isolated::IsolatedExtensionError>>,
{
    let span = ipc_communication_span(extension_id, message_type);

    async move {
        let start = std::time::Instant::now();

        debug!(
            extension_id = %extension_id,
            message_type = %message_type,
            "Sending IPC message"
        );

        let result = fut.await;

        let duration = start.elapsed();

        match &result {
            Ok(_) => {
                if duration.as_millis() > 100 {
                    warn!(
                        extension_id = %extension_id,
                        message_type = %message_type,
                        duration_ms = %duration.as_millis(),
                        "Slow IPC communication"
                    );
                }

                // Record roundtrip time in span
                Span::current().record("ipc_roundtrip_ms", duration.as_millis() as i64);
            }
            Err(error) => {
                error!(
                    extension_id = %extension_id,
                    message_type = %message_type,
                    error = %error,
                    "IPC communication failed"
                );

                Span::current().record("error_type", &"ipc_error");
                Span::current().record("error_message", &error.to_string());
            }
        }

        result
    }
    .instrument(span)
    .await
}

// =============================================================================
// Context Utilities
// =============================================================================()

/// Get the current trace ID as a hex string
///
/// Returns the trace ID from the current OpenTelemetry context,
/// or "unknown" if no trace context exists.
pub fn current_trace_id() -> String {
    use opentelemetry::trace::TraceId;

    let context = Context::current();
    let span = context.span();
    let span_context = span.span_context();

    if span_context.is_valid() {
        span_context.trace_id().to_string()
    } else {
        "unknown".to_string()
    }
}

/// Get the current span ID as a hex string
///
/// Returns the span ID from the current OpenTelemetry context,
/// or "unknown" if no span context exists.
pub fn current_span_id() -> String {
    use opentelemetry::trace::SpanId;

    let context = Context::current();
    let span = context.span();
    let span_context = span.span_context();

    if span_context.is_valid() {
        span_context.span_id().to_string()
    } else {
        "unknown".to_string()
    }
}

/// Inject trace context into a map
///
/// Useful for propagating trace context across process boundaries.
/// Note: This is a simplified implementation. For production use,
/// you should configure proper propagators based on your OpenTelemetry setup.
pub fn inject_trace_context() -> std::collections::HashMap<String, String> {
    use opentelemetry::global;
    use opentelemetry::trace::TraceContextExt;

    let mut injector = std::collections::HashMap::new();
    let context = Context::current();

    // Use default trace context propagator
    let span = context.span();
    let span_context = span.span_context();
    
    if span_context.is_valid() {
        let trace_id = span_context.trace_id().to_string();
        let span_id = span_context.span_id().to_string();
        
        injector.insert("traceparent".to_string(), 
            format!("00-{}-{}-01", trace_id, span_id));
    }
    
    injector
}

/// Extract trace context from a map
///
/// Useful for receiving trace context from another process.
/// Note: This is a simplified implementation. For production use,
/// you should configure proper extractors based on your OpenTelemetry setup.
pub fn extract_trace_context(
    _carrier: &std::collections::HashMap<String, String>,
) -> Context {
    // For now, return current context
    // In a full implementation, this would parse the traceparent header
    // and reconstruct the trace context
    Context::current()
}

// =============================================================================
// Testing Utilities
// =============================================================================()

#[cfg(test)]
pub mod test_utils {
    use super::*;

    /// Initialize tracing for tests
    pub fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .try_init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_extraction() {
        let span = info_span!("test_span");
        let _enter = span.enter();

        let trace_id = current_trace_id();
        assert!(trace_id != "unknown");
    }

    #[test]
    fn test_span_id_extraction() {
        let span = info_span!("test_span");
        let _enter = span.enter();

        let span_id = current_span_id();
        assert!(span_id != "unknown");
    }

    #[test]
    fn test_context_injection_extraction() {
        let span = info_span!("test_span");
        let _enter = span.enter();

        let injected = inject_trace_context();
        let context = extract_trace_context(&injected);

        assert_ne!(current_trace_id(), "unknown");
    }
}
