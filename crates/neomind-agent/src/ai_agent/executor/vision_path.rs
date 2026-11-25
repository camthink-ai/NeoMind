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

impl VisionExecutionPath {
    /// Stable label for records and logs.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InlineMultimodal => "inline_multimodal",
            Self::ViaVisionTool => "vision_tool",
            Self::Unavailable => "unavailable",
        }
    }
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
Camera / device image(s) for this turn are already attached as multimodal content \
(including event-carried frames when resources are unbound).
Your primary model can see them directly — analyze what is in the image(s) yourself.
The image is inside this message: there is no file path or URL to look up, and no \
tool is needed to read it.
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

/// Claims, made while an image IS attached, that the image or its path never
/// arrived — the other way a small model admits it never looked. These runs
/// used to land Completed: T4's 20035E finished "successful" with
/// "图片路径未提供，因此无法执行图像分析步骤", and 200360 answered
/// `"image_path": "/images/unknown.jpg"` on two thirds of its runs.
///
/// Checked only on the InlineMultimodal path (see
/// [`conclusion_admits_vision_failure_for_path`]): elsewhere "no image" may
/// simply be true. Device-side detections are NOT here — this firmware really
/// reports none, and an honest "检测结果未提供" next to a real analysis is
/// not a failure.
const INLINE_IMAGE_ABSENCE_MARKERS: &[&str] = &[
    "图片路径未提供",
    "图片路径缺失",
    "路径未提供",
    "无图像数据路径",
    "无图片路径",
    "未找到图片路径",
    "未找到图像路径",
    "无图像数据可用",
    "无图片数据可用",
    "没有接收任何图片数据",
    "未上传任何图片数据",
    "图片数据未提供",
    "图片未提供",
    "无图像数据",
    "未获取到图片",
    "未记录 image_data",
    "未记录image_data",
    "image_data 未提供",
    "image_data未提供",
    "image path not provided",
    "image path was not provided",
    "no image path",
    "image_path\": \"unknown",
    "image_path\":\"unknown",
    "/images/unknown.jpg",
    "no image data available",
    "image data not provided",
    "did not receive any image",
    "image not provided",
];

/// Explicit "did not analyze the image" phrases — used only to veto
/// [`INLINE_VISION_SUCCESS_EVIDENCE`] when a [`VISION_FAILURE_MARKERS`] hit
/// is already present. Must NOT short-circuit alone: night / black / blur
/// frames often say "无法识别" / "unable to analyze" while honestly reporting
/// what the camera shows (NM-331 over-block).
const INLINE_VISION_NEGATION_PHRASES: &[&str] = &[
    "无法分析",
    "不能分析",
    "未能分析",
    "无法查看",
    "无法识别",
    "cannot analyze",
    "unable to analyze",
    "could not analyze",
    "failed to analyze",
    "can't analyze",
];

/// Honest inline analysis evidence — completed-action phrases only.
/// Bare nouns like "attached image" / "inline" are excluded so failure
/// admissions such as "无法分析附带图像" still trip the guard.
const INLINE_VISION_SUCCESS_EVIDENCE: &[&str] = &[
    "已直接分析",
    "直接分析了",
    "已分析附带",
    "已查看附带",
    "analyzed the attached",
    "analyzed attached",
    "inspected the attached",
];

fn marker_hit_is_night_vision_false_positive(lower: &str, marker: &str) -> bool {
    // Bare "vision not available" / "视觉不可用" must not fire on night/IR wording.
    let night_sensitive = marker == "vision not available"
        || marker == "vision is not available"
        || marker == "视觉不可用";
    if !night_sensitive {
        return false;
    }
    let prefixes = [
        "night ",
        "night-",
        "ir ",
        "ir-",
        "infrared ",
        "infrared-",
        "thermal ",
        "low-light ",
        "low light ",
        "夜视",
        "红外",
        "夜间",
    ];
    // Every hit of the marker must be night/IR-prefixed; a later bare
    // "vision not available" still counts as a real failure admission.
    let m_lower = marker.to_lowercase();
    let mut any = false;
    for (idx, _) in lower.match_indices(&m_lower) {
        any = true;
        let before = &lower[..idx];
        if !prefixes.iter().any(|p| before.ends_with(p)) {
            return false;
        }
    }
    any
}

/// True when the conclusion text admits vision analysis failed / was missing.
#[cfg(test)]
pub(crate) fn conclusion_admits_vision_failure(text: &str) -> bool {
    conclusion_admits_vision_failure_for_path(text, VisionExecutionPath::None)
}

/// Path-aware variant used by the executor after the tool loop.
///
/// When `path` is InlineMultimodal and the conclusion also contains positive
/// inline-analysis evidence, incidental "no vision tool" explanations are
/// treated as commentary rather than failure (Prompt-Echo Trap) — unless an
/// explicit negation phrase is present alongside a failure marker.
///
/// On that same path the conclusion may instead claim the image never
/// arrived ([`INLINE_IMAGE_ABSENCE_MARKERS`]); with the image attached that
/// is the same admission by another name.
pub(crate) fn conclusion_admits_vision_failure_for_path(
    text: &str,
    path: VisionExecutionPath,
) -> bool {
    let lower = text.to_lowercase();
    let claims_image_absent = matches!(path, VisionExecutionPath::InlineMultimodal)
        && INLINE_IMAGE_ABSENCE_MARKERS
            .iter()
            .any(|marker| lower.contains(marker));
    let hit = claims_image_absent
        || VISION_FAILURE_MARKERS.iter().any(|m| {
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
            let has_negation = INLINE_VISION_NEGATION_PHRASES
                .iter()
                .any(|p| lower.contains(&p.to_lowercase()));
            // Negation vetoes the evidence exemption; alone it never admits.
            if !has_negation {
                return false;
            }
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

/// Machine stop_reason written when a run depended on vision tools and every
/// vision call failed (or the conclusion admits vision never ran). Surfaces as
/// [`neomind_storage::ExecutionStatus::Partial`] — undetermined, not Completed.
pub(crate) const VISION_TOOLS_FAILED_STOP_REASON: &str = "vision_tools_failed";

/// True for the host `vision` tool (and extension names that start with it).
pub(crate) fn is_vision_tool_name(name: &str) -> bool {
    name == "vision" || name.starts_with("vision_") || name.starts_with("vision-")
}

/// True when at least one vision tool ran and every vision call failed.
pub(crate) fn vision_tools_all_failed(actions: &[neomind_storage::ActionExecuted]) -> bool {
    let mut saw_vision = false;
    for action in actions {
        if !is_vision_tool_name(&action.target) {
            continue;
        }
        saw_vision = true;
        if action.success {
            return false;
        }
    }
    saw_vision
}

/// True when at least one vision tool call succeeded this run.
pub(crate) fn vision_tools_any_success(actions: &[neomind_storage::ActionExecuted]) -> bool {
    actions
        .iter()
        .any(|a| is_vision_tool_name(&a.target) && a.success)
}

/// ViaVisionTool runs that never confirm a successful vision call must not
/// settle as business Completed (Dongpingshan / NM-388): zero calls used to
/// pass because [`vision_tools_all_failed`] is false on an empty action list.
pub(crate) fn vision_required_unconfirmed(
    path: VisionExecutionPath,
    actions: &[neomind_storage::ActionExecuted],
) -> bool {
    matches!(path, VisionExecutionPath::ViaVisionTool) && !vision_tools_any_success(actions)
}

/// Collector could not prove the triggering report's frame (NM-389). Must
/// demote settle to Partial even when the model invents "现场正常" with no
/// embeddable image (path would otherwise be [`VisionExecutionPath::None`]
/// and skip the ViaVisionTool gate).
pub(crate) fn has_frame_unconfirmed(data: &[DataCollected]) -> bool {
    data.iter()
        .any(|d| d.values.get("_frame_unconfirmed").and_then(|v| v.as_bool()) == Some(true))
}

/// Whether this agent task actually needs a confirmed frame for settle.
///
/// `_frame_unconfirmed` is written at collect time for whole-device triggers;
/// non-vision tasks (plain temperature) must still Complete on their scalar
/// input even when a same-second orphan image was rejected.
pub(crate) fn task_needs_vision(agent: &neomind_storage::AiAgent) -> bool {
    let prompt = agent.user_prompt.to_ascii_lowercase();
    if prompt.contains("image")
        || prompt.contains("vision")
        || prompt.contains("camera")
        || prompt.contains("photo")
        || prompt.contains("frame")
        || agent.user_prompt.contains("图片")
        || agent.user_prompt.contains("看图")
        || agent.user_prompt.contains("摄像头")
        || agent.user_prompt.contains("画面")
        || agent.user_prompt.contains("照片")
    {
        return true;
    }
    if let Some(filter) = agent.schedule.event_filter.as_deref() {
        let lower = filter.to_ascii_lowercase();
        if lower.contains("image") || lower.contains("frame") {
            return true;
        }
    }
    agent.resources.iter().any(|r| {
        let id = r.resource_id.to_ascii_lowercase();
        let name = r.name.to_ascii_lowercase();
        id.contains("image")
            || name.contains("image")
            || id.contains("camera")
            || name.contains("camera")
    })
}

/// Rewrite conclusion / stop_reason / success_rate for a vision-undetermined run.
///
/// Strips a trailing `Verdict: criteria met` so Judgment notify cannot fire on
/// a blind run, and prefixes an undetermined marker so "现场正常" never stands alone.
pub(crate) fn mark_vision_tools_failed(
    decision_process: &mut neomind_storage::DecisionProcess,
    execution_result: &mut neomind_storage::ExecutionResult,
) {
    decision_process.stop_reason = VISION_TOOLS_FAILED_STOP_REASON.to_string();
    decision_process.confidence = None;
    let body = neutralize_criteria_met_verdict(&decision_process.conclusion);
    decision_process.conclusion = format!("未判定 / Undetermined (vision tools failed): {body}");
    execution_result.success_rate = 0.0;
    if execution_result.summary.is_empty()
        || !execution_result.summary.to_lowercase().contains("vision")
    {
        execution_result.summary = format!(
            "Vision tools failed — undetermined: {}",
            truncate_summary(&body, 200)
        );
    }
}

fn neutralize_criteria_met_verdict(text: &str) -> String {
    let trimmed = text.trim_end();
    let last_line_start = trimmed.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let last = trimmed[last_line_start..].trim();
    let lower = last.to_ascii_lowercase();
    let is_met = lower == "verdict: criteria met" || lower == "verdict: criteria met.";
    if !is_met {
        return text.to_string();
    }
    let prefix = trimmed[..last_line_start].trim_end();
    if prefix.is_empty() {
        "Verdict: insufficient evidence".to_string()
    } else {
        format!("{prefix}\nVerdict: insufficient evidence")
    }
}

fn truncate_summary(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
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
        for marker in VISION_FAILURE_MARKERS
            .iter()
            .chain(INLINE_IMAGE_ABSENCE_MARKERS)
        {
            assert!(
                !g.to_lowercase().contains(&marker.to_lowercase()),
                "guidance must not teach the failure phrase {marker:?}"
            );
        }
        // The mission text may still speak of an image "path"; the guidance
        // has to close that door without using a blacklisted phrase.
        assert!(g.contains("no file path or URL to look up"), "{g}");
    }

    /// With the image attached, claiming it never arrived is the same
    /// admission as "no vision tool" — and used to land Completed.
    #[test]
    fn inline_image_absence_claims_admit_failure() {
        // Field wording from T4 (2026-11-21 .. 11-23).
        let cases = [
            "我读取到最新的设备上报数据，但图片路径未提供，因此无法执行图像分析步骤。",
            "无图像数据可用。设备 NE301PIR-20035E 的 image_data 未提供。",
            "无图像数据路径。设备 NE301PIR-200366-东坪山未记录 image_data 字段。",
            r#"{"device_id": "200360", "image_path": "/images/unknown.jpg", "detections": "person present", "赌博行为": "无"}"#,
            r#"{"image_path": "unknown","detections": null}"#,
            "没有接收任何图片数据或检测结果。",
            "The image path was not provided, so the frame was not analyzed.",
        ];
        for text in cases {
            assert!(
                conclusion_admits_vision_failure_for_path(
                    text,
                    VisionExecutionPath::InlineMultimodal
                ),
                "an absence claim with the image attached must admit failure: {text}"
            );
        }
        // Without an attached image the same sentence is simply true.
        for path in [
            VisionExecutionPath::None,
            VisionExecutionPath::ViaVisionTool,
        ] {
            assert!(
                !conclusion_admits_vision_failure_for_path("图片路径未提供。", path),
                "absence is only a failure when the image is attached"
            );
        }
    }

    /// The gate must not turn an honest analysis into a failure: the
    /// mission's own step-1 wording, and device detections this firmware
    /// really does not report, are not absence claims.
    #[test]
    fn inline_absence_markers_spare_honest_analyses() {
        let cases = [
            "第1步，读取本次上报的 image_data（图片路径）和 ai_result.ai_result.detections。\
             第2步，画面为山林步道，两人坐在牌桌旁打麻将。",
            "设备端检测结果未提供。画面：碎石空地，无人，无赌博行为。",
            "夜间帐篷场景，无明确人物；结论：无赌博/聚集活动迹象",
            "无法获取图片中远处物体的细节，近处可见一人站立，未见赌博元素。",
            "图片：<image_data 路径>。已发送告警：两人打麻将。",
        ];
        for text in cases {
            assert!(
                !conclusion_admits_vision_failure_for_path(
                    text,
                    VisionExecutionPath::InlineMultimodal
                ),
                "honest analysis must not be refused: {text}"
            );
        }
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
        let echo = "当前环境没有 vision 工具，已直接分析附带图像：走廊空旷，无赌博行为。";
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
    fn inline_bare_noun_with_negation_still_admits_failure() {
        let cases = [
            "Vision tool not available to analyze the attached image.",
            "没有 vision 工具，无法分析附带图像。",
            "Cannot use vision (inline image unavailable).",
            "无法分析附带画面，视觉工具不可用。",
        ];
        for text in cases {
            assert!(
                conclusion_admits_vision_failure_for_path(
                    text,
                    VisionExecutionPath::InlineMultimodal
                ),
                "negation / bare-noun must admit failure: {text}"
            );
        }
    }

    #[test]
    fn night_vision_prefix_checks_all_marker_hits() {
        // First hit is night-prefixed; later bare marker must still admit failure.
        assert!(conclusion_admits_vision_failure(
            "红外视觉不可用，且当前视觉不可用无法分析"
        ));
        // Hyphenated night-/ir- prefixes must not false-positive.
        assert!(!conclusion_admits_vision_failure(
            "Night-vision not available — frame is pitch dark"
        ));
        assert!(!conclusion_admits_vision_failure(
            "IR-vision not available on this camera"
        ));
    }

    #[test]
    fn honest_dark_or_blur_report_is_not_vision_failure() {
        // Negation alone (no VISION_FAILURE_MARKERS) must not trip NM-331.
        let honest = [
            "画面过暗，无法识别是否有人",
            "Night-vision not available — pitch dark, unable to analyze",
            "画面模糊，无法查看细节，未见赌博行为",
        ];
        for text in honest {
            assert!(
                !conclusion_admits_vision_failure(text),
                "honest dark/blur report must not admit vision failure: {text}"
            );
            assert!(
                !conclusion_admits_vision_failure_for_path(
                    text,
                    VisionExecutionPath::InlineMultimodal
                ),
                "inline path must also spare honest dark/blur report: {text}"
            );
        }
    }

    #[test]
    fn nm331_tool_missing_admissions_still_fail() {
        let cases = [
            "没有 vision 工具，无法分析附带图像",
            "Vision tool not available to analyze the attached image",
        ];
        for text in cases {
            assert!(
                conclusion_admits_vision_failure(text),
                "NM-331 zero-regression: {text}"
            );
        }
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

    #[test]
    fn vision_tools_all_failed_requires_vision_actions() {
        let echo_ok = neomind_storage::ActionExecuted {
            action_type: "tool_call".into(),
            description: "echo".into(),
            target: "echo".into(),
            parameters: serde_json::Value::Null,
            success: true,
            result: None,
        };
        assert!(!vision_tools_all_failed(std::slice::from_ref(&echo_ok)));
        assert!(!vision_tools_all_failed(&[]));

        let vision_fail = neomind_storage::ActionExecuted {
            action_type: "tool_call".into(),
            description: "vision".into(),
            target: "vision".into(),
            parameters: serde_json::Value::Null,
            success: false,
            result: Some("timeout".into()),
        };
        assert!(vision_tools_all_failed(std::slice::from_ref(&vision_fail)));
        assert!(vision_tools_all_failed(&[
            vision_fail.clone(),
            echo_ok.clone()
        ]));

        let vision_ok = neomind_storage::ActionExecuted {
            success: true,
            ..vision_fail.clone()
        };
        assert!(!vision_tools_all_failed(&[vision_ok.clone(), vision_fail]));
        assert!(vision_tools_any_success(std::slice::from_ref(&vision_ok)));
        assert!(!vision_tools_any_success(&[]));
        assert!(!vision_tools_any_success(std::slice::from_ref(&echo_ok)));

        // NM-388: ViaVisionTool + zero/non-vision success → unconfirmed.
        assert!(vision_required_unconfirmed(
            VisionExecutionPath::ViaVisionTool,
            &[]
        ));
        assert!(vision_required_unconfirmed(
            VisionExecutionPath::ViaVisionTool,
            std::slice::from_ref(&echo_ok)
        ));
        assert!(!vision_required_unconfirmed(
            VisionExecutionPath::ViaVisionTool,
            std::slice::from_ref(&vision_ok)
        ));
        assert!(!vision_required_unconfirmed(
            VisionExecutionPath::InlineMultimodal,
            &[]
        ));

        // NM-389: collector marker must be readable for settle demotion.
        let unconfirmed = neomind_storage::DataCollected {
            source: "cam".into(),
            data_type: "frame_status".into(),
            timestamp: 1,
            values: serde_json::json!({ "_frame_unconfirmed": true }),
        };
        let ok = neomind_storage::DataCollected {
            source: "cam".into(),
            data_type: "temperature".into(),
            timestamp: 1,
            values: serde_json::json!({ "value": 21.5 }),
        };
        assert!(has_frame_unconfirmed(&[unconfirmed]));
        assert!(!has_frame_unconfirmed(&[ok]));
    }

    #[test]
    fn mark_vision_tools_failed_rewrites_conclusion_and_verdict() {
        let mut dp = neomind_storage::DecisionProcess {
            situation_analysis: String::new(),
            data_collected: vec![],
            reasoning_steps: vec![],
            decisions: vec![],
            conclusion: "现场正常，无异常。\nVerdict: criteria met".into(),
            confidence: Some(0.9),
            stop_reason: "natural-completion".into(),
            tokens_used: 0,
        };
        let mut er = neomind_storage::ExecutionResult {
            actions_executed: vec![],
            report: None,
            notifications_sent: vec![],
            summary: String::new(),
            success_rate: 1.0,
        };
        mark_vision_tools_failed(&mut dp, &mut er);
        assert_eq!(dp.stop_reason, VISION_TOOLS_FAILED_STOP_REASON);
        assert!(dp.confidence.is_none());
        assert!(dp.conclusion.starts_with("未判定 / Undetermined"));
        assert!(dp.conclusion.contains("Verdict: insufficient evidence"));
        assert!(!dp.conclusion.contains("criteria met"));
        assert_eq!(er.success_rate, 0.0);
        assert!(er.summary.to_lowercase().contains("vision"));
    }
}
