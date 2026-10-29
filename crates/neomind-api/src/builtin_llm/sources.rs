//! Download-source resolution for the builtin LLM stack — the mirror layer.
//!
//! Three remote sources, each overridable for restricted networks (CN
//! deployments where huggingface.co / github / raw.githubusercontent are
//! commonly unreachable):
//!
//! | Source | Default | Setting (UI) | Env |
//! |---|---|---|---|
//! | HuggingFace model downloads | `https://huggingface.co` | `llm_hf_base_url` | `NEOMIND_HF_BASE_URL` |
//! | Model catalog JSON | NeoMind-Runtimes raw URL | `llm_catalog_url` | `NEOMIND_CATALOG_URL` |
//! | Runtime update channel | NeoMind-Runtimes `runtimes.json` | `llm_runtime_channel_url` | `NEOMIND_RUNTIME_CHANNEL_URL` |
//!
//! Priority: setting > env > default (the `extension_market_url` precedent).
//! HF mirrors keep the path shape (`{base}/{repo}/resolve/main/{file}`), so
//! `https://hf-mirror.com` is a drop-in. Note the trust boundary, same as
//! the marketplace mirrors: a mirror serves the MIRROR's bytes; GGUF
//! downloads still SHA-verify against the catalog (tamper-evident), while
//! the catalog/channel themselves are trusted-by-URL like the default.

use std::sync::Arc;
use std::time::{Duration, Instant};

use neomind_storage::SettingsStore;

pub const HF_BASE_DEFAULT: &str = "https://huggingface.co";
const HF_SETTING: &str = "llm_hf_base_url";
const HF_ENV: &str = "NEOMIND_HF_BASE_URL";

pub const CATALOG_SETTING: &str = "llm_catalog_url";
pub const CATALOG_ENV: &str = "NEOMIND_CATALOG_URL";

pub const RUNTIME_CHANNEL_SETTING: &str = "llm_runtime_channel_url";
pub const RUNTIME_CHANNEL_ENV: &str = "NEOMIND_RUNTIME_CHANNEL_URL";

/// Resolve one source: setting > env > default. Values are trimmed and
/// trailing slashes normalized away.
fn resolve(setting_key: &str, env_key: &str, default: &str) -> (String, bool) {
    let from_store = SettingsStore::open_default()
        .ok()
        .and_then(|s| s.load(setting_key).ok().flatten())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty());
    if let Some(v) = from_store {
        return (v, true);
    }
    let from_env = std::env::var(env_key)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty());
    if let Some(v) = from_env {
        return (v, true);
    }
    (default.to_string(), false)
}

/// Effective HuggingFace base URL for model GGUF / mmproj downloads.
pub fn hf_base_url() -> String {
    resolve(HF_SETTING, HF_ENV, HF_BASE_DEFAULT).0
}

/// `https://huggingface.co/{repo}/resolve/main/{file}` at the effective base.
pub fn hf_file_url(repo: &str, file: &str) -> String {
    format!("{}/{repo}/resolve/main/{file}", hf_base_url())
}

/// Effective model-catalog JSON URL.
pub fn catalog_url() -> String {
    resolve(CATALOG_SETTING, CATALOG_ENV, super::catalog::CATALOG_URL).0
}

/// Effective runtime update-channel URL.
pub fn runtime_channel_url() -> String {
    resolve(
        RUNTIME_CHANNEL_SETTING,
        RUNTIME_CHANNEL_ENV,
        super::runtime_manager::RUNTIME_CHANNEL_URL,
    )
    .0
}

/// Admin-facing snapshot (GET/PUT /api/settings/llm-sources + the runtime
/// status block). Cached briefly — the runtime status is polled while the
/// upgrade dialog is open and redb reads aren't free.
pub struct LlmSourcesSnapshot {
    pub hf_base_url: String,
    pub hf_custom: bool,
    pub catalog_url: String,
    pub catalog_custom: bool,
    pub runtime_channel_url: String,
    pub runtime_channel_custom: bool,
}

const SNAPSHOT_TTL: Duration = Duration::from_secs(10);
type SnapshotCacheState = Option<(Instant, Arc<LlmSourcesSnapshot>)>;
static SNAPSHOT_CACHE: std::sync::OnceLock<std::sync::Mutex<SnapshotCacheState>> =
    std::sync::OnceLock::new();

pub fn snapshot() -> Arc<LlmSourcesSnapshot> {
    let mut slot = SNAPSHOT_CACHE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((at, snap)) = slot.as_ref() {
        if at.elapsed() < SNAPSHOT_TTL {
            return snap.clone();
        }
    }
    let (hf, hf_custom) = resolve(HF_SETTING, HF_ENV, HF_BASE_DEFAULT);
    let (catalog, catalog_custom) =
        resolve(CATALOG_SETTING, CATALOG_ENV, super::catalog::CATALOG_URL);
    let (channel, channel_custom) = resolve(
        RUNTIME_CHANNEL_SETTING,
        RUNTIME_CHANNEL_ENV,
        super::runtime_manager::RUNTIME_CHANNEL_URL,
    );
    let snap = Arc::new(LlmSourcesSnapshot {
        hf_base_url: hf,
        hf_custom,
        catalog_url: catalog,
        catalog_custom,
        runtime_channel_url: channel,
        runtime_channel_custom: channel_custom,
    });
    *slot = Some((Instant::now(), snap.clone()));
    snap
}

/// Validate + persist (or reset, when empty) one of the three overrides.
/// Returns the effective value after saving.
pub fn set_override(kind: &str, value: &str) -> Result<String, String> {
    let (setting_key, default): (&str, &str) = match kind {
        "hf_base_url" => (HF_SETTING, HF_BASE_DEFAULT),
        "catalog_url" => (CATALOG_SETTING, super::catalog::CATALOG_URL),
        "runtime_channel_url" => (
            RUNTIME_CHANNEL_SETTING,
            super::runtime_manager::RUNTIME_CHANNEL_URL,
        ),
        _ => return Err(format!("unknown source kind '{kind}'")),
    };
    let trimmed = value.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        let store =
            SettingsStore::open_default().map_err(|e| format!("open settings store: {e}"))?;
        store
            .save(setting_key, "")
            .map_err(|e| format!("reset setting: {e}"))?;
        // Bypass the snapshot cache so the change is visible immediately.
        if let Some(m) = SNAPSHOT_CACHE.get() {
            *m.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        return Ok(default.to_string());
    }
    let looks_http = trimmed.starts_with("https://") || trimmed.starts_with("http://");
    if !looks_http {
        return Err(format!("'{kind}' must be an http(s) URL: {trimmed}"));
    }
    let store = SettingsStore::open_default().map_err(|e| format!("open settings store: {e}"))?;
    store
        .save(setting_key, &trimmed)
        .map_err(|e| format!("save setting: {e}"))?;
    if let Some(m) = SNAPSHOT_CACHE.get() {
        *m.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hf_urls_use_the_effective_base() {
        // Default shape (settings store in tests resolves to the default
        // chain — the redb store at the test data dir is empty).
        let url = hf_file_url("google/gemma-4", "gemma.gguf");
        assert!(url.contains("/google/gemma-4/resolve/main/gemma.gguf"));
        let base = hf_base_url();
        assert!(url.starts_with(&base), "{url}");
    }
}
