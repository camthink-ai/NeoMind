//! Integration tests for web_fetch, file_write, and file_edit tools.
//!
//! Tests real tool execution with actual file I/O and HTTP requests.
//! No LLM needed — these test the tool logic directly.
//!
//! Run:
//!   cargo test --test web_fetch_file_tools_test
//!
//! Web fetch tests that hit the real network use #[ignore]:
//!   cargo test --test web_fetch_file_tools_test -- --ignored

use std::path::PathBuf;

use neomind_agent::toolkit::{FileEditTool, FileWriteTool, Tool, ToolOutput, WebFetchTool};

// ============================================================================
// Helpers
// ============================================================================

/// Create tools with a real temp directory that gets cleaned up after test.
struct TestEnv {
    data_dir: PathBuf,
    write_tool: FileWriteTool,
    edit_tool: FileEditTool,
    /// Reserved for web_fetch integration tests (currently no callers —
    /// kept wired so future tests don't need to re-plumb the constructor).
    _fetch_tool: WebFetchTool,
}

impl TestEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let data_dir = dir.keep();

        // Create subdirectories that exist in production
        std::fs::create_dir_all(data_dir.join("skills")).unwrap();
        std::fs::create_dir_all(data_dir.join("extensions")).unwrap();
        std::fs::create_dir_all(data_dir.join("frontend-components")).unwrap();

        Self {
            write_tool: FileWriteTool::new(data_dir.clone()),
            edit_tool: FileEditTool::new(data_dir.clone()),
            _fetch_tool: WebFetchTool::new(),
            data_dir,
        }
    }
}

fn assert_success(result: Result<ToolOutput, neomind_agent::toolkit::ToolError>) -> ToolOutput {
    let output = result.expect("tool should not return error");
    if !output.success {
        panic!("tool returned failure: {:?}", output.error);
    }
    output
}

fn assert_failure(result: Result<ToolOutput, neomind_agent::toolkit::ToolError>) {
    match result {
        Ok(output) if !output.success => {} // expected failure via ToolOutput
        Err(_) => {}                        // expected failure via Error
        Ok(output) => panic!("expected failure but got success: {:?}", output.data),
    }
}

// ============================================================================
// file_write tests
// ============================================================================

#[tokio::test]
async fn file_write_creates_file_in_data_dir() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "test.txt",
            "content": "hello world"
        }))
        .await;

    let output = assert_success(result);
    assert!(output.data["bytes_written"].as_u64().unwrap() > 0);

    // Verify file exists and has correct content
    let content = std::fs::read_to_string(env.data_dir.join("test.txt")).unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn file_write_creates_subdirectory() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "deeply/nested/dir/output.json",
            "content": "{\"key\": \"value\"}"
        }))
        .await;

    assert_success(result);
    let content =
        std::fs::read_to_string(env.data_dir.join("deeply/nested/dir/output.json")).unwrap();
    assert!(content.contains("key"));
}

#[tokio::test]
async fn file_write_creates_custom_subdirectory() {
    let env = TestEnv::new();
    // Production: LLM might create new subdirectories not pre-existing
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "my-custom-data/export.csv",
            "content": "a,b,c\n1,2,3"
        }))
        .await;

    assert_success(result);
    assert!(env.data_dir.join("my-custom-data/export.csv").exists());
}

#[tokio::test]
async fn file_write_overwrites_existing() {
    let env = TestEnv::new();

    // Write first version
    env.write_tool
        .execute(serde_json::json!({
            "path": "skills/test-skill.md",
            "content": "# Version 1"
        }))
        .await
        .unwrap();

    // Overwrite
    env.write_tool
        .execute(serde_json::json!({
            "path": "skills/test-skill.md",
            "content": "# Version 2"
        }))
        .await
        .unwrap();

    let content = std::fs::read_to_string(env.data_dir.join("skills/test-skill.md")).unwrap();
    assert_eq!(content, "# Version 2");
}

#[tokio::test]
async fn file_write_no_create_dirs_fails_when_missing_parent() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "nonexistent/subdir/file.txt",
            "content": "test",
            "create_dirs": false
        }))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn file_write_rejects_path_traversal() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "../../etc/passwd",
            "content": "hacked"
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn file_write_rejects_absolute_outside_data() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "/etc/passwd",
            "content": "hacked"
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn file_write_rejects_binary_extensions() {
    let env = TestEnv::new();
    for ext in &["so", "dll", "exe", "sys"] {
        let result = env
            .write_tool
            .execute(serde_json::json!({
                "path": format!("test.{}", ext),
                "content": "binary data"
            }))
            .await;
        assert!(result.is_err(), "should reject .{} files", ext);
    }
}

#[tokio::test]
async fn file_write_rejects_env_file() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": ".env",
            "content": "SECRET=token"
        }))
        .await;
    assert!(result.is_err(), "should reject .env");
}

#[tokio::test]
async fn file_write_allows_code_files() {
    let env = TestEnv::new();
    // .rs, .toml, .conf, .py, .js should all be allowed in data dir
    for (name, content) in &[
        ("src/lib.rs", "fn main() {}"),
        ("Cargo.toml", "[package]\nname = \"test\""),
        ("config/app.conf", "key = value"),
        ("scripts/run.py", "print('hello')"),
    ] {
        let result = env
            .write_tool
            .execute(serde_json::json!({
                "path": *name,
                "content": *content
            }))
            .await;
        let output = assert_success(result);
        assert!(
            output.data["bytes_written"].as_u64().unwrap() > 0,
            "should allow {}",
            name
        );
    }
}

#[tokio::test]
async fn file_write_unicode_content() {
    let env = TestEnv::new();
    let content = "你好世界 🌍 こんにちは世界";
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "unicode.txt",
            "content": content
        }))
        .await;

    assert_success(result);
    let read_back = std::fs::read_to_string(env.data_dir.join("unicode.txt")).unwrap();
    assert_eq!(read_back, content);
}

#[tokio::test]
async fn file_write_large_content() {
    let env = TestEnv::new();
    let content = "x".repeat(100_000);
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "large.txt",
            "content": content
        }))
        .await;

    let output = assert_success(result);
    assert_eq!(output.data["bytes_written"].as_u64().unwrap(), 100_000);
}

#[tokio::test]
async fn file_write_skill_file_realistic() {
    let env = TestEnv::new();
    let skill_content = r#"---
id: my-skill
name: My Custom Skill
triggers:
  keywords: ["custom", "test"]
---

# My Custom Skill

## Steps
1. Do thing A
2. Do thing B
3. Done
"#;
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "skills/my-skill.md",
            "content": skill_content
        }))
        .await;

    assert_success(result);
    let written = std::fs::read_to_string(env.data_dir.join("skills/my-skill.md")).unwrap();
    assert!(written.contains("my-skill"));
}

#[tokio::test]
async fn file_write_widget_bundle_realistic() {
    let env = TestEnv::new();
    let js_content = r#"var MyWidget = (function() {
  var React = window.React;
  function MyWidget(props) {
    return React.createElement('div', null, 'Hello');
  }
  return { default: MyWidget };
})();
"#;
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "frontend-components/my-widget/bundle.js",
            "content": js_content
        }))
        .await;

    assert_success(result);
}

// ============================================================================
// file_edit tests
// ============================================================================

#[tokio::test]
async fn file_edit_basic_replacement() {
    let env = TestEnv::new();

    // Create file first
    env.write_tool
        .execute(serde_json::json!({
            "path": "edit-test.txt",
            "content": "hello world\nsecond line"
        }))
        .await
        .unwrap();

    // Edit
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "edit-test.txt",
            "old_string": "hello world",
            "new_string": "goodbye world"
        }))
        .await;

    let output = assert_success(result);
    assert_eq!(output.data["replacements"].as_u64().unwrap(), 1);

    let content = std::fs::read_to_string(env.data_dir.join("edit-test.txt")).unwrap();
    assert_eq!(content, "goodbye world\nsecond line");
}

#[tokio::test]
async fn file_edit_multiline_replacement() {
    let env = TestEnv::new();

    env.write_tool
        .execute(serde_json::json!({
            "path": "multiline.md",
            "content": "# Title\n\nOld paragraph.\n\n## Section"
        }))
        .await
        .unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "multiline.md",
            "old_string": "Old paragraph.",
            "new_string": "New paragraph with **bold**."
        }))
        .await;

    assert_success(result);
    let content = std::fs::read_to_string(env.data_dir.join("multiline.md")).unwrap();
    assert!(content.contains("**bold**"));
    assert!(!content.contains("Old paragraph"));
}

#[tokio::test]
async fn file_edit_replace_all() {
    let env = TestEnv::new();

    env.write_tool
        .execute(serde_json::json!({
            "path": "replace-all.txt",
            "content": "foo bar foo baz foo"
        }))
        .await
        .unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "replace-all.txt",
            "old_string": "foo",
            "new_string": "qux",
            "replace_all": true
        }))
        .await;

    let output = assert_success(result);
    assert_eq!(output.data["replacements"].as_u64().unwrap(), 3);

    let content = std::fs::read_to_string(env.data_dir.join("replace-all.txt")).unwrap();
    assert_eq!(content, "qux bar qux baz qux");
}

#[tokio::test]
async fn file_edit_rejects_multiple_matches_without_replace_all() {
    let env = TestEnv::new();

    env.write_tool
        .execute(serde_json::json!({
            "path": "multi-match.txt",
            "content": "foo bar foo"
        }))
        .await
        .unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "multi-match.txt",
            "old_string": "foo",
            "new_string": "baz"
        }))
        .await;

    // Should fail (multiple matches, replace_all=false)
    assert_failure(result);
}

#[tokio::test]
async fn file_edit_not_found_returns_helpful_error() {
    let env = TestEnv::new();

    env.write_tool
        .execute(serde_json::json!({
            "path": "exists.txt",
            "content": "line1\nline2\nline3"
        }))
        .await
        .unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "exists.txt",
            "old_string": "nonexistent text",
            "new_string": "replacement"
        }))
        .await;

    let output = result.expect("tool should not error");
    assert!(!output.success);
    // Error should contain file preview to help LLM
    let error_msg = output.error.unwrap_or_default();
    assert!(
        error_msg.contains("not found") || error_msg.contains("preview"),
        "error should contain helpful context: {}",
        error_msg
    );
}

#[tokio::test]
async fn file_edit_file_not_found() {
    let env = TestEnv::new();
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "nonexistent.txt",
            "old_string": "a",
            "new_string": "b"
        }))
        .await;

    let output = result.unwrap();
    assert!(!output.success);
}

#[tokio::test]
async fn file_edit_identical_strings() {
    let env = TestEnv::new();
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "test.txt",
            "old_string": "same",
            "new_string": "same"
        }))
        .await;

    let output = result.unwrap();
    assert!(!output.success);
}

#[tokio::test]
async fn file_edit_rejects_path_traversal() {
    let env = TestEnv::new();
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "../../etc/passwd",
            "old_string": "a",
            "new_string": "b"
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn file_edit_skill_update_realistic() {
    let env = TestEnv::new();

    // Create skill
    env.write_tool
        .execute(serde_json::json!({
            "path": "skills/my-skill.md",
            "content": "---\nid: my-skill\nname: Test\n---\n\n# Step 1\nDo the old thing."
        }))
        .await
        .unwrap();

    // Update skill content
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "skills/my-skill.md",
            "old_string": "Do the old thing.",
            "new_string": "Do the new thing.\n\n## Common Errors\n- Error A: fix by doing B"
        }))
        .await;

    assert_success(result);
    let content = std::fs::read_to_string(env.data_dir.join("skills/my-skill.md")).unwrap();
    assert!(content.contains("new thing"));
    assert!(content.contains("Common Errors"));
}

// ============================================================================
// web_fetch tests
// ============================================================================

#[tokio::test]
async fn web_fetch_rejects_localhost() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://localhost:9375/api/docs"
        }))
        .await;

    assert_failure(result);
}

#[tokio::test]
async fn web_fetch_rejects_private_ip() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://192.168.1.1"
        }))
        .await;

    assert_failure(result);
}

#[tokio::test]
async fn web_fetch_rejects_127() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://127.0.0.1:9375"
        }))
        .await;

    assert_failure(result);
}

#[tokio::test]
async fn web_fetch_rejects_10_network() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://10.0.0.1"
        }))
        .await;

    assert_failure(result);
}

#[tokio::test]
async fn web_fetch_rejects_172_16() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://172.16.0.1"
        }))
        .await;

    assert_failure(result);
}

#[tokio::test]
async fn web_fetch_rejects_ftp() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "ftp://example.com"
        }))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn web_fetch_rejects_invalid_url() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "not-a-url"
        }))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn web_fetch_missing_url() {
    let tool = WebFetchTool::new();
    let result = tool.execute(serde_json::json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn web_fetch_allows_public_ip() {
    let tool = WebFetchTool::new();
    // 8.8.8.8 is Google DNS — public IP, should pass SSRF check
    // (network test below will actually fetch)
    let result = tool
        .execute(serde_json::json!({
            "url": "http://8.8.8.8"
        }))
        .await;

    // May succeed or fail depending on network, but should NOT be blocked by SSRF
    match result {
        Ok(output) => {
            // If it got through SSRF check, it either succeeded or got HTTP error
            assert!(
                output.success || output.error.is_some(),
                "Should succeed or have HTTP error, not SSRF block"
            );
        }
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                !msg.contains("private") && !msg.contains("local"),
                "Should not block 8.8.8.8 as private: {}",
                msg
            );
        }
    }
}

/// Real network test — fetches example.com
#[tokio::test]
#[ignore] // Run with --ignored to hit real network
async fn web_fetch_real_example_com() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "https://example.com",
            "format": "text"
        }))
        .await;

    let output = assert_success(result);
    let content = output.data["content"].as_str().unwrap();
    assert!(
        content.contains("Example Domain"),
        "should contain page text"
    );
    assert!(!content.contains("<html>"), "should strip HTML tags");
}

/// Real network test — raw format preserves HTML
#[tokio::test]
#[ignore]
async fn web_fetch_raw_format() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "https://example.com",
            "format": "raw"
        }))
        .await;

    let output = assert_success(result);
    let content = output.data["content"].as_str().unwrap();
    // example.com uses lowercase <html — check for tag presence
    assert!(
        content.contains("<html") || content.contains("<HTML"),
        "raw format should preserve HTML tags, got: {}",
        &content[..content.len().min(200)]
    );
}

/// Real network test — max_length truncation
#[tokio::test]
#[ignore]
async fn web_fetch_truncation() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "https://example.com",
            "max_length": 100
        }))
        .await;

    let output = assert_success(result);
    assert!(
        output.data["truncated"].as_bool().unwrap(),
        "should be truncated"
    );
}

/// Real network test — JSON API
#[tokio::test]
#[ignore]
async fn web_fetch_json_api() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "https://httpbin.org/json"
        }))
        .await;

    let output = assert_success(result);
    let content_type = output.data["content_type"].as_str().unwrap();
    assert!(content_type.contains("json"));
    let content = output.data["content"].as_str().unwrap();
    assert!(content.contains("slideshow") || content.contains("url"));
}

/// Real network test — 404 error handling
#[tokio::test]
#[ignore]
async fn web_fetch_404() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "https://httpbin.org/status/404"
        }))
        .await;

    let output = result.unwrap();
    assert!(!output.success, "404 should be reported as failure");
}

// ============================================================================
// End-to-end: write → edit → verify chain
// ============================================================================

#[tokio::test]
async fn e2e_write_edit_chain() {
    let env = TestEnv::new();

    // Step 1: Write a widget manifest
    let manifest = r#"{
  "id": "my-widget",
  "name": "My Widget",
  "version": "1.0.0",
  "global_name": "MyWidget",
  "has_data_source": false
}"#;
    env.write_tool
        .execute(serde_json::json!({
            "path": "frontend-components/my-widget/manifest.json",
            "content": manifest
        }))
        .await
        .unwrap();

    // Step 2: Write bundle.js
    let bundle = "var MyWidget = (function() { return { default: function() {} }; })();";
    env.write_tool
        .execute(serde_json::json!({
            "path": "frontend-components/my-widget/bundle.js",
            "content": bundle
        }))
        .await
        .unwrap();

    // Step 3: Edit manifest to update version
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "frontend-components/my-widget/manifest.json",
            "old_string": "\"version\": \"1.0.0\"",
            "new_string": "\"version\": \"1.1.0\""
        }))
        .await;

    let output = assert_success(result);
    assert_eq!(output.data["replacements"].as_u64().unwrap(), 1);

    // Verify final state
    let final_manifest = std::fs::read_to_string(
        env.data_dir
            .join("frontend-components/my-widget/manifest.json"),
    )
    .unwrap();
    assert!(final_manifest.contains("1.1.0"));
    assert!(!final_manifest.contains("1.0.0"));
    assert!(env
        .data_dir
        .join("frontend-components/my-widget/bundle.js")
        .exists());
}

// ============================================================================
// Tool registration verification
// ============================================================================

#[test]
fn tool_definitions_are_valid() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.keep();

    let write_tool = FileWriteTool::new(data_dir.clone());
    let edit_tool = FileEditTool::new(data_dir);
    let fetch_tool = WebFetchTool::new();

    // Verify tool names
    assert_eq!(write_tool.name(), "file_write");
    assert_eq!(edit_tool.name(), "file_edit");
    assert_eq!(fetch_tool.name(), "web_fetch");

    // Verify parameters are valid JSON Schema
    for tool in [
        &write_tool as &dyn Tool,
        &edit_tool as &dyn Tool,
        &fetch_tool as &dyn Tool,
    ] {
        let params = tool.parameters();
        assert!(
            params.is_object(),
            "{} params should be object",
            tool.name()
        );
        assert!(
            params.get("properties").is_some(),
            "{} should have properties",
            tool.name()
        );
        assert!(
            params.get("required").is_some(),
            "{} should have required",
            tool.name()
        );
    }
}

// ============================================================================
// Additional tests for security model enhancements
// ============================================================================

#[tokio::test]
async fn file_write_rejects_env_variants() {
    let env = TestEnv::new();
    for name in &[".env.local", ".env.production", ".env.development"] {
        let result = env
            .write_tool
            .execute(serde_json::json!({
                "path": *name,
                "content": "SECRET=token"
            }))
            .await;
        assert!(result.is_err(), "should reject {}", name);
    }
}

#[tokio::test]
async fn file_write_rejects_case_insensitive_binary() {
    let env = TestEnv::new();
    for ext in &["DLL", "EXE", "SO", "Sys"] {
        let result = env
            .write_tool
            .execute(serde_json::json!({
                "path": format!("test.{}", ext),
                "content": "binary data"
            }))
            .await;
        assert!(result.is_err(), "should reject .{} files", ext);
    }
}

#[tokio::test]
async fn file_write_rejects_content_too_large() {
    let env = TestEnv::new();
    // 1 MB + 1 byte
    let content = "x".repeat(1024 * 1024 + 1);
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "large.txt",
            "content": content
        }))
        .await;
    assert!(result.is_err(), "should reject content > 1MB");
}

#[tokio::test]
async fn file_edit_empty_old_string_appends() {
    let env = TestEnv::new();
    // Create a file first
    env.write_tool
        .execute(serde_json::json!({
            "path": "test.txt",
            "content": "hello world"
        }))
        .await
        .unwrap();

    // v2 semantics: an EMPTY old_string (single-pair form) appends
    // new_string to the end of the file instead of rejecting. A line
    // separator is inserted when the file doesn't already end with one.
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "test.txt",
            "old_string": "",
            "new_string": "appended line"
        }))
        .await
        .unwrap();
    assert!(result.success, "empty old_string is append mode");

    let content = std::fs::read_to_string(env.data_dir.join("test.txt")).unwrap();
    assert_eq!(content, "hello world\nappended line");
}

#[tokio::test]
async fn web_fetch_rejects_ipv4_compatible_ipv6() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://[::192.168.1.1]/"
        }))
        .await;
    assert!(
        result.is_err(),
        "should reject IPv4-compatible IPv6 to private IP"
    );
}

#[tokio::test]
async fn web_fetch_rejects_ipv4_mapped_ipv6() {
    let tool = WebFetchTool::new();
    let result = tool
        .execute(serde_json::json!({
            "url": "http://[::ffff:127.0.0.1]:9375/"
        }))
        .await;
    assert!(
        result.is_err(),
        "should reject IPv4-mapped IPv6 to localhost"
    );
}

// ============================================================================
// Additional coverage tests
// ============================================================================

#[tokio::test]
async fn file_edit_handles_crlf_endings() {
    let env = TestEnv::new();
    // Create file with CRLF line endings
    let crlf_content = "line1\r\nline2\r\nline3";
    std::fs::write(env.data_dir.join("crlf.txt"), crlf_content).unwrap();

    // LLM sends LF in old_string — should still match
    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "crlf.txt",
            "old_string": "line2",
            "new_string": "line2_updated"
        }))
        .await;

    let output = assert_success(result);
    assert_eq!(output.data["replacements"].as_u64().unwrap(), 1);

    // Verify content — CRLF preserved for non-edited lines
    let updated = std::fs::read_to_string(env.data_dir.join("crlf.txt")).unwrap();
    assert!(
        updated.contains("line1\r\n"),
        "CRLF should be preserved on line1"
    );
    assert!(updated.contains("line2_updated"), "replacement applied");
}

#[tokio::test]
async fn file_write_rejects_empty_path() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "",
            "content": "hello"
        }))
        .await;
    assert!(result.is_err(), "should reject empty path");

    // Whitespace-only path should also be rejected
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "   ",
            "content": "hello"
        }))
        .await;
    assert!(result.is_err(), "should reject whitespace-only path");
}

#[tokio::test]
async fn file_edit_rejects_file_too_large() {
    let env = TestEnv::new();
    // Create a file larger than 10 MB
    let large_content = "x".repeat(11 * 1024 * 1024);
    std::fs::write(env.data_dir.join("large.txt"), &large_content).unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "large.txt",
            "old_string": "x",
            "new_string": "y"
        }))
        .await;

    assert!(result.is_err(), "should reject editing file > 10MB");
}

#[tokio::test]
async fn file_write_empty_content_allowed() {
    let env = TestEnv::new();
    let result = env
        .write_tool
        .execute(serde_json::json!({
            "path": "empty.txt",
            "content": ""
        }))
        .await;
    let output = assert_success(result);
    assert_eq!(output.data["bytes_written"].as_u64().unwrap(), 0);
    assert_eq!(
        std::fs::read_to_string(env.data_dir.join("empty.txt")).unwrap(),
        ""
    );
}

#[tokio::test]
async fn file_edit_not_found_helpful_error() {
    let env = TestEnv::new();
    env.write_tool
        .execute(serde_json::json!({
            "path": "skills/test.md",
            "content": "# Title\nLine 2\nLine 3\nLine 4\nLine 5"
        }))
        .await
        .unwrap();

    let result = env
        .edit_tool
        .execute(serde_json::json!({
            "path": "skills/test.md",
            "old_string": "nonexistent text",
            "new_string": "replaced"
        }))
        .await
        .unwrap();

    assert!(!result.success);
    let error = result.error.unwrap();
    assert!(error.contains("not found"), "error should say not found");
    assert!(
        error.contains("First lines"),
        "should include the file's first lines for context"
    );
}
