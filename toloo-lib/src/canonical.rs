use std::cmp::Ordering;
use std::fmt::Write as _;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{Map, Number, Value};

use crate::types::{DatumBody, Envelope};

/// Compare two strings by their UTF-16 code units (as required by Toloo spec).
/// Optimized: encodes both strings to `Vec<u16>` once and compares slices directly,
/// leveraging Rust's optimized slice comparison (SIMD-accelerated on supported platforms).
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    // Fast path for ASCII strings (common case for JSON keys)
    if a.is_ascii() && b.is_ascii() {
        return a.as_bytes().cmp(b.as_bytes());
    }
    let a_utf16: Vec<u16> = a.encode_utf16().collect();
    let b_utf16: Vec<u16> = b.encode_utf16().collect();
    a_utf16.cmp(&b_utf16)
}


/// Serialize any serde value into canonical JSON used by Toloo signatures/hashes.
pub fn canonical<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value)?;
    canonical_value(&value)
}

/// Canonical JSON as UTF-8 bytes (skips intermediate String allocation).
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let json_value = serde_json::to_value(value)?;
    Ok(canonical_value(&json_value)?.into_bytes())
}

/// Canonical JSON bytes for `DatumBody` with caching.
/// Uses `DatumBody.cached_canonical` OnceLock to avoid re-serialization.
/// Note: OnceLock can only be set once. For mutable fields (nonce), create a new DatumBody.
pub fn canonical_datum_bytes(d: &DatumBody) -> Result<Vec<u8>> {
    if let Some(cached) = d.cached_canonical.get() {
        return Ok(cached.clone());
    }
    let bytes = canonical_datum_bytes_direct(d)?;
    let _ = d.cached_canonical.set(bytes.clone());
    Ok(bytes)
}

/// Canonical JSON for a `DatumBody`, written directly to bytes.
///
/// Skips the `serde_json::Value` tree entirely: fields are collected, sorted
/// by UTF-16 code units (including flattened `extra` entries), and emitted.
/// Byte-for-byte identical to `canonical_bytes(d)`.
pub fn canonical_datum_bytes_direct(d: &DatumBody) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(256 + d.extra.len() * 32);
    write_datum_bytes(d, &mut out)?;
    Ok(out)
}

/// Canonical JSON for an `Envelope` (`{"d":…,"p":…}`), written directly.
pub fn canonical_envelope_bytes_direct(env: &Envelope) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(512);
    write_envelope_bytes(env, &mut out)?;
    Ok(out)
}

enum DatumField<'a> {
    Str(&'a str),
    U64(u64),
    Value(&'a Value),
    Env(&'a Envelope),
}

fn write_envelope_bytes(env: &Envelope, out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(b"{\"d\":");
    if let Some(cached) = env.d.cached_canonical.get() {
        out.extend_from_slice(cached);
    } else {
        let start = out.len();
        write_datum_bytes(&env.d, out)?;
        let _ = env.d.cached_canonical.set(out[start..].to_vec());
    }
    out.extend_from_slice(b",\"p\":");
    write_string_bytes(&env.p, out);
    out.push(b'}');
    Ok(())
}

fn write_datum_bytes(d: &DatumBody, out: &mut Vec<u8>) -> Result<()> {
    let mut entries: Vec<(&str, DatumField<'_>)> = Vec::with_capacity(11 + d.extra.len());
    entries.push(("n", DatumField::Str(&d.n)));
    entries.push(("v", DatumField::Str(&d.v)));
    entries.push(("t", DatumField::Str(&d.t)));
    entries.push(("ts", DatumField::U64(d.ts)));
    if let Some(r) = &d.r {
        entries.push(("r", DatumField::Str(r)));
    }
    if let Some(to) = &d.to {
        entries.push(("to", DatumField::Str(to)));
    }
    if let Some(c) = &d.c {
        entries.push(("c", DatumField::Value(c)));
    }
    if let Some(env) = &d.env {
        entries.push(("env", DatumField::Env(env)));
    }
    if let Some(tc) = d.tc {
        entries.push(("tc", DatumField::U64(tc)));
    }
    if let Some(exp) = d.exp {
        entries.push(("exp", DatumField::U64(exp)));
    }
    if let Some(nonce) = d.nonce {
        entries.push(("nonce", DatumField::U64(nonce)));
    }

    // Flattened extra fields; a colliding key replaces the known field,
    // matching what `serde_json::to_value` produces for `#[serde(flatten)]`.
    if !d.extra.is_empty() {
        for (key, value) in &d.extra {
            if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == key.as_str()) {
                slot.1 = DatumField::Value(value);
            } else {
                entries.push((key.as_str(), DatumField::Value(value)));
            }
        }
    }

    entries.sort_by(|(a, _), (b, _)| utf16_cmp(a, b));
    out.push(b'{');
    for (idx, (key, value)) in entries.iter().enumerate() {
        if idx > 0 {
            out.push(b',');
        }
        write_string_bytes(key, out);
        out.push(b':');
        match value {
            DatumField::Str(s) => write_string_bytes(s, out),
            DatumField::U64(n) => write_u64_bytes(*n, out),
            DatumField::Value(v) => write_value_bytes(v, out)?,
            DatumField::Env(env) => write_envelope_bytes(env, out)?,
        }
    }
    out.push(b'}');
    Ok(())
}

fn write_u64_bytes(mut n: u64, out: &mut Vec<u8>) {
    let mut buf = [0u8; 20];
    let mut idx = buf.len();
    loop {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[idx..]);
}

/// Direct-to-bytes canonical JSON — avoids String→Vec<u8> conversion.
pub fn canonical_value_bytes(value: &Value) -> Result<Vec<u8>> {
    let estimated = estimate_json_size(value);
    let mut out = Vec::with_capacity(estimated);
    write_value_bytes(value, &mut out)?;
    Ok(out)
}

/// Write canonical JSON directly into a provided buffer (no reallocation).
pub fn canonical_value_bytes_into(value: &Value, out: &mut Vec<u8>) -> Result<()> {
    out.clear();
    let estimated = estimate_json_size(value);
    out.reserve(estimated);
    write_value_bytes(value, out)?;
    Ok(())
}

fn write_value_bytes(value: &Value, out: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(v) => out.extend_from_slice(if *v { b"true" } else { b"false" }),
        Value::Number(n) => write_number_bytes(n, out)?,
        Value::String(s) => write_string_bytes(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 { out.push(b','); }
                write_value_bytes(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => write_object_bytes(map, out)?,
    }
    Ok(())
}

fn write_object_bytes(map: &Map<String, Value>, out: &mut Vec<u8>) -> Result<()> {
    let mut entries: Vec<(&str, &Value)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by(|(a, _), (b, _)| utf16_cmp(a, b));
    out.push(b'{');
    for (idx, (k, v)) in entries.iter().enumerate() {
        if idx > 0 { out.push(b','); }
        write_string_bytes(k, out);
        out.push(b':');
        write_value_bytes(v, out)?;
    }
    out.push(b'}');
    Ok(())
}

fn write_string_bytes(input: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    let bytes = input.as_bytes();
    let mut last = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let escape: Option<&[u8]> = match b {
            b'"' => Some(b"\\\""),
            b'\\' => Some(b"\\\\"),
            b'\x08' => Some(b"\\b"),
            b'\x0C' => Some(b"\\f"),
            b'\n' => Some(b"\\n"),
            b'\r' => Some(b"\\r"),
            b'\t' => Some(b"\\t"),
            0x00..=0x1F => None,
            _ => continue,
        };
        if let Some(esc) = escape {
            out.extend_from_slice(&bytes[last..i]);
            out.extend_from_slice(esc);
            last = i + 1;
        } else if b <= 0x1F {
            out.extend_from_slice(&bytes[last..i]);
            out.extend_from_slice(&[b'\\', b'u', b'0', b'0']);
            out.push(hex_lower(b >> 4));
            out.push(hex_lower(b & 0xF));
            last = i + 1;
        }
    }
    out.extend_from_slice(&bytes[last..]);
    out.push(b'"');
}

#[inline]
fn hex_lower(n: u8) -> u8 {
    if n < 10 { b'0' + n } else { b'a' + (n - 10) }
}

fn write_number_bytes(number: &Number, out: &mut Vec<u8>) -> Result<()> {
    if let Some(v) = number.as_f64() {
        if !v.is_finite() {
            bail!("non-finite numbers are not valid JSON");
        }
    }
    let s = number.to_string();
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// Serialize a JSON value into canonical JSON.
pub fn canonical_value(value: &Value) -> Result<String> {
    // Estimate capacity: roughly 2x the serialized size for safety
    let estimated = estimate_json_size(value);
    let mut out = String::with_capacity(estimated);
    write_value(value, &mut out)?;
    Ok(out)
}

/// Roughly estimate JSON serialized size for capacity reservation.
fn estimate_json_size(value: &Value) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(_) => 5,
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => s.len() + 2, // quotes + escapes
        Value::Array(items) => 2 + items.iter().map(estimate_json_size).sum::<usize>() + items.len().saturating_sub(1),
        Value::Object(map) => 2 + map.iter().map(|(k, v)| k.len() + 1 + estimate_json_size(v) + 1).sum::<usize>(),
    }
}

fn write_value(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(v) => {
            if *v {
                out.push_str("true");
            } else {
                out.push_str("false");
            }
        }
        Value::Number(n) => write_number(n, out)?,
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => write_object(map, out)?,
    }
    Ok(())
}

fn write_object(map: &Map<String, Value>, out: &mut String) -> Result<()> {
    let mut entries: Vec<(&str, &Value)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by(|(a, _), (b, _)| utf16_cmp(a, b));

    out.push('{');
    for (idx, (k, v)) in entries.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        write_string(k, out);
        out.push(':');
        write_value(v, out)?;
    }
    out.push('}');
    Ok(())
}

fn write_string(input: &str, out: &mut String) {
    out.push('"');
    let mut last = 0;
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\x08' => Some("\\b"),
            b'\x0C' => Some("\\f"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0x00..=0x1F => None, // handled below
            _ => continue,
        };
        if let Some(esc) = escape {
            out.push_str(&input[last..i]);
            out.push_str(esc);
            last = i + 1;
        } else if b <= 0x1F {
            out.push_str(&input[last..i]);
            let _ = write!(out, "\\u{:04x}", b);
            last = i + 1;
        }
    }
    out.push_str(&input[last..]);
    out.push('"');
}

fn write_number(number: &Number, out: &mut String) -> Result<()> {
    if let Some(v) = number.as_f64() {
        if !v.is_finite() {
            bail!("non-finite numbers are not valid JSON");
        }
    }
    // Write directly to output to avoid String allocation
    out.push_str(&number.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    

    use serde_json::json;

    use crate::types::DatumBody;

    use super::{canonical, canonical_value_bytes};

    #[test]
    fn canonicalizes_known_readme_example() {
        let v = json!({
            "ts": 1,
            "n": "A",
            "v": "0.2",
            "t": "room.message"
        });
        let got = canonical(&v).expect("canonicalization should succeed");
        assert_eq!(got, r#"{"n":"A","t":"room.message","ts":1,"v":"0.2"}"#);
    }

    #[test]
    fn sorts_object_keys_by_utf16_units() {
        let v = json!({
            "\u{E000}": 1,
            "\u{10000}": 2
        });
        let got = canonical(&v).expect("canonicalization should succeed");
        let pos_u10000 = got.find('\u{10000}').expect("should contain U+10000");
        let pos_ue000 = got.find('\u{E000}').expect("should contain U+E000");
        assert!(pos_u10000 < pos_ue000);
    }

    #[test]
    fn omits_none_fields_when_struct_uses_skip_serializing_if() {
        let d = DatumBody {
            n: "A".to_owned(),
            t: "room.message".to_owned(),
            ts: 1,
            ..DatumBody::default()
        };

        let got = canonical(&d).expect("canonicalization should succeed");
        assert_eq!(got, r#"{"n":"A","t":"room.message","ts":1,"v":"0.2"}"#);
        assert!(!got.contains("null"));
    }

    #[test]
    fn canonical_value_bytes_matches_string_output() {
        let v = json!({
            "ts": 1,
            "n": "A",
            "v": "0.2",
            "t": "room.message"
        });
        let from_value = canonical_value_bytes(&v).expect("bytes canonicalization should succeed");
        let from_string = canonical(&v).expect("string canonicalization should succeed");
        assert_eq!(from_value, from_string.into_bytes());
    }

    #[test]
    fn canonical_value_bytes_handles_control_chars() {
        let v = json!({ "x": "hello\t\nworld" });
        let bytes = canonical_value_bytes(&v).expect("should succeed");
        let s = canonical(&v).expect("string should succeed");
        assert_eq!(bytes, s.into_bytes());
    }

    #[test]
    fn direct_datum_bytes_match_value_path() {
        use std::collections::HashMap;

        use crate::types::Envelope;

        let mut extra = HashMap::new();
        extra.insert("aa".to_owned(), json!({"k": [1, true, null]}));
        extra.insert("\u{E000}".to_owned(), json!(2));

        let inner_d = DatumBody {
            n: "n1".to_owned(),
            t: "room.message".to_owned(),
            ts: 10,
            r: Some("room".to_owned()),
            c: Some(json!({"ch": 1, "body": "hi\tthere"})),
            ..DatumBody::default()
        };
        let inner = Envelope {
            d: inner_d.clone(),
            p: "c2ln".to_owned(),
            cached_sig: Default::default(),
        };

        let outer_d = DatumBody {
            n: "n2".to_owned(),
            t: "commit".to_owned(),
            ts: 11,
            tc: Some(11),
            env: Some(Box::new(inner)),
            extra,
            nonce: Some(7),
            ..DatumBody::default()
        };
        let outer = Envelope {
            d: outer_d,
            p: "c2ln".to_owned(),
            cached_sig: Default::default(),
        };

        // Depth-1 datum, with extra/unicode keys and control characters.
        let direct = super::canonical_datum_bytes_direct(&inner_d).expect("direct");
        let via_value = super::canonical_bytes(&inner_d).expect("value");
        assert_eq!(direct, via_value, "datum direct bytes must match Value path");

        // Nested envelope.
        let direct_env = super::canonical_envelope_bytes_direct(&outer).expect("direct env");
        let value_env = super::canonical_bytes(&outer).expect("value env");
        assert_eq!(direct_env, value_env, "envelope direct bytes must match Value path");
    }
}
