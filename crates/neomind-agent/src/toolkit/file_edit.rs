//! File edit tool for precise string replacement in files.
//!
//! v2 capabilities (aligned with the industry patterns proven out by
//! Claude Code / Codex / Aider):
//! - **Batch edits**: `edits: [{old_string, new_string, replace_all?}]` —
//!   every pair is validated FIRST; one bad pair rejects the whole call
//!   (nothing applied). The legacy single-pair form still works.
//! - **Append mode**: a single pair with an EMPTY `old_string` appends
//!   `new_string` to the end of the file.
//! - **Whitespace-tolerant fallback**: when an exact match fails, an
//!   indentation-insensitive line match is attempted; a hit is applied and
//!   DECLARED in the result (`whitespace_tolerant: true`) so the model
//!   knows the original indentation was replaced.
//! - **Syntax gate**: `.json` / `.toml` results are parsed BEFORE writing —
//!   invalid content never reaches disk and the error carries the parser
//!   position. (`file_write` has the same gate.)
//! - **Context echo**: every applied change returns the edited region ±5
//!   lines with line numbers, so the model can verify without a re-read
//!   round (the single biggest round-saver on small models).

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

/// One edit operation, parsed from either the batch `edits` array or the
/// legacy single-pair form. `old_string` empty ⇒ append mode.
struct EditOp {
    old_string: String,
    new_string: String,
    replace_all: bool,
}

/// Outcome of applying one op (for reporting).
struct AppliedOp {
    replacements: usize,
    whitespace_tolerant: bool,
    /// 1-based line of the FIRST replacement in the FINAL content (echo).
    first_line: usize,
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
        r##"Edit a file by exact text replace. Prefer the BATCH form `edits:[{old_string,new_string,replace_all?}]` to apply several changes in ONE call (all pairs are validated first; any mismatch rejects the whole call, nothing is half-applied). Single-pair legacy form (path+old_string+new_string) still works; an EMPTY old_string appends new_string to the end of the file. Indentation-mismatched old_string still matches via whitespace-tolerant fallback (declared in the result). JSON/TOML files are syntax-checked before writing — invalid content is rejected with the parser position and nothing is written. Result echoes each edited region ±5 lines with line numbers, so no re-read is needed. Allowed dirs only (data dir + NEOMIND_ALLOWED_WRITE_DIRS). Max 10MB.

For whole-file create use `file_write`; for agent cross-run memory use `memory`."##
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "File path (relative to data/ or absolute)"
                },
                "edits": {
                    "type": "array",
                    "description": "Batch form: list of {old_string, new_string, replace_all?} applied top-to-bottom in one atomic call. Preferred over the single-pair form.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string", "description": "Exact text to find (empty = append, single-pair only)" },
                            "new_string": { "type": "string", "description": "Replacement text" },
                            "replace_all": { "type": "boolean", "description": "Replace every occurrence (default: false)" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                },
                "old_string": {
                    "type": "string",
                    "description": "Legacy single-pair form: the exact text to find. EMPTY string = append new_string to the end of the file."
                },
                "new_string": {
                    "type": "string",
                    "description": "Legacy single-pair form: the text to replace old_string with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Legacy single-pair form: replace all occurrences instead of just the first (default: false)"
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

        // ── Parse: batch `edits` array wins; else legacy single pair.
        let ops: Vec<EditOp> = if let Some(list) = args["edits"].as_array() {
            if list.is_empty() {
                return Ok(ToolOutput::error(
                    "edits must contain at least one {old_string, new_string} pair",
                ));
            }
            let mut parsed = Vec::with_capacity(list.len());
            for (i, e) in list.iter().enumerate() {
                let old_string = e["old_string"].as_str().unwrap_or("").to_string();
                let new_string = e["new_string"]
                    .as_str()
                    .ok_or_else(|| {
                        ToolError::InvalidArguments(format!("edits[{i}].new_string is required"))
                    })?
                    .to_string();
                let replace_all = e["replace_all"].as_bool().unwrap_or(false);
                if old_string.is_empty() && list.len() > 1 {
                    return Ok(ToolOutput::error(format!(
                        "edits[{i}]: empty old_string (append mode) is only valid as the single edit, not inside a batch"
                    )));
                }
                if old_string == new_string {
                    return Ok(ToolOutput::error(format!(
                        "edits[{i}]: old_string and new_string are identical — remove the no-op pair"
                    )));
                }
                parsed.push(EditOp {
                    old_string,
                    new_string,
                    replace_all,
                });
            }
            parsed
        } else if let (Some(old_string), Some(new_string)) =
            (args["old_string"].as_str(), args["new_string"].as_str())
        {
            if old_string == new_string && !old_string.is_empty() {
                return Ok(ToolOutput::error(
                    "old_string and new_string are identical — no change needed",
                ));
            }
            vec![EditOp {
                old_string: old_string.to_string(),
                new_string: new_string.to_string(),
                replace_all: args["replace_all"].as_bool().unwrap_or(false),
            }]
        } else {
            return Ok(ToolOutput::error(
                "Provide either edits:[{old_string,new_string}] (batch) or old_string+new_string (single)",
            ));
        };

        // ── Resolve + read.
        let resolved = self.validator.resolve_path(path_str)?;

        if !resolved.exists() {
            return Ok(ToolOutput::error(format!(
                "File not found: {}",
                resolved.display()
            )));
        }
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
        let original = std::fs::read_to_string(&resolved).map_err(|e| {
            ToolError::Execution(format!("Failed to read '{}': {}", resolved.display(), e))
        })?;

        // CRLF-normalized working copy (models emit LF); original kept for
        // the CRLF back-conversion at write time.
        let crlf = original.contains("\r\n");
        let mut content = original.replace("\r\n", "\n");

        // ── Validate ALL ops against the content as it will look once the
        // earlier ops have applied (sequential simulation). One failure
        // rejects the whole call — nothing is half-applied.
        let mut applied: Vec<AppliedOp> = Vec::with_capacity(ops.len());
        let mut validation_errors: Vec<String> = Vec::new();

        for (i, op) in ops.iter().enumerate() {
            let normalized_old = if crlf {
                op.old_string.replace("\r\n", "\n")
            } else {
                op.old_string.clone()
            };
            let new_string = if crlf {
                op.new_string.replace("\r\n", "\n")
            } else {
                op.new_string.clone()
            };

            // Append mode (single op only, enforced at parse).
            if normalized_old.is_empty() {
                let first_line = content.lines().count() + 1;
                if !(content.is_empty() || content.ends_with('\n')) {
                    content.push('\n');
                }
                content.push_str(&new_string);
                applied.push(AppliedOp {
                    replacements: 1,
                    whitespace_tolerant: false,
                    first_line,
                });
                continue;
            }

            let match_count = content.matches(&normalized_old).count();
            if match_count == 0 {
                // Whitespace-tolerant fallback: match line sequences
                // ignoring per-line leading/trailing whitespace.
                match ws_tolerant_replace(
                    &mut content,
                    &normalized_old,
                    &new_string,
                    op.replace_all,
                ) {
                    WsOutcome::Applied { count } => {
                        let first_line = line_of(&content, &new_string);
                        applied.push(AppliedOp {
                            replacements: count,
                            whitespace_tolerant: true,
                            first_line,
                        });
                        continue;
                    }
                    WsOutcome::Ambiguous(n) => {
                        validation_errors.push(format!(
                            "edits[{i}]: old_string not found exactly, and the whitespace-tolerant match is ambiguous ({n} candidates). Include surrounding lines or the exact indentation."
                        ));
                    }
                    WsOutcome::NotFound => {
                        let (hint, preview) = not_found_context(&content);
                        validation_errors.push(format!(
                            "edits[{i}]: old_string not found (exact or whitespace-tolerant). {hint} File has {} lines. First lines:\n{preview}",
                            content.lines().count(),
                        ));
                    }
                }
                break;
            }
            if match_count > 1 && !op.replace_all {
                validation_errors.push(format!(
                    "edits[{i}]: old_string found {match_count} times. Use replace_all=true or include more surrounding context to make it unique."
                ));
                break;
            }

            if op.replace_all {
                content = content.replace(&normalized_old, &new_string);
            } else {
                content = content.replacen(&normalized_old, &new_string, 1);
            }
            // AFTER the replacement — the echo centers on the new text's
            // position in the RESULT, not the old text's position.
            let first_line = line_of(&content, &new_string);
            applied.push(AppliedOp {
                replacements: if op.replace_all { match_count } else { 1 },
                whitespace_tolerant: false,
                first_line,
            });
        }

        if !validation_errors.is_empty() {
            return Ok(ToolOutput::error(
                serde_json::json!({
                    "message": "Validation failed — NO changes were applied (validate-all-then-apply)",
                    "errors": validation_errors,
                })
                .to_string(),
            ));
        }

        // ── Syntax gate: never write invalid JSON/TOML.
        if let Some(err) = syntax_check(&resolved, &content) {
            return Ok(ToolOutput::error(
                serde_json::json!({
                    "message": format!("Result would be INVALID — nothing written. Fix the content and retry: {err}"),
                    "syntax_error": err,
                })
                .to_string(),
            ));
        }

        // ── Write (CRLF files keep their line endings).
        let out = if crlf {
            content.replace('\n', "\r\n")
        } else {
            content
        };
        super::path_validator::atomic_write(&resolved, &out).map_err(|e| {
            ToolError::Execution(format!("Failed to write '{}': {}", resolved.display(), e))
        })?;

        let replacements: usize = applied.iter().map(|a| a.replacements).sum();
        tracing::info!(
            path = %resolved.display(),
            replacements,
            ops = applied.len(),
            "File edited"
        );

        // ── Context echo: each change region ±5 lines with line numbers
        // (≤3 echoed; more echoes the first and last).
        let echo_regions: Vec<String> = {
            let lines: Vec<&str> = out.lines().collect();
            let idxs: Vec<usize> = applied
                .iter()
                .map(|a| a.first_line.saturating_sub(1))
                .collect();
            let show: Vec<usize> = if idxs.len() <= 3 {
                idxs
            } else {
                vec![*idxs.first().unwrap(), *idxs.last().unwrap()]
            };
            show.into_iter()
                .map(|center| echo_snippet(&lines, center))
                .collect()
        };

        Ok(ToolOutput::success(serde_json::json!({
            "path": resolved.display().to_string(),
            "edits_applied": applied.len(),
            "replacements": replacements,
            "whitespace_tolerant": applied.iter().any(|a| a.whitespace_tolerant),
            "context": echo_regions,
            "message": format!(
                "Applied {} edit(s), {} replacement(s) in {}. The `context` field shows each edited region (±5 lines, numbered) — verify there, no re-read needed.",
                applied.len(),
                replacements,
                resolved.display()
            ),
        })))
    }
}

// ─────────────────────────── helpers ───────────────────────────

enum WsOutcome {
    Applied { count: usize },
    Ambiguous(usize),
    NotFound,
}

/// Indentation-insensitive replace: slide a window of `old`'s LINES over the
/// content's lines; a window matches when every pair is equal after
/// trim(). Matched original lines are replaced by `new`'s lines (the new
/// text's own indentation becomes the file's).
fn ws_tolerant_replace(content: &mut String, old: &str, new: &str, replace_all: bool) -> WsOutcome {
    let old_lines: Vec<&str> = old.lines().map(str::trim).collect();
    if old_lines.is_empty() || old_lines.iter().all(|l| l.is_empty()) {
        return WsOutcome::NotFound;
    }
    let lines: Vec<&str> = content.lines().collect();
    let mut hits: Vec<usize> = Vec::new();
    if old_lines.len() <= lines.len() {
        for start in 0..=(lines.len() - old_lines.len()) {
            if (0..old_lines.len()).all(|k| lines[start + k].trim() == old_lines[k]) {
                if !replace_all && hits.len() == 1 {
                    return WsOutcome::Ambiguous(2);
                }
                hits.push(start);
            }
        }
    }
    match hits.first() {
        None => WsOutcome::NotFound,
        Some(_) => {
            let new_block = new.to_string();
            let mut out = String::with_capacity(content.len());
            let mut cursor = 0usize;
            let mut replaced = 0usize;
            for h in &hits {
                for line in &lines[cursor..*h] {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str(&new_block);
                out.push('\n');
                cursor = h + old_lines.len();
                replaced += 1;
            }
            for line in &lines[cursor.min(lines.len())..] {
                out.push_str(line);
                out.push('\n');
            }
            if !content.ends_with('\n') && out.ends_with('\n') {
                out.pop();
            }
            *content = out;
            WsOutcome::Applied { count: replaced }
        }
    }
}

/// 1-based line number of the first occurrence of `needle` in `haystack`.
fn line_of(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .map(|byte| haystack[..byte].matches('\n').count() + 1)
        .unwrap_or(1)
        .max(1)
}

/// The not-found helper: first-20-lines preview with numbers (kept from v1).
fn not_found_context(content: &str) -> (String, String) {
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
    (
        "Make sure old_string matches the file exactly (whitespace included).".to_string(),
        preview,
    )
}

/// ±5 numbered lines around `center` (0-based index).
fn echo_snippet(lines: &[&str], center: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let start = center.saturating_sub(5);
    let end = (center + 5).min(lines.len() - 1);
    lines[start..=end]
        .iter()
        .enumerate()
        .map(|(k, line)| format!("{}: {}", start + k + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse-by-extension syntax gate shared with `file_write`. `None` = fine
/// or not a checked format (only JSON/TOML are checked — serde_yaml is not
/// a workspace dep; those formats pass through untouched).
pub(crate) fn syntax_check(path: &std::path::Path, content: &str) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "json" => match serde_json::from_str::<serde_json::Value>(content) {
            Ok(_) => None,
            Err(e) => Some(format!(
                "invalid JSON at line {}, column {}: {}",
                e.line(),
                e.column(),
                e
            )),
        },
        "toml" => match content.parse::<toml::Value>() {
            Ok(_) => None,
            Err(e) => Some(format!("invalid TOML: {e}")),
        },
        _ => None,
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

    async fn seed(tool: &FileEditTool, name: &str, content: &str) {
        use super::super::file_write::FileWriteTool;
        let writer = FileWriteTool::new(tool.validator.data_dir().to_path_buf());
        writer
            .execute(serde_json::json!({"path": name, "content": content}))
            .await
            .expect("seed write");
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

    #[tokio::test]
    async fn identical_pair_rejected() {
        let t = tool();
        seed(&t, "b.txt", "x\n").await;
        let r = t
            .execute(serde_json::json!({"path": "b.txt", "old_string": "x", "new_string": "x"}))
            .await
            .unwrap();
        assert!(!r.success);
    }

    #[tokio::test]
    async fn no_pairs_is_an_error() {
        let t = tool();
        seed(&t, "c.txt", "x\n").await;
        let r = t
            .execute(serde_json::json!({"path": "c.txt"}))
            .await
            .unwrap();
        assert!(!r.success, "no pairs at all must be an error");
    }

    #[tokio::test]
    async fn append_mode_single_pair() {
        let t = tool();
        seed(&t, "append.txt", "first\n").await;
        let r = t
            .execute(
                serde_json::json!({"path": "append.txt", "old_string": "", "new_string": "second"}),
            )
            .await
            .unwrap();
        assert!(r.success, "{:?}", r.data);
        let on_disk =
            std::fs::read_to_string(t.validator.resolve_path("append.txt").expect("resolve"))
                .unwrap();
        assert_eq!(on_disk, "first\nsecond");
        assert_eq!(r.data["replacements"], serde_json::json!(1));
    }

    #[tokio::test]
    async fn batch_edits_apply_atomically() {
        let t = tool();
        seed(&t, "batch.txt", "alpha\nbeta\ngamma\n").await;
        let r = t
            .execute(serde_json::json!({
                "path": "batch.txt",
                "edits": [
                    {"old_string": "alpha", "new_string": "ALPHA"},
                    {"old_string": "gamma", "new_string": "GAMMA"}
                ]
            }))
            .await
            .unwrap();
        assert!(r.success, "{:?}", r.data);
        assert_eq!(r.data["edits_applied"], serde_json::json!(2));
        let path = t.validator.resolve_path("batch.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "ALPHA\nbeta\nGAMMA\n"
        );
        let ctx = r.data["context"].as_array().unwrap();
        assert!(!ctx.is_empty() && ctx[0].as_str().unwrap().contains("1: "));
    }

    #[tokio::test]
    async fn batch_one_bad_pair_applies_nothing() {
        let t = tool();
        seed(&t, "atomic.txt", "keep\n").await;
        let r = t
            .execute(serde_json::json!({
                "path": "atomic.txt",
                "edits": [
                    {"old_string": "keep", "new_string": "CHANGED"},
                    {"old_string": "does-not-exist", "new_string": "x"}
                ]
            }))
            .await
            .unwrap();
        assert!(!r.success, "one bad pair must reject the whole call");
        let path = t.validator.resolve_path("atomic.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "keep\n",
            "nothing may be half-applied"
        );
    }

    #[tokio::test]
    async fn whitespace_tolerant_fallback_declares_itself() {
        let t = tool();
        seed(
            &t,
            "indent.txt",
            "function foo() {\n    return 1;\n}\nlet z = 0;\n",
        )
        .await;
        // Wrong indentation AND multi-line so it is NOT a substring of the
        // file (a single under-indented line would substring-match and take
        // the exact path instead of the fallback).
        let r = t
            .execute(serde_json::json!({
                "path": "indent.txt",
                "old_string": "function foo() {\n  return 1;\n}",
                "new_string": "function foo() {\n    return 2;\n}"
            }))
            .await
            .unwrap();
        assert!(r.success, "{:?}", r.data);
        assert_eq!(
            r.data["whitespace_tolerant"],
            serde_json::json!(true),
            "fallback use must be declared"
        );
        let path = t.validator.resolve_path("indent.txt").unwrap();
        let disk = std::fs::read_to_string(path).unwrap();
        assert!(
            disk.contains("    return 2;"),
            "new text's indentation wins: {disk}"
        );
    }

    #[tokio::test]
    async fn invalid_json_never_reaches_disk() {
        let t = tool();
        seed(&t, "m.json", "{\"ok\": true}\n").await;
        let r = t
            .execute(serde_json::json!({
                "path": "m.json",
                "old_string": "\"ok\": true",
                "new_string": "\"ok\": tru," // trailing comma → invalid JSON
            }))
            .await
            .unwrap();
        assert!(!r.success, "syntax gate must reject");
        assert!(
            r.error.unwrap_or_default().contains("INVALID"),
            "error must say nothing was written"
        );
        let path = t.validator.resolve_path("m.json").unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "{\"ok\": true}\n",
            "file must be untouched"
        );
    }

    #[tokio::test]
    async fn context_echo_has_numbered_lines() {
        let t = tool();
        let body: String = (0..20)
            .map(|i| format!("line{i}\n"))
            .collect::<Vec<_>>()
            .join("");
        seed(&t, "echo.txt", &body).await;
        let r = t
            .execute(serde_json::json!({
                "path": "echo.txt",
                "old_string": "line10", "new_string": "LINE10"
            }))
            .await
            .unwrap();
        assert!(r.success);
        let ctx = r.data["context"][0].as_str().unwrap();
        assert!(
            ctx.contains("11: LINE10"),
            "echo must center the change: {ctx}"
        );
        assert!(ctx.contains("6: line5"), "echo must show ±5 lines: {ctx}");
    }
}
