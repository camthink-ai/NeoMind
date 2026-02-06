//! Error types for the agent crate.
//!
//! This module re-exports the unified error type from core.

// Re-export the core error type
pub use neomind_core::error::Error as NeoMindError;
pub use neomind_core::error::Result as CoreResult;

/// Result type for agent operations.
pub type Result<T> = CoreResult<T>;

// Helper functions for converting crate-specific errors
/// Convert from MemoryError
pub fn from_memory_err(err: impl std::fmt::Display) -> NeoMindError {
    NeoMindError::Memory(err.to_string())
}

/// Convert from ToolError
pub fn from_tool_err(err: impl std::fmt::Display) -> NeoMindError {
    NeoMindError::Tool(err.to_string())
}

/// Convert from DeviceError
pub fn from_device_err(err: impl std::fmt::Display) -> NeoMindError {
    NeoMindError::Device(err.to_string())
}

/// Convert from serialization error (alias for InvalidInput)
pub fn invalid_input(msg: impl Into<String>) -> NeoMindError {
    NeoMindError::Validation(msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = NeoMindError::NotFound("test_key".to_string());
        assert!(err.to_string().contains("test_key"));
    }

    #[test]
    fn test_error_convenience_constructors() {
        let err = NeoMindError::not_found("test_key");
        assert!(err.to_string().contains("test_key"));

        let validation_err = NeoMindError::validation("invalid input");
        assert!(validation_err.to_string().contains("invalid input"));
    }

    #[test]
    fn test_timeout_error() {
        let err = NeoMindError::timeout("operation timed out");
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn test_invalid_input_alias() {
        let err = invalid_input("test error");
        assert!(err.to_string().contains("test error"));
    }
}
