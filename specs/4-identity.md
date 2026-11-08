# 4. Identity

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts, [2] Data Model, [3] Cryptography

## 4.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 4.2. Node Identity

A node is identified by two permanent keypairs:

- **Ed25519 signing keypair** (`sig_pub` / `sig_priv`): The signing
  public key is the node's identity. It is permanent and non-rotatable,
  represented as a 43-character base64url string (see [2#base64url]).

- **X25519 encryption keypair** (`enc_pub` / `enc_priv`): Used for
  private message encryption in both directions. The public key is
  published in `node.meta` and carried in each v2 private payload's
  `c.from`; the private key derives the pairwise key with each peer.
  Generated alongside the signing keypair when a node is created.

Both keypairs are generated together at node creation. Losing the signing
private key means losing the node identity permanently. There is no key
recovery or rotation mechanism.

## 4.3. Node Metadata

Every node publishes a signed metadata datum describing itself:

```json
{
  "d": {
    "n": "NODE_SIG",
    "v": "0.2",
    "t": "node.meta",
    "ts": 1710000000000,
    "c": {
      "enc": "NODE_X25519_PUB",
      "endpoints": [
        {
          "proto": "https",
          "host": "example.com",
          "port": 443,
          "path": "/toloo",
          "direct": true
        },
        {
          "proto": "ws",
          "host": "203.0.113.10",
          "port": 7700
        }
      ],
      "name": "Alice's Node",
      "bio": "Optional description",
      "avatar": "CONTENT_HASH"
    }
  },
  "p": "SIGNATURE"
}
```

### 4.3.1. Metadata Fields

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `n` | string | REQUIRED | Node's Ed25519 signing public key |
| `t` | string | REQUIRED | Fixed value "node.meta" |
| `ts` | uint64 | REQUIRED | Timestamp when metadata was created/updated |
| `c.enc` | string | REQUIRED | Node's X25519 public key for private messages (base64url, 43 chars) |
| `c.endpoints` | array | OPTIONAL | List of endpoint descriptors (connection options) |
| `c.name` | string | OPTIONAL | Human-readable node name |
| `c.bio` | string | OPTIONAL | Optional description |
| `c.avatar` | string | OPTIONAL | Content hash of avatar blob |

Identity is defined only by `n` (and signatures over `d`). Transport
configuration in `c.endpoints[]` does not change node identity.

The `c.enc` field is the node's long-lived X25519 public key for private
message encryption (see [3#private-message-encryption]). It is distinct
from `c.endpoints[].key`, which is a per-endpoint transport encryption key.
Every node MUST publish `c.enc` in its `node.meta`.

### 4.3.2. Endpoint Descriptor (`c.endpoints[]`)

Each `c.endpoints` entry is a structured object describing a reachable
transport endpoint.

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `proto` | string | REQUIRED | Transport protocol identifier (see [9#registered-protocols]) |
| `host` | string | REQUIRED* | Hostname or IP address (* required for most protos) |
| `port` | integer | REQUIRED* | Port number (* required for most protos) |
| `path` | string | OPTIONAL | URL path for HTTP/WS protos (default `/`) |
| `direct` | boolean | OPTIONAL | Key-holder routing hint (default `false`) |

Defaults for omitted fields:

```json
{
  "proto": "",
  "host": "",
  "port": 0,
  "path": "/",
  "direct": false
}
```

Endpoint descriptor validation rules:
- `proto` MUST be a recognized transport protocol identifier.
- If the proto requires `host` and `port`, they MUST be present.
- Implementations MUST ignore descriptors with unrecognized `proto` values.
- Browser implementations SHOULD only advertise HTTP/WS-compatible protos
  (`ws`, `wss`, `http`, `https`).
- Unknown fields in endpoint descriptors MAY be preserved but MUST NOT change
  these semantics.

Endpoint descriptor examples:

WebSocket over TLS (browser + native):
```json
{"proto": "wss", "host": "relay.example.com", "port": 443, "path": "/toloo"}
```

Plain HTTPS (browser-friendly, TLS):
```json
{"proto": "https", "host": "api.example.com", "port": 443, "path": "/toloo"}
```

Plain WebSocket (trusted LAN, no TLS):
```json
{"proto": "ws", "host": "192.168.1.10", "port": 7700}
```

### 4.3.3. Public vs NAT'd Reachability

A node is considered **publicly reachable** if it advertises at least one
valid endpoint descriptor in `via[]` that accepts inbound connections.

A node is considered **NAT'd** if it has no such direct endpoint. NAT'd
nodes SHOULD advertise relay/proxy endpoint descriptors in `via`.

Endpoint descriptors are transport instances, not identities. A single
endpoint service MAY carry traffic for multiple node identities.

### 4.3.4. User Metadata

The `meta` object is optional and free-form. Common fields include:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Display name (max 256 characters) |
| `bio` | string | Short biography (max 1024 characters) |
| `avatar` | string | Content hash of avatar image (64-character hex) |

The entire `meta` object MUST NOT exceed 4096 bytes when serialized as JSON.
Implementations MUST treat `meta` as profile/user metadata only.
Transport configuration MUST be expressed in `via[]` endpoint descriptors,
not in `meta`.

Nodes MAY ignore unknown fields in `meta`. Nodes MUST NOT reject metadata
solely because `meta` contains unknown fields.

## 4.4. Trust On First Use (TOFU)

Nodes implement Trust On First Use for peer identity:

1. When node A first sees a datum signed by `n = NODE_SIG`, A records this binding
2. A MUST store: `NODE_SIG → first_seen_timestamp`
3. All future datums claiming to be from this node MUST have `d.n == NODE_SIG`
4. If a datum has a different `d.n` but claims the same identity, A MUST reject it

TOFU pins are permanent. Once a node is "known," its public key cannot change.

Nodes SHOULD persist TOFU pins across restarts to prevent impersonation attacks.

## 4.5. Toloo URI Scheme and Data Files

The `toloo://` URI and `.toloo` file are the same format: one base64url-
encoded signed envelope per line. In URIs, `.` replaces `\n` as the line
separator. Every payload is signed envelopes — the same `{d, p}` used
everywhere. No special config format exists.

### 4.5.1. Format

```
URI:   toloo://BASE64URL_ENVELOPE[.BASE64URL_ENVELOPE...]
File:  BASE64URL_ENVELOPE\n[BASE64URL_ENVELOPE\n...]
```

Each line (or `.`-separated segment) is a base64url-encoded signed
envelope (`{d, p}`). To parse either format:

```
1. If input starts with "toloo://", strip prefix and replace "." with "\n"
2. Split on "\n"
3. Skip empty lines and lines starting with "#" (comments, files only)
4. Base64url-decode each line (no padding)
5. Parse each decoded line as JSON
6. Validate each as a signed envelope ({d, p})
7. Verify signature p over canonical(d) using d.n
```

The first envelope is the **primary intent** — its `d.t` determines how
the client interprets the data:

| First `d.t` | Intent | Example Use |
|-------------|--------|-------------|
| `node.meta` | Peer connection config | "Connect to this peer" |
| `commit` (wrapping `room.create`) | Room invite | "Join this room" |
| Any other | Generic envelope share | Application-defined |

Subsequent envelopes provide supporting context (peer endpoints, room
metadata, etc.).

### 4.5.2. Examples

Peer connection config (single `node.meta` envelope):

```
toloo://eyJkIjp7Im4iOiJBTElDRV9TSUciLC...
```

Room invite (committed `room.create` + peer `node.meta`):

```
toloo://eyJkIjp7...room_create_commit...fQ.eyJkIjp7...peer_node_meta...fQ
```

Same data as a `.toloo` file:

```
# Room invite — My Room
eyJkIjp7...room_create_commit...fQ
eyJkIjp7...peer_node_meta...fQ
```

### 4.5.3. Toloo Data Files

`.toloo` files use newlines instead of dots. They support comments and
have no size limit:

```
# Toloo data file — comments start with #
eyJkIjp7Im4iOiJBTElDRV9TSUciLC...
eyJkIjp7Im4iOiJCT0JfU0lHIiwidjoi...
eyJkIjp7Im4iOiJST09NX1NJRyIsInYi...
```

- File extension: `.toloo`
- MIME type: `application/x-toloo`
- One base64url-encoded envelope per line
- Lines starting with `#` are comments (ignored)
- Empty lines are ignored
- No size limit — a few KB (peer config) to gigabytes (pool export)

### 4.5.4. URI Generation

```
1. For each envelope: base64url(canonical({"d": ..., "p": ...}))
2. Join segments with "."
3. Prepend "toloo://"
```

### 4.5.5. Bootstrap Scenarios

| Scenario | Format | Contents |
|----------|--------|----------|
| "Connect to me" | URI | `node.meta` envelope |
| "Join this room" | URI | `room.create` commit + peer `node.meta` |
| "Here's the network" | `.toloo` file | Multiple `node.meta` + room creates |
| "Full room backup" | `.toloo` file | All envelopes for a room |
| "Sneakernet sync" | `.toloo` file | Pool export (any size) |

Peers also distribute configs organically through `pool.exchange` — a
node's pool naturally accumulates `node.meta` envelopes for known peers,
serving as a backup connection directory (see [12#pool-gossip]).

## 4.6. Node Discovery

Nodes discover peers through:

1. **Pre-bundled seed nodes**: Hardcoded in client software
2. **toloo:// URIs / .toloo files**: Shared out-of-band (QR codes, links, config files)
3. **Pool gossip**: Nodes advertise their metadata via `node.meta` datums
4. **Room metadata**: Rooms include creator and optional `via[]` endpoint descriptors in metadata

No mechanism is mandatory. Implementations MAY support any combination.

## 4.7. First Contact

First contact begins by connecting to an endpoint descriptor from `via[]`.
The `proto` field determines the transport carrier. Confidentiality depends
on the carrier (TLS for `wss`/`https`); plain carriers (`ws`, `http`) provide
none and are appropriate only on trusted networks.

## 4.8. Identity Verification

Node identity is verified via signatures on datums. Every datum includes
`d.n` (the signer's public key) and `p` (the signature). Verification is
automatic:

```
1. Extract d.n from datum
2. Verify p against canonical(d) using d.n
3. If verification fails, reject datum
4. If verification succeeds, accept datum as authored by d.n
```

This provides cryptographic proof of authorship. No additional verification
mechanism is needed at the protocol level.

Out-of-band verification (e.g., comparing key fingerprints in person) is
outside the scope of this specification but is encouraged for high-security
scenarios.

## 4.9. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC3986] Uniform Resource Identifier (URI): Generic Syntax
