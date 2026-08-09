//! File write tool for creating or overwriting files within allowed directories.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::path_validator::{PathValidator, MAX_CONTENT_SIZE};
use super::tool::{object_schema, Tool, ToolOutput};

/// File write tool — creates or overwrites files within allowed directories.
pub struct FileWriteTool {
    validator: PathValidator,
}

impl FileWriteTool {
    /// Create a new file write tool with the given data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            validator: PathValidator::new(data_dir),
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        r#"Create or overwrite a file with the given content.

Writes files within the data directory or any configured allowed directories (NEOMIND_ALLOWED_WRITE_DIRS).
Use relative paths (e.g., 'skills/my-skill.md') or absolute paths within allowed directories.
Cannot write binary files (.so, .dll, .exe) or security files (.env, .env.*).
Maximum content size: 1 MB. Parent directories are created automatically by default.

Use this for creating skill files, widget bundles, extension source code, config files, or any data files.

For the agent's own cross-run memory (facts, SOPs, preferences), use the `memory` tool instead of writing files. To modify part of an existing file in place, use `file_edit`."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "File path (relative to data/ or absolute). Example: 'skills/my-skill.md', 'extensions/my-ext/src/lib.rs'"
                },
                "content": {
                    "type": "string",
                    "description": "The file content to write"
                },
                "create_dirs": {
                    "type": "boolean",
                    "description": "Create parent directories if they don't exist (default: true)"
                }
            }),
            vec!["path".to_string(), "content".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("path is required".into()))?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("content is required".into()))?;

        let create_dirs = args["create_dirs"].as_bool().unwrap_or(true);

        // Check content size limit
        if content.len() > MAX_CONTENT_SIZE {
            return Err(ToolError::InvalidArguments(format!(
                "Content too large: {} bytes (max {} bytes / {} MB)",
                content.len(),
                MAX_CONTENT_SIZE,
                MAX_CONTENT_SIZE / 1024 / 1024
            )));
        }

        // Validate and resolve path
        let resolved = self.validator.resolve_path(path_str)?;

        // Create parent directories if needed
        if create_dirs {
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Execution(format!(
                        "Failed to create directories '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        // Write file atomically (temp file + rename)
        let bytes = content.len();
        super::path_validator::atomic_write(&resolved, content).map_err(|e| {
            ToolError::Execution(format!(
                "Failed to write file '{}': {}",
                resolved.display(),
                e
            ))
        })?;

        tracing::info!(
            path = %resolved.display(),
            bytes = bytes,
            "File written"
        );

        Ok(ToolOutput::success(serde_json::json!({
            "path": resolved.display().to_string(),
            "bytes_written": bytes,
            "message": format!("Successfully wrote {} bytes to {}", bytes, resolved.display()),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> FileWriteTool {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep();
        FileWriteTool::new(path)
    }

    #[test]
    fn test_rejects_path_traversal() {
        let tool = tool();
        let result = tool.validator.resolve_path("../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_binary_extensions() {
        let tool = tool();
        for ext in &["so", "dll", "exe", "sys"] {
            let result = tool.validator.resolve_path(&format!("malicious.{}", ext));
            assert!(result.is_err(), "Should reject .{}", ext);
        }
    }

    #[test]
    fn test_rejects_env_files() {
        let tool = tool();
        assert!(tool.validator.resolve_path(".env").is_err());
        assert!(tool.validator.resolve_path(".env.local").is_err());
    }

    #[test]
    fn test_rejects_absolute_outside_data() {
        let tool = tool();
        let result = tool.validator.resolve_path("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_accepts_data_relative() {
        let tool = tool();
        assert!(tool.validator.resolve_path("test.txt").is_ok());
    }

    #[test]
    fn test_accepts_rs_file() {
        let tool = tool();
        assert!(tool
            .validator
            .resolve_path("extensions/my-ext/src/lib.rs")
            .is_ok());
    }

    #[test]
    fn test_accepts_toml_file() {
        let tool = tool();
        assert!(tool.validator.resolve_path("Cargo.toml").is_ok());
    }

    #[test]
    fn test_accepts_conf_file() {
        let tool = tool();
        assert!(tool.validator.resolve_path("config/app.conf").is_ok());
    }

    #[test]
    fn test_tool_name() {
        let tool = tool();
        assert_eq!(tool.name(), "file_write");
    }
}
