# 14. Lifecycle

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [5] Rooms, [6] Events

## 14.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 14.2. Room Lifecycle Stages

Rooms progress through these stages:

| Stage | Description |
|-------|-------------|
| **Creation** | Room keypair generated, initial metadata committed |
| **Active** | Members join, messages flow, state evolves |
| **Dormant** | No key holder online, state frozen, messages accumulate as uncommitted |
| **Forked** | Members create new room from existing history |
| **Abandoned** | All members stop storing/seeding data |

## 14.3. Room Creation

Room creation is defined in [5#room-creation]. Summary:

```
1. Generate Ed25519 keypair for room
2. Create room.create event with initial metadata
3. Wrap in depth-2 commit envelope (see [6#committed-event-structure])
4. Distribute to initial members
5. Room is now active
```

The creator becomes the initial key holder.

## 14.4. Key Transfer

Room authority (the private key) can be transferred between nodes.

### 14.4.1. Transfer Process

Key transfer happens **out-of-band** (not via the Toloo protocol):

```
1. Current key holder exports room private key
2. Key is encrypted with a passphrase or public key
3. Encrypted key is transmitted via secure channel (PGP email, encrypted file, QR code)
4. Recipient decrypts and imports key
5. Recipient can now commit events
6. Original key holder MAY continue committing or MAY delete their copy
```

There is no protocol-level key transfer mechanism. This is intentional:
private keys are sensitive and should be handled out-of-band.

### 14.4.2. Migration Announcement

After key transfer, the new key holder MAY announce the migration:

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "room.migrate",
    "r": "ROOM_SIG",
    "ts": 1710000000000,
    "c": {
      "from": "OLD_HOLDER_SIG",
      "to": "NEW_HOLDER_SIG",
      "reason": "Infrastructure migration"
    }
  },
  "p": "ROOM_SIGNATURE"
}
```

This is informational and optional. The room's identity (`ROOM_SIG`) does
not change; only the location of the private key changes.

### 14.4.3. Multi-Key-Holder Rooms

Multiple nodes can hold the room key simultaneously:

```
Key holder A commits event E1 at tc=1000
Key holder B commits event E2 at tc=1001

Both commits are valid.
Both propagate through the network.
Events are ordered by tc (then by eid if tc is equal).
```

This provides:
- Redundancy (key loss by one holder does not kill the room)
- Geographic distribution (key holders in different regions)
- Improved availability (at least one key holder usually online)

## 14.5. Forking

Forking creates a new room from an existing room's history.

### 14.5.1. When to Fork

Forking is appropriate when:
- Room key is permanently lost
- Key holder is unresponsive for extended period
- Key holder is malicious or abusive
- Community splits (governance dispute, philosophy split)

Forking is **not** appropriate for:
- Temporary key holder absence (use uncommitted events)
- Minor disputes (use moderation or leave)
- Network partitions (pool handles these automatically)

### 14.5.2. Fork Process

```
1. Forking member generates new Ed25519 keypair for forked room
2. Forking member copies committed events from original room
3. Forking member creates room.create event for new room
4. Forking member announces fork via side.fork event
5. Other members decide whether to join the fork
```

The fork is a completely independent room with a new identity.

### 14.5.3. Fork Announcement

```json
{
  "d": {
    "n": "FORKER_SIG",
    "v": "0.2",
    "t": "side.fork",
    "ts": 1710000500000,
    "c": {
      "original": "ORIGINAL_ROOM_SIG",
      "forked": "NEW_ROOM_SIG",
      "reason": "Original key lost; community consensus to fork"
    }
  },
  "p": "FORKER_SIGNATURE"
}
```

This side-event propagates through pools, informing members of the fork.

### 14.5.4. Fork History Preservation

The forked room MAY include:
- All committed events from the original room (preserving history)
- Selected committed events (partial history)
- No history (clean start)

Original events retain their original proofs (signed by the original room
and authors). These proofs remain valid in the fork.

### 14.5.5. Fork vs. New Room

| Aspect | Fork | New Room |
|--------|------|----------|
| History | Includes original room's history | Starts fresh |
| Members | Inherit from original | Start with creator only |
| Announcement | via side.fork | No announcement |
| Social signal | Continuity of community | New community |

## 14.6. Key Loss

If a room's private key is lost:

### 14.6.1. Immediate Effects

- No new state changes can be committed (joins, bans, metadata updates)
- Additive events (messages) continue as uncommitted
- Room becomes read-only for state-changing operations

### 14.6.2. Recovery Options

**Option 1: Fork**
- Generate new keypair
- Copy history to new room
- Announce fork via side.fork
- Members migrate to fork

**Option 2: Continue Read-Only**
- Room remains accessible
- Uncommitted messages flow (if uncommitted_ttl allows)
- No new members can join
- Eventually members abandon the room

**Option 3: Abandon**
- All members stop storing/seeding data
- Room effectively ceases to exist

There is no protocol-level key recovery. Key loss is permanent.

## 14.7. Room Abandonment

Rooms are abandoned when all members stop storing and seeding data.

### 14.7.1. Abandonment is Gradual

```
1. Key holder goes offline
2. Active members leave
3. Remaining members stop participating
4. Eventually, no node stores room data
5. Room ceases to exist in practice
```

There is no protocol-level "delete room" operation. Abandonment is a natural
consequence of all members choosing to leave.

### 14.7.2. Data Retention After Abandonment

Archival nodes MAY retain data from abandoned rooms indefinitely. This is
an operational decision, not a protocol requirement.

If a single node retains data, the room can be "revived" by:
- Original key holder returning (can commit again)
- Members re-joining (if key holder is active)
- Forking (if key is lost)

## 14.8. Merge (Anti-Pattern)

Toloo does not support room merging. Merging two rooms with independent
histories creates conflicts:
- Event ID collisions
- Conflicting membership states
- Ambiguous event ordering

If two communities want to merge:

**Option 1: Create New Room**
- Create new room with combined initial membership
- Announce via room.update in both original rooms
- Members migrate organically

**Option 2: One Absorbs the Other**
- Invite all members from Room A to Room B
- Announce Room A is deprecated
- Members migrate to Room B

Historical data is not merged; each room retains its own history.

## 14.9. Metadata Evolution

Room metadata (channel -1) can evolve over time.

### 14.9.1. Metadata Updates

Key holders can update metadata:

```json
{
  "d": {
    "n": "KEY_HOLDER_SIG",
    "v": "0.2",
    "t": "room.update",
    "r": "ROOM_SIG",
    "ts": 1710001000000,
    "c": {
      "name": "New Room Name",
      "rules": [ ]
    }
  },
  "p": "KEY_HOLDER_SIGNATURE"
}
```

Once committed, this supersedes previous metadata. Nodes SHOULD use the
latest committed metadata.

### 14.9.2. Rule Changes

Rule changes take effect immediately upon commit. Example:

```
1. Room has rule: {"t": "post", "allow": "*"}
2. Key holder updates to: {"t": "post", "allow": ["ADMIN_SIG"]}
3. Update is committed at tc=1710001000000
4. All nodes switch to new rules
5. Non-admin uncommitted messages posted after tc=1710001000000 are rejected
```

Rule changes are not retroactive. Existing committed events remain valid
even if they would violate new rules.

### 14.9.3. Metadata Conflicts

If multiple key holders update metadata concurrently:

```
Key holder A updates at tc=1710001000000
Key holder B updates at tc=1710001000500

Both commits are valid.
The update with later tc wins (B's update).
All nodes converge to B's metadata.
```

Last-write-wins based on tc. This is deterministic; all nodes converge.

## 14.10. Deprecation

To deprecate a room:

```
1. Key holder updates metadata with deprecation notice:
   {"name": "Old Room (DEPRECATED)", "description": "Moved to NEW_ROOM_SIG"}
2. Key holder stops committing new events
3. Key holder announces deprecation via room.update
4. Members migrate to new room
5. Old room becomes dormant
```

There is no protocol enforcement of deprecation. It is a social signal.

## 14.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
