# Appendix D: Conformance

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md)

## D.1. Purpose

This appendix defines conformance requirements for Toloo implementations.
It distinguishes between mandatory features (MUST implement) and optional
features (MAY implement).

## D.2. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## D.3. Conformance Levels

Toloo defines three conformance levels:

| Level | Description | Use Cases |
|-------|-------------|-----------|
| **Minimal** | Read-only participation | Observers, archival nodes |
| **Client** | Full read/write participation | End users, mobile apps |
| **Relay** | Accept connections from others | Public nodes, infrastructure |

## D.4. Minimal Conformance

A **minimally conformant node** can:
- Verify datum signatures
- Parse canonical JSON
- Sync from other nodes (pool.exchange)
- Store received datums
- Interpret room rules

### D.4.1. Required Capabilities

**Cryptography**:
- Ed25519 keypair generation and signature verification (RFC 8032)
- X25519 keypair generation (RFC 7748) — every node MUST generate an `enc` keypair
- SHA-256 hashing (FIPS 180-4)
- X25519 key agreement (RFC 7748) — for private message encryption
- ChaCha20-Poly1305 AEAD (RFC 8439) — for private message encryption
- HKDF-SHA256 (RFC 5869) — for private message key derivation

**Data Model**:
- Canonical JSON parsing (RFC 8785)
- Envelope structure validation (`d`, `p` — exactly two fields, `v` inside `d`)
- Event ID (eid) computation
- Base64url encoding/decoding (RFC 4648 Section 5)

**Protocol**:
- Endpoint connection handling for advertised `via[]` descriptors (MUST support `wss` protos)
- Pool exchange (pool.exchange request/response)
- Room sync (room.sync, events.sync)
- Error handling (all error codes in [A#error-codes])

**Storage**:
- Store received datums indexed by eid
- Maintain membership state (channel -2)
- Maintain room metadata (channel -1)

### D.4.2. Optional for Minimal Nodes

- Proof-of-Work computation (read-only nodes don't send events)
- Committing (no room authority)
- Relay mode (don't accept incoming connections)

### D.4.3. Example: Archival Node

An archival node operates at minimal conformance:
- Connects to public relays
- Syncs all rooms it knows about
- Stores full history
- Does not send events or accept connections

## D.5. Client Conformance

A **client conformant node** extends minimal conformance with:
- Event creation and signing
- Room joining and participation
- Local storage management

### D.5.1. Additional Required Capabilities

**Key Management**:
- Ed25519 signing keypair generation
- X25519 encryption keypair generation (both created at node creation)
- Secure key storage (encrypted at rest)
- Key export/import

**Event Creation**:
- Sign events with Ed25519 (d.n field)
- Compute Proof-of-Work (if room rules require)
- Create all standard event types (room.message, room.join, etc.)

**Room Management**:
- Join rooms (room.join flow)
- Send messages (room.message)
- React, edit, delete (room.react, room.edit, room.delete)
- Leave rooms (room.leave)

**Storage Management**:
- Prune old events (respect retention rules)
- Export room data
- Import room data

### D.5.2. Recommended Features

**User Experience**:
- TOFU pin management (verify, update, clear)
- Attestation display (show peer reputation)
- Offline mode (queue uncommitted events)
- Notifications (new messages, mentions)

**Discovery**:
- toloo:// URI and .toloo file parsing
- QR code scanning (for URI import)

**Moderation**:
- Local blocklist/spam policy controls
- Flag uncommitted events (room.flag)

### D.5.3. Example: Mobile Client

A mobile client operates at client conformance:
- Generates keypair on first launch
- Joins rooms via invitations
- Sends/receives messages
- Handles network changes gracefully
- Prunes old messages to save storage

## D.6. Relay Conformance

A **relay conformant node** extends client conformance with:
- Accept incoming connections
- Forward uncommitted events to key holders
- Participate in pool gossip

### D.6.1. Additional Required Capabilities

**Transport**:
- WebSocket server (required; `ws` plain, `wss` over TLS)
- HTTP/HTTPS server (optional)
- Support endpoint descriptors (`proto`, `host`, `port`, `path`) per [4#4.3.2]

**Pool Operations**:
- Accept pool.exchange from clients
- Serve datums from local pool
- Forward uncommitted events to room key holders

**Rate Limiting**:
- Connection rate limiting (max N connections/min per IP)
- Request rate limiting (max M requests/sec per connection)
- Event size validation (reject oversized payloads)

**Monitoring**:
- Log connection attempts
- Track bandwidth usage
- Publish uptime statistics (for attestations)

### D.6.2. Recommended Features

**Infrastructure**:
- Static IP address or stable domain
- IPv6 support (in addition to IPv4)
- Geographic redundancy (multiple regions)

**Security**:
- DDoS mitigation (Cloudflare, AWS Shield)
- Intrusion detection (fail2ban, Snort)
- Key storage in HSM/TPM

**Observability**:
- Prometheus metrics export
- Health check endpoint (implementation-defined path, not protocol-standardized)
- Debug logging (with privacy protections)

### D.6.3. Example: Public Relay

A public relay operates at relay conformance:
- Runs on VPS with public IP
- Accepts connections from any client
- Forwards uncommitted events to key holders
- Implements rate limiting
- Participates in pool gossip

## D.7. Feature Matrix

| Feature | Minimal | Client | Relay |
|---------|---------|--------|-------|
| **Verify signatures** | ✓ | ✓ | ✓ |
| **Endpoint descriptor connection support** | ✓ | ✓ | ✓ |
| **Pool sync (client)** | ✓ | ✓ | ✓ |
| **Store datums** | ✓ | ✓ | ✓ |
| **Generate keypair** | | ✓ | ✓ |
| **Sign events** | | ✓ | ✓ |
| **Compute PoW** | | ✓ | ✓ |
| **Join rooms** | | ✓ | ✓ |
| **Send messages** | | ✓ | ✓ |
| **Prune storage** | | ✓ | ✓ |
| **Accept connections** | | | ✓ |
| **Forward events** | | | ✓ |
| **Rate limiting** | | | ✓ |
| **Pool gossip** | | | ✓ |
| **Private messaging** | | OPT | OPT |
| **Blob support** | | OPT | OPT |
| **Room summary** | OPT | OPT | OPT |

## D.8. Optional Features

The following features are OPTIONAL for all conformance levels:

### D.8.1. Room Authority

**Feature**: Hold room private key, commit events.

**Endpoints**:
- Commit uncommitted events (wrap in depth-2 commit envelopes)
- Sign membership changes (room.join, room.ban, etc.)
- Update room metadata (room.update)

**Not required for conformance.** Most nodes are not key holders.

### D.8.2. Archival Mode

**Feature**: Store full history (no pruning).

**Storage**:
- Keep all events forever (no retention limits)
- Serve historical data to new joiners

**Not required for conformance.** Regular nodes MAY prune.

### D.8.3. Trust Aggregation

**Feature**: Collect and publish attestation data.

**Endpoints**:
- Accept attestations via API
- Provide trust scores via API
- Publish aggregated reputation data

**Not required for conformance.** This is a service layer on top of the protocol.

### D.8.4. Group End-to-End Encryption

**Feature**: Encrypt room.message content before sending.

**Not specified in v0.2.** Implementations MAY experiment with group E2E encryption, but it is not standardized.

### D.8.5. Private Messaging

**Feature**: Encrypted 1:1 messages between nodes.

**Endpoints**:
- `private.message`: Encrypted v2 message with mandatory `c.v=2`, `c.from`, `c.salt`, and `c.encrypted` (v1 `c.eph` is decrypt-only)
- `private.read`: Optional read receipts

**Cryptography**:
- Static-static X25519 (pairwise key, cacheable) plus a random per-message salt
- No forward secrecy: compromise of either static `enc` key reveals stored messages
- ChaCha20-Poly1305 AEAD encryption
- HKDF-SHA256 key derivation

Since all nodes publish `enc` (X25519 public key) in `node.meta`, any
node can receive private messages.

**OPTIONAL for all conformance levels.** Nodes that do not implement
private messaging simply ignore `private.message` envelopes.

### D.8.6. Blob Support

**Feature**: Content-addressed binary blobs (images, files, media).

**Event Types**:
- `room.blob`: Blob metadata in a room channel (hash, size, mime, name)
- `private.blob`: Encrypted blob metadata for 1:1 messages
- `blob.fetch`: Fetch blob binary by SHA-256 content hash

**Storage**:
- Blob binaries are cacheable (re-fetchable from peers)
- Nodes MAY opt out of blob storage entirely

**OPTIONAL for all conformance levels.** Nodes that do not support blobs
return `not_found` for all `blob.fetch` requests. Messages referencing
blobs via `c.blobs` are still valid; the blob content is simply unavailable.

### D.8.7. Room Summary

**Feature**: Lightweight room status for bandwidth-constrained clients.

**Endpoints**:
- `room.summary`: Request status for multiple rooms in one call
- `room.summary.result`: Per-room response with name, member count,
  and per-channel activity

**Use Cases**:
- Mobile wake-up sync (check unread counts without full event download)
- Room preview before joining
- Background badge refresh

**OPTIONAL for all conformance levels.** Nodes that do not support
`room.summary` return `not_implemented`.

### D.8.8. Tor Transport

**Feature**: Use Tor for all connections.

**Transport**:
- Connect to relays via .onion addresses
- Route traffic through Tor

**Not required for conformance.** Implementations MAY add Tor support as a transport option.

## D.9. Capability Negotiation

Toloo does NOT have a capability negotiation protocol.

**Rationale**: Complexity and downgrade attack surface.

**Consequences**:
- All conformant nodes MUST support the same core features
- Optional features are discovered implicitly (e.g., relay mode = accepts connections)

**Future versions MAY add capability flags** (e.g., in node.meta), but v0.2 does not.

## D.10. Version Compatibility

### D.10.1. Protocol Version Field

All envelopes include `"v": "0.2"` inside the `d` object (`d.v`).
Each nesting layer has its own `d.v`, allowing signers to declare
their protocol version independently.

**Rules**:
- Nodes MUST reject envelopes with unknown `d.v` values
- Nodes MUST NOT attempt to parse future versions
- Forward compatibility is NOT guaranteed

### D.10.2. Major vs. Minor Versions

**Major version** (0.x → 1.x):
- Breaking changes
- Incompatible with previous version
- Nodes MUST upgrade or become isolated

**Minor version** (0.2 → 0.3):
- Backward-compatible additions
- Old nodes can interoperate with new nodes
- New features are optional

**v0.2 is a major version** (first public release). Future 0.x releases will maintain backward compatibility.

## D.11. Test Vectors

Implementations MUST validate against the comprehensive test vectors in
[E] Test Vectors. Appendix E provides:

- Deterministic test identities (§E.2) with known seeds
- Canonical JSON test cases (§E.3) — verifiable without cryptography
- Envelope signing and verification (§E.4)
- Depth-2 commit wrapping (§E.5)
- Envelope identity and datum ID computation (§E.6)
- Private message encryption/decryption (§E.7)
- Proof-of-Work (§E.9)
- Membership reconstruction (§E.10)
- Room rule evaluation (§E.11)
- Blob piece hashing (§E.12)

The cross-implementation verification procedure (§E.15) defines the
recommended testing sequence.

## D.12. Interoperability Testing

Implementations SHOULD test interoperability with reference implementations.

### D.12.1. Test Scenarios

**Scenario 1: Private Message Encryption Roundtrip**
- Alice (implementation A) encrypts a private message to Bob (implementation B)
- Bob decrypts with his `enc` key and recovers the plaintext
- Tampered ciphertexts fail authentication
- Encrypted messages decrypted successfully

**Scenario 2: Room Creation and Join**
- Carol (impl A) creates room
- Alice (impl B) receives invitation
- Alice joins room
- Carol commits join
- Alice sees committed membership

**Scenario 3: Message Flow**
- Alice sends uncommitted message
- Relay (impl A) forwards to Carol (impl B)
- Carol commits message
- Bob (impl C) syncs and receives committed message

**Scenario 4: Pool Exchange**
- Alice (impl A) requests pool.exchange from Relay (impl B)
- Relay serves 50 events
- Alice verifies all signatures
- Alice updates local pool

**Scenario 5: Bisection Sync**
- Bob (impl A) offline for 1 hour (500 events missed)
- Bob reconnects to Relay (impl B)
- room.compare finds divergence
- Bob fetches missing range
- Bob's view converges with Relay

### D.12.2. Test Harness

A reference test harness SHOULD:
- Automate the scenarios above
- Validate against [E] Test Vectors (deterministic crypto values)
- Validate interoperability between implementations
- Report detailed errors (which step failed, why)

## D.13. Validation Checklist

Implementations MUST validate the following:

### D.13.1. Envelope Validation

- [ ] Exactly two top-level fields: `d` (object) and `p` (string)
- [ ] `p` is 86-character base64url Ed25519 signature
- [ ] `d.v` field present and equals `"0.2"`
- [ ] `d.n` field present and is 43-character base64url string (signer pub key)
- [ ] `d.t` field present and matches known type (see [A#datum-types])
- [ ] `d.ts` field present and is positive integer
- [ ] `d.r` field present for room events
- [ ] Signature `p` verifies against canonical(`d`) using `d.n`
- [ ] Per-layer `d` size ≤ 16,384 bytes
- [ ] Total envelope size ≤ 49,152 bytes (3 × 16,384)

### D.13.2. Event Validation

- [ ] Event type matches channel (e.g., room.join on channel -2)
- [ ] Required content fields present (see [A#content-fields])
- [ ] Field sizes within limits (e.g., body ≤ 4096 chars)
- [ ] Timestamp reasonable (not too far in past/future)
- [ ] Author is member of room (for room events)
- [ ] Room rules satisfied (permissions, rate limits, PoW)

### D.13.3. Commit Envelope Validation (Depth 2)

- [ ] Outer `d.t` equals `"commit"`
- [ ] Outer `d.n` equals the room public key (`d.env.d.r`)
- [ ] Outer `d.tc` present and is positive integer
- [ ] Outer `d.tc >= d.env.d.ts` (commit time ≥ author time)
- [ ] Outer `d.env` present and contains inner envelope `{d, p}`
- [ ] Outer signature `p` verifies against canonical(outer `d`) using outer `d.n`
- [ ] Inner signature `d.env.p` verifies against canonical(`d.env.d`) using `d.env.d.n`
- [ ] Inner envelope passes normal event validation (D.13.2)

### D.13.4. Transport Validation

- [ ] Endpoint `proto` is recognized (`ws`, `wss`, `http`, `https`)
- [ ] Endpoint descriptor defaults are applied consistently (`path`, `direct`)
- [ ] If proto requires `host`/`port`: they are present and valid
- [ ] Carrier framing (JSON or binary piece frames) is parsed safely
- [ ] On every carrier: signed envelope validation remains mandatory

## D.14. Error Handling

Conformant implementations MUST handle these errors gracefully:

| Error | Behavior |
|-------|----------|
| **Invalid signature** | Reject datum, log warning, do not propagate |
| **Unknown event type** | Store datum (forward compatibility), do not process |
| **Oversized datum** | Reject, return `payload_too_large` error |
| **Rate limit exceeded** | Reject, return `rate_limit_exceeded` error |
| **Permission denied** | Reject, return `permission_denied` error |
| **Network timeout** | Retry with exponential backoff (up to 5 minutes) |
| **Invalid PoW** | Reject, return `invalid_proof` error |
| **Malformed JSON** | Reject, return `invalid_request` error |

Implementations MUST NOT crash or hang on invalid input.

## D.15. Security Requirements

Conformant implementations MUST:

### D.15.1. Cryptographic Requirements

- Use constant-time Ed25519 implementation (prevent timing attacks)
- Use cryptographically secure random number generator (CSPRNG)
- Never reuse nonces in ChaCha20-Poly1305
- Zeroize keys on deallocation (prevent memory leaks)
- Validate all public keys before use (reject invalid curve points)

### D.15.2. Key Storage Requirements

- Encrypt private keys at rest
- Use OS-provided secure storage if available
- Require user authentication before key use (on mobile)
- Support key export with password encryption
- Warn users before exporting keys

### D.15.3. Network Requirements

- TLS 1.3 is RECOMMENDED when using `https`/`wss` carriers
- TLS certificate verification is RECOMMENDED (reject self-signed in production)
- Implement connection timeouts (prevent resource exhaustion)
- Implement rate limiting (prevent DoS)
- Log connection attempts (for security audits)

## D.16. Performance Requirements

Conformant implementations SHOULD meet these benchmarks:

| Operation | Target Performance |
|-----------|-------------------|
| **Signature verification** | ≥ 10,000 signatures/sec (single core) |
| **PoW computation** | Difficulty 20 in ≤ 10 sec (mobile) |
| **Pool sync** | ≥ 100 events/sec (network bound) |
| **Event storage** | ≥ 1,000 inserts/sec (database bound) |

These are guidelines, not hard requirements. Actual performance depends on hardware and network conditions.

## D.17. Platform Support

Conformant implementations SHOULD support:

| Platform | Priority | Notes |
|----------|----------|-------|
| **Linux** | High | Primary deployment target |
| **macOS** | High | Developer platform |
| **Windows** | Medium | Desktop users |
| **Android** | High | Mobile majority |
| **iOS** | High | Mobile minority |
| **Web (WASM)** | Medium | Browser-based clients |

Minimum versions:
- Linux: Kernel 4.0+ (for modern crypto APIs)
- macOS: 10.15+ (Catalina, for Keychain APIs)
- Windows: 10+ (for CryptoNG APIs)
- Android: 8.0+ (Oreo, for Keystore APIs)
- iOS: 13.0+ (for CryptoKit)

## D.18. Conformance Testing

### D.18.1. Test Suite

A conformance test suite SHOULD include:

1. **Unit tests**: Crypto primitives, data model, validation
2. **Integration tests**: Private message roundtrip, room flows, pool sync
3. **Interoperability tests**: Cross-implementation scenarios
4. **Fuzzing**: Malformed input handling
5. **Performance tests**: Benchmarking critical operations

### D.18.2. Certification

**No official certification process exists for v0.2.**

In the future, a third-party test suite MAY be published for self-certification. Implementations can run the suite and publish results.

## D.19. Deprecation Policy

**Breaking changes require major version bump** (0.x → 1.x).

**Deprecated features**:
- MUST be documented as deprecated for at least one minor version
- MUST continue to work until major version bump
- SHOULD log warnings when used

**Example**:
- v0.2: Feature X introduced
- v0.3: Feature X deprecated, Feature Y introduced as replacement
- v1.0: Feature X removed, Feature Y required

## D.20. Compliance Statement

Implementations SHOULD publish a compliance statement:

```markdown
# Toloo Protocol Compliance: MyClient v1.0

**Conformance Level**: Client
**Protocol Version**: 0.2
**Last Updated**: 2026-01-15

## Required Features
- [x] Ed25519 signature verification
- [x] Pool sync
- [x] Event creation and signing
- [x] PoW computation
- [x] Room joining and messaging

## Optional Features
- [x] Tor transport
- [x] Archival mode
- [ ] Trust aggregation (not implemented)

## Platforms
- [x] Linux (x86_64, arm64)
- [x] macOS (x86_64, arm64)
- [x] Android (arm64-v8a, armeabi-v7a)
- [x] iOS (arm64)

## Test Results
- Unit tests: 234/234 passing
- Integration tests: 45/45 passing
- Interoperability: Tested against ReferenceNode v0.2

## Known Limitations
- Mobile: PoW difficulty > 24 not recommended (battery drain)
- Blob support: OPTIONAL (nodes may opt out of binary storage)
```

## D.21. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC8785] JSON Canonicalization Scheme (JCS)
- [RFC8032] Edwards-Curve Digital Signature Algorithm (Ed25519)
- [RFC7748] Elliptic Curves for Security (X25519)
- [RFC8439] ChaCha20 and Poly1305 for IETF Protocols

## D.22. Conclusion

Toloo v0.2 defines clear conformance levels:
- **Minimal**: Read-only observation
- **Client**: Full participation
- **Relay**: Infrastructure support

All conformant nodes MUST implement core cryptography, data model, and
protocol mechanics. Optional features enable specialized roles (archival,
trust aggregation, room authority).

Implementations SHOULD publish compliance statements and test against
[E] Test Vectors. Interoperability testing between implementations
ensures a healthy ecosystem.

The protocol is version-locked (v0.2). Future versions will maintain
backward compatibility within the same major version (0.x).
