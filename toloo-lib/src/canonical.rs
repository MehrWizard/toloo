use std::cmp::Ordering;
use std::fmt::Write as _;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{Map, Number, Value};

use crate::types::DatumBody;

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

/// Canonical JSON as UTF-8 bytes.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(canonical(value)?.into_bytes())
}

/// Canonical JSON bytes for `DatumBody` with caching.
/// Uses `DatumBody.cached_canonical` OnceLock to avoid re-serialization.
/// Note: OnceLock can only be set once. For mutable fields (nonce), create a new DatumBody.
pub fn canonical_datum_bytes(d: &DatumBody) -> Result<Vec<u8>> {
    if let Some(cached) = d.cached_canonical.get() {
        return Ok(cached.clone());
    }
    let bytes = canonical(d)?.into_bytes();
    let _ = d.cached_canonical.set(bytes.clone());
    Ok(bytes)
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

    use super::canonical;

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
}
