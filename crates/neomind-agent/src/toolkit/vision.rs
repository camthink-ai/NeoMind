//! Vision tool for multi-modal image analysis using VLM backends.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use neomind_core::llm::backend::{LlmInput, LlmRuntime};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};
use neomind_core::tools::ToolCategory;
use serde_json::Value;
use tokio::sync::RwLock;

use super::error::{Result, ToolError};
use super::tool::{object_schema, string_property, Tool, ToolOutput};
use crate::image_utils::{is_private_host, resolve_image};
use crate::llm_backends::LlmBackendInstanceManager;

/// Maximum image size in bytes (10 MB). VLMs typically downsample to
/// ~448-672px anyway, so 10 MB is more than sufficient.
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Configuration for the vision tool.
#[derive(Debug, Clone)]
pub struct VisionConfig {
    /// Whether the vision tool is enabled (controlled by auto-detection).
    pub enabled: bool,
    /// Optional: explicit VLM backend ID to use.
    pub vlm_backend_id: Option<String>,
    /// Maximum tokens for VLM response (default 1024).
    pub max_tokens: u32,
    /// Timeout for HTTP image fetch in seconds (default 10).
    pub capture_timeout_secs: u64,
    /// Maximum image dimension (width/height) in pixels for VLM dispatch (default 1280).
    /// Images fetched via HTTP or read from disk are downscaled to fit within this
    /// box before base64 encoding. VLMs internally downsample to ~448-672px, so
    /// 1280 preserves full perceivable detail while cutting bandwidth 3-5x on
    /// cloud backends. Set to 0 to disable resizing (send original).
    pub max_image_dim: u32,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            vlm_backend_id: None,
            max_tokens: 1024,
            capture_timeout_secs: crate::toolkit::timeouts::vision_capture().as_secs(),
            max_image_dim: 1280,
        }
    }
}

/// Vision tool — analyzes images using a vision-language model.
///
/// Accepts base64 data, data URLs, local file paths, or HTTP/HTTPS URLs.
/// Returns natural language analysis of the image content.
/// Process-local health tracking for VLM candidate backends. Lets
/// `resolve_vlm_candidates` skip backends that recently failed in a way that
/// either won't self-heal soon (404 model-missing, 403/quota exhausted) or
/// just needs a short cool-down (429 rate limit, a "can't see the image"
/// reply from a fake-multimodal model). Every window is best-effort: when
/// all candidates are blocked, `resolve_vlm_candidates` falls back to the
/// full list, so vision never hard-deadlocks on stale state.
#[derive(Debug, Default)]
struct VlmHealthTable {
    /// Hard failures (404 model-not-found, 403/quota). Won't recover without
    /// admin action (reinstall model, top up quota), so a long window.
    hard: HashMap<String, Instant>,
    /// "Produced no usable analysis" — a model marked multimodal that can't
    /// actually see images. Stable per model, but shorter than hard so a
    /// future model swap / re-check still gets a chance.
    cant_see: HashMap<String, Instant>,
    /// Transient: 429 rate limit / network. Recovers quickly.
    rate_limited: HashMap<String, Instant>,
}

impl VlmHealthTable {
    const HARD_TTL: Duration = Duration::from_secs(600);
    const CANT_SEE_TTL: Duration = Duration::from_secs(300);
    const RATE_TTL: Duration = Duration::from_secs(60);

    /// Record a runtime error from a candidate. Classifies by content:
    /// 404 / model-not-found / 403 / quota → hard; 429 / rate-limit → rate;
    /// anything else is ignored (don't penalize one-off unknown errors).
    fn record_failure(&mut self, id: &str, err: &str) {
        let e = err.to_lowercase();
        let now = Instant::now();
        if e.contains("404")
            || e.contains("not found")
            || e.contains("insufficient_quota")
            || e.contains("free quota")
            || e.contains("freetieronly")
            || e.contains("quota")
            || e.contains("403")
        {
            self.hard.insert(id.to_string(), now);
        } else if e.contains("429") || e.contains("rate limit") || e.contains("rate_limit") {
            self.rate_limited.insert(id.to_string(), now);
        }
        // Unknown error → don't penalize (could be a one-off transient).
    }

    /// Record a "produced no usable analysis" (empty or can't-see reply).
    fn record_cant_see(&mut self, id: &str) {
        self.cant_see.insert(id.to_string(), Instant::now());
    }

    /// Clear all failure records for a backend that just succeeded.
    fn clear(&mut self, id: &str) {
        self.hard.remove(id);
        self.cant_see.remove(id);
        self.rate_limited.remove(id);
    }

    /// True if `id` is still inside any failure cool-down window.
    fn is_blocked(&self, id: &str) -> bool {
        let now = Instant::now();
        Self::within(self.hard.get(id), Self::HARD_TTL, now)
            || Self::within(self.cant_see.get(id), Self::CANT_SEE_TTL, now)
            || Self::within(self.rate_limited.get(id), Self::RATE_TTL, now)
    }

    fn within(ts: Option<&Instant>, ttl: Duration, now: Instant) -> bool {
        match ts {
            Some(t) => now.duration_since(*t) < ttl,
            None => false,
        }
    }
}

pub struct VisionTool {
    llm_manager: Arc<LlmBackendInstanceManager>,
    config: VisionConfig,
    http_client: reqwest::Client,
    /// Process-local VLM candidate health: backends that recently failed in a
    /// non-self-healing way (404 model-missing, 403/quota) or that need a
    /// short cool-down (429, "can't see the image"). Lets
    /// `resolve_vlm_candidates` skip known-bad backends instead of re-trying
    /// them every call — each 429 also pays a multi-second backoff.
    health: Arc<RwLock<VlmHealthTable>>,
}

impl VisionTool {
    /// Create a new vision tool.
    pub fn new(config: VisionConfig, llm_manager: Arc<LlmBackendInstanceManager>) -> Self {
        // Custom redirect policy: validate each redirect target against SSRF rules,
        // matching the pattern used by WebFetchTool.
        let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url().clone();
            if let Err(e) = validate_url(&url) {
                tracing::warn!(url = %url, error = %e, "Vision: redirect blocked by SSRF check");
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Redirect to '{}' blocked: {}", url, e),
                ));
            }
            if attempt.previous().len() >= 5 {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Too many redirects",
                ));
            }
            attempt.follow()
        });

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.capture_timeout_secs))
            .redirect(redirect_policy)
            .no_proxy()
            .build()
            .expect("Failed to build reqwest client for vision tool");

        Self {
            llm_manager,
            config,
            http_client,
            health: Arc::new(RwLock::new(VlmHealthTable::default())),
        }
    }

    /// Resolve VLM candidate runtimes for image analysis, in priority order.
    ///
    /// Returns MULTIPLE candidates so `analyze` can fall through on a 404
    /// (model marked multimodal but not actually installed) to the next
    /// multimodal backend, instead of failing the whole tool on one bad backend.
    ///
    /// Priority order (de-duplicated):
    /// 1. Explicit `vlm_backend_id` in config (and `model` param, handled above)
    /// 2. Other multimodal-capable instances (dedicated VLMs)
    /// 3. Current active backend (fallback) — tried last because a backend's
    ///    `supports_multimodal` flag can be wrong (a text model wrongly marked
    ///    multimodal); preferring a dedicated VLM avoids burning a round on the
    ///    active backend's false claim before health-tracking demotes it.
    async fn resolve_vlm_candidates(
        &self,
        model: Option<&str>,
    ) -> Result<Vec<(String, Arc<dyn LlmRuntime>)>> {
        let mut candidates: Vec<(String, Arc<dyn LlmRuntime>)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 0. Explicit `model` param — pin a specific VLM by model name. Use
        //    this when the auto-picked model can't handle images (text-only
        //    model wrongly marked multimodal). Case-insensitive substring on
        //    inst.model. Pinned first so it's tried before the fallbacks.
        if let Some(m) = model {
            let needle = m.to_lowercase();
            for inst in self.llm_manager.list_instances() {
                if inst.model.to_lowercase().contains(&needle) && !seen.contains(&inst.id) {
                    if let Ok(rt) = self.llm_manager.get_runtime(&inst.id).await {
                        seen.insert(inst.id.clone());
                        candidates.push((inst.id.clone(), rt));
                    }
                }
            }
        }

        // 1. Explicit backend ID
        if let Some(ref id) = self.config.vlm_backend_id {
            if let Ok(rt) = self.llm_manager.get_runtime(id).await {
                seen.insert(id.clone());
                candidates.push((id.clone(), rt));
            }
        }

        // 2. Other multimodal-capable instances (dedicated VLMs) — preferred
        //    over the active backend. A backend's `supports_multimodal` flag
        //    can be wrong (text model wrongly marked multimodal), and the
        //    active chat backend is the most common offender, so trying a
        //    dedicated VLM first avoids a wasted failed round before
        //    health-tracking demotes the fake-multimodal active backend.
        for inst in self.llm_manager.list_instances() {
            if inst.capabilities.supports_multimodal && !seen.contains(&inst.id) {
                if let Ok(rt) = self.llm_manager.get_runtime(&inst.id).await {
                    seen.insert(inst.id.clone());
                    candidates.push((inst.id.clone(), rt));
                }
            }
        }

        // 3. Active backend (fallback) — only reached if no dedicated VLM was
        //    found above. Gated on supports_multimodal; if that flag is a false
        //    positive, health-tracking demotes it after one failure.
        if let Some(active) = self.llm_manager.get_active_instance() {
            if active.capabilities.supports_multimodal && !seen.contains(&active.id) {
                if let Ok(rt) = self.llm_manager.get_runtime(&active.id).await {
                    seen.insert(active.id.clone());
                    candidates.push((active.id.clone(), rt));
                }
            }
        }

        if candidates.is_empty() {
            return Err(ToolError::Execution(
                "No vision model configured. Install a VLM (e.g., qwen2.5-vl, minicpm-v) via Ollama and `neomind llm activate` it.".into(),
            ));
        }
        // Drop backends still inside a failure cool-down window, so we don't
        // re-try a 404-model / quota-exhausted / rate-limited backend on every
        // call. If ALL candidates are blocked, keep the full list rather than
        // hard-failing — a window may be about to expire, or the user pinned a
        // `model` that's temporarily blocked and deserves one attempt.
        let blocked: Vec<String> = {
            let h = self.health.read().await;
            candidates
                .iter()
                .filter_map(|(id, _)| {
                    if h.is_blocked(id) {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        if blocked.len() < candidates.len() {
            candidates.retain(|(id, _)| !blocked.contains(id));
        }
        Ok(candidates)
    }

    /// Resolve image input to (base64_data, mime_type).
    ///
    /// Wraps the free `image_utils::resolve_image` (raw bytes) and applies
    /// vision's resize-on-input behavior via `process_image_bytes`. The free
    /// function returns raw bytes; vision base64-encodes (after optional
    /// Lanczos3 downscale per `config.max_image_dim`) for VLM dispatch.
    async fn resolve_image(&self, image: &str) -> Result<(String, String)> {
        let (bytes, mime): (Vec<u8>, String) =
            resolve_image(image, &self.http_client, MAX_IMAGE_SIZE)
                .await
                .map_err(ToolError::from)?;
        Ok(self.process_image_bytes(bytes, &mime).await)
    }

    /// Process raw image bytes: optionally resize to `max_image_dim`, then
    /// base64-encode. The decode/resize/encode pipeline is CPU-intensive
    /// (Lanczos3 on multi-MB images can take 100-500ms), so it runs on
    /// `spawn_blocking` to avoid starving the tokio runtime.
    ///
    /// Fail-open: resize errors fall back to the original bytes. The only
    /// hard error path is `spawn_blocking` itself failing (JoinError), which
    /// is astronomically rare (would require a panic inside the image crate).
    async fn process_image_bytes(&self, bytes: Vec<u8>, detected_mime: &str) -> (String, String) {
        let max_dim = self.config.max_image_dim;
        let mime_owned = detected_mime.to_string();
        match tokio::task::spawn_blocking(move || {
            let (final_bytes, mime) = resize_image_if_needed(bytes, &mime_owned, max_dim);
            let b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &final_bytes);
            (b64, mime)
        })
        .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(error = %e, "spawn_blocking panicked during image processing; image lost");
                // Bytes were moved into the closure and are unrecoverable on panic.
                // Return an explicit empty result so the caller surfaces a VLM error
                // rather than silently sending corrupt data.
                (String::new(), detected_mime.to_string())
            }
        }
    }

    /// Run VLM analysis on the resolved image.
    ///
    /// Tries each candidate VLM backend in priority order. A failure on one
    /// (404 model-not-found, empty response, etc.) falls through to the next
    /// — a backend can be marked multimodal yet have its model uninstalled,
    /// and that shouldn't kill the whole tool. Only when ALL candidates fail
    /// is a clear error returned naming every backend tried.
    async fn analyze(
        &self,
        data: &str,
        mime: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String> {
        let candidates = self.resolve_vlm_candidates(model).await?;
        let mut errors: Vec<String> = Vec::new();

        for (id, runtime) in &candidates {
            let msg = Message::new(
                MessageRole::User,
                Content::Parts(vec![
                    ContentPart::text(prompt),
                    ContentPart::image_base64(data, mime),
                ]),
            );
            let input = LlmInput {
                messages: vec![msg],
                params: neomind_core::llm::GenerationParams {
                    max_tokens: Some(self.config.max_tokens as usize),
                    ..Default::default()
                },
                model: None,
                stream: false,
                tools: None,
            };
            match runtime.generate(input).await {
                Ok(output)
                    if !output.text.trim().is_empty()
                        && !looks_like_vision_failure(&output.text) =>
                {
                    // Recovered: clear any stale failure record so this
                    // backend is preferred again next time.
                    self.health.write().await.clear(id);
                    return Ok(output.text);
                }
                Ok(output) => {
                    // Empty, OR a non-empty "I can't see the image" reply —
                    // typically a text-only model that silently ignored the
                    // image bytes. Treat as failure so we fall through to the
                    // next (hopefully truly multimodal) candidate, and cool
                    // this backend down so we don't keep asking a blind model.
                    self.health.write().await.record_cant_see(id);
                    tracing::warn!(
                        backend_id = %id,
                        analysis_preview = %output.text.chars().take(80).collect::<String>(),
                        "VLM candidate produced no usable analysis, trying next"
                    );
                    errors.push(format!(
                        "{}: {}",
                        id,
                        output.text.chars().take(120).collect::<String>()
                    ));
                }
                Err(e) => {
                    let m = e.to_string();
                    self.health.write().await.record_failure(id, &m);
                    tracing::warn!(backend_id = %id, error = %m, "VLM candidate failed, trying next");
                    errors.push(format!("{}: {}", id, m));
                }
            }
        }

        Err(ToolError::Execution(format!(
            "VLM inference failed on all {} candidate backend(s) — {}. Activate a working multimodal backend via `neomind llm activate <id>` (its model must be installed in Ollama).",
            candidates.len(),
            errors.join("; ")
        )))
    }
}

#[async_trait]
impl Tool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }

    fn description(&self) -> &str {
        r#"Analyze images you CANNOT already see, using a vision-language model. Pass an image source and get back a description/analysis.

Use this tool when you need to analyze images from sources other than the current message:
- `$cached:xxx` — a cache ref returned by other tools (e.g. `device get --metric <image_field>` output, or extension image outputs). Pass it directly as `image`; it resolves to the full image bytes. Prefer this over base64-decoding/saving files yourself.
- HTTP/HTTPS image URLs (e.g., camera snapshots, web images) — fetches automatically, private/local URLs blocked
- /path/to/file.jpg — local image file on disk (must have an image extension)
- data:image/...;base64,... — base64 data URL from extension outputs
- raw base64 string — decoded as JPEG by default

Do NOT call this for images you can already see — analyze those yourself directly (e.g. images uploaded by the user in chat, or embedded in the current message from bound data sources — those are already visible to you).

Optional `model` param (e.g. "minicpm-v4.6") forces a specific VLM backend. Use it when the auto-picked model replies it can't see the image (a text-only model wrongly selected). Find available model names via `neomind llm list`."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "image": string_property("Image source: base64 data URL, raw base64, local image file path, or public HTTP/HTTPS URL"),
                "prompt": string_property("Analysis instructions for the image. Match the user's language and detail level from the conversation."),
                "model": serde_json::json!({"type": "string", "description": "Optional VLM model name to force (e.g. `minicpm-v4.6`). Use when the auto-picked model says it can't see the image. Find names via `neomind llm list`."})
            }),
            vec!["image".to_string(), "prompt".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let image = args["image"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("image is required".into()))?;
        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("prompt is required".into()))?;
        let model = args.get("model").and_then(|v| v.as_str());

        if image.trim().is_empty() {
            return Err(ToolError::InvalidArguments("image cannot be empty".into()));
        }
        if prompt.trim().is_empty() {
            return Err(ToolError::InvalidArguments("prompt cannot be empty".into()));
        }

        tracing::info!(image_len = image.len(), "Vision tool: resolving image");

        let (data, mime) = self.resolve_image(image).await?;
        let analysis = self.analyze(&data, &mime, prompt, model).await?;

        Ok(ToolOutput::success(serde_json::json!({
            "analysis": analysis,
            "image_type": mime,
        })))
    }
}

// ---------------------------------------------------------------------------
// Vision-only helpers
// ---------------------------------------------------------------------------

/// Heuristic: detect a VLM reply that means "I didn't actually receive/see
/// the image". A text-only model wrongly marked multimodal ignores the image
/// bytes and replies with variants of "I can't view images / 我无法查看图片".
/// Treat such replies as failure so `analyze` falls through to the next
/// candidate instead of returning a useless "I can't see it" as the result.
fn looks_like_vision_failure(text: &str) -> bool {
    let lower = text.to_lowercase();
    const MARKERS: &[&str] = &[
        "cannot view",
        "can't view",
        "cannot see",
        "can't see",
        "unable to view",
        "unable to see",
        "cannot process image",
        "i don't see",
        "i do not see",
        "no image",
        "image is not",
        "not visible",
        "cannot display",
        "cannot access",
        "no access to",
        "无法查看",
        "看不到",
        "无法看到",
        "无法识别图片",
        "没有权限加载",
        "无法直接查看",
        "无法解析",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

// ---------------------------------------------------------------------------
// Shared helpers (SSRF protection + MIME detection)
// ---------------------------------------------------------------------------

/// Validate a reqwest::Url against SSRF rules.
fn validate_url(url: &reqwest::Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(ToolError::PermissionDenied(
                "Only http:// and https:// URLs are allowed".into(),
            ))
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| ToolError::InvalidArguments("URL has no host".into()))?;

    if is_private_host(host) {
        return Err(ToolError::PermissionDenied(format!(
            "Access to private address '{}' is not allowed",
            host
        )));
    }

    Ok(())
}

/// Resize image bytes if either dimension exceeds `max_dim`.
///
/// - If `max_dim == 0` or the image is already within bounds, returns the
///   original bytes with the detected MIME unchanged.
/// - Otherwise resizes with Lanczos3 and re-encodes as JPEG (mime → `image/jpeg`).
/// - **Fail-open**: any decode/encode error logs a warning and returns the
///   original bytes so the VLM still receives the image. This covers
///   formats the `image` crate can't decode (e.g. TIFF/BMP/HEIC when the
///   feature isn't enabled) without blocking the analysis.
fn resize_image_if_needed(bytes: Vec<u8>, detected_mime: &str, max_dim: u32) -> (Vec<u8>, String) {
    if max_dim == 0 || bytes.is_empty() {
        return (bytes, detected_mime.to_string());
    }
    let img = match image::load_from_memory(&bytes) {
        Ok(img) => img,
        Err(e) => {
            tracing::debug!(
                error = %e, mime = %detected_mime, bytes = bytes.len(),
                "Image decode failed for resize; sending original bytes"
            );
            return (bytes, detected_mime.to_string());
        }
    };
    let (w, h) = (img.width(), img.height());
    if w <= max_dim && h <= max_dim {
        return (bytes, detected_mime.to_string());
    }
    let resized = img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3);
    let mut buf = std::io::Cursor::new(Vec::new());
    match resized.write_to(&mut buf, image::ImageFormat::Jpeg) {
        Ok(()) => {
            let new_bytes = buf.into_inner();
            tracing::info!(
                orig_w = w,
                orig_h = h,
                new_w = resized.width(),
                new_h = resized.height(),
                orig_bytes = bytes.len(),
                new_bytes = new_bytes.len(),
                "Resized image for VLM dispatch"
            );
            (new_bytes, "image/jpeg".to_string())
        }
        Err(e) => {
            tracing::warn!(error = %e, "JPEG re-encode failed after resize; sending original bytes");
            (bytes, detected_mime.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vision_config_default() {
        let config = VisionConfig::default();
        assert!(config.enabled);
        assert!(config.vlm_backend_id.is_none());
        assert_eq!(config.max_tokens, 1024);
        assert_eq!(config.capture_timeout_secs, 10);
    }

    #[test]
    fn test_validate_url_blocks_ftp() {
        let url = reqwest::Url::parse("ftp://example.com").unwrap();
        assert!(validate_url(&url).is_err());
    }

    #[test]
    fn test_validate_url_blocks_localhost() {
        let url = reqwest::Url::parse("http://localhost:9375").unwrap();
        assert!(validate_url(&url).is_err());
    }

    #[test]
    fn test_validate_url_allows_public() {
        let url = reqwest::Url::parse("https://example.com/photo.jpg").unwrap();
        assert!(validate_url(&url).is_ok());
    }

    // --- resize_image_if_needed tests ---

    /// Helper: create a solid-color PNG of the given dimensions.
    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 50, 50]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("write test PNG");
        buf.into_inner()
    }

    #[test]
    fn test_resize_skips_when_within_limit() {
        let bytes = make_test_png(800, 600);
        let (out, mime) = resize_image_if_needed(bytes.clone(), "image/png", 1280);
        assert_eq!(mime, "image/png", "no resize → original mime preserved");
        assert_eq!(out, bytes, "no resize → bytes unchanged");
    }

    #[test]
    fn test_resize_downscales_large_image() {
        let bytes = make_test_png(3000, 2000);
        let orig_len = bytes.len();
        let (out, mime) = resize_image_if_needed(bytes, "image/png", 1280);
        assert_eq!(mime, "image/jpeg", "resized → re-encoded as JPEG");
        let img = image::load_from_memory(&out).expect("decoded resized output");
        assert!(
            img.width() <= 1280 && img.height() <= 1280,
            "dims within 1280 box"
        );
        assert!(out.len() < orig_len, "resized output smaller than original");
    }

    #[test]
    fn test_resize_disabled_when_max_dim_zero() {
        let bytes = make_test_png(3000, 2000);
        let (out, mime) = resize_image_if_needed(bytes.clone(), "image/png", 0);
        assert_eq!(out, bytes, "max_dim=0 → no resize");
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn test_resize_fail_open_on_undecodable() {
        // Garbage bytes that aren't a valid image → fail-open, return original
        let bytes = b"not-an-image-at-all!!".to_vec();
        let (out, mime) = resize_image_if_needed(bytes.clone(), "image/jpeg", 1280);
        assert_eq!(out, bytes, "decode failure → original bytes returned");
        assert_eq!(mime, "image/jpeg");
    }

    // --- VlmHealthTable: candidate failure cool-down ---

    #[test]
    fn health_404_blocks_and_clear_releases() {
        let mut h = super::VlmHealthTable::default();
        assert!(!h.is_blocked("ollama_x"));
        h.record_failure(
            "ollama_x",
            "API error 404: {\"error\":\"model 'qwen3.6' not found\"}",
        );
        assert!(h.is_blocked("ollama_x"), "404 model-not-found must block");
        h.clear("ollama_x");
        assert!(!h.is_blocked("ollama_x"), "clear releases the block");
    }

    #[test]
    fn health_quota_is_hard_failure() {
        let mut h = super::VlmHealthTable::default();
        // Real-world OpenAI/DashScope quota-exhausted payload. Must block.
        h.record_failure(
            "openai_x",
            "API error 429: insufficient_quota / FreeTierOnly free quota exhausted",
        );
        assert!(h.is_blocked("openai_x"));
    }

    #[test]
    fn health_rate_limit_blocks_within_window() {
        let mut h = super::VlmHealthTable::default();
        h.record_failure("cloud_x", "API error 429: rate limit exceeded");
        assert!(h.is_blocked("cloud_x"));
    }

    #[test]
    fn health_unknown_error_not_penalized() {
        let mut h = super::VlmHealthTable::default();
        // A one-off parse / serialization error must NOT block the backend.
        h.record_failure("cloud_x", "request body serialization failed");
        assert!(!h.is_blocked("cloud_x"));
    }

    #[test]
    fn health_cant_see_reply_blocks() {
        let mut h = super::VlmHealthTable::default();
        h.record_cant_see("ollama_tiny");
        assert!(h.is_blocked("ollama_tiny"));
    }

    #[test]
    fn health_clear_after_success_releases_all_kinds() {
        let mut h = super::VlmHealthTable::default();
        h.record_failure("a", "404 not found");
        h.record_cant_see("a");
        h.record_failure("a", "429 rate limit");
        assert!(h.is_blocked("a"));
        h.clear("a");
        assert!(!h.is_blocked("a"));
    }
}
