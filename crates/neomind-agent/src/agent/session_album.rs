//! Session image album: durable `/api/images/…` handles + optional captions.
//!
//! History / LLM-visible context always stores relative paths (NM-314). Absolute
//! advertise URLs (`NEOMIND_SERVER_URL` + path) are outbound-boundary only.

use std::collections::HashMap;

use super::types::{AgentMessage, AgentMessageImage};

/// Default caption rows kept in the prompt album block (Spark-class windows).
pub const ALBUM_CAPTION_KEEP_DEFAULT: usize = 8;
/// Tighter caption keep for ≤16K context windows.
pub const ALBUM_CAPTION_KEEP_SMALL: usize = 4;

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
pub fn build_session_album(messages: &[AgentMessage]) -> Vec<SessionImageEntry> {
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut entries: Vec<SessionImageEntry> = Vec::new();

    for msg in messages {
        if msg.role != "user" {
            continue;
        }
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
            let Some(path) = durable_image_path(&image.data) else {
                continue;
            };
            if let Some(&idx) = by_path.get(path) {
                // Prefer a later non-empty caption if the first sighting had none.
                if entries[idx].caption.is_none() {
                    if let Some(cap) = image.caption.as_ref().filter(|c| !c.trim().is_empty()) {
                        entries[idx].caption = Some(cap.clone());
                    }
                }
                continue;
            }
            let index = (entries.len() as u32) + 1;
            by_path.insert(path.to_string(), entries.len());
            entries.push(SessionImageEntry {
                index,
                path: path.to_string(),
                caption: image
                    .caption
                    .as_ref()
                    .filter(|c| !c.trim().is_empty())
                    .cloned(),
                source_user_text: source.clone(),
            });
        }
    }
    entries
}

/// Look up album index for a durable path.
pub fn album_index_for_path(album: &[SessionImageEntry], path: &str) -> Option<u32> {
    album
        .iter()
        .find(|e| e.path == path)
        .map(|e| e.index)
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

fn caption_keep_for_context(max_context: Option<usize>) -> usize {
    match max_context {
        Some(m) if m <= 16_384 => ALBUM_CAPTION_KEEP_SMALL,
        _ => ALBUM_CAPTION_KEEP_DEFAULT,
    }
}

/// Prompt block injected into the system prompt (relative paths only).
pub fn format_album_prompt_block(
    album: &[SessionImageEntry],
    max_context: Option<usize>,
) -> Option<String> {
    if album.is_empty() {
        return None;
    }
    let keep_captions = caption_keep_for_context(max_context);
    let start_full = album.len().saturating_sub(keep_captions);
    let mut out = String::from(
        "## Session images (durable refs — pass these paths to vision / OCR / image_edit)\n\
         Pixels may be omitted on text-only models; paths remain valid. Do not invent absolute HTTP URLs.\n",
    );
    for (i, entry) in album.iter().enumerate() {
        let src = entry
            .source_user_text
            .as_deref()
            .map(|s| format!(" — \"{s}\""))
            .unwrap_or_default();
        out.push_str(&format!("- img_{}: {}{}\n", entry.index, entry.path, src));
        if i >= start_full {
            if let Some(cap) = entry.caption.as_ref().filter(|c| !c.trim().is_empty()) {
                let clipped: String = cap.chars().take(120).collect();
                out.push_str(&format!("  caption: {clipped}\n"));
            }
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
    fn append_handles_is_idempotent() {
        let once = append_attached_handles("hi", [(Some(1), "/api/images/chat-x.jpg".into())]);
        let twice = append_attached_handles(&once, [(Some(1), "/api/images/chat-x.jpg".into())]);
        assert_eq!(once.matches("/api/images/chat-x.jpg").count(), 1);
        assert_eq!(twice, once);
    }
}
