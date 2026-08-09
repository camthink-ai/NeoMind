//! Image editing tool — crop, draw shapes/text/polygons, and blur regions.
//!
//! Returns an absolute file path under `data/images/` that the `vision` tool
//! (or another `image_edit` call) can consume directly. Operations are applied
//! as an in-memory RGBA pipeline; output is written atomically only after all
//! ops succeed.
//!
//! See `docs/superpowers/specs/2026-07-05-image-edit-tool-design.md` for the
//! full design contract. Notable invariants:
//! - `path` field in the response is absolute (vision.rs:263 requires this).
//! - File is written via current_dir().join() — NOT canonicalize — to avoid
//!   the macOS `/private/tmp → /var/` blocklist trap.

use crate::toolkit::error::{Result, ToolError};
use crate::toolkit::timeouts;
use crate::toolkit::tool::{Tool, ToolOutput};
use neomind_core::tools::ToolCategory;
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

/// Color newtype with hex deserialization support.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Color(#[serde(deserialize_with = "deserialize_color")] pub image::Rgba<u8>);

/// Deserialize hex color `#RRGGBB` or `#RRGGBBAA` (prefix `#` optional).
fn deserialize_color<'de, D>(deserializer: D) -> std::result::Result<image::Rgba<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    let hex = s.strip_prefix('#').unwrap_or(&s);
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(D::Error::custom)?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(D::Error::custom)?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(D::Error::custom)?;
    let a = if hex.len() == 8 {
        u8::from_str_radix(&hex[6..8], 16).map_err(D::Error::custom)?
    } else {
        255
    };
    Ok(image::Rgba([r, g, b, a]))
}

/// Individual image editing operation (crop, draw, blur, etc.).
/// Internally tagged via `type` field — {"type":"crop","x":10,"y":10,...}.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Operation {
    #[serde(rename = "crop")]
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    #[serde(rename = "draw_rect")]
    DrawRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(
            default = "default_stroke_width",
            deserialize_with = "clamp_stroke_width"
        )]
        stroke_width: u32,
        #[serde(default)]
        fill: Option<Color>,
    },
    #[serde(rename = "draw_circle")]
    DrawCircle {
        cx: i32,
        cy: i32,
        radius: u32,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(
            default = "default_stroke_width",
            deserialize_with = "clamp_stroke_width"
        )]
        stroke_width: u32,
        #[serde(default)]
        fill: Option<Color>,
    },
    #[serde(rename = "draw_line")]
    DrawLine {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(
            default = "default_stroke_width",
            deserialize_with = "clamp_stroke_width"
        )]
        stroke_width: u32,
    },
    #[serde(rename = "draw_arrow")]
    DrawArrow {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(
            default = "default_stroke_width",
            deserialize_with = "clamp_stroke_width"
        )]
        stroke_width: u32,
        #[serde(
            default = "default_head_length",
            deserialize_with = "clamp_head_length"
        )]
        head_length: u32,
    },
    #[serde(rename = "draw_polygon")]
    DrawPolygon {
        points: Vec<PolygonPoint>,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(
            default = "default_stroke_width",
            deserialize_with = "clamp_stroke_width"
        )]
        stroke_width: u32,
        #[serde(default)]
        fill: Option<Color>,
        #[serde(default = "default_true")]
        closed: bool,
    },
    #[serde(rename = "draw_text")]
    DrawText {
        x: i32,
        y: i32,
        text: String,
        #[serde(default = "default_rect_color")]
        color: Color,
        #[serde(default = "default_font_size", deserialize_with = "clamp_font_size")]
        font_size: u32,
        #[serde(default)]
        background: Option<Color>,
        #[serde(default = "default_padding", deserialize_with = "clamp_padding")]
        padding: u32,
    },
    #[serde(rename = "blur_rect")]
    BlurRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        #[serde(default)]
        mode: Option<BlurMode>,
        intensity: Option<u32>,
    },
    // other variants added in later tasks — fields inlined per variant,
    // NO #[serde(flatten)] (incompatible with internally-tagged enums).
}

/// Point for polygon drawing.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PolygonPoint {
    x: i32,
    y: i32,
}

/// Blur mode for region blurring.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlurMode {
    Pixelate,
    Gaussian,
}

/// Output image format for the encoded result.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
pub enum OutputFormat {
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "webp")]
    Webp,
}

fn default_format() -> OutputFormat {
    OutputFormat::Png
}

/// Top-level parameters for the `image_edit` tool.
#[derive(Debug, serde::Deserialize)]
pub struct ImageEditParams {
    pub image: String,
    pub operations: Vec<Operation>,
    #[serde(default = "default_format")]
    pub output_format: OutputFormat,
    #[serde(default)]
    pub output_filename: Option<String>,
    #[serde(default)]
    pub include_base64: bool,
}

/// Maximum number of operations in a single `image_edit` call.
const MAX_OPERATIONS: usize = 50;
/// Maximum decoded image dimension (width or height) in pixels.
const MAX_DIM: u32 = 8000;
/// Maximum input image size in bytes (10 MB). Matches `vision::MAX_IMAGE_SIZE`.
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Default functions for serde defaults.
fn default_rect_color() -> Color {
    Color(image::Rgba([255, 0, 0, 255]))
}
fn default_stroke_width() -> u32 {
    2
}
fn default_head_length() -> u32 {
    10
}
fn default_font_size() -> u32 {
    24
}
fn default_padding() -> u32 {
    4
}
fn default_true() -> bool {
    true
}

/// Clamp functions for deserialize_with.
fn clamp_stroke_width<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u32, D::Error> {
    use serde::Deserialize;
    let v = u32::deserialize(deserializer)?;
    Ok(v.min(100))
}
fn clamp_head_length<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u32, D::Error> {
    use serde::Deserialize;
    let v = u32::deserialize(deserializer)?;
    Ok(v.min(200))
}
fn clamp_font_size<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u32, D::Error> {
    use serde::Deserialize;
    let v = u32::deserialize(deserializer)?;
    Ok(v.min(200))
}
fn clamp_padding<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u32, D::Error> {
    use serde::Deserialize;
    let v = u32::deserialize(deserializer)?;
    Ok(v.min(50))
}

/// Operation-specific errors (pure validation/pixel logic, NO I/O).
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error("crop area ({x},{y},{w},{h}) outside image bounds {iw}×{ih}")]
    CropOutOfBounds {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        iw: u32,
        ih: u32,
    },
    #[error("zero-area crop")]
    ZeroAreaCrop,
    #[error("circle radius must be >= 1")]
    CircleRadiusZero,
    #[error("no font available")]
    NoFont,
    #[error("invalid text (empty or too long)")]
    InvalidText,
    #[error("font parse failed")]
    FontParse,
}

/// Image editing tool entry point. Owns the data directory (where outputs land)
/// and the HTTP client used to fetch remote image URLs.
pub struct ImageEditTool {
    data_dir: std::path::PathBuf,
    http_client: reqwest::Client,
}

impl ImageEditTool {
    pub fn new(data_dir: impl Into<std::path::PathBuf>) -> Self {
        // Reuse the centralized timeout tier (DEFAULT = 30s, see toolkit/timeouts.rs).
        // `.no_proxy()` is intentionally OMITTED so the platform's HTTP(S)_PROXY
        // env vars apply (corporate proxy support, matches vision.rs behavior).
        let http_client = reqwest::Client::builder()
            .timeout(timeouts::DEFAULT)
            .build()
            .expect("reqwest client for image_edit");
        Self {
            data_dir: data_dir.into(),
            http_client,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ImageEditTool {
    fn name(&self) -> &str {
        "image_edit"
    }

    fn description(&self) -> &str {
        "Crop, draw on, or blur images in a SINGLE pipeline call. Returns an \
 absolute file path under data/images/ that can be passed to the `vision` tool.\n\n\
 IMPORTANT — ONE CALL HANDLES THE WHOLE PIPELINE:\n\
 Pass ALL operations for a given image as a single `operations: [...]` array. \
 The runtime applies them in order atomically. DO NOT chain multiple image_edit \
 calls for the same logical task — that wastes rounds and overwrites prior \
 results. Example: to crop AND draw a labeled rectangle, use ONE call with \
 `operations: [crop(...), draw_rect(...), draw_text(...)]`, not three separate \
 calls. Only call image_edit again if you genuinely need to inspect an \
 intermediate result via vision first.\n\n\
 Operations are applied in order. Coordinates are pixel-based, origin top-left, \
 Y-axis down — relative to the current image state (after any previous crop).\n\n\
 HOW TO PROVIDE THE `image` ARGUMENT:\n\
 - If the user uploaded an image to chat (it appears in your context): pass \
   `\"image\": \"$cached:user_image\"`. The runtime replaces this with the \
   actual image data. For additional uploaded images use `$cached:user_image_1`, \
 `$cached:user_image_2`, etc.\n\
 - Otherwise: a data URL (data:image/...;base64,...), http(s) URL, raw base64, \
   or an absolute local file path (e.g. one returned by a previous image_edit call).\n\n\
 Operation types:\n\
 - crop: extract sub-region (x, y, width, height)\n\
 - draw_rect: rectangle outline or fill (x, y, width, height, color, stroke_width?, fill?)\n\
 - draw_circle: circle outline or fill (cx, cy, radius, color, stroke_width?, fill?)\n\
 - draw_line: line segment (x1, y1, x2, y2, color, stroke_width?)\n\
 - draw_arrow: arrow with arrowhead (x1, y1, x2, y2, color, stroke_width?, head_length?)\n\
 - draw_polygon: open or closed polygon (points: [{x,y}], color, stroke_width?, fill?, closed?)\n\
 - draw_text: render text (x, y, text, color, font_size?, background?, padding?)\n\
 - blur_rect: blur region (x, y, width, height, mode?: pixelate|gaussian, intensity?)\n\n\
 Colors: hex strings like #FF0000 (red) or #FF000080 (semi-transparent red).\n\n\
 DO NOT use this tool for:\n\
 - Analyzing image content — use `vision` instead.\n\
 - Generating images from scratch — this tool requires an existing image.\n\
 - Re-running the same operations because you can't see the result — the \
 response includes `operations_applied` and the file IS written; if you need \
 to verify visually, call `vision` with the returned path ONCE.\n\n\
 Common patterns:\n\
 - Annotate a chat-uploaded detection in ONE call: {\"image\": \"$cached:user_image\", \"operations\": [draw_rect(...), draw_text(label, x, y-20)]}\n\
 - Privacy masking: [blur_rect(face_region)]\n\
 - Region-focused analysis (2 tools, not 2 image_edit calls): image_edit(crop) then vision(returned path)\n\n\
 EMBEDDING THE RESULT IN YOUR REPLY:\n\
 The response includes a `url` field (e.g. \"/api/images/foo.png\"). To show the \
 processed image to the user in your text reply, write markdown: \
 `![description](url)`. The url stays valid for the lifetime of the file. \
 Do NOT include the base64 unless the user explicitly asks for it."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["image", "operations"],
            "properties": {
                "image": {
                    "type": "string",
                    "description": "Source image. Use \"$cached:user_image\" to reference the most recent image uploaded to chat by the user (the runtime auto-resolves this). Other accepted forms: data URL (data:image/...;base64,...), http(s) URL, raw base64, or absolute local file path returned by a previous image_edit call."
                },
                "operations": {
                    "type": "array",
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["crop", "draw_rect", "draw_circle", "draw_line", "draw_arrow", "draw_polygon", "draw_text", "blur_rect"]
                            }
                        }
                    },
                    "description": "Each operation must have a `type` field. Common fields: x/y/width/height (pixel coords, origin top-left, Y-axis down). Colors are #RRGGBB or #RRGGBBAA hex strings. See tool description for per-operation fields."
                },
                "output_format": { "type": "string", "enum": ["png", "jpeg", "webp"], "default": "png" },
                "output_filename": { "type": "string", "description": "Optional filename (no path). If omitted, a UUID-based name is generated." },
                "include_base64": { "type": "boolean", "default": false }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let params: ImageEditParams = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArguments(format!("invalid image_edit args: {}", e)))?;

        if params.operations.is_empty() {
            return Err(ToolError::InvalidArguments(
                "operations must contain at least one entry".into(),
            ));
        }
        if params.operations.len() > MAX_OPERATIONS {
            return Err(ToolError::InvalidArguments(format!(
                "operations exceeds max {}",
                MAX_OPERATIONS
            )));
        }

        // 1. Load input bytes (data URL / HTTP URL / file path / raw base64).
        let (bytes, _mime) =
            crate::image_utils::resolve_image(&params.image, &self.http_client, MAX_IMAGE_SIZE)
                .await
                .map_err(ToolError::from)?;

        // 2. Decode.
        let mut img = image::load_from_memory(&bytes)
            .map_err(|e| ToolError::Execution(format!("image decode failed: {}", e)))?;

        let (w, h) = (img.width(), img.height());
        if w > MAX_DIM || h > MAX_DIM {
            return Err(ToolError::InvalidArguments(format!(
                "decoded image {}x{} exceeds max dimensions {}x{}",
                w, h, MAX_DIM, MAX_DIM
            )));
        }

        // 3. Apply operations atomically (fail -> no file written).
        for op in &params.operations {
            apply_operation(&mut img, op)
                .map_err(|e| ToolError::Execution(format!("operation failed: {:?}", e)))?;
        }

        // 4. Encode result.
        let (out_bytes, mime, ext) = encode(&img, params.output_format)?;

        // 5. Write to data/images/<filename>.
        let path = self.write_output(&out_bytes, ext, params.output_filename.as_deref())?;

        // 6. Build response.
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let mut resp = serde_json::json!({
            "path": path.to_string_lossy(),
            "url": format!("/api/images/{}", filename),
            "width": img.width(),
            "height": img.height(),
            "size_bytes": out_bytes.len(),
            "image_type": mime,
            "operations_applied": params.operations.len(),
            "status": "success",
        });
        if params.include_base64 {
            let b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &out_bytes);
            resp["base64"] = serde_json::Value::String(b64);
        }

        Ok(ToolOutput::success(resp))
    }
}

impl ImageEditTool {
    /// Write the encoded image bytes to `<data_dir>/images/<filename>`.
    ///
    /// Returns the **absolute** path. CRITICAL: do NOT use
    /// `std::fs::canonicalize` here — on macOS it resolves `/tmp` to
    /// `/private/tmp` (which lives under `/var/` in some layouts), and
    /// `vision.rs` blocklists `/var/`. `current_dir().join(rel)` gives an
    /// absolute path without resolving symlinks.
    fn write_output(
        &self,
        bytes: &[u8],
        ext: &str,
        filename: Option<&str>,
    ) -> Result<std::path::PathBuf> {
        let images_dir = self.data_dir.join("images");
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| ToolError::Execution(format!("create data/images failed: {}", e)))?;

        let final_name = match filename {
            None => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            Some(name) => sanitize_filename(name, ext)?,
        };

        let mut path = images_dir.join(&final_name);
        if path.exists() {
            // Name collision — append a short UUID suffix to avoid clobbering.
            let suffix: String = uuid::Uuid::new_v4().to_string().chars().take(6).collect();
            let stem: String = final_name.split('.').next().unwrap_or("img").to_string();
            let new_name = format!("{}_{}.{}", stem, suffix, ext);
            path = images_dir.join(new_name);
        }

        // Write to a .tmp sidecar then rename — atomic on the same filesystem.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)
            .map_err(|e| ToolError::Execution(format!("write failed: {}", e)))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| ToolError::Execution(format!("rename failed: {}", e)))?;

        // Build absolute path WITHOUT canonicalize (see doc comment above).
        let abs = std::env::current_dir()
            .map_err(|e| ToolError::Execution(format!("current_dir failed: {}", e)))?
            .join(&path);
        Ok(abs)
    }
}

/// Sanitize a user-supplied filename: reject path components, strip non-alnum
/// chars (keeping `_`, `-`, `.`), strip any trailing image extension the user
/// may have included, then re-attach the canonical `expected_ext`.
fn sanitize_filename(name: &str, expected_ext: &str) -> Result<String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ToolError::InvalidArguments(format!(
            "output_filename contains path components: {}",
            name
        )));
    }
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    if kept.is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "output_filename has no valid chars after sanitize: {}",
            name
        )));
    }
    // Strip any known image extension(s) the user may have included, then
    // re-attach the canonical one. We match case-insensitively but preserve
    // the original case of the stem.
    let lower = kept.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(&format!(".{}", expected_ext))
        .or_else(|| lower.strip_suffix(".png"))
        .or_else(|| lower.strip_suffix(".jpeg"))
        .or_else(|| lower.strip_suffix(".jpg"))
        .or_else(|| lower.strip_suffix(".webp"))
        .map(|s| kept[..s.len()].to_string())
        .unwrap_or(kept);
    Ok(format!("{}.{}", stem, expected_ext))
}

/// Dispatch a single operation against `img`.
fn apply_operation(
    img: &mut image::DynamicImage,
    op: &Operation,
) -> std::result::Result<(), OpError> {
    match op {
        Operation::Crop {
            x,
            y,
            width,
            height,
        } => apply_crop(img, *x, *y, *width, *height),
        Operation::DrawRect {
            x,
            y,
            width,
            height,
            color,
            stroke_width,
            fill,
        } => {
            apply_draw_rect(
                img,
                *x,
                *y,
                *width,
                *height,
                color.0,
                *stroke_width,
                fill.clone().map(|c| c.0),
            );
            Ok(())
        }
        Operation::DrawCircle {
            cx,
            cy,
            radius,
            color,
            stroke_width,
            fill,
        } => apply_draw_circle(
            img,
            *cx,
            *cy,
            *radius,
            color.0,
            *stroke_width,
            fill.clone().map(|c| c.0),
        ),
        Operation::DrawLine {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
        } => {
            apply_draw_line(img, *x1, *y1, *x2, *y2, color.0, *stroke_width);
            Ok(())
        }
        Operation::DrawArrow {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
            head_length,
        } => {
            apply_draw_arrow(
                img,
                *x1,
                *y1,
                *x2,
                *y2,
                color.0,
                *stroke_width,
                *head_length,
            );
            Ok(())
        }
        Operation::DrawPolygon {
            points,
            color,
            stroke_width,
            fill,
            closed,
        } => {
            apply_draw_polygon(
                img,
                points,
                color.0,
                *stroke_width,
                fill.clone().map(|c| c.0),
                *closed,
            );
            Ok(())
        }
        Operation::DrawText {
            x,
            y,
            text,
            color,
            font_size,
            background,
            padding,
        } => {
            let font_bytes = probe_font().ok_or(OpError::NoFont)?;
            apply_draw_text(
                img,
                *x,
                *y,
                text,
                color.0,
                *font_size,
                background.clone().map(|c| c.0),
                *padding,
                font_bytes,
            )
        }
        Operation::BlurRect {
            x,
            y,
            width,
            height,
            mode,
            intensity,
        } => apply_blur_rect(img, *x, *y, *width, *height, *mode, *intensity),
    }
}

/// Encode the image to the requested format. Returns `(bytes, mime, extension)`.
fn encode(
    img: &image::DynamicImage,
    fmt: OutputFormat,
) -> Result<(Vec<u8>, &'static str, &'static str)> {
    use image::ImageFormat;
    let mut buf = std::io::Cursor::new(Vec::new());
    match fmt {
        OutputFormat::Png => {
            img.write_to(&mut buf, ImageFormat::Png)
                .map_err(|e| ToolError::Execution(format!("png encode: {}", e)))?;
            Ok((buf.into_inner(), "image/png", "png"))
        }
        OutputFormat::Jpeg => {
            // JPEG has no alpha channel — composite onto white if any transparency.
            let to_encode = if has_transparency(img) {
                composite_onto_white(img)
            } else {
                img.clone()
            };
            to_encode
                .write_to(&mut buf, ImageFormat::Jpeg)
                .map_err(|e| ToolError::Execution(format!("jpeg encode: {}", e)))?;
            Ok((buf.into_inner(), "image/jpeg", "jpeg"))
        }
        OutputFormat::Webp => match img.write_to(&mut buf, ImageFormat::WebP) {
            Ok(()) => Ok((buf.into_inner(), "image/webp", "webp")),
            Err(e) => {
                tracing::warn!(error = %e, "webp encode failed, falling back to png");
                // Replace the cursor entirely. `clear()` only empties the underlying
                // Vec but leaves the cursor position at N — subsequent writes would
                // prepend zero-padding up to N before emitting any new bytes.
                buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, ImageFormat::Png).map_err(|e2| {
                    ToolError::Execution(format!("png fallback after webp fail: {}", e2))
                })?;
                Ok((buf.into_inner(), "image/png", "png"))
            }
        },
    }
}

fn has_transparency(img: &image::DynamicImage) -> bool {
    img.as_rgba8()
        .map(|b| b.pixels().any(|p| p.0[3] < 255))
        .unwrap_or(false)
}

fn composite_onto_white(img: &image::DynamicImage) -> image::DynamicImage {
    let mut bg = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        img.width(),
        img.height(),
        image::Rgba([255, 255, 255, 255]),
    ));
    image::imageops::overlay(&mut bg, img, 0, 0);
    bg
}

fn apply_crop(
    img: &mut image::DynamicImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> std::result::Result<(), OpError> {
    if w == 0 || h == 0 {
        return Err(OpError::ZeroAreaCrop);
    }
    let (iw, ih) = (img.width(), img.height());
    // checked_add — `x + w` could overflow u32 in release mode and panic otherwise.
    let x2 = x
        .checked_add(w)
        .ok_or(OpError::CropOutOfBounds { x, y, w, h, iw, ih })?;
    let y2 = y
        .checked_add(h)
        .ok_or(OpError::CropOutOfBounds { x, y, w, h, iw, ih })?;
    if x2 > iw || y2 > ih {
        return Err(OpError::CropOutOfBounds { x, y, w, h, iw, ih });
    }
    let cropped = img.crop_imm(x, y, w, h);
    *img = cropped;
    Ok(())
}

/// Font loader with OnceLock caching.
static FONT_BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// Probe system fonts and return bytes (cached).
fn probe_font() -> Option<&'static [u8]> {
    FONT_BYTES
        .get_or_init(|| {
            let candidates: &[&str] = if cfg!(target_os = "macos") {
                &[
                    "/System/Library/Fonts/PingFang.ttc",
                    "/System/Library/Fonts/Helvetica.ttc",
                    "/Library/Fonts/Arial.ttf",
                ]
            } else if cfg!(target_os = "windows") {
                &[
                    "C:\\Windows\\Fonts\\msyh.ttc",
                    "C:\\Windows\\Fonts\\msyh.ttf",
                    "C:\\Windows\\Fonts\\arial.ttf",
                ]
            } else {
                &[
                    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
                    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                ]
            };
            for path in candidates {
                if let Ok(bytes) = std::fs::read(path) {
                    tracing::debug!(
                        font_path = %path,
                        "image_edit: font loaded ({} bytes)",
                        bytes.len()
                    );
                    return Some(bytes);
                }
            }
            tracing::debug!("image_edit: no system font found");
            None
        })
        .as_deref()
}

/// Note: `stroke_width` is accepted for API uniformity with other draw operations
/// but not honored — imageproc 0.27's rect outline is always 1px thick.
#[allow(clippy::too_many_arguments)]
fn apply_draw_rect(
    img: &mut image::DynamicImage,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: image::Rgba<u8>,
    stroke_width: u32,
    fill: Option<image::Rgba<u8>>,
) {
    use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut};
    let rect = imageproc::rect::Rect::at(x, y).of_size(w, h);
    if let Some(fill_color) = fill {
        draw_filled_rect_mut(img, rect, fill_color);
    }
    // Note: imageproc 0.27 does not support stroke_width for rects; the parameter
    // is accepted for API uniformity but the outline is always 1px thick.
    let _ = stroke_width;
    draw_hollow_rect_mut(img, rect, color);
}

fn apply_draw_circle(
    img: &mut image::DynamicImage,
    cx: i32,
    cy: i32,
    radius: u32,
    color: image::Rgba<u8>,
    stroke_width: u32,
    fill: Option<image::Rgba<u8>>,
) -> std::result::Result<(), OpError> {
    if radius < 1 {
        return Err(OpError::CircleRadiusZero);
    }
    use imageproc::drawing::{draw_filled_circle_mut, draw_hollow_circle_mut};
    if let Some(fill) = fill {
        draw_filled_circle_mut(img, (cx, cy), radius as i32, fill);
    }
    let _ = stroke_width.max(1) as i32; // Stroke width parameter not available in imageproc 0.27
    draw_hollow_circle_mut(img, (cx, cy), radius as i32, color);
    Ok(())
}

/// Note: `stroke_width` is accepted for API uniformity with other draw operations
/// but not honored — imageproc 0.27's line drawing is single-pixel width.
fn apply_draw_line(
    img: &mut image::DynamicImage,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: image::Rgba<u8>,
    _stroke_width: u32,
) {
    use imageproc::drawing::draw_line_segment_mut;
    // Note: imageproc's draw_line_segment_mut doesn't support stroke_width parameter
    draw_line_segment_mut(img, (x1 as f32, y1 as f32), (x2 as f32, y2 as f32), color);
}

/// Note: `stroke_width` is accepted for API uniformity with other draw operations
/// but not honored — imageproc 0.27's line drawing is single-pixel width.
#[allow(clippy::too_many_arguments)]
fn apply_draw_arrow(
    img: &mut image::DynamicImage,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: image::Rgba<u8>,
    _stroke_width: u32,
    head_length: u32,
) {
    use imageproc::drawing::draw_line_segment_mut;

    // Main line
    draw_line_segment_mut(img, (x1 as f32, y1 as f32), (x2 as f32, y2 as f32), color);

    // Arrowhead: two lines at ~25 degrees from the main line
    let dx = x2 - x1;
    let dy = y2 - y1;
    let angle = f32::atan2(dy as f32, dx as f32);
    let head_angle1 = angle + 25.0_f32.to_radians();
    let head_angle2 = angle - 25.0_f32.to_radians();
    let hl = head_length as f32;

    let hx1 = x2 as f32 - hl * head_angle1.cos();
    let hy1 = y2 as f32 - hl * head_angle1.sin();
    let hx2 = x2 as f32 - hl * head_angle2.cos();
    let hy2 = y2 as f32 - hl * head_angle2.sin();

    draw_line_segment_mut(img, (x2 as f32, y2 as f32), (hx1, hy1), color);
    draw_line_segment_mut(img, (x2 as f32, y2 as f32), (hx2, hy2), color);
}

/// Note: `stroke_width` is accepted for API uniformity with other draw operations
/// but not honored — imageproc 0.27's polygon outline is single-pixel width.
fn apply_draw_polygon(
    img: &mut image::DynamicImage,
    points: &[PolygonPoint],
    color: image::Rgba<u8>,
    _stroke_width: u32,
    fill: Option<image::Rgba<u8>>,
    closed: bool,
) {
    use imageproc::drawing::draw_line_segment_mut;
    if points.is_empty() {
        return;
    }

    let ipoints: Vec<imageproc::point::Point<i32>> = points
        .iter()
        .map(|p| imageproc::point::Point { x: p.x, y: p.y })
        .collect();

    if closed {
        // imageproc requires at least 3 points for a polygon
        if ipoints.len() < 3 {
            tracing::warn!(
                "draw_polygon with closed=true requires >=3 points, got {}. Falling back to line.",
                ipoints.len()
            );
            // Draw consecutive lines (no auto-close for 2 points)
            for window in ipoints.windows(2) {
                draw_line_segment_mut(
                    img,
                    (window[0].x as f32, window[0].y as f32),
                    (window[1].x as f32, window[1].y as f32),
                    color,
                );
            }
            return;
        }
        // Fill first (so outline draws on top). imageproc 0.27's
        // `draw_polygon_mut` fills AND outlines with the SAME color (no
        // separate fill/outline), so we do a manual bounding-box scanline
        // fill using a point-in-polygon test. O(area * points) — fine for
        // annotation-scale polygons.
        if let Some(fill_color) = fill {
            use image::{GenericImage, GenericImageView};
            let min_x = ipoints.iter().map(|p| p.x).min().unwrap();
            let max_x = ipoints.iter().map(|p| p.x).max().unwrap();
            let min_y = ipoints.iter().map(|p| p.y).min().unwrap();
            let max_y = ipoints.iter().map(|p| p.y).max().unwrap();
            let (iw, ih) = img.dimensions();
            let xs_lo = min_x.max(0);
            let xs_hi = max_x.min(iw as i32 - 1);
            let ys_lo = min_y.max(0);
            let ys_hi = max_y.min(ih as i32 - 1);
            for py in ys_lo..=ys_hi {
                for px in xs_lo..=xs_hi {
                    if point_in_polygon(px, py, &ipoints) {
                        // put_pixel overwrites — matches imageproc's filled circle behavior.
                        if (px as u32) < iw && (py as u32) < ih {
                            img.put_pixel(px as u32, py as u32, fill_color);
                        }
                    }
                }
            }
        }
        // Outline: draw all edges including the closing edge (last -> first).
        // We draw edges manually rather than using `draw_polygon_mut` because
        // the latter FILLS the polygon with the outline color, which would
        // clobber the fill_color we just laid down (and is wrong for the
        // `fill: None` case where the user wants outline only).
        let n = ipoints.len();
        for i in 0..n {
            let a = &ipoints[i];
            let b = &ipoints[(i + 1) % n];
            draw_line_segment_mut(
                img,
                (a.x as f32, a.y as f32),
                (b.x as f32, b.y as f32),
                color,
            );
        }
    } else {
        // Open polygon: draw consecutive lines only (no auto-close)
        for window in ipoints.windows(2) {
            draw_line_segment_mut(
                img,
                (window[0].x as f32, window[0].y as f32),
                (window[1].x as f32, window[1].y as f32),
                color,
            );
        }
    }
}

/// Standard ray-casting point-in-polygon test.
fn point_in_polygon(px: i32, py: i32, pts: &[imageproc::point::Point<i32>]) -> bool {
    let mut inside = false;
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (pts[i].x, pts[i].y);
        let (xj, yj) = (pts[j].x, pts[j].y);
        if (yi > py) != (yj > py) {
            let x_inter = (xj - xi) as f32 * (py - yi) as f32 / (yj - yi) as f32 + xi as f32;
            if (px as f32) < x_inter {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

#[allow(clippy::too_many_arguments)]
fn apply_draw_text(
    img: &mut image::DynamicImage,
    x: i32,
    y: i32,
    text: &str,
    color: image::Rgba<u8>,
    font_size: u32,
    background: Option<image::Rgba<u8>>,
    padding: u32,
    font_bytes: &[u8],
) -> std::result::Result<(), OpError> {
    if font_bytes.is_empty() {
        return Err(OpError::NoFont);
    }
    if text.is_empty() || text.len() > 200 {
        return Err(OpError::InvalidText);
    }

    let font = ab_glyph::FontRef::try_from_slice(font_bytes).map_err(|_| OpError::FontParse)?;

    // Layout text to get bounding box
    let scale = ab_glyph::PxScale::from(font_size as f32);

    // Calculate text bounds using font metrics - simplified approach
    // Just estimate width based on character count and font size
    let text_w = (text.len() as u32 * font_size * 3 / 5).max(1); // Rough estimate
    let text_h = font_size; // Approximate height

    // Draw background if provided
    if let Some(bg) = background {
        apply_draw_rect(
            img,
            x,
            y,
            text_w + 2 * padding,
            text_h + 2 * padding,
            bg,
            1,
            Some(bg),
        );
    }

    // Draw text at (x + padding, y + padding)
    use imageproc::drawing::draw_text_mut;
    let draw_x = x + padding as i32;
    let draw_y = y + padding as i32;
    draw_text_mut(img, color, draw_x, draw_y, scale, &font, text);

    Ok(())
}

fn apply_blur_rect(
    img: &mut image::DynamicImage,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    mode: Option<BlurMode>,
    intensity: Option<u32>,
) -> std::result::Result<(), OpError> {
    let (x, y) = (x.max(0), y.max(0));
    let (w, h) = (w, h);
    if w == 0 || h == 0 {
        return Ok(());
    }
    let (iw, ih) = (img.width(), img.height());
    if x as u32 >= iw || y as u32 >= ih {
        return Ok(());
    }
    let x2 = ((x as u32) + w).min(iw);
    let y2 = ((y as u32) + h).min(ih);
    let (cw, ch) = (x2 - x as u32, y2 - y as u32);

    match mode.unwrap_or(BlurMode::Pixelate) {
        BlurMode::Pixelate => {
            let block = intensity.unwrap_or(16).clamp(1, 256);
            let mut rgba = img.to_rgba8();
            let mut by = y as u32;
            while by < y2 {
                let mut bx = x as u32;
                while bx < x2 {
                    let mut rs: u64 = 0;
                    let mut gs: u64 = 0;
                    let mut bs: u64 = 0;
                    let mut as_: u64 = 0;
                    let mut n: u64 = 0;
                    let by_end = (by + block).min(y2);
                    let bx_end = (bx + block).min(x2);
                    for yy in by..by_end {
                        for xx in bx..bx_end {
                            let p = rgba.get_pixel(xx, yy);
                            rs += p.0[0] as u64;
                            gs += p.0[1] as u64;
                            bs += p.0[2] as u64;
                            as_ += p.0[3] as u64;
                            n += 1;
                        }
                    }
                    if n > 0 {
                        let mean = image::Rgba([
                            (rs / n) as u8,
                            (gs / n) as u8,
                            (bs / n) as u8,
                            (as_ / n) as u8,
                        ]);
                        for yy in by..by_end {
                            for xx in bx..bx_end {
                                rgba.put_pixel(xx, yy, mean);
                            }
                        }
                    }
                    bx += block;
                }
                by += block;
            }
            *img = image::DynamicImage::ImageRgba8(rgba);
        }
        BlurMode::Gaussian => {
            let radius = intensity.unwrap_or(5).clamp(1, 100) as f32;
            // CRITICAL: use `crop_imm` to get a sub-image, then blur and overlay.
            // NOTE: crop_imm returns a NEW image, it doesn't mutate the receiver.
            let sub = img.crop_imm(x as u32, y as u32, cw, ch);
            let blurred = image::imageops::blur(&sub, radius);
            image::imageops::overlay(img, &blurred, x as i64, y as i64);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GenericImageView, RgbaImage};

    #[test]
    fn tool_name_and_category() {
        let t = ImageEditTool::new("/tmp");
        assert_eq!(t.name(), "image_edit");
        assert!(matches!(t.category(), ToolCategory::System));
    }

    #[tokio::test]
    async fn execute_rejects_empty_operations() {
        let t = ImageEditTool::new("/tmp");
        let res = t
            .execute(serde_json::json!({
                "image": "data:image/png;base64,",
                "operations": []
            }))
            .await;
        assert!(res.is_err());
    }

    #[test]
    fn sanitize_strips_path_components() {
        assert!(sanitize_filename("../../../etc/foo", "png").is_err());
        assert!(sanitize_filename("a/b", "png").is_err());
        assert!(sanitize_filename("a\\b", "png").is_err());
    }

    #[test]
    fn sanitize_replaces_invalid_chars_and_forces_ext() {
        // Spaces and `!` are dropped; user-supplied `.png` is stripped, `.jpeg` appended.
        let n = sanitize_filename("Alert Zone!.png", "jpeg").unwrap();
        assert_eq!(n, "AlertZone.jpeg");
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(sanitize_filename("!!!", "png").is_err());
    }

    #[test]
    fn sanitize_preserves_dashes_and_underscores() {
        let n = sanitize_filename("my-snapshot_01", "png").unwrap();
        assert_eq!(n, "my-snapshot_01.png");
    }

    #[test]
    fn write_output_returns_absolute_path() {
        // Construct data_dir under current_dir() to keep the test hermetic
        // and avoid macOS /var/folders prefix (which vision.rs blocklists).
        let test_root = std::env::current_dir().unwrap().join("test-tmp-image-edit");
        std::fs::create_dir_all(&test_root).unwrap();
        let tool = ImageEditTool::new(&test_root);
        // 8-byte PNG header is enough for the writer — we don't decode it back.
        let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let path = tool.write_output(&png, "png", None).unwrap();
        assert!(path.is_absolute(), "path must be absolute, got: {:?}", path);
        #[cfg(unix)]
        assert!(path.starts_with("/"), "unix absolute must start with /");
        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn write_output_honors_custom_filename() {
        let test_root = std::env::current_dir()
            .unwrap()
            .join("test-tmp-image-edit-named");
        std::fs::create_dir_all(&test_root).unwrap();
        let tool = ImageEditTool::new(&test_root);
        let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let path = tool
            .write_output(&png, "png", Some("custom-name.png"))
            .unwrap();
        assert!(
            path.to_string_lossy().ends_with("custom-name.png"),
            "expected custom-name.png suffix, got: {:?}",
            path
        );
        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn jpeg_output_composites_alpha_onto_white() {
        // Fully transparent 10x10 image — would normally encode as black in JPEG.
        let img = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));
        let (bytes, mime, _ext) = encode(&img, OutputFormat::Jpeg).unwrap();
        assert_eq!(mime, "image/jpeg");
        let decoded = image::load_from_memory(&bytes).unwrap();
        // After compositing onto white, the top-left pixel must be near-white.
        let p = decoded.get_pixel(0, 0);
        assert!(
            p[0] > 200 && p[1] > 200 && p[2] > 200,
            "expected white background after alpha compositing, got {:?}",
            p
        );
    }

    #[test]
    fn png_output_preserves_alpha() {
        // PNG supports alpha — a transparent image should round-trip without
        // being composited.
        let img = DynamicImage::ImageRgba8(RgbaImage::new(10, 10));
        let (bytes, mime, _ext) = encode(&img, OutputFormat::Png).unwrap();
        assert_eq!(mime, "image/png");
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        let p = decoded.get_pixel(0, 0);
        // Alpha channel preserved (transparent).
        assert_eq!(p[3], 0, "png should preserve alpha=0, got {:?}", p);
    }

    /// Build a base64-encoded PNG data URL of the given dimensions.
    fn make_test_png_data_url(w: u32, h: u32) -> String {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf.into_inner());
        format!("data:image/png;base64,{}", b64)
    }

    #[tokio::test]
    async fn chain_image_edit_to_resolve_image_works() {
        // Use current_dir()-rooted path (NOT tempfile::tempdir) — on macOS,
        // tempdir() returns paths under /var/folders/... which image_utils's
        // read_local_image blocklist rejects, breaking the chain.
        let test_root = std::env::current_dir()
            .unwrap()
            .join("test-tmp-image-edit-chain");
        std::fs::create_dir_all(&test_root).unwrap();
        let tool = ImageEditTool::new(&test_root);

        // 1. Build a base64 PNG data URL input.
        let png_url = make_test_png_data_url(200, 200);

        // 2. Run image_edit with a crop operation.
        let args = serde_json::json!({
            "image": png_url,
            "operations": [{"type": "crop", "x": 10, "y": 10, "width": 100, "height": 80}],
            "output_filename": "cropped.png"
        });
        let out = tool.execute(args).await.expect("execute should succeed");
        let path_str = out.data["path"]
            .as_str()
            .expect("path field in response")
            .to_string();

        // 3. Path must be absolute.
        assert!(
            path_str.starts_with('/'),
            "path must be absolute, got: {}",
            path_str
        );

        // 4. resolve_image should accept the produced path (round-trip).
        let client = reqwest::Client::new();
        let (bytes, mime) = crate::image_utils::resolve_image(&path_str, &client, 10 * 1024 * 1024)
            .await
            .expect("resolve_image should accept the path");
        assert_eq!(mime, "image/png");
        assert!(!bytes.is_empty());

        // 5. The reloaded image should have the cropped dimensions.
        let reloaded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            reloaded.dimensions(),
            (100, 80),
            "cropped image should be 100x80"
        );

        // Cleanup.
        let _ = std::fs::remove_dir_all(&test_root);
    }
}

#[cfg(test)]
mod op_tests {
    use super::*;
    use image::{DynamicImage, GenericImage, GenericImageView, Rgba, RgbaImage};

    fn solid(w: u32, h: u32, c: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba(c)))
    }

    #[test]
    fn crop_happy_path() {
        let mut img = solid(200, 200, [255, 0, 0, 255]);
        apply_crop(&mut img, 10, 10, 100, 80).unwrap();
        assert_eq!(img.dimensions(), (100, 80));
    }

    #[test]
    fn crop_out_of_bounds_rejected() {
        let mut img = solid(200, 200, [255, 0, 0, 255]);
        let err = apply_crop(&mut img, 150, 150, 100, 100).unwrap_err();
        assert!(matches!(err, OpError::CropOutOfBounds { .. }));
    }

    #[test]
    fn crop_zero_area_rejected() {
        let mut img = solid(200, 200, [255, 0, 0, 255]);
        let err = apply_crop(&mut img, 10, 10, 0, 50).unwrap_err();
        assert!(matches!(err, OpError::ZeroAreaCrop));
    }

    #[test]
    fn crop_overflow_returns_error_not_panic() {
        let mut img = solid(200, 200, [255, 0, 0, 255]);
        // width = u32::MAX would overflow `x + w` if unchecked.
        let err = apply_crop(&mut img, 100, 100, u32::MAX, 10).unwrap_err();
        assert!(matches!(err, OpError::CropOutOfBounds { .. }));
    }

    #[test]
    fn draw_rect_default_color_red() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        apply_draw_rect(
            &mut img,
            10,
            10,
            100,
            80,
            image::Rgba([255, 0, 0, 255]),
            2,
            None,
        );
        // The rect's TOP-LEFT CORNER (10,10) is unambiguously on the outline.
        assert_eq!(img.get_pixel(10, 10), image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn draw_circle_radius_zero_rejected() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        let err = apply_draw_circle(
            &mut img,
            100,
            100,
            0,
            image::Rgba([255, 0, 0, 255]),
            2,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, OpError::CircleRadiusZero));
    }

    #[test]
    fn draw_line_colors_pixel() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        apply_draw_line(&mut img, 10, 10, 50, 50, image::Rgba([255, 0, 0, 255]), 2);
        // Midpoint of the line should be colored
        let mid = img.get_pixel(30, 30);
        assert_eq!(mid, image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn draw_arrow_draws_arrowhead() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        apply_draw_arrow(
            &mut img,
            10,
            10,
            50,
            50,
            image::Rgba([255, 0, 0, 255]),
            2,
            10,
        );
        // Pixel near arrowhead tip (50,50) should be colored
        let tip = img.get_pixel(50, 50);
        assert_eq!(tip, image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn draw_polygon_closed_connects_last_to_first() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        let points = vec![
            PolygonPoint { x: 10, y: 10 },
            PolygonPoint { x: 50, y: 10 },
            PolygonPoint { x: 50, y: 50 },
        ];
        apply_draw_polygon(
            &mut img,
            &points,
            image::Rgba([255, 0, 0, 255]),
            2,
            None,
            true,
        );
        // Last point (50,50) should connect to first (10,10)
        // Pixel on the closing edge at (30, 30) should be colored
        let closing_edge = img.get_pixel(30, 30);
        assert_eq!(closing_edge, image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn draw_polygon_open_does_not_connect() {
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        let points = vec![
            PolygonPoint { x: 10, y: 10 },
            PolygonPoint { x: 50, y: 10 },
            PolygonPoint { x: 50, y: 50 },
        ];
        apply_draw_polygon(
            &mut img,
            &points,
            image::Rgba([255, 0, 0, 255]),
            2,
            None,
            false,
        );
        // Last point (50,50) should NOT connect to first (10,10)
        // Pixel on the would-be closing edge at (30, 30) should NOT be colored
        let no_edge = img.get_pixel(30, 30);
        assert_eq!(no_edge, image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn draw_polygon_fill_covers_interior() {
        // Filled triangle: vertices (10,10), (90,10), (50,90).
        // Centroid (50, ~36) is unambiguously interior — outline-only would
        // leave it black. This guards against Gap 2 regression (fill branch
        // identical to outline).
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        let points = vec![
            PolygonPoint { x: 10, y: 10 },
            PolygonPoint { x: 90, y: 10 },
            PolygonPoint { x: 50, y: 90 },
        ];
        apply_draw_polygon(
            &mut img,
            &points,
            image::Rgba([255, 0, 0, 255]),
            2,
            Some(image::Rgba([0, 255, 0, 255])), // green fill
            true,
        );
        // Interior point should be green (fill color).
        let interior = img.get_pixel(50, 36);
        assert_eq!(interior, image::Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn draw_rect_fill_covers_interior() {
        // Guards against Gap 1 regression (fill branch did nothing).
        let mut img = solid(200, 200, [0, 0, 0, 255]);
        apply_draw_rect(
            &mut img,
            10,
            10,
            100,
            80,
            image::Rgba([255, 0, 0, 255]), // red outline
            2,
            Some(image::Rgba([0, 255, 0, 255])), // green fill
        );
        // Interior pixel (not on the 1px outline) should be green.
        let interior = img.get_pixel(50, 50);
        assert_eq!(interior, image::Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn draw_text_renders_glyphs_with_background() {
        // System-font dependent: skip cleanly on systems without any of the probed fonts
        let font_bytes = match probe_font() {
            Some(b) => b.to_vec(),
            None => {
                eprintln!("skipping: no system font available");
                return;
            }
        };
        let mut img = solid(200, 100, [0, 0, 0, 255]);
        apply_draw_text(
            &mut img,
            10,
            10,
            "Hi",
            image::Rgba([255, 255, 255, 255]),
            24,
            Some(image::Rgba([0, 0, 0, 255])),
            4,
            &font_bytes,
        )
        .unwrap();
        // (x,y) is the top-left of the background rect — still pure black bg
        assert_eq!(img.get_pixel(10, 10), image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn draw_text_empty_font_bytes_returns_error() {
        let mut img = solid(100, 50, [0, 0, 0, 255]);
        let res = apply_draw_text(
            &mut img,
            10,
            10,
            "x",
            image::Rgba([255, 255, 255, 255]),
            16,
            None,
            0,
            &[], // empty font bytes
        );
        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), OpError::NoFont));
    }

    #[test]
    fn blur_rect_pixelate_uniform_blocks() {
        let mut img = solid(80, 80, [200, 100, 50, 255]); // uniform input
        apply_blur_rect(&mut img, 0, 0, 80, 80, Some(BlurMode::Pixelate), Some(16)).unwrap();
        assert_eq!(img.get_pixel(40, 40), image::Rgba([200, 100, 50, 255]));
        // Dimensions MUST be unchanged.
        assert_eq!(img.dimensions(), (80, 80));
    }

    #[test]
    fn blur_rect_gaussian_alters_pixels_and_preserves_dimensions() {
        let mut img = image::DynamicImage::ImageRgba8(image::RgbaImage::new(100, 100));
        for x in 0..50 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        for x in 50..100 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
        apply_blur_rect(&mut img, 0, 0, 100, 100, Some(BlurMode::Gaussian), Some(10)).unwrap();
        let mid = img.get_pixel(50, 50);
        assert!(
            mid[0] > 10 && mid[0] < 245,
            "gaussian produced gradient: {}",
            mid[0]
        );
        // CRITICAL regression guard: gaussian mode MUST NOT shrink the image.
        assert_eq!(img.dimensions(), (100, 100));
    }
}
