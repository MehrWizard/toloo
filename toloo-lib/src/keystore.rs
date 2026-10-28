//! Encrypted-at-rest key storage (§D.15.2).
//!
//! Uses Argon2id to derive an encryption key from a passphrase,
//! then wraps private keys with ChaCha20-Poly1305.
//!
//! This module requires the `argon2` feature.

use anyhow::{anyhow, bail, Context, Result};

use crate::base64url;
use crate::crypto::{chacha20_decrypt, chacha20_encrypt};

#[cfg(feature = "argon2")]
use argon2::{Algorithm, Argon2, Params, Version};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// Argon2id parameter presets for different device classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Argon2Preset {
    /// Standard security (19 MiB memory, 2 iterations, 1 lane).
    /// Recommended for desktop/server with >= 1GB RAM.
    Standard,
    /// Low memory (8 MiB memory, 3 iterations, 1 lane).
    /// Suitable for embedded devices with 128-256 MB RAM.
    LowMemory,
    /// Minimal memory (4 MiB memory, 4 iterations, 1 lane).
    /// For severely constrained devices (OpenWrt-class, 64 MB RAM).
    Minimal,
}

impl Argon2Preset {
    /// Returns (memory_kib, iterations, parallelism) for this preset.
    pub fn params(self) -> (u32, u32, u32) {
        match self {
            Argon2Preset::Standard => (19_456, 2, 1),
            Argon2Preset::LowMemory => (8_192, 3, 1),
            Argon2Preset::Minimal => (4_096, 4, 1),
        }
    }

    /// Detect preset from environment variable TOLOO_ARGON2_PRESET.
    /// Values: "standard", "low-memory", "minimal" (case-insensitive).
    /// Defaults to Standard.
    pub fn from_env() -> Self {
        std::env::var("TOLOO_ARGON2_PRESET")
            .ok()
            .and_then(|v| match v.to_lowercase().as_str() {
                "low-memory" | "low_memory" | "low" => Some(Argon2Preset::LowMemory),
                "minimal" | "min" => Some(Argon2Preset::Minimal),
                _ => Some(Argon2Preset::Standard),
            })
            .unwrap_or(Argon2Preset::Standard)
    }
}

/// An encrypted key blob: salt + nonce + ciphertext, base64url-encoded.
#[derive(Debug, Clone)]
pub struct EncryptedKey {
    pub blob: String,
}

#[cfg(feature = "argon2")]
/// Encrypt a private key (raw bytes) under a passphrase.
/// Uses the default preset from TOLOO_ARGON2_PRESET env var (Standard).
pub fn encrypt_key(private_key: &[u8], passphrase: &str) -> Result<EncryptedKey> {
    encrypt_key_with_preset(private_key, passphrase, Argon2Preset::from_env())
}

#[cfg(not(feature = "argon2"))]
/// Encrypt a private key (stub - requires `argon2` feature).
pub fn encrypt_key(_private_key: &[u8], _passphrase: &str) -> Result<EncryptedKey> {
    anyhow::bail!("encryption requires the 'argon2' feature")
}

#[cfg(feature = "argon2")]
/// Encrypt a private key with an explicit Argon2 preset.
pub fn encrypt_key_with_preset(
    private_key: &[u8],
    passphrase: &str,
    preset: Argon2Preset,
) -> Result<EncryptedKey> {
    if passphrase.is_empty() {
        bail!("passphrase must not be empty");
    }

    let salt = generate_salt();
    let derived = derive_key(passphrase, &salt, preset)?;
    let nonce = generate_nonce();
    let ciphertext = chacha20_encrypt(&derived, &nonce, private_key);

    // blob = salt (16) || nonce (12) || ciphertext
    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);

    Ok(EncryptedKey {
        blob: base64url::encode(&blob),
    })
}

#[cfg(not(feature = "argon2"))]
/// Encrypt a private key with an explicit Argon2 preset (stub).
pub fn encrypt_key_with_preset(
    _private_key: &[u8],
    _passphrase: &str,
    _preset: Argon2Preset,
) -> Result<EncryptedKey> {
    anyhow::bail!("encryption requires the 'argon2' feature")
}

#[cfg(feature = "argon2")]
/// Decrypt a private key from an encrypted blob using a passphrase.
/// Uses the default preset from TOLOO_ARGON2_PRESET env var (Standard).
pub fn decrypt_key(encrypted: &EncryptedKey, passphrase: &str) -> Result<Vec<u8>> {
    decrypt_key_with_preset(encrypted, passphrase, Argon2Preset::from_env())
}

#[cfg(not(feature = "argon2"))]
/// Decrypt a private key from an encrypted blob (stub - requires `argon2` feature).
pub fn decrypt_key(_encrypted: &EncryptedKey, _passphrase: &str) -> Result<Vec<u8>> {
    anyhow::bail!("decryption requires the 'argon2' feature")
}

#[cfg(feature = "argon2")]
/// Decrypt a private key with an explicit Argon2 preset.
pub fn decrypt_key_with_preset(
    encrypted: &EncryptedKey,
    passphrase: &str,
    preset: Argon2Preset,
) -> Result<Vec<u8>> {
    let raw = base64url::decode(&encrypted.blob).context("invalid encrypted key blob encoding")?;

    if raw.len() < SALT_LEN + NONCE_LEN + 16 {
        bail!("encrypted key blob too short");
    }

    let salt = &raw[..SALT_LEN];
    let nonce: [u8; NONCE_LEN] = raw[SALT_LEN..SALT_LEN + NONCE_LEN]
        .try_into()
        .map_err(|_| anyhow!("invalid nonce length"))?;
    let ciphertext = &raw[SALT_LEN + NONCE_LEN..];

    let derived = derive_key(passphrase, salt, preset)?;
    chacha20_decrypt(&derived, &nonce, ciphertext)
        .context("decryption failed — wrong passphrase or corrupted data")
}

#[cfg(not(feature = "argon2"))]
/// Decrypt a private key with an explicit Argon2 preset (stub).
pub fn decrypt_key_with_preset(
    _encrypted: &EncryptedKey,
    _passphrase: &str,
    _preset: Argon2Preset,
) -> Result<Vec<u8>> {
    anyhow::bail!("decryption requires the 'argon2' feature")
}

#[cfg(feature = "argon2")]
/// Derive a 32-byte key from a passphrase and salt using Argon2id with the given preset.
fn derive_key(passphrase: &str, salt: &[u8], preset: Argon2Preset) -> Result<[u8; 32]> {
    let (mem_kib, iters, lanes) = preset.params();
    let params = Params::new(mem_kib, iters, lanes, Some(32))
        .map_err(|e| anyhow!("argon2 params error: {e}"))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut output)
        .map_err(|e| anyhow!("argon2 key derivation failed: {e}"))?;

    Ok(output)
}

fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).expect("system RNG failure");
    salt
}

fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).expect("system RNG failure");
    nonce
}

#[cfg(all(test, feature = "argon2"))]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = b"my_secret_private_key_32_bytes!!";
        let passphrase = "correct horse battery staple";

        let encrypted = encrypt_key(key, passphrase).expect("encrypt should succeed");
        let decrypted = decrypt_key(&encrypted, passphrase).expect("decrypt should succeed");
        assert_eq!(decrypted, key);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let key = b"my_secret_private_key_32_bytes!!";
        let encrypted = encrypt_key(key, "right").expect("encrypt should succeed");
        assert!(decrypt_key(&encrypted, "wrong").is_err());
    }

    #[test]
    fn empty_passphrase_rejected() {
        assert!(encrypt_key(b"key", "").is_err());
    }

    #[test]
    fn different_encryptions_produce_different_blobs() {
        let key = b"same_key_different_salt";
        let enc1 = encrypt_key(key, "pass").unwrap();
        let enc2 = encrypt_key(key, "pass").unwrap();
        assert_ne!(
            enc1.blob, enc2.blob,
            "random salt should produce different blobs"
        );
    }

    #[test]
    fn preset_low_memory_works() {
        let key = b"test_key_32_bytes_long_exactly!!";
        let passphrase = "test passphrase";
        let preset = Argon2Preset::LowMemory;

        let encrypted = encrypt_key_with_preset(key, passphrase, preset).expect("encrypt");
        let decrypted = decrypt_key_with_preset(&encrypted, passphrase, preset).expect("decrypt");
        assert_eq!(decrypted, key);
    }

    #[test]
    fn preset_minimal_works() {
        let key = b"test_key_32_bytes_long_exactly!!";
        let passphrase = "test passphrase";
        let preset = Argon2Preset::Minimal;

        let encrypted = encrypt_key_with_preset(key, passphrase, preset).expect("encrypt");
        let decrypted = decrypt_key_with_preset(&encrypted, passphrase, preset).expect("decrypt");
        assert_eq!(decrypted, key);
    }

    #[test]
    fn preset_mismatch_fails() {
        let key = b"test_key_32_bytes_long_exactly!!";
        let passphrase = "test passphrase";

        let encrypted = encrypt_key_with_preset(key, passphrase, Argon2Preset::Standard).expect("encrypt");
        // Decrypting with a different preset should fail (different derived key)
        assert!(decrypt_key_with_preset(&encrypted, passphrase, Argon2Preset::Minimal).is_err());
    }
}