//! Static image file server for data/images/.
//!
//! Used by chat markdown rendering: the LLM writes `![alt](/api/images/...)`
//! and the browser fetches the image from this route.
//!
//! ## Path Formats
//!
//! Two layouts are stored and served under `<data_dir>/images/`:
//!
//! - Structured (device snapshot binary storage):
//!   ```text
//!   /api/images/<device_id>/<metric>/<timestamp>.<ext>
//!   ```
//!   e.g. `/api/images/camera-001/image/1634567890000.jpg`
//!
//! - Flat (tool outputs such as `image_edit` / image analyzers):
//!   ```text
//!   /api/images/<filename>
//!   ```
//!   e.g. `/api/images/550e8400-e29b-41d4-a716-446655440000.png`
//!
//!   The img-T1 rework regressed this layout (handler began requiring exactly
//!   three components); it is the original 201701d0 serving contract.

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::handlers::ServerState;

/// Allowed extensions (lowercase, no leading dot).
const ALLOWED_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "bin"];

/// Handler for serving image files from the structured or flat path.
///
/// # Path Formats
///
/// ```text
/// /api/images/<filename>
/// /api/images/<device_id>/<metric>/<timestamp>.<ext>
/// ```
pub async fn get_image_handler(
    State(state): State<ServerState>,
    Path(path): Path<String>,
) -> Response {
    serve_image_request(&state.data_dir, &path).await
}

/// Parse + validate an `/api/images/` sub-path, then serve the file.
///
/// Split from the HTTP handler so tests can exercise both path layouts
/// without constructing a full `ServerState`.
async fn serve_image_request(data_dir: &std::path::Path, path: &str) -> Response {
    // 1. Split path into components and validate structure. Both the flat
    //    tool-output layout (`<filename>`) and the structured snapshot layout
    //    (`<device_id>/<metric>/<filename>`) are accepted.
    let components: Vec<&str> = path.split('/').collect();

    let rel: &[&str] = match components.len() {
        1 | 3 => &components,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "invalid path format, expected: /api/images/<filename> or /api/images/device_id/metric/timestamp.ext",
            )
                .into_response()
        }
    };

    // 2. Validate path components to prevent traversal
    if rel.len() == 3 && (!is_safe_component(rel[0]) || !is_safe_component(rel[1])) {
        return (StatusCode::BAD_REQUEST, "invalid device_id or metric").into_response();
    }

    let filename = rel[rel.len() - 1];
    if !is_safe_filename(filename) {
        return (StatusCode::BAD_REQUEST, "invalid filename").into_response();
    }

    // 3. Build file path: <data_dir>/images/[<device_id>/<metric>/]<filename>
    let images_dir = data_dir.join("images");
    let mut file_path = images_dir.clone();
    for component in rel {
        file_path = file_path.join(component);
    }

    // 4. Resolve canonical paths and verify the file is actually inside
    //    images_dir. This defeats symlinks: if `images_dir/...` is a
    //    symlink to `/etc/passwd`, canonicalize() resolves it and the
    //    starts_with check fails.
    let canon_images = match images_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "images dir not present").into_response(),
    };
    let canon_file = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "image not found").into_response(),
    };
    if !canon_file.starts_with(&canon_images) {
        // Symlink escape attempt.
        tracing::warn!(
            path = %path,
            "rejected image path outside images dir"
        );
        return (StatusCode::NOT_FOUND, "image not found").into_response();
    }

    // 5. Read bytes (could use tokio fs, but images are small — sync read in
    //    blocking task is fine).
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(&file_path)).await {
        Ok(Ok(b)) => b,
        Ok(Err(_)) => return (StatusCode::NOT_FOUND, "image not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "read failed").into_response(),
    };

    // 6. Derive Content-Type from extension.
    let content_type = mime_from_ext(filename);

    // 7. Build response with immutable cache headers.
    let mut resp = bytes.into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=2592000, immutable"),
    );
    resp
}

/// Validate that `component` is a safe path segment (device_id or metric).
fn is_safe_component(component: &str) -> bool {
    if component.is_empty() || component.starts_with('.') {
        return false;
    }
    // Reject any path separator or traversal token.
    if component.contains('/') || component.contains('\\') || component.contains("..") {
        return false;
    }
    if component.contains('\0') {
        return false;
    }
    // All chars must be in safe set: alphanumeric, dash, underscore, dot.
    if !component
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return false;
    }
    true
}

/// Validate that `name` is a safe single-segment filename.
fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') {
        return false;
    }
    // Reject any path separator or traversal token.
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    if name.contains('\0') {
        return false;
    }
    // Must have an allowed extension.
    let lower = name.to_ascii_lowercase();
    let Some(ext) = lower.rsplit('.').next() else {
        return false;
    };
    if !ALLOWED_EXTS.contains(&ext) {
        return false;
    }
    // All chars must be in safe set: alphanumeric, dash, underscore, dot.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return false;
    }
    true
}

/// Map a filename extension to a MIME type. Falls back to JPEG.
fn mime_from_ext(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filenames_accepted() {
        assert!(is_safe_filename("abc123.png"));
        assert!(is_safe_filename("1634567890000.jpg"));
        assert!(is_safe_filename("image_edit_2026.webp"));
        assert!(is_safe_filename(
            "550e8400-e29b-41d4-a716-446655440000.jpeg"
        ));
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(!is_safe_filename("../etc/passwd.png"));
        assert!(!is_safe_filename("a/b.png"));
        assert!(!is_safe_filename("a\\b.png"));
        assert!(!is_safe_filename(".hidden.png"));
        assert!(!is_safe_filename("foo..png")); // contains ".."
        assert!(!is_safe_filename("foo\x00.png")); // null byte
    }

    #[test]
    fn bad_extensions_rejected() {
        assert!(!is_safe_filename("foo.txt"));
        assert!(!is_safe_filename("foo")); // no extension
        assert!(!is_safe_filename("foo.exe"));
        assert!(!is_safe_filename("foo.PnG.exe"));
    }

    #[test]
    fn unsafe_chars_rejected() {
        assert!(!is_safe_filename("foo bar.png")); // space
        assert!(!is_safe_filename("foo+bar.png")); // plus
        assert!(!is_safe_filename("café.png")); // non-ascii
    }

    #[test]
    fn mime_mapping() {
        assert_eq!(mime_from_ext("x.png"), "image/png");
        assert_eq!(mime_from_ext("x.JPG"), "image/jpeg");
        assert_eq!(mime_from_ext("x.webp"), "image/webp");
        assert_eq!(mime_from_ext("x.unknown"), "image/jpeg");
    }

    #[test]
    fn safe_components_accepted() {
        assert!(is_safe_component("device-001"));
        assert!(is_safe_component("camera_01"));
        assert!(is_safe_component("temperature.sensor"));
        assert!(is_safe_component("metric-name"));
    }

    #[test]
    fn component_traversal_rejected() {
        assert!(!is_safe_component("../etc"));
        assert!(!is_safe_component("device/001"));
        assert!(!is_safe_component("device\\001"));
        assert!(!is_safe_component(".hidden"));
        assert!(!is_safe_component("foo..bar"));
        assert!(!is_safe_component("dev\x00ice"));
    }

    #[test]
    fn component_empty_rejected() {
        assert!(!is_safe_component(""));
    }

    #[test]
    fn component_unsafe_chars_rejected() {
        assert!(!is_safe_component("device 001")); // space
        assert!(!is_safe_component("device+001")); // plus
        assert!(!is_safe_component("caméra")); // non-ascii
    }

    /// Regression: `image_edit` returns `/api/images/<uuid>.<ext>` (flat,
    /// single segment) and embeds it in chat markdown — the img-T1 rework
    /// made the handler reject it with 400, breaking chat image rendering.
    #[tokio::test]
    async fn serves_flat_tool_output_layout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join("images").join("abc.png"), b"\x89PNG-fake").unwrap();

        let resp = serve_image_request(dir.path(), "abc.png").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"\x89PNG-fake");
    }

    #[tokio::test]
    async fn serves_structured_snapshot_layout() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("images").join("cam-001").join("image");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("123.jpg"), b"jpegdata").unwrap();

        let resp = serve_image_request(dir.path(), "cam-001/image/123.jpg").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
    }

    #[tokio::test]
    async fn rejects_two_segment_paths() {
        let dir = tempfile::tempdir().unwrap();
        let resp = serve_image_request(dir.path(), "a/b.png").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rejects_traversal_component_in_structured_path() {
        let dir = tempfile::tempdir().unwrap();
        let resp = serve_image_request(dir.path(), "../secret/image/x.png").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = serve_image_request(dir.path(), "cam-001/../x.png").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn missing_flat_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        let resp = serve_image_request(dir.path(), "nope.png").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
