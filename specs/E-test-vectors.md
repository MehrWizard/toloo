# Appendix E: Test Vectors

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [2] Data Model, [3] Cryptography

## E.1. Purpose

This appendix provides deterministic test vectors for verifying protocol
implementations. Any correct implementation given the same inputs MUST
produce identical outputs.

Test vectors cover: key derivation, canonical JSON, envelope signing,
commit wrapping, envelope identity, private message encryption, endpoint
encryption, and proof-of-work.

Values marked `[REF]` are computed by the reference implementation and
filled in at that time. The inputs and algorithms are fully specified —
any correct implementation can compute the expected outputs independently.

## E.2. Test Identities

All test identities use deterministic Ed25519 seeds. The seed is the
32-byte Ed25519 private key input as defined in [RFC8032] Section 5.1.5.

### E.2.1. Alice (Regular User)

```
seed_hex:    4f6a183e5a4b3c2d1e0f987654321abc4f6a183e5a4b3c2d1e0f987654321abc
sig_pub_hex: [REF]
sig_pub_b64: [REF]
```

Alice also has an X25519 encryption keypair. The X25519 private key is
derived by applying the same seed to X25519 key generation [RFC7748]:

```
enc_seed_hex: 5a4b3c2d1e0f987654321abc4f6a183e5a4b3c2d1e0f987654321abc4f6a183e
enc_pub_hex:  [REF]
enc_pub_b64:  [REF]
```

### E.2.2. Bob (Regular User)

```
seed_hex:    1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809
sig_pub_hex: [REF]
sig_pub_b64: [REF]

enc_seed_hex: 2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a
enc_pub_hex:  [REF]
enc_pub_b64:  [REF]
```

### E.2.3. Carol (Room Creator / Key Holder)

```
seed_hex:    aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344
sig_pub_hex: [REF]
sig_pub_b64: [REF]

enc_seed_hex: bbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344aabb
enc_pub_hex:  [REF]
enc_pub_b64:  [REF]
```

### E.2.4. Room (Ed25519 Only — No X25519)

```
seed_hex:    deadbeef00112233deadbeef00112233deadbeef00112233deadbeef00112233
sig_pub_hex: [REF]
sig_pub_b64: [REF]
```

Rooms have Ed25519 keypairs only. No X25519 key.

### E.2.5. Relay (Public Node)

```
seed_hex:    ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00
sig_pub_hex: [REF]
sig_pub_b64: [REF]

enc_seed_hex: 00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff
enc_pub_hex:  [REF]
enc_pub_b64:  [REF]
```

## E.3. Canonical JSON

These test cases verify RFC 8785 canonicalization. No cryptography —
pure JSON transformation.

### E.3.1. Key Ordering

Input (pretty-printed):

```json
{
  "ts": 1710000000000,
  "n": "ALICE_SIG",
  "v": "0.2",
  "t": "room.message"
}
```

Expected canonical output (single line, keys sorted lexicographically):

```
{"n":"ALICE_SIG","t":"room.message","ts":1710000000000,"v":"0.2"}
```

### E.3.2. Nested Objects

Input:

```json
{
  "v": "0.2",
  "t": "commit",
  "n": "ROOM_SIG",
  "ts": 1710000100000,
  "tc": 1710000100000,
  "env": {
    "p": "INNER_SIG",
    "d": {
      "ts": 1710000000000,
      "c": {"ch": 1, "body": "hello"},
      "n": "ALICE_SIG",
      "v": "0.2",
      "t": "room.message",
      "r": "ROOM_SIG"
    }
  }
}
```

Expected canonical output:

```
{"env":{"d":{"c":{"body":"hello","ch":1},"n":"ALICE_SIG","r":"ROOM_SIG","t":"room.message","ts":1710000000000,"v":"0.2"},"p":"INNER_SIG"},"n":"ROOM_SIG","t":"commit","tc":1710000100000,"ts":1710000100000,"v":"0.2"}
```

Note: `env` sorts before `n` because `e` < `n`. Within `env.d`, keys
sort as `c` < `n` < `r` < `t` < `ts` < `v`. Within `c`, `body` < `ch`.

### E.3.3. Unicode and Escaping

Input:

```json
{
  "t": "room.message",
  "n": "TEST",
  "v": "0.2",
  "ts": 0,
  "c": {"body": "سلام"}
}
```

Expected canonical output (UTF-8 preserved per RFC 8785):

```
{"c":{"body":"سلام"},"n":"TEST","t":"room.message","ts":0,"v":"0.2"}
```

RFC 8785 requires UTF-8 characters outside the ASCII control range to be
preserved as-is (not escaped). Only characters that JSON requires to be
escaped (U+0000 through U+001F, `"`, `\`) use escape sequences.

### E.3.4. Numbers

Input:

```json
{
  "n": "X",
  "v": "0.2",
  "t": "test",
  "ts": 1710000000000,
  "c": {"a": 0, "b": -1, "c": 1.5, "d": 100}
}
```

Expected canonical output:

```
{"c":{"a":0,"b":-1,"c":1.5,"d":100},"n":"X","t":"test","ts":1710000000000,"v":"0.2"}
```

RFC 8785 number rules: no leading zeros, no trailing zeros after decimal,
no positive sign, `-0` becomes `0`.

### E.3.5. Empty and Null Values

Input:

```json
{
  "n": "X",
  "v": "0.2",
  "t": "test",
  "ts": 0,
  "c": {"empty": "", "list": [], "obj": {}, "null_val": null, "bool": true}
}
```

Expected canonical output:

```
{"c":{"bool":true,"empty":"","list":[],"null_val":null,"obj":{}},"n":"X","t":"test","ts":0,"v":"0.2"}
```

### E.3.6. UTF-8 Byte Length

The canonical JSON from §E.3.2 encodes to these UTF-8 bytes (hex):

```
[REF] (hex dump of canonical JSON bytes — important for signature input)
```

Byte length: `[REF]`

## E.4. Envelope Signing

### E.4.1. Simple Room Message (Depth 1)

Alice sends a message to a room.

**Input `d` object** (with real test identity public keys from §E.2):

```json
{
  "n": "[Alice sig_pub_b64]",
  "v": "0.2",
  "t": "room.message",
  "r": "[Room sig_pub_b64]",
  "ts": 1710000000000,
  "c": {
    "ch": 1,
    "body": "Hello, world!"
  }
}
```

**Step 1**: Canonicalize `d`:

```
[REF: canonical JSON string]
```

**Step 2**: Encode as UTF-8 bytes:

```
[REF: hex of UTF-8 bytes]
```

Byte length: `[REF]`

**Step 3**: Sign with Alice's Ed25519 private key:

```
signature_hex: [REF]
signature_b64: [REF]
```

**Complete envelope**:

```json
{
  "d": {
    "n": "[Alice sig_pub_b64]",
    "v": "0.2",
    "t": "room.message",
    "r": "[Room sig_pub_b64]",
    "ts": 1710000000000,
    "c": {
      "ch": 1,
      "body": "Hello, world!"
    }
  },
  "p": "[REF: signature_b64]"
}
```

### E.4.2. Node Meta (With Content)

Alice publishes her node metadata.

**Input `d` object**:

```json
{
  "n": "[Alice sig_pub_b64]",
  "v": "0.2",
  "t": "node.meta",
  "ts": 1710000000000,
  "c": {
    "enc": "[Alice enc_pub_b64]",
    "endpoints": [
      {
        "proto": "wss",
        "host": "relay.example.com",
        "port": 443,
        "path": "/toloo"
      }
    ]
  }
}
```

**Canonical JSON**:

```
[REF]
```

**Signature**: `[REF]`

### E.4.3. Verification Failure

Given the valid envelope from §E.4.1, modify `d.c.body` to
`"Hello, world!!"` (extra `!`) without re-signing. The signature `p`
MUST fail verification because the signed bytes no longer match.

Implementations MUST verify that this modified envelope is rejected.

## E.5. Depth-2 Commit Envelope

### E.5.1. Room Commits Alice's Message

Carol (room key holder) commits Alice's depth-1 message from §E.4.1.

**Inner envelope**: The complete `{d, p}` from §E.4.1.

**Outer `d` object** (commit wrapper):

```json
{
  "n": "[Room sig_pub_b64]",
  "v": "0.2",
  "t": "commit",
  "ts": 1710000100000,
  "tc": 1710000100000,
  "env": {
    "d": { ... },
    "p": "..."
  }
}
```

Note: `env` contains the EXACT inner envelope from §E.4.1 — byte-for-byte
identical. The room signs over `canonical(outer_d)` which includes
the inner envelope inside `env`.

**Canonical JSON of outer d**:

```
[REF]
```

**Signature** (by Room private key):

```
[REF]
```

**Complete depth-2 envelope**:

```json
{
  "d": {
    "n": "[Room sig_pub_b64]",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000100000,
    "tc": 1710000100000,
    "env": {
      "d": {
        "n": "[Alice sig_pub_b64]",
        "v": "0.2",
        "t": "room.message",
        "r": "[Room sig_pub_b64]",
        "ts": 1710000000000,
        "c": {"ch": 1, "body": "Hello, world!"}
      },
      "p": "[REF: Alice's signature]"
    }
  },
  "p": "[REF: Room's signature]"
}
```

### E.5.2. Verification of Depth-2

To verify the depth-2 envelope:

1. Verify outer: `Ed25519.verify(d.n, canonical(d), p)` using Room's
   public key → MUST pass
2. Verify `d.n == d.env.d.r` (room signed the commit, inner targets
   that room) → MUST pass
3. Verify inner: `Ed25519.verify(d.env.d.n, canonical(d.env.d), d.env.p)`
   using Alice's public key → MUST pass
4. Verify signer alternation: depth-1 signer is Alice (sender), depth-2
   signer is Room (receiver) → MUST pass

## E.6. Envelope Identity

### E.6.1. Room Envelope Identity (eid)

Using the depth-2 envelope from §E.5.1:

```
innermost d = d.env.d
ch = d.env.d.c.ch = 1
ts = d.env.d.ts = 1710000000000
n = d.env.d.n = [Alice sig_pub_b64]

eid = "1:1710000000000:[Alice sig_pub_b64]"
```

The eid is the same whether computed from the depth-1 or depth-2
envelope (identity is stable across depths).

### E.6.2. Direct Envelope Identity

Alice's `node.meta` from §E.4.2:

```
innermost d = d (no env, depth 1)
ts = d.ts = 1710000000000
n = d.n = [Alice sig_pub_b64]

id = "1710000000000:[Alice sig_pub_b64]"
```

### E.6.3. Datum ID (Content Hash)

The datum_id of the depth-1 envelope from §E.4.1:

```
canonical_envelope = canonical({"d": ..., "p": "..."})
```

Canonical envelope JSON:

```
[REF]
```

```
datum_id = hex(SHA-256(utf8(canonical_envelope)))
         = [REF]
```

The datum_id of the depth-2 envelope from §E.5.1 is DIFFERENT (because
the envelope content changed by wrapping):

```
datum_id_depth2 = [REF]
```

## E.7. Private Message Encryption

> **Legacy v1 vector.** v2 private messages are static-static X25519 with a
> random per-message salt (§3.5.3). This vector remains valid for v1 decrypt
> compatibility; a deterministic v2 vector will be added with the versioned
> spec once fixed pair/salt inputs are published.

### E.7.1. Alice Sends Private Message to Bob

**Inputs**:
- Alice's signing seed: from §E.2.1
- Bob's X25519 encryption public key: from §E.2.2
- Ephemeral X25519 seed (deterministic for testing):
  `1111111111111111111111111111111111111111111111111111111111111111`
- Plaintext content: `{"body":"Hello Bob"}`
- Timestamp: `1710000500000`

**Step 1**: Generate ephemeral X25519 keypair from seed:

```
eph_priv_hex: 1111111111111111111111111111111111111111111111111111111111111111
eph_pub_hex:  [REF]
eph_pub_b64:  [REF]
```

**Step 2**: X25519 shared secret:

```
shared = X25519(eph_priv, bob_enc_pub)
shared_hex: [REF]
```

**Step 3**: Salt:

```
salt = SHA-256(eph_pub_bytes || bob_enc_pub_bytes)
salt_hex: [REF]
```

**Step 4**: HKDF-SHA256:

```
okm = HKDF-SHA256(
  ikm = shared,
  salt = salt,
  info = "toloo-private-v0.2" (UTF-8 bytes),
  L = 44
)
okm_hex: [REF]
key = okm[0:32]
key_hex: [REF]
nonce = okm[32:44]
nonce_hex: [REF]
```

**Step 5**: Canonicalize plaintext:

```
canonical_plaintext = {"body":"Hello Bob"}
plaintext_bytes_hex: [REF]
```

**Step 6**: ChaCha20-Poly1305 encrypt:

```
ciphertext = ChaCha20-Poly1305(key, nonce, plaintext_bytes, aad="")
ciphertext_hex: [REF]
ciphertext_b64: [REF]
```

**Step 7**: Create and sign the envelope:

```json
{
  "d": {
    "n": "[Alice sig_pub_b64]",
    "v": "0.2",
    "t": "private.message",
    "ts": 1710000500000,
    "to": "[Bob sig_pub_b64]",
    "c": {
      "eph": "[REF: eph_pub_b64]",
      "encrypted": "[REF: ciphertext_b64]"
    }
  },
  "p": "[REF: Alice's Ed25519 signature]"
}
```

### E.7.2. Bob Decrypts

Bob receives the envelope from §E.7.1 and decrypts:

1. Extract `c.eph` → decode base64url → ephemeral public key bytes
2. `shared = X25519(bob_enc_priv, eph_pub)` → same shared secret as §E.7.1
3. `salt = SHA-256(eph_pub || bob_enc_pub)` → same salt
4. HKDF → same key and nonce
5. `plaintext = ChaCha20-Poly1305.decrypt(key, nonce, ciphertext, aad="")`
6. Parse JSON: `{"body":"Hello Bob"}`

Implementations MUST verify that decrypted output matches the original
plaintext from §E.7.1.

## E.9. Proof of Work

### E.9.1. PoW Verification

Given a `d` object with difficulty requirement 16 (16 leading zero bits):

```json
{
  "n": "[Alice sig_pub_b64]",
  "v": "0.2",
  "t": "room.join",
  "r": "[Room sig_pub_b64]",
  "ts": 1710000200000,
  "nonce": 0
}
```

**Algorithm**: Increment `nonce` until `SHA-256(canonical(d))` has at
least 16 leading zero bits.

```
nonce = 0:
  canonical = [REF]
  hash = SHA-256(utf8(canonical))
  hash_hex = [REF]
  leading_zeros = [REF] (< 16, fail)

nonce = [REF: winning nonce]:
  canonical = [REF]
  hash_hex = [REF: starts with 0000...]
  leading_zeros = [REF] (>= 16, pass)
```

Implementations MUST verify that the winning nonce produces a hash with
at least 16 leading zero bits.

### E.9.2. Leading Zero Bit Counting

Test cases for the leading zero bit counter:

| Hash (first 4 bytes hex) | Leading zero bits |
|--------------------------|-------------------|
| `00000000 ...` | >= 32 |
| `0000ffff ...` | 16 |
| `00ffffff ...` | 8 |
| `0fffffff ...` | 4 |
| `7fffffff ...` | 1 |
| `80000000 ...` | 0 |
| `ffffffff ...` | 0 |

## E.10. Membership Reconstruction

### E.10.1. Basic Membership Sequence

Given these committed channel -2 events in tc order:

| seq | tc | Type | Inner d.n | Content |
|-----|-----|------|-----------|---------|
| 1 | 1710000100000 | room.join | Alice | — |
| 2 | 1710000200000 | room.join | Bob | — |
| 3 | 1710000300000 | room.join | Carol | — |
| 4 | 1710000400000 | room.leave | Bob | — |
| 5 | 1710000500000 | room.ban | Carol | `c.banned = Alice` |
| 6 | 1710000600000 | room.join | Dave | — |

**Expected state after processing all 6 events**:

```
members = {Carol, Dave}
banned = {Alice}
```

Trace:
1. Alice joins → members = {Alice}
2. Bob joins → members = {Alice, Bob}
3. Carol joins → members = {Alice, Bob, Carol}
4. Bob leaves → members = {Alice, Carol}
5. Carol bans Alice → members = {Carol}, banned = {Alice}
6. Dave joins (not banned) → members = {Carol, Dave}

### E.10.2. Ban Prevents Rejoin

After the state from §E.10.1, if Alice attempts `room.join`:

```
Alice is in banned set → room.join MUST be rejected
```

### E.10.3. Unban Then Rejoin

Continuing from §E.10.1, add:

| seq | tc | Type | Inner d.n | Content |
|-----|-----|------|-----------|---------|
| 7 | 1710000700000 | room.unban | Carol | `c.unbanned = Alice` |
| 8 | 1710000800000 | room.join | Alice | — |

**Expected state**:

```
members = {Carol, Dave, Alice}
banned = {}
```

## E.11. Room Rule Evaluation

### E.11.1. Last Matching Rule Wins

Given rules (in order):

```json
[
  {"t": "post", "who": "*", "allow": true},
  {"t": "post", "who": "ALICE_SIG", "allow": false}
]
```

Evaluation for Alice posting:
1. Rule 1: `who=*` matches Alice → `allow=true`
2. Rule 2: `who=ALICE_SIG` matches Alice → `allow=false`
3. Last match wins → **DENY**

Evaluation for Bob posting:
1. Rule 1: `who=*` matches Bob → `allow=true`
2. Rule 2: `who=ALICE_SIG` does not match Bob
3. Last match wins → **ALLOW**

### E.11.2. Rate Limit Rule

Given rules:

```json
[
  {"t": "rate_limit", "who": "*", "max": 10, "window": 60000}
]
```

Alice has sent 10 messages in the last 60,000 ms. Her 11th message:
→ **DENY** (rate limit exceeded)

Alice has sent 9 messages in the last 60,000 ms. Her 10th message:
→ **ALLOW** (at limit, not over)

## E.12. Blob Pieces

### E.12.1. Piece Hashing

Given a 512KB blob (524,288 bytes) with piece_size = 262,144 (256KB):

```
piece_0 = blob_bytes[0..262143]        (262,144 bytes)
piece_1 = blob_bytes[262144..524287]   (262,144 bytes)

piece_0_hash = hex(SHA-256(piece_0))
piece_1_hash = hex(SHA-256(piece_1))
blob_hash = hex(SHA-256(piece_0 || piece_1))
```

The `room.blob` envelope's `c.pieces` array MUST equal
`[piece_0_hash, piece_1_hash]` and `c.hash` MUST equal `blob_hash`.

### E.12.2. Single-Piece Blob

A blob smaller than piece_size (e.g., 1,000 bytes with piece_size
262,144):

```
pieces = [SHA-256(blob_bytes)]
hash = SHA-256(blob_bytes)
pieces[0] == hash  // MUST be true for single-piece blobs
```

### E.12.3. Piece Verification Independence

Given piece_1 data received from a peer:

```
received_hash = SHA-256(received_piece_data)
expected_hash = room_blob.c.pieces[1]
assert received_hash == expected_hash
```

Each piece is verifiable without any other pieces.

## E.13. Base64url Encoding

### E.13.1. Encoding Test Cases

| Input (hex) | Base64url (no padding) |
|-------------|----------------------|
| (empty) | (empty) |
| `00` | `AA` |
| `ff` | `_w` |
| `00ff` | `AP8` |
| `deadbeef` | `3q2-7w` |
| 32 zero bytes | `AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA` (43 chars) |

Note: base64url uses `-` and `_` instead of `+` and `/`. Padding `=` is
always omitted.

### E.13.2. Key Length Verification

| Data | Raw bytes | Base64url length |
|------|-----------|-----------------|
| Ed25519 public key | 32 | 43 characters |
| Ed25519 signature | 64 | 86 characters |
| X25519 public key | 32 | 43 characters |
| SHA-256 hash | 32 | 43 characters (if base64url) or 64 characters (if hex) |

## E.14. Canonical Envelope Ordering

### E.14.1. Top-Level Field Order

The canonical form of an envelope `{d, p}` always has `d` before `p`
because `"d" < "p"` lexicographically:

```
{"d":{...},"p":"..."}
```

Never:

```
{"p":"...","d":{...}}
```

## E.15. Cross-Implementation Verification Procedure

To verify a new implementation against these test vectors:

1. **Key generation** (§E.2): Generate Ed25519 and X25519 keypairs from
   the specified seeds. Compare public keys against `[REF]` values.

2. **Canonical JSON** (§E.3): Canonicalize each test input. Compare
   output string byte-for-byte against expected output.

3. **Envelope signing** (§E.4): Construct each `d` object using derived
   public keys, canonicalize, sign. Compare signature against `[REF]`
   values.

4. **Commit wrapping** (§E.5): Wrap the depth-1 envelope. Verify both
   layers. Compare against `[REF]` values.

5. **Identity computation** (§E.6): Compute eid and datum_id. Compare.

6. **Private encryption** (§E.7): Encrypt with specified ephemeral key.
   Verify decryption recovers original plaintext. Compare intermediate
   values (shared secret, salt, key, nonce, ciphertext).


8. **PoW** (§E.9): Find winning nonce. Verify hash.

9. **Membership** (§E.10): Replay events, verify final state.

10. **Rules** (§E.11): Evaluate rules, verify allow/deny decisions.

11. **Blobs** (§E.12): Split, hash, verify pieces.

If all test vectors match, the implementation is compatible with the
Toloo v0.2 protocol.

## E.16. References

- [RFC8032] Edwards-Curve Digital Signature Algorithm (Ed25519)
- [RFC7748] Elliptic Curves for Security (X25519)
- [RFC8439] ChaCha20 and Poly1305 for IETF Protocols
- [RFC5869] HMAC-based Extract-and-Expand Key Derivation Function (HKDF)
- [RFC8785] JSON Canonicalization Scheme (JCS)
- [RFC4648] The Base16, Base32, and Base64 Data Encodings (Section 5: base64url)
