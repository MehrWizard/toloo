# 5. Rooms

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts, [2] Data Model, [3] Cryptography, [4] Identity

## 5.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 5.2. Room Identity

A room is identified by its Ed25519 public key (`room_sig`). This key is
permanent and non-rotatable. The public key is represented as a 43-character
base64url string (see [2#base64url]).

The corresponding private key (`room_priv`) is the **room authority**. Any
node possessing `room_priv` can commit events to the room (see [6#committing]).

## 5.3. Room Creation

To create a room, a node performs the following steps locally:

```
1. Generate a new Ed25519 keypair: (room_priv, room_pub)
2. Create depth-1 room.create envelope:
   d = {n: CREATOR_SIG, v: "0.2", t: "room.create", r: room_pub, ts: now,
        c: {name: "...", rules: [...]}}
   p = Ed25519.sign(creator_priv, canonical(d))
3. Commit it immediately (creator holds room_priv):
   commit_d = {n: room_pub, v: "0.2", t: "commit", ts: now, tc: now,
               env: {d, p}}
   commit_p = Ed25519.sign(room_priv, canonical(commit_d))
4. Store the depth-2 envelope in local pool
5. Submit to peers via pool.exchange
```

The creator is the initial **key holder** because they generated
`room_priv`. The key MAY be transferred to other nodes or replicated
across multiple nodes.

## 5.4. Channels

A room is divided into **channels**, which are numbered sub-logs within the
room. Channels enable independent sync, retention, and prioritization of
different event types.

### 5.4.1. Channel Numbering

Channels are identified by signed integers:

| Range | Purpose |
|-------|---------|
| Negative (`< 0`) | Reserved for room infrastructure |
| Zero and positive (`>= 0`) | User content and custom channels |

### 5.4.2. Reserved Channels

Two channels are reserved with special semantics:

**Channel -1: Metadata**
- Contains room metadata (name, description, rules, configuration)
- Events in this channel update room-level settings
- Only key holders can publish to channel -1 (via committing)
- All members MUST subscribe to channel -1

**Channel -2: Membership**
- Contains individual committed membership events (joins, leaves, bans, invites)
- Only key holders can publish to channel -2 (via committing)
- All members MUST subscribe to channel -2

**Retention-proof.** All room-internal channels (`ch < 0`, including future
room-state channels such as blob mappings) MUST NOT be pruned by nodes:
they define room state and are small (O(members + bans + edits), not
O(messages)). See [11#1151-message-pruning] and [A#reserved-channel-numbers].

### 5.4.2.1. Channel 0: Official Announcements

Channel 0 is the room's **official public voice** — announcements, news,
public posts. It is the contentful face of the room (alongside the
metadata profile on channel -1):

- Events on channel 0 MUST be `room.announce`, authored **by the room key
  itself**: `d.n == d.r`. The room key author-signs and self-commits the
  announcement, so its authenticity is self-evident from the signature
  chain — members cannot forge announcements, and per-member posting
  rules do not apply.
- Clients SHOULD surface announcements across all channels (pinned-style).
  Opening an announcement reveals channel 0, the room's official channel.

### 5.4.2.2. Room Watermark

### 5.4.2.1. Room Watermark

The **room watermark** is the latest commit time (`tc`) across all
room-internal channels. Every room-internal event is committed by the room
key, so the watermark is implicitly room-key-signed.

`room.sync` responses begin with an advisory control frame
`{"wr": <watermark>}`. A client whose known watermark is behind the
relay's has not seen recent room-internal events (e.g. a ban affecting it)
and SHOULD re-sync room-internal channels before posting. A client only
needs room-internal events from its **own committed join** onward: bans of
members who joined before it do not affect its eligibility, and other
members' messages are covered by the room key's per-message commit
signature. The watermark is advisory; posting eligibility is enforced by
the key holder at commit time, so a forged watermark can cause at worst
extra syncs, never false acceptance.

### 5.4.3. Channel Assignment by Event Type

The channel for an event is determined by its type, not by a `c.ch` field
(except for content events on channels 0+):

| Event Type | Channel | `c.ch` Field |
|------------|---------|-------------|
| `room.create` | -1 | Implicit (always -1) |
| `room.update` | -1 | Implicit (always -1) |
| `room.join` | -2 | Implicit (always -2) |
| `room.leave` | -2 | Implicit (always -2) |
| `room.ban` | -2 | Implicit (always -2) |
| `room.unban` | -2 | Implicit (always -2) |
| `room.invite` | -2 | Implicit (always -2) |
| `room.message` | `c.ch` | Required (default 0) |
| `room.react` | Same as target | Implicit (from target eid) |
| `room.edit` | Same as target | Implicit (from target eid) |
| `room.delete` | Same as target | Implicit (from target eid) |
| `room.blob` | `c.ch` | Required (default 0) |

For state events (join, leave, ban, etc.), the channel is fixed by type.
Implementations MUST reject state events that claim a channel inconsistent
with their type. For content events, `c.ch` determines the channel and
is used in eid computation.

### 5.4.4. Content Channels

Channels 0 and above are for user-generated content:

**Channel 0 (default)**
- General messages and discussions
- Used when no specific channel is specified

**Channels 1+**
- May be created for specific topics, threads, or purposes
- Channel semantics are application-defined
- The protocol treats all content channels identically

## 5.5. Channel -1: Metadata

Channel -1 contains room metadata events. The latest event in this channel
represents the current room state.

### 5.5.1. Metadata Schema

```json
{
  "d": {
    "n": "CREATOR_SIG",
    "v": "0.2",
    "t": "room.create",
    "r": "ROOM_SIG",
    "ts": 1710000000000,
    "c": {
      "name": "My Room",
      "description": "Optional description",
      "rules": [ ]
    }
  },
  "p": "CREATOR_SIGNATURE"
}
```

This is the uncommitted event (depth 1). The key holder commits it by
wrapping in a depth-2 commit envelope (see [6#committed-event-structure]):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000001000,
    "tc": 1710000001000,
    "env": {
      "d": {
        "n": "CREATOR_SIG",
        "v": "0.2",
        "t": "room.create",
        "r": "ROOM_SIG",
        "ts": 1710000000000,
        "c": {
          "name": "My Room",
          "description": "Optional description",
          "rules": [ ]
        }
      },
      "p": "CREATOR_SIGNATURE"
    }
  },
  "p": "ROOM_SIGNATURE"
}
```

### 5.5.2. Metadata Fields

| Field in `c` | Type | Presence | Description |
|--------------|------|----------|-------------|
| `name` | string | REQUIRED | Room name (1-256 characters) |
| `description` | string | OPTIONAL | Room description (max 1024 characters) |
| `rules` | array | OPTIONAL | Array of rule objects (see §5.7) |

### 5.5.3. Metadata Updates

To update room metadata, a key holder publishes a new event to channel -1.
The new event supersedes all previous metadata events. Nodes SHOULD retain
only the latest metadata event and MAY prune older ones.

## 5.6. Channel -2: Membership Events

Channel -2 contains individual committed membership events. There are no
snapshots; membership is reconstructed by replaying all committed events
in channel -2.

### 5.6.1. Membership State

The membership state consists of:

- **Members**: Set of node signatures currently joined
- **Banned**: Set of node signatures currently banned

### 5.6.2. Membership Events

Individual membership changes are recorded as committed events:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.join",
    "r": "ROOM_SIG",
    "ts": 1710000000000
  },
  "p": "ALICE_SIGNATURE"
}
```

Event types:
- `room.join`: Node joins the room (adds to members set)
- `room.leave`: Node leaves the room (removes from members set)
- `room.ban`: Node is banned (ban MAY be preemptive, even if node never joined)
- `room.unban`: Node is unbanned (removes from banned set)
- `room.invite`: Prerequisite for join in invite-only rooms (no state change)

All membership events are uncommitted until committed by a key holder.

### 5.6.3. Membership Reconstruction

To reconstruct current membership from channel -2:

```
members = Set(), banned = Set()
for event in channel_minus_2_events ordered by tc:
  inner = innermost(event)
  match inner.d.t:
    "room.join":   if inner.d.n not in banned → members.add(inner.d.n)
    "room.leave":  members.discard(inner.d.n)   // no-op if not a member
    "room.ban":    members.discard(c.banned); banned.add(c.banned)
    "room.unban":  banned.discard(c.unbanned)    // no-op if not banned
    "room.invite": pass  // prerequisite for join, no state change
```

Edge cases (all are no-ops — implementations MUST NOT reject them):
- `room.join` for a node already in `members`: no state change (idempotent)
- `room.leave` for a node not in `members`: ignored
- `room.unban` for a node not in `banned`: ignored
- `room.ban` for a node not in `members`: adds to `banned` only (preemptive ban)
- Duplicate events with same eid: skip (eid deduplication)

Membership proof: a node is a member if and only if it has a committed
`room.join` event with no subsequent committed `room.ban` or `room.leave`
event for that node.

Nodes MUST NOT prune committed channel -2 events, as they are required
for membership reconstruction.

### 5.6.4. Proof-Carrying Membership

Membership is a **capability**, not a roster entry. There are no member
lists in the protocol; nothing in verification requires materializing one.

- **A member's credential is their committed `room.join` envelope.** It is
  self-contained: author signature + room-key commit signature. Members
  SHOULD retain it. A member presenting this envelope to a node that has
  no record of them (proof re-presentation) is re-admitted after the node
  verifies the signature chain and checks the envelope against its
  retained ban history.
- **Verifying other members' messages requires no member list.** Every
  committed room message carries a depth-2 commit signed by the room key;
  the key holder checks membership before committing, so a valid commit
  signature attests membership at commit time. Uncommitted messages from
  members a client cannot vouch for remain in the pending state until
  committed (or expire per the uncommitted TTL).
- **Bans are the unprunable core.** `room.ban` events (and `room.leave`,
  by policy) are never pruned and cannot be presented away: a re-presented
  join proof is checked against the node's retained ban events with a
  later `tc`.
- **Nodes MAY forget inactive members** from their working state (bounded
  memory); the forgotten member re-presents their credential on return.
  Full channel -2 replay remains the catch-up path for nodes that retain
  everything.

Rooms that treat membership as time-limited MAY require join freshness
via the `exp` field on `room.join` events or a room rule: stale
credentials must be re-presented as a fresh `room.join` (author signs,
room key commits — a cheap, normal operation).

## 5.7. Room Rules

Room rules define access control and behavior policies. Rules are stored in
the `rules` array of the latest channel -1 metadata.

### 5.7.1. Rule Structure

Each rule is an object with a `t` (type) field and type-specific parameters:

```json
{"t": "rule_type", "param1": "value1", "param2": "value2"}
```

### 5.7.2. Rule Types

#### join

Controls who may join the room.

```json
{"t": "join", "allow": "*"}
{"t": "join", "allow": ["SIG_1", "SIG_2"]}
{"t": "join", "deny": ["SIG_BANNED"]}
```

Parameters:
- `allow`: Either `"*"` (anyone) or array of allowed node signatures
- `deny`: Array of denied node signatures

#### post

Controls who may post messages.

```json
{"t": "post", "allow": "*"}
{"t": "post", "allow": ["SIG_1", "SIG_2"]}
{"t": "post", "allow": "members"}
```

Parameters:
- `allow`: Either `"*"` (anyone), `"members"` (current members only), or array of allowed node signatures
- `deny`: Array of denied node signatures

#### invite_only

Requires an invitation before joining.

```json
{"t": "invite_only", "inviters": ["CREATOR_SIG", "ADMIN_SIG"]}
```

Parameters:
- `inviters`: Array of node signatures authorized to issue invitations

When this rule is present, a node MUST have a committed `room.invite` event
before their `room.join` will be committed.

#### join_pow

Requires Proof-of-Work to join.

```json
{"t": "join_pow", "difficulty": 20}
```

Parameters:
- `difficulty`: Number of leading zero bits required in SHA-256(canonical(d))

The `room.join` event MUST include a `nonce` field satisfying the PoW
requirement (see [3#pow]).

#### post_pow

Requires Proof-of-Work for each message.

```json
{"t": "post_pow", "difficulty": 16}
```

Parameters:
- `difficulty`: Number of leading zero bits required

All `room.message` events MUST include a `nonce` field satisfying the PoW
requirement.

#### rate_limit

Limits message rate per node.

```json
{"t": "rate_limit", "max": 10, "per": 60000}
{"t": "rate_limit", "max": 100, "per": 60000, "for": ["SIG_ADMIN"]}
```

Parameters:
- `max`: Maximum number of messages (integer)
- `per`: Time window in milliseconds (integer)
- `for`: (Optional) Array of node signatures to which this limit applies. If omitted, applies to all nodes.

Nodes SHOULD enforce rate limits locally by rejecting messages that exceed
the limit. Key holders SHOULD NOT commit events that violate rate limits.

#### retention

Suggests client-side retention period for messages.

```json
{"t": "retention", "days": 90}
```

Parameters:
- `days`: Suggested retention period in days (integer)

This is advisory. Nodes MAY prune messages older than this period. Archival
nodes MAY ignore this and keep full history.

#### uncommitted_ttl

Controls how long uncommitted events may be held.

```json
{"t": "uncommitted_ttl", "ms": 86400000}
{"t": "uncommitted_ttl", "ms": 0}
```

Parameters:
- `ms`: Time-to-live in milliseconds (uint64)

Special values:
- `0`: Online-only mode. Uncommitted events MUST NOT be stored or displayed. Events are rejected if no key holder is reachable.
- Positive value: Uncommitted events MAY be stored and displayed for up to this duration.
- Omitted: No protocol-level limit (node decides).

#### replication

Controls who may store and serve room data.

```json
{"t": "replication", "mode": "open"}
{"t": "replication", "mode": "members"}
{"t": "replication", "mode": "designated", "nodes": ["SIG_1", "SIG_2"]}
```

Parameters:
- `mode`: One of "open", "members", or "designated"
- `nodes`: (Required if mode is "designated") Array of node signatures authorized to serve data

Modes:
- `open`: Any node may store and seed room data (default if omitted)
- `members`: Only current room members SHOULD store and seed room data
- `designated`: Only the listed nodes SHOULD store and serve room data

This rule is advisory. Nodes SHOULD honor it, but a modified client can
ignore it and redistribute data.

#### history

Controls who may read historical messages.

```json
{"t": "history", "allow": "members"}
{"t": "history", "allow": "*"}
```

Parameters:
- `allow`: Either `"members"` (only current members) or `"*"` (anyone)

This rule is advisory and cannot be cryptographically enforced without
end-to-end encryption.

#### uncommitted_moderation

Allows trusted nodes to moderate uncommitted events.

```json
{"t": "uncommitted_moderation", "moderators": ["SIG_MOD_1", "SIG_MOD_2"]}
```

Parameters:
- `moderators`: Array of node signatures authorized to flag uncommitted events

Moderators can publish `room.flag` side-events indicating that an uncommitted
event should be hidden or rejected. Nodes SHOULD respect these flags even
before the event is committed or rejected by a key holder.

### 5.7.3. Rule Evaluation

Rules are evaluated in order. The last matching rule determines the outcome.

Example:

```json
{
  "rules": [
    {"t": "post", "allow": "*"},
    {"t": "post", "deny": ["SIG_SPAMMER"]},
    {"t": "post", "allow": ["SIG_SPAMMER"]}
  ]
}
```

Evaluation for `SIG_SPAMMER`:
1. Rule 1 matches (allow: "*") → ALLOW
2. Rule 2 matches (deny specific) → DENY
3. Rule 3 matches (allow specific) → ALLOW
4. Final result: ALLOW (last matching rule)

If no rule matches an action, the default is DENY.

### 5.7.4. Rule Extensibility

Nodes encountering unknown rule types MUST skip them (not error). This
allows protocol extensions without breaking existing implementations.

Nodes MUST NOT reject a room's metadata solely because the `rules` array
contains unknown rule types.

## 5.8. Portable Authority

The room's private key (`room_priv`) is portable data. It can live on any
device or be replicated across multiple devices. Possession of `room_priv`
grants the ability to commit events.

### 5.8.1. Key Holders

A **key holder** is any node that possesses `room_priv`. There may be zero,
one, or many key holders at any time:

- **Zero key holders**: Room is read-only. Uncommitted events queue until a key holder appears.
- **One key holder**: Standard case. Single authority commits events.
- **Many key holders**: Multiple nodes can commit in parallel. Commits merge via timestamp ordering.

### 5.8.2. Key Transfer

To transfer room authority:

1. Current key holder exports `room_priv` (out-of-band, encrypted)
2. Recipient imports `room_priv` into their node
3. Recipient can now commit events to the room
4. Original key holder MAY continue committing or MAY delete their copy

There is no protocol-level key transfer mechanism. Transfer happens outside
the protocol (encrypted file, QR code, secure channel).

### 5.8.3. Key Loss

If `room_priv` is permanently lost:
- Additive uncommitted events (messages) continue to flow
- State-changing uncommitted events (joins, bans) remain uncommitted forever
- Members can fork the room (see [14#forking])

Key loss does not destroy the room, but makes it impossible to commit new
state changes.

## 5.9. Room Discovery

Rooms are discovered via:

1. **Direct sharing**: `toloo://` URIs containing room's committed `room.create` envelope and peer `node.meta` envelopes
2. **Node metadata**: Creators include room references in their `node.meta`
3. **Pool gossip**: Committed events propagate through pools (if replication policy allows)

No centralized room directory exists at the protocol level.

## 5.10. Room Lifetime

A room exists as long as at least one node stores its data. Rooms do not
"expire" or get "deleted" at the protocol level.

To "delete" a room, all members must:
1. Stop storing room data
2. Stop seeding room data to peers

This is a social/operational action, not a protocol action.

## 5.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
