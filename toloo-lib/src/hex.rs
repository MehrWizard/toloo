/// Hexadecimal encoding/decoding — self-implemented, no external dependency.
///
/// `encode` produces lowercase hex. `decode` accepts both upper and lower case.
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Encode bytes as a lowercase hexadecimal string.
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    let input = input.as_ref();
    let mut out = String::with_capacity(input.len() * 2);
    for &b in input {
        out.push(HEX_LOWER[(b >> 4) as usize] as char);
        out.push(HEX_LOWER[(b & 0x0f) as usize] as char);
    }
    out
}

/// Decode a hexadecimal string to bytes.
/// Accepts both upper and lower case hex digits.
pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        anyhow::bail!("hex string has odd length");
    }

    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.as_chunks::<2>().0 {
        let hi = hex_val(chunk[0]).ok_or_else(|| anyhow::anyhow!("invalid hex character: {:?}", chunk[0] as char))?;
        let lo = hex_val(chunk[1]).ok_or_else(|| anyhow::anyhow!("invalid hex character: {:?}", chunk[1] as char))?;
        out.push((hi << 4) | lo);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn encode_known_vector() {
        assert_eq!(encode(b"abc"), "616263");
        assert_eq!(encode(b"\x00\xff"), "00ff");
    }

    #[test]
    fn roundtrip() {
        let input: Vec<u8> = (0..=255).collect();
        let encoded = encode(&input);
        let decoded = decode(&encoded).expect("decode should succeed");
        assert_eq!(decoded, input);
    }

    #[test]
    fn accepts_uppercase() {
        assert_eq!(decode("ABCD").unwrap(), vec![0xab, 0xcd]);
        assert_eq!(decode("abcd").unwrap(), vec![0xab, 0xcd]);
        assert_eq!(decode("AbCd").unwrap(), vec![0xab, 0xcd]);
    }

    #[test]
    fn rejects_odd_length() {
        assert!(decode("abc").is_err());
    }

    #[test]
    fn rejects_invalid_chars() {
        assert!(decode("xy").is_err());
        assert!(decode("ag").is_err());
    }
}
