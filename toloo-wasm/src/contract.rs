//! JSON contract shared by the wasm bindings and their tests.
//!
//! All inputs and outputs are JSON strings so the JS side never has to deal
//! with pointers, slices, or wasm memory lifetimes.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use toloo_core::base64url;
use toloo_core::canonical::canonical_value;
use toloo_core::crypto::{
    ed25519_generate, ed25519_pub_from_seed, x25519_generate, x25519_pub_from_seed,
};
use toloo_core::envelope::{
    depth, envelope_to_json_string, innermost, make_envelope, parse_envelope, verify_chain,
    verify_chains_batch, DatumBodyExtra,
};
use toloo_core::ids::{channel_of, datum_id, eid};
use toloo_core::private::{
    decrypt_private_v2, decrypt_private_v2_with_key, encrypt_private_v2,
    encrypt_private_v2_with_key, PairKeyCache, PrivateCiphertextV2,
};
use toloo_core::types::{Envelope, Keypair, LocalNode};

pub const PROTOCOL_VERSION: &str = "0.2";

pub fn core_version() -> &'static str {
    PROTOCOL_VERSION
}

/// Generate a new node identity.
///
/// Returns `{"sigPub","sigSeed","encPub","encPriv"}` (base64url strings).
pub fn generate_identity() -> Result<String> {
    let (sig_pub, sig_seed) = ed25519_generate();
    let (enc_pub, enc_priv) = x25519_generate();
    Ok(json!({
        "sigPub": base64url::encode(&sig_pub),
        "sigSeed": base64url::encode(&sig_seed),
        "encPub": base64url::encode(&enc_pub),
        "encPriv": base64url::encode(&enc_priv),
    })
    .to_string())
}

/// Rebuild a node identity from its secrets.
pub fn import_identity(sig_seed: &str, enc_priv: &str) -> Result<String> {
    let sig_seed_bytes = base64url::decode(sig_seed).context("invalid sig seed base64url")?;
    let sig_pub = ed25519_pub_from_seed(&sig_seed_bytes).context("invalid sig seed")?;

    let enc_priv_bytes = base64url::decode(enc_priv).context("invalid enc priv base64url")?;
    let enc_priv_arr: [u8; 32] = enc_priv_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("enc priv must decode to 32 bytes"))?;
    let enc_pub = x25519_pub_from_seed(&enc_priv_arr);

    Ok(json!({
        "sigPub": base64url::encode(&sig_pub),
        "sigSeed": sig_seed,
        "encPub": base64url::encode(&enc_pub),
        "encPriv": enc_priv,
    })
    .to_string())
}

/// Build and sign a datum envelope.
///
/// `extra_json` is an optional object with `r`, `to`, `tc`, `exp`, `nonce`.
pub fn make_node_envelope(
    kind: &str,
    datum_json: &str,
    sig_pub: &str,
    sig_seed: &str,
    extra_json: Option<&str>,
) -> Result<String> {
    let datum: Value = serde_json::from_str(datum_json).context("invalid datum JSON")?;
    let extra = parse_extra(extra_json)?;
    let node = LocalNode {
        sig: Keypair {
            pub_key: sig_pub.to_owned(),
            priv_key: sig_seed.to_owned(),
        },
        enc: Keypair {
            pub_key: String::new(),
            priv_key: String::new(),
        },
    };
    let envelope = make_envelope(kind, Some(datum), &node, extra)?;
    Ok(envelope_to_json_string(&envelope))
}

fn parse_extra(extra_json: Option<&str>) -> Result<Option<DatumBodyExtra>> {
    let Some(raw) = extra_json else {
        return Ok(None);
    };
    if raw.trim().is_empty() || raw.trim() == "null" {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(raw).context("invalid extra JSON")?;
    if value.is_null() {
        return Ok(None);
    }
    let obj = value.as_object().context("extra must be a JSON object")?;
    Ok(Some(DatumBodyExtra {
        r: obj.get("r").and_then(Value::as_str).map(str::to_owned),
        to: obj.get("to").and_then(Value::as_str).map(str::to_owned),
        tc: obj.get("tc").and_then(Value::as_u64),
        exp: obj.get("exp").and_then(Value::as_u64),
        nonce: obj.get("nonce").and_then(Value::as_u64),
    }))
}

fn parse_envelope_json(json_str: &str) -> Result<Envelope> {
    let value: Value = serde_json::from_str(json_str).context("invalid envelope JSON")?;
    parse_envelope(value)
}

/// Verify an envelope chain.
///
/// Returns `{"ok":true}` or `{"ok":false,"error":"..."}`; verification
/// failures are values, not exceptions.
pub fn verify_chain_result(envelope_json: &str) -> String {
    let result = parse_envelope_json(envelope_json).and_then(|env| verify_chain(&env));
    verdict_json(result.err())
}

fn verdict_json(error: Option<anyhow::Error>) -> String {
    match error {
        None => json!({ "ok": true }).to_string(),
        Some(e) => json!({ "ok": false, "error": format!("{e:#}") }).to_string(),
    }
}

/// Verify many envelope chains with one batch; returns an array of verdicts
/// in input order.
pub fn verify_chains_result(envelopes_json: &str) -> Result<String> {
    let values: Vec<Value> =
        serde_json::from_str(envelopes_json).context("envelopes must be a JSON array")?;
    let mut parsed: Vec<Option<Envelope>> = Vec::with_capacity(values.len());
    let mut parse_errors: Vec<Option<String>> = Vec::with_capacity(values.len());
    for value in values {
        match parse_envelope(value) {
            Ok(env) => {
                parsed.push(Some(env));
                parse_errors.push(None);
            }
            Err(e) => {
                parsed.push(None);
                parse_errors.push(Some(format!("{e:#}")));
            }
        }
    }

    let refs: Vec<&Envelope> = parsed.iter().flatten().collect();
    let mut verdicts = verify_chains_batch(&refs).into_iter();
    let mut output = Vec::with_capacity(parsed.len());
    for (env, parse_error) in parsed.iter().zip(parse_errors) {
        if env.is_some() {
            let result = verdicts.next().expect("one verdict per parsed envelope");
            output.push(verdict_json(result.err()));
        } else {
            output
                .push(json!({ "ok": false, "error": parse_error.unwrap_or_default() }).to_string());
        }
    }
    Ok(format!("[{}]", output.join(",")))
}

/// Envelope identifiers and shape.
pub fn envelope_info(envelope_json: &str) -> Result<String> {
    let envelope = parse_envelope_json(envelope_json)?;
    let inner = innermost(&envelope);
    Ok(json!({
        "eid": eid(&envelope),
        "datumId": datum_id(&envelope),
        "depth": depth(&envelope),
        "type": inner.d.t,
        "channel": channel_of(&inner.d),
        "ts": inner.d.ts,
        "signer": inner.d.n,
    })
    .to_string())
}

/// Canonical JSON encoding (spec §2.3).
pub fn canonicalize_json(json_str: &str) -> Result<String> {
    let value: Value = serde_json::from_str(json_str).context("invalid JSON")?;
    canonical_value(&value)
}

/// Encrypt a private message payload for `recipient_enc_pub`.
///
/// Returns the v2 `c` object `{"v":2,"from","salt","encrypted"}`.
pub fn private_encrypt(
    content_json: &str,
    sender_enc_priv: &str,
    sender_enc_pub: &str,
    recipient_enc_pub: &str,
) -> Result<String> {
    let content: Value = serde_json::from_str(content_json).context("invalid content JSON")?;
    let cipher = encrypt_private_v2(&content, sender_enc_priv, sender_enc_pub, recipient_enc_pub)?;
    Ok(cipher.to_content().to_string())
}

fn cipher_from_json(cipher_json: &str) -> Result<PrivateCiphertextV2> {
    let value: Value = serde_json::from_str(cipher_json).context("invalid cipher JSON")?;
    Ok(PrivateCiphertextV2 {
        from: value
            .get("from")
            .and_then(Value::as_str)
            .context("missing cipher.from")?
            .to_owned(),
        salt: value
            .get("salt")
            .and_then(Value::as_str)
            .context("missing cipher.salt")?
            .to_owned(),
        encrypted: value
            .get("encrypted")
            .and_then(Value::as_str)
            .context("missing cipher.encrypted")?
            .to_owned(),
    })
}

/// Decrypt a v2 private payload; returns the content JSON.
pub fn private_decrypt(
    cipher_json: &str,
    recipient_enc_priv: &str,
    recipient_enc_pub: &str,
) -> Result<String> {
    let cipher = cipher_from_json(cipher_json)?;
    let content = decrypt_private_v2(&cipher, recipient_enc_priv, recipient_enc_pub)?;
    Ok(content.to_string())
}

/// Private-message encryption with a pairwise-key cache.
///
/// The cache belongs to one local identity; call `clear` when it changes.
pub struct CachedPrivate {
    cache: PairKeyCache,
}

impl CachedPrivate {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: PairKeyCache::new(capacity),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn encrypt(
        &mut self,
        content_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
        peer_enc_pub: &str,
    ) -> Result<String> {
        let content: Value = serde_json::from_str(content_json).context("invalid content JSON")?;
        let mut pair_key = self.cache.pair_key(own_enc_priv, peer_enc_pub)?;
        let result = encrypt_private_v2_with_key(&content, own_enc_pub, peer_enc_pub, &pair_key);
        pair_key.fill(0);
        Ok(result?.to_content().to_string())
    }

    pub fn decrypt(
        &mut self,
        cipher_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
    ) -> Result<String> {
        let cipher = cipher_from_json(cipher_json)?;
        let mut pair_key = self.cache.pair_key(own_enc_priv, &cipher.from)?;
        let result = decrypt_private_v2_with_key(&cipher, own_enc_pub, &pair_key);
        pair_key.fill(0);
        Ok(result?.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trip_preserves_keys() {
        let identity = generate_identity().unwrap();
        let value: Value = serde_json::from_str(&identity).unwrap();
        let imported = import_identity(
            value["sigSeed"].as_str().unwrap(),
            value["encPriv"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(identity, imported);
    }

    #[test]
    fn envelope_build_verify_and_info() {
        let identity = generate_identity().unwrap();
        let id: Value = serde_json::from_str(&identity).unwrap();
        let env = make_node_envelope(
            "node.meta",
            r#"{"name":"test"}"#,
            id["sigPub"].as_str().unwrap(),
            id["sigSeed"].as_str().unwrap(),
            None,
        )
        .unwrap();

        let verdict = verify_chain_result(&env);
        assert_eq!(verdict, r#"{"ok":true}"#);

        let info: Value = serde_json::from_str(&envelope_info(&env).unwrap()).unwrap();
        assert_eq!(info["type"], "node.meta");
        assert_eq!(info["depth"], 1);
        assert_eq!(info["signer"], id["sigPub"].as_str().unwrap());
    }

    #[test]
    fn batch_verify_keeps_input_order() {
        let identity = generate_identity().unwrap();
        let id: Value = serde_json::from_str(&identity).unwrap();
        let env = make_node_envelope(
            "node.meta",
            r#"{"name":"one"}"#,
            id["sigPub"].as_str().unwrap(),
            id["sigSeed"].as_str().unwrap(),
            None,
        )
        .unwrap();
        let valid: Value = serde_json::from_str(&env).unwrap();
        let mut tampered = valid.clone();
        tampered["d"]["c"]["name"] = json!("two");

        let input = json!([valid, tampered]).to_string();
        let verdicts: Vec<Value> =
            serde_json::from_str(&verify_chains_result(&input).unwrap()).unwrap();
        assert_eq!(verdicts[0]["ok"], true);
        assert_eq!(verdicts[1]["ok"], false);
    }

    #[test]
    fn batch_verify_aligns_around_parse_errors() {
        let identity = generate_identity().unwrap();
        let id: Value = serde_json::from_str(&identity).unwrap();
        let env = make_node_envelope(
            "node.meta",
            r#"{"name":"one"}"#,
            id["sigPub"].as_str().unwrap(),
            id["sigSeed"].as_str().unwrap(),
            None,
        )
        .unwrap();
        let valid: Value = serde_json::from_str(&env).unwrap();

        let input = json!([valid.clone(), { "not": "an envelope" }, valid]).to_string();
        let verdicts: Vec<Value> =
            serde_json::from_str(&verify_chains_result(&input).unwrap()).unwrap();
        assert_eq!(verdicts.len(), 3);
        assert_eq!(verdicts[0]["ok"], true);
        assert_eq!(verdicts[1]["ok"], false);
        assert_eq!(verdicts[2]["ok"], true);
    }

    #[test]
    fn private_v2_round_trip() {
        let alice: Value = serde_json::from_str(&generate_identity().unwrap()).unwrap();
        let bob: Value = serde_json::from_str(&generate_identity().unwrap()).unwrap();

        let cipher = private_encrypt(
            r#"{"body":"hello"}"#,
            alice["encPriv"].as_str().unwrap(),
            alice["encPub"].as_str().unwrap(),
            bob["encPub"].as_str().unwrap(),
        )
        .unwrap();
        let plaintext = private_decrypt(
            &cipher,
            bob["encPriv"].as_str().unwrap(),
            bob["encPub"].as_str().unwrap(),
        )
        .unwrap();
        let content: Value = serde_json::from_str(&plaintext).unwrap();
        assert_eq!(content["body"], "hello");
    }

    #[test]
    fn canonicalize_is_stable() {
        let canonical = canonicalize_json(r#"{ "b": 2, "a": 1 }"#).unwrap();
        assert_eq!(canonical, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn cached_private_round_trip() {
        let alice: Value = serde_json::from_str(&generate_identity().unwrap()).unwrap();
        let bob: Value = serde_json::from_str(&generate_identity().unwrap()).unwrap();
        let mut alice_cache = CachedPrivate::new(8);
        let mut bob_cache = CachedPrivate::new(8);

        for body in ["one", "two", "three"] {
            let cipher = alice_cache
                .encrypt(
                    &json!({ "body": body }).to_string(),
                    alice["encPriv"].as_str().unwrap(),
                    alice["encPub"].as_str().unwrap(),
                    bob["encPub"].as_str().unwrap(),
                )
                .unwrap();
            let plaintext = bob_cache
                .decrypt(
                    &cipher,
                    bob["encPriv"].as_str().unwrap(),
                    bob["encPub"].as_str().unwrap(),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&plaintext).unwrap()["body"],
                body
            );
        }
    }
}
