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
        let api_key = std::env::var("NEOMIND_API_KEY")
            .ok()
            .or_else(crate::auto_auth::read_default_api_key);
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.to_string(),
            client,
            api_key: RwLock::new(api_key),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn add_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let key = self.api_key.read().unwrap().clone();
        if let Some(key) = key {
            req.header("Authorization", format!("Bearer {}", key))
        } else {
            req
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

    pub async fn get(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self.add_auth(self.client.get(&url)).send().await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self
                .add_auth(self.client.post(&url).json(body))
                .send()
                .await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn post_raw(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self.add_auth(self.client.post(&url)).send().await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn put(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self
                .add_auth(self.client.put(&url).json(body))
                .send()
                .await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn patch(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self
                .add_auth(self.client.patch(&url).json(body))
                .send()
                .await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn delete(&self, path: &str) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self.add_auth(self.client.delete(&url)).send().await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }

    pub async fn delete_with_body(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        for attempt in 0..=MAX_RETRIES {
            let url = format!("{}{}", self.base_url, path);
            let resp = self
                .add_auth(self.client.delete(&url).json(body))
                .send()
                .await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
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

        let url = format!("{}{}", self.base_url, path);
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
                .await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
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

        let url = format!("{}{}", self.base_url, path);

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
            let resp = self.add_auth(req).send().await?;
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
                        unauthorized_hint()
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        "\nHint: is the server running? Try: neomind health"
                    } else {
                        ""
                    }
                );
            }
            return Ok(resp_body);
        }
        anyhow::bail!("API request failed after retry — is the server running? Try: neomind health")
    }
}

/// Extract the inner `data` payload from a standard API response envelope.
///
/// All NeoMind API endpoints return `{"success": bool, "data": <payload>}`.
/// Callers that need to read fields from the payload (e.g. a newly-created
/// entity's `id`) MUST go through this helper — indexing the envelope
/// directly returns Null, silently masking the real value.
///
/// Falls back to the original value if the envelope shape is unexpected
/// (e.g. legacy endpoints that return the payload without wrapping).
pub fn extract_inner_data(resp: serde_json::Value) -> serde_json::Value {
    resp.get("data").cloned().unwrap_or(resp)
}

/// Extract error message from API response body.
fn extract_error_message(body: &serde_json::Value) -> String {
    body.get("error")
        .and_then(|e| e.get("message").and_then(|v| v.as_str()))
        .or_else(|| body.get("message").and_then(|v| v.as_str()))
        .or_else(|| body.get("error").and_then(|v| v.as_str()))
        .unwrap_or("Unknown error")
        .to_string()
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

#[cfg(test)]
mod tests {
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
}
