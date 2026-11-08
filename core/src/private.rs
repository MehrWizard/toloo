use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::base64url;
use crate::canonical::canonical_value_bytes;
use crate::crypto::{
    chacha20_decrypt_in_place, chacha20_encrypt_in_place, derive_private_message_key, hkdf_sha256,
    sha256, x25519_generate, x25519_shared_secret,
};

const PAIR_KEY_INFO: &[u8] = b"toloo-private-pair-v2";
const MESSAGE_INFO: &[u8] = b"toloo-private-v2";

/// The encrypted payload stored in `c` of a `private.message` envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateCiphertext {
    /// 43-char base64url ephemeral X25519 public key.
    pub eph: String,
    /// base64url ChaCha20-Poly1305 ciphertext (canonical JSON of content).
    pub encrypted: String,
}

/// Encrypt `content` for `recipient_enc_pub` (spec §3.5.3).
///
/// Returns the `PrivateCiphertext` that goes into `c.eph` / `c.encrypted`.
pub fn encrypt_private(content: &Value, recipient_enc_pub: &str) -> Result<PrivateCiphertext> {
    encrypt_private_with_eph(content, recipient_enc_pub, None)
}

/// Same as `encrypt_private` but allows injecting a fixed ephemeral seed for
/// deterministic test vectors. Pass `None` for random (production use).
pub fn encrypt_private_with_eph(
    content: &Value,
    recipient_enc_pub: &str,
    eph_seed: Option<&[u8; 32]>,
) -> Result<PrivateCiphertext> {
    let rec_enc_bytes =
        base64url::decode(recipient_enc_pub).context("invalid recipient enc pub base64url")?;

    // Step 1: ephemeral X25519 keypair.
    let (eph_pub_bytes, eph_priv_bytes) = match eph_seed {
        Some(seed) => {
            use x25519_dalek::{PublicKey, StaticSecret};
            let secret = StaticSecret::from(*seed);
            let public = PublicKey::from(&secret);
            (public.as_bytes().to_vec(), secret.to_bytes().to_vec())
        }
        None => x25519_generate(),
    };

    // Step 2: X25519 shared secret.
    let shared = x25519_shared_secret(&eph_priv_bytes, &rec_enc_bytes)?;

    // Step 3: salt = SHA-256(eph_pub || recipient_enc_pub) — stack allocated.
    let mut salt_input = [0u8; 64];
    salt_input[..32].copy_from_slice(&eph_pub_bytes);
    salt_input[32..].copy_from_slice(&rec_enc_bytes);
    let salt = sha256(&salt_input);

    // Steps 4-5: HKDF → key + nonce.
    let (key, nonce) = derive_private_message_key(&shared, &salt);

    // Step 6-7: encrypt canonical(content).
    let mut plaintext_bytes = canonical_value_bytes(content)?;
    chacha20_encrypt_in_place(&key, &nonce, &mut plaintext_bytes);

    Ok(PrivateCiphertext {
        eph: base64url::encode(&eph_pub_bytes),
        encrypted: base64url::encode(&plaintext_bytes),
    })
}

/// Decrypt a `PrivateCiphertext` using the recipient's X25519 keypair (spec §3.5.3).
///
/// The public key is used only for the salt; passing it avoids recomputing it
/// from the private key on every message.
pub fn decrypt_private(
    c: &PrivateCiphertext,
    recipient_enc_priv: &str,
    recipient_enc_pub: &str,
) -> Result<Value> {
    let eph_pub_bytes = base64url::decode(&c.eph).context("invalid eph base64url")?;
    let rec_enc_priv_bytes =
        base64url::decode(recipient_enc_priv).context("invalid recipient enc priv base64url")?;
    let rec_enc_pub_bytes =
        base64url::decode(recipient_enc_pub).context("invalid recipient enc pub base64url")?;

    // Step 1: shared secret using recipient's private key.
    let shared = x25519_shared_secret(&rec_enc_priv_bytes, &eph_pub_bytes)?;

    // Step 2: same salt derivation as encryption.
    let mut salt_input = Vec::with_capacity(64);
    salt_input.extend_from_slice(&eph_pub_bytes);
    salt_input.extend_from_slice(&rec_enc_pub_bytes);
    let salt = sha256(&salt_input);

    // Steps 3-4: same HKDF.
    let (key, nonce) = derive_private_message_key(&shared, &salt);

    // Step 5: decrypt.
    let mut plaintext_bytes =
        base64url::decode(&c.encrypted).context("invalid encrypted base64url")?;
    chacha20_decrypt_in_place(&key, &nonce, &mut plaintext_bytes)
        .context("ChaCha20-Poly1305 decryption failed")?;

    serde_json::from_slice(&plaintext_bytes).context("decrypted content is not valid JSON")
}

/// v2 static-static private payload: the sender's long-lived X25519 public key,
/// a random per-message salt, and the ciphertext (spec §3.5.3 v2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateCiphertextV2 {
    /// 43-char base64url sender X25519 public key.
    pub from: String,
    /// 43-char base64url random 32-byte salt.
    pub salt: String,
    /// base64url ChaCha20-Poly1305 ciphertext (canonical JSON of content).
    pub encrypted: String,
}

impl PrivateCiphertextV2 {
    /// Serialize to the envelope `c` object.
    pub fn to_content(&self) -> Value {
        json!({
            "v": 2,
            "from": self.from,
            "salt": self.salt,
            "encrypted": self.encrypted,
        })
    }
}

fn decode_key(value: &str, label: &str) -> Result<[u8; 32]> {
    let bytes = base64url::decode(value).with_context(|| format!("invalid {label} base64url"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("{label} must decode to 32 bytes"))
}

/// Derive the cached pairwise key for a peer.
///
/// This is one X25519 operation per peer, not per message; callers should cache
/// the result (for example with `PairKeyCache`) and reuse it for every message.
pub fn derive_pair_key(own_enc_priv: &str, peer_enc_pub: &str) -> Result<[u8; 32]> {
    let own = decode_key(own_enc_priv, "local enc priv")?;
    let peer = decode_key(peer_enc_pub, "peer enc pub")?;
    let mut shared = x25519_shared_secret(&own, &peer)?;
    let okm = hkdf_sha256(&shared, PAIR_KEY_INFO, b"pair", 32);
    shared.fill(0);

    let mut key = [0u8; 32];
    key.copy_from_slice(&okm[..32]);
    Ok(key)
}

fn message_key_from_pair(pair_key: &[u8; 32], salt: &[u8; 32]) -> ([u8; 32], [u8; 12]) {
    let okm = hkdf_sha256(pair_key, salt, MESSAGE_INFO, 44);
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 12];
    key.copy_from_slice(&okm[0..32]);
    nonce.copy_from_slice(&okm[32..44]);
    (key, nonce)
}

fn message_salt_hash(sender_pub: &[u8; 32], recipient_pub: &[u8; 32], salt: &[u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 96];
    input[..32].copy_from_slice(sender_pub);
    input[32..64].copy_from_slice(recipient_pub);
    input[64..].copy_from_slice(salt);
    sha256(&input)
}

/// Encrypt a v2 private message with a freshly derived pairwise key.
///
/// Prefer `encrypt_private_v2_with_key` with a cached pair key for repeated
/// messages to the same recipient.
pub fn encrypt_private_v2(
    content: &Value,
    sender_enc_priv: &str,
    sender_enc_pub: &str,
    recipient_enc_pub: &str,
) -> Result<PrivateCiphertextV2> {
    let mut pair_key = derive_pair_key(sender_enc_priv, recipient_enc_pub)?;
    let result = encrypt_private_v2_with_key(content, sender_enc_pub, recipient_enc_pub, &pair_key);
    pair_key.fill(0);
    result
}

/// Encrypt a v2 private message using a cached pairwise key.
pub fn encrypt_private_v2_with_key(
    content: &Value,
    sender_enc_pub: &str,
    recipient_enc_pub: &str,
    pair_key: &[u8; 32],
) -> Result<PrivateCiphertextV2> {
    let sender_pub = decode_key(sender_enc_pub, "sender enc pub")?;
    let recipient_pub = decode_key(recipient_enc_pub, "recipient enc pub")?;

    let mut salt = [0u8; 32];
    getrandom::fill(&mut salt).expect("system RNG failure");
    let salt_hash = message_salt_hash(&sender_pub, &recipient_pub, &salt);
    let (mut key, nonce) = message_key_from_pair(pair_key, &salt_hash);

    let mut plaintext_bytes = canonical_value_bytes(content)?;
    chacha20_encrypt_in_place(&key, &nonce, &mut plaintext_bytes);
    key.fill(0);

    Ok(PrivateCiphertextV2 {
        from: sender_enc_pub.to_owned(),
        salt: base64url::encode(&salt),
        encrypted: base64url::encode(&plaintext_bytes),
    })
}

/// Decrypt a v2 private message with a freshly derived pairwise key.
pub fn decrypt_private_v2(
    c: &PrivateCiphertextV2,
    recipient_enc_priv: &str,
    recipient_enc_pub: &str,
) -> Result<Value> {
    let mut pair_key = derive_pair_key(recipient_enc_priv, &c.from)?;
    let result = decrypt_private_v2_with_key(c, recipient_enc_pub, &pair_key);
    pair_key.fill(0);
    result
}

/// Decrypt a v2 private message using a cached pairwise key.
pub fn decrypt_private_v2_with_key(
    c: &PrivateCiphertextV2,
    recipient_enc_pub: &str,
    pair_key: &[u8; 32],
) -> Result<Value> {
    let sender_pub = decode_key(&c.from, "sender enc pub")?;
    let recipient_pub = decode_key(recipient_enc_pub, "recipient enc pub")?;
    let salt = decode_key(&c.salt, "private salt")?;

    let salt_hash = message_salt_hash(&sender_pub, &recipient_pub, &salt);
    let (mut key, nonce) = message_key_from_pair(pair_key, &salt_hash);

    let mut plaintext_bytes =
        base64url::decode(&c.encrypted).context("invalid encrypted base64url")?;
    let decrypted = chacha20_decrypt_in_place(&key, &nonce, &mut plaintext_bytes);
    key.fill(0);
    decrypted.context("ChaCha20-Poly1305 decryption failed")?;

    serde_json::from_slice(&plaintext_bytes).context("decrypted content is not valid JSON")
}

/// Small cache of pairwise keys for repeated messages to the same peers.
///
/// One cache belongs to one local identity: changing `own_enc_priv` requires
/// `clear()`. When full, an arbitrary entry is evicted; callers can size the
/// cache to their contact set.
pub struct PairKeyCache {
    capacity: usize,
    keys: HashMap<String, [u8; 32]>,
}

impl PairKeyCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            keys: HashMap::new(),
        }
    }

    pub fn pair_key(&mut self, own_enc_priv: &str, peer_enc_pub: &str) -> Result<[u8; 32]> {
        if let Some(key) = self.keys.get(peer_enc_pub) {
            return Ok(*key);
        }

        let key = derive_pair_key(own_enc_priv, peer_enc_pub)?;
        if self.keys.len() >= self.capacity {
            if let Some(oldest) = self.keys.keys().next().cloned() {
                if let Some(mut old) = self.keys.remove(&oldest) {
                    old.fill(0);
                }
            }
        }
        self.keys.insert(peer_enc_pub.to_owned(), key);
        Ok(key)
    }

    pub fn clear(&mut self) {
        for key in self.keys.values_mut() {
            key.fill(0);
        }
        self.keys.clear();
    }
}

impl Drop for PairKeyCache {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decrypt_private, decrypt_private_v2, derive_pair_key, encrypt_private, encrypt_private_v2,
        encrypt_private_v2_with_key, encrypt_private_with_eph, PairKeyCache, PrivateCiphertext,
        PrivateCiphertextV2,
    };
    use crate::base64url;
    use crate::fixtures::*;

    use serde_json::json;

    // Helper: derive X25519 public key from a 32-byte seed.
    fn x25519_pub_from_seed(seed_hex: &str) -> (String, String) {
        let priv_bytes: [u8; 32] = crate::hex::decode(seed_hex).unwrap().try_into().unwrap();
        let pub_b64 = crate::base64url::encode(&crate::crypto::x25519_pub_from_seed(&priv_bytes));
        let priv_b64 = crate::base64url::encode(&priv_bytes);
        (pub_b64, priv_b64)
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let recipient = mk_node();
        let content = json!({"body": "hello world", "blobs": []});

        let ct = encrypt_private(&content, &recipient.enc.pub_key).expect("encrypt should succeed");

        assert_eq!(ct.eph.len(), 43, "eph must be 43-char base64url");
        assert!(!ct.encrypted.is_empty(), "encrypted must not be empty");

        let decrypted = decrypt_private(&ct, &recipient.enc.priv_key, &recipient.enc.pub_key)
            .expect("decrypt should succeed");

        assert_eq!(decrypted, content);
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let recipient = mk_node();
        let content = json!({"body": "secret"});

        let mut ct =
            encrypt_private(&content, &recipient.enc.pub_key).expect("encrypt should succeed");

        // Corrupt the last character of the ciphertext.
        let mut bytes = base64url::decode(&ct.encrypted).unwrap();
        let last = bytes.last_mut().unwrap();
        *last ^= 0xff;
        ct.encrypted = base64url::encode(&bytes);

        assert!(decrypt_private(&ct, &recipient.enc.priv_key, &recipient.enc.pub_key).is_err());
    }

    #[test]
    fn wrong_recipient_key_fails() {
        let recipient = mk_node();
        let wrong = mk_node();
        let content = json!({"body": "top secret"});

        let ct = encrypt_private(&content, &recipient.enc.pub_key).expect("encrypt should succeed");

        assert!(decrypt_private(&ct, &wrong.enc.priv_key, &wrong.enc.pub_key).is_err());
    }

    #[test]
    fn eph_key_differs_across_encryptions() {
        let recipient = mk_node();
        let content = json!({"body": "same"});

        let ct1 = encrypt_private(&content, &recipient.enc.pub_key).unwrap();
        let ct2 = encrypt_private(&content, &recipient.enc.pub_key).unwrap();

        assert_ne!(
            ct1.eph, ct2.eph,
            "each call must generate a fresh ephemeral key"
        );
        assert_ne!(ct1.encrypted, ct2.encrypted);

        // Both must decrypt to same content.
        assert_eq!(
            decrypt_private(&ct1, &recipient.enc.priv_key, &recipient.enc.pub_key).unwrap(),
            content
        );
        assert_eq!(
            decrypt_private(&ct2, &recipient.enc.priv_key, &recipient.enc.pub_key).unwrap(),
            content
        );
    }

    /// Test vector: §E.7 — deterministic encryption using fixed ephemeral seed.
    /// Verifies byte-for-byte compatibility with toloo.js.
    #[test]
    fn test_vector_e7_deterministic_encryption() {
        // Babak's X25519 encryption keypair (from §E.2.2).
        let babak_enc_seed = "2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a";
        let (babak_enc_pub, babak_enc_priv) = x25519_pub_from_seed(babak_enc_seed);

        // Ephemeral seed (§E.7.1) — all 0x11 bytes.
        let eph_seed_hex = "1111111111111111111111111111111111111111111111111111111111111111";
        let eph_seed_bytes: Vec<u8> = (0..eph_seed_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&eph_seed_hex[i..i + 2], 16).unwrap())
            .collect();
        let eph_seed: [u8; 32] = eph_seed_bytes.try_into().unwrap();

        let content = json!({"body": "Hello Babak"});

        let ct = encrypt_private_with_eph(&content, &babak_enc_pub, Some(&eph_seed))
            .expect("deterministic encrypt should succeed");

        // Eph pub must be stable (deterministic from seed).
        assert_eq!(ct.eph.len(), 43);

        // Decryption must recover original content.
        let decrypted =
            decrypt_private(&ct, &babak_enc_priv, &babak_enc_pub).expect("decrypt should succeed");
        assert_eq!(decrypted, content);

        // Encrypting again with same seed must produce identical output.
        let ct2 = encrypt_private_with_eph(&content, &babak_enc_pub, Some(&eph_seed)).unwrap();
        assert_eq!(ct.eph, ct2.eph);
        assert_eq!(ct.encrypted, ct2.encrypted);
    }

    #[test]
    fn invalid_recipient_pub_returns_error() {
        let content = json!({"body": "test"});
        assert!(encrypt_private(&content, "not-a-key").is_err());
    }

    #[test]
    fn invalid_eph_in_ciphertext_returns_error() {
        let recipient = mk_node();
        let ct = PrivateCiphertext {
            eph: "bad".to_owned(),
            encrypted: "dGVzdA".to_owned(),
        };
        assert!(decrypt_private(&ct, &recipient.enc.priv_key, &recipient.enc.pub_key).is_err());
    }

    #[test]
    fn v2_roundtrip_with_cached_pair_key() {
        let sender = mk_node();
        let recipient = mk_node();
        let content = json!({"body": "hello v2", "blobs": []});

        let mut cache = PairKeyCache::new(16);
        let send_key = cache
            .pair_key(&sender.enc.priv_key, &recipient.enc.pub_key)
            .expect("sender pair key");
        let ct = encrypt_private_v2_with_key(
            &content,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
            &send_key,
        )
        .expect("encrypt");

        assert_eq!(ct.salt.len(), 43);
        assert!(!ct.encrypted.is_empty());

        let recv_key = cache
            .pair_key(&recipient.enc.priv_key, &sender.enc.pub_key)
            .expect("recipient pair key");
        assert_eq!(send_key, recv_key, "X25519 pair keys must be symmetric");

        let decrypted = super::decrypt_private_v2_with_key(&ct, &recipient.enc.pub_key, &recv_key)
            .expect("decrypt");
        assert_eq!(decrypted, content);
    }

    #[test]
    fn v2_cold_roundtrip_and_salt_uniqueness() {
        let sender = mk_node();
        let recipient = mk_node();
        let content = json!({"body": "same content"});

        let ct1 = encrypt_private_v2(
            &content,
            &sender.enc.priv_key,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
        )
        .expect("encrypt 1");
        let ct2 = encrypt_private_v2(
            &content,
            &sender.enc.priv_key,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
        )
        .expect("encrypt 2");

        assert_ne!(ct1.salt, ct2.salt, "salt must be fresh per message");
        assert_ne!(ct1.encrypted, ct2.encrypted);
        assert_eq!(ct1.from, sender.enc.pub_key);

        for ct in [ct1, ct2] {
            let decrypted =
                decrypt_private_v2(&ct, &recipient.enc.priv_key, &recipient.enc.pub_key)
                    .expect("decrypt");
            assert_eq!(decrypted, content);
        }
    }

    #[test]
    fn v2_tamper_and_wrong_sender_fail() {
        let sender = mk_node();
        let recipient = mk_node();
        let wrong = mk_node();
        let content = json!({"body": "secret"});

        let ct = encrypt_private_v2(
            &content,
            &sender.enc.priv_key,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
        )
        .expect("encrypt");

        let mut tampered = ct.clone();
        let mut bytes = base64url::decode(&tampered.encrypted).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        tampered.encrypted = base64url::encode(&bytes);
        assert!(
            decrypt_private_v2(&tampered, &recipient.enc.priv_key, &recipient.enc.pub_key).is_err()
        );

        let mut impostor = ct;
        impostor.from = wrong.enc.pub_key.clone();
        assert!(
            decrypt_private_v2(&impostor, &recipient.enc.priv_key, &recipient.enc.pub_key).is_err()
        );
    }

    #[test]
    fn v2_invalid_fields_are_rejected() {
        let sender = mk_node();
        let recipient = mk_node();
        let content = json!({"body": "x"});
        let key = derive_pair_key(&sender.enc.priv_key, &recipient.enc.pub_key).unwrap();

        let mut ct = encrypt_private_v2_with_key(
            &content,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
            &key,
        )
        .unwrap();
        ct.salt = "bad".to_owned();
        assert!(super::decrypt_private_v2_with_key(&ct, &recipient.enc.pub_key, &key).is_err());
    }

    #[test]
    fn pair_key_cache_reuses_and_clears() {
        let sender = mk_node();
        let recipient = mk_node();
        let mut cache = PairKeyCache::new(2);

        let first = cache
            .pair_key(&sender.enc.priv_key, &recipient.enc.pub_key)
            .unwrap();
        let second = cache
            .pair_key(&sender.enc.priv_key, &recipient.enc.pub_key)
            .unwrap();
        assert_eq!(first, second);

        cache.clear();
        let third = cache
            .pair_key(&sender.enc.priv_key, &recipient.enc.pub_key)
            .unwrap();
        assert_eq!(first, third);
    }

    #[test]
    fn v2_content_shape_is_versioned() {
        let sender = mk_node();
        let recipient = mk_node();
        let ct = encrypt_private_v2(
            &json!({"body": "x"}),
            &sender.enc.priv_key,
            &sender.enc.pub_key,
            &recipient.enc.pub_key,
        )
        .unwrap();
        let content = ct.to_content();
        assert_eq!(content["v"], 2);
        assert_eq!(content["from"], sender.enc.pub_key);
        assert_eq!(content["salt"], ct.salt);
        assert_eq!(content["encrypted"], ct.encrypted);
    }

    #[test]
    fn v2_empty_ciphertext_fails() {
        let recipient = mk_node();
        let ct = PrivateCiphertextV2 {
            from: recipient.enc.pub_key.clone(),
            salt: base64url::encode(&[0u8; 32]),
            encrypted: String::new(),
        };
        assert!(decrypt_private_v2(&ct, &recipient.enc.priv_key, &recipient.enc.pub_key).is_err());
    }
}
