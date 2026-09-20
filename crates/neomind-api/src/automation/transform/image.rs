#![allow(clippy::too_many_arguments)]

//! `image` — split from the former transform.rs monolith.

use serde_json::Value;

/// Resolve image data from a JSON value, handling both URLs and base64 data.
///
/// This function first checks for image URLs (e.g., `/api/images/...`), reads the
/// corresponding files, and converts them to base64. If no URLs are found, it falls
/// back to scanning for existing base64 data.
///
/// # Arguments
/// * `value` - JSON value that may contain image URLs or base64 data
///
/// # Returns
/// Base64-encoded image data (with or without data URI prefix), or empty string if not found
pub(crate) fn resolve_image_data(value: &Value) -> String {
    use std::path::Path;

    if let Some(url) = find_image_url(value) {
        let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
        if let Some(data_url) = neomind_devices::image_storage::resolve_internal_image_to_data_url(
            &url,
            Path::new(&data_dir),
        ) {
            tracing::debug!(url = %url, "Successfully resolved image URL to base64");
            return data_url;
        } else {
            tracing::warn!(url = %url, "Failed to read image file, falling back to base64 scan");
        }
    }

    find_image_data(value).to_string()
}

/// Find image URLs in a JSON value by scanning for /api/images/... patterns.
/// Checks nested objects recursively; returns the first match.
pub(crate) fn find_image_url(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s.starts_with("/api/images/") => Some(s.clone()),
        Value::Object(map) => {
            // Prioritize keys containing "image", "photo", "picture", "url", or "src"
            for (k, v) in map {
                let kl = k.to_lowercase();
                if kl.contains("image")
                    || kl.contains("photo")
                    || kl.contains("picture")
                    || kl.contains("url")
                    || kl.contains("src")
                {
                    if let Some(s) = v.as_str() {
                        if s.starts_with("/api/images/") {
                            return Some(s.to_string());
                        }
                    }
                }
            }
            // Fallback: scan all values
            for v in map.values() {
                if let Some(url) = find_image_url(v) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(arr) => {
            for v in arr {
                if let Some(url) = find_image_url(v) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

/// Find image data in a JSON value by scanning for large base64 strings.
/// Checks nested objects recursively; returns the first match.
pub(crate) fn find_image_data(value: &Value) -> &str {
    pub(crate) fn search(v: &Value) -> Option<&str> {
        match v {
            Value::String(s) if s.len() > 200 => {
                // Looks like base64 image data (data URI or raw base64)
                if s.starts_with("data:image") || !s.contains(' ') && !s.contains('\n') {
                    // Strip data URI prefix if present
                    Some(s)
                } else {
                    None
                }
            }
            Value::Object(map) => {
                // Prioritize keys containing "image" or "photo" or "picture"
                for (k, v) in map {
                    let kl = k.to_lowercase();
                    if kl.contains("image") || kl.contains("photo") || kl.contains("picture") {
                        if let Some(s) = v.as_str() {
                            if s.len() > 200 {
                                return Some(s);
                            }
                        }
                    }
                }
                // Fallback: scan all values
                for v in map.values() {
                    if let Some(s) = search(v) {
                        return Some(s);
                    }
                }
                None
            }
            _ => None,
        }
    }
    search(value).unwrap_or("")
}

/// Extract image dimensions from base64-encoded image data (PNG/JPEG headers)
pub(crate) fn extract_image_dimensions(b64: &str) -> Option<(u32, u32)> {
    if b64.len() < 40 {
        return None;
    }

    use base64::Engine;
    let decode_len = (std::cmp::min(b64.len(), 5600) / 4) * 4;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64.as_bytes()[..decode_len])
        .ok()?;

    if bytes.len() < 24 {
        return None;
    }

    // PNG: signature at byte 0, IHDR chunk has width/height at bytes 16-23
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if width > 0 && height > 0 {
            return Some((width, height));
        }
    }

    // JPEG: scan for SOF0 (0xFFC0) or SOF2 (0xFFC2) marker
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut pos = 2;
        while pos + 9 <= bytes.len() {
            if bytes[pos] != 0xFF {
                break;
            }
            let marker = bytes[pos + 1];
            if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
                let height = u16::from_be_bytes([bytes[pos + 5], bytes[pos + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[pos + 7], bytes[pos + 8]]) as u32;
                if width > 0 && height > 0 {
                    return Some((width, height));
                }
            }
            if pos + 3 >= bytes.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
            if seg_len < 2 {
                break;
            }
            pos += 2 + seg_len;
        }
    }

    None
}
