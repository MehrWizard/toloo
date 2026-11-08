# 9. Transport

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [4] Identity, [10] Endpoints

## 9.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 9.2. Carrier and Endpoint Model

Toloo separates:
- **Datum security**: envelope signatures (`p` at each nesting depth) prove authenticity.
- **Transport security**: TLS provided by the carrier protocol (`wss`, `https`).

Nodes connect using structured endpoint descriptors from `node.meta` content (`c.endpoints[]`):

```json
{
  "proto": "wss",
  "host": "relay.example.com",
  "port": 443,
  "path": "/toloo"
}
```

Endpoint descriptor parsing and defaulting follow [4#4.3.2].
Transport behavior uses the normalized values:
- `proto` determines the transport carrier.
- `direct` is a routing hint only (never an identity claim).
- Descriptors with unrecognized `proto` values MUST be ignored.

### 9.2.1. Registered Transport Protocols

| Proto | Transport | Built-in Encryption | Browser | Required Fields |
|-------|-----------|-------------------|---------|-----------------|
| `ws` | WebSocket | No | Yes | `host`, `port` |
| `wss` | WebSocket + TLS | TLS | Yes | `host`, `port` |
| `http` | HTTP | No | Yes | `host`, `port` |
| `https` | HTTP + TLS | TLS | Yes | `host`, `port` |

Future transport protocols MAY be added via registry extension.

## 9.3. Transport Policy

- `wss` and `https` are RECOMMENDED as defaults: TLS provides transport
  confidentiality and server authentication, and self-signed certificates
  combined with TOFU pinning (see [11#11.3.1]) cover deployments without a
  certificate authority.
- Plain `ws` and `http` are acceptable on trusted networks (e.g. LAN) and
  behind a user-configured local tunnel.
- Connections MAY be routed through a SOCKS5 proxy. Proxying is a
  deployment-level concern (client configuration); it requires no protocol
  support and is invisible to the wire format.

This specification does not force a global "always encrypted transport"
policy; implementations define local policy for endpoint acceptance.

## 9.4. Message Framing

Frames are carried directly on the carrier channel. There is no
application-layer encryption layer:

- **WebSocket**: Each WebSocket message is one frame. Text messages carry
  JSON (envelopes or errors). Binary messages carry binary pieces:
  first byte `0x01`, then 32-byte SHA-256 piece hash, then piece data.
- **HTTP**: Request/response bodies carry JSON. Binary piece transfers use
  `Content-Type: application/octet-stream` with the 32-byte piece hash
  prepended.

The receiver of a binary piece verifies `SHA-256(piece_data) == piece_hash`.
See [11#blob-piece-storage] for the piece model.

## 9.5. Node Introduction

Nodes MUST identify themselves by signed protocol datums (`d.n`, `p`) before
privileged operations are accepted.

Endpoints are transport instances and MUST NOT be treated as identity.
One endpoint MAY serve multiple node identities.

## 9.6. Connection Lifecycle

- No session lifecycle datums are defined.
- Either side may close transport when done.
- Implementations MAY define idle timeouts and keepalives.

## 9.7. Error Handling

Application errors SHOULD use JSON objects:

```json
{"error":"permission_denied","message":"Not a member of this room"}
```

## 9.8. Security Considerations

### 9.8.1. Carrier Choice

TLS-based protos (`wss`, `https`) reduce passive observation compared to
plain protos (`ws`, `http`). Using plain protos is a deployment policy
decision, not a protocol violation.

### 9.8.2. Authentication Source of Truth

Datum signatures, not transport endpoints, are the authoritative identity
proof in Toloo.

### 9.8.3. DPI and Fingerprinting

TLS reduces content visibility but does not eliminate traffic analysis
based on IP/port, timing, and packet size patterns.

Operational recommendations for hostile networks:
- Use `wss`/`https` so carrier traffic is indistinguishable from ordinary
  TLS traffic
- Route connections through a SOCKS5 proxy to hide the destination IP
- Rotate endpoint descriptors periodically
- Use multiple geographically diverse endpoints

## 9.9. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC8032] Edwards-Curve Digital Signature Algorithm (EdDSA)
- [RFC8439] ChaCha20 and Poly1305 for IETF Protocols
- [RFC6455] The WebSocket Protocol
- [RFC1928] SOCKS Protocol Version 5
