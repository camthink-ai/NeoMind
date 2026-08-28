//! At-rest sealing for secret fields (LLM provider API keys).
//!
//! Secrets are sealed with the shared `CryptoService` (same
//! `data/encryption_key` the platform uses for its own API keys) and tagged
//! with a versioned marker, so rows written before encryption existed still
//! load as plaintext and get re-sealed on their next save.

use std::path::Path;

use neomind_core::crypto::CryptoService;

/// Marker prefix identifying a sealed value. Bump the version on any format
/// change so old rows fail closed (marker mismatch → undecryptable, never
/// misread as plaintext).
const SEALED_PREFIX: &str = "enc1:";

/// Build the shared cipher for secrets stored in a database at `db_path`.
///
/// The key file lives next to the database (e.g. `data/encryption_key` for
/// `data/settings.redb`), matching what the api crate's auth store uses —
/// one key per data dir, no cross-instance sharing. `NEOMIND_ENCRYPTION_KEY`
/// still wins when set.
///
/// Paths without a real parent directory (`":memory:"`, bare filenames —
/// test-only usage) get an ephemeral in-process key instead: persisting one
/// into the process CWD would leak a key file into whatever directory the
/// tests happen to run from.
pub(crate) fn crypto_for_db(db_path: &Path) -> CryptoService {
    let dir = db_path
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty() && *s != ".");
    match dir {
        Some(dir) => CryptoService::from_env_or_generate_with_data_dir(dir),
        None => CryptoService::generate_random(),
    }
}

/// Seal a secret for at-rest storage. `None` and empty values pass through
/// unchanged, as do values that are already sealed.
pub(crate) fn seal(crypto: &CryptoService, secret: &Option<String>) -> Option<String> {
    let plain = secret.as_ref()?;
    if plain.is_empty() || plain.starts_with(SEALED_PREFIX) {
        return secret.clone();
    }
    match crypto.encrypt_str(plain) {
        Ok(enc) => Some(format!("{SEALED_PREFIX}{enc}")),
        Err(e) => {
            // AES-GCM with a valid key essentially never fails; if it somehow
            // does, keep the platform usable (store plaintext) but scream.
            tracing::error!(
                category = "storage",
                error = %e,
                "Failed to seal secret for at-rest storage — storing plaintext"
            );
            secret.clone()
        }
    }
}

/// Unseal a secret loaded from storage. Values without the marker are
/// legacy plaintext and pass through unchanged.
pub(crate) fn unseal(crypto: &CryptoService, stored: Option<String>) -> Option<String> {
    let value = stored?;
    if let Some(enc) = value.strip_prefix(SEALED_PREFIX) {
        match crypto.decrypt_str(enc) {
            Ok(plain) => Some(plain),
            Err(e) => {
                tracing::error!(
                    category = "storage",
                    error = %e,
                    "Stored secret could not be unsealed (encryption key changed?) — treating as unset"
                );
                None
            }
        }
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crypto() -> CryptoService {
        CryptoService::new(b"storage_secret_test_master_key_32_bytes!!!").unwrap()
    }

    #[test]
    fn seal_unseal_roundtrip() {
        let c = crypto();
        let sealed = seal(&c, &Some("sk-test-1234567890".to_string())).unwrap();
        assert!(sealed.starts_with(SEALED_PREFIX));
        assert!(!sealed.contains("sk-test"));
        assert_eq!(
            unseal(&c, Some(sealed)),
            Some("sk-test-1234567890".to_string())
        );
    }

    #[test]
    fn none_and_empty_pass_through() {
        let c = crypto();
        assert_eq!(seal(&c, &None), None);
        assert_eq!(unseal(&c, None), None);
        assert_eq!(
            seal(&c, &Some(String::new())),
            Some(String::new()) // empty stays empty, no marker added
        );
    }

    #[test]
    fn legacy_plaintext_passes_through() {
        let c = crypto();
        // Pre-encryption rows have no marker — must load unchanged.
        assert_eq!(
            unseal(&c, Some("legacy-plaintext-key".to_string())),
            Some("legacy-plaintext-key".to_string())
        );
    }

    #[test]
    fn sealed_value_is_idempotent() {
        let c = crypto();
        let sealed = seal(&c, &Some("secret".to_string())).unwrap();
        // Re-sealing an already-sealed value must not double-encrypt.
        assert_eq!(seal(&c, &Some(sealed.clone())), Some(sealed));
    }

    #[test]
    fn wrong_key_unseal_fails_closed() {
        let c1 = crypto();
        let c2 = CryptoService::new(b"another_master_key_of_32_bytes_for_sure").unwrap();
        let sealed = seal(&c1, &Some("secret".to_string())).unwrap();
        // Wrong key → None (treated as unset), never the marker string.
        assert_eq!(unseal(&c2, Some(sealed)), None);
    }
}
