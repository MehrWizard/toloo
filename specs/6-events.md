# 6. Events

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts, [2] Data Model, [5] Rooms

## 6.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 6.2. Event Categories

Toloo has three categories of envelopes:

### 6.2.1. Uncommitted Events (Depth 1)

A depth-1 envelope signed by its author. The author is authenticated, but
the room has not yet acknowledged the event.

Whether uncommitted events are stored and displayed depends on the room's
`uncommitted_ttl` rule (see [5#uncommitted-ttl]):
- If TTL is `0`: Uncommitted events MUST NOT be stored or displayed
- If TTL is positive: Additive uncommitted events (messages) MAY be
  displayed with a "pending" indicator
- State-changing uncommitted events (joins, bans) are held until committed

Uncommitted events are tentatively ordered by `d.ts` (author timestamp).

### 6.2.2. Committed Events (Depth 2)

The room key holder wraps the author's depth-1 envelope, producing a
depth-2 envelope. The room's `d` layer contains:
- `d.n`: the room's public key
- `d.t`: `"commit"`
- `d.tc`: commit timestamp (authoritative ordering)
- `d.env`: the original author envelope `{d, p}`

The room's signature `p` covers `canonical(d)`, which includes the entire
inner envelope. Both the author's proof and the room's proof are
independently verifiable.

Committed events form the authoritative room log. They are ordered by
`d.tc`, then by innermost `d.ts` for events committed at the same
millisecond.

### 6.2.3. Acknowledged Events (Depth 3)

The original author wraps the committed envelope, producing a depth-3
envelope. This confirms the author received the room's commitment.

Depth-3 envelopes are terminal: nodes stop active forwarding and MAY
archive or garbage collect them sooner than unresolved envelopes.

Acknowledgment is OPTIONAL. Room event logs use depth-2 (committed)
envelopes as the canonical form. Depth-3 exists for delivery confirmation
in store-and-forward scenarios.

### 6.2.4. Side-Events

Signed by their creator, never committed by any room. Side-events remain
at depth 1 permanently. They include:
- Fork announcements (`side.fork`)
- Attestations (`side.attestation`)

Side-events propagate through pools but exist outside room event logs.

## 6.3. Uncommitted Event Structure

Uncommitted events targeting a room have this structure:

```json
{
  "d": {
    "n": "AUTHOR_SIG",
    "v": "0.2",
    "t": "room.message",
    "r": "ROOM_SIG",
    "ts": 1710000000000,
    "c": {"ch": 1, "body": "Hello, world"}
  },
  "p": "AUTHOR_SIGNATURE"
}
```

Required fields in `d`:
- `n`: Author's Ed25519 public key
- `v`: Protocol version
- `t`: Event type (see §6.6)
- `r`: Target room's Ed25519 public key
- `ts`: Timestamp when event was created
- `c`: Event-specific content (structure depends on `t`)

Optional fields in `d`:
- `nonce`: Proof-of-Work nonce (required if room has `join_pow` or
  `post_pow` rules)
- `exp`: Expiry timestamp (see [2#expiry])

## 6.4. Committed Event Structure (Envelope Wrapping)

A room key holder commits an uncommitted event by wrapping it in a
depth-2 envelope:

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000000500,
    "tc": 1710000000500,
    "env": {
      "d": {
        "n": "AUTHOR_SIG",
        "v": "0.2",
        "t": "room.message",
        "r": "ROOM_SIG",
        "ts": 1710000000000,
        "c": {"ch": 1, "body": "Hello, world"}
      },
      "p": "AUTHOR_SIGNATURE"
    }
  },
  "p": "ROOM_SIGNATURE"
}
```

Outer `d` fields:
- `n`: Room's Ed25519 public key (the signer of this layer)
- `v`: Protocol version
- `t`: `"commit"` (fixed value for room commit envelopes)
- `ts`: When the room key holder created this layer
- `tc`: Commit timestamp, used for canonical ordering
- `env`: The complete inner envelope `{d, p}` being wrapped

The outer `p` is the room's signature over `canonical(d)`, which
cryptographically covers the entire inner envelope (since `env` is inside
`d`).

Rules:
- `tc` MUST be greater than or equal to innermost `d.ts`
- The inner envelope MUST be valid (signature verifies)
- The inner `d.r` MUST equal the outer `d.n` (room committing its own
  events)
- Removing the outer layer recovers the original uncommitted envelope

### 6.4.1. Superseding

When a node receives a committed envelope (depth 2) whose identity
matches an uncommitted envelope (depth 1) already in its pool, the
committed envelope supersedes the uncommitted one. Identity is computed
from the innermost `d` per [2#envelope-identity].

## 6.5. Acknowledged Event Structure

The original author MAY wrap the committed envelope to confirm receipt:

```json
{
  "d": {
    "n": "AUTHOR_SIG",
    "v": "0.2",
    "t": "ack",
    "ts": 1710000001000,
    "env": {
      "d": {
        "n": "ROOM_SIG",
        "v": "0.2",
        "t": "commit",
        "ts": 1710000000500,
        "tc": 1710000000500,
        "env": {
          "d": {
            "n": "AUTHOR_SIG",
            "v": "0.2",
            "t": "room.message",
            "r": "ROOM_SIG",
            "ts": 1710000000000,
            "c": {"ch": 1, "body": "Hello, world"}
          },
          "p": "AUTHOR_SIGNATURE"
        }
      },
      "p": "ROOM_SIGNATURE"
    }
  },
  "p": "AUTHOR_ACK_SIGNATURE"
}
```

Depth-3 envelopes are terminal. Nodes SHOULD stop active forwarding and
MAY garbage collect them per local policy.

## 6.6. Event Types

Event types are namespaced by prefix:

| Prefix | Category | Depth 1 signer |
|--------|----------|-----------------|
| `room.*` | Room events | Author (`d.n`), committed by room at depth 2 |
| `side.*` | Side-events | Creator (never committed) |
| `node.*` | Node metadata | Node |

### 6.6.1. Room Event Types

#### room.create

Creates a new room. This is the first committed event in every room.

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
      "description": "Optional",
      "rules": [ ]
    }
  },
  "p": "CREATOR_SIGNATURE"
}
```

This event is typically committed immediately by the key holder wrapping
it in a depth-2 commit envelope. The room's private key is initially held
by the creator.

#### room.message

Sends a message to a room channel.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.message",
    "r": "ROOM_SIG",
    "ts": 1710000100000,
    "c": {
      "ch": 1,
      "body": "Hello, world"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields:
- `ch`: Channel number (integer, default 0 if omitted)
- `body`: Message text (string, max 4096 characters)

#### room.join

Requests to join a room.

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "room.join",
    "r": "ROOM_SIG",
    "ts": 1710000200000,
    "nonce": 12345
  },
  "p": "BOB_SIGNATURE"
}
```

The `nonce` field is required if the room has a `join_pow` rule.

#### room.leave

Leaves a room.

```json
{
  "d": {
    "n": "CAROL_SIG",
    "v": "0.2",
    "t": "room.leave",
    "r": "ROOM_SIG",
    "ts": 1710000300000
  },
  "p": "CAROL_SIGNATURE"
}
```

#### room.invite

Invites a node to join a room.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.invite",
    "r": "ROOM_SIG",
    "ts": 1710000400000,
    "c": {"invitee": "BOB_SIG"}
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields:
- `invitee`: Public key of the node being invited

#### room.ban

Bans a node from a room.

```json
{
  "d": {
    "n": "ADMIN_SIG",
    "v": "0.2",
    "t": "room.ban",
    "r": "ROOM_SIG",
    "ts": 1710000500000,
    "c": {
      "banned": "SPAMMER_SIG",
      "reason": "Spam"
    }
  },
  "p": "ADMIN_SIGNATURE"
}
```

Content fields:
- `banned`: Public key of the node being banned
- `reason`: (Optional) Human-readable reason (max 250 characters)

`room.ban` MAY be emitted preemptively for a node with no prior `room.join`
event in channel -2.

#### room.unban

Removes a ban.

```json
{
  "d": {
    "n": "ADMIN_SIG",
    "v": "0.2",
    "t": "room.unban",
    "r": "ROOM_SIG",
    "ts": 1710000600000,
    "c": {"unbanned": "REFORMED_SIG"}
  },
  "p": "ADMIN_SIGNATURE"
}
```

Content fields:
- `unbanned`: Public key of the node being unbanned

#### room.blob

Publishes blob (media/file) metadata to a room channel.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.blob",
    "r": "ROOM_SIG",
    "ts": 1710001200000,
    "c": {
      "ch": 1,
      "hash": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
      "size": 2097152,
      "piece_size": 262144,
      "pieces": ["aaa111...", "bbb222...", "ccc333...", "..."],
      "mime": "image/jpeg",
      "name": "photo.jpg"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields:
- `hash`: SHA-256 hex hash of the complete blob (64 chars). REQUIRED.
- `size`: Total blob size in bytes (integer). REQUIRED.
- `piece_size`: Bytes per piece (integer). REQUIRED. Last piece may be
  smaller. Suggested default: 262,144 (256 KB).
- `pieces`: Ordered array of SHA-256 hex hashes, one per piece. REQUIRED.
  `pieces[i]` is the hash of the i-th piece. For blobs smaller than
  `piece_size`, `pieces` has one entry equal to `hash`.
- `mime`: MIME type string. OPTIONAL.
- `name`: Original filename. OPTIONAL.

Pieces are independently storable, verifiable, and transferable. Nodes
may partially seed a blob (store a subset of pieces). See
[11#blob-piece-storage] for storage details.

Messages reference blobs via `c.blobs` (array of blob eids).

#### room.update

Updates room metadata (channel -1 only).

```json
{
  "d": {
    "n": "ADMIN_SIG",
    "v": "0.2",
    "t": "room.update",
    "r": "ROOM_SIG",
    "ts": 1710000800000,
    "c": {
      "name": "Updated Room Name",
      "rules": [ ]
    }
  },
  "p": "ADMIN_SIGNATURE"
}
```

This event updates room metadata. Only key holders can commit this event.

#### room.react

Adds a reaction to a message.

```json
{
  "d": {
    "n": "DAVE_SIG",
    "v": "0.2",
    "t": "room.react",
    "r": "ROOM_SIG",
    "ts": 1710000900000,
    "c": {
      "target": "0:1710000100000:ALICE_SIG",
      "emoji": "👍"
    }
  },
  "p": "DAVE_SIGNATURE"
}
```

Content fields:
- `target`: Event ID (eid) of the message being reacted to
- `emoji`: Emoji or reaction string (max 16 characters)

#### room.edit

Edits a previously sent message.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.edit",
    "r": "ROOM_SIG",
    "ts": 1710001000000,
    "c": {
      "target": "0:1710000100000:ALICE_SIG",
      "body": "Corrected message text"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields:
- `target`: Event ID (eid) of the message being edited
- `body`: New message text

Only the original author can edit their own messages. Clients SHOULD enforce
this constraint even before the edit is committed.

#### room.delete

Requests deletion of a message.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.delete",
    "r": "ROOM_SIG",
    "ts": 1710001100000,
    "c": {"target": "0:1710000100000:ALICE_SIG"}
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields:
- `target`: Event ID (eid) of the message to delete

Authors can delete their own messages. Moderators (as defined by room rules)
can delete any message.

### 6.6.2. Side-Event Types

#### side.fork

Announces a room fork.

```json
{
  "d": {
    "n": "FORKER_SIG",
    "v": "0.2",
    "t": "side.fork",
    "ts": 1710002000000,
    "c": {
      "original": "ORIGINAL_ROOM_SIG",
      "forked": "NEW_ROOM_SIG",
      "reason": "Governance split"
    }
  },
  "p": "FORKER_SIGNATURE"
}
```

Content fields:
- `original`: Public key of the room being forked
- `forked`: Public key of the new forked room
- `reason`: (Optional) Human-readable reason (max 250 characters)

#### side.attestation

Makes a claim about another node.

```json
{
  "d": {
    "n": "ATTESTER_SIG",
    "v": "0.2",
    "t": "side.attestation",
    "ts": 1710003000000,
    "c": {
      "subject": "NODE_SIG",
      "claim": "seeded",
      "detail": {"bytes": 52428800, "period_days": 30}
    }
  },
  "p": "ATTESTER_SIGNATURE"
}
```

Content fields:
- `subject`: Public key of the node being attested to
- `claim`: Claim type (see [8#attestation-claims])
- `detail`: (Optional) Claim-specific structured data

### 6.6.3. Private Event Types

Private events are 1:1 encrypted messages between two nodes. They use
`d.to` (not `d.r`) and MUST always be encrypted (see
[3#private-message-encryption]).

#### private.message

Sends an encrypted private message to another node.

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "private.message",
    "ts": 1710000000000,
    "to": "BOB_SIG",
    "c": {
      "v": 2,
      "from": "ALICE_ENC_X25519_PUB",
      "salt": "RANDOM_32_BYTE_SALT",
      "encrypted": "BASE64URL_CIPHERTEXT"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Content fields (v2):
- `v`: Payload version. REQUIRED for v2 and MUST be `2`.
- `from`: Sender X25519 encryption public key (base64url, 43 chars). REQUIRED.
- `salt`: Random 32-byte per-message salt (base64url, 43 chars). REQUIRED.
- `encrypted`: Base64url-encoded ciphertext. REQUIRED.

Plaintext content (inside `c.encrypted`): `{"body": "Hello Bob"}`

Validation: `v` absent selects legacy v1 (`c.eph`, `c.encrypted`). If `v`
is present it MUST be `2`, and missing/malformed `from`, `salt`, or
`encrypted` → DROP. Unencrypted private messages are forbidden. v1
payloads MAY be decrypted for stored history but MUST NOT be generated.

Depth lifecycle: Same as room events. Depth 1 (Alice→Bob), depth 2
(Bob wraps to acknowledge), depth 3 (Alice confirms). Signer alternation:
`d.n` → `d.to` → `d.n`.

**Depth-2 acknowledgment** (Bob wraps Alice's message):

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "ack",
    "ts": 1710000050000,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "private.message",
        "ts": 1710000000000,
        "to": "BOB_SIG",
        "c": {
          "v": 2,
          "from": "ALICE_ENC_X25519_PUB",
          "salt": "RANDOM_32_BYTE_SALT",
          "encrypted": "BASE64URL_CIPHERTEXT"
        }
      },
      "p": "ALICE_SIGNATURE"
    }
  },
  "p": "BOB_ACK_SIGNATURE"
}
```

The ack is NOT encrypted — it is a structural wrapper confirming Bob
received the message. The encrypted content is carried through unchanged
inside `env`. Alice sees the depth-2 envelope and knows Bob received it.

#### private.read

Optional read receipt for private messages.

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "private.read",
    "ts": 1710000100000,
    "to": "ALICE_SIG",
    "c": {
      "ids": ["1710000000000:ALICE_SIG"]
    }
  },
  "p": "BOB_SIGNATURE"
}
```

Content fields:
- `ids`: Array of direct envelope IDs (`{ts}:{n}`) being marked as read.

Read receipts are side-events (depth 1, never committed). They are
additive and OPTIONAL.

#### private.blob

Publishes encrypted blob metadata as a private message. Uses the same
encrypted envelope pattern as `private.message`. The plaintext content
contains blob metadata. The blob binary SHOULD be encrypted separately
(encryption key included in the encrypted metadata).

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "private.blob",
    "ts": 1710001300000,
    "to": "BOB_SIG",
    "c": {
      "v": 2,
      "from": "ALICE_ENC_X25519_PUB",
      "salt": "RANDOM_32_BYTE_SALT",
      "encrypted": "BASE64URL_CIPHERTEXT"
    }
  },
  "p": "ALICE_SIGNATURE"
}
```

Plaintext content (inside `c.encrypted`):
`{"hash": "...", "size": 2097152, "piece_size": 262144, "pieces": [...], "mime": "image/jpeg", "name": "photo.jpg", "blob_key": "..."}`

The `blob_key` is a symmetric key for decrypting the blob pieces. Each
piece is encrypted before transfer; the receiver decrypts each piece
individually after hash verification. The `pieces` array and `piece_size`
follow the same format as `room.blob`.

## 6.7. Action Classification

Events are classified by their impact on room state:

### 6.7.1. Additive Actions

Do not modify existing state, only add new data:
- `room.message`
- `room.react`
- `room.blob`
- `private.message`
- `private.read`
- `private.blob`

Additive uncommitted events MAY be displayed immediately (subject to
`uncommitted_ttl` policy) because they cannot break consistency.

### 6.7.2. State-Changing Actions

Modify room state and require authority:
- `room.join`
- `room.leave`
- `room.ban`
- `room.unban`
- `room.invite`
- `room.update`

State-changing uncommitted events MUST be held until committed (wrapped at
depth 2), regardless of `uncommitted_ttl` policy. They are not applied to
room state until the room commits them.

### 6.7.3. Editing Actions

Modify existing events:
- `room.edit`
- `room.delete`

Editing uncommitted events SHOULD be displayed tentatively (e.g., with a
strikethrough for deletes, updated text for edits) but are not applied to
canonical state until committed.

## 6.8. Event Immutability

Once committed, the inner envelope is immutable. The room's commit
signature cryptographically locks the author's envelope because `env` is
inside the signed `d` — any modification to the inner envelope would
invalidate the room's signature.

Edits and deletes do not modify the original event. They are separate events
that reference the target event's eid. Clients interpret these semantically
(e.g., hide deleted messages, show edited text).

## 6.9. Timestamps

### 6.9.1. Author Timestamp (ts)

The innermost `d.ts` field is the timestamp when the author created the
event. It is set by the author and may not be accurate (authors can lie
about time).

Uncommitted events are ordered by `d.ts` tentatively.

### 6.9.2. Commit Timestamp (tc)

The `d.tc` field on the commit layer (depth 2) is the timestamp when the
room committed the event. It is set by the key holder and is considered
authoritative.

Committed events are ordered by `tc`, then by innermost `d.ts` for events
committed at the same millisecond.

### 6.9.3. Timestamp Validity

Timestamps (`d.ts`, `d.tc`) are metadata, not validity criteria. Nodes
MUST NOT reject otherwise-valid envelopes based on timestamp alone.

Forwarding and storage decisions use `d.exp` (expiry) and room rules
(`uncommitted_ttl`, `retention`), not clock comparison against `d.ts`.

## 6.10. Event Identity (eid)

Events within a room channel are identified by their eid (see
[2#envelope-identity]):

```
eid = "{channel}:{ts}:{author_sig}"
```

The eid is deterministic and computed from the **innermost** `d`:
- `channel`: `d.c.ch` (default 0 if absent)
- `ts`: `d.ts`
- `author_sig`: `d.n`

The eid is stable across depths. Wrapping an event in a commit or
acknowledgment envelope does not change its eid.

## 6.11. Unknown Event Types

Nodes encountering unknown event types (e.g., `room.future_feature`) MUST
NOT reject the envelope. The signatures are still valid; the event type
is simply unknown to this implementation.

Nodes SHOULD:
- Store unknown event types if they are committed
- Forward unknown event types during pool exchange
- Display unknown event types with a placeholder in the UI (e.g.,
  "Unsupported event type: room.future_feature")

This allows protocol extensions without breaking existing implementations.

## 6.12. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
