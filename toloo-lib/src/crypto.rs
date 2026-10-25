use anyhow::{anyhow, Context, Result};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

type HmacSha256 = Hmac<Sha256>;

/// Create HMAC-SHA256 key from bytes using KeyInit trait.
fn hmac_sha256_key(key: &[u8]) -> HmacSha256 {
    <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC key")
}

const PRIVATE_INFO: &[u8] = b"toloo-private-v0.2";

/// Generate an Ed25519 keypair (public key, seed) from system randomness.
pub fn ed25519_generate() -> (Vec<u8>, Vec<u8>) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("system RNG failure");
    let signing = SigningKey::from_bytes(&seed);
    (signing.verifying_key().to_bytes().to_vec(), seed.to_vec())
}

/// Derive Ed25519 public key from a 32-byte seed.
pub fn ed25519_pub_from_seed(seed: &[u8]) -> Result<Vec<u8>> {
    let seed_bytes = exact_bytes(seed, "Ed25519 seed")?;
    let signing = SigningKey::from_bytes(&seed_bytes);
    Ok(signing.verifying_key().to_bytes().to_vec())
}

/// Sign a message with a 32-byte Ed25519 seed.
pub fn ed25519_sign(message: &[u8], seed: &[u8]) -> Result<Vec<u8>> {
    let seed_bytes = exact_bytes(seed, "Ed25519 seed")?;
    let signing = SigningKey::from_bytes(&seed_bytes);
    let signature: Signature = signing.sign(message);
    Ok(signature.to_bytes().to_vec())
}

/// Verify Ed25519 signature.
pub fn ed25519_verify(message: &[u8], sig: &[u8], pub_key: &[u8]) -> Result<()> {
    let sig_bytes = exact_bytes(sig, "Ed25519 signature")?;
    let pub_bytes = exact_bytes(pub_key, "Ed25519 public key")?;
    let verifying =
        VerifyingKey::from_bytes(&pub_bytes).context("invalid Ed25519 public key bytes")?;
    let signature = Signature::from_bytes(&sig_bytes);

    // Verification in dalek uses constant-time internals for signature checks.
    verifying
        .verify(message, &signature)
        .context("Ed25519 signature verification failed")
}

/// Generate an X25519 keypair (public key, private key) from system randomness.
pub fn x25519_generate() -> (Vec<u8>, Vec<u8>) {
    let mut priv_bytes = [0u8; 32];
    getrandom::fill(&mut priv_bytes).expect("system RNG failure");
    let secret = StaticSecret::from(priv_bytes);
    (PublicKey::from(&secret).as_bytes().to_vec(), priv_bytes.to_vec())
}

/// Derive an X25519 public key from a 32-byte private key.
pub fn x25519_pub_from_seed(priv_key: &[u8; 32]) -> [u8; 32] {
    *PublicKey::from(&StaticSecret::from(*priv_key)).as_bytes()
}

/// Compute X25519 shared secret.
pub fn x25519_shared_secret(my_priv: &[u8], their_pub: &[u8]) -> Result<Vec<u8>> {
    let my_priv_bytes = exact_bytes(my_priv, "X25519 private key")?;
    let their_pub_bytes = exact_bytes(their_pub, "X25519 public key")?;
    let my_secret = StaticSecret::from(my_priv_bytes);
    let their_public = PublicKey::from(their_pub_bytes);
    let shared = my_secret.diffie_hellman(&their_public);
    Ok(shared.as_bytes().to_vec())
}

/// SHA-256 digest.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Generic HKDF-SHA256 (RFC 5869).
pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    if length == 0 {
        return Vec::new();
    }

    // HKDF-Extract: PRK = HMAC-SHA256(salt, IKM)
    let mut mac = hmac_sha256_key(salt);
    mac.update(ikm);
    let prk = mac.finalize().into_bytes();

    // HKDF-Expand: OKM = T(1) | T(2) | ... where T(i) = HMAC-SHA256(PRK, T(i-1) | info | i)
    let mut okm = Vec::with_capacity(length);
    let mut t = Vec::new();
    let mut counter = 1u8;

    while okm.len() < length {
        let mut mac = hmac_sha256_key(&prk);
        mac.update(&t);
        mac.update(info);
        mac.update(&[counter]);
        t = mac.finalize().into_bytes().to_vec();
        okm.extend_from_slice(&t);
        counter += 1;
    }

    okm.truncate(length);
    okm
}

/// Derive private message content key + nonce.
pub fn derive_private_message_key(shared: &[u8], salt: &[u8]) -> ([u8; 32], [u8; 12]) {
    let okm = hkdf_sha256(shared, salt, PRIVATE_INFO, 44);
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 12];
    key.copy_from_slice(&okm[0..32]);
    nonce.copy_from_slice(&okm[32..44]);
    (key, nonce)
}

/// ChaCha20-Poly1305 encryption.
pub fn chacha20_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .encrypt(Nonce::from_slice(nonce), plaintext)
        .expect("encryption failed for input length")
}

/// ChaCha20-Poly1305 decryption.
pub fn chacha20_decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("ChaCha20-Poly1305 authentication failed"))
}

fn exact_bytes<const N: usize>(input: &[u8], name: &str) -> Result<[u8; N]> {
    input
        .try_into()
        .map_err(|_| anyhow!("{name} must be {N} bytes"))
}

#[cfg(test)]
mod tests {
    use crate::hex::encode as hex_encode;

    use super::{
        chacha20_decrypt, chacha20_encrypt, derive_private_message_key, ed25519_generate,
        ed25519_pub_from_seed, ed25519_sign, ed25519_verify, sha256, x25519_generate,
        x25519_shared_secret,
    };

    #[test]
    fn ed25519_sign_and_verify_roundtrip() {
        let (pub_key, seed) = ed25519_generate();
        let msg = b"toloo-signature-surface";
        let sig = ed25519_sign(msg, &seed).expect("sign should succeed");
        ed25519_verify(msg, &sig, &pub_key).expect("verify should succeed");
        assert!(ed25519_verify(b"tampered", &sig, &pub_key).is_err());
    }

    #[test]
    fn ed25519_pub_derivation_matches_generated_pub() {
        let (pub_key, seed) = ed25519_generate();
        let derived = ed25519_pub_from_seed(&seed).expect("pub derivation should succeed");
        assert_eq!(derived, pub_key);
    }

    #[test]
    fn x25519_shared_secret_matches_both_directions() {
        let (a_pub, a_priv) = x25519_generate();
        let (b_pub, b_priv) = x25519_generate();
        let ab = x25519_shared_secret(&a_priv, &b_pub).expect("shared secret should succeed");
        let ba = x25519_shared_secret(&b_priv, &a_pub).expect("shared secret should succeed");
        assert_eq!(ab, ba);
        assert_eq!(ab.len(), 32);
    }

    #[test]
    fn sha256_matches_known_vector() {
        let digest = sha256(b"abc");
        assert_eq!(
            hex_encode(digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn private_message_derivation_has_expected_sizes() {
        let shared = [1u8; 32];
        let salt = [2u8; 32];
        let (key, nonce) = derive_private_message_key(&shared, &salt);
        assert_eq!(key.len(), 32);
        assert_eq!(nonce.len(), 12);
    }

    #[test]
    fn chacha20_encrypt_decrypt_roundtrip_and_detects_tamper() {
        let key = [9u8; 32];
        let nonce = [3u8; 12];
        let plaintext = b"toloo-private-payload";
        let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);
        assert_eq!(ciphertext.len(), plaintext.len() + 16);

        let decrypted = chacha20_decrypt(&key, &nonce, &ciphertext).expect("decrypt should pass");
        assert_eq!(decrypted, plaintext);

        let mut tampered = ciphertext.clone();
        tampered[0] ^= 0x01;
        assert!(chacha20_decrypt(&key, &nonce, &tampered).is_err());
    }
}
