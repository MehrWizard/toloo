# 10. Endpoints

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [2] Data Model, [6] Events, [9] Transport

## 10.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 10.2. Endpoint Classes

Toloo endpoints are exchanged through structured `node.meta` content
(`c.endpoints[]`) descriptors. Protocol behavior is endpoint-driven, not host-level:

1. Network endpoints:
   - `proto` identifies the transport carrier (e.g., `ws`, `wss`, `https`).
2. Local implementation endpoints:
   - Non-interoperable local APIs (CLI/RPC/UI), out of wire scope.

Endpoint descriptor example:

```json
{
  "proto": "wss",
  "host": "relay.example.com",
  "port": 443,
  "path": "/toloo"
}
```

Default values when omitted:

```json
{
  "proto": "",
  "host": "",
  "port": 0,
  "path": "/",
  "direct": false
}
```

Descriptor validation is defined in [4#4.3.2]. Endpoint implementations MUST:
- Ignore descriptors with unrecognized `proto` values.
- Require `host` and `port` for protos that need them.
- Treat `direct` as a routing hint only.
- Treat endpoint descriptors as transport config only, never as identity.

## 10.3. Request Envelope Format

Every network request MUST be a signed envelope (`{d, p}`) where
`d.t` is the endpoint name:

```json
{
  "d": {
    "n": "REQUESTER_SIG",
    "v": "0.2",
    "t": "events.sync",
    "ts": 1710000000000,
    "c": {
      "rooms": ["ROOM_SIG"],
      "after": 1710000000000
    }
  },
  "p": "REQUEST_SIGNATURE"
}
```

Request rules:
- `d.n` identifies the requester.
- `d.v` is the protocol version.
- `d.t` identifies the requested operation.
- `d.ts` is request creation time.
- `d.c` contains endpoint-specific arguments.

## 10.4. Response Model

### 10.4.1. Success Responses

Successful responses are a stream of signed envelopes (`{d, p}`), each
independently verifiable. The responder MAY return:
- Existing committed envelopes (depth 2) or uncommitted envelopes (depth 1)
- Existing side-event envelopes
- Node-produced signed envelopes (for local facts such as peer lists)

No outer wrapper is used on success.

### 10.4.1.1. Private Message Delivery

Private messages (`private.message`) are submitted as complete signed
envelopes. The client creates the full event (including encryption)
locally and submits the envelope via `pool.exchange`, the same as any
other event. No special private message endpoint exists.

### 10.4.2. Error Responses

Errors are unsigned JSON objects (not envelopes):

```json
{
  "error": "permission_denied",
  "message": "Not a member of this room"
}
```

Non-envelope messages are limited to exactly two kinds: error objects (above)
and the room watermark control frame `{"wr": <tc>}` in `room.sync` responses
(§10.7.2). All other response values MUST be signed envelopes.

### 10.4.3. Stream Completion

Endpoint behavior is request-scoped:
- The responder processes one request envelope and emits zero or more response
  envelopes.
- Completion is implicit when the responder starts processing the next
  request or closes the connection.

Implementations MAY use strict request/response sequencing (send one request,
wait for completion) to avoid ambiguity.

## 10.5. Error Codes

Standard error codes:

| Code | Meaning |
|------|---------|
| `authentication_required` | Requester identity is unknown or blocked |
| `permission_denied` | Requester lacks room permission |
| `not_found` | Requested room/event/resource does not exist |
| `invalid_request` | Malformed request envelope or missing required fields |
| `invalid_signature` | Request signature verification failed |
| `invalid_proof` | Proof-of-Work verification failed |
| `rate_limit_exceeded` | Requester exceeded policy limits |
| `payload_too_large` | Request exceeds configured limits |
| `internal_error` | Relay/node internal error |

## 10.6. Event Submission

Toloo does not have separate "request" endpoints for creating events.
Authors create and sign event envelopes locally (see [6] Events for all
event types), then submit them to peers via `pool.exchange`.

The flow for any room event:

```
1. Author creates event envelope locally (e.g., room.message, room.join)
2. Author signs it with their Ed25519 key (depth 1)
3. Author submits envelope to peers via pool.exchange
4. Peer validates envelope (signature, room rules, replication policy)
5. Peer stores valid envelope in local pool
6. Envelope propagates to other peers through subsequent pool exchanges
7. Key holder discovers uncommitted event during sync, commits it (depth 2)
```

Peers receiving an envelope via `pool.exchange` MUST validate it against
room rules before accepting it into their pool. Invalid envelopes (bad
signature, rule violation, banned author) are rejected silently.

For room creation, the creator generates both the room Ed25519 keypair
and the initial `room.create` event, then commits it locally as the
initial key holder (see [5#room-creation]).

## 10.7. Sync Endpoints

### 10.7.1. events.sync

Incremental sync across multiple rooms.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "events.sync",
    "ts": 1710001000000,
    "c": {
      "rooms": ["ROOM_SIG_A", "ROOM_SIG_B"],
      "after": 1710000000000,
      "limit": 200
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- Matching signed envelopes in canonical sync order
- Room events may be committed envelopes (depth 2) or uncommitted (depth 1), subject to policy

### 10.7.2. room.sync

Fetches data for one room with channel and range filters.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.sync",
    "ts": 1710001100000,
    "c": {
      "r": "ROOM_SIG",
      "channel": 0,
      "start_tc": 1710000000000,
      "end_tc": 1710002000000,
      "limit": 100
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- Room watermark control frame, always first: `{"wr": <tc>}` — the latest
  commit time across room-internal channels (`ch < 0`). Advisory; see
  [5#5421-room-watermark] for semantics. Not a signed envelope; relays
  cannot grant access by forging it (commit-time enforcement).
- Matching room envelopes for the requested range

### 10.7.3. room.compare

Compares sampled event IDs to detect divergence.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.compare",
    "ts": 1710001200000,
    "c": {
      "r": "ROOM_SIG",
      "channel": 0,
      "samples": [
        {"seq": 1, "eid": "0:1710000000000:ALICE_SIG"},
        {"seq": 50, "eid": "0:1710001000000:BOB_SIG"},
        {"seq": 100, "eid": "0:1710002000000:CAROL_SIG"}
      ]
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- A responder-signed result envelope (`room.compare.result`) with matches and
  mismatches, or
- Equivalent evidence envelopes sufficient for caller-side comparison

Implementations SHOULD support at least one of these response patterns.

## 10.8. Pool and Discovery Endpoints

### 10.8.1. pool.exchange

Exchanges pool summaries and data.

Request:

```json
{
  "d": {
    "n": "PEER_SIG",
    "v": "0.2",
    "t": "pool.exchange",
    "ts": 1710002000000,
    "c": {
      "offer": [
        {"datum_id": "HASH_1", "t": "room.message", "r": "ROOM_SIG", "ts": 1710000000000}
      ],
      "want": {
        "rooms": ["ROOM_SIG"],
        "types": ["room.message", "room.join"],
        "after": 1710000000000,
        "limit": 200
      }
    }
  },
  "p": "PEER_SIGNATURE"
}
```

The `rooms` filter matches different fields depending on event type:
- Room events: matches `d.r`
- Private events: matches `d.n` or `d.to` (sender or receiver acts as "room")
- Node events: matches `d.n`

See [7#exchange-request] for the full matching table.

Success response stream:
- JSON frames: requested signed envelopes
- Binary frames: blob piece data for `room.blob` or `private.blob`
  envelopes included in the response (piece-hash-prefixed, see [9#framing])
- Optional responder-signed `pool.exchange.result` envelope for bookkeeping

Responders SHOULD include `node.meta` envelopes for known reachable peers
in pool exchange responses. This provides the requester with backup
connection options if the current peer becomes unreachable. A node's pool
naturally accumulates `node.meta` envelopes, serving as a distributed
peer directory.

Responders MAY also include `room.create` or latest `room.update` envelopes
for rooms referenced in offered datums that the requester does not yet know
about. This enables organic room discovery through pool exchange without
requiring explicit search.

### 10.8.2. node.peers

Requests known peers.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "node.peers",
    "ts": 1710002100000,
    "c": {
      "limit": 20
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- `node.meta` envelopes for discovered peers

### 10.8.3. node.rooms

Requests publicly shared rooms for a node.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "node.rooms",
    "ts": 1710002200000,
    "c": {
      "node": "TARGET_SIG",
      "limit": 100
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- Room metadata envelopes (`room.create` and latest `room.update`) visible to
  the requester

### 10.8.4. system.fetch

Fetches specific datums by ID.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "system.fetch",
    "ts": 1710002300000,
    "c": {
      "datum_ids": ["HASH_A", "HASH_B", "HASH_C"]
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- Each available envelope matching requested IDs
- Missing IDs are omitted (caller infers by difference)

### 10.8.5. blob.fetch

Fetches blob pieces by content hash.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "blob.fetch",
    "ts": 1710002400000,
    "c": {
      "hash": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response: one or more binary frames (type `0x01`, see
[9#framing]) containing the blob data. Each binary frame is prefixed with
the 32-byte SHA-256 hash. The requester accumulates binary frames and
verifies `SHA-256(accumulated_bytes) == hash`.

Error response: JSON frame with `{"error": "not_found"}` if the node does
not store the requested blob.

Nodes MAY opt out of blob storage entirely. A node that does not store
blobs returns `not_found` for all `blob.fetch` requests.

#### 10.8.5.1. Piece-Level Fetching

The `hash` field in `blob.fetch` accepts either a **blob hash** or a
**piece hash**:

- **Blob hash**: The node returns all locally available pieces for that
  blob. Each piece is sent as a separate binary frame (type `0x01`)
  prefixed with the piece's own SHA-256 hash. Partial seeders return only
  the pieces they have.
- **Piece hash**: The node returns exactly one binary frame containing
  that single piece, prefixed with the piece hash.

The requester determines which mode was used by comparing the response
hash prefixes against the `pieces[]` array from the `room.blob` envelope.

This enables resumable downloads: a client that already has pieces 0-3
can request the remaining pieces individually by piece hash from any
available node.

### 10.8.6. room.summary

Requests lightweight room status for one or more rooms. Designed for
mobile clients and low-bandwidth scenarios where full sync is too expensive.

Request:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.summary",
    "ts": 1710002500000,
    "c": {
      "rooms": ["ROOM_SIG_A", "ROOM_SIG_B"]
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Success response stream:
- One `room.summary.result` envelope per requested room (signed by the
  responder):

```json
{
  "d": {
    "n": "RESPONDER_SIG",
    "v": "0.2",
    "t": "room.summary.result",
    "ts": 1710002500100,
    "c": {
      "r": "ROOM_SIG_A",
      "name": "My Room",
      "members": 42,
      "channels": [
        {"ch": 0, "latest_tc": 1710002400000, "count": 12},
        {"ch": 1, "latest_tc": 1710002300000, "count": 9500},
        {"ch": 2, "latest_tc": 1710002200000, "count": 120}
      ]
    }
  },
  "p": "RESPONDER_SIGNATURE"
}
```

Content fields:
- `r`: Room public key.
- `name`: Current room name (from latest channel -1 metadata).
- `members`: Current member count (computed from channel -2).
- `channels[]`: Per-channel summary with `ch` (channel number),
  `latest_tc` (most recent commit timestamp), and `count` (total
  committed events).

Rooms the responder does not know about are omitted from the response.
The requester infers unknown rooms by difference.

Implementations SHOULD limit `c.rooms` to at most 50 entries per request.

## 10.9. Optional Metadata Mirror Endpoints

Implementations MAY expose unsigned HTTP metadata mirrors for convenience,
but these are non-normative and SHOULD NOT be relied on for protocol
interoperability.

To reduce scanner fingerprinting, deployments MAY avoid public mirrors
entirely and rely only on signed envelopes exchanged after node introduction.

## 10.10. Local-Only Endpoints

The following operations are local implementation APIs and are not part of
wire protocol interoperability:
- local blocklists/spam filters
- local moderation state inspection
- local UI/UX moderation controls

Implementations MAY expose these operations over CLI, RPC, or UI APIs.

## 10.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
