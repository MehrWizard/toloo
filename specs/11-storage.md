# 11. Storage

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [5] Rooms, [7] Replication

## 11.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 11.2. Storage Model

Toloo does not mandate a storage engine. Nodes MAY use any storage
implementation: SQL databases, key-value stores, flat files, or in-memory
structures.

This document defines retention classes and pruning policies, not storage
formats.

## 11.3. Retention Classes

Data is classified by retention requirements:

### 11.3.1. Permanent Data

MUST be stored for the lifetime of the node:

| Data | Rationale |
|------|-----------|
| Own Ed25519 signing keypair (`sig_priv`) | Node identity |
| Own X25519 encryption keypair (`enc_priv`) | Private message decryption |
| TOFU pins (known node signatures) | Prevents impersonation |
| Room private keys (if key holder) | Room authority |
| Room-internal channel events (`ch < 0`) for synced rooms | Membership and room state; retention-proof ([11.5.1]) |
| Own committed `room.join` envelope (per room) | The member's membership credential |

Losing permanent data means losing identity or authority. A member who
loses their committed join envelope can simply re-join (subject to room
join rules); a node that loses a room's internal channel history must
re-sync it from another node before it can evaluate membership again.

A member's own join envelope is self-contained proof: author signature +
room-key commit. Members present it when reconnecting to a node that has
no record of them (proof re-presentation). Membership verification of
*other* members' messages requires no member list at all: a committed
message carries the room key's depth-2 commit signature, which attests
membership at commit time.

### 11.3.2. Required Data (Per Room)

MUST be stored while the node is a member:

| Data | Rationale |
|------|-----------|
| Latest channel -1 metadata | Current room configuration |
| All committed channel -2 events | Required for membership reconstruction |

Nodes MUST NOT prune committed channel -2 events. These are needed to
reconstruct the current membership state (see [5#membership-reconstruction]).

### 11.3.3. Cacheable Data

MAY be pruned; can be re-fetched from peers:

| Data | Default Retention | Re-fetch Via |
|------|-------------------|--------------|
| Committed events (channels 0+) | Room's `retention` rule (e.g., 90 days) | `room.sync` |
| Old channel -1 metadata versions | Keep 3 most recent | `room.sync` |
| Blob pieces | Implementation-defined | `blob.fetch` |
| Attestations | 365 days | `pool.exchange` |

### 11.3.4. Temporary Data

Short-lived; expires automatically:

| Data | Lifetime | Reason |
|------|----------|--------|
| Uncommitted events | Room's `uncommitted_ttl` or node default | Becomes stale |
| TLS session keys | Until connection closes | Managed by the TLS stack |

## 11.4. Archival Nodes

Nodes MAY operate in **archival mode**: retaining full history regardless
of retention rules.

Archival nodes:
- Keep all committed events (never prune by age)
- Keep all diffs (no snapshots required)
- Serve historical data to new members
- Are not required for protocol operation

Public nodes (servers with reachable addresses) often run in archival mode.

## 11.5. Pruning Policies

### 11.5.1. Message Pruning

For committed events in content channels (0+):

```
1. Get room's retention rule: {"t": "retention", "days": 90}
2. Calculate cutoff: cutoff_ts = current_time - (retention_days * 86400000)
3. Delete committed events where tc < cutoff_ts
4. Keep uncommitted events per uncommitted_ttl policy
```

**Room-internal channels are retention-proof.** Nodes MUST NOT prune
committed events on any room-internal channel (`ch < 0` — metadata,
membership, and future room-state channels; see [A#reserved-channel-numbers]).
These events define room state (who is a member, current rules) and their
size is O(members + bans + edits), not O(messages). Content channels
(`ch >= 0`) are the only prunable class.

Uncommitted events on room-internal channels (e.g. a join the key holder
never committed) are NOT room state and expire per the uncommitted_ttl
policy like any other uncommitted event.

### 11.5.2. Uncommitted Event Expiry

For uncommitted events:

```
1. Get room's uncommitted_ttl rule: {"t": "uncommitted_ttl", "ms": 86400000}
2. Calculate expiry: expiry_ts = event.d.ts + uncommitted_ttl_ms
3. If current_time > expiry_ts, delete uncommitted event
4. If room has uncommitted_ttl=0 (online-only), MUST NOT store uncommitted events
```

### 11.5.3. Attestation Pruning

Nodes MAY prune attestations older than implementation-defined period
(suggested: 365 days).

Positive attestations (seeded, committed, available) MAY be retained longer.
Negative attestations (failed_proof, false_data) MAY be retained indefinitely.

### 11.5.4. Side-Event Pruning

Side-events (forks, announcements) MAY be pruned per local policy. Nodes
SHOULD retain fork announcements for rooms they are members of.

## 11.6. Garbage Collection

### 11.6.1. Committed Events

After pruning by age:

```
1. Delete datum from pool
2. Delete from local indices (seq, eid)
3. Update room metadata (last_seq, event_count)
4. Do NOT delete from channel -1 or channel -2
```

### 11.6.2. Uncommitted Events

After expiry or commitment:

```
1. If uncommitted event has matching committed variant (depth-2 envelope with same identity), delete uncommitted
2. If uncommitted event is expired, delete
3. Otherwise, retain
```

### 11.6.3. TOFU Pins

TOFU pins MUST NEVER be garbage-collected. Once a node is known, its
identity is permanent for this observing node.

## 11.7. Blob Piece Storage

Blob binary data is stored as independent pieces, each keyed by its
SHA-256 hash. The `room.blob` envelope provides the piece index
(`c.pieces[]`) that maps piece order to piece hashes.

### 11.7.1. Piece Independence

Each piece is a self-contained storage unit:
- Stored independently, keyed by piece hash
- Verified independently: `SHA-256(piece_data) == pieces[i]`
- Served independently via `blob.fetch` by piece hash
- A node MAY store a subset of pieces (partial seeding)

### 11.7.2. Seeding Modes

| Mode | Behavior |
|------|----------|
| Full seeder | Stores all pieces for a blob |
| Partial seeder | Stores some pieces (e.g., first 50%) |
| No blob storage | Stores no pieces, returns `not_found` for all `blob.fetch` |

Nodes track which piece hashes they have locally. When responding to
`blob.fetch` for a blob hash, the node returns only the pieces it has.

### 11.7.3. Piece Deduplication

Pieces are content-addressed. If two blobs happen to share identical
pieces (same data, same hash), the piece is stored once.

### 11.7.4. Piece Pruning

Blob pieces follow the same retention rules as their parent room's
content channel. When a `room.blob` envelope is pruned, its pieces
become eligible for garbage collection. Pieces referenced by other
(non-pruned) `room.blob` envelopes MUST NOT be pruned.

### 11.7.5. Schema Extension

SQLite:

```sql
CREATE TABLE pieces (
  piece_hash TEXT PRIMARY KEY,  -- SHA-256 hex (64 chars)
  data BLOB NOT NULL,           -- piece binary data
  size INTEGER NOT NULL,        -- piece size in bytes
  received_at INTEGER NOT NULL
);

CREATE TABLE blob_pieces (
  blob_hash TEXT NOT NULL,      -- SHA-256 of complete blob
  piece_index INTEGER NOT NULL, -- position in pieces array
  piece_hash TEXT NOT NULL,     -- references pieces.piece_hash
  PRIMARY KEY (blob_hash, piece_index)
);
```

Key-value store:

```
Key: piece:{PIECE_HASH}
Value: raw binary data

Key: blob_index:{BLOB_HASH}
Value: [PIECE_HASH_0, PIECE_HASH_1, ..., PIECE_HASH_N]
```

Flat files:

```
/data/
  blobs/
    pieces/
      {PIECE_HASH_PREFIX_4}/{PIECE_HASH}  # binary data
    index/
      {BLOB_HASH}.json                    # piece hash list
```

## 11.8. Storage Size Estimates

Approximate storage requirements per room:

| Data | Size | Example |
|------|------|---------|
| Metadata (channel -1) | <10 KB | Name, rules, config |
| Membership events (all time) | ~200 bytes/event | 1000 events = 200 KB |
| Messages (90 days, 1000 msgs/day) | ~1 KB/msg | 90,000 msgs = 90 MB |
| Attestations (1 year) | ~500 bytes each | 10,000 = 5 MB |

A node in 10 active rooms with 90-day retention: ~900 MB + overhead.

A node in 100 rooms (mostly inactive) with 30-day retention: ~500 MB + overhead.

An archival node with full history: unbounded (grows continuously).

## 11.9. Storage Backends

Suggested storage approaches (non-normative):

### 11.9.1. SQLite (Single-Node)

```sql
CREATE TABLE envelopes (
  eid TEXT PRIMARY KEY,        -- envelope identity (innermost d)
  depth INTEGER NOT NULL,      -- 1=uncommitted, 2=committed, 3=acknowledged
  envelope_json TEXT NOT NULL, -- complete {d, p} JSON
  inner_n TEXT NOT NULL,       -- innermost d.n (author), indexed
  inner_t TEXT NOT NULL,       -- innermost d.t (event type), indexed
  inner_r TEXT,                -- innermost d.r (room), indexed
  inner_ts INTEGER NOT NULL,   -- innermost d.ts (author timestamp)
  tc INTEGER,                  -- commit d.tc (NULL for depth 1)
  received_at INTEGER NOT NULL
);

CREATE INDEX idx_room_channel_tc ON envelopes(inner_r, tc) WHERE tc IS NOT NULL;
CREATE INDEX idx_uncommitted ON envelopes(inner_r, inner_ts) WHERE depth = 1;
```

### 11.9.2. PostgreSQL (Multi-Node)

Similar schema with:
- JSONB column for the complete envelope (enables indexed queries on nested fields)
- Partitioning by `inner_r` (room signature) for large deployments
- Separate tables for TOFU pins, room keys, attestations

### 11.9.3. Key-Value Store

```
Key: eid (envelope identity)
Value: {d: {...}, p: "..."}  // complete envelope at current depth

Secondary indices:
- room_tc:{ROOM_SIG}:{tc} → eid
- room_uncommitted:{ROOM_SIG}:{ts} → eid
- attestation_subject:{NODE_SIG}:{ts} → eid
```

### 11.9.4. Flat Files (Embedded/Mobile)

```
/data/
  rooms/
    {ROOM_SIG}/
      metadata.json         # Latest channel -1
      membership/            # All channel -2 committed events
      channels/
        0/
          committed/
            {tc}-{eid}.json
          uncommitted/
            {ts}-{eid}.json
  attestations/
    {NODE_SIG}/
      {ts}-{claim}.json
  tofu.json
  keys.json
```

## 11.10. Migration and Backup

### 11.10.1. Export Format

Nodes SHOULD support exporting data to a portable format:

```json
{
  "version": "0.2",
  "exported_at": 1710000000000,
  "node": {
    "sig_pub": "NODE_SIG",
    "sig_priv_encrypted": "...",
    "enc_pub": "NODE_X25519_PUB",
    "enc_priv_encrypted": "..."
  },
  "rooms": [
    {
      "r": "ROOM_SIG",
      "room_priv_encrypted": "...",  // if key holder
      "envelopes": [
        {"d": {...}, "p": "..."}
      ]
    }
  ],
  "tofu_pins": [
    {"sig": "NODE_SIG_1", "first_seen": 1700000000000},
    {"sig": "NODE_SIG_2", "first_seen": 1700000100000}
  ]
}
```

Private keys are encrypted with a user-provided passphrase.

### 11.10.2. Import

Nodes SHOULD support importing from the export format. Conflicts (duplicate
datums, conflicting TOFU pins) MUST be handled gracefully.

## 11.11. Performance Considerations

### 11.11.1. Indexing

Implementations SHOULD index:
- `(inner_r, tc)` for room sync queries
- `(inner_r, inner_ts)` for uncommitted events
- `inner_n` for attestation queries
- `eid` for deduplication

### 11.11.2. Caching

Implementations MAY cache:
- Latest channel -1 metadata (in memory)
- Computed channel -2 membership state (in memory)
- TOFU pins (in memory)
- Frequently accessed rooms (in memory)

### 11.11.3. Batching

Implementations SHOULD batch:
- Multiple uncommitted events committed in one transaction
- Multiple datums received during pool exchange
- Pruning operations (bulk delete old events)

## 11.12. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
