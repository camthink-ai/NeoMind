//! Remote model catalog — the open-catalog "remote directory" channel.
//!
//! NeoMind fetches `models/catalog.json` from the NeoMind-Runtimes repo so a
//! new model can ship as "edit the JSON, clients pick it up on refresh" —
//! no product release required. Graceful degradation is the contract:
//!
//! - offline / timeout / parse error → `None` → callers fall back to the
//!   compiled-in curated models. A network failure must never degrade the
//!   core.
//! - the fetch is cached in-process with a TTL, so repeated `/models` calls
//!   don't hammer the network.
//!
//! The `CatalogModel` shape IS the unified model description used by the
//! picker / download / spawn — the compiled-in curated defs are converted
//! into it, so remote and builtin models are indistinguishable downstream.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// One model entry in the remote catalog (mirrors `models/catalog.json`).
/// Default for [`CatalogModel::format`] — everything predating the format
/// contract is GGUF.
fn default_catalog_format() -> String {
    "gguf".to_string()
}

/// Shape check for a catalog `format` value: lowercase ascii/digits/hyphen,
/// 2-16 chars. Open-ended on PURPOSE (vendors keep appearing — rkllm today,
/// others later): an UNKNOWN-but-well-formed format must parse fine and be
/// surfaced as "unsupported on this platform" by the eligibility layer, not
/// break catalog loading. Only malformed strings are rejected (entry
/// skipped + warned).
pub fn format_is_wellformed(f: &str) -> bool {
    (2..=16).contains(&f.len())
        && f.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    #[serde(rename = "name")]
    pub display_name: String,
    pub file_name: String,
    pub sha256: String,
    pub quant: String,
    /// Model package format (open string, see [`format_is_wellformed`]).
    /// Absent in older catalogs → "gguf".
    #[serde(default = "default_catalog_format")]
    pub format: String,
    /// SoC/board constraints for hardware-specific formats (e.g.
    /// `["rk3588", "rk3572"]`). `None`/empty = runs anywhere the format is
    /// supported. Parsed + exposed now; host-eligibility filtering arrives
    /// with the first hardware-specific model.
    #[serde(default)]
    pub target_hardware: Option<Vec<String>>,
    /// Minimum llama.cpp runtime tag (e.g. "b11960") — downloads are
    /// refused with guidance while the active runtime is older. `None` = no
    /// floor. Compared with `tag_cmp` (b-prefixed numeric ordering).
    #[serde(default)]
    pub min_runtime: Option<String>,
    pub hf_repo: String,
    pub hf_file: String,
    pub size_bytes: u64,
    pub default_ctx: u32,
    /// Native context ceiling of the model (what the UI should SHOW —
    /// "supports up to X"); `default_ctx` is the conservative run default
    /// (spawn -c), tuned for small-memory edge devices. Absent in older
    /// catalogs → fall back to default_ctx.
    #[serde(default)]
    pub max_ctx: Option<u32>,
    #[serde(default)]
    pub default_thinking: bool,
    #[serde(default)]
    pub min_ram_mb: u32,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub recommended: bool,
    /// Per-model sampling defaults (official where validated; absent = the
    /// platform-wide 0.6/0.85/20 legacy point). Applied at spawn (server
    /// defaults) and written into the backend instance.
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    /// min_p override (llama.cpp default 0.05); absent = server default.
    #[serde(default)]
    pub min_p: Option<f32>,
    /// repeat_penalty override (llama.cpp default 1.0 = off); absent =
    /// server default.
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
    /// Vision projector shipped with the model; absent = text-only (and in
    /// pre-mmproj catalogs — resolve_model inherits from the compiled-in
    /// registry entry of the same id so a stale remote catalog can't
    /// silently disable vision).
    #[serde(default)]
    pub mmproj: Option<neomind_core::builtin_llm::manifest::MmprojSource>,
    /// Instance role: absent = main chat model; `Some("vlm")` = vision
    /// companion (own port, never the active chat backend). Pre-role
    /// catalogs parse as all-main — safe default.
    #[serde(default)]
    pub role: Option<String>,
}

/// Catalog fetch target. The Runtimes repo's `models/catalog.json` on the
/// default branch (bumped catalog_version to invalidate clients).
/// Default catalog source. Override with `NEOMIND_CATALOG_URL` (deployments
/// can point at a mirror; tests can force the offline path).
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Runtimes/main/models/catalog.json";

fn catalog_url() -> String {
    // Settings layer (mirror config) > env > default — see `sources.rs`.
    super::sources::catalog_url()
}
// Short TTL: catalog edits on GitHub should reach users on refresh-ish
// timescales (the "add a model = edit the JSON" flow), while still shielding
// the network from per-request fetches. Failed fetches keep the fallback.
const CATALOG_TTL: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// In-process cache state: `(fetched_at, Option<models>)`. `Some(list)` = last
/// fetch succeeded; `None` = last fetch failed (kept so an offline period
/// doesn't hammer the network on every /models call, and so callers correctly
/// fall back to the compiled-in curated set instead of an empty catalog).
type CatalogCacheState = Option<(Instant, Option<Vec<CatalogModel>>)>;

static CACHE: OnceLock<Arc<RwLock<CatalogCacheState>>> = OnceLock::new();
fn cache() -> &'static Arc<RwLock<CatalogCacheState>> {
    CACHE.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// Last-fetched catalog from the in-process cache (no network). None when
/// nothing has been fetched yet (cold process before the first /models call).
pub fn cached_catalog() -> Option<Vec<CatalogModel>> {
    let guard = cache().read().ok()?;
    guard.as_ref().and_then(|(_, models)| models.clone())
}

/// Fetch the remote catalog, honoring the TTL cache.
///
/// Returns `None` on any failure (network / timeout / parse / non-200) —
/// callers fall back to the compiled-in curated models.
pub async fn fetch_catalog() -> Option<Vec<CatalogModel>> {
    // Fresh enough → serve from cache. A previous FAILURE returns None too
    // (callers keep the builtin fallback), never an empty list.
    {
        let guard = cache().read().ok()?;
        if let Some((at, models)) = guard.as_ref() {
            if at.elapsed() < CATALOG_TTL {
                return models.clone();
            }
        }
    }

    let fetched = fetch_catalog_uncached().await;
    if let Ok(mut guard) = cache().write() {
        *guard = Some((Instant::now(), fetched.clone()));
    }
    fetched
}

async fn fetch_catalog_uncached() -> Option<Vec<CatalogModel>> {
    let client = match reqwest::Client::builder().timeout(FETCH_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "catalog: client build failed");
            return None;
        }
    };
    let resp = match client.get(catalog_url()).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, url = catalog_url(), "catalog: fetch failed");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "catalog: HTTP error");
        return None;
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "catalog: read body failed");
            return None;
        }
    };
    let parsed: CatalogRoot = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "catalog: parse failed");
            return None;
        }
    };
    // Shape-check `format` per entry: an unknown-but-wellformed vendor
    // format (rkllm, a future dxnn, …) MUST survive parsing — it becomes an
    // "unsupported on this platform" entry later. Only malformed strings
    // (spaces, uppercase, …) are dropped, and they drop alone.
    let models: Vec<CatalogModel> = parsed
        .models
        .into_iter()
        .filter(|m| {
            let ok = format_is_wellformed(&m.format);
            if !ok {
                tracing::warn!(id = %m.id, format = %m.format,
                    "catalog: entry has a malformed format string — skipping");
            }
            ok
        })
        .collect();
    Some(models)
}

#[derive(Deserialize)]
struct CatalogRoot {
    models: Vec<CatalogModel>,
}

/// Look up a model by id in the catalog cache (or hardcoded set via the
/// caller's conversion — this only consults the remote cache).
pub fn catalog_model(id: &str) -> Option<CatalogModel> {
    let guard = cache().read().ok()?;
    let models = guard.as_ref()?.1.as_ref()?;
    models.iter().find(|m| m.id == id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mmproj extension must parse from real catalog JSON (the Runtimes
    /// file ships this shape since catalog_version 11) — and pre-mmproj
    /// entries must keep parsing (serde default, no deny_unknown_fields).
    /// Locks the JSON key names against the `MmprojSource` field names: a
    /// rename on either side silently turns every vision entry into
    /// text-only.
    #[test]
    fn catalog_json_with_and_without_mmproj_parses() {
        let raw = r#"{"catalog_version": 11, "schema": "neomind-model-catalog/v1", "models": [
            {"id":"gemma4-e2b","name":"Gemma4-E2B","file_name":"m.gguf","sha256":"aa",
             "quant":"q4_0","hf_repo":"r","hf_file":"m.gguf","size_bytes":1,
             "default_ctx":8192,
             "mmproj":{"file_name":"mm.gguf","sha256":"bb","hf_repo":"r",
                        "hf_file":"mm.gguf","size_bytes":2}},
            {"id":"old","name":"Old","file_name":"o.gguf","sha256":"cc",
             "quant":"q4_0","hf_repo":"r","hf_file":"o.gguf","size_bytes":1,
             "default_ctx":8192},
            {"id":"minicpm-v-4.7-1b","name":"MiniCPM-V-4.7-1B","file_name":"v.gguf","sha256":"dd",
             "quant":"q4_k_m","hf_repo":"r","hf_file":"v.gguf","size_bytes":3,
             "default_ctx":32768,"role":"vlm"}
        ]}"#;
        let parsed: CatalogRoot = serde_json::from_str(raw).expect("catalog parses");
        assert_eq!(parsed.models.len(), 3);
        let mm = parsed.models[0].mmproj.as_ref().expect("mmproj parsed");
        assert_eq!(mm.file_name, "mm.gguf");
        assert_eq!(mm.sha256, "bb");
        assert_eq!(mm.hf_repo, "r");
        assert_eq!(mm.hf_file, "mm.gguf");
        assert_eq!(mm.size_bytes, 2);
        assert!(
            parsed.models[1].mmproj.is_none(),
            "entry without mmproj must parse to None"
        );
        assert_eq!(parsed.models[1].role, None, "main model has no role");
        assert_eq!(
            parsed.models[2].role.as_deref(),
            Some("vlm"),
            "companion role must parse"
        );
        // Entries without a format field default to gguf.
        assert_eq!(parsed.models[1].format, "gguf");
    }

    #[test]
    fn format_shape_rules() {
        for good in ["gguf", "rkllm", "rkllm2", "dxnn", "a-b9"] {
            assert!(format_is_wellformed(good), "{good}");
        }
        for bad in [
            "",
            "g",
            "GGUF",
            "rk llm",
            "rk_llm",
            "rkllm/",
            "waytoolongformatstring",
        ] {
            assert!(!format_is_wellformed(bad), "{bad}");
        }
    }

    #[test]
    fn unknown_vendor_formats_parse_and_malformed_entries_drop_alone() {
        let raw = r#"{"models":[
            {"id":"rk","name":"RK","file_name":"llm.rkllm","sha256":"aa",
             "quant":"w4a16","format":"rkllm","target_hardware":["rk3588","rk3572"],
             "hf_repo":"r","hf_file":"llm.rkllm","size_bytes":1,"default_ctx":8192},
            {"id":"dx","name":"DX","file_name":"llm.dxnn","sha256":"bb",
             "quant":"w4","format":"dxnn",
             "hf_repo":"r","hf_file":"llm.dxnn","size_bytes":1,"default_ctx":8192},
            {"id":"bad","name":"Bad","file_name":"x","sha256":"cc",
             "quant":"q4_0","format":"Not A Format",
             "hf_repo":"r","hf_file":"x","size_bytes":1,"default_ctx":8192},
            {"id":"gg","name":"GG","file_name":"g.gguf","sha256":"dd",
             "quant":"q4_0","hf_repo":"r","hf_file":"g.gguf","size_bytes":1,
             "default_ctx":8192}
        ]}"#;
        let parsed: CatalogRoot = serde_json::from_str(raw).expect("parses");
        let kept: Vec<&CatalogModel> = parsed
            .models
            .iter()
            .filter(|m| format_is_wellformed(&m.format))
            .collect();
        // The future vendor formats parse — forward compatibility is the
        // whole point of the open string.
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].format, "rkllm");
        assert_eq!(
            kept[0].target_hardware.as_deref(),
            Some(["rk3588".to_string(), "rk3572".to_string()].as_slice())
        );
        assert_eq!(kept[1].format, "dxnn");
        // Untouched gguf entries keep working regardless.
        assert_eq!(kept[2].format, "gguf");
    }
}
