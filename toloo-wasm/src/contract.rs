//! JSON contract shared by the wasm bindings and their tests.
//!
//! All inputs and outputs are JSON strings so the JS side never has to deal
//! with pointers, slices, or wasm memory lifetimes.

use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use toloo_core::base64url;
use toloo_core::canonical::canonical_value;
use toloo_core::crypto::{
    ed25519_generate, ed25519_pub_from_seed, sha256, x25519_generate, x25519_pub_from_seed,
};
use toloo_core::discovery;
use toloo_core::envelope::{
    depth, envelope_to_json_string, innermost, make_envelope, parse_envelope, sign_envelope,
    verify_chain, verify_chains_batch, wrap_commit as wrap_commit_envelope, DatumBodyExtra,
};
use toloo_core::events::{self, BlobManifest};
use toloo_core::ids::{channel_of, datum_id, eid};
use toloo_core::membership;
use toloo_core::pow;
use toloo_core::private::{
    decrypt_private_v2, decrypt_private_v2_with_key, encrypt_private_v2,
    encrypt_private_v2_with_key, PairKeyCache, PrivateCiphertextV2,
};
use toloo_core::rules::RuleSet;
use toloo_core::types::{EndpointDescriptor, Envelope, Keypair, LocalNode, LocalRoom};

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

/// Generate a room signing identity (`{"sigPub","sigSeed"}`).
pub fn generate_room_identity() -> Result<String> {
    let (room_pub, room_seed) = ed25519_generate();
    Ok(json!({
        "sigPub": base64url::encode(&room_pub),
        "sigSeed": base64url::encode(&room_seed),
    })
    .to_string())
}

/// Rebuild a room signing identity from its seed.
pub fn import_room_identity(sig_seed: &str) -> Result<String> {
    let seed_bytes = base64url::decode(sig_seed).context("invalid room seed base64url")?;
    let sig_pub = ed25519_pub_from_seed(&seed_bytes).context("invalid room seed")?;
    Ok(json!({
        "sigPub": base64url::encode(&sig_pub),
        "sigSeed": sig_seed,
    })
    .to_string())
}

fn identity_field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("identity missing {name}"))
}

fn parse_identity(identity_json: &str) -> Result<LocalNode> {
    let value: Value = serde_json::from_str(identity_json).context("invalid identity JSON")?;
    if !value.is_object() {
        bail!("identity must be a JSON object");
    }
    Ok(LocalNode {
        sig: Keypair {
            pub_key: identity_field(&value, "sigPub")?.to_owned(),
            priv_key: identity_field(&value, "sigSeed")?.to_owned(),
        },
        enc: Keypair {
            pub_key: identity_field(&value, "encPub")?.to_owned(),
            priv_key: identity_field(&value, "encPriv")?.to_owned(),
        },
    })
}

fn parse_room_identity(room_json: &str) -> Result<LocalRoom> {
    let value: Value = serde_json::from_str(room_json).context("invalid room identity JSON")?;
    if !value.is_object() {
        bail!("room identity must be a JSON object");
    }
    Ok(LocalRoom {
        sig: Keypair {
            pub_key: identity_field(&value, "sigPub")?.to_owned(),
            priv_key: identity_field(&value, "sigSeed")?.to_owned(),
        },
    })
}

fn optional_json<T: DeserializeOwned>(raw: Option<&str>, label: &str) -> Result<Option<T>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.trim().is_empty() || raw.trim() == "null" {
        return Ok(None);
    }
    serde_json::from_str(raw)
        .with_context(|| format!("invalid {label} JSON"))
        .map(Some)
}

fn string_array(raw: &str, label: &str) -> Result<Vec<String>> {
    serde_json::from_str(raw).with_context(|| format!("{label} must be a JSON string array"))
}

fn optional_string_array(raw: Option<&str>, label: &str) -> Result<Vec<String>> {
    Ok(optional_json::<Vec<String>>(raw, label)?.unwrap_or_default())
}

fn envelope_pair_json(created: Envelope, committed: Envelope) -> Result<String> {
    Ok(json!({
        "created": serde_json::to_value(&created)?,
        "committed": serde_json::to_value(&committed)?,
    })
    .to_string())
}

fn parse_envelopes_array(raw: &str) -> Result<Vec<Envelope>> {
    let values: Vec<Value> = serde_json::from_str(raw).context("envelopes must be a JSON array")?;
    values.into_iter().map(parse_envelope).collect()
}

fn make_request_envelope(identity_json: &str, kind: &str, content: Value) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&make_envelope(
        kind,
        Some(content),
        &node,
        None,
    )?))
}

fn insert_opt_u64(content: &mut Value, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        content[key] = json!(value);
    }
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

/// node.meta — publish signing/encryption keys and reachability endpoints.
pub fn make_node_meta(identity_json: &str, endpoints_json: Option<&str>) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let endpoints =
        optional_json::<Vec<EndpointDescriptor>>(endpoints_json, "endpoints")?.unwrap_or_default();
    Ok(envelope_to_json_string(&events::make_node_meta(
        &node, endpoints,
    )?))
}

/// room.create — returns `{"created","committed"}` envelopes.
pub fn make_room_create(
    identity_json: &str,
    room_json: &str,
    name: Option<&str>,
    rules_json: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let room = parse_room_identity(room_json)?;
    let rules = optional_json::<Value>(rules_json, "room rules")?;
    let (created, committed) = events::make_room_create(&node, &room, name, rules)?;
    envelope_pair_json(created, committed)
}

/// room.announce — official channel-0 post, authored and committed by the room.
pub fn make_room_announcement(room_json: &str, body: &str) -> Result<String> {
    let room = parse_room_identity(room_json)?;
    let (created, committed) = events::make_room_announcement(&room, body)?;
    envelope_pair_json(created, committed)
}

pub fn make_room_join(identity_json: &str, room_pub: &str, nonce: Option<u64>) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_join(
        &node, room_pub, nonce,
    )?))
}

pub fn make_room_leave(identity_json: &str, room_pub: &str) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_leave(
        &node, room_pub,
    )?))
}

pub fn make_room_invite(identity_json: &str, room_pub: &str, invitee: &str) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_invite(
        &node, room_pub, invitee,
    )?))
}

pub fn make_room_ban(
    identity_json: &str,
    room_pub: &str,
    target: &str,
    reason: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_ban(
        &node, room_pub, target, reason,
    )?))
}

pub fn make_room_unban(identity_json: &str, room_pub: &str, target: &str) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_unban(
        &node, room_pub, target,
    )?))
}

pub fn make_room_update(
    identity_json: &str,
    room_json: &str,
    name: Option<&str>,
    rules_json: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let room = parse_room_identity(room_json)?;
    let rules = optional_json::<Value>(rules_json, "room rules")?;
    Ok(envelope_to_json_string(&events::make_room_update(
        &node, &room, name, rules,
    )?))
}

pub fn make_room_message(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    body: &str,
    blobs_json: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let blobs = optional_string_array(blobs_json, "blobs")?;
    Ok(envelope_to_json_string(&events::make_room_message(
        &node, room_pub, channel, body, blobs,
    )?))
}

pub fn make_room_react(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
    emoji: &str,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_react(
        &node, room_pub, channel, target_eid, emoji,
    )?))
}

pub fn make_room_edit(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
    body: &str,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_edit(
        &node, room_pub, channel, target_eid, body,
    )?))
}

pub fn make_room_delete(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_delete(
        &node, room_pub, channel, target_eid,
    )?))
}

pub fn make_room_blob(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    manifest_json: &str,
    mime: Option<&str>,
    name: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let manifest: BlobManifest =
        serde_json::from_str(manifest_json).context("invalid blob manifest JSON")?;
    Ok(envelope_to_json_string(&events::make_room_blob(
        &node, room_pub, channel, &manifest, mime, name,
    )?))
}

pub fn make_private_message(
    identity_json: &str,
    to: &str,
    to_enc_pub: &str,
    body: &str,
    blobs_json: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let blobs = optional_string_array(blobs_json, "blobs")?;
    Ok(envelope_to_json_string(&events::make_private_message(
        &node, to, to_enc_pub, body, blobs,
    )?))
}

pub fn make_private_read(identity_json: &str, to: &str, ids_json: &str) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let ids = string_array(ids_json, "ids")?;
    Ok(envelope_to_json_string(&events::make_private_read(
        &node, to, ids,
    )?))
}

pub fn make_side_fork(
    identity_json: &str,
    room_pub: &str,
    forked_from: &str,
    reason: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_side_fork(
        &node,
        room_pub,
        forked_from,
        reason,
    )?))
}

pub fn make_side_attestation(
    identity_json: &str,
    target_node: &str,
    level: &str,
    reason: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_side_attestation(
        &node,
        target_node,
        level,
        reason,
    )?))
}

pub fn make_room_flag(
    identity_json: &str,
    room_pub: &str,
    target_eid: &str,
    category: &str,
    reason: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    Ok(envelope_to_json_string(&events::make_room_flag(
        &node, room_pub, target_eid, category, reason,
    )?))
}

pub fn make_room_migrate(
    identity_json: &str,
    room_json: &str,
    new_room_pub: &str,
    reason: Option<&str>,
) -> Result<String> {
    let node = parse_identity(identity_json)?;
    let room = parse_room_identity(room_json)?;
    Ok(envelope_to_json_string(&events::make_room_migrate(
        &node,
        &room,
        new_room_pub,
        reason,
    )?))
}

pub fn wrap_commit(envelope_json: &str, room_json: &str, tc: u64) -> Result<String> {
    let envelope = parse_envelope_json(envelope_json)?;
    let room = parse_room_identity(room_json)?;
    Ok(envelope_to_json_string(&wrap_commit_envelope(
        envelope, &room, tc,
    )?))
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
        "room": inner.d.r,
        "to": inner.d.to,
        "tc": envelope.d.tc,
        "content": inner.d.c,
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

fn cipher_from_value(value: &Value) -> Result<PrivateCiphertextV2> {
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

fn cipher_from_json(cipher_json: &str) -> Result<PrivateCiphertextV2> {
    let value: Value = serde_json::from_str(cipher_json).context("invalid cipher JSON")?;
    cipher_from_value(&value)
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

    /// Build a signed private.message envelope using the cached pair key.
    pub fn make_message(
        &mut self,
        identity_json: &str,
        to: &str,
        to_enc_pub: &str,
        body: &str,
        blobs_json: Option<&str>,
    ) -> Result<String> {
        let node = parse_identity(identity_json)?;
        let blobs = optional_string_array(blobs_json, "blobs")?;
        let mut pair_key = self.cache.pair_key(&node.enc.priv_key, to_enc_pub)?;
        let result = events::make_private_message_with_pair_key(
            &node, to, to_enc_pub, &pair_key, body, blobs,
        );
        pair_key.fill(0);
        Ok(envelope_to_json_string(&result?))
    }

    /// Verify and decrypt a private.message envelope.
    pub fn open_message(&mut self, envelope_json: &str, identity_json: &str) -> Result<String> {
        let envelope = parse_envelope_json(envelope_json)?;
        verify_chain(&envelope)?;
        let node = parse_identity(identity_json)?;
        let inner = innermost(&envelope);
        if inner.d.t != "private.message" {
            bail!("envelope is not a private.message");
        }
        let cipher = cipher_from_value(
            inner
                .d
                .c
                .as_ref()
                .context("private.message missing content")?,
        )?;
        let mut pair_key = self.cache.pair_key(&node.enc.priv_key, &cipher.from)?;
        let result = decrypt_private_v2_with_key(&cipher, &node.enc.pub_key, &pair_key);
        pair_key.fill(0);
        Ok(result?.to_string())
    }
}

// ── Request helpers ────────────────────────────────────────────────────

pub fn request_events_sync(
    identity_json: &str,
    rooms_json: &str,
    after: Option<u64>,
    limit: Option<u64>,
) -> Result<String> {
    let rooms = string_array(rooms_json, "rooms")?;
    let mut content = json!({ "rooms": rooms });
    insert_opt_u64(&mut content, "after", after);
    insert_opt_u64(&mut content, "limit", limit);
    make_request_envelope(identity_json, "events.sync", content)
}

pub fn request_room_sync(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    start_tc: Option<u64>,
    end_tc: Option<u64>,
    limit: Option<u64>,
) -> Result<String> {
    let mut content = json!({ "r": room_pub, "channel": channel });
    insert_opt_u64(&mut content, "start_tc", start_tc);
    insert_opt_u64(&mut content, "end_tc", end_tc);
    insert_opt_u64(&mut content, "limit", limit);
    make_request_envelope(identity_json, "room.sync", content)
}

pub fn request_room_compare(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    samples_json: &str,
) -> Result<String> {
    let samples: Vec<Value> =
        serde_json::from_str(samples_json).context("samples must be a JSON array")?;
    make_request_envelope(
        identity_json,
        "room.compare",
        json!({ "r": room_pub, "channel": channel, "samples": samples }),
    )
}

pub fn request_pool_exchange(identity_json: &str, payload_json: &str) -> Result<String> {
    let payload: Value =
        serde_json::from_str(payload_json).context("invalid pool.exchange payload JSON")?;
    if !payload.is_object() {
        bail!("pool.exchange payload must be a JSON object");
    }
    make_request_envelope(identity_json, "pool.exchange", payload)
}

pub fn request_node_peers(identity_json: &str, limit: Option<u64>) -> Result<String> {
    let mut content = json!({});
    insert_opt_u64(&mut content, "limit", limit);
    make_request_envelope(identity_json, "node.peers", content)
}

pub fn request_node_rooms(identity_json: &str, target: &str, limit: Option<u64>) -> Result<String> {
    let mut content = json!({ "node": target });
    insert_opt_u64(&mut content, "limit", limit);
    make_request_envelope(identity_json, "node.rooms", content)
}

pub fn request_system_fetch(identity_json: &str, datum_ids_json: &str) -> Result<String> {
    let ids = string_array(datum_ids_json, "datum_ids")?;
    make_request_envelope(identity_json, "system.fetch", json!({ "datum_ids": ids }))
}

pub fn request_blob_fetch(
    identity_json: &str,
    blob_id: &str,
    piece_index: Option<u64>,
) -> Result<String> {
    let mut content = json!({ "blob_id": blob_id });
    insert_opt_u64(&mut content, "piece_index", piece_index);
    make_request_envelope(identity_json, "blob.fetch", content)
}

pub fn request_room_summary(identity_json: &str, rooms_json: &str) -> Result<String> {
    let rooms = string_array(rooms_json, "rooms")?;
    make_request_envelope(identity_json, "room.summary", json!({ "rooms": rooms }))
}

/// Rebuild membership for `room_pub` from a verified, canonically ordered
/// channel `-2` event list.
///
/// Returns `{"members":[...],"banned":[...]}` with sorted public keys.
pub fn build_membership(room_pub: &str, envelopes_json: &str) -> Result<String> {
    let envelopes = parse_envelopes_array(envelopes_json)?;
    let refs: Vec<&Envelope> = envelopes.iter().collect();
    let verdicts = verify_chains_batch(&refs);
    for (index, verdict) in verdicts.into_iter().enumerate() {
        verdict.with_context(|| format!("envelope {index} failed verification"))?;
    }

    let room_events = envelopes
        .iter()
        .filter(|envelope| innermost(envelope).d.r.as_deref() == Some(room_pub));
    let state = membership::from_events(room_events);
    let mut members: Vec<String> = state.members.into_iter().collect();
    let mut banned: Vec<String> = state.banned.into_iter().collect();
    members.sort();
    banned.sort();
    Ok(json!({ "members": members, "banned": banned }).to_string())
}

/// Parsed room rules with the same evaluation used by the native relay.
pub struct RoomRules {
    rules: RuleSet,
}

impl RoomRules {
    pub fn new(rules_json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(rules_json).context("invalid room rules JSON")?;
        Ok(Self {
            rules: RuleSet::from_json(&value),
        })
    }

    fn member_refs(members_json: &str) -> Result<Vec<String>> {
        string_array(members_json, "members")
    }

    pub fn can_join(&self, node: &str, members_json: &str) -> Result<bool> {
        let members = Self::member_refs(members_json)?;
        let refs: Vec<&str> = members.iter().map(String::as_str).collect();
        Ok(self.rules.can_join_node(node, &refs))
    }

    pub fn can_post(&self, node: &str, members_json: &str) -> Result<bool> {
        let members = Self::member_refs(members_json)?;
        let refs: Vec<&str> = members.iter().map(String::as_str).collect();
        Ok(self.rules.can_post_node(node, &refs))
    }

    pub fn required_join_pow(&self) -> Option<u32> {
        self.rules.required_join_pow()
    }

    pub fn required_post_pow(&self) -> Option<u32> {
        self.rules.required_post_pow()
    }

    pub fn max_events_per_hour(&self) -> Option<u32> {
        self.rules.max_events_per_hour()
    }

    pub fn is_invite_only(&self) -> bool {
        self.rules.is_invite_only()
    }

    pub fn retention_days(&self) -> Option<u64> {
        self.rules.retention_days()
    }

    pub fn uncommitted_ttl_ms(&self) -> Option<u64> {
        self.rules.uncommitted_ttl_ms()
    }
}

// ── Discovery, PoW, hashing, validation ───────────────────────────────

pub fn encode_uri(envelopes_json: &str) -> Result<String> {
    discovery::encode_uri(&parse_envelopes_array(envelopes_json)?)
}

pub fn encode_file(envelopes_json: &str) -> Result<String> {
    discovery::encode_file(&parse_envelopes_array(envelopes_json)?)
}

pub fn decode_discovery(input: &str) -> Result<String> {
    let envelopes = discovery::decode(input)?;
    serde_json::to_string(&envelopes).context("failed to serialize decoded envelopes")
}

pub fn verify_pow_result(envelope_json: &str, required_bits: u32) -> String {
    let result = parse_envelope_json(envelope_json)
        .and_then(|envelope| pow::verify_pow(&innermost(&envelope).d, required_bits));
    verdict_json(result.err())
}

/// Add a valid PoW nonce to a depth-1 envelope and re-sign it.
pub fn solve_pow(envelope_json: &str, sig_seed: &str, required_bits: u32) -> Result<String> {
    let mut envelope = parse_envelope_json(envelope_json)?;
    if depth(&envelope) != 1 {
        bail!("solve_pow expects a depth-1 envelope");
    }
    // Mutating d invalidates the parsed canonical cache.
    envelope.d.cached_canonical = OnceLock::new();
    envelope.d.verified_layer = OnceLock::new();
    pow::find_pow_nonce(&mut envelope.d, required_bits);
    let signed = sign_envelope(envelope.d, sig_seed)?;
    Ok(envelope_to_json_string(&signed))
}

pub fn validate_event_result(envelope_json: &str) -> String {
    let result = parse_envelope_json(envelope_json)
        .and_then(|envelope| events::validate_event_content(&envelope));
    verdict_json(result.err())
}

pub fn sha256_hex(data: &[u8]) -> String {
    toloo_core::hex::encode(sha256(data))
}

pub fn base64url_encode(data: &[u8]) -> String {
    base64url::encode(data)
}

pub fn base64url_decode(encoded: &str) -> Result<Vec<u8>> {
    base64url::decode(encoded)
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

    fn field<'a>(value: &'a Value, key: &str) -> &'a str {
        value[key]
            .as_str()
            .unwrap_or_else(|| panic!("missing {key}"))
    }

    #[test]
    fn room_events_requests_and_commits() {
        let identity = generate_identity().unwrap();
        let node: Value = serde_json::from_str(&identity).unwrap();
        let room = generate_room_identity().unwrap();
        let room_id: Value = serde_json::from_str(&room).unwrap();
        let room_pub = field(&room_id, "sigPub").to_owned();

        let created_pair: Value = serde_json::from_str(
            &make_room_create(&identity, &room, Some("Test room"), None).unwrap(),
        )
        .unwrap();
        let created = created_pair["created"].to_string();
        let committed = created_pair["committed"].to_string();
        assert_eq!(verify_chain_result(&created), r#"{"ok":true}"#);
        assert_eq!(verify_chain_result(&committed), r#"{"ok":true}"#);
        let info: Value = serde_json::from_str(&envelope_info(&committed).unwrap()).unwrap();
        assert_eq!(info["type"], "room.create");
        assert_eq!(info["depth"], 2);
        assert_eq!(info["signer"], field(&node, "sigPub"));

        let join = make_room_join(&identity, &room_pub, None).unwrap();
        assert_eq!(validate_event_result(&join), r#"{"ok":true}"#);
        let committed_join = wrap_commit(&join, &room, 9_000_000_000_000).unwrap();
        assert_eq!(verify_chain_result(&committed_join), r#"{"ok":true}"#);

        let message = make_room_message(&identity, &room_pub, 1, "hello", None).unwrap();
        assert_eq!(verify_chain_result(&message), r#"{"ok":true}"#);
        let info: Value = serde_json::from_str(&envelope_info(&message).unwrap()).unwrap();
        assert_eq!(info["type"], "room.message");
        assert_eq!(info["channel"], 1);

        let sync =
            request_events_sync(&identity, r#"["room-a","room-b"]"#, Some(10), Some(50)).unwrap();
        assert_eq!(verify_chain_result(&sync), r#"{"ok":true}"#);
        let info: Value = serde_json::from_str(&envelope_info(&sync).unwrap()).unwrap();
        assert_eq!(info["type"], "events.sync");

        let room_sync =
            request_room_sync(&identity, &room_pub, -1, Some(0), None, Some(100)).unwrap();
        let info: Value = serde_json::from_str(&envelope_info(&room_sync).unwrap()).unwrap();
        assert_eq!(info["type"], "room.sync");

        let exchange = request_pool_exchange(
            &identity,
            r#"{"offer":[],"want":{"rooms":["room-a"],"limit":5}}"#,
        )
        .unwrap();
        let info: Value = serde_json::from_str(&envelope_info(&exchange).unwrap()).unwrap();
        assert_eq!(info["type"], "pool.exchange");

        let blob = request_blob_fetch(&identity, "blob-id", Some(2)).unwrap();
        let info: Value = serde_json::from_str(&envelope_info(&blob).unwrap()).unwrap();
        assert_eq!(info["type"], "blob.fetch");
    }

    #[test]
    fn cached_private_envelope_round_trip() {
        let alice = generate_identity().unwrap();
        let bob = generate_identity().unwrap();
        let bob_id: Value = serde_json::from_str(&bob).unwrap();
        let to = field(&bob_id, "sigPub");
        let to_enc = field(&bob_id, "encPub");

        let mut alice_cache = CachedPrivate::new(8);
        let mut bob_cache = CachedPrivate::new(8);

        let envelope = alice_cache
            .make_message(&alice, to, to_enc, "secret body", Some(r#"["blob-eid"]"#))
            .unwrap();
        assert_eq!(verify_chain_result(&envelope), r#"{"ok":true}"#);
        assert_eq!(validate_event_result(&envelope), r#"{"ok":true}"#);

        let plaintext = bob_cache.open_message(&envelope, &bob).unwrap();
        let content: Value = serde_json::from_str(&plaintext).unwrap();
        assert_eq!(content["body"], "secret body");
        assert_eq!(content["blobs"][0], "blob-eid");
    }

    #[test]
    fn discovery_round_trip() {
        let identity = generate_identity().unwrap();
        let meta = make_node_meta(
            &identity,
            Some(r#"[{"proto":"wss","host":"relay.example","port":443,"path":"/toloo"}]"#),
        )
        .unwrap();
        assert_eq!(verify_chain_result(&meta), r#"{"ok":true}"#);
        let envelopes = json!([serde_json::from_str::<Value>(&meta).unwrap()]).to_string();

        let uri = encode_uri(&envelopes).unwrap();
        assert!(uri.starts_with("toloo://"));
        let decoded: Value = serde_json::from_str(&decode_discovery(&uri).unwrap()).unwrap();
        assert_eq!(decoded.as_array().unwrap().len(), 1);
        assert_eq!(decoded[0]["d"]["t"], "node.meta");

        let file = encode_file(&envelopes).unwrap();
        let decoded: Value = serde_json::from_str(&decode_discovery(&file).unwrap()).unwrap();
        assert_eq!(decoded.as_array().unwrap().len(), 1);
    }

    #[test]
    fn proof_of_work_solve_and_verify() {
        let identity = generate_identity().unwrap();
        let node: Value = serde_json::from_str(&identity).unwrap();
        let room = generate_room_identity().unwrap();
        let room_id: Value = serde_json::from_str(&room).unwrap();
        let join = make_room_join(&identity, field(&room_id, "sigPub"), None).unwrap();

        let solved = solve_pow(&join, field(&node, "sigSeed"), 4).unwrap();
        assert_eq!(verify_pow_result(&solved, 4), r#"{"ok":true}"#);
        assert_eq!(verify_chain_result(&solved), r#"{"ok":true}"#);
        let info: Value = serde_json::from_str(&envelope_info(&solved).unwrap()).unwrap();
        assert_eq!(info["type"], "room.join");
    }

    #[test]
    fn hash_and_base64_helpers() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let encoded = base64url_encode(b"hello");
        assert_eq!(encoded, "aGVsbG8");
        assert_eq!(base64url_decode(&encoded).unwrap(), b"hello");
    }

    #[test]
    fn membership_and_rules() {
        let owner = generate_identity().unwrap();
        let owner_id: Value = serde_json::from_str(&owner).unwrap();
        let bob = generate_identity().unwrap();
        let bob_id: Value = serde_json::from_str(&bob).unwrap();
        let bob_pub = field(&bob_id, "sigPub").to_owned();
        let room = generate_room_identity().unwrap();
        let room_id: Value = serde_json::from_str(&room).unwrap();
        let room_pub = field(&room_id, "sigPub").to_owned();

        let join = wrap_commit(
            &make_room_join(&bob, &room_pub, None).unwrap(),
            &room,
            9_000_000_000_000,
        )
        .unwrap();
        let ban = wrap_commit(
            &make_room_ban(&owner, &room_pub, &bob_pub, Some("spam")).unwrap(),
            &room,
            9_000_000_000_001,
        )
        .unwrap();
        let input = json!([
            serde_json::from_str::<Value>(&join).unwrap(),
            serde_json::from_str::<Value>(&ban).unwrap(),
        ])
        .to_string();

        let state: Value =
            serde_json::from_str(&build_membership(&room_pub, &input).unwrap()).unwrap();
        assert!(state["members"].as_array().unwrap().is_empty());
        assert_eq!(state["banned"][0], bob_pub);

        let unban = wrap_commit(
            &make_room_unban(&owner, &room_pub, &bob_pub).unwrap(),
            &room,
            9_000_000_000_002,
        )
        .unwrap();
        let rejoin = wrap_commit(
            &make_room_join(&bob, &room_pub, None).unwrap(),
            &room,
            9_000_000_000_003,
        )
        .unwrap();
        let input = json!([
            serde_json::from_str::<Value>(&join).unwrap(),
            serde_json::from_str::<Value>(&ban).unwrap(),
            serde_json::from_str::<Value>(&unban).unwrap(),
            serde_json::from_str::<Value>(&rejoin).unwrap(),
        ])
        .to_string();
        let state: Value =
            serde_json::from_str(&build_membership(&room_pub, &input).unwrap()).unwrap();
        assert_eq!(state["members"][0], bob_pub);
        assert!(state["banned"].as_array().unwrap().is_empty());

        let rules = RoomRules::new(
            r#"[{"t":"join","allow":"*"},{"t":"post","allow":"members"},{"t":"join_pow","difficulty":8},{"t":"retention","days":30}]"#,
        )
        .unwrap();
        let owner_pub = field(&owner_id, "sigPub");
        let members = json!([owner_pub]).to_string();
        assert!(rules.can_join(&bob_pub, "[]").unwrap());
        assert!(!rules.can_post(&bob_pub, "[]").unwrap());
        assert!(rules.can_post(owner_pub, &members).unwrap());
        assert_eq!(rules.required_join_pow(), Some(8));
        assert_eq!(rules.retention_days(), Some(30));
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
