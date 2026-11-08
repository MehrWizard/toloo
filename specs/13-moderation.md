# 13. Moderation

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [5] Rooms, [6] Events

## 13.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 13.2. Moderation Model

Toloo distinguishes between:
- **Local moderation**: Actions a node takes on its own data (blocking, flagging)
- **Room moderation**: Actions requiring room authority (banning, deleting)

Local moderation is immediate and private. Room moderation requires committing
by a key holder and is public to all members.

## 13.3. Local Moderation

Local moderation affects only the node performing the action.

### 13.3.1. Local Blocklist

Blocks a node entirely. All datums from the blocked node are rejected and
not stored.

```json
{
  "blocked": ["SPAMMER_SIG"],
  "updated_at": 1710000000000
}
```

This is local configuration state, not a protocol datum.

Effects:
- All future datums from `SPAMMER_SIG` are rejected at receipt
- Existing datums from `SPAMMER_SIG` in pool MAY be deleted
- Blocked node's messages are hidden in all rooms
- Blocked node cannot interact with this node via any mechanism

### 13.3.2. Local Unblock

Removes a block.

```json
{
  "blocked": [],
  "updated_at": 1710000001000
}
```

Future datums from the node are accepted again.

### 13.3.3. Local Spam Classification

Marks a node as spam without fully blocking. Datums are accepted but
quarantined or de-prioritized.

```json
{
  "spam": ["SUSPICIOUS_SIG"],
  "updated_at": 1710000002000
}
```

Effects (implementation-specific):
- Messages from spammer displayed with warning
- Messages moved to spam folder
- Spammer's attestations ignored
- Spammer not gossiped to peers

### 13.3.4. Local vs. Network Effects

Local moderation does NOT affect:
- Other nodes (they continue to see the blocked node's messages)
- Room state (the blocked node remains a member)
- Pool propagation (other nodes continue to exchange the blocked node's datums)

Local moderation is client-side filtering, not network-level enforcement.

## 13.4. Room Moderation

Room moderation affects all members and requires committing.

### 13.4.1. room.ban

Bans a node from a room.

Uncommitted event:
```json
{
  "d": {
    "n": "MODERATOR_SIG",
    "v": "0.2",
    "t": "room.ban",
    "r": "ROOM_SIG",
    "ts": 1710000000000,
    "c": {
      "banned": "VIOLATOR_SIG",
      "reason": "Repeated spam after warnings"
    }
  },
  "p": "MODERATOR_SIGNATURE"
}
```

This event must be committed by a key holder. Once committed:
- `VIOLATOR_SIG` is added to the banned set (channel -2)
- Future events from `VIOLATOR_SIG` are rejected by room rules
- Existing messages remain (ban is not retroactive)

### 13.4.2. room.unban

Removes a ban.

```json
{
  "d": {
    "n": "MODERATOR_SIG",
    "v": "0.2",
    "t": "room.unban",
    "r": "ROOM_SIG",
    "ts": 1710000100000,
    "c": {"unbanned": "REFORMED_SIG"}
  },
  "p": "MODERATOR_SIGNATURE"
}
```

Once committed, the node is removed from the banned set and may rejoin.

### 13.4.3. room.delete

Requests deletion of a message.

```json
{
  "d": {
    "n": "MODERATOR_SIG",
    "v": "0.2",
    "t": "room.delete",
    "r": "ROOM_SIG",
    "ts": 1710000200000,
    "c": {"target": "0:1710000000000:VIOLATOR_SIG"}
  },
  "p": "MODERATOR_SIGNATURE"
}
```

Semantics:
- Authors can delete their own messages (always allowed)
- Moderators (as defined by room rules) can delete any message
- Deletion does not remove the original message from the event log
- Clients interpret the deletion event by hiding the target message

Key holders decide whether to commit delete requests based on room rules.

### 13.4.4. Moderator Authorization

Room rules define who can perform moderation actions. Common patterns:

**Creator-only moderation:**
```json
{"t": "ban", "allow": ["CREATOR_SIG"]},
{"t": "delete", "allow": ["CREATOR_SIG"]}
```

**Multi-moderator:**
```json
{"t": "ban", "allow": ["CREATOR_SIG", "MOD1_SIG", "MOD2_SIG"]},
{"t": "delete", "allow": ["CREATOR_SIG", "MOD1_SIG", "MOD2_SIG"]}
```

**Member voting (via uncommitted_moderation):**
```json
{"t": "uncommitted_moderation", "moderators": "*"}
```

All members can flag uncommitted events; key holder decides whether to
commit based on flag count.

## 13.5. Anti-Spam Mechanisms

### 13.5.1. Proof-of-Work

Rooms MAY require PoW for joins or messages (see [5#join-pow], [5#post-pow]).

```json
{"t": "join_pow", "difficulty": 20},
{"t": "post_pow", "difficulty": 16}
```

PoW raises the computational cost of spam:
- 1 message at difficulty 16: ~1 second
- 1000 messages: ~1000 seconds (~17 minutes)
- 10,000 messages: ~10,000 seconds (~2.7 hours)

This is effective against bulk spam but not targeted harassment.

### 13.5.2. Rate Limiting

Rooms MAY rate-limit messages per node (see [5#rate-limit]).

```json
{"t": "rate_limit", "max": 10, "per": 60000}
```

Key holders SHOULD enforce rate limits when committing. Nodes MAY enforce
rate limits locally by rejecting excess uncommitted events.

### 13.5.3. Invite-Only

Rooms MAY require invitations to join (see [5#invite-only]).

```json
{"t": "invite_only", "inviters": ["CREATOR_SIG", "MOD1_SIG"]}
```

This prevents anonymous join spam. Invitations create accountability: the
inviter vouches for the invitee.

### 13.5.4. Designated Replication

Rooms MAY restrict data storage to designated nodes (see [5#replication]).

```json
{"t": "replication", "mode": "designated", "nodes": ["SERVER_SIG"]}
```

This prevents spam events from propagating widely. Only designated nodes
store room data; spammers cannot flood the network.

## 13.6. Moderation Transparency

All room moderation actions are publicly visible in the event log:

```
- Who was banned
- Who issued the ban
- When the ban occurred
- What reason was given
```

This provides accountability. Abusive moderators are visible to all members,
enabling fork decisions (see [14#forking]).

Local moderation (blocking, marking spam) is private and not logged.

## 13.7. Moderation vs. Censorship

Toloo's architecture resists censorship at the protocol level:
- No central authority can ban a node globally
- Room keys are portable (can move to different infrastructure)
- Forking is a first-class operation (escape abusive moderation)
- Data is verifiable (cannot be silently tampered with)

However, room-level moderation is necessary for usable communities. The
protocol provides tools; communities decide policy.

## 13.8. Flagging Uncommitted Events

Nodes MAY flag uncommitted events without waiting for commitment.

### 13.8.1. Flagging Mechanism

```json
{
  "d": {
    "n": "FLAGGER_SIG",
    "v": "0.2",
    "t": "room.flag",
    "r": "ROOM_SIG",
    "ts": 1710000300000,
    "c": {
      "target": "0:1710000000000:SPAMMER_SIG",
      "reason": "spam"
    }
  },
  "p": "FLAGGER_SIGNATURE"
}
```

This is a side-event (not committed by the room). It expresses the flagger's
opinion that the target event should be hidden or rejected.

### 13.8.2. Flag Aggregation

Clients MAY aggregate flags:

```
For uncommitted event E:
  flags = count(room.flag events where target = E.eid)
  if flags > THRESHOLD:
    hide E with "flagged by community" indicator
```

Thresholds are implementation-specific (e.g., 3 flags from distinct nodes).

### 13.8.3. Flag Weight

Flags from high-trust nodes MAY carry more weight:

```
weighted_flags = sum(trust_score(flagger) for each flag)
```

This resists flag spam from new or low-trust accounts.

## 13.9. Moderation Bots

Nodes MAY run automated moderation:

### 13.9.1. Auto-Ban on Patterns

```
if message.body matches SPAM_REGEX:
  submit room.ban(author, reason="automated spam detection")
```

### 13.9.2. Auto-Flag on Heuristics

```
if message.body contains URL and author.age < 24_hours:
  submit room.flag(message.eid, reason="new account posting links")
```

### 13.9.3. Bot Accountability

Moderation bots are just nodes. Their actions are signed and visible. If a
bot misbehaves, it can be blocked or banned like any other node.

## 13.10. Appeal Mechanisms

The protocol does not define appeal mechanisms. These are social processes:

- Banned member contacts moderator out-of-band
- Banned member joins a related room to request unban
- Community votes on unban (external to protocol)

Alternatively, the banned member can fork the room and invite others to join
the fork (see [14#forking]).

## 13.11. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
