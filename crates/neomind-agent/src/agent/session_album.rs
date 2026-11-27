//! Session image album: durable `/api/images/…` handles + optional captions.
//!
//! History / LLM-visible context always stores relative paths (NM-314). Absolute
//! advertise URLs (`NEOMIND_SERVER_URL` + path) are outbound-boundary only.

use std::collections::HashMap;

use super::types::{AgentMessage, AgentMessageImage};

/// Default album rows kept in the prompt block (Spark-class windows).
/// Caps the entire block (paths + optional user text + captions), not captions alone.
pub const ALBUM_ENTRY_KEEP_DEFAULT: usize = 8;
/// Tighter album keep for ≤16K context windows.
pub const ALBUM_ENTRY_KEEP_SMALL: usize = 4;

/// Backward-compatible aliases for caption-window naming.
pub const ALBUM_CAPTION_KEEP_DEFAULT: usize = ALBUM_ENTRY_KEEP_DEFAULT;
/// See [`ALBUM_ENTRY_KEEP_SMALL`].
pub const ALBUM_CAPTION_KEEP_SMALL: usize = ALBUM_ENTRY_KEEP_SMALL;

/// One durable chat/session image, numbered in first-seen order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionImageEntry {
    /// 1-based session index (`img_1` …).
    pub index: u32,
    /// Relative handle `/api/images/…` only.
    pub path: String,
    pub caption: Option<String>,
    pub source_user_text: Option<String>,
}

/// Return the durable relative path when `data` is already an `/api/images/` ref.
pub fn durable_image_path(data: &str) -> Option<&str> {
    let trimmed = data.trim();
    if trimmed.starts_with("/api/images/")
        && !trimmed[12..].contains('/')
        && !trimmed.contains("..")
    {
        Some(trimmed)
    } else {
        None
    }
}

/// Format a single attached-handle line for history / strip injection.
pub fn format_attached_handle(index: Option<u32>, path: &str) -> String {
    match index {
        Some(i) => format!("[attached img_{i}: {path}]"),
        None => format!("[attached: {path}]"),
    }
}

/// True when `content` already mentions this path as an attached handle.
pub fn content_has_attached_path(content: &str, path: &str) -> bool {
    content.contains(path)
}

/// Append missing durable handles to user content (idempotent).
pub fn append_attached_handles(
    content: &str,
    paths: impl IntoIterator<Item = (Option<u32>, String)>,
) -> String {
    let mut out = content.to_string();
    for (index, path) in paths {
        if path.is_empty() || content_has_attached_path(&out, &path) {
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format_attached_handle(index, &path));
    }
    out
}

/// Build album from conversation memory (first-seen path order, deduped).
///
/// Includes both user-uploaded images and durable `/api/images/...` outputs
/// produced by assistant tool calls (e.g. `image_edit`) or persisted image
/// attachments, so follow-up turns ("圈出图片中的钢筋" after rotating) see
/// the newly edited image as the latest entry instead of only the original.
pub fn build_session_album(messages: &[AgentMessage]) -> Vec<SessionImageEntry> {
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut entries: Vec<SessionImageEntry> = Vec::new();

    let mut push_path = |raw_path: &str, caption: Option<String>, source_text: Option<String>| {
        let Some(path) = durable_image_path(raw_path) else {
            return;
        };
        if let Some(&idx) = by_path.get(path) {
            if entries[idx].caption.is_none() {
                if let Some(cap) = caption.filter(|c| !c.trim().is_empty()) {
                    entries[idx].caption = Some(cap);
                }
            }
            return;
        }
        let index = (entries.len() as u32) + 1;
        by_path.insert(path.to_string(), entries.len());
        entries.push(SessionImageEntry {
            index,
            path: path.to_string(),
            caption: caption.filter(|c| !c.trim().is_empty()),
            source_user_text: source_text,
        });
    };

    for msg in messages {
        if msg.role == "user" {
            let Some(images) = msg.images.as_ref() else {
                continue;
            };
            let source = {
                let t = msg.content.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.chars().take(80).collect::<String>())
                }
            };
            for image in images {
                push_path(&image.data, image.caption.clone(), source.clone());
            }
        } else if msg.role == "assistant" {
            // 1. Persisted `image` attachments on the assistant reply.
            if let Some(attachments) = msg.attachments.as_ref() {
                for att in attachments {
                    if att.get("kind").and_then(|v| v.as_str()) == Some("image") {
                        if let Some(url) = att
                            .get("imageUrl")
                            .or_else(|| att.get("image_url"))
                            .and_then(|v| v.as_str())
                        {
                            let title = att
                                .get("title")
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            push_path(url, None, title.or_else(|| Some("generated image".into())));
                        }
                    }
                }
            }
            // 2. Durable `/api/images/...` outputs from `image_edit` tool calls
            //    (covers in-memory history before attachments are hydrated).
            if let Some(calls) = msg.tool_calls.as_ref() {
                for tc in calls {
                    if tc.name != "image_edit" {
                        continue;
                    }
                    if let Some(res) = tc.result.as_ref() {
                        let parsed = match res {
                            serde_json::Value::String(s) => {
                                serde_json::from_str::<serde_json::Value>(s).ok()
                            }
                            other => Some(other.clone()),
                        };
                        if let Some(val) = parsed {
                            let url = val
                                .get("url")
                                .or_else(|| val.get("data").and_then(|d| d.get("url")))
                                .and_then(|v| v.as_str());
                            if let Some(url) = url {
                                push_path(url, None, Some("edited image".into()));
                            }
                        }
                    }
                }
            }
        }
    }
    entries
}

/// Look up album index for a durable path.
pub fn album_index_for_path(album: &[SessionImageEntry], path: &str) -> Option<u32> {
    album.iter().find(|e| e.path == path).map(|e| e.index)
}

/// Resolve `img_N` / bare path to a durable `/api/images/…` handle.
pub fn resolve_album_ref<'a>(
    album: &'a [SessionImageEntry],
    token: &str,
) -> Option<&'a SessionImageEntry> {
    let t = token.trim();
    if let Some(path) = durable_image_path(t) {
        return album.iter().find(|e| e.path == path);
    }
    let n = t
        .strip_prefix("img_")
        .or_else(|| t.strip_prefix("IMG_"))
        .and_then(|s| s.parse::<u32>().ok())?;
    album.iter().find(|e| e.index == n)
}

/// Most recent album entry (for "this image" defaults).
pub fn latest_album_entry(album: &[SessionImageEntry]) -> Option<&SessionImageEntry> {
    album.last()
}

fn album_entry_keep_for_context(max_context: Option<usize>) -> usize {
    match max_context {
        Some(m) if m <= 16_384 => ALBUM_ENTRY_KEEP_SMALL,
        _ => ALBUM_ENTRY_KEEP_DEFAULT,
    }
}

/// Prompt block injected into the system prompt (relative paths only).
///
/// Budgets the **entire** album block: only the most recent `keep` entries are
/// listed (path + optional ≤80-char user text + caption). Older images remain
/// addressable as `img_N` / path if the model already saw them earlier in the turn.
pub fn format_album_prompt_block(
    album: &[SessionImageEntry],
    max_context: Option<usize>,
) -> Option<String> {
    if album.is_empty() {
        return None;
    }
    let keep = album_entry_keep_for_context(max_context);
    let start = album.len().saturating_sub(keep);
    let omitted = start;
    let mut out = String::from(
        "## Session images (durable refs — pass these paths to vision / OCR / image_edit)\n\
         Pixels may be omitted on text-only models; paths remain valid. Do not invent absolute HTTP URLs. In shell, /api/images/<path> maps to $NEOMIND_IMAGES_DIR/<path>.\n",
    );
    if omitted > 0 {
        out.push_str(&format!(
            "({omitted} earlier image(s) omitted from this block; prefer recent img_N below.)\n"
        ));
    }
    for entry in &album[start..] {
        let src = entry
            .source_user_text
            .as_deref()
            .map(|s| format!(" — \"{s}\""))
            .unwrap_or_default();
        out.push_str(&format!("- img_{}: {}{}\n", entry.index, entry.path, src));
        if let Some(cap) = entry.caption.as_ref().filter(|c| !c.trim().is_empty()) {
            let clipped: String = cap.chars().take(120).collect();
            out.push_str(&format!("  caption: {clipped}\n"));
        }
    }
    Some(out)
}

/// Collect durable paths from message images (skip unresolved data URLs).
pub fn durable_paths_from_images(images: &[AgentMessageImage]) -> Vec<String> {
    images
        .iter()
        .filter_map(|img| durable_image_path(&img.data).map(str::to_string))
        .collect()
}

/// Annotate user content with album-aware attached handles for the given images.
pub fn annotate_message_content_with_images(
    content: &str,
    images: &[AgentMessageImage],
    album: &[SessionImageEntry],
) -> String {
    let pairs = images.iter().filter_map(|img| {
        let path = durable_image_path(&img.data)?;
        let index = album_index_for_path(album, path);
        Some((index, path.to_string()))
    });
    append_attached_handles(content, pairs)
}

/// Text placeholder when every image part was stripped and no path survived.
pub fn stripped_images_placeholder(stripped_paths: &[String]) -> String {
    if stripped_paths.is_empty() {
        "[image content omitted — current model does not support image input; \
         use Session images /api/images/… paths with vision or OCR if listed]"
            .to_string()
    } else {
        let mut out = String::new();
        for path in stripped_paths {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format_attached_handle(None, path));
        }
        out.push_str(
            "\n[pixels omitted — text-only model; pass the path above to vision / OCR / image_edit]",
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg_with(path: &str, caption: Option<&str>, text: &str) -> AgentMessage {
        let mut m = AgentMessage::user(text);
        m.images = Some(vec![AgentMessageImage {
            data: path.to_string(),
            mime_type: Some("image/jpeg".to_string()),
            caption: caption.map(str::to_string),
        }]);
        m
    }

    #[test]
    fn album_dedupes_path_and_numbers_in_order() {
        let history = vec![
            msg_with("/api/images/chat-aaa.jpg", Some("meter"), "first"),
            AgentMessage::assistant("ok"),
            msg_with("/api/images/chat-bbb.jpg", None, "second"),
            msg_with("/api/images/chat-aaa.jpg", Some("ignored dup"), "again"),
        ];
        let album = build_session_album(&history);
        assert_eq!(album.len(), 2);
        assert_eq!(album[0].index, 1);
        assert_eq!(album[0].path, "/api/images/chat-aaa.jpg");
        assert_eq!(album[0].caption.as_deref(), Some("meter"));
        assert_eq!(album[1].index, 2);
        assert_eq!(album[1].path, "/api/images/chat-bbb.jpg");
    }

    #[test]
    fn format_block_includes_paths_not_absolute_urls() {
        let album = build_session_album(&[msg_with(
            "/api/images/chat-aaa.jpg",
            Some("a beige meter"),
            "ocr",
        )]);
        let block = format_album_prompt_block(&album, Some(131_072)).unwrap();
        assert!(block.contains("img_1: /api/images/chat-aaa.jpg"));
        assert!(block.contains("caption: a beige meter"));
        assert!(!block.contains("http://"));
        assert!(!block.contains("https://"));
    }

    #[test]
    fn format_block_budgets_entire_album_not_just_captions() {
        let history: Vec<_> = (1..=12)
            .map(|i| {
                msg_with(
                    &format!("/api/images/chat-{i:02}.jpg"),
                    Some("cap"),
                    &format!("user text for image {i}"),
                )
            })
            .collect();
        let album = build_session_album(&history);
        assert_eq!(album.len(), 12);
        let block = format_album_prompt_block(&album, Some(8_192)).unwrap();
        assert!(block.contains("earlier image(s) omitted"));
        assert!(!block.contains("img_1:"), "{block}");
        assert!(block.contains("img_12:"));
        // Small window keep = 4 → only img_9..img_12 entry lines.
        let entry_lines = block.lines().filter(|l| l.starts_with("- img_")).count();
        assert_eq!(entry_lines, 4, "{block}");
    }

    #[test]
    fn resolve_img_n_and_path() {
        let album = build_session_album(&[
            msg_with("/api/images/chat-aaa.jpg", None, "a"),
            msg_with("/api/images/chat-bbb.jpg", None, "b"),
        ]);
        assert_eq!(
            resolve_album_ref(&album, "img_2").map(|e| e.path.as_str()),
            Some("/api/images/chat-bbb.jpg")
        );
        assert_eq!(
            resolve_album_ref(&album, "/api/images/chat-aaa.jpg").map(|e| e.index),
            Some(1)
        );
    }

    #[test]
    fn album_includes_assistant_edited_images_as_latest() {
        let mut assistant = AgentMessage::assistant("Rotated.");
        assistant.attachments = Some(vec![serde_json::json!({
            "kind": "image",
            "title": "Edited image",
            "imageUrl": "/api/images/rotated-123.jpg"
        })]);
        let history = vec![
            msg_with("/api/images/chat-orig.jpg", None, "rotate this"),
            assistant,
        ];
        let album = build_session_album(&history);
        assert_eq!(album.len(), 2);
        assert_eq!(album[0].path, "/api/images/chat-orig.jpg");
        assert_eq!(album[1].path, "/api/images/rotated-123.jpg");
        assert_eq!(
            latest_album_entry(&album).map(|e| e.path.as_str()),
            Some("/api/images/rotated-123.jpg")
        );
    }

    #[test]
    fn append_handles_is_idempotent() {
        let once = append_attached_handles("hi", [(Some(1), "/api/images/chat-x.jpg".into())]);
        let twice = append_attached_handles(&once, [(Some(1), "/api/images/chat-x.jpg".into())]);
        assert_eq!(once.matches("/api/images/chat-x.jpg").count(), 1);
        assert_eq!(twice, once);
    }
}
