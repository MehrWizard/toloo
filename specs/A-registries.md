# Appendix A: Registries

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md)

## A.1. Purpose

This appendix defines normative registries of protocol identifiers. All
values listed here are part of the Toloo Protocol v0.2 specification.

Implementations MUST recognize all identifiers in these registries.
Unknown identifiers SHOULD be handled gracefully (stored, forwarded, but
not processed).

## A.2. Datum Types

Datum types are identified by the `d.t` field.

### A.2.1. Node Events

| Type | Category | Description |
|------|----------|-------------|
| `node.meta` | Node | Node metadata (identity, endpoint descriptors, profile info) |

### A.2.2. Room Events

| Type | Category | Description | Requires Commit |
|------|----------|-------------|-----------------|
| `room.create` | State | Creates a new room | Yes (self-signed) |
| `room.message` | Content | Sends a message to a channel | Optional |
| `room.join` | State | Requests to join a room | Yes |
| `room.leave` | State | Leaves a room | Yes |
| `room.invite` | State | Invites a node to join | Yes |
| `room.ban` | State | Bans a node from a room | Yes |
| `room.unban` | State | Removes a ban | Yes |
| `room.update` | State | Updates room metadata (channel -1 only) | Yes |
| `room.react` | Content | Adds a reaction to a message | Optional |
| `room.edit` | Content | Edits a previously sent message | Optional |
| `room.delete` | Content | Requests deletion of a message | Yes |
| `room.blob` | Content | Blob/media metadata in a channel | Optional |
| `room.announce` | Content | Official room announcement (channel 0, room-key-signed) | Yes (self-signed) |
| `room.migrate` | State | Announces key holder migration | Yes (room-signed) |

### A.2.3. Side-Events

| Type | Category | Description |
|------|----------|-------------|
| `side.fork` | Governance | Announces a room fork |
| `side.attestation` | Trust | Makes a claim about another node |
| `room.flag` | Moderation | Flags an uncommitted event (side-event, never committed) |

### A.2.4. Envelope Structure

| Field | Type | Presence | Description |
|-------|------|----------|-------------|
| `d` | object | REQUIRED | Signed data (contains all content, `n`, `v`, and optionally `env`) |
| `p` | string | REQUIRED | Ed25519 signature over canonical(d) |

No other top-level fields. Protocol version (`v`) is inside `d`.

### A.2.5. Envelope Nesting Types

These `d.t` values are used at depth 2 and 3 for wrapping envelopes:

| Type | Depth | Signer | Description |
|------|-------|--------|-------------|
| `commit` | 2 | Room (`d.r` of inner) | Room key holder commits an event. `d.tc` present. |
| `ack` | 3 | Author (innermost `d.n`) | Author confirms receipt of commitment. |

### A.2.6. Endpoint Request and Result Types

These `d.t` values are used for request/response control
envelopes (see [10] Endpoints):

| Type | Category | Description |
|------|----------|-------------|
| `events.sync` | Sync request | Incremental multi-room sync request |
| `room.sync` | Sync request | Room/channel range sync request |
| `room.compare` | Sync request | Divergence comparison request |
| `room.compare.result` | Sync result | Comparison result datum |
| `pool.exchange` | Replication request | Pool offer/want exchange request |
| `pool.exchange.result` | Replication result | Optional exchange summary datum |
| `node.peers` | Discovery request | Request known peer metadata |
| `node.rooms` | Discovery request | Request public room metadata |
| `system.fetch` | Utility request | Request datums by datum_id |
| `blob.fetch` | Blob request | Fetch blob binary by content hash |
| `room.summary` | Discovery request | Request lightweight room status |
| `room.summary.result` | Discovery result | Room status summary datum |

### A.2.7. Private Events

| Type | Category | Description |
|------|----------|-------------|
| `private.message` | Private | Encrypted 1:1 message (mandatory E2E) |
| `private.blob` | Private | Encrypted blob/media metadata (mandatory E2E) |
| `private.read` | Private | Read receipt for private messages (optional) |

### A.2.8. Endpoint Descriptor Fields (`node.meta c.endpoints[]`)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `proto` | string | Yes | Transport protocol identifier |
| `host` | string | Yes* | Hostname or IP address (* required for most protos) |
| `port` | integer | Yes* | Port number (* required for most protos) |
| `path` | string | No | URL path for HTTP/WS protos (default `/`) |
| `direct` | boolean | No | Key-holder routing hint (default `false`) |

`via[]` fields are transport configuration only and MUST NOT be interpreted
as node identity fields.

Normalized defaults for omitted fields:

```json
{
  "proto": "",
  "host": "",
  "port": 0,
  "path": "/",
  "direct": false
}
```

Validation rules:
- `proto` MUST be a recognized transport protocol identifier.
- If the proto requires `host` and `port`, they MUST be present.
- Implementations MUST ignore descriptors with unrecognized `proto` values.

## A.3. Rule Types

Rule types are identified by the `t` field in rule objects within room
metadata.

| Type | Parameters | Description |
|------|------------|-------------|
| `join` | `allow`, `deny` | Controls who may join the room |
| `post` | `allow`, `deny` | Controls who may post messages |
| `invite_only` | `inviters` | Requires an invitation before joining |
| `join_pow` | `difficulty` | Requires Proof-of-Work to join |
| `post_pow` | `difficulty` | Requires Proof-of-Work per message |
| `rate_limit` | `max`, `per`, `for` | Limits message rate per node |
| `retention` | `days` | Suggests client-side retention period |
| `uncommitted_ttl` | `ms` | Controls how long uncommitted events may be held |
| `replication` | `mode`, `nodes` | Controls who may store and serve room data |
| `history` | `allow` | Controls who may read historical messages |
| `uncommitted_moderation` | `moderators` | Allows nodes to flag uncommitted events |

## A.4. Error Codes

Error codes are returned in `error` field of error responses.

| Code | HTTP Equivalent | Description |
|------|-----------------|-------------|
| `authentication_required` | 401 | Request identity missing, invalid, or blocked |
| `permission_denied` | 403 | Request identity authenticated but not authorized |
| `not_found` | 404 | Requested resource does not exist |
| `invalid_request` | 400 | Malformed request or missing required fields |
| `invalid_signature` | 400 | Cryptographic signature verification failed |
| `invalid_proof` | 400 | Proof-of-Work verification failed |
| `rate_limit_exceeded` | 429 | Too many requests from this node |
| `payload_too_large` | 413 | Request exceeds size limits |
| `internal_error` | 500 | Server-side error |
| `not_implemented` | 501 | Feature not implemented by this node |
| `timeout` | 504 | Operation timed out |
| `conflict` | 409 | State conflict (e.g., already joined) |

## A.5. Attestation Claims

Attestation claim types are identified by the `claim` field in
`side.attestation` events.

### A.5.1. Positive Claims

| Claim | Meaning | Common Detail Fields |
|-------|---------|---------------------|
| `seeded` | Subject provided room data to attester | `bytes`, `rooms`, `period_days` |
| `committed` | Subject forwarded event to key holder, resulting in commit | `room`, `events` |
| `available` | Subject was reachable and responsive | `uptime_percent`, `latency_ms`, `period_days` |
| `state_confirmed` | Subject's room state claim verified as correct | `room`, `channel` |

### A.5.2. Negative Claims

| Claim | Meaning | Common Detail Fields |
|-------|---------|---------------------|
| `failed_proof` | Subject provided data with invalid proofs | `datum_id`, `reason` |
| `false_data` | Subject provided fabricated or tampered data | `datum_id`, `reason` |
| `state_denied` | Subject's room state claim verified as false | `room`, `channel`, `reason` |
| `timeout` | Subject was unreachable or unresponsive | `timeout_ms`, `attempts` |

## A.6. Content Fields

Content field schemas for specific event types.

### A.6.1. room.create

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `name` | string | Yes | Room name (1-256 characters) |
| `description` | string | No | Room description (max 1024 characters) |
| `rules` | array | No | Initial rules array |

### A.6.2. room.message

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Channel number (default 1) |
| `body` | string | Yes | Message text (max 4096 characters) |
| `blobs` | array of strings | No | Array of blob eids referenced by this message (max 10) |

### A.6.2.1. room.announce

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Must be 0 (omission defaults to 0) |
| `body` | string | Yes | Announcement text (max 4096 characters) |

Additional rule: `d.n` MUST equal `d.r` (authored by the room key).

### A.6.3. room.join

No content fields required. Content MAY be empty (`{}`).

When a room has a `join_pow` rule, the author MUST set the `d.nonce` field
(see [2#2.3.3]) to satisfy the required difficulty before submitting.

### A.6.4. room.leave

No content fields required. Content MAY be empty (`{}`).

### A.6.5. room.invite

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `invitee` | string | Yes | Public key of node being invited |

### A.6.6. room.ban

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `banned` | string | Yes | Public key of node being banned |
| `reason` | string | No | Human-readable reason (max 250 characters) |

### A.6.7. room.unban

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `unbanned` | string | Yes | Public key of node being unbanned |

### A.6.8. room.update

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `name` | string | No | New room name (1-256 characters) |
| `description` | string | No | New room description (max 1024 characters) |
| `rules` | array | No | New rules array |

At least one field must be present.

### A.6.9. room.react

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Channel number (default 1). Must match the channel of the target event. |
| `target` | string | Yes | Event ID (eid) of message being reacted to |
| `emoji` | string | Yes | Emoji or reaction string (max 16 characters) |

### A.6.10. room.edit

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Channel number (default 1). Must match the channel of the target event. |
| `target` | string | Yes | Event ID (eid) of message being edited |
| `body` | string | Yes | New message text (max 4096 characters) |

### A.6.11. room.delete

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Channel number (default 1). Must match the channel of the target event. |
| `target` | string | Yes | Event ID (eid) of message to delete |

### A.6.12. room.migrate

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `from` | string | No | Public key of old key holder |
| `to` | string | No | Public key of new key holder |
| `reason` | string | No | Human-readable reason (max 250 characters) |

### A.6.13. side.fork

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `original` | string | Yes | Public key of room being forked |
| `forked` | string | Yes | Public key of new forked room |
| `reason` | string | No | Human-readable reason (max 250 characters) |

### A.6.14. side.attestation

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `subject` | string | Yes | Public key of node being attested to |
| `claim` | string | Yes | Claim type (see A.5) |
| `detail` | object | No | Claim-specific structured data |

### A.6.15. room.flag

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `target` | string | Yes | Event ID (eid) of event being flagged |
| `reason` | string | No | Human-readable reason (max 250 characters) |

### A.6.16. room.blob

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ch` | integer | No | Channel number (default 1) |
| `hash` | string | Yes | SHA-256 hex hash of complete blob (64 chars) |
| `size` | integer | Yes | Total blob size in bytes |
| `piece_size` | integer | Yes | Bytes per piece (last piece may be smaller) |
| `pieces` | array of strings | Yes | SHA-256 hex hashes of each piece, in order |
| `mime` | string | No | MIME type |
| `name` | string | No | Original filename |

Piece count = `ceil(size / piece_size)`. Each `pieces[i]` is the SHA-256
of the i-th piece. Final verification: `SHA-256(all_pieces_concatenated) == hash`.
For blobs smaller than `piece_size`, `pieces` has one entry equal to `hash`.

### A.6.17. private.message

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `eph` | string | Yes | Ephemeral X25519 public key (base64url, 43 chars) |
| `encrypted` | string | Yes | Base64url-encoded ciphertext |

Plaintext content (inside `encrypted`): `{"body": "...", "blobs": []}`

### A.6.18. private.blob

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `eph` | string | Yes | Ephemeral X25519 public key (base64url, 43 chars) |
| `encrypted` | string | Yes | Base64url-encoded ciphertext |

Plaintext content (inside `encrypted`):
`{"hash": "...", "size": ..., "piece_size": ..., "pieces": [...], "mime": "...", "name": "...", "blob_key": "..."}`

`blob_key` is a symmetric key for decrypting the blob pieces. Each piece
is encrypted before transfer; the receiver decrypts after hash verification.

### A.6.19. private.read

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `ids` | array of strings | Yes | Direct envelope IDs (`{ts}:{n}`) being marked as read |

### A.6.20. room.summary

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `rooms` | array of strings | Yes | Room public keys to summarize (max 50) |

### A.6.21. room.summary.result

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `r` | string | Yes | Room public key |
| `name` | string | Yes | Current room name |
| `members` | integer | Yes | Current member count |
| `channels` | array of objects | Yes | Per-channel summary |

Each entry in `channels`:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `ch` | integer | Yes | Channel number |
| `latest_tc` | integer | Yes | Most recent commit timestamp |
| `count` | integer | Yes | Total committed events |

### A.6.22. blob.fetch

| Field in `c` | Type | Required | Description |
|--------------|------|----------|-------------|
| `hash` | string | Yes | SHA-256 hex hash (64 chars) — blob hash or piece hash |

When `hash` matches a blob hash, the responder returns all locally
available pieces. When it matches a single piece hash, the responder
returns that piece only. See [10#piece-level-fetching].

## A.7. Reserved Channel Numbers

Channels partition into two classes:

- **Room-internal (`ch < 0`)** — protocol-defined, define room state.
  Events on these channels are **retention-proof**: nodes MUST NOT prune
  them (see [11#retention-classes]). They are small (O(members + bans +
  edits), not O(messages)).
- **Conversation (`ch >= 0`)** — actual chat, subject to room retention
  policies. Each channel MAY carry its own policy via room rules.

| Channel | Class | Purpose | Events |
|---------|-------|---------|--------|
| -1 | Room-internal | Room metadata | `room.create`, `room.update` |
| -2 | Room-internal | Membership and moderation state | `room.join`, `room.leave`, `room.ban`, `room.unban`, `room.invite` |
| < -2 | Room-internal | Reserved for future room state (e.g. blob mappings) | TBD |
| 0 | Conversation (official) | Room announcements/news — the room's public voice | `room.announce` (room-key-signed only) |
| 1+ | Conversation | Member chat channels, each with optional per-channel policy | `room.message`, `room.react`, `room.edit`, `room.delete`, `room.blob` |

**Channel 0 — official announcements.** Events on channel 0 MUST be authored
by the room key itself: `d.n == d.r` (and `d.t == "room.announce"`). The
announcement is therefore self-evident from its signatures — the author *is*
the room — and members cannot forge one. Clients SHOULD surface official
announcements across all channels (pinned-style); opening one reveals
channel 0, the room's official channel.

**Default channel.** Member content events (`room.message` etc.) without
`c.ch` default to **channel 1** — never 0, so ordinary chat can never land
in the official feed.

### A.7.1. Room Watermark

The **room watermark** is the latest commit time (`tc`) across all
room-internal channels (`ch < 0`). Because every such event is committed by
the room key, the watermark is implicitly room-key-signed.

`room.sync` responses begin with an advisory control frame:

```json
{"wr": 1710000100000}
```

Clients compare the relay's watermark against their known value:
- Equal → the client's view of room state (membership, rules) is current.
- Behind → room-internal events exist that the client has not seen. The
  client SHOULD re-sync room-internal channels before posting. A client only
  needs room-internal events from its **own committed join** onward: bans of
  members who joined before it do not affect its eligibility, and other
  members' messages are covered by the room key's per-message commit
  signature (depth 2).

The watermark is advisory. A relay cannot admit a non-member by forging it:
posting eligibility is enforced by the room key holder at commit time, so a
forged watermark can cause at worst extra syncs or delayed refusals.

## A.8. Transport Protocols

| Proto | Transport | Built-in Encryption | Browser | Required Fields |
|-------|-----------|-------------------|---------|-----------------|
| `ws` | WebSocket | No | Yes | `host`, `port` |
| `wss` | WebSocket + TLS | TLS | Yes | `host`, `port` |
| `http` | HTTP | No | Yes | `host`, `port` |
| `https` | HTTP + TLS | TLS | Yes | `host`, `port` |

## A.9. MIME Types

| Type | Usage |
|------|-------|
| `application/x-toloo` | `.toloo` data files (base64url envelopes, one per line) |
| `application/octet-stream` | Encrypted frame carrier payloads |
| `application/json` | Individual JSON messages |

## A.10. Discovery Bootstrap Records

This version defines no name-service bootstrap records.

Bootstrap inputs are:
- Pre-bundled seed `.toloo` files (signed `node.meta` envelopes)
- `toloo://` URIs (signed envelopes, see [4#toloo-uri-scheme])
- `.toloo` files (signed envelopes, see [4#toloo-data-files])
- `pool.exchange` responses (organic peer and room discovery)

## A.11. URI Scheme

### A.11.1. toloo:// URI and .toloo Files

Same format — one base64url-encoded envelope per line. In URIs, `.`
replaces `\n` as the line separator.

```
URI:   toloo://BASE64URL_ENVELOPE[.BASE64URL_ENVELOPE...]
File:  BASE64URL_ENVELOPE\nBASE64URL_ENVELOPE\n...
```

| Property | URI | File |
|----------|-----|------|
| Scheme prefix | `toloo://` | None |
| Line separator | `.` (period) | `\n` (newline) |
| Comments | No | Lines starting with `#` |
| MIME type | — | `application/x-toloo` |
| File extension | — | `.toloo` |

## A.12. Protocol Version

Current version: `0.2`

This value appears in the `d.v` field of all envelopes. Each nesting
layer has its own `v`, allowing signers to declare their protocol version
independently.

Version format: `<major>.<minor>`
- Major version increments indicate breaking changes
- Minor version increments indicate backward-compatible additions

## A.13. Constants

| Constant | Value | Unit | Description |
|----------|-------|------|-------------|
| Frame type: JSON | `0x00` | byte | JSON envelope or error frame |
| Frame type: Binary | `0x01` | byte | Binary blob frame (32-byte hash prefix + data) |
| Max message body | 4096 | characters | Text message limit |
| Binary frame hash prefix | 32 | bytes | SHA-256 piece hash at start of binary frame data |
| Default piece size | 262144 | bytes | 256 KiB per piece (RECOMMENDED) |
| Max reason text | 250 | bytes | Reason field limit |
| Max node metadata | 4096 | bytes | Total d object size for node.meta |
| Max room metadata | 4096 | bytes | Total d object size for room.create/update |
| Max d size (per layer) | 16384 | bytes | Single layer d object size (excluding env content) |
| Max envelope size | 49152 | bytes | Total envelope including all nesting (3 x 16384) |
| Ed25519 key size | 32 | bytes | Signing key size |
| X25519 key size | 32 | bytes | Encryption key size |
| SHA-256 output size | 32 | bytes | Hash output size |
| ChaCha20-Poly1305 nonce | 12 | bytes | AEAD nonce size |
| ChaCha20-Poly1305 tag | 16 | bytes | AEAD tag size |

## A.14. Default Values

These are RECOMMENDED defaults for implementations. They are not protocol
requirements.

| Parameter | Default | Description |
|-----------|---------|-------------|
| Endpoint connection idle timeout | 300000 | milliseconds (5 minutes idle) |
| Keepalive interval | 60000 | milliseconds (1 minute) |
| Retention period | 90 | days |
| Uncommitted TTL | 86400000 | milliseconds (24 hours) |
| Pool exchange limit | 50 | datums per exchange |
| Gossip peer limit | 20 | peers per response |
| Rate limit (default) | 10 per 60000 | messages per minute |
| PoW difficulty (join) | 20 | bits |
| PoW difficulty (post) | 16 | bits |
| Max reconnect delay | 300000 | milliseconds (5 minutes) |

## A.15. Extension Mechanism

Unknown datum types, rule types, and attestation claims SHOULD be preserved
when forwarding data, even if the node does not understand them.

This allows the protocol to be extended without breaking existing
implementations. Extensions SHOULD be documented in future revisions of
this specification or in separate extension specifications.
