//! Extension type definitions (V2 - Re-exports from system.rs)
//!
//! This module re-exports the V2 extension system types.

// ============================================================================
// Re-exports from V2 system.rs
// ============================================================================

pub use super::system::{
    // C-compatible metadata (V2)
    CExtensionMetadata,
    // Dynamic extension type (V2)
    DynExtension,
    // Core Extension trait (V2)
    Extension,
    ExtensionCommand,
    // Error types (V2)
    ExtensionError,
    // Metadata types (V2)
    ExtensionMetadata,
    ExtensionMetricValue,
    // Extension state (V2)
    ExtensionState,
    // Extension stats (V2)
    ExtensionStats,
    MetricDataType,
    // Metrics and commands (V2)
    MetricDescriptor,
    ParamMetricValue,
    // Result type (V2)
    Result,
    // Tool descriptor (V2)
    ToolDescriptor,
    // ABI version (V3)
    ABI_VERSION,
};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_version() {
        // V2 SDK uses ABI version 3
        assert_eq!(ABI_VERSION, 3);
    }
}
