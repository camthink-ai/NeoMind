//! Cryptographic utilities for API key encryption.
//!
//! This module provides AES-256-GCM encryption for sensitive data like API keys.
//! The encryption key is derived from a master secret using PBKDF2.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use sha2::Sha256;
use std::env;
use tracing::{info, warn};

// Import Engine trait for base64 operations
use base64::Engine;

const ENCRYPTION_KEY_ENV: &str = "NEOMIND_ENCRYPTION_KEY";
const DEFAULT_ITERATIONS: u32 = 100_000;

/// Error type for cryptographic operations.
#[derive(Debug, Clone, PartialEq)]
pub enum CryptoError {
    KeyTooShort,
    EncryptionFailed,
    DecryptionFailed,
    InvalidKeyFormat,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::KeyTooShort => write!(f, "Encryption key is too short (min 32 bytes)"),
            CryptoError::EncryptionFailed => write!(f, "Failed to encrypt data"),
            CryptoError::DecryptionFailed => write!(f, "Failed to decrypt data"),
            CryptoError::InvalidKeyFormat => write!(f, "Invalid key format"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Cryptographic service for encrypting and decrypting sensitive data.
#[derive(Clone)]
pub struct CryptoService {
    cipher: Aes256Gcm,
}

impl CryptoService {
    /// Create a new CryptoService with a master key.
    ///
    /// The master key should be 32 bytes (256 bits). If a shorter key is provided,
    /// it will be derived using PBKDF2.
    ///
    /// # Arguments
    ///
    /// * `master_key` - The master encryption key (will be derived if < 32 bytes)
    pub fn new(master_key: &[u8]) -> Result<Self, CryptoError> {
        let key = Self::derive_key(master_key);
        let cipher = Aes256Gcm::new(&key.into());
        Ok(Self { cipher })
    }

    /// Create a CryptoService from environment variable or persistent file.
    ///
    /// Priority:
    /// 1. `NEOMIND_ENCRYPTION_KEY` environment variable
    /// 2. `data/encryption_key` file (auto-generated on first run)
    /// 3. Generate random key and persist to file
    pub fn from_env_or_generate() -> Self {
        Self::from_env_or_generate_with_data_dir("data")
    }

    /// Same as [`from_env_or_generate`](Self::from_env_or_generate) but with a
    /// custom data directory for the encryption key file.
    ///
    /// This fixes a path-pairing bug: when `AuthState::new_with_data_dir` opens
    /// the redb from a custom path, the encryption key MUST also be read from
    /// that same directory — otherwise decryption fails.
    pub fn from_env_or_generate_with_data_dir(data_dir: &str) -> Self {
        let key_file = format!("{}/encryption_key", data_dir);

        // 1. Try environment variable first
        if let Ok(key_str) = env::var(ENCRYPTION_KEY_ENV) {
            let key = key_str.as_bytes();
            return Self::new(key).unwrap_or_else(|_| {
                warn!(
                    category = "crypto",
                    "Invalid encryption key in environment, falling back to file"
                );
                Self::from_file_or_generate(&key_file)
            });
        }

        // 2. Try persistent key file, or generate and save
        Self::from_file_or_generate(&key_file)
    }

    /// Load encryption key from file, or generate and persist a new one.
    fn from_file_or_generate(key_file: &str) -> Self {
        // Try to load from file
        if let Ok(key_hex) = std::fs::read_to_string(key_file) {
            let key_hex = key_hex.trim();
            if let Ok(key_bytes) = hex::decode(key_hex) {
                if key_bytes.len() >= 32 {
                    info!(
                        category = "crypto",
                        "Loaded encryption key from {}", key_file
                    );
                    return Self::new(&key_bytes).unwrap_or_else(|_| Self::generate_random());
                }
            }
            warn!(
                category = "crypto",
                "Invalid encryption key file, regenerating"
            );
        }

        // Generate a new random 32-byte key and persist it
        let raw_key = Aes256Gcm::generate_key(&mut OsRng);
        let key_hex = hex::encode(raw_key);

        // Ensure data directory exists
        if let Some(parent) = std::path::Path::new(key_file).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Write with 0600 perms: this key encrypts every API key in
        // api_keys.redb (and auto_auth decrypts the default admin key with
        // it), so a world-readable file lets any local user recover the
        // wildcard API key. std::fs::write creates files 0644 — create the
        // file explicitly with restrictive mode, then write through it.
        // Best-effort chmod afterwards covers pre-existing files too.
        #[cfg(unix)]
        let write_result: std::io::Result<()> = {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(key_file)
                .and_then(|mut f| f.write_all(key_hex.as_bytes()))
                .and_then(|_| {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(key_file, std::fs::Permissions::from_mode(0o600))
                })
        };
        #[cfg(not(unix))]
        let write_result: std::io::Result<()> = std::fs::write(key_file, &key_hex);

        match write_result {
            Ok(()) => {
                info!(
                    category = "crypto",
                    "Generated and persisted encryption key (0600) to {}", key_file
                );
            }
            Err(e) => {
                warn!(
                    category = "crypto",
                    error = %e,
                    "Failed to persist encryption key to file"
                );
            }
        }

        let cipher = Aes256Gcm::new(&raw_key);
        Self { cipher }
    }

    /// Generate a random encryption key.
    pub fn generate_random() -> Self {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        let cipher = Aes256Gcm::new(&key);
        Self { cipher }
    }

    /// Derive a 256-bit key from the input using PBKDF2.
    fn derive_key(input: &[u8]) -> [u8; 32] {
        if input.len() >= 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&input[..32]);
            return key;
        }

        // Use PBKDF2 to derive a key from shorter input
        let salt = b"NeoMind-API-Key-Salt-2024";
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(input, salt, DEFAULT_ITERATIONS, &mut key);
        key
    }

    /// Encrypt data using AES-256-GCM.
    ///
    /// Returns a base64-encoded string containing the nonce and ciphertext.
    ///
    /// # Arguments
    ///
    /// * `plaintext` - The data to encrypt
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, CryptoError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        self.cipher
            .encrypt(&nonce, plaintext)
            .map(|ciphertext| {
                // Combine nonce + ciphertext and encode as base64
                let mut combined = nonce.to_vec();
                combined.extend_from_slice(&ciphertext);
                base64::engine::general_purpose::STANDARD.encode(combined)
            })
            .map_err(|_| CryptoError::EncryptionFailed)
    }

    /// Encrypt a string.
    pub fn encrypt_str(&self, plaintext: &str) -> Result<String, CryptoError> {
        self.encrypt(plaintext.as_bytes())
    }

    /// Decrypt data that was encrypted with `encrypt`.
    ///
    /// # Arguments
    ///
    /// * `encoded` - Base64-encoded string containing nonce + ciphertext
    pub fn decrypt(&self, encoded: &str) -> Result<Vec<u8>, CryptoError> {
        let combined = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| CryptoError::DecryptionFailed)?;

        if combined.len() < 12 {
            return Err(CryptoError::DecryptionFailed);
        }

        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)
    }

    /// Decrypt to a string.
    pub fn decrypt_str(&self, encoded: &str) -> Result<String, CryptoError> {
        String::from_utf8(self.decrypt(encoded)?).map_err(|_| CryptoError::DecryptionFailed)
    }

    /// Hash an API key for validation (one-way, not reversible).
    ///
    /// This is used for storing API key hashes for comparison without storing
    /// the actual key.
    pub fn hash_api_key(&self, api_key: &str) -> String {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(api_key.as_bytes());
        hasher.update(b"NeoMind-API-Key-v1");
        format!("{:x}", hasher.finalize())
    }
}

impl Default for CryptoService {
    fn default() -> Self {
        Self::from_env_or_generate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let crypto =
            CryptoService::new(b"this_is_a_32_byte_master_key_for_testing_purposes").unwrap();
        let plaintext = "Hello, World! This is a secret message.";

        let encrypted = crypto.encrypt_str(plaintext).unwrap();
        let decrypted = crypto.decrypt_str(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
        assert_ne!(plaintext, encrypted);
    }

    #[test]
    fn test_hash_api_key() {
        let crypto =
            CryptoService::new(b"this_is_a_32_byte_master_key_for_testing_purposes").unwrap();
        let key1 = "nmk_1234567890abcdef";
        let key2 = "nmk_1234567890abcdef";
        let key3 = "nmk_different_key";

        assert_eq!(crypto.hash_api_key(key1), crypto.hash_api_key(key2));
        assert_ne!(crypto.hash_api_key(key1), crypto.hash_api_key(key3));
    }

    #[test]
    fn test_short_key_derivation() {
        let crypto = CryptoService::new(b"short").unwrap();
        let plaintext = "Test message";

        let encrypted = crypto.encrypt_str(plaintext).unwrap();
        let decrypted = crypto.decrypt_str(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_invalid_decryption_fails() {
        let crypto =
            CryptoService::new(b"this_is_a_32_byte_master_key_for_testing_purposes").unwrap();
        let invalid = "not_valid_base64!!";

        assert!(crypto.decrypt_str(invalid).is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let crypto1 =
            CryptoService::new(b"this_is_a_32_byte_master_key_for_testing_purposes").unwrap();
        let crypto2 =
            CryptoService::new(b"different_32_byte_master_key_for_testing_purposes!!").unwrap();

        let encrypted = crypto1.encrypt_str("secret").unwrap();
        assert!(crypto2.decrypt_str(&encrypted).is_err());
    }

    #[test]
    fn test_data_dir_path_pairing() {
        // Regression: from_env_or_generate_with_data_dir must place the key file
        // inside the given data_dir, NOT the hardcoded "data/encryption_key".
        let tmp = std::env::temp_dir().join(format!(
            "neomind-crypto-pairing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Ensure the env var doesn't interfere
        std::env::remove_var(ENCRYPTION_KEY_ENV);

        let _crypto = CryptoService::from_env_or_generate_with_data_dir(tmp.to_str().unwrap());

        let key_file = tmp.join("encryption_key");
        assert!(
            key_file.exists(),
            "encryption_key should be inside the custom data_dir"
        );

        // Loading again should reuse the same file (round-trip)
        let crypto2 = CryptoService::from_env_or_generate_with_data_dir(tmp.to_str().unwrap());
        let encrypted = crypto2.encrypt_str("hello").unwrap();
        let decrypted = crypto2.decrypt_str(&encrypted).unwrap();
        assert_eq!(decrypted, "hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
