# 3. Cryptography

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts, [2] Data Model

## 3.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 3.2. Algorithm Selection

Toloo uses a fixed cryptographic suite. There is no algorithm negotiation.
All implementations MUST support these algorithms:

| Purpose | Algorithm | Reference |
|---------|-----------|-----------|
| Signing | Ed25519 | [RFC8032] |
| Encryption | X25519 + ChaCha20-Poly1305 | [RFC7748], [RFC8439] |
| Hashing | SHA-256 | [FIPS180-4] |

## 3.3. Signing (Ed25519)

### 3.3.1. Key Generation

On node creation, implementations MUST generate both keypairs:

1. **Ed25519 signing keypair** (as defined in [RFC8032] Section 5.1.5):
   - Private key (`sig_priv`): 32 bytes of cryptographically secure random data
   - Public key (`sig_pub`): 32 bytes derived via the Ed25519 algorithm

2. **X25519 encryption keypair** (as defined in [RFC7748] Section 6.1):
   - Private key (`enc_priv`): 32 bytes of cryptographically secure random data
   - Public key (`enc_pub`): 32 bytes derived via X25519

Both private keys MUST be generated using a cryptographically secure
random number generator (CSPRNG). The signing key MUST be used for the
lifetime of the node identity (signing keys are not rotatable). The
encryption key is permanent and published in every `node.meta` datum.

### 3.3.2. Signature Generation

Signatures are computed as defined in [RFC8032] Section 5.1.6.

Input:
- Message: The canonical JSON serialization of the `d` object (see [2#canonical-json])
- Private key: The signer's Ed25519 private key (32 bytes)

Output:
- Signature: 64 bytes

The signature is encoded as base64url without padding (86 characters) and
placed in the `p` field of the datum.

### 3.3.3. Signature Verification

Signatures are verified as defined in [RFC8032] Section 5.1.7.

Input:
- Message: The canonical JSON serialization of the `d` object
- Signature: The `p` field, decoded from base64url (64 bytes)
- Public key: The `d.n` field, decoded from base64url (32 bytes)

Output:
- Boolean: True if signature is valid, False otherwise

Nodes MUST reject datums where signature verification returns False.

### 3.3.4. Public Key Representation

Ed25519 public keys (32 bytes) are encoded as base64url without padding,
yielding 43-character strings. Examples:

```
xK4f7J2mN3pQ8rS9tU0vW1yZ2aB3cD4eF5gH6iJ7kL8m
A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2
```

### 3.3.5. Commit Envelope Signature

Room key holders commit events by wrapping them in a depth-2 envelope
(see [2#envelope-nesting], [6#committed-event-structure]). The room's
signature at depth 2 uses the same Ed25519 signing algorithm:

```
commit_d = {n: ROOM_SIG, v: "0.2", t: "commit", ts: ..., tc: ..., env: inner_envelope}
p = Ed25519.sign(room_private_key, utf8(canonicalize(commit_d)))
```

Verification uses the outer `d.n` (room public key):

```
Ed25519.verify(d.n, utf8(canonicalize(d)), p)
```

Since the inner envelope `{d, p}` is inside the commit `d` via the `env`
field, the room's signature cryptographically covers the entire inner
envelope. This is the same verification algorithm used at every nesting
level — no special countersignature computation is needed.

## 3.4. Transport Security

Transport confidentiality and integrity are provided by the carrier
protocol: `wss` and `https` run over TLS. Toloo defines no additional
transport-level encryption or handshake; envelope signatures remain the
authenticity mechanism on every carrier (see [9]).

The X25519 + HKDF-SHA256 + ChaCha20-Poly1305 primitive stack is used for
private message end-to-end encryption (§3.5.3) and encrypted-at-rest key
storage — not for transport frames.

## 3.5. Hashing (SHA-256)

### 3.5.1. Envelope Identity

Envelope identity is computed from the **innermost** `d` object (see
[2#envelope-identity]):

- Room envelopes: `eid = "{ch}:{ts}:{n}"` (from innermost `d`)
- Direct envelopes: `id = "{ts}:{n}"` (from innermost `d`)

SHA-256 is used for content hashing and Proof-of-Work, but not for
envelope identity. Envelope identity is deterministic and sortable
without hashing.

### 3.5.2. Content Hashing

Content hashes (e.g., for media files in future extensions) use SHA-256.

```
content_hash = SHA-256(content_bytes)
```

Output: 32 bytes (64-character lowercase hexadecimal string representation)

### 3.5.3. Private Message Encryption

Private messages use **static-static X25519** (payload version 2). The
sender's long-lived `enc` key and the recipient's published `enc` key
derive a pairwise key; a random 32-byte per-message salt makes every
message key unique. The same construction is used in both directions.

**Pair key derivation** (once per peer, cacheable):

```
shared   = X25519(own_enc_priv, peer_enc_pub)
pair_key = HKDF-SHA256(ikm=shared, salt="toloo-private-pair-v2",
                       info="pair", L=32)
```

**Encryption (sender)**:

```
salt      = 32 random bytes from a CSPRNG (per message)
salt_hash = SHA-256(sender_enc_pub || recipient_enc_pub || salt)
key||nonce = HKDF-SHA256(ikm=pair_key, salt=salt_hash,
                         info="toloo-private-v2", L=44)
ciphertext = ChaCha20-Poly1305(key, nonce, canonical(content_json), aad="")
```

**Decryption (recipient)**:

```
pair_key   = X25519(recipient_enc_priv, sender_enc_pub)  (or cached)
salt_hash  = SHA-256(sender_enc_pub || recipient_enc_pub || salt)
key||nonce = HKDF-SHA256(ikm=pair_key, salt=salt_hash,
                         info="toloo-private-v2", L=44)
content_json = ChaCha20-Poly1305.decrypt(key, nonce, ciphertext, aad="")
```

The envelope content is:

```
{
  "v": 2,
  "from": "<sender enc pub, 43-char base64url>",
  "salt": "<32 random bytes, 43-char base64url>",
  "encrypted": "<base64url ciphertext>"
}
```

Validation: missing `v` selects the legacy v1 payload below; `v` MUST be
`2` when present, and `from`, `salt`, and `encrypted` are REQUIRED.
`from` and `salt` MUST decode to 32 bytes. Implementations SHOULD cache
the pair key per peer (one X25519) and MUST generate a fresh random salt
for every message. The salt is public; its only requirement is uniqueness
within a pair key. Implementations MUST reject a v2 payload whose fields
are absent or malformed.

**Security properties.** This design is stateless and asynchronous: every
message is self-contained and can be decrypted in any order after any
delay. It provides **no forward secrecy**: someone who obtains a node's
static `enc` private key can decrypt every stored message sent to that
node; because the sender's static key is now used as well, the sender's
key compromise also reveals messages that node sent. A salt collision
under the same pair key is a nonce-reuse failure and MUST be treated as a
security bug.

**Legacy v1 (decrypt-only).** Version 1 payloads are
`{"eph": "<ephemeral X25519 pub>", "encrypted": "..."}` with
`salt = SHA-256(eph_pub || recipient_enc_pub)`, a fresh ephemeral
keypair per message, and HKDF info `"toloo-private-v0.2"`. v1 never
provided forward secrecy against compromise of the recipient's long-term
key (the ephemeral public key is stored with the ciphertext). New
messages MUST use v2; implementations MAY decrypt v1 for stored history.
Test vector §E.7 remains a v1 vector.

## 3.6. Proof-of-Work

Proof-of-Work (PoW) is used for anti-spam in rooms that enable it via the
`join_pow` or `post_pow` rules (see [5#rules]).

### 3.6.1. PoW Computation

To satisfy a PoW requirement with difficulty `d`:

1. Start with nonce = 0
2. Set `d.nonce` to nonce
3. Compute `hash = SHA-256(canonical(d))`
4. Count leading zero bits in hash
5. If `leading_zeros >= d`, PoW is satisfied; use this nonce
6. Else, increment nonce and repeat from step 2

The `nonce` field in `d` is a uint64, allowing up to 2^64 attempts.

### 3.6.2. PoW Verification

To verify PoW for a datum with difficulty requirement `d`:

1. Extract `d` from the datum
2. Compute `hash = SHA-256(canonical(d))`
3. Count leading zero bits in hash
4. If `leading_zeros >= d`, PoW is valid
5. Else, PoW is invalid; reject the datum

Verification is O(1) — a single hash operation. This makes PoW cheap to
verify but expensive to forge.

### 3.6.3. Leading Zero Bits

Leading zero bits are counted starting from the most significant bit of
the hash output:

```
hash bytes:  0x00 0x00 0x0F ...
binary:      00000000 00000000 00001111 ...
leading_zeros: 16 + 4 = 20
```

### 3.6.4. Difficulty Guidelines

PoW difficulty is specified as the number of leading zero bits required.
Higher values increase computational cost exponentially:

| Difficulty | Average Hashes | Approximate Time (2023 consumer CPU) |
|------------|----------------|--------------------------------------|
| 12 | 4,096 | 0.5 seconds |
| 16 | 65,536 | 1 second |
| 20 | 1,048,576 | 5 seconds |
| 24 | 16,777,216 | 1 minute |

These are guidelines, not protocol requirements. Rooms MAY set any
difficulty value. Higher difficulties provide stronger Sybil resistance
but increase user wait time.

## 3.7. Key Storage

### 3.7.1. Private Key Security

Implementations MUST store private keys securely:
- Ed25519 signing private key (`sig_priv`): Permanent node identity
- X25519 encryption private key (`enc_priv`): Permanent, for private message decryption
- Room Ed25519 private keys (`room_priv`, if key holder): Commit authority

Private keys SHOULD be encrypted at rest using a user-provided passphrase
or platform-specific secure storage (e.g., OS keychain).

Private keys MUST NOT be transmitted over the network except when
explicitly exporting a node identity (user-initiated action).

### 3.7.2. Public Key Distribution

Public keys are distributed via:
- Ed25519 signing public key (`sig_pub`): In every datum's `d.n` field
- X25519 encryption public key (`enc_pub`): In `node.meta` content field `c.enc`
- Room Ed25519 public key: In room event field `d.r`

Signing keys define identity. The `c.enc` key enables private messaging.

## 3.8. Trust On First Use (TOFU)

Toloo uses TOFU for node identity:

1. When node A first encounters node B's public key (`d.n`), A records it
2. All future datums from node B MUST use the same `d.n` value
3. If a datum claims to be from node B but has a different `d.n`, A rejects it

TOFU is enforced at the protocol level. Once a node has been "seen" with
a particular public key, that key-identity binding is permanent for that
observing node.

Key rotation is not supported. A compromised key requires creating a new
node identity and migrating memberships out-of-band.

## 3.9. Cryptographic Agility

Toloo does not support cryptographic agility. The algorithm suite is fixed.
Future protocol versions (if any) that require different algorithms will
use a different protocol version field (`v`) and will be explicitly
incompatible.

This design choice prioritizes simplicity and reduces the attack surface
from algorithm downgrade attacks.

## 3.10. Security Considerations

### 3.10.1. Key Compromise

If a node's Ed25519 private key is compromised, all datums signed with that
key are compromised. There is no revocation mechanism. The compromised node
must create a new identity and announce the migration out-of-band.

### 3.10.2. Weak Randomness

Key generation depends on cryptographically secure random number generation.
Weak or predictable randomness compromises security. Implementations MUST
use platform-provided CSPRNGs (e.g., `/dev/urandom` on Unix, `CryptGenRandom`
on Windows, `crypto.getRandomValues()` in browsers).

### 3.10.3. Side-Channel Attacks

Implementations SHOULD use constant-time cryptographic operations to resist
timing side-channel attacks. Most modern Ed25519 and X25519 libraries
provide constant-time implementations by default.

## 3.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC8032] Edwards-Curve Digital Signature Algorithm (EdDSA)
- [RFC7748] Elliptic Curves for Security (X25519)
- [RFC8439] ChaCha20 and Poly1305 for IETF Protocols
- [FIPS180-4] Secure Hash Standard (SHS) - SHA-256
