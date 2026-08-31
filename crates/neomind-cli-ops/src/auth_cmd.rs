//! `neomind login` / `logout` / `whoami` command implementations.
//!
//! Provides a `gh auth login`-style credential bootstrap so that the CLI works
//! from any CWD after a single `neomind login`. Credentials are stored in the
//! platform config dir (`dirs::config_dir()/neomind/api_key`) with 0600 perms.

use crate::auto_auth;
use crate::types::CliResponse;
use anyhow::Result;

/// Mask an API key for display: `nmk_a1b2...x9y8`.
fn mask_key(key: &str) -> String {
    if key.len() <= 12 {
        return format!("{}...", &key[..key.len().min(8)]);
    }
    format!("{}...{}", &key[..8], &key[key.len() - 4..])
}

/// Resolve the server data directory for login.
///
/// Priority:
/// 1. Explicit `--data-dir` override
/// 2. `NEOMIND_DATA_DIR` env
/// 3. `./data` (if it exists — backward compat with the main scenario)
/// 4. Platform default `dirs::data_local_dir()/neomind` (if it exists)
/// 5. Error
pub fn resolve_login_data_dir(explicit: Option<String>) -> Result<String> {
    if let Some(d) = explicit {
        return Ok(d);
    }
    if let Ok(dir) = std::env::var("NEOMIND_DATA_DIR") {
        if !dir.is_empty() {
            return Ok(dir);
        }
    }
    if neomind_core::paths::store_path("api_keys.redb")
        .as_path()
        .exists()
    {
        return Ok("data".to_string());
    }
    if let Some(local) = dirs::data_local_dir() {
        let candidate = local.join("neomind");
        if candidate.join("api_keys.redb").exists() {
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    anyhow::bail!(
        "No NeoMind server data directory found. \
         Start the server first with: neomind serve\n\
         Or specify the data directory with --data-dir"
    )
}

/// `neomind login` — read a key from the server's auth DB and persist it to
/// the CLI config dir so subsequent commands work from any CWD.
pub async fn run_login(data_dir: Option<String>, force: bool) -> Result<CliResponse> {
    let resolved_dir = resolve_login_data_dir(data_dir)?;

    let db_path = format!("{}/api_keys.redb", resolved_dir);
    if !std::path::Path::new(&db_path).exists() {
        return Ok(CliResponse::error_with_suggestion(
            "Server has not been initialized yet",
            "NOT_INITIALIZED",
            "Start the server first with: neomind serve",
        ));
    }

    // Check existing credential (unless --force)
    if !force && auto_auth::read_logged_in_key().is_some() {
        return Ok(CliResponse::success(
            serde_json::json!({"already_logged_in": true}),
            "Already logged in. Use --force to refresh the credential.".to_string(),
        ));
    }

    // Read the plaintext key from the server's auth DB
    let key = match auto_auth::read_default_api_key_from(&resolved_dir) {
        Some(k) => k,
        None => {
            return Ok(CliResponse::error_with_suggestion(
                "Could not read an API key from the server database. \
                 This usually means the encryption key file is missing or \
                 the database is empty.",
                "KEY_NOT_FOUND",
                format!(
                    "Ensure --data-dir points to the correct server data directory (resolved: {}). \
                     If the issue persists, restart the server with: neomind serve",
                    resolved_dir
                ),
            ));
        }
    };

    // Persist
    auto_auth::write_credential(&key)?;

    let masked = mask_key(&key);
    let cred_path = auto_auth::cli_credential_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown)".to_string());

    Ok(CliResponse::success(
        serde_json::json!({
            "key_preview": masked,
            "credential_path": cred_path,
        }),
        format!(
            "Login successful. API key: {} (saved to {})",
            masked, cred_path
        ),
    ))
}

/// `neomind logout` — remove the CLI credential file.
pub async fn run_logout() -> Result<CliResponse> {
    let existed = auto_auth::cli_credential_path()
        .map(|p| p.exists())
        .unwrap_or(false);

    auto_auth::remove_credential()?;

    if existed {
        Ok(CliResponse::success(
            serde_json::json!({"logged_out": true}),
            "Logged out. Credential file removed.".to_string(),
        ))
    } else {
        Ok(CliResponse::success(
            serde_json::json!({"logged_out": false}),
            "Not logged in (no credential file found).".to_string(),
        ))
    }
}

/// `neomind whoami` — validate the current key against the server.
pub async fn run_whoami() -> Result<CliResponse> {
    // Determine current key (same resolution as ApiClient)
    let key = std::env::var("NEOMIND_API_KEY")
        .ok()
        .or_else(auto_auth::read_logged_in_key)
        .or_else(|| auto_auth::read_default_api_key_from(&auto_auth::resolve_data_dir()));

    let key = match key {
        Some(k) => k,
        None => {
            return Ok(CliResponse::error_with_suggestion(
                "No API key found. Log in first with: neomind login",
                "NO_KEY",
                "Run: neomind login",
            ));
        }
    };

    let masked = mask_key(&key);

    // Validate against the server via a lightweight authed endpoint
    let client = crate::ApiClient::new();
    let base_url = client.base_url().to_string();

    match client.get("/settings/timezone").await {
        Ok(_) => Ok(CliResponse::success(
            serde_json::json!({
                "key_preview": masked,
                "server": base_url,
                "source": key_source(),
            }),
            format!("Authenticated as {} @ {}", masked, base_url),
        )),
        Err(e) => {
            let msg = format!("{}", e);
            if msg.contains("401") {
                Ok(CliResponse::error_with_suggestion(
                    format!("API key {} is invalid or revoked", masked),
                    "INVALID_KEY",
                    "Refresh with: neomind login --force",
                ))
            } else {
                Ok(CliResponse::error_with_suggestion(
                    format!("Cannot reach server at {}: {}", base_url, msg),
                    "SERVER_UNREACHABLE",
                    "Ensure the server is running: neomind serve",
                ))
            }
        }
    }
}

/// Human-readable description of where the current key came from.
fn key_source() -> &'static str {
    if std::env::var("NEOMIND_API_KEY").is_ok() {
        "env (NEOMIND_API_KEY)"
    } else if auto_auth::read_logged_in_key().is_some() {
        "credential file (neomind login)"
    } else {
        "redb auto-auth"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key_long() {
        let key = "nmk_abcdef1234567890";
        let masked = mask_key(key);
        assert!(masked.starts_with("nmk_abc"));
        assert!(masked.ends_with("7890"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn test_mask_key_short() {
        let key = "nmk_short";
        let masked = mask_key(key);
        assert!(masked.contains("..."));
    }

    #[test]
    fn test_resolve_login_data_dir_explicit() {
        let dir = resolve_login_data_dir(Some("/tmp/explicit-test".to_string())).unwrap();
        assert_eq!(dir, "/tmp/explicit-test");
    }

    /// Serializes tests that touch the process-global NEOMIND_DATA_DIR env:
    /// the env and not-found tests set/remove the same variable, and running
    /// them concurrently lets one delete what the other just set (a flake
    /// that surfaced under the new full-test CI gate).
    static DATA_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_resolve_login_data_dir_env() {
        let _lock = DATA_DIR_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("NEOMIND_DATA_DIR", "/tmp/env-data-dir-test");
        // Explicit takes priority
        let dir = resolve_login_data_dir(Some("/tmp/explicit".to_string())).unwrap();
        assert_eq!(dir, "/tmp/explicit");
        // Without explicit, env is used
        let dir = resolve_login_data_dir(None).unwrap();
        assert_eq!(dir, "/tmp/env-data-dir-test");
        std::env::remove_var("NEOMIND_DATA_DIR");
    }

    #[test]
    fn test_resolve_login_data_dir_not_found() {
        let _lock = DATA_DIR_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Clear all sources so resolution fails.
        std::env::remove_var("NEOMIND_DATA_DIR");
        // This test may pass or fail depending on whether ./data or platform
        // default exists. In CI it likely errors. We only assert that it
        // doesn't panic.
        let _ = resolve_login_data_dir(None);
    }
}
