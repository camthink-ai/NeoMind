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
        r#"Create or overwrite a WHOLE file. JSON/TOML content is syntax-checked BEFORE writing — invalid content is rejected with the parser position and nothing is written (fix and retry). Writing content identical to what is already on disk is a confirmed no-op ("identical": true). For targeted changes to an EXISTING file prefer `file_edit` (batch edits, context echo); for agent cross-run memory use `memory`. Allowed dirs only (data dir + NEOMIND_ALLOWED_WRITE_DIRS). Max 10MB."#
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

        // Idempotency short-circuit: a rewrite with IDENTICAL content is a
        // no-op. Small models in long tool loops re-emit the same file
        // verbatim without remembering they wrote it (observed live: the
        // same manifest written 3× across rounds); answering "already up to
        // date" gives them the missing signal AND skips the disk churn.
        if let Ok(existing) = std::fs::read_to_string(&resolved) {
            if existing == content {
                tracing::info!(path = %resolved.display(), bytes = content.len(), "File write skipped — identical content already on disk");
                return Ok(ToolOutput::success(serde_json::json!({
                    "path": resolved.display().to_string(),
                    "bytes_written": 0,
                    "identical": true,
                    "message": "Identical content already on disk — no write needed. You already wrote this exact file in a previous step; move on to the NEXT step. / 该文件内容已完全相同，无需重写，请继续下一步。"
                })));
            }
        }

        // Syntax gate (shared with file_edit): invalid JSON/TOML never
        // reaches disk. A typo'd manifest used to surface rounds later as a
        // mysteriously broken artifact; now it is an immediate, positioned
        // error the model can fix in one retry.
        if let Some(err) = super::file_edit::syntax_check(&resolved, content) {
            tracing::info!(path = %resolved.display(), "File write rejected by syntax gate");
            return Ok(ToolOutput::error(serde_json::json!({
                "message": format!("Content is INVALID — nothing written. Fix and retry: {err}"),
                "syntax_error": err,
            }).to_string()));
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

    #[tokio::test]
    async fn invalid_json_never_reaches_disk() {
        let tool = tool();
        let r = tool
            .execute(serde_json::json!({
                "path": "bad.json",
                "content": "{\"a\": 1,}" // trailing comma
            }))
            .await
            .unwrap();
        assert!(!r.success, "syntax gate must reject");
        assert!(
            r.error.unwrap_or_default().contains("INVALID"),
            "error must state nothing was written"
        );
        assert!(
            !tool.validator.resolve_path("bad.json").unwrap().exists(),
            "file must not be created"
        );
        // The VALID version writes fine (regression: gate must not block good content).
        let ok = tool
            .execute(serde_json::json!({"path": "good.json", "content": "{\"a\": 1}"}))
            .await
            .unwrap();
        assert!(ok.success, "{:?}", ok.data);
    }

    #[tokio::test]
    async fn identical_rewrite_is_a_noop_with_signal() {
        let tool = tool();
        let args = serde_json::json!({
            "path": "idem-test.txt",
            "content": "hello widget"
        });
        // First write: real.
        let first = tool.execute(args.clone()).await.expect("first write");
        assert!(first.success);
        assert_eq!(first.data["bytes_written"], serde_json::json!(12));
        // Second write, IDENTICAL content: short-circuits — bytes_written 0,
        // identical flag set, and an explicit move-on hint for the model.
        let second = tool.execute(args).await.expect("second write");
        assert!(second.success, "idempotent rewrite must still succeed");
        assert_eq!(second.data["bytes_written"], serde_json::json!(0));
        assert_eq!(second.data["identical"], serde_json::json!(true));
        assert!(
            second.data["message"]
                .as_str()
                .unwrap_or("")
                .contains("NEXT step"),
            "the no-op result must tell the model to move on"
        );
        // A CHANGED content still writes for real.
        let third = tool
            .execute(serde_json::json!({ "path": "idem-test.txt", "content": "hello widget v2" }))
            .await
            .expect("changed write");
        assert_eq!(third.data["bytes_written"], serde_json::json!(15));
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
