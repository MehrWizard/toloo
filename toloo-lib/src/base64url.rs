/// URL-safe base64 encoding without padding (RFC 4648 §5, base64url).
///
/// Self-implemented — no external `base64ct` dependency. Uses the alphabet
/// `A-Za-z0-9-_` and emits no `=` padding.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn char_val(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

/// URL-safe base64 encoding without padding.
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    let len = input.len();

    while i + 3 <= len {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHABET[(n >> 18) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }

    let rem = len - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHABET[(n >> 18) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHABET[(n >> 18) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
    }

    out
}

/// URL-safe base64 decoding without padding.
///
/// Rejects `+`, `/`, `=`, and any non-alphabet characters.
pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = s.as_bytes();
    // In unpadded base64, 1 leftover char is impossible.
    if bytes.len() % 4 == 1 {
        anyhow::bail!("invalid base64url input length (mod 4 == 1)");
    }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    let len = bytes.len();

    while i + 4 <= len {
        let n = (char_val(bytes[i]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 18)
            | (char_val(bytes[i + 1]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 12)
            | (char_val(bytes[i + 2]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 6)
            | char_val(bytes[i + 3]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))?;
        out.push((n >> 16) as u8);
        out.push(((n >> 8) & 0xff) as u8);
        out.push((n & 0xff) as u8);
        i += 4;
    }

    let rem = len - i;
    if rem == 2 {
        let n = (char_val(bytes[i]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 18)
            | (char_val(bytes[i + 1]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 12);
        // Only first byte is valid; second 6-bit group is zero bits.
        out.push((n >> 16) as u8);
    } else if rem == 3 {
        let n = (char_val(bytes[i]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 18)
            | (char_val(bytes[i + 1]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 12)
            | (char_val(bytes[i + 2]).ok_or_else(|| anyhow::anyhow!("invalid base64url character"))? << 6);
        out.push((n >> 16) as u8);
        out.push(((n >> 8) & 0xff) as u8);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn encode_basic() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg");
        assert_eq!(encode(b"fo"), "Zm8");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg");
        assert_eq!(encode(b"fooba"), "Zm9vYmE");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encodes_zero_32_bytes_to_expected_value() {
        let input = vec![0u8; 32];
        let got = encode(&input);
        assert_eq!(got, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn roundtrips_bytes() {
        let input: Vec<u8> = (0..=63).collect();
        let encoded = encode(&input);
        let decoded = decode(&encoded).expect("decode should succeed");
        assert_eq!(decoded, input);
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(decode("a+b/").is_err());
        assert!(decode("AA==").is_err());
        assert!(decode("Zg=").is_err());
    }

    #[test]
    fn reject_plus_and_slash() {
        assert!(decode("+").is_err());
        assert!(decode("/").is_err());
    }

    #[test]
    fn roundtrip_all_byte_values() {
        for first in 0u8..=255 {
            let bytes = vec![first];
            let encoded = encode(&bytes);
            let decoded = decode(&encoded).expect("roundtrip should succeed");
            assert_eq!(decoded, bytes);
        }
    }
}
