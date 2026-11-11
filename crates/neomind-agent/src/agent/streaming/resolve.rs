use super::super::types::LargeDataCache;

/// Argument names that typically hold image/base64 data.
const IMAGE_ARG_NAMES: &[&str] = &["image", "image_base64", "base64_data", "image_data", "img"];

/// Tools that legitimately consume an image argument. The omitted-field
/// auto-inject below is gated on this list to prevent leaking user-uploaded
/// images into tools that have nothing to do with images (e.g. `file_write`,
/// `shell`, extension tools that log arguments).
const IMAGE_AWARE_TOOLS: &[&str] = &["image_edit", "vision"];

/// Resolve `$cached:tool_name` references in tool arguments by replacing them
/// with the full cached data. Also **auto-injects** cached image data for any
/// image-related argument — the LLM cannot reliably pass binary image data, so
/// explicit source references are preserved; placeholders can use the latest cache.
///
/// HTTP(S), internal/local paths, and explicit cache selections are preserved.
pub(crate) fn resolve_cached_arguments(
    arguments: &serde_json::Value,
    cache: &LargeDataCache,
    tool_name: &str,
) -> serde_json::Value {
    // file_read is the `$cached:` READ-BACK door: its `path` may BE a
    // reference, and expanding it here would inject the whole blob where a
    // path belongs — the exact "91,680 bytes resolved into a path argument"
    // failure observed on 2026-11-09. Pass file_read arguments through
    // VERBATIM; the dispatch layer serves the reference from the cache
    // itself (`try_cached_file_read`).
    if crate::tools::resolve_tool_name(tool_name) == "file_read" {
        return arguments.clone();
    }
    match arguments {
        // Explicit $cached: reference → resolve
        serde_json::Value::String(s) if s.starts_with("$cached:") => {
            if let Some(resolved) = cache.resolve_reference(s) {
                tracing::info!(
                    reference = %s,
                    resolved_bytes = resolved.len(),
                    "Resolved cached data reference in tool arguments"
                );
                serde_json::Value::String(resolved)
            } else {
                tracing::warn!(reference = %s, "Cached data reference not found, using as-is");
                arguments.clone()
            }
        }
        serde_json::Value::Object(map) => {
            let mut resolved: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| {
                    let resolved_val = resolve_cached_arguments(v, cache, tool_name);
                    // Auto-injection for image arguments:
                    // The LLM cannot reliably pass binary image data — it will copy
                    // truncated previews, output MIME types, or invent values.
                    // If we have cached image data, always prefer it over the LLM's value.
                    if IMAGE_ARG_NAMES.contains(&k.as_str()) {
                        if let serde_json::Value::String(ref s) = resolved_val {
                            // Pass through valid HTTP(S) URLs — those are legitimate references
                            let explicit_reference = v.as_str().is_some_and(|original| {
                                original.starts_with("$cached:")
                                    || original.starts_with("http://")
                                    || original.starts_with("https://")
                                    || original.starts_with('/')
                                    || original.starts_with("./")
                            });
                            if !explicit_reference {
                                if let Some((image_data, source)) = cache.get_latest_image() {
                                    tracing::info!(
                                        arg_name = %k,
                                        original_preview = %s.chars().take(80).collect::<String>(),
                                        source = %source,
                                        injected_bytes = image_data.len(),
                                        "Auto-injected cached image data (LLM cannot pass binary data)"
                                    );
                                    return (k.clone(), serde_json::Value::String(image_data));
                                }
                            }
                        }
                    }
                    (k.clone(), resolved_val)
                })
                .collect();
            // Defense-in-depth: if the LLM entirely OMITTED an image argument
            // (common when chat-uploaded images are visible in its context but
            // the LLM has no string to pass), and we have a cached user image,
            // inject it under the canonical `image` key. Without this, tools
            // like `image_edit` fail with "missing field `image`" before any
            // tool-specific logic runs.
            //
            // CRITICAL: gated on `IMAGE_AWARE_TOOLS` so user-uploaded images
            // don't leak into tools that have no business receiving image data
            // (could cause silent privacy leaks via tool arg logging).
            if IMAGE_AWARE_TOOLS.contains(&tool_name)
                && !resolved
                    .keys()
                    .any(|k| IMAGE_ARG_NAMES.contains(&k.as_str()))
            {
                if let Some((image_data, source)) = cache.get_latest_image() {
                    tracing::info!(
                        tool = %tool_name,
                        source = %source,
                        injected_bytes = image_data.len(),
                        "Auto-injected `image` field — LLM omitted all image args but cached user image exists"
                    );
                    resolved.insert("image".to_string(), serde_json::Value::String(image_data));
                }
            }
            serde_json::Value::Object(resolved)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| resolve_cached_arguments(v, cache, tool_name))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Map simplified tool names to real tool names.
///
/// Simplified names are used in LLM prompts (e.g., "device.discover")
/// while real names are used in ToolRegistry (e.g., "list_devices").
///
/// NOTE: This now uses the unified ToolNameMapper to ensure consistency.
pub(crate) fn resolve_tool_name(simplified_name: &str) -> String {
    crate::tools::resolve_tool_name(simplified_name)
}

/// The `$cached:` TEXT read-back door (2026-11-09 round).
///
/// `file_read(path="$cached:<ref>")` reads the blob from the CURRENT
/// session/execution cache — the reference reaches the reader VERBATIM
/// (`resolve_cached_arguments` deliberately leaves file_read args
/// untouched). Serves character windows with a hard cap; unknown/evicted/
/// cross-session references produce an explicit error, never empty
/// content. Image references keep their existing consumers
/// (vision/image_edit via `resolve_cached_arguments`) — this door only
/// serves file_read.
///
/// Registry-gated (2026-11-09 review round): the door serves a read ONLY
/// when the tool is registered and NOT disabled — otherwise it returns
/// `None` and the call falls through to the regular registry execution,
/// which produces the same `Disabled`/`NotFound` error any other disabled
/// tool call gets. A cache short-circuit must never bypass the registry's
/// defense-in-depth checks.
///
/// Returns `None` when the call is not a (servable) cached read: normal
/// file reads, disabled/unregistered file_read, and every other tool flow
/// through the regular execution path unchanged.
pub fn try_cached_file_read(
    registry: &crate::toolkit::registry::ToolRegistry,
    tool_name: &str,
    arguments: &serde_json::Value,
    cache: &LargeDataCache,
) -> Option<crate::toolkit::ToolOutput> {
    let resolved = crate::tools::resolve_tool_name(tool_name);
    if resolved != "file_read" {
        return None;
    }
    // Existence + disabled-state check BEFORE touching the cache. Falling
    // through to registry execution preserves the exact Disabled/NotFound
    // errors the normal path would produce.
    if registry.get(&resolved).is_none() || registry.is_disabled(&resolved) {
        return None;
    }
    let path = arguments.get("path")?.as_str()?;
    if !path.starts_with("$cached:") {
        return None;
    }
    let char_offset = arguments
        .get("char_offset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let char_length = arguments
        .get("char_length")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    Some(cache.file_read_cached(path, char_offset, char_length))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_image_selection_is_not_replaced_by_latest_upload() {
        let mut cache = LargeDataCache::new();
        cache.store_user_image("user_image", "data:image/png;base64,iVBORw0KGgo=");
        cache.store_user_image("selected", "data:image/jpeg;base64,/9j/AA==");
        for source in [
            "/api/images/device/image/selected.jpg",
            "/home/user/selected.jpg",
            "$cached:selected",
            "$cached:missing",
        ] {
            let args = serde_json::json!({"image":source});
            let actual = resolve_cached_arguments(&args, &cache, "vision");
            let expected = if source == "$cached:selected" {
                cache.resolve_reference(source).unwrap()
            } else {
                source.to_string()
            };
            assert_eq!(actual["image"], expected, "source {source}");
        }
    }

    #[test]
    fn test_resolve_cached_arguments_passthrough() {
        let cache = LargeDataCache::new();
        let args = serde_json::json!({"name": "test", "count": 42});
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        assert_eq!(resolved["name"], "test");
        assert_eq!(resolved["count"], 42);
    }

    #[test]
    fn test_resolve_cached_arguments_missing_ref() {
        let cache = LargeDataCache::new();
        let args = serde_json::json!("$cached:nonexistent");
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        // Should pass through as-is when ref not found
        assert_eq!(resolved, serde_json::json!("$cached:nonexistent"));
    }

    #[test]
    fn test_http_urls_passed_through() {
        let cache = LargeDataCache::new();
        let args = serde_json::json!({"image": "https://example.com/img.png"});
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        assert_eq!(resolved["image"], "https://example.com/img.png");
    }

    #[test]
    fn test_resolve_tool_name_passthrough() {
        // Unknown simplified names should pass through unchanged
        let result = resolve_tool_name("unknown_tool");
        assert_eq!(result, "unknown_tool");
    }

    fn registry_with_file_read() -> crate::toolkit::registry::ToolRegistry {
        let mut registry = crate::toolkit::registry::ToolRegistry::new();
        registry.register(std::sync::Arc::new(
            crate::toolkit::file_read::FileReadTool::new(std::env::temp_dir()),
        ));
        registry
    }

    /// The `$cached:` read-back door only serves file_read with a
    /// reference path — every other call (other tools, plain file reads,
    /// DISABLED or unregistered file_read) flows through the regular
    /// execution path unchanged.
    #[test]
    fn try_cached_file_read_door_contract() {
        let mut cache = LargeDataCache::new();
        let payload = "{\"history\":[1,2,3]} ".repeat(4_000); // >32KB
        cache.store("shell.stdout", &payload);
        let registry = registry_with_file_read();
        let ref_args = serde_json::json!({"path": "$cached:shell.stdout", "char_offset": 10, "char_length": 500});

        // Not file_read → None (vision/shell keep their own paths).
        assert!(try_cached_file_read(
            &registry,
            "vision",
            &serde_json::json!({"image": "$cached:shell.stdout"}),
            &cache
        )
        .is_none());
        assert!(try_cached_file_read(
            &registry,
            "shell",
            &serde_json::json!({"command": "cat x"}),
            &cache
        )
        .is_none());
        // file_read with a NORMAL path → None (regular file execution).
        assert!(try_cached_file_read(
            &registry,
            "file_read",
            &serde_json::json!({"path": "logs/app.log", "offset": 1}),
            &cache
        )
        .is_none());
        // UNREGISTERED file_read → None (registry NotFound governs).
        let empty = crate::toolkit::registry::ToolRegistry::new();
        assert!(try_cached_file_read(&empty, "file_read", &ref_args, &cache).is_none());
        // DISABLED file_read → None (falls through to the registry gate).
        let disabled = registry_with_file_read();
        disabled.set_disabled(std::collections::HashSet::from(["file_read".to_string()]));
        assert!(try_cached_file_read(&disabled, "file_read", &ref_args, &cache).is_none());

        // Registered + enabled + reference → served from the cache, verbatim
        // ref in, windowed content out, char params honored.
        let out = try_cached_file_read(&registry, "file_read", &ref_args, &cache)
            .expect("door serves cached reads");
        assert!(out.success);
        let data = &out.data;
        assert_eq!(data["reference"], "$cached:shell.stdout");
        assert_eq!(data["char_offset"], serde_json::json!(10));
        assert_eq!(data["returned_chars"], serde_json::json!(500));
        assert_eq!(
            data["content"].as_str().unwrap(),
            &payload.chars().skip(10).take(500).collect::<String>()
        );

        // Unknown ref → the door still answers, with an explicit error.
        let out = try_cached_file_read(
            &registry,
            "file_read",
            &serde_json::json!({"path": "$cached:gone#1234abcd"}),
            &cache,
        )
        .expect("door answers even on failure");
        assert!(!out.success);
        assert!(
            out.error
                .unwrap_or_default()
                .contains("Unknown $cached reference"),
            "explicit unknown-ref error"
        );
    }

    /// Fallthrough contract: when the door declines a DISABLED file_read,
    /// the regular registry execution produces the Disabled error — the
    /// same refusal any other disabled tool call gets.
    #[tokio::test]
    async fn disabled_file_read_gets_registry_disabled_error() {
        let registry = registry_with_file_read();
        registry.set_disabled(std::collections::HashSet::from(["file_read".to_string()]));
        let err = registry
            .execute("file_read", serde_json::json!({"path": "x"}))
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::toolkit::error::ToolError::Disabled(ref n) if n == "file_read"),
            "registry refuses disabled tool: {:?}",
            err
        );
    }

    /// The resolver must NOT expand `$cached:` references inside file_read
    /// arguments — the ref has to reach the read-back door VERBATIM (the
    /// 2026-11-09 incident had 91,680 bytes resolved INTO the path arg).
    #[test]
    fn resolver_leaves_file_read_refs_verbatim() {
        let mut cache = LargeDataCache::new();
        let payload = "x".repeat(91_680);
        cache.store("shell.stdout", &payload);

        let args = serde_json::json!({"path": "$cached:shell.stdout", "char_offset": 0});
        let resolved = resolve_cached_arguments(&args, &cache, "file_read");
        assert_eq!(
            resolved["path"], "$cached:shell.stdout",
            "path stays the REFERENCE, not the expanded blob"
        );
        assert_eq!(resolved["char_offset"], serde_json::json!(0));

        // Control: image consumers keep full resolution (unchanged path).
        let img_args = serde_json::json!({"image": "$cached:user_image"});
        let resolved_img = resolve_cached_arguments(&img_args, &cache, "vision");
        assert_eq!(
            resolved_img["image"], "$cached:user_image",
            "unknown image ref still passes through as-is for vision"
        );
    }

    /// Regression: when the LLM omits the `image` field entirely but the user
    /// uploaded an image to chat (cached as `user_image`), we must inject it
    /// so tools like `image_edit` don't fail with "missing field `image`".
    #[test]
    fn test_omitted_image_field_auto_injected_from_user_upload() {
        let mut cache = LargeDataCache::new();
        // Must exceed CACHE_THRESHOLD_BYTES (32KB) or store() passes through unchanged.
        // Use a fake base64 data URL of the right size.
        let big_b64: String = "A".repeat(40_000);
        let data_url = format!("data:image/png;base64,{}", big_b64);
        let _summary = cache.store("user_image", &data_url);

        // LLM omitted `image` field entirely — args only have `operations`.
        let args = serde_json::json!({
            "operations": [{"type": "crop", "x": 0, "y": 0, "width": 10, "height": 10}]
        });
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");

        // `image` should now be present, with the cached data URL.
        assert!(
            resolved.get("image").is_some(),
            "image field should be auto-injected; got: {}",
            resolved
        );
        let injected = resolved["image"].as_str().unwrap();
        assert!(
            injected.contains("data:image/png;base64,"),
            "injected value should be the data URL, got: {}...",
            &injected[..injected.len().min(50)]
        );
    }

    /// PRIVACY REGRESSION: a tool that is NOT image-aware (e.g. file_write,
    /// shell, extension tools) must NOT have a cached user image silently
    /// injected into its arguments. Such tools may log args verbatim, which
    /// would leak the user's uploaded image as base64 in their logs.
    #[test]
    fn test_no_injection_for_non_image_aware_tool() {
        let mut cache = LargeDataCache::new();
        let big_b64: String = "P".repeat(40_000);
        let data_url = format!("data:image/png;base64,{}", big_b64);
        cache.store("user_image", &data_url);

        // LLM called file_write with no image arg.
        let args = serde_json::json!({"path": "/tmp/x", "content": "hi"});
        let resolved = resolve_cached_arguments(&args, &cache, "file_write");
        assert!(
            resolved.get("image").is_none(),
            "non-image-aware tool must NOT receive auto-injected image data; got: {}",
            resolved
        );
    }

    /// Sanity: when LLM already provided an `image` arg (non-URL), the inject
    /// path still overwrites it with the cached value — this is the existing
    /// behavior and the new "missing field" branch must not interfere.
    #[test]
    fn test_present_non_url_image_arg_overwritten_by_cache() {
        let mut cache = LargeDataCache::new();
        let big_b64: String = "B".repeat(40_000);
        let data_url = format!("data:image/png;base64,{}", big_b64);
        cache.store("user_image", &data_url);

        // LLM passed a garbage/truncated preview.
        let args = serde_json::json!({"image": "data:image/png;base64,AAAA"});
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        let injected = resolved["image"].as_str().unwrap();
        assert!(
            injected.len() > 100,
            "expected full cached data URL, got len {}",
            injected.len()
        );
    }

    /// Sanity: HTTP URLs must still pass through even when cache has data.
    #[test]
    fn test_http_url_not_overwritten_when_cache_has_image() {
        let mut cache = LargeDataCache::new();
        let big_b64: String = "C".repeat(40_000);
        let data_url = format!("data:image/png;base64,{}", big_b64);
        cache.store("user_image", &data_url);

        let args = serde_json::json!({"image": "https://example.com/photo.jpg"});
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        assert_eq!(resolved["image"], "https://example.com/photo.jpg");
    }

    /// Sanity: no injection when there's no cached user image.
    #[test]
    fn test_no_injection_when_cache_empty() {
        let cache = LargeDataCache::new();
        let args = serde_json::json!({"operations": []});
        let resolved = resolve_cached_arguments(&args, &cache, "image_edit");
        // `image` should NOT be auto-added.
        assert!(resolved.get("image").is_none());
    }

    /// Regression (task #50, "Garbage Monitoring"): a scheduled/event agent
    /// receives its bound device image ONLY as a multimodal vision part. The
    /// LLM cannot reproduce base64 from a vision embedding, so it emits a
    /// truncated/fragmentary `data:image/jpeg;base64,...` (recognizable JPEG
    /// header, junk body) into an extension tool's `image` arg — the extension
    /// then decodes garbage and returns `null`.
    ///
    /// When the cache is seeded with the real bound image (`seed_bound_image`),
    /// the existing present-arg overwrite path must replace that fragment with
    /// the full image, for ANY tool whose arg name is image-shaped (extension
    /// tools are deliberately NOT in IMAGE_AWARE_TOOLS, but the present-arg
    /// branch is name-based, not tool-gated).
    #[test]
    fn seeded_bound_image_replaces_llm_fragment_for_extension_tool() {
        let mut cache = LargeDataCache::new();
        // The real bound image (a data URL the agent injects into the cache).
        let real_b64 = "U".repeat(8192);
        let bound_url = format!("data:image/jpeg;base64,{}", real_b64);
        cache.seed_bound_image(&bound_url);

        // What the LLM actually emitted — a fragment starting with the
        // well-known JPEG data-URI header, but NOT the real image bytes.
        let llm_fragment = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEASABIAAD";
        let args = serde_json::json!({ "image": llm_fragment });

        // Extension tool name — not in IMAGE_AWARE_TOOLS.
        let resolved =
            resolve_cached_arguments(&args, &cache, "yolo-device-inference:analyze_image");

        let got = resolved["image"].as_str().expect("image arg present");
        assert_ne!(
            got, llm_fragment,
            "LLM's truncated fragment must not pass through to the extension"
        );
        assert!(
            got.contains(&real_b64),
            "extension should receive the real bound image, got: {}...",
            &got[..got.len().min(80)]
        );
    }

    /// `seed_bound_image` must work even for a small bound image (the regular
    /// `store()` skips anything under the 32KB threshold, which would leave
    /// small device images uninjected — the exact task #50 failure mode).
    #[test]
    fn seed_bound_image_bypasses_size_threshold() {
        let mut cache = LargeDataCache::new();
        // ~1KB — well under CACHE_THRESHOLD_BYTES (32KB).
        let small_b64 = "Q".repeat(1024);
        let bound_url = format!("data:image/png;base64,{}", small_b64);
        cache.seed_bound_image(&bound_url);

        // get_latest_image must surface it despite being tiny.
        let (data, _key) = cache.get_latest_image().expect("small bound image seeded");
        assert!(
            data.contains(&small_b64),
            "seeded small image should be retrievable via get_latest_image"
        );
    }
}
