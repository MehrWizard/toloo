# Appendix B: Wire Examples

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md)

## B.1. Purpose

This appendix provides annotated protocol traces for common operations.
Examples use shortened placeholder keys/signatures for readability.

In production:
- Ed25519/X25519 public keys are 32-byte values (43-char base64url)
- Ed25519 signatures are 64-byte values (86-char base64url)
- Timestamps are Unix milliseconds

## B.2. Notation

```
Alice (ALICE_SIG)  - Regular user
Bob (BOB_SIG)      - Regular user
Carol (CAROL_SIG)  - Room creator / key holder
Relay (RELAY_SIG)  - Public relay node

→  Send
←  Receive
// Inline comment
```

## B.4. Room Creation

### B.4.1. Creating a New Room

**Step 1: Carol creates room.create request datum**

```json
{
  "d": {
    "n": "CAROL_SIG",
    "v": "0.2",
    "t": "room.create",
    "ts": 1710000000000,
    "c": {
      "name": "Secret Project",
      "description": "Planning for launch",
      "rules": [
        {"t": "post", "allow": "*"},
        {"t": "invite_only", "inviters": ["CAROL_SIG"]},
        {"t": "rate_limit", "max": 10, "per": 60000},
        {"t": "retention", "days": 90},
        {"t": "uncommitted_ttl", "ms": 86400000},
        {"t": "replication", "mode": "members"}
      ]
    }
  },
  "p": "CAROL_SIG_ROOM_CREATE_REQ"
}
```

**Step 2: Key holder emits the room.create event datum**

Uncommitted event (depth 1):

```json
{
  "d": {
    "n": "CAROL_SIG",
    "v": "0.2",
    "t": "room.create",
    "r": "ROOM_SIG",
    "ts": 1710000000100,
    "c": {
      "ch": -1,
      "name": "Secret Project",
      "description": "Planning for launch",
      "rules": [
        {"t": "post", "allow": "*"},
        {"t": "invite_only", "inviters": ["CAROL_SIG"]},
        {"t": "rate_limit", "max": 10, "per": 60000},
        {"t": "retention", "days": 90},
        {"t": "uncommitted_ttl", "ms": 86400000},
        {"t": "replication", "mode": "members"}
      ]
    }
  },
  "p": "CAROL_SIG_ROOM_CREATE_EVENT"
}
```

**Step 3: Commit via depth-2 envelope**

The room key holder wraps the author's envelope inside a commit envelope:

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000000200,
    "tc": 1710000000200,
    "env": {
      "d": {
        "n": "CAROL_SIG",
        "v": "0.2",
        "t": "room.create",
        "r": "ROOM_SIG",
        "ts": 1710000000100,
        "c": {
          "ch": -1,
          "name": "Secret Project",
          "description": "Planning for launch",
          "rules": [
            {"t": "post", "allow": "*"},
            {"t": "invite_only", "inviters": ["CAROL_SIG"]},
            {"t": "rate_limit", "max": 10, "per": 60000},
            {"t": "retention", "days": 90},
            {"t": "uncommitted_ttl", "ms": 86400000},
            {"t": "replication", "mode": "members"}
          ]
        }
      },
      "p": "CAROL_SIG_ROOM_CREATE_EVENT"
    }
  },
  "p": "ROOM_SIG_COMMIT_ROOM_CREATE"
}
```

Commit signature:

```
p = Ed25519.sign(room_priv, canonical(d))
```

## B.5. Joining a Room

### B.5.1. Invitation Flow

**Step 1: Carol emits invite event**

Uncommitted (depth 1):

```json
{
  "d": {
    "n": "CAROL_SIG",
    "v": "0.2",
    "t": "room.invite",
    "r": "ROOM_SIG",
    "ts": 1710000100000,
    "c": {"ch": -2, "invitee": "ALICE_SIG"}
  },
  "p": "CAROL_SIG_INVITE"
}
```

Committed (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000100500,
    "tc": 1710000100500,
    "env": {
      "d": {
        "n": "CAROL_SIG",
        "v": "0.2",
        "t": "room.invite",
        "r": "ROOM_SIG",
        "ts": 1710000100000,
        "c": {"ch": -2, "invitee": "ALICE_SIG"}
      },
      "p": "CAROL_SIG_INVITE"
    }
  },
  "p": "ROOM_SIG_COMMIT_INVITE"
}
```

**Step 2: Alice submits join request**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.join",
    "ts": 1710000110000,
    "c": {
      "r": "ROOM_SIG",
      "nonce": 48151623
    }
  },
  "p": "ALICE_SIG_JOIN_REQ"
}
```

**Step 3: Key holder creates room.join event**

Uncommitted (depth 1):

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.join",
    "r": "ROOM_SIG",
    "ts": 1710000110200,
    "nonce": 48151623
  },
  "p": "ALICE_SIG_JOIN_EVENT"
}
```

Committed (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000110600,
    "tc": 1710000110600,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "room.join",
        "r": "ROOM_SIG",
        "ts": 1710000110200,
        "nonce": 48151623
      },
      "p": "ALICE_SIG_JOIN_EVENT"
    }
  },
  "p": "ROOM_SIG_COMMIT_JOIN"
}
```

## B.6. Sending Messages

### B.6.1. Uncommitted Then Committed Message Flow

**Step 1: Alice creates and signs room.message locally (depth 1)**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.message",
    "r": "ROOM_SIG",
    "ts": 1710000200000,
    "c": {"ch": 1, "body": "Hello team"}
  },
  "p": "ALICE_SIG_ROOM_MESSAGE_EVENT"
}
```

**Step 2: Alice submits envelope to peers via pool.exchange**

The depth-1 envelope is included in the `offer` of a `pool.exchange` request.
Peers validate the envelope (signature, room rules) and store it in their pool.
UI may show the message as pending while no depth-2 commit exists.

**Step 3: Key holder commits via depth-2 envelope**

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000200800,
    "tc": 1710000200800,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "room.message",
        "r": "ROOM_SIG",
        "ts": 1710000200100,
        "c": {"ch": 1, "body": "Hello team"}
      },
      "p": "ALICE_SIG_ROOM_MESSAGE_EVENT"
    }
  },
  "p": "ROOM_SIG_COMMIT_MSG_1"
}
```

Resulting eid:

```
eid = "0:1710000200100:ALICE_SIG"
```

## B.7. Pool Exchange

### B.7.1. Bilateral Pool Sync

**Step 1: Alice requests exchange**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "pool.exchange",
    "ts": 1710000300000,
    "c": {
      "offer": [
        {"datum_id": "H_A1", "t": "room.message", "r": "ROOM_SIG", "ts": 1710000200100},
        {"datum_id": "H_A2", "t": "side.attestation", "ts": 1710000250000}
      ],
      "want": {
        "rooms": ["ROOM_SIG"],
        "types": ["room.message", "room.join", "room.leave"],
        "after": 1710000000000,
        "limit": 100
      }
    }
  },
  "p": "ALICE_SIG_POOL_EXCHANGE_REQ"
}
```

**Step 2: Bob streams matching datums**

Committed message from Bob (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000290500,
    "tc": 1710000290500,
    "env": {
      "d": {
        "n": "BOB_SIG",
        "v": "0.2",
        "t": "room.message",
        "r": "ROOM_SIG",
        "ts": 1710000290000,
        "c": {"ch": 1, "body": "I have the docs"}
      },
      "p": "BOB_SIG_ROOM_MESSAGE_EVENT"
    }
  },
  "p": "ROOM_SIG_COMMIT_MSG_2"
}
```

Committed join from Carol (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000050100,
    "tc": 1710000050100,
    "env": {
      "d": {
        "n": "CAROL_SIG",
        "v": "0.2",
        "t": "room.join",
        "r": "ROOM_SIG",
        "ts": 1710000050000
      },
      "p": "CAROL_SIG_JOIN_EVENT"
    }
  },
  "p": "ROOM_SIG_COMMIT_JOIN_CAROL"
}
```

**Step 3: Alice verifies each datum independently**

```
For depth-1 envelope:
  Ed25519.verify(d.n, canonical(d), p)

For depth-2 (committed) envelope:
  // Verify outer (room commit)
  Ed25519.verify(d.n, canonical(d), p)
  // Verify inner (author event)
  Ed25519.verify(d.env.d.n, canonical(d.env.d), d.env.p)
```

## B.8. Bisection Sync

### B.8.1. Finding Divergence

**Step 1: Alice sends compare request**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.compare",
    "ts": 1710000400000,
    "c": {
      "r": "ROOM_SIG",
      "channel": 0,
      "samples": [
        {"seq": 1, "eid": "0:1710000001000:CAROL_SIG"},
        {"seq": 50, "eid": "0:1710000200100:ALICE_SIG"},
        {"seq": 100, "eid": "0:1710000350000:BOB_SIG"}
      ]
    }
  },
  "p": "ALICE_SIG_COMPARE_REQ"
}
```

**Step 2: Bob responds with signed result datum**

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "room.compare.result",
    "ts": 1710000400200,
    "c": {
      "r": "ROOM_SIG",
      "channel": 0,
      "matches": [1, 50],
      "mismatches": [100]
    }
  },
  "p": "BOB_SIG_COMPARE_RESULT"
}
```

**Step 3: Alice requests missing range**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.sync",
    "ts": 1710000400300,
    "c": {
      "r": "ROOM_SIG",
      "channel": 0,
      "start_seq": 51,
      "end_seq": 100,
      "limit": 100
    }
  },
  "p": "ALICE_SIG_ROOM_SYNC_REQ"
}
```

Bob streams signed datums for the requested range.

## B.9. Partition Recovery

### B.9.1. Network Split and Merge

**During split**

```
Partition A (Alice, Relay-1): continues writing/committing events
Partition B (Bob, Relay-2):   continues writing/committing events
```

Partition A committed event (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000500300,
    "tc": 1710000500300,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "room.message",
        "r": "ROOM_SIG",
        "ts": 1710000500000,
        "c": {"ch": 1, "body": "From partition A"}
      },
      "p": "ALICE_SIG_A_MSG"
    }
  },
  "p": "ROOM_SIG_A_COMMIT"
}
```

Partition B committed event (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000500400,
    "tc": 1710000500400,
    "env": {
      "d": {
        "n": "BOB_SIG",
        "v": "0.2",
        "t": "room.message",
        "r": "ROOM_SIG",
        "ts": 1710000500100,
        "c": {"ch": 1, "body": "From partition B"}
      },
      "p": "BOB_SIG_B_MSG"
    }
  },
  "p": "ROOM_SIG_B_COMMIT"
}
```

**After heal**

Nodes run `pool.exchange`/`events.sync` and merge by:
1. `tc` (from the outer commit envelope's `d.tc`)
2. `d.env.d.ts` (inner author timestamp)
3. `eid`

No special reconciliation datum is required.

## B.10. Forking

### B.10.1. Fork Announcement

**Step 1: New room is created**

```
OLD_ROOM_SIG -> NEW_ROOM_SIG
```

**Step 2: Fork announcement side-event**

Side-events are depth 1 and never committed:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "side.fork",
    "ts": 1710000600000,
    "c": {
      "original": "OLD_ROOM_SIG",
      "forked": "NEW_ROOM_SIG",
      "reason": "Governance split"
    }
  },
  "p": "ALICE_SIG_SIDE_FORK"
}
```

**Step 3: Migrate membership in new room**

Example committed `room.invite` in new room (depth 2):

```json
{
  "d": {
    "n": "NEW_ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000601300,
    "tc": 1710000601300,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "room.invite",
        "r": "NEW_ROOM_SIG",
        "ts": 1710000601000,
        "c": {"ch": -2, "invitee": "BOB_SIG"}
      },
      "p": "ALICE_SIG_NEW_INVITE"
    }
  },
  "p": "NEW_ROOM_COMMIT_INVITE"
}
```

## B.11. Attestation

### B.11.1. Positive Attestation Flow

Alice attests Bob seeded data correctly:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "side.attestation",
    "ts": 1710000700000,
    "c": {
      "subject": "BOB_SIG",
      "claim": "seeded",
      "detail": {
        "room": "ROOM_SIG",
        "bytes": 52428800,
        "period_days": 30
      }
    }
  },
  "p": "ALICE_SIG_ATTEST_SEEDED"
}
```

### B.11.2. Negative Attestation Flow

Alice receives invalid signature from Bob and records failure:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "side.attestation",
    "ts": 1710000705000,
    "c": {
      "subject": "BOB_SIG",
      "claim": "failed_proof",
      "detail": {
        "datum_id": "BAD_HASH",
        "reason": "invalid commit signature"
      }
    }
  },
  "p": "ALICE_SIG_ATTEST_FAILED_PROOF"
}
```

## B.12. Proof-of-Work

### B.12.1. PoW for Join

Room requires:

```json
{"t":"join_pow","difficulty":20}
```

Alice computes nonce for `room.join`:

```
nonce = 0
repeat:
  d.nonce = nonce
  h = SHA-256(canonical(d))
  if leading_zero_bits(h) >= 20: break
  nonce += 1
```

Example join event after solving (depth 1, uncommitted):

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.join",
    "r": "ROOM_SIG",
    "ts": 1710000800000,
    "nonce": 1048573
  },
  "p": "ALICE_SIG_JOIN_WITH_POW"
}
```

Committed version (depth 2):

```json
{
  "d": {
    "n": "ROOM_SIG",
    "v": "0.2",
    "t": "commit",
    "ts": 1710000800500,
    "tc": 1710000800500,
    "env": {
      "d": {
        "n": "ALICE_SIG",
        "v": "0.2",
        "t": "room.join",
        "r": "ROOM_SIG",
        "ts": 1710000800000,
        "nonce": 1048573
      },
      "p": "ALICE_SIG_JOIN_WITH_POW"
    }
  },
  "p": "ROOM_SIG_COMMIT_JOIN_WITH_POW"
}
```

## B.13. Online-Only Room

### B.13.1. uncommitted_ttl = 0

Room rule:

```json
{"t":"uncommitted_ttl","ms":0}
```

Alice creates a `room.message` locally and submits via `pool.exchange`:

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.message",
    "r": "ROOM_SIG",
    "ts": 1710000900000,
    "c": {"ch": 1, "body": "Anyone online?"}
  },
  "p": "ALICE_SIG_MESSAGE_EVENT"
}
```

Peer rejects the envelope because `uncommitted_ttl = 0` and no key holder
is reachable to commit immediately:

```json
{
  "error": "permission_denied",
  "message": "online-only room requires reachable key holder"
}
```

No uncommitted `room.message` envelope is stored in any peer's pool.

## B.14. Membership Reconstruction

### B.14.1. Rebuilding Membership State

Given committed channel -2 events, reconstruct membership by replaying all
events in `tc` order:

```
members = Set(), banned = Set()

Event 1 (tc=1710000050100): room.join by CAROL_SIG
  → members = {CAROL_SIG}

Event 2 (tc=1710000100500): room.invite by CAROL_SIG (invitee: ALICE_SIG)
  → no state change (invite is prerequisite only)

Event 3 (tc=1710000110600): room.join by ALICE_SIG
  → members = {CAROL_SIG, ALICE_SIG}

Event 4 (tc=1710000200000): room.join by BOB_SIG
  → members = {CAROL_SIG, ALICE_SIG, BOB_SIG}

Event 5 (tc=1710000300000): room.ban by CAROL_SIG (banned: SPAMMER_SIG)
  → banned = {SPAMMER_SIG}

Event 6 (tc=1710000400000): room.leave by BOB_SIG
  → members = {CAROL_SIG, ALICE_SIG}

Final state:
  members = {CAROL_SIG, ALICE_SIG}
  banned  = {SPAMMER_SIG}
```

Membership proof: ALICE_SIG is a member because she has a committed
`room.join` (event 3) with no subsequent `room.ban` or `room.leave`.

## B.15. Private Message Flow

### B.15.1. Encrypted Private Message

**Step 1: Alice encrypts and sends private.message to Bob**

Alice looks up Bob's `enc` key from his `node.meta`. She derives the
pairwise key (cacheable per peer) and encrypts with a fresh random salt:

```
pair_key = HKDF-SHA256(X25519(ALICE_ENC_PRIV, BOB_ENC_PUB),
                       salt="toloo-private-pair-v2", info="pair", L=32)
salt = random_32_bytes()
salt_hash = SHA-256(ALICE_ENC_PUB || BOB_ENC_PUB || salt)
okm = HKDF-SHA256(pair_key, salt_hash, "toloo-private-v2", 44)
key = okm[0:32], nonce = okm[32:44]
plaintext = canonical({"body": "Hey Bob, meeting at 3pm?"})
ciphertext = ChaCha20-Poly1305.encrypt(key, nonce, plaintext, aad="")
```

Depth-1 envelope:

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

**Step 2: Bob receives and decrypts**

```
pair_key = HKDF-SHA256(X25519(BOB_ENC_PRIV, ALICE_ENC_PUB),
                       salt="toloo-private-pair-v2", info="pair", L=32)
salt_hash = SHA-256(ALICE_ENC_PUB || BOB_ENC_PUB || salt)
okm = HKDF-SHA256(pair_key, salt_hash, "toloo-private-v2", 44)
key = okm[0:32], nonce = okm[32:44]
plaintext = ChaCha20-Poly1305.decrypt(key, nonce, ciphertext, aad="")
// plaintext = {"body": "Hey Bob, meeting at 3pm?"}
```

**Step 3: Bob acknowledges (depth 2)**

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "ack",
    "ts": 1710000001000,
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

**Step 4: Alice confirms (depth 3, optional)**

Alice wraps Bob's depth-2 ack in a depth-3 envelope, confirming delivery.

**Metadata visibility**: `d.n`, `d.to`, `d.ts` are visible in the pool.
Content (`c.encrypted`) is encrypted. Only Alice and Bob can read the
message body.

## B.16. Blob Upload and Fetch

### B.16.1. Uploading a Blob to a Room

**Step 1: Alice splits the file into pieces and computes hashes**

Alice has a 2MB JPEG (2,097,152 bytes). With default piece size of
262,144 bytes (256 KB), this produces 8 pieces:

```
piece_0: bytes[0..262143]       → SHA-256 = "aaa111..."
piece_1: bytes[262144..524287]  → SHA-256 = "bbb222..."
piece_2: bytes[524288..786431]  → SHA-256 = "ccc333..."
...
piece_7: bytes[1835008..2097151] → SHA-256 = "hhh888..."

blob_hash = SHA-256(piece_0 || piece_1 || ... || piece_7) = "a1b2c3...a1b2"
```

**Step 2: Alice sends each piece as a binary frame**

Each piece is sent as a binary frame (type `0x01`) prefixed with that
piece's own 32-byte SHA-256 hash:

```
→ binary frame: 0x01 || aaa111...(32 bytes) || piece_0_data(262144 bytes)
→ binary frame: 0x01 || bbb222...(32 bytes) || piece_1_data(262144 bytes)
...
→ binary frame: 0x01 || hhh888...(32 bytes) || piece_7_data(262144 bytes)
```

The peer verifies each piece independently:
`SHA-256(piece_data) == piece_hash`.

**Step 3: Alice creates room.blob metadata event**

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
      "pieces": [
        "aaa111...", "bbb222...", "ccc333...", "ddd444...",
        "eee555...", "fff666...", "ggg777...", "hhh888..."
      ],
      "mime": "image/jpeg",
      "name": "photo.jpg"
    }
  },
  "p": "ALICE_SIG_BLOB_META"
}
```

**Step 4: Alice sends a message referencing the blob**

```json
{
  "d": {
    "n": "ALICE_SIG",
    "v": "0.2",
    "t": "room.message",
    "r": "ROOM_SIG",
    "ts": 1710001200100,
    "c": {
      "ch": 1,
      "body": "Check out this photo!",
      "blobs": ["0:1710001200000:ALICE_SIG"]
    }
  },
  "p": "ALICE_SIG_MSG_WITH_BLOB"
}
```

### B.16.2. Fetching a Blob (Full)

**Step 1: Bob requests all pieces by blob hash**

```json
{
  "d": {
    "n": "BOB_SIG",
    "v": "0.2",
    "t": "blob.fetch",
    "ts": 1710001300000,
    "c": {
      "hash": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
    }
  },
  "p": "BOB_SIG_BLOB_FETCH"
}
```

**Step 2: Node responds with binary frames (one per piece)**

```
← binary frame: 0x01 || aaa111...(32 bytes) || piece_0_data
← binary frame: 0x01 || bbb222...(32 bytes) || piece_1_data
...
← binary frame: 0x01 || hhh888...(32 bytes) || piece_7_data
```

A partial seeder returns only the pieces it has. Bob checks which
pieces are missing by comparing received piece hashes against the
`pieces[]` array from the `room.blob` envelope.

**Step 3: Bob verifies each piece**

```
for i, piece in enumerate(received_pieces):
  assert SHA-256(piece.data) == room_blob.c.pieces[i]

// Final verification: concatenate all pieces in order
full_blob = piece_0 || piece_1 || ... || piece_7
assert SHA-256(full_blob) == room_blob.c.hash
```

### B.16.3. Resumable Fetch (Piece-Level)

Bob already has pieces 0-4 from a previous interrupted download. He
requests missing pieces individually by piece hash:

```json
// Request piece 5
{
  "d": {
    "n": "BOB_SIG", "v": "0.2", "t": "blob.fetch", "ts": 1710001400000,
    "c": {"hash": "fff666..."}
  },
  "p": "BOB_SIG_FETCH_P5"
}
```

```
← binary frame: 0x01 || fff666...(32 bytes) || piece_5_data
```

Bob repeats for pieces 6 and 7 (from the same or different nodes).
Once all pieces are collected, final hash verification passes.

## B.17. References

- [2] Data Model (envelope format `{d, p}`, depth-2 commit envelopes, canonical JSON)
- [6] Events (event types and commit semantics)
- [7] Replication (pool exchange and sync)
- [9] Transport (carrier model, message framing)
- [10] Endpoints (signed request datums and response model)
