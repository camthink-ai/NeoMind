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
use ab_glyph::Font as _;
use neomind_core::tools::ToolCategory;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, OnceLock};

/// Color newtype with hex/CSS deserialization support.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Color(#[serde(deserialize_with = "deserialize_color")] pub image::Rgba<u8>);

/// Parse a color: `#RRGGBB` / `#RRGGBBAA` hex (prefix `#` optional) or CSS
/// `rgb(r,g,b)` / `rgba(r,g,b,a)` (alpha 0-1 or 0-255; components 0-255).
///
/// Small models routinely send the CSS form (observed: `rgba(0,0,0,0.7)`
/// for a text background); rejecting it with serde's raw
/// "invalid digit found in string" left the model nothing to act on and it
/// flailed through four more calls. Returns `None` for anything else so the
/// pre-parse validator can name the offending value and the accepted forms.
fn parse_color(s: &str) -> Option<image::Rgba<u8>> {
    let s = s.trim();
    let as_hex = |hex: &str| -> Option<image::Rgba<u8>> {
        if hex.len() != 6 && hex.len() != 8 {
            return None;
        }
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = if hex.len() == 8 {
            u8::from_str_radix(&hex[6..8], 16).ok()?
        } else {
            255
        };
        Some(image::Rgba([r, g, b, a]))
    };
    // Bare hex (no '#') stays valid — the original parser accepted it.
    if let Some(hex) = s.strip_prefix('#') {
        return as_hex(hex);
    }
    if !(s.starts_with("rgb(") || s.starts_with("rgba(")) {
        return as_hex(s);
    }
    let inner = s
        .strip_prefix("rgba(")
        .and_then(|x| x.strip_suffix(')'))
        .or_else(|| s.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')))?;
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let num = |v: &str| v.parse::<u32>().ok().filter(|n| *n <= 255);
    let r = num(parts[0])? as u8;
    let g = num(parts[1])? as u8;
    let b = num(parts[2])? as u8;
    let a = if parts.len() == 4 {
        // Accept both CSS alpha (0.0-1.0) and raw 0-255.
        if let Ok(f) = parts[3].parse::<f32>() {
            if (0.0..=1.0).contains(&f) {
                (f * 255.0).round() as u8
            } else if (0.0..=255.0).contains(&f) {
                f.round() as u8
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        255
    };
    Some(image::Rgba([r, g, b, a]))
}

/// Deserialize via [`parse_color`]; the error names the value and the forms.
fn deserialize_color<'de, D>(deserializer: D) -> std::result::Result<image::Rgba<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    parse_color(&s).ok_or_else(|| {
        D::Error::custom(format!(
            "invalid color '{s}': use #RRGGBB, #RRGGBBAA, rgb(r,g,b) or rgba(r,g,b,a)"
        ))
    })
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
    #[serde(rename = "rotate")]
    Rotate { angle: i32 },
    #[serde(rename = "flip")]
    Flip { direction: FlipDirection },
    #[serde(rename = "resize")]
    Resize {
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        fit: Option<ResizeFit>,
        #[serde(default)]
        background: Option<Color>,
    },
    #[serde(rename = "adjust")]
    Adjust {
        #[serde(default)]
        brightness: Option<i32>,
        #[serde(default)]
        contrast: Option<i32>,
        #[serde(default)]
        saturation: Option<i32>,
    },
    #[serde(rename = "grayscale")]
    Grayscale {},
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
        // Position is optional: the watermark case ("draw the current time")
        // routinely arrives without coordinates, and a hard failure on a
        // missing `x` aborts the whole pipeline. Missing x/y default to
        // `position` (if provided) or centering — see `resolve_text_position`.
        #[serde(default)]
        x: Option<i32>,
        #[serde(default)]
        y: Option<i32>,
        #[serde(default)]
        position: Option<TextPosition>,
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

/// Flip direction for `flip` operation.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlipDirection {
    Horizontal,
    Vertical,
}

/// Resize fit mode when both `width` and `height` are supplied.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResizeFit {
    Contain,
    Cover,
    Stretch,
}

/// Named anchor position for `draw_text` watermarks.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
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

fn infer_output_format(input_mime: &str) -> OutputFormat {
    match input_mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => OutputFormat::Jpeg,
        "image/webp" => OutputFormat::Webp,
        _ => OutputFormat::Png,
    }
}

/// Top-level parameters for the `image_edit` tool.
#[derive(Debug, serde::Deserialize)]
pub struct ImageEditParams {
    pub image: String,
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub output_format: Option<OutputFormat>,
    #[serde(default)]
    pub quality: Option<u8>,
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
    #[error("invalid rotate angle {0}: must be a multiple of 90 (e.g. 90, 180, 270, -90)")]
    InvalidRotateAngle(i32),
    #[error("invalid resize dimensions: {0}")]
    InvalidResizeDims(String),
    #[error("circle radius must be >= 1")]
    CircleRadiusZero,
    #[error("no font available")]
    NoFont,
    #[error("invalid text (empty or too long)")]
    InvalidText,
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
        "Edit image via ordered ops. Source: $cached:user_image[_N], URL, path, or /api/images/…. crop/draw_rect/blur_rect: x,y,width,height; rotate: angle(90|180|270); flip: direction(horizontal|vertical); resize: width/height,fit(contain|cover|stretch); adjust: brightness/contrast/saturation; grayscale; draw_circle: cx,cy,radius; draw_line/draw_arrow: x1,y1,x2,y2; draw_polygon: points; draw_text: text,position?. #RRGGBB[AA]."
    }

    fn parameters(&self) -> Value {
        let mut op_props = serde_json::Map::new();
        let core_props = serde_json::json!({
            "type": {
                "type": "string",
                "enum": [
                    "crop", "rotate", "flip", "resize", "adjust", "grayscale",
                    "draw_rect", "draw_circle", "draw_line", "draw_arrow",
                    "draw_polygon", "draw_text", "blur_rect"
                ]
            },
            "x": { "type": "integer" },
            "y": { "type": "integer" },
            "width": { "type": "integer" },
            "height": { "type": "integer" },
            "angle": { "type": "integer" },
            "direction": { "type": "string" },
            "fit": { "type": "string" },
            "brightness": { "type": "integer" },
            "contrast": { "type": "integer" },
            "saturation": { "type": "integer" }
        });
        let draw_props = serde_json::json!({
            "cx": { "type": "integer" },
            "cy": { "type": "integer" },
            "radius": { "type": "integer" },
            "x1": { "type": "integer" },
            "y1": { "type": "integer" },
            "x2": { "type": "integer" },
            "y2": { "type": "integer" },
            "points": { "type": "array", "items": { "type": "object" } },
            "text": { "type": "string" },
            "position": { "type": "string" },
            "color": { "type": "string" },
            "fill": { "type": "string" },
            "background": { "type": "string" },
            "stroke_width": { "type": "integer" },
            "font_size": { "type": "integer" },
            "mode": { "type": "string" }
        });
        if let Value::Object(m) = core_props {
            op_props.extend(m);
        }
        if let Value::Object(m) = draw_props {
            op_props.extend(m);
        }
        serde_json::json!({
            "type": "object",
            "required": ["image", "operations"],
            "properties": {
                "image": { "type": "string" },
                "operations": {
                    "type": "array",
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "required": ["type"],
                        "properties": Value::Object(op_props)
                    }
                },
                "output_format": { "type": "string" },
                "quality": { "type": "integer" },
                "output_filename": { "type": "string" }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        validate_operations(&args)?;
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
        let (bytes, input_mime) =
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
        // Each op reports `pixels_changed` — the "watermark was never visible"
        // failure mode (text drawn off-canvas / behind other ops) otherwise
        // surfaces only when the user complains.
        let mut warnings: Vec<String> = Vec::new();
        let mut operations_detail: Vec<Value> = Vec::new();
        for (idx, op) in params.operations.iter().enumerate() {
            let before: Option<(Vec<u8>, usize)> = {
                let bytes = img.as_bytes();
                (bytes.len() <= PIXEL_DIFF_MAX_BUFFER)
                    .then(|| (bytes.to_vec(), img.color().channel_count() as usize))
            };
            apply_operation(&mut img, op, &mut warnings, idx)
                .map_err(|e| ToolError::Execution(format!("operation failed: {:?}", e)))?;
            let pixels_changed =
                before.and_then(|(buf, ch)| count_changed_pixels(&buf, img.as_bytes(), ch));
            if let Some(0) = pixels_changed {
                if is_positional_draw_op(op) {
                    // For draw ops, 0 changed pixels almost always means the edit
                    // landed outside the canvas or was fully occluded.
                    warnings.push(format!(
                        "operations[{}] ({}): changed 0 pixels — verify coordinates land inside the {}x{} canvas",
                        idx,
                        operation_type_name(op),
                        img.width(),
                        img.height()
                    ));
                }
            }
            operations_detail.push(serde_json::json!({
                "index": idx,
                "type": operation_type_name(op),
                "pixels_changed": pixels_changed,
            }));
        }

        // 4. Encode result (preserve input format when caller omitted output_format).
        let target_format = params
            .output_format
            .unwrap_or_else(|| infer_output_format(&input_mime));
        let (out_bytes, mime, ext) = encode_with_quality(&img, target_format, params.quality)?;

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
            "operations_detail": Value::Array(operations_detail),
            "status": "success",
        });
        if !warnings.is_empty() {
            resp["warnings"] = Value::Array(warnings.into_iter().map(Value::String).collect());
        }
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

/// Required fields per operation type, beyond the `type` tag itself.
/// `draw_text` position (x/y/position) is intentionally absent — defaults to centered.
fn required_fields_for(op_type: &str) -> &'static [&'static str] {
    match op_type {
        "crop" | "draw_rect" | "blur_rect" => &["x", "y", "width", "height"],
        "rotate" => &["angle"],
        "flip" => &["direction"],
        "draw_circle" => &["cx", "cy", "radius"],
        "draw_line" | "draw_arrow" => &["x1", "y1", "x2", "y2"],
        "draw_polygon" => &["points"],
        "draw_text" => &["text"],
        _ => &[],
    }
}

/// Compact per-type required-fields table, embedded in actionable errors (and
/// mirrored in the tool schema description) so the LLM can self-correct on the
/// next round instead of staring at a bare serde "missing field `x`".
const OPERATIONS_CHEAT_SHEET: &str = "crop/draw_rect/blur_rect: x,y,width,height | rotate: angle(90|180|270) | flip: direction(horizontal|vertical) | resize: width and/or height (fit?: contain|cover|stretch) | adjust: brightness/contrast/saturation (-100..100) | grayscale | draw_circle: cx,cy,radius | draw_line/draw_arrow: x1,y1,x2,y2 | draw_polygon: points:[{x,y},...] | draw_text: text (x,y or position optional, defaults to centered)";

/// Operation type tag for `operations_detail` reporting.
fn operation_type_name(op: &Operation) -> &'static str {
    match op {
        Operation::Crop { .. } => "crop",
        Operation::Rotate { .. } => "rotate",
        Operation::Flip { .. } => "flip",
        Operation::Resize { .. } => "resize",
        Operation::Adjust { .. } => "adjust",
        Operation::Grayscale {} => "grayscale",
        Operation::DrawRect { .. } => "draw_rect",
        Operation::DrawCircle { .. } => "draw_circle",
        Operation::DrawLine { .. } => "draw_line",
        Operation::DrawArrow { .. } => "draw_arrow",
        Operation::DrawPolygon { .. } => "draw_polygon",
        Operation::DrawText { .. } => "draw_text",
        Operation::BlurRect { .. } => "blur_rect",
    }
}

/// Coordinate-based draw/blur operations where 0 changed pixels strongly
/// indicates off-canvas coordinates or full occlusion.
fn is_positional_draw_op(op: &Operation) -> bool {
    matches!(
        op,
        Operation::DrawRect { .. }
            | Operation::DrawCircle { .. }
            | Operation::DrawLine { .. }
            | Operation::DrawArrow { .. }
            | Operation::DrawPolygon { .. }
            | Operation::DrawText { .. }
            | Operation::BlurRect { .. }
    )
}

/// Skip per-op pixel diffing above this buffer size — a full-frame clone per
/// operation would double memory for huge images for a purely informational stat.
const PIXEL_DIFF_MAX_BUFFER: usize = 64 * 1024 * 1024;

/// Count pixels whose bytes differ between two same-layout frame buffers.
/// `None` when lengths differ (layout/dimension change, e.g. crop) or the
/// channel count is invalid.
fn count_changed_pixels(before: &[u8], after: &[u8], channels: usize) -> Option<u64> {
    if channels == 0 || before.len() != after.len() {
        return None;
    }
    Some(
        before
            .chunks_exact(channels)
            .zip(after.chunks_exact(channels))
            .filter(|(b, a)| b != a)
            .count() as u64,
    )
}

/// Pre-scan raw `operations` JSON for missing required fields before serde
/// rejects the whole call with an opaque "missing field `x`". Emits an error
/// that names the failing operation index and exactly what to add.
fn validate_operations(args: &Value) -> Result<()> {
    let Some(ops) = args.get("operations").and_then(|v| v.as_array()) else {
        return Ok(()); // absent / wrong shape — let serde report it
    };
    for (i, op) in ops.iter().enumerate() {
        let Some(obj) = op.as_object() else {
            continue;
        };
        let Some(op_type) = obj.get("type").and_then(|t| t.as_str()) else {
            return Err(ToolError::InvalidArguments(format!(
                "operations[{}] has no `type` field. Valid types: crop, rotate, flip, resize, adjust, grayscale, draw_rect, draw_circle, draw_line, draw_arrow, draw_polygon, draw_text, blur_rect. Required fields per type: {}",
                i, OPERATIONS_CHEAT_SHEET
            )));
        };
        if op_type == "resize" && !obj.contains_key("width") && !obj.contains_key("height") {
            return Err(ToolError::InvalidArguments(format!(
                "operations[{}] (type \"resize\") requires at least one of: width, height. Required fields per type: {}",
                i, OPERATIONS_CHEAT_SHEET
            )));
        }
        let missing: Vec<&str> = required_fields_for(op_type)
            .iter()
            .filter(|f| !obj.contains_key(**f))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(ToolError::InvalidArguments(format!(
                "operations[{}] (type \"{}\") is missing required field(s): {}. Required fields per type: {}",
                i,
                op_type,
                missing.join(", "),
                OPERATIONS_CHEAT_SHEET
            )));
        }
        // Color-format pre-check: serde's raw failure ("invalid digit found
        // in string") names neither the field nor the accepted forms — the
        // model gets nothing to act on (observed: an `rgba(...)` background
        // produced four flailing retries after it).
        for field in ["color", "fill", "background"] {
            if let Some(Value::String(raw)) = obj.get(field) {
                if parse_color(raw).is_none() {
                    return Err(ToolError::InvalidArguments(format!(
                        "operations[{}] (type \"{}\") field \"{}\": invalid color '{}'. Use #RRGGBB, #RRGGBBAA, rgb(r,g,b) or rgba(r,g,b,a).",
                        i, op_type, field, raw
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Measure rendered text bounding box `(width, height)` via `imageproc::drawing::text_size`.
fn measure_text_box(font: &ab_glyph::FontArc, font_size: u32, text: &str) -> (u32, u32) {
    let scale = ab_glyph::PxScale::from(font_size as f32);
    let (w, h) = imageproc::drawing::text_size(scale, font, text);
    (w.max(1), h.max(font_size))
}

/// Resolve draw_text top-left `(x, y)` using actual glyph dimensions.
///
/// Priority:
/// 1. Explicit `x` and/or `y` coordinates;
/// 2. Named `position` anchor (`top_left`, `top_right`, `bottom_left`, `bottom_right`, `center`);
/// 3. Fallback to centered (sets `defaulted = true` so a non-fatal notice is surfaced).
#[allow(clippy::too_many_arguments)]
fn resolve_text_position(
    img: &image::DynamicImage,
    x: Option<i32>,
    y: Option<i32>,
    position: Option<TextPosition>,
    text_w: u32,
    text_h: u32,
    padding: u32,
) -> (i32, i32, bool) {
    let box_w = text_w + 2 * padding;
    let box_h = text_h + 2 * padding;
    let iw = img.width() as i32;
    let ih = img.height() as i32;
    let margin = ((img.width().min(img.height()) / 40).clamp(8, 32)) as i32;

    let (anchor_x, anchor_y) = match position.unwrap_or(TextPosition::Center) {
        TextPosition::TopLeft => (margin, margin),
        TextPosition::TopRight => ((iw - box_w as i32 - margin).max(0), margin),
        TextPosition::BottomLeft => (margin, (ih - box_h as i32 - margin).max(0)),
        TextPosition::BottomRight => (
            (iw - box_w as i32 - margin).max(0),
            (ih - box_h as i32 - margin).max(0),
        ),
        TextPosition::Center => (
            ((iw - box_w as i32) / 2).max(0),
            ((ih - box_h as i32) / 2).max(0),
        ),
    };

    let defaulted = x.is_none() && y.is_none() && position.is_none();
    (x.unwrap_or(anchor_x), y.unwrap_or(anchor_y), defaulted)
}

/// Dispatch a single operation against `img`.
///
/// `warnings` collects non-fatal fallback notices (e.g. defaulted draw_text
/// position) surfaced in the tool response so the LLM can adjust next round.
fn apply_operation(
    img: &mut image::DynamicImage,
    op: &Operation,
    warnings: &mut Vec<String>,
    op_index: usize,
) -> std::result::Result<(), OpError> {
    match op {
        Operation::Crop {
            x,
            y,
            width,
            height,
        } => apply_crop(img, *x, *y, *width, *height),
        Operation::Rotate { angle } => apply_rotate(img, *angle),
        Operation::Flip { direction } => {
            apply_flip(img, *direction);
            Ok(())
        }
        Operation::Resize {
            width,
            height,
            fit,
            background,
        } => apply_resize(img, *width, *height, *fit, background.as_ref().map(|c| c.0)),
        Operation::Adjust {
            brightness,
            contrast,
            saturation,
        } => {
            apply_adjust(img, *brightness, *contrast, *saturation);
            Ok(())
        }
        Operation::Grayscale {} => {
            apply_grayscale(img);
            Ok(())
        }
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
            position,
            text,
            color,
            font_size,
            background,
            padding,
        } => {
            if text.is_empty() || text.chars().count() > 200 {
                return Err(OpError::InvalidText);
            }
            let Some((font, covered, total)) = select_font_for(text) else {
                return Err(OpError::NoFont);
            };
            if covered < total {
                // Missing glyphs rasterize as tofu boxes — surface it instead
                // of letting the model relay a bare "success" for a garbled
                // watermark.
                warnings.push(format!(
                    "operations[{}]: draw_text — best probed font ({}) still lacks glyphs for {}/{} char(s) (e.g. CJK); they will render as boxes. Install a CJK font (Linux: `apt install fonts-noto-cjk` / `dnf install google-noto-sans-cjk-ttc-fonts`)",
                    op_index,
                    font.source,
                    total - covered,
                    total
                ));
            }
            let (text_w, text_h) = measure_text_box(&font.font, *font_size, text);
            let (rx, ry, defaulted) =
                resolve_text_position(img, *x, *y, *position, text_w, text_h, *padding);
            if defaulted {
                warnings.push(format!(
                    "operations[{}]: draw_text x/y omitted — defaulted to centered ({}, {})",
                    op_index, rx, ry
                ));
            }
            apply_draw_text(
                img,
                rx,
                ry,
                text,
                color.0,
                *font_size,
                background.clone().map(|c| c.0),
                *padding,
                &font.font,
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

/// Encode the image to the requested format using default quality.
#[cfg(test)]
fn encode(
    img: &image::DynamicImage,
    fmt: OutputFormat,
) -> Result<(Vec<u8>, &'static str, &'static str)> {
    encode_with_quality(img, fmt, None)
}

/// Encode the image to the requested format with optional JPEG quality (1..=100).
/// Returns `(bytes, mime, extension)`.
fn encode_with_quality(
    img: &image::DynamicImage,
    fmt: OutputFormat,
    quality: Option<u8>,
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
            let q = quality.unwrap_or(85).clamp(1, 100);
            let rgb = to_encode.to_rgb8();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q);
            enc.encode_image(&rgb)
                .map_err(|e| ToolError::Execution(format!("jpeg encode: {}", e)))?;
            Ok((buf.into_inner(), "image/jpeg", "jpg"))
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

fn apply_rotate(img: &mut image::DynamicImage, angle: i32) -> std::result::Result<(), OpError> {
    match angle.rem_euclid(360) {
        0 => Ok(()),
        90 => {
            *img = img.rotate90();
            Ok(())
        }
        180 => {
            *img = img.rotate180();
            Ok(())
        }
        270 => {
            *img = img.rotate270();
            Ok(())
        }
        _ => Err(OpError::InvalidRotateAngle(angle)),
    }
}

fn apply_flip(img: &mut image::DynamicImage, direction: FlipDirection) {
    match direction {
        FlipDirection::Horizontal => *img = img.fliph(),
        FlipDirection::Vertical => *img = img.flipv(),
    }
}

fn apply_resize(
    img: &mut image::DynamicImage,
    width: Option<u32>,
    height: Option<u32>,
    fit: Option<ResizeFit>,
    background: Option<image::Rgba<u8>>,
) -> std::result::Result<(), OpError> {
    use image::imageops::FilterType;
    let (iw, ih) = (img.width(), img.height());
    if iw == 0 || ih == 0 {
        return Err(OpError::InvalidResizeDims(
            "source image has zero size".into(),
        ));
    }

    let validate_dim = |label: &str, v: u32| -> std::result::Result<u32, OpError> {
        if v == 0 || v > MAX_DIM {
            Err(OpError::InvalidResizeDims(format!(
                "{label} must be in 1..={MAX_DIM}, got {v}"
            )))
        } else {
            Ok(v)
        }
    };

    match (width, height) {
        (None, None) => Err(OpError::InvalidResizeDims(
            "provide at least width or height".into(),
        )),
        (Some(w), None) => {
            let w = validate_dim("width", w)?;
            let h = (((ih as u64) * (w as u64) + (iw as u64 / 2)) / (iw as u64)).max(1) as u32;
            let h = validate_dim("computed height", h)?;
            *img = img.resize_exact(w, h, FilterType::Lanczos3);
            Ok(())
        }
        (None, Some(h)) => {
            let h = validate_dim("height", h)?;
            let w = (((iw as u64) * (h as u64) + (ih as u64 / 2)) / (ih as u64)).max(1) as u32;
            let w = validate_dim("computed width", w)?;
            *img = img.resize_exact(w, h, FilterType::Lanczos3);
            Ok(())
        }
        (Some(w), Some(h)) => {
            let w = validate_dim("width", w)?;
            let h = validate_dim("height", h)?;
            match fit {
                None => {
                    *img = img.resize(w, h, FilterType::Lanczos3);
                }
                Some(ResizeFit::Stretch) => {
                    *img = img.resize_exact(w, h, FilterType::Lanczos3);
                }
                Some(ResizeFit::Cover) => {
                    *img = img.resize_to_fill(w, h, FilterType::Lanczos3);
                }
                Some(ResizeFit::Contain) => {
                    let scaled = img.resize(w, h, FilterType::Lanczos3);
                    let bg_color = background.unwrap_or(image::Rgba([255, 255, 255, 255]));
                    let mut canvas = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                        w, h, bg_color,
                    ));
                    let dx = ((w.saturating_sub(scaled.width())) / 2) as i64;
                    let dy = ((h.saturating_sub(scaled.height())) / 2) as i64;
                    image::imageops::overlay(&mut canvas, &scaled, dx, dy);
                    *img = canvas;
                }
            }
            Ok(())
        }
    }
}

fn apply_adjust(
    img: &mut image::DynamicImage,
    brightness: Option<i32>,
    contrast: Option<i32>,
    saturation: Option<i32>,
) {
    if let Some(b) = brightness {
        let clamped = b.clamp(-100, 100);
        if clamped != 0 {
            let delta = ((clamped as f32) * 1.28).round() as i32;
            *img = img.brighten(delta);
        }
    }
    if let Some(c) = contrast {
        let clamped = c.clamp(-100, 100);
        if clamped != 0 {
            *img = img.adjust_contrast(clamped as f32);
        }
    }
    if let Some(s) = saturation {
        let clamped = s.clamp(-100, 100);
        if clamped != 0 {
            let factor = 1.0 + (clamped as f32) / 100.0;
            let mut rgba = img.to_rgba8();
            for p in rgba.pixels_mut() {
                let r = p.0[0] as f32;
                let g = p.0[1] as f32;
                let b = p.0[2] as f32;
                let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                p.0[0] = (luma + (r - luma) * factor).round().clamp(0.0, 255.0) as u8;
                p.0[1] = (luma + (g - luma) * factor).round().clamp(0.0, 255.0) as u8;
                p.0[2] = (luma + (b - luma) * factor).round().clamp(0.0, 255.0) as u8;
            }
            *img = image::DynamicImage::ImageRgba8(rgba);
        }
    }
}

fn apply_grayscale(img: &mut image::DynamicImage) {
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let luma = (0.2126 * p.0[0] as f32 + 0.7152 * p.0[1] as f32 + 0.0722 * p.0[2] as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        p.0[0] = luma;
        p.0[1] = luma;
        p.0[2] = luma;
    }
    *img = image::DynamicImage::ImageRgba8(rgba);
}

/// A parsed system font kept alive for the process lifetime. Only SELECTED
/// winners are ever materialized (see [`FONT_WINNERS`]) — ab_glyph's owned
/// faces each carry a full copy of the source file, so caching every face of
/// every TTC parked ~200MB of fonts on macOS (Hiragino 24.7MB x4 faces +
/// STHeiti 53.2MB x2).
struct FontCandidate {
    font: ab_glyph::FontArc,
    source: &'static str,
}

/// What [`select_font_for`] hands the caller: an Arc HANDLE to the winning
/// face (clone-cheap, shares the cached tables) plus where it came from for
/// diagnostics.
struct FontSelection {
    font: ab_glyph::FontArc,
    source: &'static str,
}

/// A face ADDRESS — path + TTC index, no font data held. The scan reads only
/// file headers, so listing every candidate face is effectively free.
struct FaceAddr {
    path: &'static str,
    index: u32,
}

/// All probed face addresses (scanned once). CJK-capable paths are listed
/// FIRST in [`system_font_paths`] so they win coverage ties.
static FACE_ADDRS: OnceLock<Vec<FaceAddr>> = OnceLock::new();

/// Parsed WINNERS, one per dominant script class (CJK / everything else).
/// draw_text traffic resolves to one of at most two faces in practice, which
/// bounds the font-resident set to two file copies however many TTCs exist.
static FONT_WINNERS: std::sync::Mutex<[Option<FontCandidate>; 2]> =
    std::sync::Mutex::new([None, None]);

#[derive(Clone, Copy, PartialEq)]
enum ScriptClass {
    Cjk,
    Other,
}

fn script_class(text: &str) -> ScriptClass {
    // Any CJK char makes the text CJK-class: mixed watermark strings
    // ("有人 NE302 …") must resolve to the CJK face, never the Latin one.
    let cjk = text.chars().any(|c| {
        matches!(c as u32,
            0x3000..=0x303F   // CJK punctuation
            | 0x3040..=0x30FF // kana
            | 0x3400..=0x4DBF // ext A
            | 0x4E00..=0x9FFF // unified
            | 0xF900..=0xFAFF // compat
            | 0xFF00..=0xFFEF // fullwidth
        )
    });
    if cjk {
        ScriptClass::Cjk
    } else {
        ScriptClass::Other
    }
}

/// Candidate system font files, CJK-capable first. Order matters: for text
/// every font covers equally (pure Latin), the earliest candidate is chosen.
fn system_font_paths() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\msyh.ttf",
            "C:\\Windows\\Fonts\\simhei.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ]
    } else {
        &[
            // Noto CJK — the standard answer on Ubuntu/Debian (fonts-noto-cjk)
            // and Arch-style layouts.
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            // WenQuanYi and Droid fallbacks — common on minimal/headless boxes.
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            // Latin-only last resort (no CJK glyphs — coverage check will
            // surface a warning instead of silently drawing tofu).
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]
    }
}

/// Enumerate candidate faces (headers only, no font data retained). TTC
/// collections contribute one address per face: `try_from_vec` alone would
/// only ever get face 0 (for NotoSansCJK-*.ttc that is the JP variant —
/// legible for Chinese, but the other faces are kept as candidates too).
/// ab_glyph exposes no name table, so faces are distinguished only by glyph
/// coverage downstream.
fn parse_face_addrs(bytes: &[u8], path: &'static str, out: &mut Vec<FaceAddr>) {
    const MAX_CACHED_FACES: usize = 8;
    if out.len() >= MAX_CACHED_FACES {
        return;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"ttcf" {
        // TTC header: tag(4) version(4) numFonts(4 BE) offsets[...]
        let num_faces = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        for index in 0..num_faces.min(8) {
            out.push(FaceAddr { path, index });
            if out.len() >= MAX_CACHED_FACES {
                break;
            }
        }
    } else {
        out.push(FaceAddr { path, index: 0 });
    }
}

/// Scan system font paths and enumerate faces. Reads only the first 12
/// bytes of each existing file — cheap, and nothing is retained.
fn probe_faces() -> &'static [FaceAddr] {
    FACE_ADDRS.get_or_init(|| {
        let mut out = Vec::new();
        for path in system_font_paths() {
            // Header-only read: face enumeration must not load font tables.
            let mut header = [0u8; 12];
            match std::fs::File::open(path)
                .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut header))
            {
                Ok(()) => {
                    let before = out.len();
                    parse_face_addrs(&header, path, &mut out);
                    if out.len() > before {
                        tracing::debug!(
                            font_path = %path,
                            faces = out.len() - before,
                            "image_edit: font discovered"
                        );
                    }
                }
                Err(_) => continue,
            }
        }
        if out.is_empty() {
            tracing::debug!("image_edit: no system font found");
        }
        out
    })
}

/// Parse ONE face into an owned FontArc (one file copy — the unit of memory
/// this module budgets).
fn load_face(addr: &FaceAddr, bytes: Vec<u8>) -> Option<FontCandidate> {
    if addr.index == 0 && !(bytes.len() >= 12 && &bytes[0..4] == b"ttcf") {
        ab_glyph::FontArc::try_from_vec(bytes).ok()
    } else {
        ab_glyph::FontVec::try_from_vec_and_index(bytes, addr.index)
            .ok()
            .map(ab_glyph::FontArc::new)
    }
    .map(|font| FontCandidate {
        font,
        source: addr.path,
    })
}

/// Glyph coverage of `text` in a ZERO-COPY borrowed face (probe without
/// paying the owned-copy). Same counting rules as [`coverage`].
fn coverage_of_ref(font: &ab_glyph::FontRef, text: &str) -> (usize, usize) {
    let mut covered = 0usize;
    let mut total = 0usize;
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if font.glyph_id(ch).0 != 0 {
            covered += 1;
        }
    }
    (covered, total.max(1))
}

/// Available-face predicate for tests (warning paths probe inline).
#[cfg(test)]
fn probe_fonts_available() -> bool {
    !probe_faces().is_empty()
}

/// Glyph coverage of `text` in `font`: (covered, total) over non-whitespace
/// chars. `glyph_id == 0` is .notdef — the tofu box the rasterizer draws for
/// unsupported scripts.
fn coverage(font: &ab_glyph::FontArc, text: &str) -> (usize, usize) {
    let mut covered = 0usize;
    let mut total = 0usize;
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if font.glyph_id(ch).0 != 0 {
            covered += 1;
        }
    }
    (covered, total.max(1))
}

/// Pick the font that covers the most of `text`; ties go to the earlier
/// candidate (CJK-capable paths are ordered first in [`system_font_paths`]).
/// Returns the candidate with its (covered, total) so the caller can warn on
/// partial coverage.
/// Byte-bounded cache of raw font-file bytes for the SLOW probe path (text
/// the cached winners don't fully cover — emoji, rare scripts). Without it
/// every such call re-reads every candidate file from disk (~80MB of
/// transient reads on a stock macOS font set, worse on edge-device SD
/// cards); with it, repeat probes are CPU-only. FIFO eviction, budget well
/// under the two owned winner copies the module already holds.
struct FontBytesCache {
    entries: Vec<(&'static str, Arc<Vec<u8>>)>,
    bytes: usize,
}

impl FontBytesCache {
    const BUDGET: usize = 32 * 1024 * 1024;

    fn get(&self, path: &str) -> Option<Arc<Vec<u8>>> {
        self.entries
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| Arc::clone(b))
    }

    fn put(&mut self, path: &'static str, bytes: Arc<Vec<u8>>) {
        // A single file over budget would evict everything else for one
        // entry — skip caching it (it still gets read this call).
        if bytes.len() > Self::BUDGET {
            return;
        }
        if self.get(path).is_some() {
            return;
        }
        self.bytes += bytes.len();
        self.entries.push((path, bytes));
        while self.bytes > Self::BUDGET {
            let Some((_, evicted)) = self.entries.first() else {
                break;
            };
            self.bytes -= evicted.len();
            self.entries.remove(0);
        }
    }
}

static FONT_BYTES_CACHE: std::sync::Mutex<FontBytesCache> = std::sync::Mutex::new(FontBytesCache {
    entries: Vec::new(),
    bytes: 0,
});

/// Read a font file through [`FONT_BYTES_CACHE`] — hit: Arc clone, miss:
/// disk read + insert. Lock failures degrade to an uncached read.
fn read_font_bytes(path: &'static str) -> Option<Arc<Vec<u8>>> {
    if let Ok(cache) = FONT_BYTES_CACHE.lock() {
        if let Some(hit) = cache.get(path) {
            return Some(hit);
        }
    }
    let bytes = Arc::new(std::fs::read(path).ok()?);
    if let Ok(mut cache) = FONT_BYTES_CACHE.lock() {
        cache.put(path, Arc::clone(&bytes));
    }
    Some(bytes)
}

fn select_font_for(text: &str) -> Option<(FontSelection, usize, usize)> {
    let class = script_class(text) as usize;
    // Fast path: the cached winner for this script class still fully covers
    // the text (re-checked — coverage is per-text, and a mixed follow-up
    // string can outrun a Latin winner).
    if let Ok(winners) = FONT_WINNERS.lock() {
        if let Some(cand) = &winners[class] {
            let (covered, total) = coverage(&cand.font, text);
            if covered == total {
                return Some((
                    FontSelection {
                        font: ab_glyph::FontArc::clone(&cand.font),
                        source: cand.source,
                    },
                    covered,
                    total,
                ));
            }
        }
    }

    // Slow path: transient zero-copy probing of every candidate face, then
    // materialize ONLY the winner (one owned copy) into the class slot.
    let mut best: Option<(FaceAddr, usize, usize)> = None;
    for addr in probe_faces() {
        let Some(bytes) = read_font_bytes(addr.path) else {
            continue;
        };
        let face = if addr.index == 0 && !(bytes.len() >= 12 && &bytes[0..4] == b"ttcf") {
            ab_glyph::FontRef::try_from_slice(&bytes)
        } else {
            ab_glyph::FontRef::try_from_slice_and_index(&bytes, addr.index)
        };
        let Ok(face) = face else { continue };
        let (covered, total) = coverage_of_ref(&face, text);
        let take = match best {
            None => true,
            Some((_, best_covered, _)) => covered > best_covered,
        };
        if take {
            best = Some((
                FaceAddr {
                    path: addr.path,
                    index: addr.index,
                },
                covered,
                total,
            ));
        }
    }
    let (addr, covered, total) = best?;

    // Prefer an already-cached face that ties (memory budget beats re-read).
    if let Ok(winners) = FONT_WINNERS.lock() {
        if let Some(existing) = &winners[class] {
            let (ecov, etot) = coverage(&existing.font, text);
            if ecov >= covered {
                return Some((
                    FontSelection {
                        font: ab_glyph::FontArc::clone(&existing.font),
                        source: existing.source,
                    },
                    ecov,
                    etot,
                ));
            }
        }
    }

    let bytes = read_font_bytes(addr.path)?;
    let cand = load_face(&addr, (*bytes).clone())?;
    let selection = FontSelection {
        font: ab_glyph::FontArc::clone(&cand.font),
        source: cand.source,
    };
    if let Ok(mut winners) = FONT_WINNERS.lock() {
        winners[class] = Some(cand);
    }
    Some((selection, covered, total))
}

/// Porter-Duff `source-over` alpha blend of `src` onto pixel `(x, y)` of `rgba`.
/// Out-of-bounds coordinates are safely ignored.
#[inline]
fn blend_pixel(rgba: &mut image::RgbaImage, x: i32, y: i32, src: image::Rgba<u8>) {
    if x < 0 || y < 0 {
        return;
    }
    let (ux, uy) = (x as u32, y as u32);
    if ux >= rgba.width() || uy >= rgba.height() {
        return;
    }
    let sa = src.0[3];
    if sa == 0 {
        return;
    }
    let dst = rgba.get_pixel_mut(ux, uy);
    if sa == 255 {
        *dst = src;
        return;
    }
    let sa_f = sa as f32 / 255.0;
    let da_f = dst.0[3] as f32 / 255.0;
    let inv_sa = 1.0 - sa_f;
    let out_a = sa_f + da_f * inv_sa;
    if out_a <= 0.0 {
        *dst = image::Rgba([0, 0, 0, 0]);
        return;
    }
    for ch in 0..3 {
        let out_c = (src.0[ch] as f32 * sa_f + dst.0[ch] as f32 * da_f * inv_sa) / out_a;
        dst.0[ch] = out_c.round().clamp(0.0, 255.0) as u8;
    }
    dst.0[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Fill `[x..x+w, y..y+h)` with alpha blending (clipped to image bounds).
fn draw_filled_rect_blended(
    rgba: &mut image::RgbaImage,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: image::Rgba<u8>,
) {
    if w == 0 || h == 0 || color.0[3] == 0 {
        return;
    }
    let iw = rgba.width() as i64;
    let ih = rgba.height() as i64;
    let x0 = (x as i64).max(0).min(iw);
    let y0 = (y as i64).max(0).min(ih);
    let x1 = (x as i64 + w as i64).max(0).min(iw);
    let y1 = (y as i64 + h as i64).max(0).min(ih);
    for py in y0..y1 {
        for px in x0..x1 {
            blend_pixel(rgba, px as i32, py as i32, color);
        }
    }
}

/// Draw a capsule-shaped thick line segment with exact `stroke_width` and
/// single-pass alpha blending (no double-blending at overlapping pixels).
fn draw_thick_segment(
    rgba: &mut image::RgbaImage,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: image::Rgba<u8>,
    stroke_width: u32,
) {
    if color.0[3] == 0 {
        return;
    }
    let iw = rgba.width() as i32;
    let ih = rgba.height() as i32;
    if iw == 0 || ih == 0 {
        return;
    }
    let half = (stroke_width.max(1) as f32) * 0.5;
    let r = if stroke_width <= 1 { 0.55_f32 } else { half };
    let r_sq = r * r;

    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    let min_y = ((y1.min(y2) - r).floor() as i32).max(0);
    let max_y = ((y1.max(y2) + r).ceil() as i32).min(ih - 1);
    let seg_min_x = ((x1.min(x2) - r).floor() as i32).max(0);
    let seg_max_x = ((x1.max(x2) + r).ceil() as i32).min(iw - 1);
    if min_y > max_y || seg_min_x > seg_max_x {
        return;
    }

    // For near-horizontal or short segments, scanline bounding box is tiny; for
    // tall diagonal segments, tighten x-span per row around the line intersection.
    let inv_dy = if dy.abs() > 1e-4 { Some(dx / dy) } else { None };
    let x_pad = if let Some(slope) = inv_dy {
        r * (1.0 + slope * slope).sqrt() + 1.0
    } else {
        0.0
    };

    for py in min_y..=max_y {
        let py_f = py as f32;
        let (row_x0, row_x1) = if let Some(slope) = inv_dy {
            let x_at_y = x1 + (py_f - y1) * slope;
            let lo = ((x_at_y - x_pad).floor() as i32).max(seg_min_x);
            let hi = ((x_at_y + x_pad).ceil() as i32).min(seg_max_x);
            (lo, hi)
        } else {
            (seg_min_x, seg_max_x)
        };
        for px in row_x0..=row_x1 {
            let px_f = px as f32;
            let dist_sq = if len_sq < 1e-6 {
                let ox = px_f - x1;
                let oy = py_f - y1;
                ox * ox + oy * oy
            } else {
                let t = (((px_f - x1) * dx + (py_f - y1) * dy) / len_sq).clamp(0.0, 1.0);
                let cx = x1 + t * dx;
                let cy = y1 + t * dy;
                let ox = px_f - cx;
                let oy = py_f - cy;
                ox * ox + oy * oy
            };
            if dist_sq <= r_sq {
                blend_pixel(rgba, px, py, color);
            }
        }
    }
}

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
    if w == 0 || h == 0 {
        return;
    }
    let mut rgba = img.to_rgba8();
    if let Some(fill_color) = fill {
        draw_filled_rect_blended(&mut rgba, x, y, w, h, fill_color);
    }
    let t = stroke_width.max(1);
    let top_h = t.min(h);
    let bot_h = t.min(h.saturating_sub(top_h));
    // Top and bottom horizontal edges
    draw_filled_rect_blended(&mut rgba, x, y, w, top_h, color);
    if bot_h > 0 {
        let bot_y =
            (y as i64 + h as i64 - bot_h as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        draw_filled_rect_blended(&mut rgba, x, bot_y, w, bot_h, color);
    }
    // Left and right vertical edges (between top and bottom bands — no corner double-blend)
    let mid_h = h.saturating_sub(top_h + bot_h);
    if mid_h > 0 {
        let mid_y = (y as i64 + top_h as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let left_w = t.min(w);
        let right_w = t.min(w.saturating_sub(left_w));
        draw_filled_rect_blended(&mut rgba, x, mid_y, left_w, mid_h, color);
        if right_w > 0 {
            let right_x = (x as i64 + w as i64 - right_w as i64)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            draw_filled_rect_blended(&mut rgba, right_x, mid_y, right_w, mid_h, color);
        }
    }
    *img = image::DynamicImage::ImageRgba8(rgba);
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
    let mut rgba = img.to_rgba8();
    let iw = rgba.width() as i32;
    let ih = rgba.height() as i32;
    let r = radius as f32;
    let t = stroke_width.max(1) as f32;
    let r_out = r + (t - 1.0) * 0.5 + 0.5;
    let r_in = (r - t * 0.5).max(0.0);
    let r_out_sq = r_out * r_out;
    let r_in_sq = r_in * r_in;
    let fill_sq = r * r;

    let min_x = ((cx as f32 - r_out).floor() as i32).max(0);
    let max_x = ((cx as f32 + r_out).ceil() as i32).min(iw - 1);
    let min_y = ((cy as f32 - r_out).floor() as i32).max(0);
    let max_y = ((cy as f32 + r_out).ceil() as i32).min(ih - 1);

    for py in min_y..=max_y {
        let dy = py as f32 - cy as f32;
        let dy_sq = dy * dy;
        for px in min_x..=max_x {
            let dx = px as f32 - cx as f32;
            let dist_sq = dx * dx + dy_sq;
            if let Some(fill_color) = fill {
                if dist_sq <= fill_sq {
                    blend_pixel(&mut rgba, px, py, fill_color);
                }
            }
            if dist_sq <= r_out_sq && dist_sq >= r_in_sq {
                blend_pixel(&mut rgba, px, py, color);
            }
        }
    }
    *img = image::DynamicImage::ImageRgba8(rgba);
    Ok(())
}

fn apply_draw_line(
    img: &mut image::DynamicImage,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: image::Rgba<u8>,
    stroke_width: u32,
) {
    let mut rgba = img.to_rgba8();
    draw_thick_segment(
        &mut rgba,
        x1 as f32,
        y1 as f32,
        x2 as f32,
        y2 as f32,
        color,
        stroke_width,
    );
    *img = image::DynamicImage::ImageRgba8(rgba);
}

#[allow(clippy::too_many_arguments)]
fn apply_draw_arrow(
    img: &mut image::DynamicImage,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: image::Rgba<u8>,
    stroke_width: u32,
    head_length: u32,
) {
    let mut rgba = img.to_rgba8();

    // Main line
    draw_thick_segment(
        &mut rgba,
        x1 as f32,
        y1 as f32,
        x2 as f32,
        y2 as f32,
        color,
        stroke_width,
    );

    // Arrowhead: two lines at ~25 degrees from the main line
    let dx = x2 as f32 - x1 as f32;
    let dy = y2 as f32 - y1 as f32;
    let angle = f32::atan2(dy, dx);
    let head_angle1 = angle + 25.0_f32.to_radians();
    let head_angle2 = angle - 25.0_f32.to_radians();
    let hl = head_length as f32;

    let hx1 = x2 as f32 - hl * head_angle1.cos();
    let hy1 = y2 as f32 - hl * head_angle1.sin();
    let hx2 = x2 as f32 - hl * head_angle2.cos();
    let hy2 = y2 as f32 - hl * head_angle2.sin();

    draw_thick_segment(
        &mut rgba,
        x2 as f32,
        y2 as f32,
        hx1,
        hy1,
        color,
        stroke_width,
    );
    draw_thick_segment(
        &mut rgba,
        x2 as f32,
        y2 as f32,
        hx2,
        hy2,
        color,
        stroke_width,
    );
    *img = image::DynamicImage::ImageRgba8(rgba);
}

fn apply_draw_polygon(
    img: &mut image::DynamicImage,
    points: &[PolygonPoint],
    color: image::Rgba<u8>,
    stroke_width: u32,
    fill: Option<image::Rgba<u8>>,
    closed: bool,
) {
    if points.is_empty() {
        return;
    }

    let mut rgba = img.to_rgba8();
    let ipoints: Vec<imageproc::point::Point<i32>> = points
        .iter()
        .map(|p| imageproc::point::Point { x: p.x, y: p.y })
        .collect();

    if closed {
        if ipoints.len() < 3 {
            tracing::warn!(
                "draw_polygon with closed=true requires >=3 points, got {}. Falling back to line.",
                ipoints.len()
            );
            for window in ipoints.windows(2) {
                draw_thick_segment(
                    &mut rgba,
                    window[0].x as f32,
                    window[0].y as f32,
                    window[1].x as f32,
                    window[1].y as f32,
                    color,
                    stroke_width,
                );
            }
            *img = image::DynamicImage::ImageRgba8(rgba);
            return;
        }
        if let Some(fill_color) = fill {
            let min_x = ipoints.iter().map(|p| p.x).min().unwrap();
            let max_x = ipoints.iter().map(|p| p.x).max().unwrap();
            let min_y = ipoints.iter().map(|p| p.y).min().unwrap();
            let max_y = ipoints.iter().map(|p| p.y).max().unwrap();
            let (iw, ih) = (rgba.width() as i32, rgba.height() as i32);
            let xs_lo = min_x.max(0);
            let xs_hi = max_x.min(iw - 1);
            let ys_lo = min_y.max(0);
            let ys_hi = max_y.min(ih - 1);
            for py in ys_lo..=ys_hi {
                for px in xs_lo..=xs_hi {
                    if point_in_polygon(px, py, &ipoints) {
                        blend_pixel(&mut rgba, px, py, fill_color);
                    }
                }
            }
        }
        let n = ipoints.len();
        for i in 0..n {
            let a = &ipoints[i];
            let b = &ipoints[(i + 1) % n];
            draw_thick_segment(
                &mut rgba,
                a.x as f32,
                a.y as f32,
                b.x as f32,
                b.y as f32,
                color,
                stroke_width,
            );
        }
    } else {
        for window in ipoints.windows(2) {
            draw_thick_segment(
                &mut rgba,
                window[0].x as f32,
                window[0].y as f32,
                window[1].x as f32,
                window[1].y as f32,
                color,
                stroke_width,
            );
        }
    }
    *img = image::DynamicImage::ImageRgba8(rgba);
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
    font: &ab_glyph::FontArc,
) -> std::result::Result<(), OpError> {
    if text.is_empty() || text.chars().count() > 200 {
        return Err(OpError::InvalidText);
    }

    // Measure actual glyph bounds (fixes CJK width over-estimation from UTF-8 byte len).
    let (text_w, text_h) = measure_text_box(font, font_size, text);

    // Draw alpha-blended background if provided
    if let Some(bg) = background {
        let mut rgba = img.to_rgba8();
        draw_filled_rect_blended(
            &mut rgba,
            x,
            y,
            text_w + 2 * padding,
            text_h + 2 * padding,
            bg,
        );
        *img = image::DynamicImage::ImageRgba8(rgba);
    }

    // Draw text at (x + padding, y + padding)
    use imageproc::drawing::draw_text_mut;
    let scale = ab_glyph::PxScale::from(font_size as f32);
    let draw_x = x + padding as i32;
    let draw_y = y + padding as i32;
    draw_text_mut(img, color, draw_x, draw_y, scale, font, text);

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
                            (rs.checked_div(n).unwrap_or(0)) as u8,
                            (gs.checked_div(n).unwrap_or(0)) as u8,
                            (bs.checked_div(n).unwrap_or(0)) as u8,
                            (as_.checked_div(n).unwrap_or(0)) as u8,
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

    /// A throwaway directory for the writer, outside the working tree.
    ///
    /// `std::env::temp_dir()` is not usable here: on macOS it resolves under
    /// `/var/folders/…`, and `image_utils::read_local_image` blocklists `/var/`
    /// (and `/tmp/`), which breaks the read-back these tests exercise. `target/`
    /// is the next best thing — git ignores it, so a run leaves nothing behind
    /// for `git status` to show, and the name carries the pid so two copies of
    /// the suite running at once cannot share a directory.
    ///
    /// `parent()` twice rather than `join("../../target")`: the latter leaves a
    /// literal `..` in the string, and `resolve_image` rejects any path that
    /// contains one.
    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("crates/<crate> sits two levels under the workspace root");
        let dir = workspace
            .join("target")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_preserves_dashes_and_underscores() {
        let n = sanitize_filename("my-snapshot_01", "png").unwrap();
        assert_eq!(n, "my-snapshot_01.png");
    }

    #[test]
    fn write_output_returns_absolute_path() {
        let test_root = scratch_dir("test-tmp-image-edit");
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
        let test_root = scratch_dir("test-tmp-image-edit-named");
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
        let test_root = scratch_dir("test-tmp-image-edit-chain");
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

    /// Regression for the "missing field `x`" failure: a draw_text without
    /// coordinates must succeed (centered) instead of aborting the whole call.
    #[tokio::test]
    async fn draw_text_without_position_defaults_centered() {
        if !probe_fonts_available() {
            eprintln!("skipping: no system font available");
            return;
        }
        let test_root = scratch_dir("test-tmp-image-edit-center");
        let tool = ImageEditTool::new(&test_root);
        let args = serde_json::json!({
            "image": make_test_png_data_url(200, 100),
            "operations": [{ "type": "draw_text", "text": "2026-09-29 11:37" }]
        });
        let out = tool
            .execute(args)
            .await
            .expect("draw_text without x/y must succeed");
        assert_eq!(out.data["status"].as_str(), Some("success"));
        let warnings = out.data["warnings"].as_array().expect("warnings surfaced");
        assert!(
            warnings
                .iter()
                .any(|w| w.as_str().unwrap_or("").contains("centered")),
            "expected a centered-default warning, got: {:?}",
            warnings
        );
        let _ = std::fs::remove_dir_all(&test_root);
    }

    /// Missing required fields must yield an actionable error naming the
    /// operation index and the per-type cheat sheet — not a bare serde
    /// "missing field `x`" the model cannot recover from.
    #[tokio::test]
    async fn missing_required_field_gives_actionable_error() {
        let tool = ImageEditTool::new("/tmp");
        let args = serde_json::json!({
            "image": make_test_png_data_url(20, 20),
            "operations": [{ "type": "draw_text" }]
        });
        let err = tool.execute(args).await.expect_err("must fail");
        let msg = match err {
            ToolError::InvalidArguments(m) => m,
            _ => panic!("expected InvalidArguments"),
        };
        assert!(
            msg.contains("operations[0]"),
            "error must name the failing index: {}",
            msg
        );
        assert!(
            msg.contains("text"),
            "error must name the missing field: {}",
            msg
        );
        assert!(
            msg.contains("crop/draw_rect/blur_rect"),
            "error must carry the cheat sheet: {}",
            msg
        );
    }

    #[tokio::test]
    async fn operation_without_type_gives_actionable_error() {
        let tool = ImageEditTool::new("/tmp");
        let args = serde_json::json!({
            "image": make_test_png_data_url(20, 20),
            "operations": [{}]
        });
        let err = tool.execute(args).await.expect_err("must fail");
        match err {
            ToolError::InvalidArguments(m) => {
                assert!(
                    m.contains("operations[0]") && m.contains("no `type`"),
                    "got: {}",
                    m
                );
            }
            _ => panic!("expected InvalidArguments"),
        }
    }

    /// The "watermark was never visible" failure mode: text drawn far
    /// off-canvas must report `pixels_changed: 0` + a warning instead of a
    /// bare success the model relays as "done".
    #[tokio::test]
    async fn pixels_changed_flags_off_canvas_text() {
        if !probe_fonts_available() {
            eprintln!("skipping: no system font available");
            return;
        }
        let test_root = scratch_dir("test-tmp-image-edit-offcanvas");
        let tool = ImageEditTool::new(&test_root);
        let args = serde_json::json!({
            "image": make_test_png_data_url(100, 100),
            "operations": [
                { "type": "draw_text", "text": "invisible", "x": -5000, "y": -5000 }
            ]
        });
        let out = tool.execute(args).await.expect("execute succeeds");
        let detail = out.data["operations_detail"][0].clone();
        assert_eq!(detail["pixels_changed"].as_u64(), Some(0));
        let warnings = out.data["warnings"].as_array().expect("warnings surfaced");
        assert!(
            warnings
                .iter()
                .any(|w| w.as_str().unwrap_or("").contains("changed 0 pixels")),
            "expected a 0-pixel warning, got: {:?}",
            warnings
        );
        let _ = std::fs::remove_dir_all(&test_root);
    }

    /// A visible edit must report a positive pixel count and no 0-pixel warning.
    #[tokio::test]
    async fn pixels_changed_positive_for_visible_edit() {
        let test_root = scratch_dir("test-tmp-image-edit-visible");
        let tool = ImageEditTool::new(&test_root);
        let args = serde_json::json!({
            "image": make_test_png_data_url(100, 100),
            "operations": [
                { "type": "draw_rect", "x": 10, "y": 10, "width": 20, "height": 20 }
            ]
        });
        let out = tool.execute(args).await.expect("execute succeeds");
        let detail = out.data["operations_detail"][0].clone();
        assert!(
            detail["pixels_changed"].as_u64().unwrap_or(0) > 0,
            "visible rect must change pixels, got: {}",
            detail["pixels_changed"]
        );
        assert!(
            out.data["warnings"]
                .as_array()
                .map(|w| w.is_empty())
                .unwrap_or(true),
            "no 0-pixel warning expected"
        );
        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn count_changed_pixels_handles_layout_change() {
        assert_eq!(
            count_changed_pixels(&[1, 2, 3, 4], &[1, 2, 3, 4], 4),
            Some(0)
        );
        assert_eq!(
            count_changed_pixels(&[1, 2, 3, 4], &[1, 2, 9, 4], 4),
            Some(1)
        );
        // Dimension/layout change (e.g. crop) → lengths differ → None.
        assert_eq!(count_changed_pixels(&[1, 2, 3, 4], &[1, 2], 4), None);
        assert_eq!(count_changed_pixels(&[1], &[1], 0), None);
    }

    /// The compact flat schema must enumerate all 13 operation types, document
    /// per-type required fields in the description, and stay small enough not
    /// to crowd out 8K-window models; runtime `validate_operations` enforces
    /// per-type required fields.
    #[test]
    fn schema_declares_per_type_required_fields() {
        let t = ImageEditTool::new("/tmp");
        let schema = t.parameters();
        let types = schema["properties"]["operations"]["items"]["properties"]["type"]["enum"]
            .as_array()
            .expect("13-value enum in flat operations items schema");
        assert_eq!(types.len(), 13, "all 13 operation types declared");
        for expected in [
            "crop",
            "rotate",
            "flip",
            "resize",
            "adjust",
            "grayscale",
            "draw_rect",
            "draw_circle",
            "draw_line",
            "draw_arrow",
            "draw_polygon",
            "draw_text",
            "blur_rect",
        ] {
            assert!(
                types.iter().any(|v| v == expected),
                "missing operation type {expected}"
            );
        }
        let desc = t.description();
        assert!(desc.contains("crop/draw_rect/blur_rect: x,y,width,height"));
        assert!(desc.contains("rotate: angle"));
        assert!(desc.contains("draw_text: text"));
        // Runtime validation enforces per-type requirements while keeping draw_text x/y optional.
        assert!(validate_operations(&serde_json::json!({
            "image": "$cached:user_image",
            "operations": [{"type": "crop", "x": 0, "y": 0}]
        }))
        .is_err());
        assert!(validate_operations(&serde_json::json!({
            "image": "$cached:user_image",
            "operations": [{"type": "resize"}]
        }))
        .is_err());
        assert!(validate_operations(&serde_json::json!({
            "image": "$cached:user_image",
            "operations": [{"type": "rotate"}]
        }))
        .is_err());
        assert!(validate_operations(&serde_json::json!({
            "image": "$cached:user_image",
            "operations": [{"type": "draw_text"}]
        }))
        .is_err());
        assert!(validate_operations(&serde_json::json!({
            "image": "$cached:user_image",
            "operations": [{"type": "draw_text", "text": "ok"}]
        }))
        .is_ok());
    }

    #[tokio::test]
    async fn rotate_jpeg_preserves_format_and_swaps_dimensions() {
        let test_root = scratch_dir("test-tmp-image-edit-rotate-jpg");
        let tool = ImageEditTool::new(&test_root);

        let mut raw = RgbaImage::from_pixel(120, 80, image::Rgba([20, 40, 60, 255]));
        raw.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(raw)
            .to_rgb8()
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf.into_inner());
        let jpg_url = format!("data:image/jpeg;base64,{}", b64);

        let out = tool
            .execute(serde_json::json!({
                "image": jpg_url,
                "operations": [{"type": "rotate", "angle": 90}],
                "quality": 80
            }))
            .await
            .expect("rotate jpeg should succeed");
        assert_eq!(out.data["width"].as_u64(), Some(80));
        assert_eq!(out.data["height"].as_u64(), Some(120));
        assert_eq!(out.data["image_type"].as_str(), Some("image/jpeg"));
        assert!(
            out.data["url"].as_str().unwrap_or("").ends_with(".jpg"),
            "expected .jpg url, got {:?}",
            out.data["url"]
        );
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
        let Some((sel, _, _)) = select_font_for("Hi") else {
            eprintln!("skipping: no system font available");
            return;
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
            &sel.font,
        )
        .unwrap();
        // (x,y) is the top-left of the background rect — still pure black bg
        assert_eq!(img.get_pixel(10, 10), image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn select_font_covers_drawn_text_when_cjk_font_present() {
        // The field bug: watermark "有人 … 厦门" rendered as tofu because the
        // probe fell back to a Latin-only font. If ANY probed font covers the
        // drawn CJK text, selection must report full coverage.
        let text = "有人 NE302 2026-09-30 厦门市公安局";
        if !probe_fonts_available() {
            eprintln!("skipping: no system font available");
            return;
        }
        let Some((_, covered, total)) = select_font_for(text) else {
            panic!("fonts probed but select_font_for returned None");
        };
        // Transient zero-copy scan: does ANY candidate face cover CJK?
        let any_cjk = probe_faces().iter().any(|addr| {
            let Ok(b) = std::fs::read(addr.path) else {
                return false;
            };
            let face = if addr.index == 0 && !(b.len() >= 12 && &b[0..4] == b"ttcf") {
                ab_glyph::FontRef::try_from_slice(&b)
            } else {
                ab_glyph::FontRef::try_from_slice_and_index(&b, addr.index)
            };
            face.map(|f| coverage_of_ref(&f, "厦门") == (2, 2))
                .unwrap_or(false)
        });
        if !any_cjk {
            eprintln!("skipping: no CJK-capable font probed (expected on bare CI)");
            return;
        }
        assert_eq!(
            covered, total,
            "CJK-capable font present but selection lacks glyphs"
        );
    }

    #[test]
    fn cjk_draw_text_changes_pixels() {
        if !probe_fonts_available() {
            eprintln!("skipping: no system font available");
            return;
        }
        let any_cjk = probe_faces().iter().any(|addr| {
            let Ok(b) = std::fs::read(addr.path) else {
                return false;
            };
            let face = if addr.index == 0 && !(b.len() >= 12 && &b[0..4] == b"ttcf") {
                ab_glyph::FontRef::try_from_slice(&b)
            } else {
                ab_glyph::FontRef::try_from_slice_and_index(&b, addr.index)
            };
            face.map(|f| coverage_of_ref(&f, "厦门") == (2, 2))
                .unwrap_or(false)
        });
        if !any_cjk {
            eprintln!("skipping: no CJK-capable font probed");
            return;
        }
        let mut img = solid(200, 60, [0, 0, 0, 255]);
        let before = img.as_bytes().to_vec();
        apply_draw_text(
            &mut img,
            10,
            10,
            "厦门市公安局",
            image::Rgba([255, 255, 255, 255]),
            24,
            None,
            0,
            &select_font_for("厦门市公安局").unwrap().0.font,
        )
        .unwrap();
        assert_ne!(img.as_bytes(), before.as_slice(), "CJK text drew no pixels");
    }

    /// Parses the exact font the server deployment uses. Opt-in via
    /// NEOMIND_TEST_CJK_TTC=/path/to/NotoSansCJK-Regular.ttc (CI has no CJK
    /// fonts); run locally after `apt install fonts-noto-cjk`.
    #[test]
    fn noto_cjk_ttc_exposes_all_faces_with_full_hanzi_coverage() {
        let Ok(path) = std::env::var("NEOMIND_TEST_CJK_TTC") else {
            eprintln!("skipping: NEOMIND_TEST_CJK_TTC not set");
            return;
        };
        let bytes = std::fs::read(&path).expect("read NEOMIND_TEST_CJK_TTC");
        let mut addrs = Vec::new();
        parse_face_addrs(&bytes, "env", &mut addrs);
        assert!(
            addrs.len() >= 2,
            "ttc must expose multiple faces, got {}",
            addrs.len()
        );
        // Every face of NotoSansCJK covers the full pan-CJK charset — the
        // field bug (tofu) must be impossible once this font is probed.
        for addr in &addrs {
            let face =
                load_face(addr, std::fs::read(&path).expect("re-read face")).expect("face parses");
            assert_eq!(
                coverage(&face.font, "厦门有人公安"),
                (6, 6),
                "face {}[{}] lacks hanzi coverage",
                face.source,
                addr.index
            );
        }
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

    #[test]
    fn rotate_90_180_270_and_negative() {
        let mut img = solid(100, 50, [0, 0, 0, 255]);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        apply_rotate(&mut img, 90).unwrap();
        assert_eq!(img.dimensions(), (50, 100));
        // Top-left (0, 0) rotated 90° CW lands at top-right (49, 0)
        assert_eq!(img.get_pixel(49, 0), Rgba([255, 0, 0, 255]));

        // -90° rotates back to original (100, 50) with red at (0, 0)
        apply_rotate(&mut img, -90).unwrap();
        assert_eq!(img.dimensions(), (100, 50));
        assert_eq!(img.get_pixel(0, 0), Rgba([255, 0, 0, 255]));

        // Non-multiple of 90 rejected cleanly
        assert!(matches!(
            apply_rotate(&mut img, 45),
            Err(OpError::InvalidRotateAngle(45))
        ));
    }

    #[test]
    fn flip_horizontal_and_vertical() {
        let mut img = solid(40, 20, [0, 0, 0, 255]);
        img.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        apply_flip(&mut img, FlipDirection::Horizontal);
        assert_eq!(img.get_pixel(39, 0), Rgba([10, 20, 30, 255]));
        apply_flip(&mut img, FlipDirection::Vertical);
        assert_eq!(img.get_pixel(39, 19), Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn resize_aspect_contain_cover_and_stretch() {
        // Single dimension keeps aspect ratio (200x100 -> width 100 => 100x50)
        let mut a = solid(200, 100, [100, 100, 100, 255]);
        apply_resize(&mut a, Some(100), None, None, None).unwrap();
        assert_eq!(a.dimensions(), (100, 50));

        // Cover crops to exact target (200x100 -> 80x80)
        let mut b = solid(200, 100, [50, 100, 150, 255]);
        apply_resize(&mut b, Some(80), Some(80), Some(ResizeFit::Cover), None).unwrap();
        assert_eq!(b.dimensions(), (80, 80));

        // Contain pads to exact target with requested background color
        let mut c = solid(200, 100, [10, 10, 10, 255]);
        apply_resize(
            &mut c,
            Some(100),
            Some(100),
            Some(ResizeFit::Contain),
            Some(Rgba([255, 255, 0, 255])),
        )
        .unwrap();
        assert_eq!(c.dimensions(), (100, 100));
        assert_eq!(c.get_pixel(0, 0), Rgba([255, 255, 0, 255]));
        assert_eq!(c.get_pixel(50, 50), Rgba([10, 10, 10, 255]));

        // Stretch hits exact dimensions
        let mut d = solid(200, 100, [0, 0, 0, 255]);
        apply_resize(&mut d, Some(60), Some(90), Some(ResizeFit::Stretch), None).unwrap();
        assert_eq!(d.dimensions(), (60, 90));
    }

    #[test]
    fn adjust_and_grayscale_modify_pixels() {
        let mut img = solid(20, 20, [200, 50, 50, 255]);
        apply_grayscale(&mut img);
        let p = img.get_pixel(10, 10);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 255);

        let mut bright = solid(20, 20, [100, 100, 100, 255]);
        apply_adjust(&mut bright, Some(30), Some(10), None);
        assert!(bright.get_pixel(10, 10)[0] > 100);

        let mut sat = solid(20, 20, [200, 50, 50, 255]);
        apply_adjust(&mut sat, None, None, Some(-100));
        let ps = sat.get_pixel(10, 10);
        assert_eq!(ps[0], ps[1]);
        assert_eq!(ps[1], ps[2]);
    }

    #[test]
    fn stroke_width_and_alpha_blend_are_honored() {
        // Thick 6px red border on black canvas: pixel at (13, 13) (3px inward) must be red!
        let mut img = solid(100, 100, [0, 0, 0, 255]);
        apply_draw_rect(&mut img, 10, 10, 60, 60, Rgba([255, 0, 0, 255]), 6, None);
        assert_eq!(img.get_pixel(13, 13), Rgba([255, 0, 0, 255]));
        // Interior beyond 6px stays black
        assert_eq!(img.get_pixel(30, 30), Rgba([0, 0, 0, 255]));

        // Semi-transparent blue fill over white background blends (~50% blue) instead of erasing white!
        let mut blend_img = solid(100, 100, [255, 255, 255, 255]);
        apply_draw_rect(
            &mut blend_img,
            10,
            10,
            40,
            40,
            Rgba([0, 0, 255, 255]),
            1,
            Some(Rgba([0, 0, 255, 128])),
        );
        let mid = blend_img.get_pixel(30, 30);
        assert!(
            mid[0] > 100 && mid[0] < 150 && mid[2] == 255 && mid[3] == 255,
            "expected ~50% alpha-blended blue over white, got {:?}",
            mid
        );
    }

    #[test]
    fn draw_text_accepts_more_than_67_cjk_chars() {
        // 80 CJK characters = 240 UTF-8 bytes (> 200 bytes) but only 80 chars (<= 200 chars).
        let text = "测".repeat(80);
        assert!(text.len() > 200 && text.chars().count() == 80);
        let Some((sel, _, _)) = select_font_for(&text) else {
            return;
        };
        let mut img = solid(400, 80, [0, 0, 0, 255]);
        apply_draw_text(
            &mut img,
            10,
            10,
            &text,
            Rgba([255, 255, 255, 255]),
            16,
            None,
            2,
            &sel.font,
        )
        .expect("80 CJK chars must not hit byte-length limit");
    }

    #[test]
    fn resolve_text_position_honors_named_anchors() {
        let img = solid(400, 300, [0, 0, 0, 255]);
        let (tlx, tly, d_tl) =
            resolve_text_position(&img, None, None, Some(TextPosition::TopLeft), 80, 20, 4);
        let (brx, bry, d_br) =
            resolve_text_position(&img, None, None, Some(TextPosition::BottomRight), 80, 20, 4);
        assert!(!d_tl && !d_br, "explicit position must not flag defaulted");
        assert!(tlx < 30 && tly < 30);
        assert!(brx > 280 && bry > 240);
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    fn make_test_png_data_url(w: u32, h: u32) -> String {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf.into_inner());
        format!("data:image/png;base64,{}", b64)
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let dir = workspace
            .join("target")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_color_accepts_all_documented_forms() {
        assert_eq!(parse_color("#FF0000"), Some(image::Rgba([255, 0, 0, 255])));
        assert_eq!(parse_color("00ff00"), Some(image::Rgba([0, 255, 0, 255])));
        assert_eq!(parse_color("#00000080"), Some(image::Rgba([0, 0, 0, 128])));
        // The exact field failure: CSS rgba with fractional alpha.
        assert_eq!(
            parse_color("rgba(0, 0, 0, 0.7)"),
            Some(image::Rgba([0, 0, 0, 179]))
        );
        // rgb() = opaque; alpha 0-255 also accepted.
        assert_eq!(
            parse_color("rgb(10, 20, 30)"),
            Some(image::Rgba([10, 20, 30, 255]))
        );
        assert_eq!(
            parse_color("rgba(1, 2, 3, 128)"),
            Some(image::Rgba([1, 2, 3, 128]))
        );
        assert!(parse_color("red").is_none());
        assert!(parse_color("rgba(300, 0, 0, 1)").is_none());
        assert!(parse_color("#GGGGGG").is_none());
        assert!(parse_color("").is_none());
    }

    /// The flail-prevention contract: a bad color names the field, the value,
    /// and the accepted forms — not serde's "invalid digit found in string".
    #[tokio::test]
    async fn bad_color_error_names_field_and_forms() {
        let tool = ImageEditTool::new("/tmp");
        let args = serde_json::json!({
            "image": make_test_png_data_url(20, 20),
            "operations": [
                { "type": "draw_text", "text": "x", "background": "notacolor" }
            ]
        });
        let err = tool.execute(args).await.expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("background"), "{msg}");
        assert!(msg.contains("notacolor"), "{msg}");
        assert!(msg.contains("#RRGGBB"), "{msg}");
    }

    /// End-to-end: the CSS rgba form that used to hard-fail now renders.
    #[tokio::test]
    async fn rgba_background_text_renders() {
        if !probe_fonts_available() {
            eprintln!("skipping: no system font available");
            return;
        }
        let test_root = scratch_dir("test-tmp-image-edit-rgba");
        let tool = ImageEditTool::new(&test_root);
        let args = serde_json::json!({
            "image": make_test_png_data_url(100, 100),
            "operations": [
                { "type": "draw_text", "text": "CamThink", "background": "rgba(0,0,0,0.7)", "font_size": 24 }
            ]
        });
        let out = tool
            .execute(args)
            .await
            .expect("rgba background must succeed");
        assert_eq!(out.data["status"].as_str(), Some("success"));
        assert!(
            out.data["operations_detail"][0]["pixels_changed"]
                .as_u64()
                .unwrap_or(0)
                > 0,
            "text+background must change pixels"
        );
        let _ = std::fs::remove_dir_all(&test_root);
    }
}
