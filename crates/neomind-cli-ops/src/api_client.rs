use anyhow::Result;
use reqwest::Client;
use std::sync::RwLock;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://localhost:9375/api";
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_RETRIES: usize = 1;

pub struct ApiClient {
    base_url: String,
    client: Client,
    api_key: RwLock<Option<String>>,
    explicit_api_key: bool,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiClient {
    pub fn new() -> Self {
        let base_url =
            std::env::var("NEOMIND_API_BASE").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(&base_url)
    }

    pub fn with_base_url(base_url: &str) -> Self {
        Self::with_base_url_and_key(base_url, None)
    }

    /// Construct a client with an explicit base URL and optional explicit API key.
    ///
    /// When `api_key` is `Some`, it is used directly without reading or mutating
    /// `NEOMIND_API_KEY`. When `None`, falls back to `NEOMIND_API_KEY` or the
    /// default auto-auth key.
    pub fn with_base_url_and_key(base_url: &str, api_key: Option<String>) -> Self {
        // The /api-suffix convention has bitten three independent callers
        // (tauri frontend — CLAUDE.md gotcha #9; this crate's own auth-key
        // handlers double-prefixed into /api/api/... and 404'd; the eval
        // harness aligns by hand). Normalize HERE, but DEDUPLICATE ONLY:
        // an /api-suffixed base collapses to exactly one clean "/api"
        // (…/api/, …//api → …/api), while a non-/api base (e.g. an OpenAI-
        // compatible "/v1" gateway) passes through untouched — appending
        // would break that caller (pinned by test_api_client_base_url_
        // formatting). Path-side duplicates are stripped at join time
        // (url_for), so neither side can produce /api/api/... alone.
        let mut b = base_url.trim().trim_end_matches('/').to_string();
        if b.ends_with("/api") {
            let stem = b[..b.len() - 4].trim_end_matches('/');
            b = format!("{stem}/api");
        }
        let base_url = b;
        let explicit_api_key = api_key.is_some();
        let resolved_key = api_key.or_else(|| {
            std::env::var("NEOMIND_API_KEY")
                .ok()
                .or_else(crate::auto_auth::read_default_api_key)
        });
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self {
            base_url,
            client,
            api_key: RwLock::new(resolved_key),
            explicit_api_key,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn unauthorized_hint_for_client(&self) -> &'static str {
        if self.explicit_api_key {
            "\nHint: the configured API key was rejected by the server — verify or refresh your credentials"
        } else {
            unauthorized_hint()
        }
    }

    fn add_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let key = self.api_key.read().unwrap().clone();
        if let Some(key) = key {
            req.header("Authorization", format!("Bearer {}", key))
        } else {
            req
        }
    }

    /// Tag reqwest failures with a stable kind prefix so callers can tell
    /// connect-refuse (not submitted) from timeout (result unknown).
    fn map_reqwest_error(err: reqwest::Error) -> anyhow::Error {
        if err.is_timeout() {
            anyhow::anyhow!("http_timeout: {err}")
        } else if err.is_connect() {
            anyhow::anyhow!("http_connect: {err}")
        } else {
            anyhow::anyhow!("http_transport: {err}")
        }
    }

    /// Refresh the API key on 401 retry.
    ///
    /// Bypasses env var and credential file — those sources were already used
    /// in the initial load and just failed. Go straight to redb for a fresh
    /// key. This prevents stale-key lockout when the credential file key has
    /// been revoked or the server was re-initialized.
    fn refresh_api_key(&self) {
        let new_key =
            crate::auto_auth::read_default_api_key_from(&crate::auto_auth::resolve_data_dir());
        if new_key.is_some() {
            tracing::debug!(
                category = "api_client",
                "Refreshed API key from redb after 401 (bypassed credential file)"
            );
        }
        *self.api_key.write().unwrap() = new_key;
    }

    /// Join base + path with duplicate-/api stripping: a path that still
    /// carries the "/api" prefix against an "/api"-suffixed base collapses
    /// to one prefix (the double-prefix 404 class can no longer happen even
    /// if a handler regresses to "/api/..." paths).
    fn url_for(&self, path: &str) -> String {
        if self.base_url.ends_with("/api") {
            if let Some(stripped) = path.strip_prefix("/api") {
                if stripped.is_empty() || stripped.starts_with('/') {
                    return format!("{}{}", self.base_url, stripped);
                }
            }
        }
        format!("{}{}", self.base_url, path)
    }

    pub async fn get(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.get(&url))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.post(&url).json(body))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    /// POST with a per-request timeout that overrides the client default.
    ///
    /// For endpoints whose server side deliberately waits longer than the
    /// 30s client default (e.g. `agent invoke`'s 60s patience window): the
    /// caller must receive the endpoint's structured "still executing"
    /// response, not a client-side timeout that reads as business failure.
    pub async fn post_timeout(
        &self,
        path: &str,
        body: &serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(
                    self.client
                        .post(&url)
                        .timeout(Duration::from_secs(timeout_secs))
                        .json(body),
                )
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn post_raw(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.post(&url))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn put(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.put(&url).json(body))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn patch(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.patch(&url).json(body))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn delete(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.delete(&url))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn delete_with_body(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = self.url_for(path);
            let resp = self
                .add_auth(self.client.delete(&url).json(body))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    /// Upload a single file as multipart with the specified field name.
    pub async fn post_file_named(
        &self,
        path: &str,
        file_path: &str,
        field_name: &str,
    ) -> Result<serde_json::Value> {
        use reqwest::multipart;
        use std::fs::File;
        use std::io::Read;

        let url = self.url_for(path);
        let mut file = File::open(file_path)?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let mut file_content = Vec::new();
        file.read_to_end(&mut file_content)?;

        for attempt in 0..=MAX_RETRIES {
            let form = {
                let part =
                    multipart::Part::bytes(file_content.clone()).file_name(file_name.to_string());
                multipart::Form::new().part(field_name.to_string(), part)
            };
            let resp = self
                .add_auth(self.client.post(&url).multipart(form))
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    /// Upload multiple named parts as multipart/form-data.
    /// Each tuple is (field_name, bytes, filename).
    pub async fn post_multipart(
        &self,
        path: &str,
        parts: Vec<(&str, Vec<u8>, String)>,
    ) -> Result<serde_json::Value> {
        use reqwest::multipart;

        let url = self.url_for(path);

        // Clone parts data for retry rebuilds
        let parts_clone: Vec<(String, Vec<u8>, String)> = parts
            .into_iter()
            .map(|(name, bytes, filename)| (name.to_string(), bytes, filename))
            .collect();

        for attempt in 0..=MAX_RETRIES {
            let mut form = multipart::Form::new();
            for (field_name, bytes, filename) in &parts_clone {
                let part = multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                form = form.part(field_name.clone(), part);
            }
            let req = self.client.post(&url).multipart(form);
            let resp = self
                .add_auth(req)
                .send()
                .await
                .map_err(Self::map_reqwest_error)?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt < MAX_RETRIES {
                self.refresh_api_key();
                continue;
            }
            let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "API error ({}): {}{}",
                    status,
                    extract_error_message(&resp_body),
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        self.unauthorized_hint_for_client()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        // The server ANSWERED — a 404 is not a connectivity
                        // problem. The old "is the server running?" hint here
                        // sent users and LLM agents on wasted health checks.
                        "\nHint: not found — check the exact ID using this resource's list command"
                    } else {
                        ""
                    }
                );
            }
            return unwrap_api_envelope(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }
}

/// True when a JSON object matches the standard `ApiResponse<T>` envelope
/// (`{"success": true, "data": <payload>, "error"?: ..., "meta"?: ...}`).
///
/// Rejects domain objects that merely happen to carry a `data` key (such as
/// telemetry `{"device_id": ..., "data": {...}}` or data-source listings
/// `{"data": [...], "total": N}`) so unwrapping never drops sibling fields.
fn is_api_response_envelope(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    obj.get("success").and_then(|v| v.as_bool()) == Some(true)
        && obj.contains_key("data")
        && obj
            .keys()
            .all(|k| matches!(k.as_str(), "success" | "data" | "error" | "meta"))
}

/// Unwrap a standard NeoMind `ApiResponse<T>` envelope at the `ApiClient`
/// boundary and turn HTTP 200 business failures (`"success": false`) into `Err`.
///
/// - Peels outer `ApiResponse` wrappers (including accidental double wraps)
///   so `cli-ops` wraps payloads in a single `CliResponse` layer instead of
///   producing `data.data` nesting.
/// - Intercepts `"success": false` on either the outer envelope or the
///   unwrapped top-level payload (covering HTTP 200 fake-success endpoints in
///   `extensions/marketplace.rs`, `mqtt/{brokers,subscriptions}.rs`,
///   `automations.rs`, `instances.rs`, etc.).
/// - Uses an `"API business error: ..."` prefix so downstream pre-submission
///   classifiers (`outcome_is_pre_submission_rejection`) do not misclassify
///   server-side business failures as client-side `Rejected` JSON errors.
pub fn unwrap_api_envelope(body: serde_json::Value) -> Result<serde_json::Value> {
    let mut current = body;
    for _ in 0..4 {
        if current.get("success").and_then(|v| v.as_bool()) == Some(false) {
            anyhow::bail!("API business error: {}", extract_error_message(&current));
        }
        let is_envelope = current.as_object().is_some_and(is_api_response_envelope);
        if !is_envelope {
            break;
        }
        current = current
            .as_object_mut()
            .and_then(|obj| obj.remove("data"))
            .unwrap_or(serde_json::Value::Null);
    }
    if current.get("success").and_then(|v| v.as_bool()) == Some(false) {
        anyhow::bail!("API business error: {}", extract_error_message(&current));
    }
    Ok(current)
}

/// Extract the inner `data` payload from a standard `ApiResponse` envelope.
///
/// `ApiClient` methods already unwrap `ApiResponse` via [`unwrap_api_envelope`].
/// This helper remains safe if called on an already-unwrapped payload: it only
/// peels objects that actually match the `ApiResponse` envelope shape, so
/// domain objects with their own `data` field are left untouched.
pub fn extract_inner_data(resp: serde_json::Value) -> serde_json::Value {
    if resp.as_object().is_some_and(is_api_response_envelope) {
        resp.get("data").cloned().unwrap_or(resp)
    } else {
        resp
    }
}

/// Extract error message from API response body — message PLUS the API's
/// `hint` when present.
///
/// The hint field carries schema cheat-sheets and recovery guidance written
/// for agents (e.g. add-components' "position needs {x,y,w,h}" example).
/// Dropping it left agents with bare serde errors and burned rounds guessing
/// required fields (live: four attempts at the component schema, each one
/// field further). Every CLI-surfaced error now keeps the hint.
fn extract_error_message(body: &serde_json::Value) -> String {
    let non_empty = |s: &str| !s.trim().is_empty();
    let message = body
        .get("error")
        .and_then(|e| {
            e.get("message")
                .and_then(|v| v.as_str())
                .filter(|s| non_empty(s))
        })
        .or_else(|| {
            body.get("message")
                .and_then(|v| v.as_str())
                .filter(|s| non_empty(s))
        })
        .or_else(|| {
            body.get("error")
                .and_then(|v| v.as_str())
                .filter(|s| non_empty(s))
        })
        .unwrap_or("Unknown error")
        .to_string();
    let hint = body
        .get("error")
        .and_then(|e| e.get("hint").and_then(|v| v.as_str()))
        .or_else(|| body.get("hint").and_then(|v| v.as_str()));
    match hint {
        Some(h) if !h.trim().is_empty() => format!("{message}\nHint: {h}"),
        _ => message,
    }
}

/// Context-aware next-command hint for 401 responses.
///
/// The bare "Run: neomind login" advice dead-ends when a credential already
/// exists — `neomind login` then short-circuits with "already logged in"
/// (it checks file existence, not validity), so an agent or user following
/// the hint loops forever. Route each starting state to a command that can
/// actually make progress:
/// - credential file present but rejected → diagnose with `whoami`,
///   refresh with `login --force`
/// - NEOMIND_API_KEY env set but rejected → the env var shadows every other
///   source; unset it or fix its value
/// - nothing stored → bootstrap with `login`
fn unauthorized_hint() -> &'static str {
    if std::env::var_os("NEOMIND_API_KEY").is_some() {
        "\nHint: NEOMIND_API_KEY was rejected by the server — unset the env var or correct its value"
    } else if crate::auto_auth::read_logged_in_key().is_some() {
        "\nHint: stored credential was rejected. Diagnose with: neomind whoami — refresh with: neomind login --force"
    } else {
        "\nHint: not logged in? Run: neomind login"
    }
}

// ——— base normalization: the /api-suffix single source of truth ———
// Three callers independently got this wrong before normalization existed
// (tauri frontend, this crate's auth-key handlers, eval harness); these
// tests pin the contract so a regression is a red test, not a 404.
#[test]
fn base_normalization_accepts_every_caller_convention() {
    for (input, expected) in [
        // dedup: exactly one clean /api when the caller already prefixed it
        ("http://localhost:9375/api", "http://localhost:9375/api"),
        ("http://localhost:9375/api/", "http://localhost:9375/api"),
        ("http://host:8080//api//", "http://host:8080/api"),
        // pass-through: non-/api bases (v1 gateways, bare origins) untouched
        ("http://localhost:9000/v1", "http://localhost:9000/v1"),
        ("http://localhost:9375", "http://localhost:9375"),
    ] {
        let c = ApiClient::with_base_url(input);
        assert_eq!(c.base_url, expected, "input {input:?}");
    }
}

#[test]
fn join_strips_duplicate_api_prefix() {
    // The original bug: base ".../api" + path "/api/auth/keys" → 404.
    // url_for collapses it; suffix-free paths are unchanged; a /v1 gateway
    // base keeps /api/... paths intact (they are real routes there).
    let c = ApiClient::with_base_url("http://localhost:9375/api");
    assert_eq!(
        c.url_for("/api/auth/keys"),
        "http://localhost:9375/api/auth/keys"
    );
    assert_eq!(
        c.url_for("/auth/keys"),
        "http://localhost:9375/api/auth/keys"
    );
    assert_eq!(c.url_for("/api"), "http://localhost:9375/api");
    let gw = ApiClient::with_base_url("http://localhost:9000/v1");
    assert_eq!(
        gw.url_for("/api/models"),
        "http://localhost:9000/v1/api/models"
    );
}

#[cfg(test)]
mod tests {
    /// The API's `hint` (agent-facing cheat-sheets / recovery guidance) must
    /// survive the CLI's error extraction — dropping it cost a live run four
    /// schema-guessing rounds.
    #[test]
    fn extract_error_message_keeps_hint() {
        let body = serde_json::json!({
            "success": false,
            "error": {
                "code": "BAD_REQUEST",
                "message": "Invalid component data: component[0]: missing field `position`",
                "hint": "Each component needs: id, type, title, position {x, y, w, h}."
            }
        });
        let out = extract_error_message(&body);
        assert!(out.contains("missing field"), "{out}");
        assert!(out.contains("Hint:"), "hint must be appended: {out}");
        assert!(out.contains("position {x, y, w, h}"), "{out}");
        // No hint → message unchanged, no dangling "Hint:".
        let bare = serde_json::json!({"error": {"message": "boom"}});
        assert_eq!(extract_error_message(&bare), "boom");
    }

    use super::*;

    #[test]
    fn test_api_client_new() {
        let client = ApiClient::new();
        assert_eq!(client.base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn test_api_client_with_custom_base_url() {
        let custom_url = "http://example.com:8080/api";
        let client = ApiClient::with_base_url(custom_url);
        assert_eq!(client.base_url(), custom_url);
    }

    #[test]
    fn test_api_client_base_url_formatting() {
        let client = ApiClient::with_base_url("http://localhost:9000/v1");
        assert_eq!(client.base_url(), "http://localhost:9000/v1");
    }

    #[test]
    fn test_api_client_default_url_const() {
        assert_eq!(DEFAULT_BASE_URL, "http://localhost:9375/api");
    }

    #[test]
    fn test_api_client_timeout_const() {
        assert_eq!(DEFAULT_TIMEOUT_SECS, 30);
    }

    #[test]
    fn test_api_key_rwlock_works() {
        let client = ApiClient::with_base_url("http://localhost:9375/api");
        let key = client.api_key.read().unwrap().clone();
        let _ = key; // Key may or may not exist depending on environment
    }

    #[test]
    fn test_refresh_api_key_does_not_panic() {
        // refresh_api_key() bypasses credential file and reads redb directly.
        // It may return None when no redb is available in the test CWD — that's fine,
        // we only verify it doesn't panic and updates the internal state.
        let client = ApiClient::with_base_url("http://localhost:9375/api");
        client.refresh_api_key();
        // Should not panic
    }

    /// Guards the two process-global env vars the hint branches read.
    static HINT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores both env vars on drop, even on panic.
    struct HintEnvGuard {
        api_key_value: Option<std::ffi::OsString>,
        config_dir_value: Option<std::ffi::OsString>,
    }
    impl HintEnvGuard {
        fn take() -> Self {
            Self {
                api_key_value: std::env::var_os("NEOMIND_API_KEY"),
                config_dir_value: std::env::var_os("NEOMIND_CONFIG_DIR"),
            }
        }
    }
    impl Drop for HintEnvGuard {
        fn drop(&mut self) {
            match &self.api_key_value {
                Some(v) => std::env::set_var("NEOMIND_API_KEY", v),
                None => std::env::remove_var("NEOMIND_API_KEY"),
            }
            match &self.config_dir_value {
                Some(v) => std::env::set_var("NEOMIND_CONFIG_DIR", v),
                None => std::env::remove_var("NEOMIND_CONFIG_DIR"),
            }
        }
    }

    /// The 401 hint must route each starting state to a command that makes
    /// progress. Regression for the incident where a stored-but-stale
    /// credential got "Run: neomind login" → "already logged in" → dead end.
    #[serial_test::serial]
    #[test]
    fn test_unauthorized_hint_routes_by_state() {
        let _lock = HINT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = HintEnvGuard::take();

        // 1. Rejected NEOMIND_API_KEY env var — the strongest shadowing source.
        //    Advice must name the env var, not send the user to login (whose
        //    result the env var would override anyway).
        std::env::set_var("NEOMIND_API_KEY", "nmk_rejected");
        std::env::remove_var("NEOMIND_CONFIG_DIR");
        let hint = unauthorized_hint();
        assert!(
            hint.contains("NEOMIND_API_KEY"),
            "env-key 401 must name the env var, got: {hint}"
        );

        // 2. Stored credential file exists (env unset) — must NOT suggest bare
        //    `login` (dead-ends with "already logged in"); point at whoami /
        //    login --force.
        std::env::remove_var("NEOMIND_API_KEY");
        let cfg = tempfile::tempdir().unwrap();
        std::env::set_var("NEOMIND_CONFIG_DIR", cfg.path());
        crate::auto_auth::write_credential("nmk_stale").unwrap();
        let hint = unauthorized_hint();
        assert!(
            hint.contains("whoami") && hint.contains("--force"),
            "stored-credential 401 must route to whoami/login --force, got: {hint}"
        );
        assert!(!hint.contains("Run: neomind login\n"));

        // 3. Nothing stored — bootstrap advice is correct here. Point the
        //    config dir at an EMPTY tempdir rather than unsetting the env:
        //    unsetting would fall through to the real platform config dir,
        //    which may legitimately hold a credential on a dev machine.
        let empty = tempfile::tempdir().unwrap();
        std::env::set_var("NEOMIND_CONFIG_DIR", empty.path());
        let hint = unauthorized_hint();
        assert!(
            hint.contains("Run: neomind login"),
            "bare 401 must suggest login, got: {hint}"
        );
    }

    #[test]
    fn unwrap_api_envelope_strips_single_and_double_wrappers() {
        let single = serde_json::json!({
            "success": true,
            "data": { "id": "dash-1", "name": "Main" },
            "meta": { "timestamp": 1700000000 }
        });
        let unwrapped = unwrap_api_envelope(single).unwrap();
        assert_eq!(unwrapped["id"], "dash-1");
        assert!(unwrapped.get("success").is_none());
        assert!(unwrapped.get("data").is_none());
        assert!(unwrapped.get("meta").is_none());

        let double = serde_json::json!({
            "success": true,
            "data": {
                "success": true,
                "data": { "id": "rule-1" }
            },
            "meta": {}
        });
        let unwrapped = unwrap_api_envelope(double).unwrap();
        assert_eq!(unwrapped["id"], "rule-1");
        assert!(unwrapped.get("success").is_none());
        assert!(unwrapped.get("data").is_none());
    }

    #[test]
    fn unwrap_api_envelope_preserves_domain_payloads_with_data_key() {
        let telemetry = serde_json::json!({
            "success": true,
            "data": {
                "device_id": "dev-1",
                "data": { "temperature": [{ "timestamp": 1000, "value": 24.5 }] },
                "pagination": { "total": 1 }
            },
            "meta": {}
        });
        let unwrapped = unwrap_api_envelope(telemetry).unwrap();
        assert_eq!(unwrapped["device_id"], "dev-1");
        assert!(
            unwrapped["data"]["temperature"].is_array(),
            "telemetry inner `data` map must stay intact: {unwrapped}"
        );
        assert_eq!(unwrapped["pagination"]["total"], 1);

        let sources = serde_json::json!({
            "success": true,
            "data": { "data": [{ "id": "device:d1:temp" }], "total": 1 }
        });
        let unwrapped = unwrap_api_envelope(sources).unwrap();
        assert_eq!(unwrapped["total"], 1);
        assert!(unwrapped["data"].is_array());
    }

    #[test]
    fn unwrap_api_envelope_intercepts_outer_and_embedded_success_false() {
        let cases = [
            // Outer ApiResponse error
            (
                serde_json::json!({
                    "success": false,
                    "error": { "code": "INTERNAL", "message": "storage write failed", "hint": "check disk" }
                }),
                "storage write failed",
            ),
            // extensions/marketplace.rs HTTP 200 fake success
            (
                serde_json::json!({
                    "success": true,
                    "data": {
                        "success": false,
                        "extension_id": "bogus-ext",
                        "error": "Extension not found: 404 Not Found"
                    }
                }),
                "Extension not found: 404 Not Found",
            ),
            // mqtt/brokers.rs HTTP 200 fake success
            (
                serde_json::json!({
                    "success": true,
                    "data": {
                        "success": false,
                        "message": "Invalid broker configuration: empty host"
                    }
                }),
                "Invalid broker configuration: empty host",
            ),
            // mqtt/subscriptions.rs HTTP 200 fake success
            (
                serde_json::json!({
                    "success": true,
                    "data": {
                        "success": false,
                        "message": "No MQTT adapter available"
                    }
                }),
                "No MQTT adapter available",
            ),
            // automations.rs test-code HTTP 200 fake success
            (
                serde_json::json!({
                    "success": true,
                    "data": {
                        "success": false,
                        "error": "ReferenceError: x is not defined"
                    }
                }),
                "ReferenceError: x is not defined",
            ),
            // instances.rs HTTP 200 fake success
            (
                serde_json::json!({
                    "success": true,
                    "data": {
                        "success": false,
                        "status": "offline",
                        "error": "HTTP 502"
                    }
                }),
                "HTTP 502",
            ),
        ];

        for (payload, expected_substr) in cases {
            let err = unwrap_api_envelope(payload)
                .expect_err("embedded or outer success:false must return Err")
                .to_string();
            assert!(
                err.contains(expected_substr),
                "error should carry real business reason `{expected_substr}`, got: {err}"
            );
            let lower = err.to_lowercase();
            assert!(
                !lower.contains("invalid json") && !lower.contains("api error (400"),
                "business error must not be labeled as client JSON/400 error: {err}"
            );
        }
    }

    #[tokio::test]
    async fn api_client_http_200_unwraps_payload_and_rejects_embedded_failure() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let responses = [
                r#"{"success":true,"data":{"id":"dash-1","name":"Ops"},"meta":{"timestamp":1}}"#,
                r#"{"success":true,"data":{"success":false,"error":"Extension not found: 404 Not Found"}}"#,
            ];
            for body in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(resp.as_bytes()).await.unwrap();
            }
        });

        let client = ApiClient::with_base_url(&format!("http://{addr}/api"));

        let ok_val = client.get("/dashboards/dash-1").await.unwrap();
        assert_eq!(ok_val["id"], "dash-1");
        assert!(ok_val.get("success").is_none());
        assert!(ok_val.get("data").is_none());

        let err = client
            .post(
                "/extensions/market/install",
                &serde_json::json!({"id": "bogus"}),
            )
            .await
            .expect_err("HTTP 200 embedded success:false must fail")
            .to_string();
        assert!(err.contains("Extension not found"), "got: {err}");

        server.await.unwrap();
    }
}
