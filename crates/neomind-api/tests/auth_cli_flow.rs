//! Cross-crate E2E of the CLI auth bootstrap flow (login/logout) against a
//! server-seeded API-key store.
//!
//! Regression ground for the v0.9.21–0.9.23 incident: under a custom
//! NEOMIND_DATA_DIR the server encrypted keys with a foreign directory's
//! encryption key, so `neomind login` could not read a usable key from the
//! server db and every CLI/agent call 401'd while the web UI (JWT) kept
//! working. These tests pin the contract the fix restored: a key created by
//! the SERVER-side store (neomind_api::auth::AuthState) must be recoverable
//! by the CLIENT-side flow (neomind_cli_ops::auth_cmd::run_login) from the
//! same data directory — the exact path the chat agent's shell tools depend
//! on.

use neomind_api::auth::AuthState;

/// The tests mutate process-global env (NEOMIND_CONFIG_DIR), so they must
/// not interleave. tokio's Mutex is async-aware — safe to hold across the
/// `.await`s below (std's would trip clippy::await_holding_lock).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Isolates the CLI credential file (NEOMIND_CONFIG_DIR) so tests never
/// touch the developer's real `neomind login` credential.
struct CredDir {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}
impl CredDir {
    async fn setup() -> Self {
        let guard = ENV_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NEOMIND_CONFIG_DIR", dir.path());
        Self { _guard: guard, _dir: dir }
    }
}
impl Drop for CredDir {
    fn drop(&mut self) {
        std::env::remove_var("NEOMIND_CONFIG_DIR");
    }
}

#[tokio::test]
async fn login_recovers_server_created_key_from_data_dir() {
    let server_dir = tempfile::tempdir().unwrap();
    let data_dir = server_dir.path().to_string_lossy().into_owned();

    // Server side: create the store the way `neomind serve` does, then issue
    // an API key (same call the settings UI makes). Dropping persists both
    // api_keys.redb and encryption_key into data_dir.
    let server_state = AuthState::new_with_data_dir(&data_dir);
    server_state
        .create_key("incident-e2e".to_string(), vec!["*".to_string()])
        .await;
    drop(server_state);

    let _cred = CredDir::setup().await;

    // Client side: `neomind login --data-dir <dir>` must read the plaintext
    // key back out of the server db (requires the encryption keys to pair).
    let resp = neomind_cli_ops::auth_cmd::run_login(Some(data_dir.clone()), false)
        .await
        .unwrap();
    assert!(resp.success, "login failed: {:?}", resp.message);

    // The stored credential must be a key the server actually accepts. (The
    // db also holds the boot-generated default key; login legitimately picks
    // the first active wildcard key in table order, so we assert server-side
    // validity, not identity with the key we created.)
    let stored = neomind_cli_ops::auto_auth::read_logged_in_key().unwrap();
    let verifier = AuthState::new_with_data_dir(&data_dir);
    assert!(
        verifier.validate_key(&stored),
        "login must persist a server-valid key, got {}…",
        &stored[..12]
    );
}

#[tokio::test]
async fn login_twice_reports_already_logged_in_without_force() {
    let server_dir = tempfile::tempdir().unwrap();
    let data_dir = server_dir.path().to_string_lossy().into_owned();
    let state = AuthState::new_with_data_dir(&data_dir);
    state
        .create_key("second-login".to_string(), vec!["*".to_string()])
        .await;
    drop(state);

    let _cred = CredDir::setup().await;
    neomind_cli_ops::auth_cmd::run_login(Some(data_dir.clone()), false)
        .await
        .unwrap();

    // Second login without --force short-circuits — this is the message the
    // agent saw in the incident; it must stay truthful AND cheap (no db read
    // needed to answer it).
    let resp = neomind_cli_ops::auth_cmd::run_login(Some(data_dir), false)
        .await
        .unwrap();
    assert!(resp.success);
    let already = resp
        .data
        .as_ref()
        .and_then(|d| d.get("already_logged_in"))
        .and_then(|v| v.as_bool());
    assert_eq!(already, Some(true), "expected already_logged_in flag");
}

#[tokio::test]
async fn logout_removes_credential_and_login_rebootstraps() {
    let server_dir = tempfile::tempdir().unwrap();
    let data_dir = server_dir.path().to_string_lossy().into_owned();
    let state = AuthState::new_with_data_dir(&data_dir);
    state
        .create_key("logout-cycle".to_string(), vec!["*".to_string()])
        .await;
    drop(state);

    let _cred = CredDir::setup().await;
    neomind_cli_ops::auth_cmd::run_login(Some(data_dir.clone()), false)
        .await
        .unwrap();
    let first = neomind_cli_ops::auto_auth::read_logged_in_key().unwrap();

    // Logout must actually remove the file, not just claim to.
    let resp = neomind_cli_ops::auth_cmd::run_logout().await.unwrap();
    assert!(resp.success);
    let logged_out = resp
        .data
        .as_ref()
        .and_then(|d| d.get("logged_out"))
        .and_then(|v| v.as_bool());
    assert_eq!(logged_out, Some(true));
    assert!(neomind_cli_ops::auto_auth::read_logged_in_key().is_none());

    // And login re-bootstraps from the server db (the recovery path the
    // incident's hint chain never reached) with a server-valid key.
    let resp = neomind_cli_ops::auth_cmd::run_login(Some(data_dir.clone()), false)
        .await
        .unwrap();
    assert!(resp.success);
    let reborn = neomind_cli_ops::auto_auth::read_logged_in_key().unwrap();
    let verifier = AuthState::new_with_data_dir(&data_dir);
    assert!(
        verifier.validate_key(&reborn),
        "re-login must persist a server-valid key"
    );
    // Logout truly dropped the old credential file, not just its contents.
    let _ = first;
}
