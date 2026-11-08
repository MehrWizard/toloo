# 1. Concepts and Architecture

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md)

## 1.1. Introduction

Toloo is a messaging protocol where room-level policy controls the degree
of decentralization. It defines a self-authenticating data model, a
cryptographic commit flow, and a pool-based replication system that
supports deployments ranging from fully decentralized peer-to-peer
networks to centralized single-server products.

Every participant runs the same software. There is no client/server split
at the protocol level. Nodes that can accept incoming connections serve
as relays for nodes that cannot, but all nodes use the same data format,
cryptography, and verification logic.

## 1.2. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC 2119] [RFC 8174] when, and only when, they appear in ALL CAPITALS,
as shown here.

## 1.3. Notation

| Convention | Meaning |
|------------|---------|
| base64url | Base64 encoding with URL-safe alphabet, no padding (RFC 4648 section 5) |
| uint64 | Unsigned 64-bit integer |
| timestamp | Unix time in milliseconds (uint64) |
| canonical JSON | Deterministic JSON serialization defined in [2] |
| `ROOM_SIG`, `NODE_SIG` | Placeholder for an Ed25519 public key (base64url) |

## 1.4. Architecture

Toloo is a protocol, not a topology. The same data model and commit flow
support multiple deployment modes:

- **Decentralized.** Every member stores and seeds room data. Messages
  flow through pools across public nodes. No single point of failure.

- **Private group.** Only members replicate data among themselves. Data
  does not leave the group. Members still benefit from P2P seeding.

- **Centralized.** A designated server controls data distribution.
  Members connect only to that server. Comparable to a traditional
  messaging service, but using Toloo's data format and cryptography.

The degree of decentralization is a **room-level policy choice**, not a
protocol constraint. A single network can contain rooms of all types.

All modes share the same foundation:
- Self-authenticating data (cryptographic signatures on every datum).
- Two-phase commit flow (author signs, room countersigns).
- Portable data (all datums are verifiable by any node, independent of
  where they were received).

```
Decentralized room:             Centralized room:

  Node A ──── Node B              Server
  │    ╲    ╱    │               ╱  │  ╲
  │     Node C   │           User  User  User
  │    ╱    ╲    │
  Node D ──── Node E
  (all seed, all verify)     (server serves, users verify)
```

For decentralized and private-group rooms, public nodes (nodes with
reachable addresses) enable NAT traversal and offline message delivery.
NAT'd nodes connect outbound to public nodes, and messages flow through
the pool network. Public nodes are regular nodes that happen to have
reachable addresses — they run the same software and follow the same rules.

For centralized rooms, the designated server is the sole data source.
If it goes offline, the room is unavailable. This is the trade-off the
room creator accepts in exchange for full control over data distribution.

## 1.5. Design Goals

- Operate in censored and filtered network environments.
- Work over unreliable, high-latency, low-bandwidth connections.
- Survive takedowns: data is portable, identities are self-sovereign, keys are mobile.
- Support one-directional transports (plain HTTP request-response).
- Trust no single node: all data is self-authenticating.
- Survive network partitions: forking is a first-class operation.
- Reward good actors naturally: reputation accrues through useful behavior.

## 1.6. Non-Goals

- Voice/video streaming.
- Group end-to-end encryption.
- Real-time presence or typing indicators.
- Cryptographic suite negotiation.
- Defining UI/UX behavior or client implementation details.
- Mandating storage engines or deployment topology.
- Global consensus or blockchain.

## 1.7. Core Concepts

### 1.7.1. Node

A node is an actor in the network, identified by two permanent keypairs:
an Ed25519 signing keypair (`sig`) and an X25519 encryption keypair
(`enc`). The signing public key is the node's identity forever — it
cannot be rotated or transferred. The encryption public key enables any
node to receive private messages. Nodes connect to peers, store room
data locally, seed data to other nodes, and build reputation through
useful behavior.

Both keypairs are generated together when a node is created. The Ed25519
key is used for signing all datums. The X25519 key is used for private
message encryption. Endpoint-level transport encryption uses separate
per-endpoint keys (`via[].key`), not the node's `enc` key.

Defined in: [4] Identity.

### 1.7.2. Public Node

A node with at least one publicly reachable endpoint descriptor in
`via[]`. Public
nodes can accept incoming connections and therefore serve as connectivity
bridges for NAT'd nodes. They are regular nodes — same software, same
rules — that happen to be reachable.

Public nodes often run in archival mode (storing full room history) and
serve as seed nodes for new users joining the network.

Defined in: [4] Identity, [12] Discovery.

### 1.7.3. NAT'd Node

A node behind a firewall or NAT that cannot accept incoming connections.
It maintains outbound connections to one or more public nodes listed in
its `via[]` endpoints. Messages reach it through the pool network via those
public nodes.

Most end-user devices (phones, laptops, browsers) are NAT'd nodes.

Defined in: [4] Identity.

### 1.7.4. Peer

A peer is any node you exchange signed datums with over a shared endpoint.
"Peer" is a relationship, not a distinct identity type. Two nodes become
peers when one or both exchange valid signed protocol datums.

Defined in: [9] Transport.

### 1.7.5. Room

A room is an event log identified by an Ed25519 keypair. The public key
is the room's permanent identity. The private key is the room's authority:
whoever holds it can commit events and change room state.

A room is **data, not a service**. It has no network presence of its own.
It exists because nodes choose to store and seed its events. The room key
can live on any device — a dedicated server, a phone, multiple devices
simultaneously.

Rooms contain channels, have rules, and support forking as a governance
mechanism. A room's replication policy controls whether data is seeded
openly, restricted to members, or served only by designated nodes (see
[5#rules]).

Defined in: [5] Rooms.

### 1.7.6. Channel

A channel is a numbered sub-log inside a room. Channels separate different
types of events with independent retention and sync policies.

Negative channels are reserved for room infrastructure:
- Channel -1: room metadata (name, rules, config).
- Channel -2: membership (joins, leaves, bans, invites).

Channel 0 and above are for user content (messages, reactions, etc.).
Each channel syncs independently — a node can subscribe to channel 0
without downloading channel 3.

Defined in: [5] Rooms.

### 1.7.7. Envelope

The envelope is the universal container for all verifiable data in Toloo.
Every piece of signed data — events, metadata, attestations, commit
records — is an envelope with exactly two fields:

- **d**: the data object (contains all signed content, the signer's
  public key `n`, and protocol version `v`).
- **p**: Ed25519 signature over the canonical serialization of `d`.

Envelopes nest recursively via `d.env`, which contains a complete inner
envelope `{d, p}`. Since `env` is inside `d`, the outer signature
cryptographically covers the entire inner chain. This provides both
structural and cryptographic tamper resistance.

The signer's identity is inside `d`, so tampering with any field
(including the signer) invalidates the signature.

Defined in: [2] Data Model.

### 1.7.8. Event

An event is a datum that records an action in a room channel. Events
represent messages, joins, leaves, bans, reactions, edits, state changes,
and other room activity.

Events are identified by an **eid** (event ID) computed from the channel
number, the author's timestamp, and the author's public key. The eid is
deterministic and never stored inside the datum.

Defined in: [6] Events.

### 1.7.9. Uncommitted Event

A depth-1 envelope signed by its author but not yet committed by the
room. The author's signature is valid, but the room has not wrapped it.

Whether uncommitted events are stored and displayed is controlled by the
room's `uncommitted_ttl` rule (see [5#rules]):
- If the room allows it, additive uncommitted events (messages, reactions)
  MAY be displayed with a "pending" indicator for up to the specified TTL.
- State-changing uncommitted events (joins, bans) are always held until
  committed, regardless of TTL.
- If the TTL is zero, the room operates in online-only mode: uncommitted
  events are rejected unless a key holder is immediately reachable.

Defined in: [6] Events, [5#rules] for `uncommitted_ttl`.

### 1.7.10. Committed Event

A committed event is a depth-2 envelope where the room key holder wraps
the author's depth-1 envelope. The room's `d` layer contains
`t: "commit"`, `tc` (commit timestamp), and `env` (the original author
envelope). Both the author's proof and the room's proof are
independently verifiable by any node.

Committing is Toloo's two-phase flow from "author claims" to "room
confirms."

Defined in: [6] Events.

### 1.7.11. Side-Event

A datum signed by its creator that is never committed by any room. Fork
announcements, attestations, and node metadata announcements are
side-events. They propagate through pools but exist outside any room's
event log.

Defined in: [6] Events.

### 1.7.12. Pool

The pool is a node's local store of all data it knows about: committed
events, uncommitted events, and side-events. All data enters and leaves
a node through its pool.

Nodes exchange pools with peers to replicate data across the network.
Committed events replace uncommitted events with matching eids. Invalid
data is dropped and may trigger negative attestations.

The pool is the single replication layer — there is no separate gossip
channel, message queue, or relay protocol.

Defined in: [7] Replication.

### 1.7.13. Key Holder

A node that possesses a room's private key. Key holders commit
uncommitted events by wrapping them in depth-2 commit envelopes.
Multiple nodes can hold the same room key simultaneously; their commit
streams merge deterministically via timestamp ordering.

When no key holder is reachable, behavior depends on the room's
`uncommitted_ttl` rule: rooms that allow it continue to accept
uncommitted messages (displayed as "pending"), while online-only rooms
become read-only until a key holder returns.

Defined in: [5] Rooms, [7] Replication.

### 1.7.14. Attestation

A signed statement one node makes about another. Attestations are
side-event datums that propagate through pools. They record observations
(e.g., "this node seeded data to me" or "this node provided invalid
proofs").

Attestations are receipts, not currency. They cannot be transferred or
traded. Each node evaluates attestations locally to build its own view of
peer reputation.

Defined in: [8] Trust.

### 1.7.15. Fork

A governance action where a member creates a new room from an existing
room's history. The new room has a new keypair and new authority.
Committed events from the original room retain their proofs in the fork.

Forking is the escape mechanism when a room key is lost, a key holder is
malicious, or a community splits. It is not used for temporary outages
or network partitions — the pool handles those.

Defined in: [14] Lifecycle.

### 1.7.16. Private Message

A 1:1 encrypted message between two nodes. Private messages use `d.to`
(not `d.r`) and are end-to-end encrypted using static-static X25519
(payload v2): a pairwise key derived from both nodes' long-lived `enc`
keys plus a random per-message salt. This is stateless and asynchronous
but not forward secret. The sender's `d.n` and recipient's `d.to` are
visible in the pool, but message content is encrypted.

Private messages follow the same depth lifecycle as room events: depth 1
(sender creates), depth 2 (recipient acknowledges), depth 3 (sender
confirms). Signer alternation is `d.n` → `d.to` → `d.n`.

Every node publishes its `enc` (X25519 public key) in `node.meta`,
so all nodes can receive private messages.

Defined in: [3] Cryptography, [6] Events.

## 1.8. Protocol Layers

Toloo is organized in layers. Each layer depends on the ones below it but
not above it.

```
┌──────────────────────────────────────────────┐
│  Discovery & Moderation  [12] [13]           │
├──────────────────────────────────────────────┤
│  Endpoints               [10]                │
├──────────────────────────────────────────────┤
│  Transport               [9]                 │
├──────────────────────────────────────────────┤
│  Trust & Reputation      [8]                 │
├──────────────────────────────────────────────┤
│  Replication             [7]                 │
├──────────────────────────────────────────────┤
│  Events                  [6]                 │
├──────────────────────────────────────────────┤
│  Rooms & Identity        [4] [5]             │
├──────────────────────────────────────────────┤
│  Data Model & Crypto     [2] [3]             │
└──────────────────────────────────────────────┘
```

| Layer | Documents | Concern |
|-------|-----------|---------|
| Foundation | [2] Data Model, [3] Cryptography | How data is structured, signed, and hashed. |
| Entities | [4] Identity, [5] Rooms | What nodes and rooms are, how they are identified. |
| Events | [6] Events | What happens inside rooms, event lifecycle. |
| Replication | [7] Replication | How data moves between nodes (pool, sync, committing). |
| Trust | [8] Trust | How nodes evaluate each other. |
| Transport | [9] Transport | How endpoint carriers are used. |
| Endpoints | [10] Endpoints | The request/response API between nodes. |
| Operations | [11] Storage, [12] Discovery, [13] Moderation, [14] Lifecycle | Operational concerns: retention, bootstrap, spam, governance. |
