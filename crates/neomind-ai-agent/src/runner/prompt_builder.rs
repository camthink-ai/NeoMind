//! Layered prompt construction from evidence with media extraction & size guards.

use std::collections::HashSet;

use serde_json::Value;

use crate::model::{CallerInput, EvidenceDetail, EvidenceItem, EvidenceQuality, EvidenceReport};
use crate::ports::LlmImage;

/// Maximum number of multimodal images attached to a single LLM turn.
const MAX_PROMPT_IMAGES: usize = 4;
/// Maximum characters kept for any single non-image string field inside evidence JSON.
const MAX_FIELD_STRING_CHARS: usize = 4_000;
/// Maximum characters kept for one formatted evidence item body.
const MAX_EVIDENCE_ITEM_CHARS: usize = 8_000;
/// Maximum characters kept for the overall evidence prompt text.
const MAX_TOTAL_PROMPT_CHARS: usize = 32_000;
/// Minimum length to treat an unbroken base64-only non-image string as binary bloat.
const MIN_GENERIC_BASE64_BLOAT_LEN: usize = 2_048;

pub fn build_evidence_prompt(user_prompt: &str, evidence: &EvidenceReport) -> String {
    build_evidence_prompt_with_media(user_prompt, evidence).0
}

/// Build the user prompt text AND extract multimodal image attachments so raw
/// base64 payloads never enter the text token stream.
pub fn build_evidence_prompt_with_media(
    user_prompt: &str,
    evidence: &EvidenceReport,
) -> (String, Vec<LlmImage>) {
    let mut images = Vec::new();
    let mut seen_images = HashSet::new();

    let sanitized_user_prompt =
        sanitize_text_for_prompt(user_prompt, "prompt", &mut images, &mut seen_images);
    let mut parts = vec![sanitized_user_prompt, String::new(), "## Evidence".into()];
    for item in &evidence.items {
        parts.push(format_item(item, &mut images, &mut seen_images));
    }
    if !evidence.missing_keys.is_empty() {
        parts.push(format!(
            "\n## Missing\n{}",
            evidence.missing_keys.join(", ")
        ));
        parts.push(
            "For any conclusion that depends on missing keys, state 依据不足 / insufficient evidence."
                .into(),
        );
    }
    let joined = parts.join("\n");
    let capped = truncate_chars(&joined, MAX_TOTAL_PROMPT_CHARS);
    (capped, images)
}

/// Append this-turn caller input to the standing user prompt (run-local only).
pub fn prompt_with_caller_input(user_prompt: &str, caller: &CallerInput) -> String {
    let section = format_caller_section(caller);
    if section.is_empty() {
        return user_prompt.to_string();
    }
    format!("{user_prompt}\n{section}")
}

/// Run-local agent memory injected from durable journal & knowledge files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryContext {
    /// Recent execution journal summaries (oldest → newest).
    pub journal_entries: Vec<String>,
    /// Agent knowledge files as `(name, description, content)`.
    pub knowledge_files: Vec<(String, String, String)>,
}

impl MemoryContext {
    pub fn is_empty(&self) -> bool {
        self.journal_entries.iter().all(|s| s.trim().is_empty())
            && self
                .knowledge_files
                .iter()
                .all(|(_, _, content)| content.trim().is_empty())
    }
}

/// Append recent execution journal and agent knowledge files to the standing
/// prompt (run-local only; never mutates the persisted `AgentSpec`).
///
/// Explicitly frames memory as historical context so the model uses it for
/// trend comparison and standing criteria without fabricating missing current
/// readings.
pub fn prompt_with_memory_context(user_prompt: &str, memory: &MemoryContext) -> String {
    if memory.is_empty() {
        return user_prompt.to_string();
    }

    let mut sections: Vec<String> = Vec::new();
    let journal: Vec<&str> = memory
        .journal_entries
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !journal.is_empty() {
        let lines = journal
            .into_iter()
            .map(|e| format!("- {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Recent Execution Journal\n{lines}"));
    }

    let files: Vec<(&str, &str, &str)> = memory
        .knowledge_files
        .iter()
        .filter_map(|(name, desc, content)| {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((name.trim(), desc.trim(), trimmed))
            }
        })
        .collect();
    if !files.is_empty() {
        let mut blocks = Vec::with_capacity(files.len());
        for (name, desc, content) in files {
            if desc.is_empty() {
                blocks.push(format!("### {name}\n{content}"));
            } else {
                blocks.push(format!("### {name} ({desc})\n{content}"));
            }
        }
        sections.push(format!("## Agent Knowledge Files\n{}", blocks.join("\n\n")));
    }

    if sections.is_empty() {
        return user_prompt.to_string();
    }

    format!(
        "{user_prompt}\n\n## Agent Memory\nHistorical context from prior runs (for trend comparison and standards only — never fabricate missing current readings from memory).\n\n{}\n",
        sections.join("\n\n")
    )
}

/// Append persisted operator notes (`user_messages`) to the standing prompt (run-local only).
pub fn prompt_with_operator_notes(user_prompt: &str, notes: &[String]) -> String {
    let non_empty: Vec<&str> = notes
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if non_empty.is_empty() {
        return user_prompt.to_string();
    }
    let body = non_empty
        .into_iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{user_prompt}\n\n## Operator Notes\n{body}\n")
}

fn format_caller_section(caller: &CallerInput) -> String {
    if caller.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(ref source) = caller.source {
        if !source.is_empty() {
            parts.push(format!("来源/Source: {source}"));
        }
    }
    if let Some(ref content) = caller.content {
        if !content.trim().is_empty() {
            parts.push(format!("内容/Content: {content}"));
        }
    }
    if let Some(ref data) = caller.data {
        if !data.is_null() {
            let mut ignored_imgs = Vec::new();
            let mut seen = HashSet::new();
            let clean = sanitize_json_value(data, "caller", false, &mut ignored_imgs, &mut seen);
            let data_str = serde_json::to_string_pretty(&clean).unwrap_or_default();
            if !data_str.is_empty() && data_str != "null" && data_str != "{}" {
                parts.push(format!(
                    "附加数据/Data:\n{}",
                    truncate_chars(&data_str, MAX_EVIDENCE_ITEM_CHARS)
                ));
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "\n## Caller Input (invoked by external request)\n{}\n",
            parts.join("\n")
        )
    }
}

fn format_item(
    item: &EvidenceItem,
    images: &mut Vec<LlmImage>,
    seen_images: &mut HashSet<String>,
) -> String {
    let quality = match &item.quality {
        EvidenceQuality::Fresh => "fresh",
        EvidenceQuality::Stale => "stale",
        EvidenceQuality::Missing => "missing",
        EvidenceQuality::Error { reason } => return format!("- {}: ERROR ({reason})", item.key),
    };
    let window = item
        .valid_window
        .map(|(a, b)| format!(" window=[{a},{b}]"))
        .unwrap_or_default();
    let body = match &item.detail {
        Some(EvidenceDetail::Structured { value }) => {
            let clean = sanitize_json_value(value, &item.key, false, images, seen_images);
            truncate_chars(&clean.to_string(), MAX_EVIDENCE_ITEM_CHARS)
        }
        Some(EvidenceDetail::Text { text }) => {
            let clean = sanitize_text_for_prompt(text, &item.key, images, seen_images);
            truncate_chars(&clean, MAX_EVIDENCE_ITEM_CHARS)
        }
        Some(EvidenceDetail::TimeSeries { points, value }) => match value {
            Some(v) => {
                let clean = sanitize_json_value(v, &item.key, false, images, seen_images);
                let serialized = truncate_chars(&clean.to_string(), MAX_EVIDENCE_ITEM_CHARS);
                format!("{points} points {serialized}")
            }
            None => format!("{points} points"),
        },
        Some(EvidenceDetail::Vision {
            image_source_field,
            vision_path,
        }) => {
            let raw_path = vision_path
                .split(';')
                .next()
                .map(str::trim)
                .unwrap_or(vision_path.as_str());
            let display_path = if let Some((mime, b64)) = parse_data_image_url(raw_path) {
                let attached = push_image(
                    images,
                    seen_images,
                    b64,
                    Some(mime.clone()),
                    Some(format!("{}.{}", item.key, image_source_field)),
                );
                let kb = raw_path.len() / 1024;
                if attached {
                    format!("[image attached: {mime}, {kb}KB]")
                } else {
                    format!("[image omitted: {mime}, {kb}KB]")
                }
            } else {
                if raw_path.starts_with("/api/images/")
                    || raw_path.starts_with("http://")
                    || raw_path.starts_with("https://")
                {
                    push_image(
                        images,
                        seen_images,
                        raw_path.to_string(),
                        None,
                        Some(format!("{}.{}", item.key, image_source_field)),
                    );
                }
                truncate_chars(vision_path, 512)
            };
            format!("vision {image_source_field} via {display_path}")
        }
        None => "null".into(),
    };
    format!(
        "- {} ({quality}) source={} proof={}{window}: {body}",
        item.key, item.source_id, item.binding_proof
    )
}

fn sanitize_json_value(
    value: &Value,
    label: &str,
    in_image_field: bool,
    images: &mut Vec<LlmImage>,
    seen_images: &mut HashSet<String>,
) -> Value {
    match value {
        Value::String(s) => Value::String(sanitize_string_field(
            s,
            label,
            in_image_field,
            images,
            seen_images,
        )),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .enumerate()
                .map(|(i, item)| {
                    sanitize_json_value(
                        item,
                        &format!("{label}[{i}]"),
                        in_image_field,
                        images,
                        seen_images,
                    )
                })
                .collect(),
        ),
        Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                let child_is_img = in_image_field || key_looks_like_image(k);
                let child_label = if label.is_empty() {
                    k.clone()
                } else {
                    format!("{label}.{k}")
                };
                out.insert(
                    k.clone(),
                    sanitize_json_value(v, &child_label, child_is_img, images, seen_images),
                );
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn sanitize_string_field(
    s: &str,
    label: &str,
    in_image_field: bool,
    images: &mut Vec<LlmImage>,
    seen_images: &mut HashSet<String>,
) -> String {
    let trimmed = s.trim();

    // 1. data:image/<mime>;base64,<data>
    if let Some((mime, b64)) = parse_data_image_url(trimmed) {
        let kb = (b64.len() / 1024).max(1);
        let attached = push_image(
            images,
            seen_images,
            b64,
            Some(mime.clone()),
            Some(label.to_string()),
        );
        return if attached {
            format!("[image attached: {mime}, ~{kb}KB]")
        } else {
            format!("[image omitted: {mime}, ~{kb}KB]")
        };
    }

    // 2. Platform internal image URL (/api/images/...)
    if trimmed.starts_with("/api/images/") {
        push_image(
            images,
            seen_images,
            trimmed.to_string(),
            None,
            Some(label.to_string()),
        );
        return trimmed.to_string();
    }

    // 3. Stringified JSON payload (e.g. `_raw` MQTT/webhook telemetry containing nested base64)
    if trimmed.len() > 256
        && ((trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']')))
    {
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            let clean = sanitize_json_value(&parsed, label, in_image_field, images, seen_images);
            return truncate_chars(&clean.to_string(), MAX_FIELD_STRING_CHARS);
        }
    }

    // 4. Raw base64 image payload (without data:image/ prefix)
    if trimmed.len() >= 128 {
        if let Some(mime) = detect_raw_base64_image_mime(trimmed) {
            let clean_b64: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
            if looks_like_base64(&clean_b64) {
                let kb = (clean_b64.len() / 1024).max(1);
                let attached = push_image(
                    images,
                    seen_images,
                    clean_b64,
                    Some(mime.to_string()),
                    Some(label.to_string()),
                );
                return if attached {
                    format!("[image attached: {mime}, ~{kb}KB]")
                } else {
                    format!("[image omitted: {mime}, ~{kb}KB]")
                };
            }
        }
    }

    // 5. Image-named field holding raw base64 >= 512 chars
    if in_image_field && trimmed.len() >= 512 {
        let clean_b64: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if looks_like_base64(&clean_b64) {
            let kb = (clean_b64.len() / 1024).max(1);
            let attached = push_image(
                images,
                seen_images,
                clean_b64,
                Some("image/jpeg".to_string()),
                Some(label.to_string()),
            );
            return if attached {
                format!("[image attached: image/jpeg, ~{kb}KB]")
            } else {
                format!("[image omitted: image/jpeg, ~{kb}KB]")
            };
        }
    }

    // 6. Any huge unbroken base64 / binary blob under an arbitrary key
    if trimmed.len() >= MIN_GENERIC_BASE64_BLOAT_LEN {
        let clean_b64: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if clean_b64.len() >= MIN_GENERIC_BASE64_BLOAT_LEN && looks_like_base64(&clean_b64) {
            let kb = (clean_b64.len() / 1024).max(1);
            return format!("[binary/base64 omitted: ~{kb}KB]");
        }
    }

    // 7. Embedded data:image/ URLs inside free-form text + length cap
    let clean_text = sanitize_text_for_prompt(s, label, images, seen_images);
    truncate_chars(&clean_text, MAX_FIELD_STRING_CHARS)
}

/// Replace any inline `data:image/<mime>;base64,<b64>` occurrences inside free-form text.
fn sanitize_text_for_prompt(
    text: &str,
    label: &str,
    images: &mut Vec<LlmImage>,
    seen_images: &mut HashSet<String>,
) -> String {
    if !text.contains("data:image/") && !text.contains("DATA:IMAGE/") {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len().min(4096));
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find("data:image/") {
        let start = cursor + rel;
        out.push_str(&text[cursor..start]);
        let rest = &text[start..];
        let end_rel = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '}' | '<' | '>')
            })
            .unwrap_or(rest.len());
        let token = &rest[..end_rel];
        if let Some((mime, b64)) = parse_data_image_url(token) {
            let kb = (b64.len() / 1024).max(1);
            let attached = push_image(
                images,
                seen_images,
                b64,
                Some(mime.clone()),
                Some(label.to_string()),
            );
            if attached {
                out.push_str(&format!("[image attached: {mime}, ~{kb}KB]"));
            } else {
                out.push_str(&format!("[image omitted: {mime}, ~{kb}KB]"));
            }
        } else {
            out.push_str(token);
        }
        cursor = start + end_rel;
    }
    out.push_str(&text[cursor..]);
    out
}

fn parse_data_image_url(s: &str) -> Option<(String, String)> {
    let prefix = s.get(..11)?;
    if !prefix.eq_ignore_ascii_case("data:image/") {
        return None;
    }
    let comma = s.find(',')?;
    let header = &s[5..comma];
    let mime = header.split(';').next()?.trim().to_ascii_lowercase();
    if !mime.starts_with("image/") {
        return None;
    }
    let raw_b64 = &s[comma + 1..];
    let clean_b64: String = raw_b64.chars().filter(|c| !c.is_whitespace()).collect();
    if clean_b64.len() < 16 {
        return None;
    }
    Some((mime, clean_b64))
}

fn detect_raw_base64_image_mime(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("/9j/") {
        Some("image/jpeg")
    } else if trimmed.starts_with("iVBORw0KGgo") {
        Some("image/png")
    } else if trimmed.starts_with("UklGR") {
        Some("image/webp")
    } else if trimmed.starts_with("R0lGOD") {
        Some("image/gif")
    } else {
        None
    }
}

fn looks_like_base64(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
}

fn key_looks_like_image(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("image")
        || lower.contains("frame")
        || lower.contains("snapshot")
        || lower == "photo"
        || lower == "img"
        || lower.ends_with("_img")
        || lower.ends_with("_b64")
        || lower.ends_with("base64")
}

fn push_image(
    images: &mut Vec<LlmImage>,
    seen: &mut HashSet<String>,
    data: String,
    mime_type: Option<String>,
    label: Option<String>,
) -> bool {
    // Deduplicate by prefix + suffix + length (fast even for 500KB base64).
    let sig = if data.len() > 128 {
        format!(
            "{}:{}..{}",
            data.len(),
            &data[..64],
            &data[data.len() - 32..]
        )
    } else {
        data.clone()
    };
    if !seen.insert(sig) {
        // Already attached from another metric (e.g. `image` and `_raw`).
        return true;
    }
    if images.len() >= MAX_PROMPT_IMAGES {
        return false;
    }
    images.push(LlmImage {
        data,
        mime_type,
        label,
    });
    true
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let kept: String = s.chars().take(max_chars).collect();
    let dropped = total - max_chars;
    format!("{kept}... [truncated {dropped} chars]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_question_lands_in_prompt() {
        let prompt = prompt_with_caller_input(
            "Standing mission.",
            &CallerInput {
                content: Some("Why is temp high?".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        assert!(prompt.contains("Standing mission."));
        assert!(prompt.contains("## Caller Input (invoked by external request)"));
        assert!(prompt.contains("Why is temp high?"));
        assert!(prompt.contains("来源/Source: api"));
    }

    #[test]
    fn empty_caller_leaves_prompt_unchanged() {
        let base = "Standing mission.";
        let prompt = prompt_with_caller_input(base, &CallerInput::default());
        assert_eq!(prompt, base);
    }

    #[test]
    fn two_questions_produce_distinct_prompts() {
        let q1 = prompt_with_caller_input(
            "Mission",
            &CallerInput {
                content: Some("Question A".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        let q2 = prompt_with_caller_input(
            "Mission",
            &CallerInput {
                content: Some("Question B".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        assert!(q1.contains("Question A"));
        assert!(!q1.contains("Question B"));
        assert!(q2.contains("Question B"));
        assert!(!q2.contains("Question A"));
    }

    #[test]
    fn memory_context_injects_journal_and_knowledge_files() {
        let mem = MemoryContext {
            journal_entries: vec![
                "[2026-11-26 08:00 UTC] [OK] Temp 22.1C normal".into(),
                "[2026-11-26 09:00 UTC] [OK] Temp 28.4C rising".into(),
            ],
            knowledge_files: vec![(
                "thresholds.md".into(),
                "Cold room limits".into(),
                "Alert above 30C.".into(),
            )],
        };
        let prompt = prompt_with_memory_context("Inspect cold room.", &mem);
        assert!(prompt.contains("Inspect cold room."));
        assert!(prompt.contains("## Agent Memory"));
        assert!(prompt.contains("## Recent Execution Journal"));
        assert!(prompt.contains("Temp 28.4C rising"));
        assert!(prompt.contains("## Agent Knowledge Files"));
        assert!(prompt.contains("### thresholds.md (Cold room limits)"));
        assert!(prompt.contains("Alert above 30C."));
        assert!(prompt.contains("never fabricate missing current readings from memory"));
    }

    #[test]
    fn empty_memory_context_leaves_prompt_unchanged() {
        let base = "Inspect cold room.";
        assert_eq!(
            prompt_with_memory_context(base, &MemoryContext::default()),
            base
        );
    }

    #[test]
    fn structured_base64_image_is_extracted_and_not_dumped_into_prompt_text() {
        let big_jpeg_b64 = format!("/9j/{}", "A".repeat(400_000));
        let data_url = format!("data:image/jpeg;base64,{big_jpeg_b64}");
        let raw_envelope = serde_json::json!({
            "image": data_url,
            "battery": 95,
        })
        .to_string();
        let evidence = EvidenceReport {
            items: vec![EvidenceItem {
                key: "device_9999".into(),
                source_id: "device:9999".into(),
                result_type: crate::model::InputModality::StructuredJson,
                observed_at: Some(1_700_000_000_000),
                valid_window: None,
                fetched_at: 1_700_000_000_000,
                quality: EvidenceQuality::Fresh,
                binding_proof: "device:9999:*".into(),
                detail: Some(EvidenceDetail::Structured {
                    value: serde_json::json!({
                        "image": { "timestamp": 1700000000, "value": data_url },
                        "_raw": { "timestamp": 1700000000, "value": raw_envelope },
                        "battery": { "timestamp": 1700000000, "value": 95 }
                    }),
                }),
            }],
            missing_keys: vec![],
            skip_reason: None,
        };

        let (prompt, images) = build_evidence_prompt_with_media("Inspect camera.", &evidence);
        assert!(
            prompt.len() < 4_000,
            "prompt must stay tiny instead of dumping 400KB base64, got {} bytes",
            prompt.len()
        );
        assert!(!prompt.contains(&"A".repeat(200)));
        assert!(prompt.contains("[image attached: image/jpeg"));
        assert!(prompt.contains("95"));
        // Deduplicated between `image` and `_raw`
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(images[0].data.len(), big_jpeg_b64.len());
    }

    #[test]
    fn internal_image_url_is_collected_for_multimodal_attachment() {
        let evidence = EvidenceReport {
            items: vec![EvidenceItem {
                key: "cam".into(),
                source_id: "device:9999".into(),
                result_type: crate::model::InputModality::StructuredJson,
                observed_at: Some(1_700_000_000_000),
                valid_window: None,
                fetched_at: 1_700_000_000_000,
                quality: EvidenceQuality::Fresh,
                binding_proof: "device:9999:*".into(),
                detail: Some(EvidenceDetail::Structured {
                    value: serde_json::json!({
                        "image": { "timestamp": 1700000000, "value": "/api/images/9999/image/1700000000_abc.jpg" }
                    }),
                }),
            }],
            missing_keys: vec![],
            skip_reason: None,
        };
        let (prompt, images) = build_evidence_prompt_with_media("Check cam.", &evidence);
        assert!(prompt.contains("/api/images/9999/image/1700000000_abc.jpg"));
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime_type, None);
        assert_eq!(images[0].data, "/api/images/9999/image/1700000000_abc.jpg");
    }
}
