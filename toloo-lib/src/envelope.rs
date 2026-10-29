use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::Signature;
use serde_json::Value;
use simd_json;

use crate::base64url;
use crate::canonical::{canonical, canonical_bytes, canonical_datum_bytes};
use crate::crypto::{ed25519_sign, ed25519_verify_fast, get_verifying_key};
use crate::types::{DatumBody, Envelope, LocalNode, LocalRoom};

const VERSION: &str = "0.2";
const P_SIGNATURE_LEN_B64: usize = 86;
const N_PUBKEY_LEN_B64: usize = 43;
const SIGNATURE_BYTES: usize = 64;
const PUBKEY_BYTES: usize = 32;
const MAX_D_BYTES: usize = 16_384;
const MAX_ENV_BYTES: usize = 49_152;
const MAX_DEPTH: u8 = 3;
const MAX_T_LEN: usize = 64;
/// JavaScript safe integer maximum (2^53 - 1). All uint64 fields in `d` MUST
/// not exceed this so they round-trip correctly through JS `number` (spec §2.12).
const JS_SAFE_MAX: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Default)]
pub struct DatumBodyExtra {
    pub r: Option<String>,
    pub to: Option<String>,
    pub tc: Option<u64>,
    pub exp: Option<u64>,
    pub nonce: Option<u64>,
}

/// Signable payload bytes for a datum: utf8(canonical(d)).
/// Uses cached canonical for signing (trusted path).
pub fn signable_bytes(d: &DatumBody) -> Vec<u8> {
    canonical_datum_bytes(d).expect("DatumBody canonical serialization should not fail")
}

/// Signable payload bytes for verification: utf8(canonical(d)).
/// Always recomputes canonical bytes to detect tampering (untrusted path).
pub fn signable_bytes_verify(d: &DatumBody) -> Vec<u8> {
    canonical(d).expect("DatumBody canonical serialization should not fail").into_bytes()
}

/// Sign a datum and produce an envelope.
pub fn sign_envelope(d: DatumBody, signer_seed: &str) -> Result<Envelope> {
    let signer_seed_bytes =
        base64url::decode(signer_seed).context("invalid signer seed base64url")?;
    if signer_seed_bytes.len() != PUBKEY_BYTES {
        bail!("signer seed must decode to {PUBKEY_BYTES} bytes");
    }

    let signature = ed25519_sign(&signable_bytes(&d), &signer_seed_bytes)?;
    let p = base64url::encode(&signature);

    if p.len() != P_SIGNATURE_LEN_B64 {
        bail!("signature must be {P_SIGNATURE_LEN_B64} base64url chars");
    }

    Ok(Envelope { d, p, cached_sig: OnceLock::new() })
}

/// Construct and sign a depth-1 envelope from event content.
pub fn make_envelope(
    t: &str,
    content: Option<Value>,
    node: &LocalNode,
    extra: Option<DatumBodyExtra>,
) -> Result<Envelope> {
    if t.is_empty() {
        bail!("event type must be non-empty");
    }
    let extra = extra.unwrap_or_default();
    let d = DatumBody {
        n: node.sig.pub_key.clone(),
        v: VERSION.to_owned(),
        t: t.to_owned(),
        ts: now_ms()?,
        r: extra.r,
        to: extra.to,
        c: content,
        tc: extra.tc,
        exp: extra.exp,
        nonce: extra.nonce,
        ..DatumBody::default()
    };
    sign_envelope(d, &node.sig.priv_key)
}

/// Wrap a depth-1 envelope in a depth-2 commit envelope signed by room key.
pub fn wrap_commit(inner: Envelope, room: &LocalRoom, tc: u64) -> Result<Envelope> {
    if depth(&inner) != 1 {
        bail!("wrap_commit expects a depth-1 envelope");
    }
    if tc < inner.d.ts {
        bail!("commit timestamp must be >= inner authored timestamp");
    }

    let outer = DatumBody {
        n: room.sig.pub_key.clone(),
        v: VERSION.to_owned(),
        t: "commit".to_owned(),
        ts: tc,
        env: Some(Box::new(inner)),
        tc: Some(tc),
        ..DatumBody::default()
    };
    sign_envelope(outer, &room.sig.priv_key)
}

/// Wrap a depth-2 commit envelope in a depth-3 ack envelope signed by the original sender.
///
/// The sender wraps the relay's commit to confirm receipt and signal relays to stop
/// active forwarding (depth-3 is terminal per spec §2.7.2 and §6.2.3).
pub fn wrap_ack(commit: Envelope, node: &LocalNode) -> Result<Envelope> {
    if depth(&commit) != 2 {
        bail!("wrap_ack expects a depth-2 envelope");
    }
    let tc = commit
        .d
        .tc
        .ok_or_else(|| anyhow::anyhow!("wrap_ack: commit missing tc"))?;
    let outer = DatumBody {
        n: node.sig.pub_key.clone(),
        v: VERSION.to_owned(),
        t: "ack".to_owned(),
        ts: now_ms()?,
        r: None,
        to: None,
        c: None,
        env: Some(Box::new(commit)),
        tc: Some(tc),
        exp: None,
        ..DatumBody::default()
    };
    sign_envelope(outer, &node.sig.priv_key)
}

/// Follow nested `d.env` pointers to the deepest envelope.
pub fn innermost(env: &Envelope) -> &Envelope {
    let mut current = env;
    while let Some(inner) = current.d.env.as_deref() {
        current = inner;
    }
    current
}

/// Return nesting depth (1..=3 in valid structures).
pub fn depth(env: &Envelope) -> u8 {
    let mut current = env;
    let mut d = 1u8;
    while let Some(inner) = current.d.env.as_deref() {
        d = d.saturating_add(1);
        current = inner;
    }
    d
}

/// Verify one envelope layer signature only.
pub fn verify_single(env: &Envelope) -> Result<()> {
    let payload = signable_bytes_verify(&env.d);

    // Cached signature parse: decode base64url once, reuse on subsequent calls.
    let signature: &Signature = if let Some(cached) = env.cached_sig.get() {
        cached
    } else {
        let sig_bytes = base64url::decode(&env.p).context("invalid envelope signature encoding")?;
        if sig_bytes.len() != SIGNATURE_BYTES {
            bail!("signature must decode to {SIGNATURE_BYTES} bytes");
        }
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| anyhow!("signature must be {SIGNATURE_BYTES} bytes"))?;
        let sig = Signature::from_bytes(&sig_arr);
        // Best-effort cache; if another thread already set it, that's fine.
        let _ = env.cached_sig.set(sig);
        env.cached_sig.get().unwrap()
    };

    // env.d.n is already base64url-encoded; pass directly to use cache.
    if env.d.n.len() != N_PUBKEY_LEN_B64 {
        bail!("signer pubkey must be {N_PUBKEY_LEN_B64} chars");
    }

    let verifying = get_verifying_key(&env.d.n)?;
    ed25519_verify_fast(&payload, signature, &verifying)
        .context("signature verification failed")
}

/// Verify all signatures and signer alternation across depth.
/// Uses cached verification for commit/ack layers when available.
pub fn verify_chain(env: &Envelope) -> Result<()> {
    // Fast path: depth-1 envelope (no nesting) — skip Vec allocation.
    if env.d.env.is_none() {
        return verify_single(env);
    }

    let chain = chain_outer_to_inner(env);
    if chain.len() > MAX_DEPTH as usize {
        bail!("envelope depth exceeds maximum ({MAX_DEPTH})");
    }

    let inner = chain.last().expect("chain always has at least one layer");

    // For depth >= 2, check if inner commit layer is already verified
    if chain.len() >= 2 {
        let commit_layer = &chain[chain.len() - 2]; // Depth-2 layer (commit)
        if commit_layer.d.verified_layer.get().is_some() {
            // Commit layer already verified - only verify outer layers
            for layer in &chain[..chain.len() - 1] {
                verify_single(layer)?;
            }
        } else {
            // Full verification needed
            for layer in &chain {
                verify_single(layer)?;
            }
            // Cache the commit layer verification result
            let _ = commit_layer.d.verified_layer.set(true);
        }
    } else {
        // Depth 1 - just verify the single layer
        verify_single(&chain[0])?;
    }

    // Signer alternation checks
    match chain.len() {
        1 => Ok(()),
        2 => {
            let expected = receiver_of(&inner.d).ok_or_else(|| {
                anyhow!("depth-2 envelope requires inner receiver (d.r or d.to) for signer check")
            })?;
            if chain[0].d.n != expected {
                bail!("depth-2 outer signer does not match expected receiver");
            }
            // Cache the commit layer verification result
            let _ = chain[0].d.verified_layer.set(true);
            Ok(())
        }
        3 => {
            let expected_receiver = receiver_of(&inner.d).ok_or_else(|| {
                anyhow!("depth-3 envelope requires inner receiver (d.r or d.to) for signer check")
            })?;
            if chain[1].d.n != expected_receiver {
                bail!("depth-3 middle signer does not match expected receiver");
            }
            if chain[0].d.n != inner.d.n {
                bail!("depth-3 outer signer must match innermost author");
            }
            // Cache the ack layer verification result
            let _ = chain[0].d.verified_layer.set(true);
            Ok(())
        }
        _ => bail!("unsupported envelope depth"),
    }
}

/// Parse and validate an envelope from a raw JSON value.
///
/// This is the correct entry point for validating incoming envelopes.
/// It checks the top-level `{d, p}` shape on the raw JSON — before
/// deserialization — so that extra fields are caught rather than silently
/// dropped by serde.
pub fn parse_envelope(value: serde_json::Value) -> Result<Envelope> {
    validate_envelope_shape_recursive(&value)?;
    let env: Envelope = serde_json::from_value(value).context("envelope deserialization failed")?;
    validate_structure_inner(&env, 1)?;
    Ok(env)
}

/// Zero-copy parse and validate an envelope from JSON bytes.
///
/// Uses simd-json for parsing without intermediate string allocation.
/// Takes a mutable byte slice (simd-json may reorder bytes for lazy parsing).
pub fn parse_envelope_bytes(bytes: &mut [u8]) -> Result<Envelope> {
    let value: Value = simd_json::from_slice(bytes).context("envelope JSON parse failed")?;
    parse_envelope(value)
}

/// Validate structure and field invariants on an already-deserialized envelope.
pub fn validate_structure(env: &Envelope) -> Result<()> {
    validate_structure_inner(env, 1)
}

fn validate_structure_inner(env: &Envelope, level: u8) -> Result<()> {
    if level > MAX_DEPTH {
        bail!("envelope depth exceeds maximum ({MAX_DEPTH})");
    }

    validate_signature_field(&env.p)?;
    validate_datum(&env.d)?;

    if canonical_datum_bytes(&env.d)?.len() > MAX_D_BYTES {
        bail!("datum exceeds {MAX_D_BYTES} bytes");
    }
    if canonical_bytes(env)?.len() > MAX_ENV_BYTES {
        bail!("envelope exceeds {MAX_ENV_BYTES} bytes");
    }

    if env.d.t == "commit" {
        let tc = env
            .d
            .tc
            .ok_or_else(|| anyhow!("commit envelope requires d.tc"))?;
        let inner = env
            .d
            .env
            .as_deref()
            .ok_or_else(|| anyhow!("commit envelope requires d.env"))?;
        if tc < inner.d.ts {
            bail!("commit timestamp must be >= inner authored timestamp");
        }
    }

    if let Some(inner) = env.d.env.as_deref() {
        validate_structure_inner(inner, level + 1)?;
    }
    Ok(())
}

/// Recursively verify that every envelope layer in the JSON value has
/// exactly the two top-level fields `d` and `p` (spec §2.2).
fn validate_envelope_shape_recursive(value: &serde_json::Value) -> Result<()> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("envelope must be a JSON object"))?;
    if obj.len() != 2 || !obj.contains_key("d") || !obj.contains_key("p") {
        bail!("envelope must contain exactly top-level fields {{d, p}}");
    }
    if let Some(env_val) = obj
        .get("d")
        .and_then(|d| d.as_object())
        .and_then(|d_obj| d_obj.get("env"))
    {
        validate_envelope_shape_recursive(env_val)?;
    }
    Ok(())
}

fn validate_signature_field(p: &str) -> Result<()> {
    if p.len() != P_SIGNATURE_LEN_B64 {
        bail!("p must be {P_SIGNATURE_LEN_B64} chars");
    }
    let sig = base64url::decode(p).context("p is not valid base64url")?;
    if sig.len() != SIGNATURE_BYTES {
        bail!("p must decode to {SIGNATURE_BYTES} bytes");
    }
    Ok(())
}

fn validate_datum(d: &DatumBody) -> Result<()> {
    if d.v != VERSION {
        bail!("d.v must be \"{VERSION}\"");
    }

    validate_pubkey_field(&d.n, "d.n")?;

    // d.t: 1–64 chars, alphanumeric + dot + underscore (spec §2.12)
    if d.t.is_empty() {
        bail!("d.t must be non-empty");
    }
    if d.t.len() > MAX_T_LEN {
        bail!("d.t must be at most {MAX_T_LEN} chars");
    }
    if !d
        .t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        bail!("d.t must contain only alphanumeric, dot, or underscore characters");
    }

    // d.ts: positive and within JS safe integer range (spec §2.12)
    if d.ts == 0 || d.ts > JS_SAFE_MAX {
        bail!("d.ts must be in range 1..={JS_SAFE_MAX}");
    }

    // Optional key fields must decode to 32-byte Ed25519 public keys (spec §2.12)
    if let Some(r) = &d.r {
        validate_pubkey_field(r, "d.r")?;
    }
    if let Some(to) = &d.to {
        validate_pubkey_field(to, "d.to")?;
    }

    // Optional integer fields must be within JS safe integer range (spec §2.12)
    if let Some(tc) = d.tc {
        if tc > JS_SAFE_MAX {
            bail!("d.tc exceeds JavaScript safe integer maximum (2^53-1)");
        }
    }
    if let Some(exp) = d.exp {
        if exp > JS_SAFE_MAX {
            bail!("d.exp exceeds JavaScript safe integer maximum (2^53-1)");
        }
    }
    if let Some(nonce) = d.nonce {
        if nonce > JS_SAFE_MAX {
            bail!("d.nonce exceeds JavaScript safe integer maximum (2^53-1)");
        }
    }

    Ok(())
}

/// Validates that a field is a 43-char base64url string that decodes to 32 bytes.
fn validate_pubkey_field(value: &str, label: &str) -> Result<()> {
    if value.len() != N_PUBKEY_LEN_B64 {
        bail!("{label} must be {N_PUBKEY_LEN_B64} chars");
    }
    let bytes =
        base64url::decode(value).with_context(|| format!("{label} is not valid base64url"))?;
    if bytes.len() != PUBKEY_BYTES {
        bail!("{label} must decode to {PUBKEY_BYTES} bytes");
    }
    Ok(())
}

fn receiver_of(d: &DatumBody) -> Option<&str> {
    d.r.as_deref().or(d.to.as_deref())
}

fn chain_outer_to_inner(env: &Envelope) -> Vec<&Envelope> {
    let mut chain = Vec::new();
    let mut current = env;
    chain.push(current);
    while let Some(inner) = current.d.env.as_deref() {
        chain.push(inner);
        current = inner;
    }
    chain
}

pub(crate) fn now_ms() -> Result<u64> {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("system clock is before Unix epoch"))?;
    Ok(dur.as_millis() as u64)
}

// ═══════════════════════════════════════════════════════════════════════
// Manual JSON serialization (zero-allocation, matches serde_json exactly)
// ═══════════════════════════════════════════════════════════════════════

/// Serialize an Envelope to JSON bytes without serde_json allocation.
/// Produces identical output to `serde_json::to_vec()`.
pub fn envelope_to_json_bytes(env: &Envelope) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    write_envelope(env, &mut buf).expect("envelope serialization should not fail");
    buf
}

/// Serialize an Envelope to JSON string without serde_json allocation.
pub fn envelope_to_json_string(env: &Envelope) -> String {
    String::from_utf8(envelope_to_json_bytes(env)).expect("envelope JSON is valid UTF-8")
}

fn write_envelope(env: &Envelope, buf: &mut Vec<u8>) -> Result<()> {
    // Envelope always has exactly {d, p} in that order (validated on parse)
    buf.push(b'{');
    write_datum(&env.d, buf)?;
    buf.push(b',');
    // "p" is always a string field
    let mut dummy_first = true;
    write_string_field("p", &env.p, buf, &mut dummy_first)?;
    buf.push(b'}');
    Ok(())
}

fn write_datum(d: &DatumBody, buf: &mut Vec<u8>) -> Result<()> {
    buf.push(b'"');
    buf.push(b'd');
    buf.push(b'"');
    buf.push(b':');
    buf.push(b'{');

    let mut first = true;

    // Required fields (always present)
    write_string_field("n", &d.n, buf, &mut first)?;
    write_string_field("v", &d.v, buf, &mut first)?;
    write_string_field("t", &d.t, buf, &mut first)?;
    write_number_field("ts", d.ts, buf, &mut first)?;

    // Optional fields (only if Some)
    if let Some(r) = &d.r {
        write_string_field("r", r, buf, &mut first)?;
    }
    if let Some(to) = &d.to {
        write_string_field("to", to, buf, &mut first)?;
    }
    if let Some(c) = &d.c {
        write_value_field("c", c, buf, &mut first)?;
    }
    if let Some(env) = &d.env {
        write_object_field("env", env.as_ref(), buf, &mut first)?;
    }
    if let Some(tc) = d.tc {
        write_number_field("tc", tc, buf, &mut first)?;
    }
    if let Some(exp) = d.exp {
        write_number_field("exp", exp, buf, &mut first)?;
    }
    if let Some(nonce) = d.nonce {
        write_number_field("nonce", nonce, buf, &mut first)?;
    }

    // Extra fields (flattened) - these are already JSON values
    for (k, v) in &d.extra {
        if !first {
            buf.push(b',');
        }
        first = false;
        write_json_field(k, v, buf)?;
    }

    buf.push(b'}');
    Ok(())
}

fn write_string_field(key: &str, val: &str, buf: &mut Vec<u8>, first: &mut bool) -> Result<()> {
    if !*first {
        buf.push(b',');
    }
    *first = false;
    buf.push(b'"');
    write_escaped_str(key, buf)?;
    buf.extend_from_slice(b"\":\"");
    write_escaped_str(val, buf)?;
    buf.push(b'"');
    Ok(())
}

fn write_number_field(key: &str, val: u64, buf: &mut Vec<u8>, first: &mut bool) -> Result<()> {
    if !*first {
        buf.push(b',');
    }
    *first = false;
    buf.push(b'"');
    write_escaped_str(key, buf)?;
    buf.extend_from_slice(b"\":");
    write_number(val, buf)?;
    Ok(())
}

fn write_number(val: u64, buf: &mut Vec<u8>) -> Result<()> {
    // Fast u64 to decimal string conversion
    if val == 0 {
        buf.push(b'0');
        return Ok(());
    }
    let mut digits = [0u8; 20];
    let mut n = val;
    let mut len = 0;
    while n > 0 {
        digits[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    for i in (0..len).rev() {
        buf.push(digits[i]);
    }
    Ok(())
}

fn write_object_field(key: &str, env: &Envelope, buf: &mut Vec<u8>, first: &mut bool) -> Result<()> {
    if !*first {
        buf.push(b',');
    }
    *first = false;
    buf.push(b'"');
    write_escaped_str(key, buf)?;
    buf.extend_from_slice(b"\":");
    // Write nested envelope directly to buffer
    write_envelope(env, buf)?;
    Ok(())
}

fn write_value_field(key: &str, val: &Value, buf: &mut Vec<u8>, first: &mut bool) -> Result<()> {
    if !*first {
        buf.push(b',');
    }
    *first = false;
    buf.push(b'"');
    write_escaped_str(key, buf)?;
    buf.extend_from_slice(b"\":");
    write_value(val, buf)?;
    Ok(())
}

fn write_json_field(key: &str, val: &Value, buf: &mut Vec<u8>) -> Result<()> {
    buf.push(b'"');
    write_escaped_str(key, buf)?;
    buf.extend_from_slice(b"\":");
    write_value(val, buf)?;
    Ok(())
}

fn write_escaped_str(s: &str, buf: &mut Vec<u8>) -> Result<()> {
    use std::io::Write;
    for ch in s.chars() {
        match ch {
            '"' => buf.extend_from_slice(b"\\\""),
            '\\' => buf.extend_from_slice(b"\\\\"),
            '\u{08}' => buf.extend_from_slice(b"\\b"),
            '\u{0C}' => buf.extend_from_slice(b"\\f"),
            '\n' => buf.extend_from_slice(b"\\n"),
            '\r' => buf.extend_from_slice(b"\\r"),
            '\t' => buf.extend_from_slice(b"\\t"),
            c if (c as u32) <= 0x1F => write!(buf, "\\u{:04x}", c as u32)?,
            c => {
                let mut utf8_buf = [0u8; 4];
                buf.extend_from_slice(c.encode_utf8(&mut utf8_buf).as_bytes());
            }
        }
    }
    Ok(())
}

fn write_value(v: &Value, buf: &mut Vec<u8>) -> Result<()> {
    match v {
        Value::Null => buf.extend_from_slice(b"null"),
        Value::Bool(b) => {
            if *b {
                buf.extend_from_slice(b"true");
            } else {
                buf.extend_from_slice(b"false");
            }
        }
        Value::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            buf.push(b'"');
            write_escaped_str(s, buf)?;
            buf.push(b'"');
        }
        Value::Array(arr) => {
            buf.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_value(item, buf)?;
            }
            buf.push(b']');
        }
        Value::Object(obj) => {
            buf.push(b'{');
            let mut first = true;
            for (k, v) in obj {
                if !first {
                    buf.push(b',');
                }
                first = false;
                buf.push(b'"');
                write_escaped_str(k, buf)?;
                buf.extend_from_slice(b"\":");
                write_value(v, buf)?;
            }
            buf.push(b'}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    

    use serde_json::json;

    use crate::types::{DatumBody, Envelope, LocalNode, LocalRoom};

    use super::{
        depth, innermost, make_envelope, now_ms, parse_envelope, parse_envelope_bytes, sign_envelope,
        validate_structure, verify_chain, verify_single, wrap_commit, DatumBodyExtra,
        envelope_to_json_bytes, VERSION,
    };
    fn mk_inner_room_event(author: &LocalNode, room: &LocalRoom, ts: u64) -> Envelope {
        let d = DatumBody {
            n: author.sig.pub_key.clone(),
            t: "room.message".to_owned(),
            ts,
            r: Some(room.sig.pub_key.clone()),
            c: Some(json!({"ch": 1, "body": "hello"})),
            ..DatumBody::default()
        };
        sign_envelope(d, &author.sig.priv_key).expect("inner sign should succeed")
    }

    #[test]
    fn sign_and_verify_single_layer() {
        let node = mk_node();
        let env = make_envelope("node.meta", Some(json!({"x": 1})), &node, None)
            .expect("make_envelope should succeed");
        assert_eq!(depth(&env), 1);
        assert_eq!(env.p.len(), 86);
        verify_single(&env).expect("single verify should pass");
        verify_chain(&env).expect("chain verify should pass");
    }

    #[test]
    fn wrap_commit_preserves_innermost_and_verifies_chain() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 1000);
        let commit = wrap_commit(inner, &room, 1001).expect("commit wrapping should succeed");

        assert_eq!(depth(&commit), 2);
        assert_eq!(innermost(&commit).d.t, "room.message");
        verify_chain(&commit).expect("depth-2 chain verify should pass");
        validate_structure(&commit).expect("structure should validate");
    }

    #[test]
    fn verify_chain_rejects_wrong_depth2_receiver_signer() {
        let author = mk_node();
        let room = mk_room();
        let wrong_receiver = mk_node();
        let inner = mk_inner_room_event(&author, &room, 2000);

        let outer_d = DatumBody {
            n: wrong_receiver.sig.pub_key.clone(),
            t: "commit".to_owned(),
            ts: 2001,
            r: Some(room.sig.pub_key.clone()),
            env: Some(Box::new(inner)),
            tc: Some(2001),
            ..DatumBody::default()
        };
        let outer = sign_envelope(outer_d, &wrong_receiver.sig.priv_key)
            .expect("signing outer should succeed");
        assert!(verify_single(&outer).is_ok());
        assert!(verify_chain(&outer).is_err());
    }

    #[test]
    fn verify_chain_accepts_depth3_alternation_author_receiver_author() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 3000);

        let middle_d = DatumBody {
            n: room.sig.pub_key.clone(),
            t: "middle.wrap".to_owned(),
            ts: 3001,
            r: Some(room.sig.pub_key.clone()),
            env: Some(Box::new(inner)),
            ..DatumBody::default()
        };
        let middle = sign_envelope(middle_d, &room.sig.priv_key).expect("middle should sign");

        let outer_d = DatumBody {
            n: author.sig.pub_key.clone(),
            t: "outer.wrap".to_owned(),
            ts: 3002,
            env: Some(Box::new(middle)),
            ..DatumBody::default()
        };
        let outer = sign_envelope(outer_d, &author.sig.priv_key).expect("outer should sign");
        assert_eq!(depth(&outer), 3);
        verify_chain(&outer).expect("depth-3 alternation should verify");
        validate_structure(&outer).expect("depth-3 structure should validate");
    }

    #[test]
    fn validate_structure_rejects_bad_signature_length() {
        let node = mk_node();
        let mut env = make_envelope("node.meta", Some(json!({"x": 1})), &node, None)
            .expect("make_envelope should succeed");
        env.p = "short".to_owned();
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn validate_structure_rejects_predating_commit_time() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 4000);

        let bad_commit_d = DatumBody {
            n: room.sig.pub_key.clone(),
            t: "commit".to_owned(),
            ts: 3999,
            r: Some(room.sig.pub_key.clone()),
            env: Some(Box::new(inner)),
            tc: Some(3999),
            ..DatumBody::default()
        };
        let bad_commit =
            sign_envelope(bad_commit_d, &room.sig.priv_key).expect("bad commit can still sign");
        assert!(validate_structure(&bad_commit).is_err());
    }

    #[test]
    fn validate_structure_rejects_depth_above_three() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 5000);

        let l2 = sign_envelope(
            DatumBody {
                n: room.sig.pub_key.clone(),
                t: "l2".to_owned(),
                ts: 5001,
                r: Some(room.sig.pub_key.clone()),
                env: Some(Box::new(inner)),
                ..DatumBody::default()
            },
            &room.sig.priv_key,
        )
        .expect("l2 should sign");

        let l3 = sign_envelope(
            DatumBody {
                n: author.sig.pub_key.clone(),
                t: "l3".to_owned(),
                ts: 5002,
                env: Some(Box::new(l2)),
                ..DatumBody::default()
            },
            &author.sig.priv_key,
        )
        .expect("l3 should sign");

        let l4 = sign_envelope(
            DatumBody {
                n: room.sig.pub_key.clone(),
                t: "l4".to_owned(),
                ts: 5003,
                env: Some(Box::new(l3)),
                ..DatumBody::default()
            },
            &room.sig.priv_key,
        )
        .expect("l4 should sign");

        assert!(validate_structure(&l4).is_err());
    }

    #[test]
    fn parse_envelope_rejects_extra_top_level_fields() {
        let node = mk_node();
        let env = make_envelope("node.meta", Some(json!({"x": 1})), &node, None)
            .expect("make_envelope should succeed");
        let mut value = serde_json::to_value(&env).expect("serialize should succeed");
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".to_owned(), json!("bad"));
        assert!(parse_envelope(value).is_err());
    }

    #[test]
    fn parse_envelope_rejects_extra_fields_in_nested_env() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 1000);
        let commit = wrap_commit(inner, &room, 1001).expect("wrap should succeed");
        let mut value = serde_json::to_value(&commit).expect("serialize should succeed");
        // inject an extra field into the nested d.env object
        value["d"]["env"]
            .as_object_mut()
            .unwrap()
            .insert("extra".to_owned(), json!("bad"));
        assert!(parse_envelope(value).is_err());
    }

    #[test]
    fn wrap_commit_outer_has_no_r_field() {
        let author = mk_node();
        let room = mk_room();
        let inner = mk_inner_room_event(&author, &room, 1000);
        let commit = wrap_commit(inner, &room, 1001).expect("wrap should succeed");
        assert!(
            commit.d.r.is_none(),
            "commit outer d.r must be None per spec §6.4"
        );
    }

    #[test]
    fn validate_datum_rejects_t_too_long() {
        let node = mk_node();
        let mut env = make_envelope("ok", None, &node, None).expect("make should succeed");
        env.d.t = "a".repeat(65);
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn validate_datum_rejects_t_invalid_chars() {
        let node = mk_node();
        let mut env = make_envelope("ok", None, &node, None).expect("make should succeed");
        env.d.t = "room message".to_owned(); // space not allowed
        assert!(validate_structure(&env).is_err());
        env.d.t = "room/message".to_owned(); // slash not allowed
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn validate_datum_rejects_ts_exceeding_js_safe_max() {
        let node = mk_node();
        let mut env = make_envelope("ok", None, &node, None).expect("make should succeed");
        env.d.ts = 9_007_199_254_740_992; // 2^53
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn validate_datum_rejects_r_wrong_length() {
        let node = mk_node();
        let mut env = make_envelope("ok", None, &node, None).expect("make should succeed");
        env.d.r = Some("tooshort".to_owned());
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn validate_datum_rejects_to_wrong_length() {
        let node = mk_node();
        let mut env = make_envelope("ok", None, &node, None).expect("make should succeed");
        env.d.to = Some("tooshort".to_owned());
        assert!(validate_structure(&env).is_err());
    }

    #[test]
    fn make_envelope_supports_extra_fields() {
        let node = mk_node();
        let extra = DatumBodyExtra {
            r: Some("room".to_owned()),
            to: Some("recipient".to_owned()),
            tc: Some(10),
            exp: Some(11),
            nonce: Some(12),
        };
        let env = make_envelope(
            "custom.event",
            Some(json!({"ok": true})),
            &node,
            Some(extra),
        )
        .expect("make_envelope should succeed");

        assert_eq!(env.d.r.as_deref(), Some("room"));
        assert_eq!(env.d.to.as_deref(), Some("recipient"));
        assert_eq!(env.d.tc, Some(10));
        assert_eq!(env.d.exp, Some(11));
        assert_eq!(env.d.nonce, Some(12));
    }

    #[test]
    fn manual_json_matches_serde_json() {
        let node = mk_node();
        let room = mk_room();

        // Test simple envelope
        let env = make_envelope("node.meta", Some(json!({"x": 1})), &node, None)
            .expect("make_envelope should succeed");
        let manual = envelope_to_json_bytes(&env);
        let serde = serde_json::to_vec(&env).expect("serde_json should succeed");
        assert_eq!(manual, serde, "simple envelope serialization mismatch");

        // Test envelope with content and extra fields
        let extra = DatumBodyExtra {
            r: Some(room.sig.pub_key.clone()),
            to: Some(node.sig.pub_key.clone()),
            tc: Some(100),
            exp: Some(200),
            nonce: Some(300),
        };
        let env2 = make_envelope(
            "room.message",
            Some(json!({"ch": 1, "body": "hello"})),
            &node,
            Some(extra),
        )
        .expect("make_envelope should succeed");
        let manual2 = envelope_to_json_bytes(&env2);
        let serde2 = serde_json::to_vec(&env2).expect("serde_json should succeed");
        assert_eq!(manual2, serde2, "complex envelope serialization mismatch");

        // Test depth-2 envelope (wrapped)
        let inner = mk_inner_room_event(&node, &room, 1000);
        let commit = wrap_commit(inner, &room, 1001).expect("wrap should succeed");
        let manual3 = envelope_to_json_bytes(&commit);
        let serde3 = serde_json::to_vec(&commit).expect("serde_json should succeed");
        assert_eq!(manual3, serde3, "depth-2 envelope serialization mismatch");

        // Test nested envelope (depth-3)
        let inner2 = mk_inner_room_event(&node, &room, 2000);
        let commit2 = wrap_commit(inner2, &room, 2001).expect("wrap should succeed");
        let env3 = make_envelope(
            "events.sync",
            Some(json!({"rooms": [room.sig.pub_key], "after": 0})),
            &node,
            Some(DatumBodyExtra {
                r: None,
                to: Some(commit2.d.n.clone()),
                tc: None,
                exp: None,
                nonce: None,
            }),
        )
        .expect("make_envelope should succeed");
        let manual4 = envelope_to_json_bytes(&env3);
        let serde4 = serde_json::to_vec(&env3).expect("serde_json should succeed");
        assert_eq!(manual4, serde4, "nested envelope serialization mismatch");
    }

    #[test]
    fn parse_envelope_bytes_zero_copy() {
        let node = mk_node();
        let room = mk_room();

        // Test simple envelope
        let env = make_envelope("node.meta", Some(json!({"x": 1})), &node, None)
            .expect("make_envelope should succeed");
        let json_bytes = serde_json::to_vec(&env).expect("serde_json should succeed");
        let mut bytes = json_bytes.clone();
        let parsed = parse_envelope_bytes(&mut bytes).expect("parse_envelope_bytes should succeed");
        assert_eq!(parsed.d.t, env.d.t);
        assert_eq!(parsed.d.n, env.d.n);
        assert_eq!(parsed.p, env.p);

        // Test depth-2 envelope
        let inner = mk_inner_room_event(&node, &room, 1000);
        let commit = wrap_commit(inner, &room, 1001).expect("wrap should succeed");
        let json_bytes2 = serde_json::to_vec(&commit).expect("serde_json should succeed");
        let mut bytes2 = json_bytes2.clone();
        let parsed2 = parse_envelope_bytes(&mut bytes2).expect("parse_envelope_bytes should succeed for depth-2");
        assert_eq!(parsed2.d.t, commit.d.t);
        assert_eq!(depth(&parsed2), 2);

        // Test depth-3 envelope
        let inner2 = mk_inner_room_event(&node, &room, 2000);
        let commit2 = wrap_commit(inner2, &room, 2001).expect("wrap should succeed");
        // Create depth-3 by wrapping the commit in another envelope (e.g., ack)
        let d3 = DatumBody {
            n: node.sig.pub_key.clone(),
            v: VERSION.to_owned(),
            t: "ack".to_owned(),
            ts: now_ms().unwrap(),
            env: Some(Box::new(commit2)),
            ..DatumBody::default()
        };
        let env3 = sign_envelope(d3, &node.sig.priv_key).expect("sign should succeed");
        let json_bytes3 = serde_json::to_vec(&env3).expect("serde_json should succeed");
        let mut bytes3 = json_bytes3.clone();
        let parsed3 = parse_envelope_bytes(&mut bytes3).expect("parse_envelope_bytes should succeed for depth-3");
        assert_eq!(depth(&parsed3), 3);
    }
}
