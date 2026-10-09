//! File read tool — the dedicated reader for the file tool family.
//!
//! Why a dedicated reader (vs `shell cat`): line-numbered output so the
//! model can construct precise `file_edit` old_strings against files it did
//! not create; windowed reads (offset/limit) so large files don't blow the
//! prompt budget; and a **freshness snapshot** (`state: {mtime_ms, size}`)
//! the model can pass back as `expected` to file_edit/file_write to
//! guarantee it is not editing a file that changed underneath it (the
//! platform process — extension installs, config API — writes the same
//! data dir concurrently). This is the Claude Code Read/Edit contract,
//! adapted to opt-in: omitting `expected` keeps legacy behavior.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::path_validator::{PathValidator, MAX_FILE_SIZE};
use super::tool::{object_schema, Tool, ToolOutput};

/// Default window: enough for precise edits on any human-written file,
/// small enough that a stray whole-file read can't blow the prompt.
const DEFAULT_LIMIT: usize = 400;
/// Hard cap on returned text regardless of the requested window.
const MAX_OUTPUT_CHARS: usize = 96 * 1024;

pub struct FileReadTool {
    validator: PathValidator,
}

impl FileReadTool {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            validator: PathValidator::new(data_dir),
        }
    }
}

/// The freshness snapshot shared by the file tool family: what the file
/// looked like when the tool last saw it. `(mtime_ms, size)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileState {
    pub mtime_ms: i64,
    pub size: u64,
}

impl FileState {
    pub fn of(path: &std::path::Path) -> std::io::Result<Self> {
        let md = std::fs::metadata(path)?;
        let mtime_ms = md
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(Self {
            mtime_ms,
            size: md.len(),
        })
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({ "mtime_ms": self.mtime_ms, "size": self.size })
    }

    /// Parse the model-supplied `expected` object; `None` when absent or
    /// malformed (malformed is the model's problem, not a hard error — the
    /// guard is opt-in).
    pub fn from_arg(v: &Value) -> Option<Self> {
        Some(Self {
            mtime_ms: v.get("mtime_ms")?.as_i64()?,
            size: v.get("size")?.as_u64()?,
        })
    }
}

/// Shared staleness guard for file_edit/file_write: when the model passes
/// `expected`, the file on disk must still match that snapshot or the
/// operation is refused (with the CURRENT state so one re-read fixes it).
/// Returns Ok(()) when the guard passes or wasn't supplied.
pub fn check_expected(
    path: &std::path::Path,
    expected: &Value,
) -> std::result::Result<(), Box<ToolOutput>> {
    let Some(want) = FileState::from_arg(expected) else {
        return Ok(());
    };
    match FileState::of(path) {
        Ok(have) if have == want => Ok(()),
        Ok(have) => Err(Box::new(ToolOutput::error(
            serde_json::json!({
                "message": "STALE FILE — the file changed since you last read/wrote it. Your `expected` snapshot does not match. Re-read with file_read and retry against the fresh content.",
                "expected": want.to_json(),
                "actual": have.to_json(),
            })
            .to_string(),
        ))),
        Err(e) => Err(Box::new(ToolOutput::error(format!(
            "Could not stat '{}' for the freshness check: {}",
            path.display(),
            e
        )))),
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        r#"Read a file with LINE NUMBERS (the reference for constructing file_edit old_strings). Default window: first 400 lines — use offset/limit for large files (result says when more lines exist). Result carries `state: {mtime_ms, size}`: pass it back as `expected` on file_edit/file_write to guarantee the file did not change underneath you (stale → the edit is refused with the fresh state). Binary files are rejected. Prefer this over `shell cat` for files you plan to edit. Allowed dirs only (data dir + NEOMIND_ALLOWED_WRITE_DIRS). Max 10MB."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "File path (relative to data/ or absolute)"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line to start from (default: 1)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max lines to return (default: 400)"
                }
            }),
            vec!["path".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("path is required".into()))?;
        let offset = args["offset"].as_u64().unwrap_or(1).max(1) as usize; // 1-based
        let limit = args["limit"]
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, 2000);

        let resolved = self.validator.resolve_path(path_str)?;

        if !resolved.exists() {
            return Ok(ToolOutput::error(format!(
                "File not found: {}",
                resolved.display()
            )));
        }
        let md = std::fs::metadata(&resolved).map_err(|e| {
            ToolError::Execution(format!("Failed to stat '{}': {}", resolved.display(), e))
        })?;
        if md.len() as usize > MAX_FILE_SIZE {
            return Err(ToolError::InvalidArguments(format!(
                "File too large: {} bytes (max {} MB). Use shell tools.",
                md.len(),
                MAX_FILE_SIZE / 1024 / 1024
            )));
        }
        let content = std::fs::read_to_string(&resolved).map_err(|e| {
            ToolError::Execution(format!("Failed to read '{}': {}", resolved.display(), e))
        })?;

        if content.contains('\0') {
            return Ok(ToolOutput::error(
                "Binary file — file_read is for text. Use vision (for images) or shell tooling.",
            ));
        }

        let total_lines = content.lines().count();
        if total_lines == 0 {
            let state = FileState::of(&resolved).ok();
            return Ok(ToolOutput::success(serde_json::json!({
                "path": resolved.display().to_string(),
                "total_lines": 0,
                "content": "",
                "message": "File is empty.",
                "state": state.map(|s| s.to_json()),
            })));
        }

        let start = offset.min(total_lines); // 1-based inclusive
        let end = (start + limit - 1).min(total_lines);
        let mut text = String::new();
        for (idx, line) in content.lines().enumerate() {
            let n = idx + 1;
            if n < start {
                continue;
            }
            if n > end {
                break;
            }
            text.push_str(&format!("{n}: {line}\n"));
        }
        let truncated_by_chars = text.chars().count() > MAX_OUTPUT_CHARS;
        if truncated_by_chars {
            text = text.chars().take(MAX_OUTPUT_CHARS).collect();
        }

        let state = FileState::of(&resolved).ok();
        let more_after = end < total_lines;
        Ok(ToolOutput::success(serde_json::json!({
            "path": resolved.display().to_string(),
            "total_lines": total_lines,
            "returned": format!("{start}-{end}"),
            "content": text,
            "more_after_line": if more_after { Value::from(end) } else { Value::Null },
            "truncated": truncated_by_chars || more_after,
            "state": state.map(|s| s.to_json()),
            "message": if more_after {
                format!("Lines {start}-{end} of {total_lines}. More exist — continue with offset={} for the next window. Pass `state` as `expected` to file_edit/file_write to guard against stale edits.", end + 1)
            } else {
                format!("Lines {start}-{end} of {total_lines} (complete). Pass `state` as `expected` to file_edit/file_write to guard against stale edits.")
            },
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::file_write::FileWriteTool;

    fn tool() -> FileReadTool {
        let dir = tempfile::tempdir().expect("tempdir");
        FileReadTool::new(dir.keep())
    }

    async fn seed(t: &FileReadTool, name: &str, content: &str) {
        let w = FileWriteTool::new(t.validator.data_dir().to_path_buf());
        w.execute(serde_json::json!({"path": name, "content": content}))
            .await
            .expect("seed");
    }

    #[tokio::test]
    async fn numbered_lines_and_state() {
        let t = tool();
        seed(&t, "r.txt", "one\ntwo\nthree\n").await;
        let r = t
            .execute(serde_json::json!({"path": "r.txt"}))
            .await
            .unwrap();
        assert!(r.success);
        let content = r.data["content"].as_str().unwrap();
        assert!(content.contains("1: one") && content.contains("3: three"));
        assert!(
            r.data["state"]["mtime_ms"].is_i64(),
            "state snapshot present"
        );
        assert_eq!(r.data["total_lines"], serde_json::json!(3));
    }

    #[tokio::test]
    async fn offset_limit_windowing_with_more_hint() {
        let t = tool();
        let body: String = (0..50)
            .map(|i| format!("l{i}\n"))
            .collect::<Vec<_>>()
            .join("");
        seed(&t, "w.txt", &body).await;
        let r = t
            .execute(serde_json::json!({"path": "w.txt", "offset": 10, "limit": 5}))
            .await
            .unwrap();
        let content = r.data["content"].as_str().unwrap();
        assert!(content.starts_with("10: l9"));
        assert!(content.contains("14: l13") && !content.contains("15: l14"));
        assert_eq!(r.data["more_after_line"], serde_json::json!(14));
        assert_eq!(r.data["truncated"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn binary_rejected() {
        let t = tool();
        let p = t.validator.data_dir().join("bin.dat");
        std::fs::write(&p, [0u8, 1, 0, 2]).unwrap();
        let r = t
            .execute(serde_json::json!({"path": "bin.dat"}))
            .await
            .unwrap();
        assert!(!r.success);
    }

    #[tokio::test]
    async fn not_found() {
        let t = tool();
        let r = t
            .execute(serde_json::json!({"path": "nope.txt"}))
            .await
            .unwrap();
        assert!(!r.success);
    }

    #[tokio::test]
    async fn expected_guard_roundtrip() {
        let t = tool();
        seed(&t, "g.txt", "a\n").await;
        let read = t
            .execute(serde_json::json!({"path": "g.txt"}))
            .await
            .unwrap();
        let state = read.data["state"].clone();

        // External change invalidates the snapshot…
        let p = t.validator.data_dir().join("g.txt");
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&p, "a\nchanged externally\n").unwrap();

        let w = FileWriteTool::new(t.validator.data_dir().to_path_buf());
        let stale = w
            .execute(serde_json::json!({
                "path": "g.txt",
                "content": "overwritten\n",
                "expected": state,
            }))
            .await
            .unwrap();
        assert!(!stale.success, "stale expected must refuse the write");
        assert!(
            stale.error.unwrap_or_default().contains("STALE"),
            "error must name the staleness"
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "a\nchanged externally\n",
            "disk untouched on refusal"
        );

        // Without `expected` the legacy behavior holds.
        let blind = w
            .execute(serde_json::json!({"path": "g.txt", "content": "fresh\n"}))
            .await
            .unwrap();
        assert!(blind.success);
    }
}
