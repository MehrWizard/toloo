# 2. Data Model

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts

## 2.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 2.2. The Envelope

All data in Toloo is encapsulated in an **envelope**. An envelope is a
JSON object with exactly two fields:

```json
{
  "d": { ... },
  "p": "BASE64URL_SIGNATURE"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `d` | object | Signed data — contains all content, metadata, version, and optionally nested envelopes |
| `p` | string | Ed25519 signature over `canonical(d)`, base64url-encoded without padding (86 characters) |

An envelope MUST contain exactly `d` and `p`. No other top-level fields.
Nodes MUST reject envelopes that violate this structure.

The signer's public key is always `d.n`. Verification is:

```
Ed25519.verify(canonical(d), base64url_decode(p), d.n) → true or reject
```

This is the only verification algorithm in the protocol. It applies
uniformly at every nesting level.

## 2.3. The Data Object (d)

The `d` object contains everything the signer commits to. Its structure
varies by envelope type, but certain fields are common.

### 2.3.1. Common Fields

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `n` | string | REQUIRED | Signer's Ed25519 public key, base64url (43 characters) |
| `v` | string | REQUIRED | Protocol version this layer was created with (e.g., "0.2") |
| `t` | string | REQUIRED | Envelope type (see [6] Events, [A] Registries) |
| `ts` | uint64 | REQUIRED | Timestamp when created, Unix milliseconds |

### 2.3.2. Addressing Fields

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `r` | string | OPTIONAL | Target room's Ed25519 public key, base64url (43 characters) |
| `to` | string | OPTIONAL | Target node's Ed25519 public key, base64url (43 characters) |

An envelope with `r` is a **room envelope** (targeting a room).
An envelope with `to` is a **direct envelope** (targeting a specific node).
For private messages, `to` identifies the recipient. Private envelopes
MUST NOT have `r`.
An envelope MAY have both `r` and `to` (e.g., sending a room event to a
specific relay that can reach the room key holder).

### 2.3.3. Content Fields

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `c` | object | OPTIONAL | Type-specific content (structure depends on `t`) |
| `nonce` | uint64 | OPTIONAL | Proof-of-Work nonce (see [3#pow]) |
| `exp` | uint64 | OPTIONAL | Absolute expiry timestamp, Unix milliseconds |

### 2.3.4. Nesting Field

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `env` | object | OPTIONAL | The previous envelope `{d, p}` being wrapped |

When `env` is present, this envelope is a **response** to the inner
envelope. The `env` field contains a complete `{d, p}` object. Since `env`
is inside `d`, it is covered by the outer signature `p`.

See §2.7 for nesting rules.

### 2.3.5. Commit Fields

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `tc` | uint64 | OPTIONAL | Commit timestamp, Unix milliseconds. Used by room key holders when committing events. |

## 2.4. Canonical JSON

To enable deterministic signatures, Toloo uses **canonical JSON** as
defined in [RFC8785]. Canonical JSON transformation MUST be applied to
the `d` object before computing or verifying signatures.

Canonical JSON requirements:
- Object keys MUST be sorted lexicographically by UTF-16 code units
- No insignificant whitespace
- Unicode characters MUST be escaped as `\uXXXX` where required by RFC8785
- Numbers MUST NOT have leading zeros (except the value 0 itself)
- No trailing decimal points

### 2.4.1. Signature Computation

```
1. Construct the d object with all fields
2. Apply canonical JSON: canonical_d = canonicalize(d)
3. Encode as UTF-8: bytes = utf8(canonical_d)
4. Sign: p = Ed25519.sign(bytes, signer_private_key)
5. Encode: p_string = base64url_encode_nopad(p)
6. Envelope = {"d": d, "p": p_string}
```

### 2.4.2. Signature Verification

```
1. Extract d and p from envelope
2. Apply canonical JSON: canonical_d = canonicalize(d)
3. Encode as UTF-8: bytes = utf8(canonical_d)
4. Decode signature: p_bytes = base64url_decode(p)
5. Extract signer: pubkey = d.n
6. Verify: Ed25519.verify(bytes, p_bytes, pubkey) → boolean
7. If verification fails, reject the envelope
8. If d.env exists, verify d.env recursively (same algorithm)
```

## 2.5. Envelope Identity

Every envelope has a deterministic identity computed from its innermost
`d` object. The identity is never stored inside the envelope.

### 2.5.1. Room Envelope Identity (eid)

For envelopes targeting a room (`d.r` present on the innermost layer):

```
eid = "{ch}:{ts}:{n}"
```

Where (all from the innermost `d`):
- `ch`: Channel number from `d.c.ch` (default 0 if absent)
- `ts`: Creation timestamp `d.ts`
- `n`: Author's public key `d.n`

Example:
```
0:1710000000000:xK4f7J2mN3pQ8rS9tU0vW1yZ2aB3cD4eF5gH6iJ7kL8m
```

Room eids contain two `:` separators.

### 2.5.2. Direct Envelope Identity

For envelopes without `d.r` on the innermost layer:

```
id = "{ts}:{n}"
```

Where (from the innermost `d`):
- `ts`: Creation timestamp `d.ts`
- `n`: Author's public key `d.n`

Example:
```
1710000000000:xK4f7J2mN3pQ8rS9tU0vW1yZ2aB3cD4eF5gH6iJ7kL8m
```

Direct envelope ids contain one `:` separator.

### 2.5.3. Identity Properties

The `:` character does not appear in base64url encoding or decimal
timestamps, making it an unambiguous separator. Counting separators
distinguishes room envelopes (two `:`) from direct envelopes (one `:`).

Envelope identities are:
- Deterministic (same innermost `d` always produces the same identity)
- Sortable (lexicographic sort orders by channel/time/author)
- Stable across depths (wrapping does not change the identity)

### 2.5.4. Datum ID (Content Hash)

A **datum_id** is the SHA-256 hash of the complete canonical envelope
JSON, used for deduplication in pool exchange and system fetch:

```
datum_id = hex(SHA-256(canonical({"d": d, "p": p})))
```

The datum_id is a 64-character lowercase hex string. Unlike `eid` (which
is computed from the innermost `d` and is stable across depths), the
datum_id changes when an envelope is wrapped at a deeper depth. This
makes datum_id useful for identifying specific envelope versions during
replication, while eid is used for logical event identity.

| Identifier | Computed From | Stable Across Depths | Purpose |
|------------|---------------|---------------------|---------|
| `eid` | Innermost `d` fields | Yes | Logical event identity |
| `datum_id` | Entire envelope | No | Dedup during replication |

### 2.5.5. Uniqueness Constraint

An author (`d.n`) MUST NOT create more than one envelope with the same
`d.ts` value in the same channel (for room envelopes) or the same `d.ts`
value (for direct envelopes). If a node needs to create multiple
envelopes in the same millisecond, it MUST increment `d.ts` by 1 for
each subsequent envelope.

## 2.6. Innermost Envelope

The **innermost envelope** is found by traversing `d.env` recursively
until reaching an envelope with no `d.env` field.

```
function innermost(envelope):
  if envelope.d.env exists:
    return innermost(envelope.d.env)
  return envelope
```

The innermost envelope determines:
- **Identity**: computed from innermost `d` (§2.5)
- **Addressing**: `d.r` and/or `d.to` on the innermost layer define the
  intended receiver
- **Allowed signers**: only innermost `d.n` and the receiver (`d.to` or
  `d.r`) may sign any layer (§2.8)

## 2.7. Envelope Nesting

Envelopes nest via the `d.env` field. Each wrapping layer adds a new
signer's response on top of the previous envelope.

### 2.7.1. Nesting Rules

1. `d.env` MUST be a valid envelope (`{d, p}` with valid signature)
2. Maximum nesting depth is 3 (the outermost envelope contains at most
   two levels of `d.env`)
3. Signers MUST alternate between the two allowed parties (§2.8)
4. Each layer's `p` covers the entire inner envelope because `env` is
   inside `d`
5. The inner envelope is immutable — the wrapping signer MUST NOT modify
   any field of the inner `{d, p}`

### 2.7.2. Depth and Lifecycle

| Depth | Structure | State | Network Action |
|-------|-----------|-------|----------------|
| 1 | `{d, p}` | **Created** — sender sent, receiver hasn't seen | Forward toward `d.to` or `d.r` |
| 2 | `{d, p}` with `d.env` | **Responded** — receiver processed, sender doesn't know | Forward toward innermost `d.n` |
| 3 | `{d, p}` with `d.env.d.env` | **Resolved** — sender confirmed receipt | Stop forwarding, archive |

Nesting depth tells any node exactly what to do with an envelope
without inspecting any fields beyond the structure.

### 2.7.3. Wrapping an Envelope

To wrap an existing envelope with a response:

```
1. Let inner = the existing envelope {d, p}
2. Construct new d:
   - n: responder's public key
   - v: responder's protocol version
   - t: response type (e.g., "commit", "ack", type-specific)
   - ts: current timestamp
   - env: inner (the complete {d, p} being wrapped)
   - (any additional response-specific fields, e.g., tc)
3. Sign: p = Ed25519.sign(canonical(new_d), responder_private_key)
4. New envelope = {"d": new_d, "p": p}
```

### 2.7.4. Superseding

When a node receives an envelope at a greater depth than a version it
already has (same identity per §2.5), it MUST replace the shallower
version with the deeper one. Depth-2 supersedes depth-1. Depth-3
supersedes depth-2.

Nodes identify matching envelopes by their identity, which is computed
from the innermost `d` and is stable across depths.

## 2.8. Signer Validation

Only two parties may sign layers of an envelope:

1. The **sender**: `d.n` on the innermost layer
2. The **receiver**: `d.to` or `d.r` on the innermost layer

### 2.8.1. Validation Algorithm

```
1. Traverse to innermost envelope
2. Extract allowed signers:
   - sender = innermost.d.n
   - receiver = innermost.d.to or innermost.d.r
3. For each layer (innermost to outermost):
   - Verify layer.d.n is either sender or receiver
   - If not, reject entire envelope (bad actor)
4. Verify signers alternate:
   - Depth 1 (innermost): sender
   - Depth 2: receiver
   - Depth 3: sender
```

### 2.8.2. Bad Actor Detection

If an envelope contains a layer signed by a node that is neither the
sender nor receiver, the entire envelope MUST be rejected. The
offending signer (`d.n` on the invalid layer) SHOULD be reported via
attestation (see [8] Trust).

## 2.9. Timestamps and Expiry

### 2.9.1. Timestamps

All timestamps are Unix time in milliseconds (uint64).

- `d.ts`: When the signer created this layer
- `d.tc`: When the room key holder committed the event (depth-2 layer
  of room envelopes)

Timestamps are metadata, not validity criteria. Nodes MUST NOT reject
otherwise-valid envelopes based on `d.ts` alone. Forwarding and storage
decisions use `d.exp` and room rules (`uncommitted_ttl`, `retention`).

### 2.9.2. Expiry (exp)

The `d.exp` field is an absolute expiry timestamp. Behavior:

- `now < d.exp`: Envelope is **active** — nodes SHOULD forward/announce
- `now >= d.exp`: Envelope is **expired** — nodes MUST stop active
  forwarding but MAY serve on explicit request
- Nodes MAY garbage collect expired + resolved envelopes per local policy
- Resolved envelopes (depth 3) MAY be garbage collected faster than
  unresolved ones

Each layer has its own `exp` (or none). The outermost layer's `exp`
governs the envelope's active forwarding lifetime. A responder MAY set
a longer `exp` than the original sender (e.g., 5-minute request wrapped
in a 90-day response).

An envelope without `exp` on any layer has no protocol-level expiry.
Its lifetime is governed by room rules or node storage policy.

## 2.10. Canonical JSON for Nested Envelopes

When computing `canonical(d)` for a `d` object that contains `env`,
the `env` value is canonicalized recursively. Since `env` is a JSON
object inside `d`, RFC8785 canonicalization applies to it naturally —
all keys sorted, no whitespace, including nested objects.

The canonical form is deterministic at all depths.

## 2.11. Base64url Encoding

All binary data is encoded using base64url ([RFC4648] Section 5) with
padding characters omitted.

| Data | Bytes | Encoded Length |
|------|-------|----------------|
| Ed25519 public key | 32 | 43 characters |
| Ed25519 signature | 64 | 86 characters |
| X25519 public key | 32 | 43 characters |

Nodes MUST reject envelopes containing base64url strings that do not
decode to the expected byte length.

## 2.12. Field Size Limits

| Field | Type | Min | Max | Notes |
|-------|------|-----|-----|-------|
| `n` | string | 43 | 43 | Ed25519 public key |
| `v` | string | 1 | 16 | Protocol version |
| `t` | string | 1 | 64 | Alphanumeric + dot + underscore |
| `ts`, `tc`, `exp` | uint64 | 0 | 2^53-1 | JavaScript safe integer range |
| `nonce` | uint64 | 0 | 2^53-1 | PoW nonce |
| `r`, `to` | string | 43 | 43 | Ed25519 public key |
| `c` (content) | object | — | 8192 bytes | Serialized JSON size |
| `d` (single layer) | object | — | 16384 bytes | Excluding `env` content |
| `p` | string | 86 | 86 | Ed25519 signature |

The size limit on `d` applies per layer (excluding the nested `env`
content). Total envelope size including all nesting MUST NOT exceed
49152 bytes (3 x 16384).

## 2.13. Type Validation

Nodes MUST validate received envelopes:

1. **Structure**: Envelope has exactly two fields (`d`, `p`)
2. **Field types**: `d` is object, `p` is 86-character string
3. **Common fields**: `d.n` (43 chars), `d.v` (string), `d.t` (string),
   `d.ts` (uint64) all present
4. **Signature**: Verify `p` against `canonical(d)` using `d.n`
5. **Nesting**: If `d.env` present, validate it recursively (same rules)
6. **Signer validation**: All layers signed by allowed parties (§2.8)
7. **Depth limit**: No more than 3 levels
8. **Size limits**: Per-layer and total size within bounds
9. **Type-specific**: Validate fields for `d.t` (see [6] Events,
   [A] Registries)

If any check fails, reject the entire envelope.

## 2.14. Unknown Fields

Nodes encountering unknown fields within `d` MUST preserve them when
forwarding (do not strip unknown fields). This allows protocol
extensions without breaking existing implementations.

Nodes MUST NOT reject an envelope solely because `d` contains unknown
fields.

## 2.15. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC4648] The Base16, Base32, and Base64 Data Encodings (Section 5)
- [RFC8785] JSON Canonicalization Scheme (JCS)
- [RFC8032] Edwards-Curve Digital Signature Algorithm (Ed25519)
