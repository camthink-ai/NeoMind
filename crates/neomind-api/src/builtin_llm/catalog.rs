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
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    #[serde(rename = "name")]
    pub display_name: String,
    pub file_name: String,
    pub sha256: String,
    pub quant: String,
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
}

/// Catalog fetch target. The Runtimes repo's `models/catalog.json` on the
/// default branch (bumped catalog_version to invalidate clients).
/// Default catalog source. Override with `NEOMIND_CATALOG_URL` (deployments
/// can point at a mirror; tests can force the offline path).
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Runtimes/main/models/catalog.json";

fn catalog_url() -> String {
    std::env::var("NEOMIND_CATALOG_URL").unwrap_or_else(|_| CATALOG_URL.to_string())
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
    Some(parsed.models)
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
