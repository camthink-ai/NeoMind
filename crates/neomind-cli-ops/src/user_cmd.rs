//! Local-only user account recovery: reset a user's password by rewriting the
//! bcrypt hash in `users.redb` directly (no running server needed).
//!
//! This is the recovery path for a forgotten admin password on an edge box.
//! It requires filesystem access to the server data directory — the same
//! trust boundary as the data itself. It intentionally does NOT verify the
//! old password, and it is NOT exposed over HTTP (an unauthenticated reset
//! endpoint would let anyone with network access take over the box).
//!
//! The [`User`] struct and `users` table below mirror
//! `neomind-api/src/auth_users.rs` (same bincode format, same redb table).
//! bincode has no forward/backward compatibility — if that struct changes,
//! update this copy too, or stored accounts become unreadable.

use std::path::Path;

use redb::{Database, TableDefinition};

use crate::types::CliResponse;

/// Mirrors `USERS_TABLE` in `neomind-api/src/auth_users.rs`.
const USERS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("users");

/// Mirrors `User` in `neomind-api/src/auth_users.rs` — field order and types
/// must match exactly for bincode round-tripping.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct User {
    id: String,
    username: String,
    password_hash: String,
    role: UserRole,
    created_at: i64,
    last_login: Option<i64>,
    active: bool,
}

/// Mirrors `UserRole` in `neomind-api/src/auth_users.rs`. Variant order is
/// significant for bincode (encoded by discriminant index).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum UserRole {
    Admin,
    User,
    Viewer,
}

/// Path to the auth database under a data directory.
fn users_db_path(data_dir: &str) -> String {
    format!("{}/users.redb", data_dir)
}

/// Read one user row from `users.redb`. `Ok(None)` if the row or DB is absent.
fn read_user_from_db(path: &str, username: &str) -> anyhow::Result<Option<User>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let db = Database::open(path)?;
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(USERS_TABLE)?;
    match table.get(username)? {
        Some(bytes) => Ok(Some(bincode::deserialize(bytes.value())?)),
        None => Ok(None),
    }
}

/// Write one user row to `users.redb`, creating the database if absent.
/// Mirrors `save_user_to_db` in `neomind-api/src/auth_users.rs`.
fn write_user_to_db(path: &str, user: &User) -> anyhow::Result<()> {
    let bytes = bincode::serialize(user)?;
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = if Path::new(path).exists() {
        Database::open(path)?
    } else {
        Database::create(path)?
    };
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(USERS_TABLE)?;
        table.insert(user.username.as_str(), bytes.as_slice())?;
    }
    write_txn.commit()?;
    Ok(())
}

/// Reset a user's password by replacing the stored bcrypt hash.
///
/// Does not verify the old password (that is the point). Preserves every other
/// account field. Enforces the same password policy as the setup wizard.
pub fn reset_user_password(
    data_dir: &str,
    username: &str,
    new_password: &str,
) -> anyhow::Result<()> {
    // Same policy as the setup wizard (neomind-api/src/handlers/setup.rs):
    // ≥8 chars, at least one letter and one number. Keeps a reset password from
    // being weaker than one the UI would accept.
    if new_password.len() < 8 {
        anyhow::bail!("Password must be at least 8 characters");
    }
    let has_letter = new_password.chars().any(|c| c.is_alphabetic());
    let has_number = new_password.chars().any(|c| c.is_numeric());
    if !has_letter || !has_number {
        anyhow::bail!("Password must contain at least one letter and one number");
    }

    let path = users_db_path(data_dir);
    let Some(mut user) = read_user_from_db(&path, username)? else {
        anyhow::bail!("User '{}' not found in {}", username, path);
    };

    let new_hash = bcrypt::hash(new_password, bcrypt::DEFAULT_COST)?;
    user.password_hash = new_hash;
    write_user_to_db(&path, &user)?;
    Ok(())
}

/// Read a new password from stdin twice (with confirmation).
///
/// Plain line reads — no hidden echo, to avoid pulling in a terminal-caps
/// dependency. The prompt only runs for someone who already has shell access.
pub fn prompt_new_password(username: &str) -> anyhow::Result<String> {
    use std::io::Write;

    let stdin = std::io::stdin();
    let mut line = String::new();

    let mut read_line = |prompt: &str| -> anyhow::Result<String> {
        print!("{}: ", prompt);
        std::io::stdout().flush()?;
        line.clear();
        stdin.read_line(&mut line)?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    };

    let pw1 = read_line(&format!("New password for {}", username))?;
    let pw2 = read_line("Confirm password")?;

    if pw1.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }
    if pw1 != pw2 {
        anyhow::bail!("Passwords do not match");
    }
    Ok(pw1)
}

/// `neomind user reset-password` — resolve the data dir, prompt for a new
/// password, and rewrite the user's bcrypt hash offline.
/// Change a user's role offline (e.g. promote the first admin on a fresh
/// install, or recover one whose account was created through the old
/// always-User self-registration). Requires shell/filesystem access.
pub(crate) fn set_user_role(data_dir: &str, username: &str, role: UserRole) -> anyhow::Result<()> {
    let path = users_db_path(data_dir);
    let mut user = read_user_from_db(&path, username)?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found in {}", username, path))?;
    user.role = role;
    write_user_to_db(&path, &user)
}

/// Parse a role name for the CLI.
pub(crate) fn parse_role(name: &str) -> anyhow::Result<UserRole> {
    match name.to_ascii_lowercase().as_str() {
        "admin" => Ok(UserRole::Admin),
        "user" => Ok(UserRole::User),
        "viewer" => Ok(UserRole::Viewer),
        _ => Err(anyhow::anyhow!(
            "unknown role '{}': expected admin|user|viewer",
            name
        )),
    }
}

pub async fn run_set_role(
    data_dir: Option<String>,
    username: &str,
    role: &str,
) -> anyhow::Result<CliResponse> {
    let resolved_dir = crate::auth_cmd::resolve_login_data_dir(data_dir)?;
    let role = parse_role(role)?;
    set_user_role(&resolved_dir, username, role.clone())?;
    Ok(CliResponse::success(
        serde_json::json!({ "username": username, "role": role }),
        format!(
            "Role of '{}' set to {:?}. Restart the server if it is running.",
            username, role
        ),
    ))
}

pub async fn run_reset_password(
    data_dir: Option<String>,
    username: &str,
) -> anyhow::Result<CliResponse> {
    let resolved_dir = crate::auth_cmd::resolve_login_data_dir(data_dir)?;
    let new_password = prompt_new_password(username)?;
    reset_user_password(&resolved_dir, username, &new_password)?;
    Ok(CliResponse::success(
        serde_json::json!({ "username": username }),
        format!("Password for '{}' has been reset.", username),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "neomind_user_reset_{}_{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed_user(data_dir: &str, username: &str, password: &str) {
        let user = User {
            id: "u-test-1".to_string(),
            username: username.to_string(),
            password_hash: bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap(),
            role: UserRole::Admin,
            created_at: 1_700_000_000,
            last_login: Some(1_700_000_100),
            active: true,
        };
        write_user_to_db(&users_db_path(data_dir), &user).unwrap();
    }

    #[test]
    fn reset_password_replaces_old_hash() {
        let dir = test_data_dir("replaces");
        let dir_str = dir.to_str().unwrap();
        seed_user(dir_str, "admin", "oldpass123");

        reset_user_password(dir_str, "admin", "newpass456").unwrap();

        let user = read_user_from_db(&users_db_path(dir_str), "admin")
            .unwrap()
            .expect("user still present");
        assert!(
            bcrypt::verify("newpass456", &user.password_hash).unwrap(),
            "new password must verify"
        );
        assert!(
            !bcrypt::verify("oldpass123", &user.password_hash).unwrap(),
            "old password must no longer verify"
        );
    }

    #[test]
    fn reset_password_preserves_account_fields() {
        let dir = test_data_dir("preserves");
        let dir_str = dir.to_str().unwrap();
        seed_user(dir_str, "admin", "oldpass123");

        reset_user_password(dir_str, "admin", "newpass456").unwrap();

        let user = read_user_from_db(&users_db_path(dir_str), "admin")
            .unwrap()
            .expect("user still present");
        assert_eq!(user.id, "u-test-1");
        assert_eq!(user.username, "admin");
        assert!(matches!(user.role, UserRole::Admin));
        assert_eq!(user.created_at, 1_700_000_000);
        assert_eq!(user.last_login, Some(1_700_000_100));
        assert!(user.active);
    }

    #[test]
    fn reset_password_unknown_user_is_error() {
        let dir = test_data_dir("unknown");
        let dir_str = dir.to_str().unwrap();
        seed_user(dir_str, "admin", "oldpass123");

        let err = reset_user_password(dir_str, "ghost", "newpass456").unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn reset_password_enforces_setup_policy() {
        let dir = test_data_dir("policy");
        let dir_str = dir.to_str().unwrap();
        seed_user(dir_str, "admin", "oldpass123");

        // Too short
        let err = reset_user_password(dir_str, "admin", "short").unwrap_err();
        assert!(err.to_string().contains("8 characters"));

        // No letter
        let err = reset_user_password(dir_str, "admin", "12345678").unwrap_err();
        assert!(err.to_string().contains("letter"));

        // No number
        let err = reset_user_password(dir_str, "admin", "abcdefgh").unwrap_err();
        assert!(err.to_string().contains("number"));
    }
}
