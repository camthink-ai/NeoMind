//! Webhook receiver for device data.
//!
//! Devices can POST data to this endpoint instead of being polled.
//! This is useful for devices that actively push data.
//!
//! All processing is delegated to `WebhookAdapter` which handles:
//! - Per-device token verification (`Authorization: Bearer` or `?token=`)
//! - Optional adapter-level API key (`X-API-Key` header)
//! - Optional IP allowlist/blocklist
//! - Per-device rate limiting
//! - Per-IP discovery-event throttling (prevents auto-onboard amplification)
//! - Auto-discovery for unknown devices
//! - Data extraction via UnifiedExtractor

use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap},
};
use std::net::SocketAddr;
use tracing::{info, warn};

use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

use neomind_devices::adapters::webhook::WebhookPayload;

/// Supported image MIME types for direct binary upload via webhook.
const IMAGE_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/jpg",
    "image/png",
    "image/webp",
    "image/gif",
    "image/bmp",
];

/// Acceptable Content-Types for the webhook body (used in 415 diagnostics).
const ACCEPTABLE_CONTENT_TYPES: &[&str] = &[
    "application/json",
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/gif",
    "image/bmp",
    "text/plain",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
];

/// Extract webhook token from request headers or query params.
///
/// Checks `Authorization: Bearer <token>` header first, then `?token=xxx` query param.
fn extract_token(
    headers: &HeaderMap,
    params: &std::collections::HashMap<String, String>,
) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .or_else(|| params.get("token").cloned())
}

/// Extract adapter-level API key from the `X-API-Key` header (case-insensitive).
///
/// Distinct from `extract_token` — that reads the per-device `Authorization: Bearer`
/// secret. The adapter-level key is a global pre-shared secret for the whole
/// adapter, useful when the platform is exposed without per-device provisioning.
fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Get the internal webhook adapter, downcast from DeviceAdapter.
async fn get_webhook_adapter(
    state: &ServerState,
) -> Result<neomind_devices::adapters::webhook::WebhookAdapter, ErrorResponse> {
    // Phase A binds the HTTP listener before Phase B registers device
    // adapters (server/mod.rs: fast path to serving) — for the first seconds
    // after startup this lookup misses. That's a transient startup state,
    // not a server fault: answer 503 with retry guidance so devices (and
    // curl users copying the onboarding example) retry instead of treating
    // it as a broken endpoint.
    let adapter = state
        .devices
        .service
        .get_adapter("internal-webhook")
        .await
        .ok_or_else(|| {
            ErrorResponse::service_unavailable(
                "Device ingestion is still starting up — retry in a few seconds",
            )
            .with_hint("The webhook endpoint becomes available shortly after the server starts")
        })?;

    adapter
        .as_any()
        .downcast_ref::<neomind_devices::adapters::webhook::WebhookAdapter>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("Failed to downcast webhook adapter"))
}

/// Inspect Content-Type and body, return a `WebhookPayload` regardless of the
/// incoming format.
///
/// Tolerance matrix:
///
/// | Content-Type                         | Result                                                |
/// |--------------------------------------|-------------------------------------------------------|
/// | `application/json` (any charset)     | Parse as `WebhookPayload`. If the body is valid JSON  |
/// |                                      | but missing the `data` field, wrap the entire        |
/// |                                      | object: `{"data": <body>}`.                           |
/// | `image/jpeg|png|webp|gif|bmp`        | Base64-encode as a data URL:                          |
/// |                                      | `{"data": {"image": "data:image/jpeg;base64,..."}}`   |
/// | `text/plain` (any charset)           | `{"data": {"text": "<body UTF-8 lossy>"}}`            |
/// | `application/x-www-form-urlencoded`  | Parse key=value pairs, wrap as `{"data": <map>}`      |
/// | missing / `application/octet-stream` | Try JSON first (some firmwares forget the header);    |
/// |                                      | fall back to text/plain.                              |
/// | anything else                        | 415 with the actual received CT + acceptable list.    |
///
/// Why: IoT firmwares are notoriously inconsistent about Content-Type headers.
/// The original handler used axum's `Json<>` extractor which hard-rejects (415)
/// any body without `application/json`, even when the body itself is valid JSON.
/// That made onboarding real devices painful because the only error message was
/// the unhelpful "Expected request with Content-Type: application/json" with no
/// hint about what the device actually sent.
/// Inspect Content-Type. Returns `(bare_type, raw_value, content_length)`:
/// - `bare_type` = main type with parameters stripped, used for dispatch
///   (e.g. `"application/json"`, `"multipart/form-data"`).
/// - `raw_value`  = the full header value, preserved for multipart boundary
///   extraction.
fn content_type(headers: &HeaderMap) -> (Option<String>, Option<String>, usize) {
    let raw = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());
    let bare = raw
        .as_ref()
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_lowercase());
    let len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    (bare, raw, len)
}

/// Build a 415 error response with actionable diagnostics.
fn unsupported_media_type(received: Option<&str>) -> ErrorResponse {
    ErrorResponse::new(
        "UNSUPPORTED_MEDIA_TYPE",
        format!(
            "Unsupported Media Type. Received Content-Type: `{}`. Acceptable: {}. \
             Hint: devices that forget Content-Type can still send JSON — set \
             `Content-Type: application/json` manually, or POST the image as \
             `image/jpeg` and the server will wrap it automatically.",
            received.unwrap_or("(none)"),
            ACCEPTABLE_CONTENT_TYPES.join(", "),
        ),
        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
    )
}

/// Tolerant body parser — see [`content_type`] for the dispatch table.
///
/// `device_id_override` is injected by callers that already know the device_id
/// (from the URL path). When set, it overrides any `device_id` field in a parsed
/// JSON body, so URL-path routing wins over body-supplied identity.
fn parse_body(
    body: Bytes,
    bare_ct: Option<String>,
    raw_ct: Option<String>,
    content_length: usize,
    device_id_override: Option<&str>,
) -> Result<WebhookPayload, ErrorResponse> {
    // Empty body — accept silently with an empty data object. Some devices POST
    // empty bodies as heartbeat / liveness probes.
    if body.is_empty() && content_length == 0 {
        let p = WebhookPayload {
            device_id: device_id_override.map(|s| s.to_string()),
            timestamp: None,
            quality: None,
            data: serde_json::Value::Object(serde_json::Map::new()),
        };
        return Ok(p);
    }

    // Multipart/form-data — common for camera devices that ship a JSON metadata
    // blob alongside a JPEG frame. We need the RAW header value (preserving the
    // `; boundary=...` parameter) since `bare_ct` strips parameters.
    if let Some(raw) = raw_ct.as_deref() {
        if let Some(boundary) = extract_multipart_boundary(raw) {
            return parse_multipart_body(&body, &boundary, device_id_override);
        }
    }

    let ct = bare_ct.as_deref();
    let payload = match ct {
        Some("application/json") => parse_json_body(&body)?,
        Some(c) if IMAGE_MIME_TYPES.contains(&c) => {
            let data_url = encode_image_data_url(c, &body);
            serde_json::json!({ "image": data_url })
        }
        Some("text/plain")
        | Some("text/plain;charset=utf-8")
        | Some("text/plain; charset=utf-8") => {
            let text = String::from_utf8_lossy(&body);
            serde_json::json!({ "text": text.as_ref() })
        }
        Some("application/x-www-form-urlencoded") => {
            parse_form_body(&body).unwrap_or_else(|| serde_json::Value::Null)
        }
        // Missing header, or `application/octet-stream`, or anything unrecognized:
        // try JSON first (forgiving), then text/plain. The "unrecognized" bucket
        // includes firmwares that send wrong CT like `text/html` for a JSON body.
        _ => {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) {
                v
            } else {
                let text = String::from_utf8_lossy(&body);
                serde_json::json!({ "text": text.as_ref() })
            }
        }
    };

    // Wrap if the parsed JSON isn't already a WebhookPayload shape. We detect
    // "shape" loosely: if `data` is present, assume it's a proper payload; if
    // not, wrap the whole object under `data` so any valid JSON device payload
    // works (e.g. `{"temp": 23}` becomes `{"data": {"temp": 23}}`).
    let mut payload_struct: WebhookPayload = if let Some(obj) = payload.as_object() {
        if obj.contains_key("data") {
            serde_json::from_value(payload).map_err(|e| {
                ErrorResponse::bad_request(format!(
                    "Invalid WebhookPayload: {}. Expected fields: data (required), \
                     device_id/timestamp/quality (optional).",
                    e
                ))
            })?
        } else {
            WebhookPayload {
                device_id: None,
                timestamp: None,
                quality: None,
                data: payload,
            }
        }
    } else {
        // Top-level scalar (string/number/array) — wrap under data.
        WebhookPayload {
            device_id: None,
            timestamp: None,
            quality: None,
            data: payload,
        }
    };

    if let Some(id) = device_id_override {
        payload_struct.device_id = Some(id.to_string());
    }
    // Reject oversized string values — but ONLY for JSON-origin payloads.
    // Image/multipart paths encode binary → base64 intentionally and are
    // already bounded by the router's body size limit, so checking them here
    // would falsely reject legitimate 4K JPEG uploads.
    let needs_value_size_check = matches!(
        bare_ct.as_deref(),
        Some("application/json")
            | Some("application/x-www-form-urlencoded")
            | Some("application/octet-stream")
            | None
    ) || bare_ct
        .as_deref()
        .map(|c: &str| c.starts_with("text/"))
        .unwrap_or(false);
    if needs_value_size_check {
        enforce_max_string_size(&payload_struct.data, MAX_VALUE_STRING_SIZE)?;
    }
    Ok(payload_struct)
}

/// Maximum byte size of any single string value inside `payload.data` when the
/// payload originated from a JSON/text/form body. Image and multipart bodies
/// are exempt (they encode binary intentionally).
const MAX_VALUE_STRING_SIZE: usize = 2 * 1024 * 1024;

/// Walk a JSON value tree and reject any string larger than `max_bytes`.
///
/// Returns the path of the first oversized string in the error message so
/// device firmware authors can locate the offending field. Object keys are
/// NOT checked (they're almost always short); only string values are.
fn enforce_max_string_size(
    value: &serde_json::Value,
    max_bytes: usize,
) -> Result<(), ErrorResponse> {
    fn walk(v: &serde_json::Value, max_bytes: usize, path: &str) -> Result<(), ErrorResponse> {
        match v {
            serde_json::Value::String(s) => {
                if s.len() > max_bytes {
                    return Err(ErrorResponse::bad_request(format!(
                        "Webhook rejected: string value at '{}' is {} bytes (max {}). \
                         Embed large images as multipart/form-data instead of base64 in JSON, \
                         or downscale before uploading.",
                        if path.is_empty() { "$" } else { path },
                        s.len(),
                        max_bytes
                    )));
                }
            }
            serde_json::Value::Object(obj) => {
                for (k, child) in obj {
                    let child_path = if path.is_empty() {
                        format!("$.{}", k)
                    } else {
                        format!("{}.{}", path, k)
                    };
                    walk(child, max_bytes, &child_path)?;
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, child) in arr.iter().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    walk(child, max_bytes, &child_path)?;
                }
            }
            // Numbers / bools / null — no size concern.
            _ => {}
        }
        Ok(())
    }
    walk(value, max_bytes, "")
}

fn parse_json_body(body: &[u8]) -> Result<serde_json::Value, ErrorResponse> {
    serde_json::from_slice::<serde_json::Value>(body).map_err(|e| {
        ErrorResponse::bad_request(format!(
            "Invalid JSON body: {}. Make sure the body is valid UTF-8 JSON.",
            e
        ))
    })
}

fn parse_form_body(body: &[u8]) -> Option<serde_json::Value> {
    let s = std::str::from_utf8(body).ok()?;
    let mut map = serde_json::Map::new();
    for (k, v) in urlencoding::decode(s).ok()?.split('&').filter_map(|pair| {
        let mut it = pair.splitn(2, '=');
        let k = it.next()?.to_string();
        let v = it.next().unwrap_or("").to_string();
        Some((k, v))
    }) {
        map.insert(k, serde_json::Value::String(v));
    }
    Some(serde_json::Value::Object(map))
}

fn encode_image_data_url(mime: &str, body: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine};
    let normalized = if mime == "image/jpg" {
        "image/jpeg"
    } else {
        mime
    };
    let b64 = general_purpose::STANDARD.encode(body);
    format!("data:{};base64,{}", normalized, b64)
}

/// Extract the multipart boundary value from a Content-Type header value.
///
/// Accepts `multipart/form-data; boundary=----WebKitFormBoundary...` (and the
/// RFC-compliant quoted variant `boundary="..."`). Returns None for non-multipart
/// content types or missing boundary.
///
/// **Validation**: RFC 2046 restricts boundary to 1-70 chars from a limited
/// charset. We reject boundaries containing control chars, whitespace, or
/// tspecials — this prevents parser confusion (e.g. `\r\n` in boundary) and
/// DoS via megabyte-long boundaries that would make `windows()` search quadratic.
fn extract_multipart_boundary(ct: &str) -> Option<String> {
    if !ct.to_lowercase().starts_with("multipart/form-data") {
        return None;
    }
    for part in ct.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("boundary=") {
            let v = rest.trim().trim_matches('"');
            if !v.is_empty() && is_valid_boundary(v) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// RFC 2046 boundary validation: 1-70 chars, no control/whitespace/tspecials.
fn is_valid_boundary(b: &str) -> bool {
    if b.is_empty() || b.len() > 70 {
        return false;
    }
    // tspecials per RFC 2046: ( ) < > @ , ; : \ " / [ ] ? = { }
    // plus control chars and whitespace are forbidden.
    b.bytes().all(|c| {
        c > 0x20
            && c < 0x7f
            && !matches!(
                c,
                b'(' | b')'
                    | b'<'
                    | b'>'
                    | b'@'
                    | b','
                    | b';'
                    | b':'
                    | b'\\'
                    | b'"'
                    | b'/'
                    | b'['
                    | b']'
                    | b'?'
                    | b'='
                    | b'{'
                    | b'}'
            )
    })
}

/// Find a subslice within `haystack[search_start..]`. Returns absolute index.
fn find_subslice(haystack: &[u8], needle: &[u8], search_start: usize) -> Option<usize> {
    if needle.is_empty() || search_start >= haystack.len() {
        return None;
    }
    haystack[search_start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + search_start)
}

/// A parsed multipart part — see [`parse_multipart_body`].
struct MultipartPart {
    name: Option<String>,
    content_type: String,
    data: Vec<u8>,
}

/// Parse a multipart/form-data body given a boundary.
///
/// Tolerates common firmware quirks: missing Content-Disposition name, missing
/// Content-Type (defaults to application/octet-stream), CRLF/LF line endings.
/// Each part's raw bytes are returned; consumers decide how to decode them.
///
/// **Hard caps**: at most `MAX_MULTIPART_PARTS` parts (DoS guard). Total body
/// size is already bounded by the router's `DefaultBodyLimit`.
fn parse_multipart_parts(body: &[u8], boundary: &str) -> Result<Vec<MultipartPart>, ErrorResponse> {
    const MAX_MULTIPART_PARTS: usize = 64;

    let delimiter = format!("--{}", boundary);
    let delim_b = delimiter.as_bytes();
    let mut parts = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel) = body[cursor..]
        .windows(delim_b.len())
        .position(|w| w == delim_b)
    {
        let abs = cursor + rel;
        // Advance past delimiter
        let mut p = abs + delim_b.len();
        // Stop at closing delimiter `--boundary--`
        if body.get(p..p + 2) == Some(b"--") {
            break;
        }
        // Skip CRLF (or LF)
        if body.get(p..p + 2) == Some(b"\r\n") {
            p += 2;
        } else if body.get(p..p + 1) == Some(b"\n") {
            p += 1;
        }
        // Find end of part headers
        let header_end = find_subslice(body, b"\r\n\r\n", p)
            .or_else(|| find_subslice(body, b"\n\n", p))
            .ok_or_else(|| {
                ErrorResponse::bad_request("Malformed multipart: missing header terminator")
            })?;
        let sep_len = if body.get(header_end..header_end + 4) == Some(b"\r\n\r\n") {
            4
        } else {
            2
        };
        let header_bytes = &body[p..header_end];
        let content_start = header_end + sep_len;

        // Find next delimiter (preceded by \r\n or \n)
        let next_delim = find_subslice(body, delim_b, content_start).ok_or_else(|| {
            ErrorResponse::bad_request("Malformed multipart: missing part terminator")
        })?;
        // Walk back trailing CRLF/LF that's part of framing, not content
        let mut content_end = next_delim;
        if content_end >= 2 && &body[content_end - 2..content_end] == b"\r\n" {
            content_end -= 2;
        } else if content_end >= 1 && &body[content_end - 1..content_end] == b"\n" {
            content_end -= 1;
        }
        let content = &body[content_start..content_end];

        // Parse part headers — only care about Content-Disposition name and Content-Type
        let mut part_name = None;
        let mut part_ct = "application/octet-stream".to_string();
        for line in header_bytes.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if let Some(rest) = line
                .strip_prefix(b"Content-Disposition:")
                .or_else(|| line.strip_prefix(b"content-disposition:"))
            {
                let s = String::from_utf8_lossy(rest);
                for field in s.split(';') {
                    let f = field.trim();
                    if let Some(v) = f.strip_prefix("name=\"") {
                        part_name = Some(v.trim_end_matches('"').to_string());
                    } else if let Some(v) = f.strip_prefix("name=") {
                        part_name = Some(v.to_string());
                    }
                }
            } else if let Some(rest) = line
                .strip_prefix(b"Content-Type:")
                .or_else(|| line.strip_prefix(b"content-type:"))
            {
                part_ct = String::from_utf8_lossy(rest).trim().to_lowercase();
            }
        }

        parts.push(MultipartPart {
            name: part_name,
            content_type: part_ct,
            data: content.to_vec(),
        });
        if parts.len() >= MAX_MULTIPART_PARTS {
            return Err(ErrorResponse::bad_request(format!(
                "Multipart body exceeds maximum of {} parts",
                MAX_MULTIPART_PARTS
            )));
        }
        cursor = next_delim;
    }

    if parts.is_empty() {
        return Err(ErrorResponse::bad_request(
            "Malformed multipart body: no parts found (boundary mismatch?)",
        ));
    }
    Ok(parts)
}

/// Parse multipart body into a `WebhookPayload`.
///
/// Resolution strategy:
/// 1. If a part named `metadata` (Content-Type: application/json) exists, use it
///    as the base `WebhookPayload` — its `data` field becomes the carrier.
/// 2. Otherwise, if exactly one JSON part exists, use it as the base.
/// 3. Otherwise, start with an empty `data: {}` object.
/// 4. Then overlay remaining parts:
///    - Image parts (Content-Type: image/*) → keyed by the part's `name=`
///      parameter when present (so devices can target a specific device-type
///      metric, e.g. `name="frame"` → `data.frame`). Parts without a name fall
///      back to `data.image` / `data.image_2` / `data.image_3` ... preserving
///      the multi-frame convention.
///    - **First-frame aliasing**: the first image part is also exposed under
///      the well-known camera-template metric names `image_data`, `frame`,
///      `snapshot` (only when those keys aren't already set by the metadata
///      JSON). This bridges firmware that sends `name="image"` or unnamed parts
///      with templates like `ne301_camera` whose metric is `image_data` —
///      without requiring either side to be modified. Subsequent frames are
///      NOT aliased to avoid conflating multi-frame payloads.
///    - JSON parts with unknown names → merged into `data` if it's an object,
///      else stored under `data.<name>`.
///    - Text parts → `data.<name_or_"text">`.
fn parse_multipart_body(
    body: &[u8],
    boundary: &str,
    device_id_override: Option<&str>,
) -> Result<WebhookPayload, ErrorResponse> {
    let parts = parse_multipart_parts(body, boundary)?;

    // Diagnostic log: helps onboarding camera devices by showing what part names
    // the firmware actually sends. Match these against the device-type template's
    // metric names — an image part named `image` won't populate a template metric
    // named `image_data`; firmware must use `name="image_data"` (or whatever the
    // template defines) so the unified extractor can route it.
    tracing::info!(
        target: "neomind::api::webhook::multipart",
        part_count = parts.len(),
        parts = ?parts.iter().map(|p| (
            p.name.as_deref().unwrap_or("<none>"),
            p.content_type.as_str(),
            p.data.len(),
        )).collect::<Vec<_>>(),
        "multipart parts received"
    );

    let mut base: Option<WebhookPayload> = None;
    let mut image_count = 0usize;

    // First pass: pick the metadata/JSON base.
    for part in &parts {
        let is_metadata_name = part.name.as_deref() == Some("metadata")
            || part.name.as_deref() == Some("meta")
            || part.name.as_deref() == Some("payload");
        let is_json = part.content_type == "application/json"
            || part.content_type.starts_with("application/json;");
        if is_metadata_name && is_json {
            base = Some(parse_json_payload(&part.data, device_id_override)?);
        }
    }
    // Fallback: single unnamed/first JSON part becomes the base.
    if base.is_none() {
        for part in &parts {
            let is_json = part.content_type == "application/json"
                || part.content_type.starts_with("application/json;");
            if is_json {
                base = Some(parse_json_payload(&part.data, device_id_override)?);
                // Other parts become "extra" — handled in second pass below.
                break;
            }
        }
    }

    let mut payload = base.unwrap_or_else(|| WebhookPayload {
        device_id: device_id_override.map(|s| s.to_string()),
        timestamp: None,
        quality: None,
        data: serde_json::Value::Object(serde_json::Map::new()),
    });

    // Ensure payload.data is an object so we can attach extras.
    if !payload.data.is_object() {
        let prev = std::mem::replace(
            &mut payload.data,
            serde_json::Value::Object(serde_json::Map::new()),
        );
        payload.data["value"] = prev;
    }
    let data_obj = payload.data.as_object_mut().expect("just ensured object");

    // Second pass: overlay remaining parts.
    for part in &parts {
        let is_json = part.content_type == "application/json"
            || part.content_type.starts_with("application/json;");
        let is_image = IMAGE_MIME_TYPES.contains(&part.content_type.as_str())
            || part.content_type.starts_with("image/");
        let is_metadata_name = part.name.as_deref() == Some("metadata")
            || part.name.as_deref() == Some("meta")
            || part.name.as_deref() == Some("payload");

        if is_image {
            // Prefer the part's `name=` parameter as the data key so devices can
            // target a specific metric defined in their device type (e.g. a camera
            // template with metric `frame` POSTs `name="frame"`). Fall back to
            // `image` / `image_2` / `image_3` ... only when the part has no name,
            // preserving the original multi-frame convention.
            image_count += 1;
            let key = part.name.clone().unwrap_or_else(|| {
                if image_count == 1 {
                    "image".to_string()
                } else {
                    format!("image_{}", image_count)
                }
            });
            let data_url = encode_image_data_url(&part.content_type, &part.data);

            // Aliasing for the FIRST image part: also expose it under the
            // well-known camera-template metric names (`image_data`, `frame`,
            // `snapshot`) so device-type templates don't silently miss the image
            // just because the firmware used a different part name. We do NOT
            // overwrite existing keys (e.g. if metadata JSON already provided
            // `image_data`, the binary part respects it). Only the first image
            // gets aliases — subsequent frames stay under their unique keys
            // (`image_2`, etc.) so multi-frame devices don't conflate them.
            //
            // `__webhook_image` is a SYSTEM metric (double-underscore prefix):
            // the unified extractor passes it through regardless of the
            // device-type template, so the image is always recoverable in the
            // frontend even when no template metric name matches. This mirrors
            // the `__last_seen_age_secs` system-metric convention.
            if image_count == 1 {
                for alias in ["__webhook_image", "image_data", "frame", "snapshot"] {
                    data_obj
                        .entry(alias.to_string())
                        .or_insert(serde_json::Value::String(data_url.clone()));
                }
            }

            data_obj.insert(key, serde_json::Value::String(data_url));
            continue;
        }

        // Skip the JSON part that was already used as base.
        if is_json && (is_metadata_name || data_obj.is_empty()) {
            // If it was the metadata part, merge any fields that aren't already set;
            // if it's the only JSON and we used it as base, also skip.
            if is_metadata_name {
                continue;
            }
            // Single-JSON-base case: already consumed. But if base.data was a scalar,
            // we wrapped it under "value" — keep the JSON intact.
            if data_obj.contains_key("value") {
                continue;
            }
        }

        // Other JSON parts: try to merge as object, otherwise store by name.
        if is_json {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&part.data) {
                if let Some(obj) = v.as_object() {
                    for (k, val) in obj {
                        data_obj.entry(k.clone()).or_insert(val.clone());
                    }
                    continue;
                }
                let key = part.name.clone().unwrap_or_else(|| "value".to_string());
                data_obj.insert(key, v);
                continue;
            }
        }

        // Text / octet-stream: store as UTF-8 string under part name.
        let text = String::from_utf8_lossy(&part.data).into_owned();
        let key = part.name.clone().unwrap_or_else(|| "text".to_string());
        data_obj.insert(key, serde_json::Value::String(text));
    }

    if let Some(id) = device_id_override {
        payload.device_id = Some(id.to_string());
    }
    Ok(payload)
}

/// Parse a JSON slice into a WebhookPayload, wrapping if necessary. Shared with
/// the non-multipart JSON path — tolerates both `{...}` with `data` and bare
/// payloads like `{"temp": 23}` (wrapped to `{"data": {"temp": 23}}`).
fn parse_json_payload(
    body: &[u8],
    device_id_override: Option<&str>,
) -> Result<WebhookPayload, ErrorResponse> {
    let v = parse_json_body(body)?;
    let mut p: WebhookPayload = if let Some(obj) = v.as_object() {
        if obj.contains_key("data") {
            serde_json::from_value(v).map_err(|e| {
                ErrorResponse::bad_request(format!(
                    "Invalid WebhookPayload (metadata part): {}. Expected: data (required), \
                     device_id/timestamp/quality (optional).",
                    e
                ))
            })?
        } else {
            WebhookPayload {
                device_id: None,
                timestamp: None,
                quality: None,
                data: v,
            }
        }
    } else {
        WebhookPayload {
            device_id: None,
            timestamp: None,
            quality: None,
            data: v,
        }
    };
    if let Some(id) = device_id_override {
        p.device_id = Some(id.to_string());
    }
    Ok(p)
}

/// Handle webhook POST from device.
///
/// Endpoint: `POST /api/devices/:id/webhook`
///
/// Content-Type tolerant (see `parse_body`): JSON, image/*, text/plain,
/// form-urlencoded, and missing-CT-with-JSON-body are all accepted.
/// Auth options (checked by the adapter, all optional individually):
/// - `Authorization: Bearer <token>` or `?token=xxx` — per-device webhook token
/// - `X-API-Key: <key>` — adapter-level pre-shared key (only enforced if configured)
pub async fn webhook_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    body: Bytes,
) -> HandlerResult<serde_json::Value> {
    let (ct, raw_ct, len) = content_type(&headers);

    // Reject genuinely unsupported Content-Types up front (so we don't waste
    // work base64-encoding a 10MB multipart upload that we can't handle anyway).
    if let Some(ref c) = ct {
        let is_supported = c == "application/json"
            || IMAGE_MIME_TYPES.contains(&c.as_str())
            || c.starts_with("text/")
            || c.starts_with("multipart/form-data");
        if !is_supported
            && c != "application/x-www-form-urlencoded"
            && c != "application/octet-stream"
        {
            warn!(
                device_id = %device_id,
                content_type = %c,
                content_length = len,
                "Webhook rejected: unsupported Content-Type"
            );
            return Err(unsupported_media_type(Some(c)));
        }
    }

    let payload = parse_body(body, ct.clone(), raw_ct.clone(), len, Some(&device_id))?;

    let adapter = get_webhook_adapter(&state).await?;
    let token = extract_token(&headers, &params);
    let api_key = extract_api_key(&headers);
    let remote_ip = connect_info.map(|ci| ci.0.ip());

    let metrics_count = adapter
        .process_webhook(
            device_id.clone(),
            payload,
            token.as_deref(),
            api_key.as_deref(),
            remote_ip.as_ref(),
        )
        .await
        .map_err(|e| {
            tracing::warn!(device_id = %device_id, error = %e, "Webhook processing failed");
            match e {
                neomind_devices::adapter::AdapterError::Connection(msg) => {
                    ErrorResponse::unauthorized(msg)
                }
                neomind_devices::adapter::AdapterError::Configuration(msg) => {
                    ErrorResponse::bad_request(msg)
                }
                _ => ErrorResponse::internal("Webhook processing failed"),
            }
        })?;

    info!(
        device_id = %device_id,
        metrics_count,
        content_type = ct.unwrap_or_else(|| "(none)".into()),
        "Webhook data processed"
    );

    ok(serde_json::json!({
        "success": true,
        "device_id": device_id,
        "metrics_received": metrics_count,
    }))
}

/// Handle webhook POST from device (alternative endpoint without device_id in URL).
///
/// Endpoint: `POST /api/devices/webhook`
///
/// Content-Type tolerant — same matrix as `webhook_handler`.
///
/// For JSON/text/form bodies, the `device_id` MUST be present in the body
/// (top-level `device_id` field). For image uploads, this endpoint cannot
/// identify the device (no body JSON) → reject with 400.
///
/// NOTE: The body-supplied `device_id` is attacker-controllable. Deployments that
/// need strong device identity should configure either per-device `webhook_token`s
/// or an adapter-level `X-API-Key`. On closed LAN deployments, leave both unset —
/// the route's rate limit plus per-IP discovery throttle is the only defense.
pub async fn webhook_generic_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    body: Bytes,
) -> HandlerResult<serde_json::Value> {
    let (ct, raw_ct, len) = content_type(&headers);

    if let Some(ref c) = ct {
        let is_supported = c == "application/json"
            || IMAGE_MIME_TYPES.contains(&c.as_str())
            || c.starts_with("text/")
            || c.starts_with("multipart/form-data");
        if !is_supported
            && c != "application/x-www-form-urlencoded"
            && c != "application/octet-stream"
        {
            return Err(unsupported_media_type(Some(c)));
        }
    }

    // The generic endpoint needs device_id from the body, so for image uploads
    // (which have no JSON envelope) we cannot identify the device.
    if let Some(ref c) = ct {
        if IMAGE_MIME_TYPES.contains(&c.as_str()) {
            return Err(ErrorResponse::bad_request(
                "Image uploads must use the path-based endpoint \
                 `POST /api/devices/:id/webhook` so the device can be identified.",
            ));
        }
    }

    let mut payload = parse_body(body, ct.clone(), raw_ct.clone(), len, None)?;

    let device_id = payload
        .device_id
        .take()
        .ok_or_else(|| ErrorResponse::bad_request("device_id is required in request body"))?;

    let adapter = get_webhook_adapter(&state).await?;
    let token = extract_token(&headers, &params);
    let api_key = extract_api_key(&headers);
    let remote_ip = connect_info.map(|ci| ci.0.ip());

    payload.device_id = Some(device_id.clone());

    let metrics_count = adapter
        .process_webhook(
            device_id.clone(),
            payload,
            token.as_deref(),
            api_key.as_deref(),
            remote_ip.as_ref(),
        )
        .await
        .map_err(|e| {
            tracing::warn!(device_id = %device_id, error = %e, "Webhook processing failed");
            match e {
                neomind_devices::adapter::AdapterError::Connection(msg) => {
                    ErrorResponse::unauthorized(msg)
                }
                neomind_devices::adapter::AdapterError::Configuration(msg) => {
                    ErrorResponse::bad_request(msg)
                }
                _ => ErrorResponse::internal("Webhook processing failed"),
            }
        })?;

    info!(
        device_id = %device_id,
        metrics_count,
        content_type = ct.unwrap_or_else(|| "(none)".into()),
        "Webhook data processed (generic endpoint)"
    );

    ok(serde_json::json!({
        "success": true,
        "device_id": device_id,
        "metrics_processed": metrics_count,
    }))
}

/// Get webhook URL for a device.
///
/// Returns the URL that devices should POST to. Lives behind the hybrid auth
/// middleware (moved out of `public_routes`) — it's an admin/UI lookup, not
/// something devices call, and the previous public placement leaked device
/// existence (404 vs 200) plus the server's configured `NEOMIND_SERVER_URL`.
pub async fn get_webhook_url_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<serde_json::Value> {
    // Verify device exists
    if state.devices.service.get_device(&device_id).is_none() {
        return Err(ErrorResponse::not_found(format!(
            "Device {} not found",
            device_id
        )));
    }

    let (server_url, url_source) = crate::handlers::common::resolve_server_url(Some(&headers));

    let mut payload = serde_json::json!({
        "device_id": device_id,
        "webhook_url": format!("{}/api/devices/{}/webhook", server_url, device_id),
        "alternative_url": format!("{}/api/devices/webhook", server_url),
        "method": "POST",
        "content_type": "application/json",
        "payload_example": {
            "timestamp": 1234567890,
            "quality": 1.0,
            "data": {
                "temperature": 23.5,
                "humidity": 65
            }
        },
        "url_source": url_source.as_str(),
    });

    if url_source == crate::handlers::common::ServerUrlSource::Fallback {
        payload["hint"] = serde_json::json!(
            "Set NEOMIND_SERVER_URL env var (or run behind a proxy that sends \
             X-Forwarded-Proto + Host) — the returned URL is a placeholder."
        );
    }

    ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a multipart/form-data body matching what the customer's camera
    /// firmware sends: a `metadata` JSON part + an `image` JPEG binary part.
    fn build_camera_multipart(boundary: &str, metadata_json: &str, image_bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        out.extend_from_slice(
            b"Content-Disposition: form-data; name=\"metadata\"\r\n\
              Content-Type: application/json\r\n\r\n",
        );
        out.extend_from_slice(metadata_json.as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        out.extend_from_slice(
            b"Content-Disposition: form-data; name=\"image\"; filename=\"frame.jpg\"\r\n\
              Content-Type: image/jpeg\r\n\r\n",
        );
        out.extend_from_slice(image_bytes);
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
        out
    }

    #[test]
    fn parses_camera_multipart_metadata_plus_image() {
        use base64::Engine;
        let boundary = "----testBoundaryXYZ";
        let metadata =
            r#"{"device_id":"cam-001","timestamp":1735900000,"data":{"width":640,"height":480}}"#;
        let image = b"\xff\xd8\xff\xe0FAKEJPEGBYTES\xff\xd9";
        let body = build_camera_multipart(boundary, metadata, image);

        let payload = parse_multipart_body(&body, boundary, Some("override-id")).expect("parse ok");

        assert_eq!(payload.device_id.as_deref(), Some("override-id"));
        assert_eq!(payload.timestamp, Some(1735900000));

        let data = payload.data.as_object().expect("data is object");
        assert_eq!(data.get("width").and_then(|v| v.as_u64()), Some(640));
        assert_eq!(data.get("height").and_then(|v| v.as_u64()), Some(480));

        let img = data
            .get("image")
            .and_then(|v| v.as_str())
            .expect("image present");
        assert!(img.starts_with("data:image/jpeg;base64,"));
        // Verify base64 round-trips back to the original bytes.
        let b64 = &img["data:image/jpeg;base64,".len()..];
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("b64 decode");
        assert_eq!(decoded, image);
    }

    #[test]
    fn parses_multipart_image_only_wraps_under_empty_data() {
        let boundary = "B";
        let mut body = Vec::new();
        body.extend_from_slice(b"--B\r\n");
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"frame\"; filename=\"f.jpg\"\r\n\
              Content-Type: image/jpeg\r\n\r\n",
        );
        body.extend_from_slice(b"\xff\xd8\xff\xd9");
        body.extend_from_slice(b"\r\n--B--\r\n");

        let payload = parse_multipart_body(&body, boundary, Some("cam-2")).expect("parse ok");
        assert_eq!(payload.device_id.as_deref(), Some("cam-2"));
        let data = payload.data.as_object().expect("object");
        // When the part has a name, it is preserved as the data key so device-type
        // templates can target a specific metric (e.g. `frame`).
        let img = data
            .get("frame")
            .and_then(|v| v.as_str())
            .expect("frame key");
        assert!(img.starts_with("data:image/jpeg;base64,"));
    }

    /// Device-type templates often name their image metric explicitly (e.g.
    /// `snapshot` / `frame` / `photo`). The part's `name=` parameter must be
    /// honored as the data key so the unified extractor can route the image
    /// into the correct metric. Regression test for the camera-onboarding
    /// bug where images were always written to `data.image` regardless of the
    /// part name, causing device-type templates to silently miss the frame.
    #[test]
    fn parses_multipart_image_part_name_preserved_as_data_key() {
        let boundary = "boundary123";
        let metadata = r#"{"device_id":"cam-X","data":{"width":1920}}"#;
        let image = b"\xff\xd8\xff\xe0TESTJPEG\xff\xd9";

        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"metadata\"\r\n\
              Content-Type: application/json\r\n\r\n",
        );
        body.extend_from_slice(metadata.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        // Part name "snapshot" — what the device-type template expects.
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"snapshot\"; filename=\"frame.jpg\"\r\n\
              Content-Type: image/jpeg\r\n\r\n",
        );
        body.extend_from_slice(image);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let payload = parse_multipart_body(&body, boundary, None).expect("parse ok");
        let data = payload.data.as_object().expect("data is object");
        // Image must land under the part's name, NOT under the hardcoded "image".
        assert!(
            data.contains_key("snapshot"),
            "expected key 'snapshot', got: {:?}",
            data.keys().collect::<Vec<_>>()
        );
        assert!(
            !data.contains_key("image"),
            "hardcoded 'image' key should NOT be set when part has a name"
        );
        let img = data
            .get("snapshot")
            .and_then(|v| v.as_str())
            .expect("snapshot value");
        assert!(img.starts_with("data:image/jpeg;base64,"));
        // Metadata fields still merged alongside.
        assert_eq!(data.get("width").and_then(|v| v.as_u64()), Some(1920));
    }

    /// Regression test for the NE301/NE302 onboarding bug: firmware sends
    /// `name="image"` but the device-type template expects `image_data`.
    /// The parser must alias the first image to the well-known camera metric
    /// names so the unified extractor can route it without firmware changes.
    #[test]
    fn first_image_aliased_to_well_known_camera_metric_names() {
        let boundary = "b";
        let metadata = r#"{"device_id":"cam-NE301","data":{"width":1280}}"#;
        let image = b"\xff\xd8\xff\xe0JPEG\xff\xd9";

        let body = build_camera_multipart(boundary, metadata, image);
        let payload = parse_multipart_body(&body, boundary, None).expect("parse ok");
        let data = payload.data.as_object().expect("data object");

        // Primary key (from `name="image"` in the test helper).
        assert!(data.contains_key("image"));
        // Aliases populated for camera template compatibility.
        let aliased = data
            .get("image_data")
            .and_then(|v| v.as_str())
            .expect("image_data alias");
        assert!(aliased.starts_with("data:image/jpeg;base64,"));
        assert!(
            data.contains_key("frame"),
            "frame alias missing: {:?}",
            data.keys().collect::<Vec<_>>()
        );
        assert!(
            data.contains_key("snapshot"),
            "snapshot alias missing: {:?}",
            data.keys().collect::<Vec<_>>()
        );
        // All aliases point to the same data URL.
        assert_eq!(aliased, data.get("image").and_then(|v| v.as_str()).unwrap());
        // Metadata still merged.
        assert_eq!(data.get("width").and_then(|v| v.as_u64()), Some(1280));
    }

    #[test]
    fn extracts_boundary_from_content_type() {
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=----WebKitFormBoundaryABC"),
            Some("----WebKitFormBoundaryABC".to_string())
        );
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=\"quotedBoundary\""),
            Some("quotedBoundary".to_string())
        );
        assert_eq!(extract_multipart_boundary("application/json"), None);
        assert_eq!(extract_multipart_boundary("multipart/form-data"), None);
    }

    #[test]
    fn rejects_malicious_boundaries() {
        // RFC 2046: boundary must be 1-70 chars, no control/whitespace/tspecials.
        // These attack-shaped boundaries should all be rejected.
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary="),
            None
        );
        // CRLF injection attempt
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=foo\r\n--evil"),
            None
        );
        // Whitespace inside boundary
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=has space"),
            None
        );
        // Tspecials
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=foo(bar)"),
            None
        );
        assert_eq!(
            extract_multipart_boundary("multipart/form-data; boundary=a/b"),
            None
        );
        // Over 70 chars
        let long_boundary = "x".repeat(71);
        assert_eq!(
            extract_multipart_boundary(&format!("multipart/form-data; boundary={}", long_boundary)),
            None
        );
        // 70 chars is OK
        let ok_boundary = "x".repeat(70);
        assert_eq!(
            extract_multipart_boundary(&format!("multipart/form-data; boundary={}", ok_boundary)),
            Some(ok_boundary)
        );
    }

    #[test]
    fn rejects_too_many_multipart_parts() {
        // 65 parts should trigger the MAX_MULTIPART_PARTS=64 cap.
        let boundary = "B";
        let mut body = Vec::new();
        for i in 0..65 {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"p{}\"\r\n\r\nv{}\r\n",
                    i, i
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(b"--B--\r\n");
        let result = parse_multipart_parts(&body, boundary);
        assert!(result.is_err(), "body with 65 parts must be rejected");
    }

    #[test]
    fn json_with_oversized_string_is_rejected() {
        // A 3MB base64-like string embedded in JSON should fail the 2MB cap.
        let huge_value = "x".repeat(3 * 1024 * 1024);
        let json_body = format!(r#"{{"data": {{"image": "{}"}}}}"#, huge_value);
        let len = json_body.len();
        let result = parse_body(
            json_body.into(),
            Some("application/json".to_string()),
            Some("application/json".to_string()),
            len,
            None,
        );
        let err = result.expect_err("3MB string in JSON must be rejected");
        // The error should mention the path so the device firmware dev can find it.
        assert!(
            err.message.contains("image"),
            "error must mention field path"
        );
        assert!(err.message.contains("max"), "error must mention size limit");
    }

    #[test]
    fn json_with_normal_strings_passes() {
        // Small strings scattered through nested objects should pass.
        let json = r#"{"data":{"name":"sensor-1","note":"hello","nested":{"k":"v"}}}"#;
        let bytes = json.as_bytes().to_vec();
        let result = parse_body(
            bytes.into(),
            Some("application/json".to_string()),
            Some("application/json".to_string()),
            json.len(),
            None,
        );
        assert!(result.is_ok(), "small strings should pass");
    }

    #[test]
    fn raw_image_upload_skips_value_size_check() {
        // A raw 3MB JPEG POST should NOT be rejected by the string-size guard
        // (only JSON-origin payloads get checked). The base64-encoded data URL
        // ends up >2MB but that's intentional.
        let fake_jpeg = vec![0xFFu8; 3 * 1024 * 1024];
        let len = fake_jpeg.len();
        let result = parse_body(
            fake_jpeg.into(),
            Some("image/jpeg".to_string()),
            Some("image/jpeg".to_string()),
            len,
            Some("cam-1"),
        );
        let payload = result.expect("raw image upload must skip value-size check");
        let data = payload.data.as_object().expect("object");
        let img = data.get("image").and_then(|v| v.as_str()).expect("image");
        assert!(img.starts_with("data:image/jpeg;base64,"));
        assert!(
            img.len() > 2 * 1024 * 1024,
            "base64 should exceed the JSON limit"
        );
    }

    #[test]
    fn rejects_malformed_multipart_missing_terminator() {
        // Missing closing boundary — should error.
        let body = b"--B\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\nhi";
        let result = parse_multipart_body(body, "B", None);
        assert!(result.is_err());
    }
}
