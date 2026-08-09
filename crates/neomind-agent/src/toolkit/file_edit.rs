//! File edit tool for precise string replacement in files.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::path_validator::{PathValidator, MAX_FILE_SIZE};
use super::tool::{object_schema, Tool, ToolOutput};

/// File edit tool — performs precise string replacement in files within allowed directories.
pub struct FileEditTool {
    validator: PathValidator,
}

impl FileEditTool {
    /// Create a new file edit tool with the given data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            validator: PathValidator::new(data_dir),
        }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        r##"Edit a file by exact text replace (old_string -> new_string). Use replace_all=true if old_string repeats. Not-found returns context to locate it. Allowed dirs only (data dir + NEOMIND_ALLOWED_WRITE_DIRS). Max 10MB.

For whole-file create use `file_write`; for agent cross-run memory use `memory`."##
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "File path (relative to data/ or absolute)"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find in the file (must be non-empty)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace old_string with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences instead of just the first (default: false)"
                }
            }),
            vec![
                "path".to_string(),
                "old_string".to_string(),
                "new_string".to_string(),
            ],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("path is required".into()))?;

        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("old_string is required".into()))?;

        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("new_string is required".into()))?;

        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        // Validate old_string is non-empty
        if old_string.is_empty() {
            return Ok(ToolOutput::error(
                "old_string cannot be empty — provide the exact text to find",
            ));
        }

        if old_string == new_string {
            return Ok(ToolOutput::error(
                "old_string and new_string are identical — no change needed",
            ));
        }

        // Resolve path
        let resolved = self.validator.resolve_path(path_str)?;

        // Read file
        if !resolved.exists() {
            return Ok(ToolOutput::error(format!(
                "File not found: {}",
                resolved.display()
            )));
        }

        // Check file size before reading into memory
        let file_metadata = std::fs::metadata(&resolved).map_err(|e| {
            ToolError::Execution(format!("Failed to stat '{}': {}", resolved.display(), e))
        })?;
        if file_metadata.len() as usize > MAX_FILE_SIZE {
            return Err(ToolError::InvalidArguments(format!(
                "File too large: {} bytes (max {} MB). Use shell tools for large files.",
                file_metadata.len(),
                MAX_FILE_SIZE / 1024 / 1024
            )));
        }

        let content = std::fs::read_to_string(&resolved).map_err(|e| {
            ToolError::Execution(format!("Failed to read '{}': {}", resolved.display(), e))
        })?;

        // Normalize CRLF → LF for matching (LLM typically sends LF)
        let normalized_content = content.replace("\r\n", "\n");
        let normalized_old = old_string.replace("\r\n", "\n");

        // Count matches
        let match_count = normalized_content.matches(&normalized_old).count();

        if match_count == 0 {
            // Help the LLM locate the right text by showing file context
            let line_count = content.lines().count();
            let preview_limit = 20;
            let preview_lines: Vec<String> = content
                .lines()
                .take(preview_limit)
                .enumerate()
                .map(|(i, line)| format!("{}: {}", i + 1, line))
                .collect();

            let mut preview = preview_lines.join("\n");
            if line_count > preview_limit {
                preview.push_str(&format!(
                    "\n... ({} more lines)",
                    line_count - preview_limit
                ));
            }

            return Ok(ToolOutput::error(serde_json::json!({
                "message": format!("old_string not found in {}", resolved.display()),
                "hint": "Make sure old_string matches the file content exactly, including whitespace and indentation.",
                "file_lines": line_count,
                "preview": preview,
            }).to_string()));
        }

        if match_count > 1 && !replace_all {
            return Ok(ToolOutput::error(serde_json::json!({
                "message": format!("old_string found {} times in the file. Use replace_all=true to replace all occurrences, or make old_string more specific.", match_count),
                "match_count": match_count,
            }).to_string()));
        }

        // Perform replacement on original content.
        // If file had CRLF, we matched against LF-normalized content,
        // so use the normalized old_string for replacement against original content.
        // This works because if original has CRLF, normalized_old (LF) won't match in original,
        // so we also need to try the CRLF version.
        let original_old = if content.contains("\r\n") && !normalized_old.contains("\r\n") {
            normalized_old.replace("\n", "\r\n")
        } else {
            normalized_old.clone()
        };

        let new_content = if replace_all {
            content.replace(&original_old, new_string)
        } else {
            content.replacen(&original_old, new_string, 1)
        };

        // Write back atomically
        super::path_validator::atomic_write(&resolved, &new_content).map_err(|e| {
            ToolError::Execution(format!("Failed to write '{}': {}", resolved.display(), e))
        })?;

        let diff = Self::diff_summary(&content, &new_content);
        let replacements = if replace_all { match_count } else { 1 };

        tracing::info!(
            path = %resolved.display(),
            replacements = replacements,
            "File edited"
        );

        Ok(ToolOutput::success(serde_json::json!({
            "path": resolved.display().to_string(),
            "replacements": replacements,
            "diff_summary": diff,
            "message": format!("Replaced {} occurrence(s) in {}", replacements, resolved.display()),
        })))
    }
}

impl FileEditTool {
    /// Generate a brief diff summary.
    fn diff_summary(old: &str, new: &str) -> String {
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let delta = new_lines.len() as isize - old_lines.len() as isize;
        let delta_str = match delta {
            0 => "0".to_string(),
            d if d > 0 => format!("+{}", d),
            d => format!("{}", d),
        };
        format!(
            "{} lines → {} lines ({} line change)",
            old_lines.len(),
            new_lines.len(),
            delta_str
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> FileEditTool {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep();
        FileEditTool::new(path)
    }

    #[test]
    fn test_rejects_path_traversal() {
        let tool = tool();
        assert!(tool.validator.resolve_path("../../etc/passwd").is_err());
    }

    #[test]
    fn test_rejects_binary_extensions() {
        let tool = tool();
        for ext in &["so", "dll", "exe", "sys"] {
            assert!(
                tool.validator
                    .resolve_path(&format!("malicious.{}", ext))
                    .is_err(),
                "Should reject .{}",
                ext
            );
        }
    }

    #[test]
    fn test_accepts_rs_file() {
        let tool = tool();
        assert!(tool.validator.resolve_path("src/main.rs").is_ok());
    }

    #[test]
    fn test_accepts_toml() {
        let tool = tool();
        assert!(tool.validator.resolve_path("Cargo.toml").is_ok());
    }

    #[test]
    fn test_diff_summary_zero_change() {
        let summary = FileEditTool::diff_summary("a\nb\nc", "x\ny\nz");
        assert!(summary.contains("3 lines → 3 lines (0 line change)"));
    }

    #[test]
    fn test_diff_summary_growth() {
        let summary = FileEditTool::diff_summary("a\nb\nc", "a\nb\nc\nd\ne");
        assert!(summary.contains("+2"));
    }

    #[test]
    fn test_diff_summary_shrink() {
        let summary = FileEditTool::diff_summary("a\nb\nc\nd", "a\nb");
        assert!(summary.contains("-2"));
    }

    #[test]
    fn test_tool_name() {
        let tool = tool();
        assert_eq!(tool.name(), "file_edit");
    }

    #[tokio::test]
    async fn test_empty_old_string_rejected() {
        let tool = tool();
        let result = tool
            .execute(serde_json::json!({
                "path": "test.txt",
                "old_string": "",
                "new_string": "hello"
            }))
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_identical_strings_rejected() {
        let tool = tool();
        let result = tool
            .execute(serde_json::json!({
                "path": "test.txt",
                "old_string": "hello",
                "new_string": "hello"
            }))
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_missing_required_fields() {
        let tool = tool();
        let result = tool
            .execute(serde_json::json!({ "path": "test.txt" }))
            .await;
        assert!(result.is_err());
    }
}
