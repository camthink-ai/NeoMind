//! Honest vision path for scheduled / event agents.
//!
//! When the primary LLM already sees bound images as multimodal content, the
//! host strips the `vision` tool. Standing prompts must then tell the model to
//! analyze those images directly — otherwise it hunts for a missing vision
//! tool/skill/extension, writes "Vision tool not available", and the run still
//! lands Completed with high confidence (T4 Dongpingshan false-success).

use neomind_storage::DataCollected;

/// How this execution can (or cannot) analyze bound images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionExecutionPath {
    /// No embeddable image payload in collected data.
    None,
    /// Primary LLM is multimodal; images go in the user message. Vision tool
    /// must be excluded and standing guidance rewritten accordingly.
    InlineMultimodal,
    /// Images present; vision tool remains the analysis path.
    ViaVisionTool,
    /// Images present but neither multimodal context nor a vision tool.
    Unavailable,
}

/// Decide the vision path from capability + tool availability.
///
/// `has_embeddable_images` must mean a payload that can actually reach the
/// model or the vision tool — not merely an `_is_image` flag with undecodable
/// base64 (stripping vision in that case would leave the run blind).
pub(crate) fn resolve_vision_execution_path(
    has_embeddable_images: bool,
    llm_supports_vision: bool,
    vision_tool_available: bool,
) -> VisionExecutionPath {
    if !has_embeddable_images {
        return VisionExecutionPath::None;
    }
    if llm_supports_vision {
        return VisionExecutionPath::InlineMultimodal;
    }
    if vision_tool_available {
        return VisionExecutionPath::ViaVisionTool;
    }
    VisionExecutionPath::Unavailable
}

/// Whether collected data contains at least one image the host can embed or
/// hand to tools (HTTP(S) URL or decodable base64).
pub(crate) fn has_embeddable_image(data_collected: &[DataCollected]) -> bool {
    data_collected.iter().any(collected_image_is_embeddable)
}

fn collected_image_is_embeddable(d: &DataCollected) -> bool {
    let is_image = d
        .values
        .get("_is_image")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_image {
        return false;
    }
    if let Some(url) = d.values.get("image_url").and_then(|v| v.as_str()) {
        if !url.is_empty() {
            return true;
        }
    }
    // Reuse the same Base64 normalizer as tool_prompt (URL-safe + padding).
    super::tool_prompt::clean_collected_image_base64(d).is_some()
}

/// Standing guidance injected when the vision tool is stripped because the
/// primary model already sees images. Placed after the task definition so it
/// overrides agent prompts that still say "use the vision tool".
///
/// Wording deliberately avoids blacklisted failure markers so small VLMs do
/// not echo them into the conclusion and trip the false-success guard.
pub(crate) fn multimodal_inline_vision_guidance() -> &'static str {
    "\
## Vision Path (IMPORTANT)
Bound camera / device image(s) are already attached as multimodal content in this turn.
Your primary model can see them directly — analyze what is in the image(s) yourself.
Do NOT call a `vision` tool, load a vision skill, or search for a vision extension.
Analyze the attached image(s) directly and state only your visual findings — do not \
mention tool availability in your conclusion.
Write findings from the attached image(s). If an image is unclear, say so honestly.\n"
}

/// Error message when images are present but no analysis path exists.
pub(crate) fn vision_unavailable_error() -> &'static str {
    "Vision required but unavailable: bound image(s) present, primary LLM is not \
multimodal, and the vision tool is not available. Bind a vision-capable backend \
or enable the vision tool — refusing Completed/false-success."
}

/// Patterns that mean the model admitted it could not analyze images.
/// Table-driven: keep Chinese + English field wording from T4 Dongpingshan.
/// Prefer "vision tool" / "vision capability" phrasing over bare "vision not
/// available" (which false-positives on "night vision not available").
const VISION_FAILURE_MARKERS: &[&str] = &[
    "vision tool not available",
    "vision extension not installed",
    "cannot use vision",
    "vision tool unavailable",
    "no vision tool",
    "vision capability not available",
    "vision capability unavailable",
    "无法使用 vision",
    "没有 vision 工具",
    "vision 工具不可用",
    "vision工具不可用",
    "无法调用 vision",
    // T4 Dongpingshan / operator ZH wording
    "视觉工具不可用",
    "视觉工具不存在",
    "无法使用视觉",
    // Night-sensitive — guarded by prefix check below
    "视觉不可用",
    "vision is not available",
    "vision not available",
];

/// Honest inline analysis evidence — when present on an InlineMultimodal run,
/// incidental "no vision tool" explanations must not trip the failure guard.
const INLINE_VISION_SUCCESS_EVIDENCE: &[&str] = &[
    "直接分析",
    "附带图像",
    "附带画面",
    "内联",
    "attached image",
    "attached frame",
    "attached images",
    "analyzed the attached",
    "analyzed attached",
    "inline",
];

fn marker_hit_is_night_vision_false_positive(lower: &str, marker: &str) -> bool {
    // Bare "vision not available" / "视觉不可用" must not fire on night/IR wording.
    let night_sensitive = marker == "vision not available"
        || marker == "vision is not available"
        || marker == "视觉不可用";
    if !night_sensitive {
        return false;
    }
    let Some(idx) = lower.find(marker) else {
        return false;
    };
    let before = &lower[..idx];
    let prefixes = [
        "night ",
        "ir ",
        "infrared ",
        "low-light ",
        "low light ",
        "夜视",
        "红外",
        "夜间",
    ];
    prefixes.iter().any(|p| before.ends_with(p))
}

/// True when the conclusion text admits vision analysis failed / was missing.
pub(crate) fn conclusion_admits_vision_failure(text: &str) -> bool {
    conclusion_admits_vision_failure_for_path(text, VisionExecutionPath::None)
}

/// Path-aware variant used by the executor after the tool loop.
///
/// When `path` is InlineMultimodal and the conclusion also contains positive
/// inline-analysis evidence, incidental "no vision tool" explanations are
/// treated as commentary rather than failure (Prompt-Echo Trap).
pub(crate) fn conclusion_admits_vision_failure_for_path(
    text: &str,
    path: VisionExecutionPath,
) -> bool {
    let lower = text.to_lowercase();
    let hit = VISION_FAILURE_MARKERS.iter().any(|m| {
        let m_lower = m.to_lowercase();
        lower.contains(&m_lower) && !marker_hit_is_night_vision_false_positive(&lower, &m_lower)
    });
    if !hit {
        return false;
    }
    if matches!(path, VisionExecutionPath::InlineMultimodal) {
        let has_evidence = INLINE_VISION_SUCCESS_EVIDENCE
            .iter()
            .any(|e| lower.contains(&e.to_lowercase()));
        if has_evidence {
            return false;
        }
    }
    true
}

/// Whether this path requires rewriting standing guidance away from the
/// separate vision tool.
pub(crate) fn should_inject_inline_vision_guidance(path: VisionExecutionPath) -> bool {
    matches!(path, VisionExecutionPath::InlineMultimodal)
}

/// Whether the `vision` tool should be removed from the tool list.
pub(crate) fn should_exclude_vision_tool(path: VisionExecutionPath) -> bool {
    matches!(path, VisionExecutionPath::InlineMultimodal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_storage::DataCollected;

    fn image_dc(b64: &str) -> DataCollected {
        DataCollected {
            source: "device:cam:image_data".into(),
            data_type: "image".into(),
            values: serde_json::json!({
                "_is_image": true,
                "image_base64": b64,
                "image_mime_type": "image/jpeg",
            }),
            timestamp: 0,
        }
    }

    #[test]
    fn resolve_path_table() {
        // (embeddable, llm_vision, vision_tool, expected)
        let cases = [
            (false, false, false, VisionExecutionPath::None),
            (false, true, true, VisionExecutionPath::None),
            (true, true, true, VisionExecutionPath::InlineMultimodal),
            (true, true, false, VisionExecutionPath::InlineMultimodal),
            (true, false, true, VisionExecutionPath::ViaVisionTool),
            (true, false, false, VisionExecutionPath::Unavailable),
        ];
        for (embed, llm, tool, expected) in cases {
            assert_eq!(
                resolve_vision_execution_path(embed, llm, tool),
                expected,
                "embed={embed} llm={llm} tool={tool}"
            );
        }
    }

    #[test]
    fn exclude_and_guidance_flags_follow_inline_path() {
        assert!(should_exclude_vision_tool(
            VisionExecutionPath::InlineMultimodal
        ));
        assert!(should_inject_inline_vision_guidance(
            VisionExecutionPath::InlineMultimodal
        ));
        for path in [
            VisionExecutionPath::None,
            VisionExecutionPath::ViaVisionTool,
            VisionExecutionPath::Unavailable,
        ] {
            assert!(!should_exclude_vision_tool(path));
            assert!(!should_inject_inline_vision_guidance(path));
        }
    }

    #[test]
    fn inline_guidance_forbids_separate_vision_tool() {
        let g = multimodal_inline_vision_guidance();
        assert!(g.contains("Do NOT call a `vision` tool"));
        assert!(g.contains("do not mention tool availability"));
        assert!(g.contains("multimodal"));
        // Must NOT teach blacklisted failure phrases for small models to echo.
        assert!(!g.to_lowercase().contains("vision tool not available"));
        assert!(!g.to_lowercase().contains("no vision tool"));
    }

    #[test]
    fn conclusion_vision_failure_markers_table() {
        let positives = [
            "Vision tool not available for NE301PIR-200366",
            "Vision extension not installed… skill load skill=\"vision\"",
            "Available domains: device, message, extension; cannot use vision.",
            "VISION TOOL NOT AVAILABLE",
            "无法使用 vision 分析图像",
            "vision 工具不可用",
            "视觉工具不可用",
            "视觉工具不存在，改用 skill load skill=\"vision\" 失败",
            "无法使用视觉分析当前帧",
            "视觉不可用，无法分析 NE301PIR-200366 图像",
        ];
        for text in positives {
            assert!(
                conclusion_admits_vision_failure(text),
                "expected failure marker: {text}"
            );
        }
        let negatives = [
            "Image shows two people at a table; no gambling detected.",
            "Analyzed the attached frame directly; scene is empty road.",
            "Used shell to send a judgment notification.",
            "画面为空旷走廊，未见赌博行为。",
            "直接分析了附带图像：道路空旷。",
            "Night vision not available — frame is pitch dark, no person seen",
            "红外夜视故障导致夜间视觉不可用，画面全黑",
            "",
        ];
        for text in negatives {
            assert!(
                !conclusion_admits_vision_failure(text),
                "must not false-positive: {text}"
            );
        }
    }

    #[test]
    fn inline_path_echo_of_no_vision_tool_with_evidence_is_ok() {
        let echo = "当前环境没有 vision 工具，已直接分析内联图像：走廊空旷，无赌博行为。";
        assert!(
            !conclusion_admits_vision_failure_for_path(echo, VisionExecutionPath::InlineMultimodal),
            "inline evidence must suppress prompt-echo false fail"
        );
        assert!(
            conclusion_admits_vision_failure_for_path(echo, VisionExecutionPath::ViaVisionTool),
            "without inline path, same text is still a failure admission"
        );
        let bare = "没有 vision 工具，无法分析。";
        assert!(conclusion_admits_vision_failure_for_path(
            bare,
            VisionExecutionPath::InlineMultimodal
        ));
    }

    #[test]
    fn embeddable_image_positive_and_negative() {
        // Valid tiny JPEG-ish base64 of "hello"
        let ok = image_dc("aGVsbG8=");
        assert!(has_embeddable_image(&[ok]));

        // URL-safe / unpadded base64 must match tool_prompt normalization.
        let url_safe = image_dc("aGVsbG8");
        assert!(has_embeddable_image(&[url_safe]));

        let url = DataCollected {
            source: "device:cam:image_data".into(),
            data_type: "image".into(),
            values: serde_json::json!({
                "_is_image": true,
                "image_url": "http://127.0.0.1:9375/api/images/x.png",
            }),
            timestamp: 0,
        };
        assert!(has_embeddable_image(&[url]));

        let bad = image_dc("!!!not-base64!!!");
        assert!(!has_embeddable_image(&[bad]));

        let flag_only = DataCollected {
            source: "device:cam:image_data".into(),
            data_type: "image".into(),
            values: serde_json::json!({ "_is_image": true }),
            timestamp: 0,
        };
        assert!(!has_embeddable_image(&[flag_only]));

        let not_image = DataCollected {
            source: "device:temp:temperature".into(),
            data_type: "metric".into(),
            values: serde_json::json!({ "value": 21.5 }),
            timestamp: 0,
        };
        assert!(!has_embeddable_image(&[not_image]));
    }
}
