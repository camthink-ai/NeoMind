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
    #[serde(default)]
    pub default_thinking: bool,
    #[serde(default)]
    pub min_ram_mb: u32,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub recommended: bool,
}

/// Catalog fetch target. The Runtimes repo's `models/catalog.json` on the
/// default branch (bumped catalog_version to invalidate clients).
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/camthink-ai/NeoMind-Runtimes/main/models/catalog.json";
const CATALOG_TTL: Duration = Duration::from_secs(3600); // 1h
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// In-process cache: (fetched_at, models). `None` = last fetch failed.
static CACHE: OnceLock<Arc<RwLock<Option<(Instant, Vec<CatalogModel>)>>>> = OnceLock::new();
fn cache() -> &'static Arc<RwLock<Option<(Instant, Vec<CatalogModel>)>>> {
    CACHE.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// Fetch the remote catalog, honoring the TTL cache.
///
/// Returns `None` on any failure (network / timeout / parse / non-200) —
/// callers fall back to the compiled-in curated models.
pub async fn fetch_catalog() -> Option<Vec<CatalogModel>> {
    // Fresh enough → serve from cache (even a previous failure counts, so an
    // offline period doesn't hammer the network every /models call).
    {
        let guard = cache().read().ok()?;
        if let Some((at, models)) = guard.as_ref() {
            if at.elapsed() < CATALOG_TTL {
                return Some(models.clone());
            }
        }
    }

    let fetched = fetch_catalog_uncached().await;
    {
        if let Ok(mut guard) = cache().write() {
            *guard = Some((Instant::now(), fetched.clone().unwrap_or_default()));
        }
    }
    fetched
}

async fn fetch_catalog_uncached() -> Option<Vec<CatalogModel>> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .ok()?;
    let resp = client.get(CATALOG_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.text().await.ok()?;
    let parsed: CatalogRoot = serde_json::from_str(&body).ok()?;
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
    let (_, models) = guard.as_ref()?;
    models.iter().find(|m| m.id == id).cloned()
}
