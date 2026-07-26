//! Edge AI Tools Crate
//!
//! This crate provides function calling capabilities for the NeoMind platform.

pub mod error;
pub mod extension_tools;
pub mod file_edit;
pub mod file_write;
pub mod image_edit;
pub mod memory_tool;
pub mod path_validator;
pub mod policy;
pub mod registry;
pub mod shell;
pub mod skill_tool;
pub mod time_utils;
pub mod timeouts;
pub mod tool;
pub mod vision;
pub mod web_fetch;

// Re-exports consumed via shortcut path (toolkit::TypeName)
pub use error::{Result, ToolError};
pub use registry::{ToolRegistry, ToolRegistryBuilder, ToolResult};
pub use tool::{Tool, ToolDefinition, ToolExample, ToolOutput};

// Re-exports from core (backward compatibility)
pub use neomind_core::tools::{
    object_schema, string_property, ToolCategory, ToolRelationships, UsageScenario,
};

pub use shell::ShellConfig;

pub use memory_tool::MemoryTool;

pub use file_edit::FileEditTool;
pub use file_write::FileWriteTool;
pub use vision::{VisionConfig, VisionTool};

pub use image_edit::ImageEditTool;

pub use web_fetch::WebFetchTool;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[tokio::test]
    async fn test_registry_empty() {
        let registry = ToolRegistryBuilder::new().build();
        assert_eq!(registry.len(), 0);
    }
}
