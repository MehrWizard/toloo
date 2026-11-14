//! Thin `wasm-bindgen` layer over the JSON contract.

use wasm_bindgen::prelude::*;

use crate::contract::{self, CachedPrivate};

fn js_err(error: anyhow::Error) -> JsError {
    JsError::new(&format!("{error:#}"))
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn opt_u64(value: Option<f64>) -> Result<Option<u64>, JsError> {
    match value {
        None => Ok(None),
        Some(v) if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= MAX_SAFE_INTEGER => {
            Ok(Some(v as u64))
        }
        Some(_) => Err(JsError::new("expected a non-negative safe integer")),
    }
}

fn req_u64(value: f64) -> Result<u64, JsError> {
    opt_u64(Some(value))?.ok_or_else(|| JsError::new("expected a timestamp"))
}

#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    contract::core_version().to_owned()
}

#[wasm_bindgen(js_name = avatarSpec)]
pub fn avatar_spec(public_key: &str) -> Result<String, JsError> {
    contract::avatar_spec(public_key).map_err(js_err)
}

#[wasm_bindgen(js_name = generateIdentity)]
pub fn generate_identity() -> Result<String, JsError> {
    contract::generate_identity().map_err(js_err)
}

#[wasm_bindgen(js_name = importIdentity)]
pub fn import_identity(sig_seed: &str, enc_priv: &str) -> Result<String, JsError> {
    contract::import_identity(sig_seed, enc_priv).map_err(js_err)
}

#[wasm_bindgen(js_name = generateRoomIdentity)]
pub fn generate_room_identity() -> Result<String, JsError> {
    contract::generate_room_identity().map_err(js_err)
}

#[wasm_bindgen(js_name = importRoomIdentity)]
pub fn import_room_identity(sig_seed: &str) -> Result<String, JsError> {
    contract::import_room_identity(sig_seed).map_err(js_err)
}

#[wasm_bindgen(js_name = makeNodeEnvelope)]
pub fn make_node_envelope(
    kind: &str,
    datum_json: &str,
    sig_pub: &str,
    sig_seed: &str,
    extra_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_node_envelope(kind, datum_json, sig_pub, sig_seed, extra_json.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = verifyChain)]
pub fn verify_chain(envelope_json: &str) -> String {
    contract::verify_chain_result(envelope_json)
}

#[wasm_bindgen(js_name = verifyChains)]
pub fn verify_chains(envelopes_json: &str) -> Result<String, JsError> {
    contract::verify_chains_result(envelopes_json).map_err(js_err)
}

#[wasm_bindgen(js_name = envelopeInfo)]
pub fn envelope_info(envelope_json: &str) -> Result<String, JsError> {
    contract::envelope_info(envelope_json).map_err(js_err)
}

#[wasm_bindgen(js_name = canonicalize)]
pub fn canonicalize(json_str: &str) -> Result<String, JsError> {
    contract::canonicalize_json(json_str).map_err(js_err)
}

#[wasm_bindgen(js_name = privateEncrypt)]
pub fn private_encrypt(
    content_json: &str,
    sender_enc_priv: &str,
    sender_enc_pub: &str,
    recipient_enc_pub: &str,
) -> Result<String, JsError> {
    contract::private_encrypt(
        content_json,
        sender_enc_priv,
        sender_enc_pub,
        recipient_enc_pub,
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = privateDecrypt)]
pub fn private_decrypt(
    cipher_json: &str,
    recipient_enc_priv: &str,
    recipient_enc_pub: &str,
) -> Result<String, JsError> {
    contract::private_decrypt(cipher_json, recipient_enc_priv, recipient_enc_pub).map_err(js_err)
}

// ── Event builders ─────────────────────────────────────────────────────

#[wasm_bindgen(js_name = makeNodeMeta)]
pub fn make_node_meta(
    identity_json: &str,
    endpoints_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_node_meta(identity_json, endpoints_json.as_deref()).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomCreate)]
pub fn make_room_create(
    identity_json: &str,
    room_json: &str,
    name: Option<String>,
    rules_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_create(
        identity_json,
        room_json,
        name.as_deref(),
        rules_json.as_deref(),
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomAnnouncement)]
pub fn make_room_announcement(room_json: &str, body: &str) -> Result<String, JsError> {
    contract::make_room_announcement(room_json, body).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomJoin)]
pub fn make_room_join(
    identity_json: &str,
    room_pub: &str,
    nonce: Option<f64>,
) -> Result<String, JsError> {
    contract::make_room_join(identity_json, room_pub, opt_u64(nonce)?).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomLeave)]
pub fn make_room_leave(identity_json: &str, room_pub: &str) -> Result<String, JsError> {
    contract::make_room_leave(identity_json, room_pub).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomInvite)]
pub fn make_room_invite(
    identity_json: &str,
    room_pub: &str,
    invitee: &str,
) -> Result<String, JsError> {
    contract::make_room_invite(identity_json, room_pub, invitee).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomBan)]
pub fn make_room_ban(
    identity_json: &str,
    room_pub: &str,
    target: &str,
    reason: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_ban(identity_json, room_pub, target, reason.as_deref()).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomUnban)]
pub fn make_room_unban(
    identity_json: &str,
    room_pub: &str,
    target: &str,
) -> Result<String, JsError> {
    contract::make_room_unban(identity_json, room_pub, target).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomUpdate)]
pub fn make_room_update(
    identity_json: &str,
    room_json: &str,
    name: Option<String>,
    rules_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_update(
        identity_json,
        room_json,
        name.as_deref(),
        rules_json.as_deref(),
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomMessage)]
pub fn make_room_message(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    body: &str,
    blobs_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_message(
        identity_json,
        room_pub,
        channel,
        body,
        blobs_json.as_deref(),
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomReact)]
pub fn make_room_react(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
    emoji: &str,
) -> Result<String, JsError> {
    contract::make_room_react(identity_json, room_pub, channel, target_eid, emoji).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomEdit)]
pub fn make_room_edit(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
    body: &str,
) -> Result<String, JsError> {
    contract::make_room_edit(identity_json, room_pub, channel, target_eid, body).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomDelete)]
pub fn make_room_delete(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    target_eid: &str,
) -> Result<String, JsError> {
    contract::make_room_delete(identity_json, room_pub, channel, target_eid).map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomBlob)]
pub fn make_room_blob(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    manifest_json: &str,
    mime: Option<String>,
    name: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_blob(
        identity_json,
        room_pub,
        channel,
        manifest_json,
        mime.as_deref(),
        name.as_deref(),
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = makePrivateMessage)]
pub fn make_private_message(
    identity_json: &str,
    to: &str,
    to_enc_pub: &str,
    body: &str,
    blobs_json: Option<String>,
) -> Result<String, JsError> {
    contract::make_private_message(identity_json, to, to_enc_pub, body, blobs_json.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = makePrivateRead)]
pub fn make_private_read(identity_json: &str, to: &str, ids_json: &str) -> Result<String, JsError> {
    contract::make_private_read(identity_json, to, ids_json).map_err(js_err)
}

#[wasm_bindgen(js_name = makeSideFork)]
pub fn make_side_fork(
    identity_json: &str,
    room_pub: &str,
    forked_from: &str,
    reason: Option<String>,
) -> Result<String, JsError> {
    contract::make_side_fork(identity_json, room_pub, forked_from, reason.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = makeSideAttestation)]
pub fn make_side_attestation(
    identity_json: &str,
    target_node: &str,
    level: &str,
    reason: Option<String>,
) -> Result<String, JsError> {
    contract::make_side_attestation(identity_json, target_node, level, reason.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomFlag)]
pub fn make_room_flag(
    identity_json: &str,
    room_pub: &str,
    target_eid: &str,
    category: &str,
    reason: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_flag(
        identity_json,
        room_pub,
        target_eid,
        category,
        reason.as_deref(),
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = makeRoomMigrate)]
pub fn make_room_migrate(
    identity_json: &str,
    room_json: &str,
    new_room_pub: &str,
    reason: Option<String>,
) -> Result<String, JsError> {
    contract::make_room_migrate(identity_json, room_json, new_room_pub, reason.as_deref())
        .map_err(js_err)
}

#[wasm_bindgen(js_name = wrapCommit)]
pub fn wrap_commit(envelope_json: &str, room_json: &str, tc: f64) -> Result<String, JsError> {
    contract::wrap_commit(envelope_json, room_json, req_u64(tc)?).map_err(js_err)
}

#[wasm_bindgen(js_name = wrapPrivateAck)]
pub fn wrap_private_ack(envelope_json: &str, identity_json: &str) -> Result<String, JsError> {
    contract::wrap_private_ack(envelope_json, identity_json).map_err(js_err)
}

#[wasm_bindgen(js_name = validateEvent)]
pub fn validate_event(envelope_json: &str) -> String {
    contract::validate_event_result(envelope_json)
}

// ── Requests ───────────────────────────────────────────────────────────

#[wasm_bindgen(js_name = requestEventsSync)]
pub fn request_events_sync(
    identity_json: &str,
    rooms_json: &str,
    after: Option<f64>,
    limit: Option<f64>,
) -> Result<String, JsError> {
    contract::request_events_sync(identity_json, rooms_json, opt_u64(after)?, opt_u64(limit)?)
        .map_err(js_err)
}

#[wasm_bindgen(js_name = requestRoomSync)]
pub fn request_room_sync(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    start_tc: Option<f64>,
    end_tc: Option<f64>,
    limit: Option<f64>,
) -> Result<String, JsError> {
    contract::request_room_sync(
        identity_json,
        room_pub,
        channel,
        opt_u64(start_tc)?,
        opt_u64(end_tc)?,
        opt_u64(limit)?,
    )
    .map_err(js_err)
}

#[wasm_bindgen(js_name = requestRoomCompare)]
pub fn request_room_compare(
    identity_json: &str,
    room_pub: &str,
    channel: i32,
    samples_json: &str,
) -> Result<String, JsError> {
    contract::request_room_compare(identity_json, room_pub, channel, samples_json).map_err(js_err)
}

#[wasm_bindgen(js_name = requestPoolExchange)]
pub fn request_pool_exchange(identity_json: &str, payload_json: &str) -> Result<String, JsError> {
    contract::request_pool_exchange(identity_json, payload_json).map_err(js_err)
}

#[wasm_bindgen(js_name = requestNodePeers)]
pub fn request_node_peers(identity_json: &str, limit: Option<f64>) -> Result<String, JsError> {
    contract::request_node_peers(identity_json, opt_u64(limit)?).map_err(js_err)
}

#[wasm_bindgen(js_name = requestNodeRooms)]
pub fn request_node_rooms(
    identity_json: &str,
    target: &str,
    limit: Option<f64>,
) -> Result<String, JsError> {
    contract::request_node_rooms(identity_json, target, opt_u64(limit)?).map_err(js_err)
}

#[wasm_bindgen(js_name = requestSystemFetch)]
pub fn request_system_fetch(identity_json: &str, datum_ids_json: &str) -> Result<String, JsError> {
    contract::request_system_fetch(identity_json, datum_ids_json).map_err(js_err)
}

#[wasm_bindgen(js_name = requestBlobFetch)]
pub fn request_blob_fetch(
    identity_json: &str,
    blob_id: &str,
    piece_index: Option<f64>,
) -> Result<String, JsError> {
    contract::request_blob_fetch(identity_json, blob_id, opt_u64(piece_index)?).map_err(js_err)
}

#[wasm_bindgen(js_name = requestRoomSummary)]
pub fn request_room_summary(identity_json: &str, rooms_json: &str) -> Result<String, JsError> {
    contract::request_room_summary(identity_json, rooms_json).map_err(js_err)
}

#[wasm_bindgen(js_name = buildMembership)]
pub fn build_membership(room_pub: &str, envelopes_json: &str) -> Result<String, JsError> {
    contract::build_membership(room_pub, envelopes_json).map_err(js_err)
}

// ── Discovery, PoW, hashing ────────────────────────────────────────────

#[wasm_bindgen(js_name = encodeUri)]
pub fn encode_uri(envelopes_json: &str) -> Result<String, JsError> {
    contract::encode_uri(envelopes_json).map_err(js_err)
}

#[wasm_bindgen(js_name = encodeFile)]
pub fn encode_file(envelopes_json: &str) -> Result<String, JsError> {
    contract::encode_file(envelopes_json).map_err(js_err)
}

#[wasm_bindgen(js_name = decodeDiscovery)]
pub fn decode_discovery(input: &str) -> Result<String, JsError> {
    contract::decode_discovery(input).map_err(js_err)
}

#[wasm_bindgen(js_name = verifyPow)]
pub fn verify_pow(envelope_json: &str, required_bits: u32) -> String {
    contract::verify_pow_result(envelope_json, required_bits)
}

#[wasm_bindgen(js_name = solvePow)]
pub fn solve_pow(
    envelope_json: &str,
    sig_seed: &str,
    required_bits: u32,
) -> Result<String, JsError> {
    contract::solve_pow(envelope_json, sig_seed, required_bits).map_err(js_err)
}

#[wasm_bindgen(js_name = sha256Hex)]
pub fn sha256_hex(data: &[u8]) -> String {
    contract::sha256_hex(data)
}

#[wasm_bindgen(js_name = base64UrlEncode)]
pub fn base64url_encode(data: &[u8]) -> String {
    contract::base64url_encode(data)
}

#[wasm_bindgen(js_name = base64UrlDecode)]
pub fn base64url_decode(encoded: &str) -> Result<Vec<u8>, JsError> {
    contract::base64url_decode(encoded).map_err(js_err)
}

/// Room rules evaluated with the same logic as the relay.
#[wasm_bindgen(js_name = RoomRules)]
pub struct RoomRules {
    inner: contract::RoomRules,
}

#[wasm_bindgen(js_class = RoomRules)]
impl RoomRules {
    #[wasm_bindgen(constructor)]
    pub fn new(rules_json: &str) -> Result<RoomRules, JsError> {
        contract::RoomRules::new(rules_json)
            .map(|inner| RoomRules { inner })
            .map_err(js_err)
    }

    #[wasm_bindgen(js_name = canJoin)]
    pub fn can_join(&self, node: &str, members_json: &str) -> Result<bool, JsError> {
        self.inner.can_join(node, members_json).map_err(js_err)
    }

    #[wasm_bindgen(js_name = canPost)]
    pub fn can_post(&self, node: &str, members_json: &str) -> Result<bool, JsError> {
        self.inner.can_post(node, members_json).map_err(js_err)
    }

    #[wasm_bindgen(js_name = requiredJoinPow)]
    pub fn required_join_pow(&self) -> Option<u32> {
        self.inner.required_join_pow()
    }

    #[wasm_bindgen(js_name = requiredPostPow)]
    pub fn required_post_pow(&self) -> Option<u32> {
        self.inner.required_post_pow()
    }

    #[wasm_bindgen(js_name = maxEventsPerHour)]
    pub fn max_events_per_hour(&self) -> Option<u32> {
        self.inner.max_events_per_hour()
    }

    #[wasm_bindgen(js_name = isInviteOnly)]
    pub fn is_invite_only(&self) -> bool {
        self.inner.is_invite_only()
    }

    #[wasm_bindgen(js_name = retentionDays)]
    pub fn retention_days(&self) -> Option<f64> {
        self.inner.retention_days().map(|days| days as f64)
    }

    #[wasm_bindgen(js_name = uncommittedTtlMs)]
    pub fn uncommitted_ttl_ms(&self) -> Option<f64> {
        self.inner.uncommitted_ttl_ms().map(|ttl| ttl as f64)
    }
}

/// Pairwise-key cache for repeated private messages with the same peers.
#[wasm_bindgen(js_name = PairKeyCache)]
pub struct PairKeyCache {
    inner: CachedPrivate,
}

#[wasm_bindgen(js_class = PairKeyCache)]
impl PairKeyCache {
    #[wasm_bindgen(constructor)]
    pub fn new(capacity: usize) -> PairKeyCache {
        PairKeyCache {
            inner: CachedPrivate::new(capacity),
        }
    }

    pub fn encrypt(
        &mut self,
        content_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
        peer_enc_pub: &str,
    ) -> Result<String, JsError> {
        self.inner
            .encrypt(content_json, own_enc_priv, own_enc_pub, peer_enc_pub)
            .map_err(js_err)
    }

    pub fn decrypt(
        &mut self,
        cipher_json: &str,
        own_enc_priv: &str,
        own_enc_pub: &str,
    ) -> Result<String, JsError> {
        self.inner
            .decrypt(cipher_json, own_enc_priv, own_enc_pub)
            .map_err(js_err)
    }

    #[wasm_bindgen(js_name = makeMessage)]
    pub fn make_message(
        &mut self,
        identity_json: &str,
        to: &str,
        to_enc_pub: &str,
        body: &str,
        blobs_json: Option<String>,
    ) -> Result<String, JsError> {
        self.inner
            .make_message(identity_json, to, to_enc_pub, body, blobs_json.as_deref())
            .map_err(js_err)
    }

    #[wasm_bindgen(js_name = openMessage)]
    pub fn open_message(
        &mut self,
        envelope_json: &str,
        identity_json: &str,
    ) -> Result<String, JsError> {
        self.inner
            .open_message(envelope_json, identity_json)
            .map_err(js_err)
    }

    #[wasm_bindgen(js_name = openSentMessage)]
    pub fn open_sent_message(
        &mut self,
        envelope_json: &str,
        identity_json: &str,
        recipient_enc_pub: &str,
    ) -> Result<String, JsError> {
        self.inner
            .open_sent_message(envelope_json, identity_json, recipient_enc_pub)
            .map_err(js_err)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}
