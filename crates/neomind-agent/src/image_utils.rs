//! Image data parsing utilities shared across chat, streaming, agent
//! analyzer, and data collector paths.
//!
//! Consolidates MIME type detection and base64 handling that was previously
//! duplicated across 4+ files with inconsistent behavior (missing
//! `data:image/jpg` alias in streaming, hardcoded `image/png` fallbacks,
//! `contains()` vs `starts_with()` for magic bytes, etc.).

/// Result of parsing an image data string.
#[derive(Debug, Clone, Copy)]
pub struct ParsedImage<'a> {
    /// Detected MIME type (e.g. `"image/jpeg"`, `"image/png"`).
    pub mime_type: &'static str,
    /// The raw base64 portion (no `data:...;base64,` prefix).
    pub base64: &'a str,
}

/// Parse an image data string into a [`ParsedImage`].
///
/// Accepts:
/// - `data:image/<subtype>;base64,<data>` URLs (mime from URL header)
/// - Raw base64 with recognizable magic prefix (mime inferred from prefix)
/// - Other non-empty strings: returns `image/png` as conservative fallback
///
/// Returns `None` only for empty input.
pub fn parse_image_data(s: &str) -> Option<ParsedImage<'_>> {
    if s.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_data_image_url(s) {
        return Some(parsed);
    }
    // Raw base64 path — detect mime from magic prefix.
    let mime_type = infer_mime_from_base64_prefix(s).unwrap_or("image/png");
    Some(ParsedImage {
        mime_type,
        base64: s,
    })
}

/// Try to parse a `data:image/<subtype>;base64,<data>` URL.
///
/// Returns `None` if `s` is not a data URL, has no comma separator, or
/// the MIME subtype is unrecognized.
pub fn parse_data_image_url(s: &str) -> Option<ParsedImage<'_>> {
    let after_data = s.strip_prefix("data:")?;
    let (header, data) = after_data.split_once(',')?;
    if data.is_empty() {
        return None;
    }
    // Header is like "image/jpeg;base64" or "image/png;base64"
    let (mime_part, _params) = header.split_once(';').unwrap_or((header, ""));
    if !mime_part.starts_with("image/") {
        return None;
    }
    let subtype = &mime_part["image/".len()..];
    let mime_type = normalize_mime_subtype(subtype)?;
    Some(ParsedImage {
        mime_type,
        base64: data,
    })
}

/// Normalize a MIME subtype (e.g. `"jpeg"`, `"jpg"`, `"png"`) into a
/// canonical `"image/<subtype>"` string. Returns `None` for unknown subtypes.
///
/// Handles the `jpg` → `jpeg` alias consistently across all callers.
pub fn normalize_mime_subtype(subtype: &str) -> Option<&'static str> {
    match subtype.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpeg" | "jpg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tiff" | "tif" => Some("image/tiff"),
        _ => None,
    }
}

/// Detect image MIME type from the magic prefix of a base64 string.
///
/// Uses `starts_with` (not `contains`) so a string that merely mentions
/// `/9j/` somewhere in the middle won't be misclassified as JPEG.
pub fn infer_mime_from_base64_prefix(s: &str) -> Option<&'static str> {
    if s.starts_with("/9j/") {
        Some("image/jpeg")
    } else if s.starts_with("iVBORw0KGgo") {
        Some("image/png")
    } else if s.starts_with("UklGR") {
        Some("image/webp")
    } else if s.starts_with("R0lGOD") {
        Some("image/gif")
    } else if s.starts_with("Qk") {
        Some("image/bmp")
    } else if s.starts_with("SUkq") || s.starts_with("TU0") {
        Some("image/tiff")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// I/O helpers moved from vision.rs (SSRF protection + MIME detection)
// ---------------------------------------------------------------------------

/// Image I/O error type for the free functions.
#[derive(Debug, Clone)]
pub enum ImageIoError {
    /// Invalid input arguments (empty data, malformed URL, etc.).
    InvalidArguments(String),
    /// Permission denied (path traversal, private host, blocked system path).
    PermissionDenied(String),
    /// Execution error (file read failed, HTTP fetch failed, etc.).
    Execution(String),
    /// Timeout (HTTP fetch exceeded time limit).
    Timeout,
}

impl std::fmt::Display for ImageIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageIoError::InvalidArguments(s) => write!(f, "Invalid arguments: {}", s),
            ImageIoError::PermissionDenied(s) => write!(f, "Permission denied: {}", s),
            ImageIoError::Execution(s) => write!(f, "Execution error: {}", s),
            ImageIoError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for ImageIoError {}

/// Maximum image size in bytes (10 MB). Must match vision.rs constant.
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Maximum base64 string length (~13.3 MB base64 for 10 MB raw).
const MAX_BASE64_LEN: usize = MAX_IMAGE_SIZE * 4 / 3 + 4;

/// Allowed image file extensions (lowercase). Only binary raster formats
/// that pass `detect_mime_from_bytes()` are included. SVG is excluded
/// because it cannot pass the magic-bytes validation for local files.
const ALLOWED_IMAGE_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif"];

/// Allowed MIME subtypes in data URLs (the part after "image/").
const ALLOWED_DATA_MIME_SUBTYPES: &[&str] = &["png", "jpeg", "jpg", "gif", "webp", "bmp", "tiff"];

/// Resolve image input from various formats to raw bytes + MIME type.
///
/// Accepts:
/// - `data:image/<subtype>;base64,<data>` URLs
/// - Incomplete data URLs (e.g. `image/jpeg;base64,/9j/...`)
/// - HTTP/HTTPS URLs (SSRF-protected, public hosts only)
/// - Raw base64 with magic prefix (`/9j/` JPEG, `iVBORw0KGgo` PNG, etc.)
/// - Local file paths (must start with `/` or `./`, security checks applied)
/// - Fallback: treat as raw base64 (JPEG by default)
///
/// Returns `(Vec<u8>, String)` where the second element is the MIME type
/// (e.g., `"image/png"`, `"image/jpeg"`).
///
/// # Errors
///
/// Returns `ImageIoError` for:
/// - Invalid arguments (empty data, malformed URL, unsupported MIME type)
/// - Permission denied (path traversal, private host, blocked system path)
/// - Execution errors (file read failed, HTTP fetch failed, timeout)
pub async fn resolve_image(
    input: &str,
    client: &reqwest::Client,
    max_size: usize,
) -> Result<(Vec<u8>, String), ImageIoError> {
    // 1. Data URL: data:image/png;base64,... (case-insensitive prefix)
    //    Also handles incomplete data URLs missing the "data:" prefix
    //    (e.g. "image/jpeg;base64,/9j/...") which some callers produce.
    let image_lower_prefix = input
        .chars()
        .take(11)
        .collect::<String>()
        .to_ascii_lowercase();
    let is_data_image_url = image_lower_prefix == "data:image/";
    let is_incomplete_data_url = !is_data_image_url
        && input.contains(";base64,")
        && image_lower_prefix.starts_with("image/");

    if is_data_image_url || is_incomplete_data_url {
        let rest = if is_data_image_url {
            input.get(11..) // skip "data:image/"
        } else {
            input.get(6..) // skip "image/"
        };
        if let Some(rest) = rest {
            if let Some((mime_suffix, b64)) = rest.split_once(";base64,") {
                if b64.is_empty() {
                    return Err(ImageIoError::InvalidArguments(
                        "Data URL contains empty base64 data".into(),
                    ));
                }
                if b64.len() > MAX_BASE64_LEN {
                    return Err(ImageIoError::InvalidArguments(format!(
                        "Data URL base64 data too large ({} chars, max {} chars)",
                        b64.len(),
                        MAX_BASE64_LEN
                    )));
                }
                let subtype = mime_suffix.to_lowercase();
                if !ALLOWED_DATA_MIME_SUBTYPES.contains(&subtype.as_str()) {
                    return Err(ImageIoError::InvalidArguments(format!(
                        "Unsupported image type '{}' in data URL. Allowed: {}",
                        subtype,
                        ALLOWED_DATA_MIME_SUBTYPES.join(", ")
                    )));
                }
                let mime = format!("image/{}", subtype);
                // Decode base64 to raw bytes
                let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                    .map_err(|e| {
                        ImageIoError::InvalidArguments(format!("Invalid base64 in data URL: {}", e))
                    })?;
                if bytes.len() > max_size {
                    return Err(ImageIoError::InvalidArguments(format!(
                        "Decoded image too large ({} bytes, max {} bytes)",
                        bytes.len(),
                        max_size
                    )));
                }
                return Ok((bytes, mime));
            }
        }
    }

    // 2. HTTP/HTTPS URL
    if input.starts_with("http://") || input.starts_with("https://") {
        let bytes = fetch_http_image(input, client, max_size).await?;
        let mime = detect_mime_from_bytes(&bytes)
            .unwrap_or("image/jpeg")
            .to_string();
        return Ok((bytes, mime));
    }

    // 3. Handle NeoMind internal image URLs (/api/images/<device>/<metric>/<ts>.<ext>)
    //    This MUST come before local file path check because these URLs start with '/'
    if input.starts_with("/api/images/") {
        // Delegate to the shared read-side helper in neomind-devices (next to
        // save_image_binary). Avoids read_local_image (canonicalize + system-
        // path/extension/magic gate meant for arbitrary local paths); the
        // helper centralizes path construction + traversal guard + MIME.
        use neomind_devices::image_storage::{read_internal_image_url, ImageStorageError};

        let data_dir = neomind_core::paths::data_dir()
            .to_string_lossy()
            .to_string();

        let (bytes, mime) = read_internal_image_url(input, std::path::Path::new(&data_dir))
            .map_err(|e| match e {
                ImageStorageError::InvalidPathComponent(_) => {
                    ImageIoError::PermissionDenied(e.to_string())
                }
                ImageStorageError::IoError(_) => ImageIoError::InvalidArguments(format!(
                    "Failed to read /api/images/ URL {input}: {e}"
                )),
                ImageStorageError::UnknownFileType => {
                    ImageIoError::InvalidArguments(format!("Unrecognized image file for {input}"))
                }
                ImageStorageError::TooLarge(_) => ImageIoError::InvalidArguments(format!(
                    "Image file too large for /api/images/ URL {input}: {e}"
                )),
            })?;

        tracing::info!(url = %input, size = bytes.len(), "Resolved /api/images/ URL");

        if bytes.len() > max_size {
            return Err(ImageIoError::InvalidArguments(format!(
                "Image file too large: {} bytes (max {})",
                bytes.len(),
                max_size
            )));
        }
        return Ok((bytes, mime.to_string()));
    }

    // 4. Block non-http URL schemes with a clear error
    if input.contains("://") {
        return Err(ImageIoError::InvalidArguments(format!(
            "Unsupported URL scheme in '{}'. Only http:// and https:// are supported.",
            input.split("://").next().unwrap_or("")
        )));
    }

    // 5. Raw base64 detection (MUST come before local file path check).
    //
    // Why: a stripped JPEG base64 starts with "/9j/" and a PNG base64 starts
    // with "iVBORw0KGgo". The "/" prefix would otherwise be misclassified
    // as a local file path, producing "Cannot resolve path '/9j/...'" errors
    // when the LLM passes raw base64 (no data URL wrapper) into the tool.
    //
    // Heuristic: looks_like_raw_base64 returns true when the string is
    // pure base64 alphabet AND either carries an image magic prefix or is
    // long enough to plausibly be image data.
    if looks_like_raw_base64(input) {
        if input.is_empty() {
            return Err(ImageIoError::InvalidArguments("Image data is empty".into()));
        }
        if input.len() > MAX_BASE64_LEN {
            return Err(ImageIoError::InvalidArguments(format!(
                "Base64 data too large ({} chars, max {} chars)",
                input.len(),
                MAX_BASE64_LEN
            )));
        }
        let mime = infer_mime_from_base64_prefix(input)
            .unwrap_or("image/jpeg")
            .to_string();
        // Decode base64 to raw bytes
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, input)
            .map_err(|e| ImageIoError::InvalidArguments(format!("Invalid base64 data: {}", e)))?;
        if bytes.len() > max_size {
            return Err(ImageIoError::InvalidArguments(format!(
                "Decoded image too large ({} bytes, max {} bytes)",
                bytes.len(),
                max_size
            )));
        }
        tracing::debug!(
            len = input.len(),
            inferred_mime = %mime,
            "Treating image argument as raw base64"
        );
        return Ok((bytes, mime));
    }

    // 6. Local file path
    if input.starts_with('/') || input.starts_with("./") {
        let bytes = read_local_image(input, max_size)?;
        let mime = detect_mime_from_bytes(&bytes)
            .unwrap_or("image/jpeg")
            .to_string();
        return Ok((bytes, mime));
    }

    // 7. Fallback: treat as raw base64
    if input.is_empty() {
        return Err(ImageIoError::InvalidArguments("Image data is empty".into()));
    }
    if input.len() > MAX_BASE64_LEN {
        return Err(ImageIoError::InvalidArguments(format!(
            "Base64 data too large ({} chars, max {} chars)",
            input.len(),
            MAX_BASE64_LEN
        )));
    }
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, input)
        .map_err(|e| ImageIoError::InvalidArguments(format!("Invalid base64 data: {}", e)))?;
    if bytes.len() > max_size {
        return Err(ImageIoError::InvalidArguments(format!(
            "Decoded image too large ({} bytes, max {} bytes)",
            bytes.len(),
            max_size
        )));
    }
    Ok((bytes, "image/jpeg".to_string()))
}

/// Read a local image file with security checks.
///
/// Blocks:
/// - Path traversal (`..` components)
/// - System paths (`/etc`, `/proc`, `/sys`, etc.)
/// - Hidden files (dotfiles) in home directories
/// - Files without allowed image extensions
/// - Files exceeding `max_size` bytes
///
/// Validates the file has image magic bytes before returning.
pub fn read_local_image(path_str: &str, max_size: usize) -> Result<Vec<u8>, ImageIoError> {
    use std::path::Path;

    let path = Path::new(path_str);

    // Block path traversal
    for component in path.components() {
        if component == std::path::Component::ParentDir {
            return Err(ImageIoError::PermissionDenied(
                "Path traversal (..) is not allowed".into(),
            ));
        }
    }

    // Canonicalize to resolve symlinks - use blocking I/O since this is a sync function
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        ImageIoError::Execution(format!("Cannot resolve path '{}': {}", path_str, e))
    })?;
    let canonical_str = canonical.to_string_lossy();

    // Block sensitive system paths (checked against canonical path)
    let canonical_lower = canonical_str.to_lowercase();
    let blocked_prefixes = [
        "/etc/", "/proc/", "/sys/", "/dev/", "/run/", "/boot/", "/root/", "/var/", "/tmp/",
        "/opt/", "/srv/",
    ];
    if blocked_prefixes
        .iter()
        .any(|prefix| canonical_lower.starts_with(prefix))
    {
        return Err(ImageIoError::PermissionDenied(
            "Access to system path is not allowed".to_string(),
        ));
    }

    // Block hidden files (dotfiles) in home directories
    if let Some(name) = canonical.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.')
            && (canonical_lower.starts_with("/home") || canonical_lower.starts_with("/users"))
        {
            return Err(ImageIoError::PermissionDenied(
                "Access to hidden file is not allowed".to_string(),
            ));
        }
    }

    // Validate extension looks like an image
    if let Some(ext) = canonical.extension().and_then(|e| e.to_str()) {
        if !ALLOWED_IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
            return Err(ImageIoError::PermissionDenied(format!(
                "File extension '.{}' is not an image format. Allowed: {}",
                ext,
                ALLOWED_IMAGE_EXTENSIONS.join(", ")
            )));
        }
    } else {
        // Files without extension: reject (must have a known image extension)
        return Err(ImageIoError::PermissionDenied(
            "File must have an image extension (e.g., .jpg, .png)".into(),
        ));
    }

    // Check file size before reading
    let metadata = std::fs::metadata(&canonical).map_err(|e| {
        ImageIoError::Execution(format!("Failed to stat file '{}': {}", path_str, e))
    })?;
    if metadata.len() as usize > max_size {
        return Err(ImageIoError::Execution(format!(
            "File too large: {} bytes (max {} bytes)",
            metadata.len(),
            max_size
        )));
    }

    let bytes = std::fs::read(&canonical).map_err(|e| {
        ImageIoError::Execution(format!("Failed to read file '{}': {}", path_str, e))
    })?;

    // Validate the file looks like an image by checking magic bytes
    if detect_mime_from_bytes(&bytes).is_none() {
        return Err(ImageIoError::Execution(format!(
            "File '{}' does not appear to be a valid image (unrecognized header)",
            path_str
        )));
    }

    Ok(bytes)
}

/// Fetch image bytes from an HTTP/HTTPS URL with SSRF protection.
///
/// Validates:
/// - URL scheme is http or https
/// - Host is not a private/local address (SSRF protection)
/// - Content-Type looks like an image
/// - Content-Length and actual bytes don't exceed `max_size`
///
/// Returns the raw image bytes on success.
pub async fn fetch_http_image(
    url: &str,
    client: &reqwest::Client,
    max_size: usize,
) -> Result<Vec<u8>, ImageIoError> {
    // SSRF: validate URL before fetching
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| ImageIoError::InvalidArguments(format!("Invalid URL: {}", e)))?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(ImageIoError::PermissionDenied(
                "Only http:// and https:// URLs are allowed".into(),
            ))
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ImageIoError::InvalidArguments("URL has no host".into()))?;

    if is_private_host(host) {
        return Err(ImageIoError::PermissionDenied(format!(
            "Access to private address '{}' is not allowed",
            host
        )));
    }

    let response = client
        .get(url)
        .header("User-Agent", "NeoMind-ImageUtils/1.0")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ImageIoError::Timeout
            } else {
                ImageIoError::Execution(format!("HTTP fetch failed: {}", e))
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(ImageIoError::Execution(format!(
            "HTTP {} fetching image",
            status.as_u16()
        )));
    }

    // Check Content-Length before downloading body
    if let Some(content_length) = response.headers().get("content-length") {
        if let Ok(len_str) = content_length.to_str() {
            if let Ok(len) = len_str.parse::<usize>() {
                if len > max_size {
                    return Err(ImageIoError::Execution(format!(
                        "Image too large (Content-Length: {} bytes, max: {} bytes)",
                        len, max_size
                    )));
                }
            }
        }
    }

    // Validate Content-Type looks like an image
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    if !media_type.starts_with("image/") && !media_type.is_empty() {
        return Err(ImageIoError::Execution(format!(
            "URL returned non-image content type: {}. Only image/* is supported.",
            content_type
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ImageIoError::Execution(format!("HTTP read failed: {}", e)))?;

    if bytes.len() > max_size {
        return Err(ImageIoError::Execution(format!(
            "Image too large: {} bytes (max: {} bytes)",
            bytes.len(),
            max_size
        )));
    }

    Ok(bytes.to_vec())
}

/// Detect MIME type from image file magic bytes.
pub fn detect_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4 {
        return None;
    }
    match &bytes[..4] {
        [0x89, 0x50, 0x4E, 0x47] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, _] => Some("image/jpeg"),
        [0x47, 0x49, 0x46, _] => Some("image/gif"),
        [0x52, 0x49, 0x46, _] if bytes.len() >= 12 && &bytes[8..12] == b"WEBP" => {
            Some("image/webp")
        }
        // BMP: starts with "BM"
        [0x42, 0x4D, _, _] => Some("image/bmp"),
        // TIFF little-endian: II*\0
        [0x49, 0x49, 0x2A, 0x00] => Some("image/tiff"),
        // TIFF big-endian: MM\0*
        [0x4D, 0x4D, 0x00, 0x2A] => Some("image/tiff"),
        _ => None,
    }
}

/// Heuristic: does this string look like raw base64-encoded image data
/// (as opposed to a filesystem path or URL)?
///
/// We say "yes" when:
/// 1. Every character belongs to the standard base64 alphabet
///    (`A-Za-z0-9+/=`), AND
/// 2. Either the string starts with a known image magic prefix
///    (`/9j/` JPEG, `iVBORw0KGgo` PNG, `UklGR` WebP, `R0lGOD` GIF,
///    `Qk` BMP, `SUkq` TIFF) AND is at least 32 chars long, OR
/// 3. The string is long enough (>=256 chars) to plausibly be image data
///    (avoids short base64-looking strings like path components).
///
/// This MUST stay before the "local file path" branch in callers that
/// resolve image inputs, because JPEG base64 starts with `/9j/` and would
/// otherwise be misclassified as an absolute path.
pub fn looks_like_raw_base64(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Quick reject: any character outside base64 alphabet.
    // '/', '+', '=' are valid base64 chars. '.', '\', ':' are NOT — they
    // reject real paths like "/tmp/foo.jpg", "C:\\...", "/home/a:b".
    let is_pure_b64 = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=');
    if !is_pure_b64 {
        return false;
    }
    // Known image base64 magic prefixes, with minimum length to avoid
    // false positives on very short strings like "/9j/" alone (4 chars).
    const MIN_MAGIC_LEN: usize = 32;
    let has_magic = s.len() >= MIN_MAGIC_LEN
        && (s.starts_with("/9j/")         // JPEG
            || s.starts_with("iVBORw0KGgo") // PNG
            || s.starts_with("UklGR")       // WebP
            || s.starts_with("R0lGOD")      // GIF
            || s.starts_with("Qk")          // BMP
            || s.starts_with("SUkq")        // TIFF LE
            || s.starts_with("TU0")); // TIFF BE
    if has_magic {
        return true;
    }
    // Long pure-base64 strings without magic — still likely image data,
    // but require a reasonable minimum to avoid false positives on hash-like
    // or token-like strings (e.g. JWT fragments, 256-char base64 tokens).
    s.len() >= 1024
}

/// Check if a hostname points to a private/local address (SSRF protection).
pub fn is_private_host(host: &str) -> bool {
    match host {
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" => return true,
        _ => {}
    }

    let host_trimmed = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_trimmed.parse::<std::net::IpAddr>() {
        return is_private_ip(&ip);
    }

    if host.ends_with(".local") || host.ends_with(".localhost") || host == "localhost.localdomain" {
        return true;
    }

    false
}

/// Check if an IP address is private/local.
pub fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets[0] == 10 {
                return true;
            }
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            if octets[0] == 127 {
                return true;
            }
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            if octets[0] == 0 {
                return true;
            }
            if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                return true;
            }
            if v4.is_broadcast() || v4.is_multicast() || v4.is_unspecified() {
                return true;
            }
        }
        std::net::IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_multicast() || v6.is_unspecified() {
                return true;
            }
            let segments = v6.segments();
            if (segments[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            if (segments[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            if let Some(v4) = v6.to_ipv4() {
                return is_private_ip(&std::net::IpAddr::V4(v4));
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_data_url_jpeg() {
        let p = parse_image_data("data:image/jpeg;base64,/9j/AAAA").unwrap();
        assert_eq!(p.mime_type, "image/jpeg");
        assert_eq!(p.base64, "/9j/AAAA");
    }

    #[test]
    fn parse_data_url_jpg_alias_canonicalized() {
        // Regression for the streaming.rs bug that only checked "jpeg".
        let p = parse_image_data("data:image/jpg;base64,/9j/AAAA").unwrap();
        assert_eq!(p.mime_type, "image/jpeg");
        assert_eq!(p.base64, "/9j/AAAA");
    }

    #[test]
    fn parse_data_url_png() {
        let p = parse_image_data("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert_eq!(p.mime_type, "image/png");
        assert_eq!(p.base64, "iVBORw0KGgo=");
    }

    #[test]
    fn parse_data_url_unknown_subtype_rejected() {
        assert!(parse_data_image_url("data:image/heic;base64,AAAA").is_none());
    }

    #[test]
    fn parse_data_url_empty_data_rejected() {
        assert!(parse_data_image_url("data:image/jpeg;base64,").is_none());
    }

    #[test]
    fn parse_raw_base64_jpeg_magic() {
        let p = parse_image_data("/9j/4AAQSkZJRgABAQ").unwrap();
        assert_eq!(p.mime_type, "image/jpeg");
    }

    #[test]
    fn parse_raw_base64_png_magic() {
        let p = parse_image_data("iVBORw0KGgoAAAANSUhEUg").unwrap();
        assert_eq!(p.mime_type, "image/png");
    }

    #[test]
    fn parse_raw_base64_webp_magic() {
        let p = parse_image_data("UklGRkBAAABAQ").unwrap();
        assert_eq!(p.mime_type, "image/webp");
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_image_data("").is_none());
    }

    #[test]
    fn parse_unknown_falls_back_to_png() {
        let p = parse_image_data("AAAAAaaaaa").unwrap();
        assert_eq!(p.mime_type, "image/png");
    }

    #[test]
    fn magic_in_middle_does_not_match() {
        // Regression for data_collector bug that used contains() instead of
        // starts_with(): a text string with "/9j/" in the middle must NOT
        // be misclassified as an image.
        let s = "device log: /9j/ something happened";
        assert!(infer_mime_from_base64_prefix(s).is_none());
    }

    #[test]
    fn normalize_jpg_alias() {
        assert_eq!(normalize_mime_subtype("jpg"), Some("image/jpeg"));
        assert_eq!(normalize_mime_subtype("jpeg"), Some("image/jpeg"));
        assert_eq!(normalize_mime_subtype("JPEG"), Some("image/jpeg"));
        assert_eq!(normalize_mime_subtype("heic"), None);
    }

    // --- detect_mime_from_bytes tests ---

    #[test]
    fn test_detect_mime_jpeg() {
        assert_eq!(
            detect_mime_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]),
            Some("image/jpeg")
        );
    }

    #[test]
    fn test_detect_mime_png() {
        assert_eq!(
            detect_mime_from_bytes(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A]),
            Some("image/png")
        );
    }

    #[test]
    fn test_detect_mime_gif() {
        assert_eq!(
            detect_mime_from_bytes(&[0x47, 0x49, 0x46, 0x38, 0x39, 0x61]),
            Some("image/gif")
        );
    }

    #[test]
    fn test_detect_mime_bmp() {
        assert_eq!(
            detect_mime_from_bytes(&[0x42, 0x4D, 0x00, 0x00]),
            Some("image/bmp")
        );
    }

    #[test]
    fn test_detect_mime_tiff_le() {
        assert_eq!(
            detect_mime_from_bytes(&[0x49, 0x49, 0x2A, 0x00, 0x00, 0x00]),
            Some("image/tiff")
        );
    }

    #[test]
    fn test_detect_mime_tiff_be() {
        assert_eq!(
            detect_mime_from_bytes(&[0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00]),
            Some("image/tiff")
        );
    }

    #[test]
    fn test_detect_mime_unknown() {
        assert_eq!(detect_mime_from_bytes(&[0x00, 0x00, 0x00, 0x00]), None);
    }

    #[test]
    fn test_detect_mime_too_small() {
        assert_eq!(detect_mime_from_bytes(&[0xFF]), None);
        assert_eq!(detect_mime_from_bytes(&[]), None);
    }

    // --- looks_like_raw_base64 tests ---

    #[test]
    fn test_looks_like_raw_base64_jpeg_magic() {
        // JPEG base64 always starts with "/9j/" — this used to be
        // misclassified as an absolute file path.
        assert!(looks_like_raw_base64("/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAA"));
    }

    #[test]
    fn test_looks_like_raw_base64_png_magic() {
        assert!(looks_like_raw_base64("iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB"));
    }

    #[test]
    fn test_looks_like_raw_base64_webp_magic() {
        assert!(looks_like_raw_base64(
            "UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoB"
        ));
    }

    #[test]
    fn test_looks_like_raw_base64_short_magic_rejects_path_like_strings() {
        // "/9j/" alone is 4 chars and not enough evidence — should NOT match
        // (avoids misclassifying weird short paths)
        assert!(!looks_like_raw_base64("/9j/"));
    }

    #[test]
    fn test_looks_like_raw_base64_rejects_path_with_extension() {
        // Real file paths contain '.', '\', ':' etc which are NOT in base64 alphabet
        assert!(!looks_like_raw_base64("/tmp/photo.jpg"));
        assert!(!looks_like_raw_base64("/home/user/file.png"));
        assert!(!looks_like_raw_base64("./images/photo.jpeg"));
        assert!(!looks_like_raw_base64("C:\\Users\\photo.jpg"));
    }

    #[test]
    fn test_looks_like_raw_base64_rejects_short_pure_alphabet() {
        // Short pure-base64-alphabet strings are ambiguous — treat as path
        assert!(!looks_like_raw_base64("/abc"));
        assert!(!looks_like_raw_base64("ABC"));
    }

    #[test]
    fn test_looks_like_raw_base64_accepts_long_pure() {
        // 1024+ chars of pure base64 alphabet without magic — still likely image
        let long: String = "ABCD".repeat(300); // 1200 chars
        assert!(looks_like_raw_base64(&long));
    }

    #[test]
    fn test_looks_like_raw_base64_rejects_medium_token_like() {
        // 320 chars without magic — could be a JWT/hash fragment, reject
        let medium: String = "ABCD".repeat(80); // 320 chars
        assert!(!looks_like_raw_base64(&medium));
    }

    #[test]
    fn test_looks_like_raw_base64_rejects_empty() {
        assert!(!looks_like_raw_base64(""));
    }

    // --- is_private_host tests ---

    #[test]
    fn test_is_private_host_localhost() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("0.0.0.0"));
        assert!(is_private_host("::1"));
    }

    #[test]
    fn test_is_private_host_private_ranges() {
        assert!(is_private_host("10.0.0.1"));
        assert!(is_private_host("172.16.0.1"));
        assert!(is_private_host("192.168.1.1"));
        assert!(is_private_host("169.254.169.254"));
    }

    #[test]
    fn test_is_private_host_public() {
        assert!(!is_private_host("8.8.8.8"));
        assert!(!is_private_host("1.1.1.1"));
        assert!(!is_private_host("example.com"));
    }

    #[test]
    fn test_is_private_ipv6_mapped() {
        assert!(is_private_host("::ffff:127.0.0.1"));
        assert!(is_private_host("::ffff:192.168.1.1"));
    }

    // --- infer_mime_from_base64_prefix tests (moved from vision.rs) ---

    #[test]
    fn test_infer_mime_jpeg() {
        assert_eq!(
            infer_mime_from_base64_prefix("/9j/4AAQSkZJRg"),
            Some("image/jpeg")
        );
    }

    #[test]
    fn test_infer_mime_png() {
        assert_eq!(
            infer_mime_from_base64_prefix("iVBORw0KGgoAAAAN"),
            Some("image/png")
        );
    }

    #[test]
    fn test_infer_mime_webp() {
        assert_eq!(
            infer_mime_from_base64_prefix("UklGRiQA"),
            Some("image/webp")
        );
    }

    #[test]
    fn test_infer_mime_gif() {
        assert_eq!(infer_mime_from_base64_prefix("R0lGODlh"), Some("image/gif"));
    }

    #[test]
    fn test_infer_mime_bmp() {
        assert_eq!(infer_mime_from_base64_prefix("Qk0+AAAA"), Some("image/bmp"));
    }

    #[test]
    fn test_infer_mime_unknown_returns_none() {
        assert_eq!(infer_mime_from_base64_prefix("randomStuff"), None);
    }

    // --- I/O tests (Step 2.1) ---

    mod io_tests {
        use super::*;
        use reqwest::Client;

        fn make_client() -> Client {
            Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .no_proxy()
                .build()
                .expect("client")
        }

        #[tokio::test]
        async fn resolve_image_data_url_returns_raw_bytes() {
            // 1×1 PNG: iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==
            let s = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
            let client = make_client();
            let (bytes, mime) = resolve_image(s, &client, 10 * 1024 * 1024)
                .await
                .expect("resolve");
            assert_eq!(mime, "image/png");
            assert!(!bytes.is_empty());
            assert_eq!(
                &bytes[..8],
                &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
            );
        }

        #[serial_test::serial]
        #[tokio::test]
        async fn resolve_image_api_images_url_reads_file() {
            // Create a temporary test directory
            let temp_dir = std::env::temp_dir();
            let test_data_dir =
                temp_dir.join(format!("neomind_test_api_images_{}", uuid::Uuid::new_v4()));

            // Set up test image directory
            crate::testing_helpers::setup_test_image_dir(&test_data_dir)
                .expect("Failed to set up test image directory");

            // Set NEOMIND_DATA_DIR for this test
            std::env::set_var("NEOMIND_DATA_DIR", test_data_dir.to_str().unwrap());

            let client = make_client();

            // Test /api/images/ URL resolution
            let url = "/api/images/test-device-001/image/1234567890000.png";
            let (bytes, mime) = resolve_image(url, &client, 10 * 1024 * 1024)
                .await
                .expect("resolve /api/images/ URL");

            assert_eq!(mime, "image/png");
            assert!(!bytes.is_empty());
            assert_eq!(
                &bytes[..8],
                &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
            );

            // Test JPEG URL
            let jpg_url = "/api/images/test-device-001/image/1234567890001.jpg";
            let (jpg_bytes, jpg_mime) = resolve_image(jpg_url, &client, 10 * 1024 * 1024)
                .await
                .expect("resolve /api/images/ JPEG URL");

            assert_eq!(jpg_mime, "image/jpeg");
            assert!(!jpg_bytes.is_empty());
            assert_eq!(&jpg_bytes[..2], &[0xFF, 0xD8]);

            // Clean up
            crate::testing_helpers::cleanup_test_image_dir(&test_data_dir)
                .expect("Failed to clean up test directory");
            std::env::remove_var("NEOMIND_DATA_DIR");
        }

        #[serial_test::serial]
        #[tokio::test]
        async fn resolve_image_api_images_url_file_not_found() {
            // Create a temporary test directory (empty, no images)
            let temp_dir = std::env::temp_dir();
            let test_data_dir = temp_dir.join(format!(
                "neomind_test_api_images_empty_{}",
                uuid::Uuid::new_v4()
            ));

            std::fs::create_dir_all(&test_data_dir).expect("Failed to create temp dir");

            // Set NEOMIND_DATA_DIR for this test
            std::env::set_var("NEOMIND_DATA_DIR", test_data_dir.to_str().unwrap());

            let client = make_client();

            // Test /api/images/ URL with non-existent file
            let url = "/api/images/test-device-001/image/9999999999.png";
            let result = resolve_image(url, &client, 10 * 1024 * 1024).await;

            assert!(result.is_err(), "Should fail for non-existent file");

            // Clean up
            std::fs::remove_dir_all(&test_data_dir).expect("Failed to clean up test directory");
            std::env::remove_var("NEOMIND_DATA_DIR");
        }

        #[serial_test::serial]
        #[tokio::test]
        async fn resolve_image_api_images_url_rejects_traversal() {
            let temp_dir = std::env::temp_dir();
            let test_data_dir = temp_dir.join(format!(
                "neomind_test_api_images_traversal_{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&test_data_dir).expect("Failed to create temp dir");
            std::env::set_var("NEOMIND_DATA_DIR", test_data_dir.to_str().unwrap());

            let client = make_client();

            for evil in [
                "/api/images/../../etc/passwd",
                "/api/images/test-device/../../../etc/passwd",
                "/api/images/test-device/image/../../../etc/shadow.png",
            ] {
                let result = resolve_image(evil, &client, 10 * 1024 * 1024).await;
                assert!(
                    matches!(result, Err(ImageIoError::PermissionDenied(_))),
                    "traversal URL {:?} should be rejected as PermissionDenied, got {:?}",
                    evil,
                    result
                );
            }

            std::fs::remove_dir_all(&test_data_dir).expect("Failed to clean up test directory");
            std::env::remove_var("NEOMIND_DATA_DIR");
        }

        #[tokio::test]
        async fn resolve_image_api_images_url_backward_compatibility() {
            // Ensure existing data URL, http URL, and base64 still work
            let client = make_client();

            // Test data URL (existing functionality)
            let data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
            let (_bytes, mime) = resolve_image(data_url, &client, 10 * 1024 * 1024)
                .await
                .expect("data URL should still work");
            assert_eq!(mime, "image/png");

            // Test raw base64 (existing functionality)
            // Use a valid base64 string with proper padding
            let raw_base64 = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQFxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAv/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCgAyAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg";
            let (_b64_bytes, b64_mime) = resolve_image(raw_base64, &client, 10 * 1024 * 1024)
                .await
                .expect("raw base64 should still work");
            assert_eq!(b64_mime, "image/jpeg");
        }
    }
}
