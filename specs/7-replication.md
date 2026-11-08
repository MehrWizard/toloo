# 7. Replication

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [2] Data Model, [5] Rooms, [6] Events, [10] Endpoints

## 7.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 7.2. The Pool

The pool is a node's local storage of all envelopes it knows about. Every
node maintains one pool containing:
- Committed room events (depth-2 envelopes with `d.t = "commit"`)
- Uncommitted room events (depth-1 envelopes)
- Private events (`private.message`, `private.blob`, acks)
- Side-events (attestations, forks, announcements)
- Binary blobs (keyed by SHA-256 hash)

All data flows through the pool. There is no separate gossip channel,
message queue, or relay protocol.

## 7.3. Pool Operations

### 7.3.1. Receive

When an envelope arrives from a peer:

```
1. Validate envelope structure (see [2#type-validation])
2. Verify signature p over canonical(d) using d.n
3. If d.env exists, verify recursively (same algorithm at each depth)
4. Validate signer alternation (see [2#signer-validation])
5. Reject malformed or invalidly signed envelopes
6. Check room replication policy (if room envelope)
7. Store envelope with metadata (received_at, source peer)
8. If committed envelope (depth 2) matches existing uncommitted (depth 1), supersede
```

Matching in step 8 is by envelope identity computed from the innermost `d`
(see [2#envelope-identity]).

### 7.3.2. Serve

When a peer requests data:

```
1. Verify request datum signature
2. Authorize request against room replication policy and local moderation policy
3. Query pool for matching datums
4. Stream matching signed datums to the peer
```

### 7.3.3. Commit

When a node holds a room's private key:

```
1. Query pool for uncommitted events (depth 1) targeting this room
2. Evaluate room rules (joins, posts, bans, etc.)
3. For each accepted event:
   a. tc = current timestamp (ms)
   b. Construct commit d: {n: ROOM_SIG, v: "0.2", t: "commit", ts: tc, tc: tc, env: inner}
   c. p = Ed25519.sign(room_priv, canonical(commit_d))
   d. Store depth-2 envelope {d: commit_d, p} in pool
   e. Mark uncommitted input envelope superseded
```

The inner envelope `{d, p}` is preserved unchanged inside `env`.

### 7.3.4. Prune

Nodes MAY prune old or expired datums:

```
1. Uncommitted events older than room uncommitted_ttl -> delete
2. Committed events older than retention period -> archive or delete
3. Side-events older than implementation-defined period -> delete
4. Keep latest channel -1 (metadata) always; keep all channel -2 events always
```

## 7.4. Pool Exchange

Nodes exchange pool contents with peers via signed `pool.exchange` requests.

### 7.4.1. Exchange Request

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "pool.exchange",
    "ts": 1710000000000,
    "c": {
      "offer": [
        {"eid": "0:1710000000000:BOB_SIG", "t": "room.message", "r": "ROOM_SIG"}
      ],
      "want": {
        "rooms": ["ROOM_SIG_A", "ROOM_SIG_B"],
        "types": ["room.message", "room.join", "side.attestation"],
        "after": 1710000000000,
        "limit": 200
      }
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Summary fields:
- `eid`: envelope identity (from innermost `d`, see [2#envelope-identity])
- `t`: event type (innermost `d.t`)

The `rooms` filter matches different fields depending on event type:

| Event Category | `rooms` matches | Example |
|---------------|----------------|---------|
| Room events | `d.r` (room key) | `room.message`, `room.join` |
| Private events | `d.n` or `d.to` (sender or receiver) | `private.message`, `ack` |
| Side-events | `d.c.original` or context-dependent | `side.fork`, `side.attestation` |
| Node events | `d.n` (node key) | `node.meta` |

For private messages, the sender or receiver sig acts as the "room" in the
filter. A client requesting its own private messages includes its own sig
in `rooms`. The responder matches envelopes where the requester is either
`d.n` (sender) or `d.to` (receiver) at any depth.
- `r`: room key (if room envelope)

### 7.4.2. Exchange Response

Success response is a stream of signed datums:
- Datums matching requester `want` criteria
- Optionally additional datums requested indirectly by policy (e.g., latest
  room metadata needed to validate streamed events)

Error response is unsigned JSON:

```json
{"error":"permission_denied","message":"room replication policy denied"}
```

### 7.4.3. Exchange Protocol

```
Alice                               Bob
  |                                  |
  | -- signed pool.exchange request ->|
  |                                  |
  |<----------- signed datum stream --|
  |                                  |
```

No additional response envelope is required.

## 7.5. Replication Policy

Room replication is controlled by the `replication` rule (see [5#rules]).

### 7.5.1. Open Replication

```json
{"t": "replication", "mode": "open"}
```

Any node MAY store and serve room data.

### 7.5.2. Members-Only Replication

```json
{"t": "replication", "mode": "members"}
```

Only current members SHOULD store and serve room data. Before serving:

```
1. Identify room from requested datum filters
2. Check requester membership in current channel -2 state
3. If member, serve; otherwise omit room datums
```

### 7.5.3. Designated Replication

```json
{"t": "replication", "mode": "designated", "nodes": ["SIG_1", "SIG_2"]}
```

Only listed nodes SHOULD store and serve room data. Non-designated nodes
that receive such data SHOULD:
- Avoid durable storage for that room
- Avoid serving that room to other peers
- Forward uncommitted events toward designated nodes

## 7.6. Seeding

Seeding is serving signed envelopes to peers. Committed room envelopes
are self-authenticating via the author's signature (inner `p`) and the
room's commit signature (outer `p`), so they are verifiable regardless
of who serves them.

```
1. Peer requests room data via events.sync, room.sync, or pool.exchange
2. Node checks replication policy
3. If authorized, node streams matching signed datums
4. Peer verifies signatures and commit state locally
```

## 7.7. Committing Flow

Uncommitted events reach key holders through direct delivery or relay
forwarding.

### 7.7.1. Direct Forwarding

```
1. Node has uncommitted event in pool
2. Node knows key holder address
3. Node sends signed envelope via selected endpoint carrier
4. Key holder validates and wraps in depth-2 commit envelope
5. Committed envelope propagates back through normal replication
```

### 7.7.2. Relay Forwarding

```
1. Node has uncommitted event for room R
2. Node cannot reach key holder directly
3. Node forwards to relays listed in room metadata
4. Relay stores uncommitted envelope in pool
5. Key holder syncs relay pool and wraps in commit envelopes
6. Committed envelopes spread through pool exchange
```

### 7.7.3. Key Holder Polling

Key holders discover uncommitted events by syncing with peers:

```
1. Key holder maintains connections to one or more relays
2. Key holder periodically runs pool.exchange or events.sync
3. Response includes uncommitted events (depth 1) for rooms it holds
4. Key holder commits each valid event (depth 2) and stores in pool
5. Next pool.exchange propagates committed versions to peers
```

There is no push notification mechanism. Key holders SHOULD poll
frequently enough to maintain acceptable commit latency (implementation-
defined, suggested: every 1-5 seconds when online).

When a key holder is offline, uncommitted events accumulate at relays
and other peers. On reconnect, the key holder's first sync picks up
all pending events. Rooms with `uncommitted_ttl = 0` become read-only
when no key holder is reachable.

### 7.7.4. Parallel Committing

Multiple key holders may attempt to commit concurrently.

If two committed envelopes exist for the same innermost `d` with different
`tc` values, nodes SHOULD prefer the smallest `tc` as canonical ordering
key and MAY keep additional variants for audit/debug purposes.

Deterministic ordering rule for committed events:
1. `tc` ascending
2. `d.ts` ascending
3. `eid` lexicographic

## 7.8. Synchronization

Nodes sync room data using incremental sync and compare-based recovery.

### 7.8.1. Incremental Sync

Request (`events.sync`):

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "events.sync",
    "ts": 1710003000000,
    "c": {
      "rooms": ["ROOM_SIG"],
      "after": 1710000000000,
      "limit": 500
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Response: stream of matching signed datums.

### 7.8.2. Compare-Based Recovery

Request (`room.compare`):

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.compare",
    "ts": 1710003100000,
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

Response:
- Responder-signed compare result datum, or
- Evidence datums needed for caller-side mismatch detection

Then requester fetches missing ranges via `room.sync`.

## 7.9. Partition Recovery

Toloo's pool model handles partitions without special consensus machinery.

### 7.9.1. During Partition

```
Partition splits nodes into groups A and B

Group A:
- continues collecting uncommitted events
- commits events if a key holder is reachable

Group B:
- same independent behavior
```

### 7.9.2. After Partition Heals

```
1. Nodes reconnect and run pool.exchange/events.sync
2. Signed datums from both sides merge deterministically
3. Committed envelopes (depth 2) supersede matching uncommitted envelopes
4. Never-committed datums remain uncommitted until ttl expiry or commit
```

Because datums are self-authenticating and immutable, merge logic is local
and deterministic.

## 7.10. Sequence Numbers (seq)

Sequence numbers are local indices assigned by each node for efficient
iteration. `seq` is NOT stored in datums.

```
1. Node stores committed datum in local database
2. Node assigns seq = max(existing_seq) + 1 for room+channel
3. seq is used for local queries (range scans)
4. seq is never used as cryptographic identity
5. Different nodes may assign different seq values to same datum
```

`seq` is an implementation detail, not a protocol proof.

## 7.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
