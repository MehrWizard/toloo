/// URL-safe base64 encoding without padding (RFC 4648 §5, base64url).
///
/// Self-implemented — no external `base64ct` dependency. Uses the alphabet
/// `A-Za-z0-9-_` and emits no `=` padding.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const INVALID: u8 = 0xFF;

const fn decode_table() -> [u8; 256] {
    let mut table = [INVALID; 256];
    let mut i = 0usize;
    while i < 64 {
        let byte = match i {
            0..=25 => b'A' + i as u8,
            26..=51 => b'a' + (i - 26) as u8,
            52..=61 => b'0' + (i - 52) as u8,
            62 => b'-',
            _ => b'_',
        };
        table[byte as usize] = i as u8;
        i += 1;
    }
    table
}

static DECODE_TABLE: [u8; 256] = decode_table();

const fn encode_pair_table() -> [[u8; 2]; 4096] {
    let mut table = [[0u8; 2]; 4096];
    let mut i = 0usize;
    while i < 4096 {
        table[i] = [ALPHABET[i >> 6], ALPHABET[i & 0x3f]];
        i += 1;
    }
    table
}

static ENCODE_TABLE: [[u8; 2]; 4096] = encode_pair_table();

fn invalid() -> anyhow::Error {
    anyhow::anyhow!("invalid base64url character")
}

/// URL-safe base64 encoding without padding.
pub fn encode(input: &[u8]) -> String {
    let out_len = input.len().div_ceil(3) * 4;
    let mut out = vec![0u8; out_len];
    let full = input.len() / 3;

    for (chunk, out4) in input[..full * 3]
        .chunks_exact(3)
        .zip(out.chunks_exact_mut(4))
    {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        let a = ENCODE_TABLE[(n >> 12) as usize];
        let b = ENCODE_TABLE[(n & 0xfff) as usize];
        out4[0] = a[0];
        out4[1] = a[1];
        out4[2] = b[0];
        out4[3] = b[1];
    }

    let i = full * 3;
    let mut j = full * 4;
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        let a = ENCODE_TABLE[(n >> 12) as usize];
        out[j] = a[0];
        out[j + 1] = a[1];
        j += 2;
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        let a = ENCODE_TABLE[(n >> 12) as usize];
        out[j] = a[0];
        out[j + 1] = a[1];
        out[j + 2] = ALPHABET[((n >> 6) & 0x3f) as usize];
        j += 3;
    }
    out.truncate(j);

    // The alphabet is ASCII by construction.
    String::from_utf8(out).expect("base64url alphabet is ASCII")
}

/// URL-safe base64 decoding without padding.
///
/// Rejects `+`, `/`, `=`, and any non-alphabet characters.
pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = s.as_bytes();
    let rem = bytes.len() % 4;
    // In unpadded base64, 1 leftover char is impossible.
    if rem == 1 {
        anyhow::bail!("invalid base64url input length (mod 4 == 1)");
    }

    let full = bytes.len() / 4;
    let tail_len = if rem == 2 {
        1
    } else if rem == 3 {
        2
    } else {
        0
    };
    let mut out = vec![0u8; full * 3 + tail_len];

    for (chunk, out3) in bytes[..full * 4]
        .chunks_exact(4)
        .zip(out.chunks_exact_mut(3))
    {
        let a = DECODE_TABLE[chunk[0] as usize];
        let b = DECODE_TABLE[chunk[1] as usize];
        let c = DECODE_TABLE[chunk[2] as usize];
        let d = DECODE_TABLE[chunk[3] as usize];
        if (a | b | c | d) == INVALID {
            return Err(invalid());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | d as u32;
        out3.copy_from_slice(&[
            (n >> 16) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as u8,
        ]);
    }

    let i = full * 4;
    let j = full * 3;
    if rem == 2 {
        let a = DECODE_TABLE[bytes[i] as usize];
        let b = DECODE_TABLE[bytes[i + 1] as usize];
        if a == INVALID || b == INVALID {
            return Err(invalid());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12);
        // Only first byte is valid; second 6-bit group is zero bits.
        out[j] = (n >> 16) as u8;
    } else if rem == 3 {
        let a = DECODE_TABLE[bytes[i] as usize];
        let b = DECODE_TABLE[bytes[i + 1] as usize];
        let c = DECODE_TABLE[bytes[i + 2] as usize];
        if (a | b | c) == INVALID {
            return Err(invalid());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6);
        out[j] = (n >> 16) as u8;
        out[j + 1] = ((n >> 8) & 0xff) as u8;
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
