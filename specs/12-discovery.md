# 12. Discovery

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [4] Identity, [7] Replication

## 12.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 12.2. Discovery Methods

Nodes discover peers and rooms through multiple mechanisms:

| Method | Use Case | Trust Level |
|--------|----------|-------------|
| Pre-bundled seeds | Cold start (first launch) | High (bundled with client) |
| `toloo://` URIs | Out-of-band sharing (QR, email, links) | Medium (verify via TOFU) |
| `.toloo` files | Config files, sneakernet, pool export | Medium (verify signatures) |
| Pool gossip | Organic discovery | Medium (verified signatures) |
| Room metadata | Via hints in rooms | Medium (room authority) |

No single method is mandatory. Implementations MAY support any combination.

All discovery methods use the same data: signed envelopes (`{d, p}`).
A `node.meta` envelope IS a peer connection config. A committed
`room.create` envelope IS a room invite. No separate config format exists.
See [4#toloo-uri-scheme] for URI format and `.toloo` file format.

## 12.3. Pre-Bundled Seed Nodes

Clients SHOULD include addresses of 10-15 public nodes at build time.

### 12.3.1. Seed Node Selection Criteria

Seed nodes SHOULD:
- Run on dedicated servers with 99%+ uptime
- Have static IP addresses or stable domain names
- Be operated by known, accountable entities
- Be geographically distributed across multiple jurisdictions
- Support IPv4 and IPv6
- Run in archival mode (full history retention)

### 12.3.2. Seed List Format

Seed nodes are distributed as `.toloo` files containing signed `node.meta`
envelopes (see [4#toloo-data-files]). Clients SHOULD ship a seed file at
build time:

```
# Toloo seed nodes — updated 2026-02-01
# Organization A (Germany) — seed1.toloo.example
eyJkIjp7Im4iOiJTRUVEMV9TSUciLCJ2IjoiMC4yIiwidCI6Im5vZGUubWV0YSIsInRzIjoxNzEw...
# Organization B (Netherlands) — 198.51.100.9:9090
eyJkIjp7Im4iOiJTRUVEMl9TSUciLCJ2IjoiMC4yIiwidCI6Im5vZGUubWV0YSIsInRzIjoxNzEw...
```

Each line decodes to a signed `node.meta` envelope containing `via[]`
endpoint descriptors. Because envelopes are signed, seeds are
self-authenticating — a tampered seed file is detected during signature
verification.

Clients MAY also hardcode seeds directly as JSON arrays of `node.meta`
envelopes (implementation choice).

### 12.3.3. First Startup Flow

On first startup with bundled seeds:

```
1. Client loads seed .toloo file → list of node.meta envelopes
2. Client verifies each envelope signature
3. Client selects 3-5 seeds randomly
4. Client connects to each seed's via[] endpoint
5. For each successful connection:
   a. Run pool.exchange
   b. Receive node.meta envelopes for other peers (backup connections)
   c. Receive room.create envelopes for discoverable rooms
   d. Receive recent events
6. Client stores all received envelopes in local pool
7. Client is now bootstrapped — can reach multiple peers
```

On first startup with a `toloo://` URI or `.toloo` file from a friend:

```
1. Client decodes envelope(s) from URI or file
2. Client verifies each envelope signature
3. Client finds node.meta envelope(s) → extracts via[] endpoints
4. Client connects to peer
5. Run pool.exchange → receive peer configs, rooms, events
6. Client is bootstrapped with the friend's view of the network
```

### 12.3.4. Seed Rotation

Clients SHOULD NOT rely solely on bundled seeds. After discovering other
peers through gossip, clients SHOULD:
- Connect to geographically closer nodes
- Prioritize higher-trust nodes (via attestations)
- Update `via[]` in metadata to reflect current relays

## 12.4. toloo:// URI and .toloo File Discovery

`toloo://` URIs and `.toloo` files both encode signed envelopes for sharing
(see [4#toloo-uri-scheme] and [4#toloo-data-files]).

### 12.4.1. Sharing Methods

URIs and `.toloo` files may be shared via:
- QR codes (scan → `toloo://` URI)
- Messaging apps (Telegram, Signal, email — URI or file attachment)
- Websites (embedded links or file downloads)
- USB drives / sneakernet (`.toloo` files)
- Clipboard (copy-paste URI)
- NFC / Bluetooth (direct transfer)

### 12.4.2. URI Trust

When receiving a `toloo://` URI or `.toloo` file:

```
1. Decode envelope(s) from URI or file
2. Verify each envelope signature (p over canonical(d) using d.n)
3. Connect to via[] endpoint from a node.meta envelope
4. Run pool.exchange to verify the peer is live
5. Pin peer identity via TOFU
```

Envelopes received over insecure channels (HTTP, email) are signed but
the channel itself may be monitored. TOFU prevents long-term impersonation
but not initial contact interception.

### 12.4.3. Room Invitation

A room invite URI contains a committed `room.create` envelope plus
`node.meta` envelopes for peers that carry the room:

```
toloo://<ROOM_CREATE_COMMIT>.<PEER_NODE_META>
```

Clients SHOULD display room info (name, member count) and prompt the user
to join. After connecting to the peer, the client syncs room data via
`pool.exchange`.

## 12.5. Pool Gossip Discovery

Nodes discover peers organically through pool exchange.

### 12.5.1. Gossip Protocol

```
Alice                              Bob
  |                                 |
  | --- pool.exchange (want peers)->|
  |                                 |
  | <-- node.meta envelopes ------- |
  |                                 |
```

Alice requests peer metadata. Bob responds with `node.meta` envelopes for
nodes he knows. Each envelope is signed by the node it describes, so Alice
can verify them independently.

**Backup peer directory**: Every `pool.exchange` response SHOULD include
`node.meta` envelopes for other known reachable peers (see
[10#pool.exchange]). This means a node's pool naturally accumulates
connection configs for many peers. If a node's current peer goes down,
it can try any cached `node.meta` endpoint — no special mechanism needed.

### 12.5.2. Peer Prioritization

Nodes SHOULD prioritize gossip about:
- Nodes with reachable enabled `via[]` endpoints
- High-trust nodes (positive attestations)
- Recently active nodes (recent `ts` in metadata)
- Geographically diverse nodes

Nodes SHOULD NOT gossip about:
- Blacklisted nodes
- Nodes with expired metadata (ts > 90 days old with no recent activity)
- Nodes the peer explicitly requested not to be gossiped

### 12.5.4. Room Discovery via Pool Exchange

Rooms are discovered organically through pool exchange. When a peer offers
datums referencing rooms the requester does not know about, the responder
MAY include `room.create` or latest `room.update` envelopes for those
rooms in the response (see [10#pool.exchange]).

This enables passive room discovery without a centralized directory:

```
Alice                              Bob
  |                                 |
  | --- pool.exchange (want all) -->|
  |                                 |
  | <-- datums (incl. room.create  |
  |     for rooms Alice doesn't    |
  |     know about) -------------- |
  |                                 |
```

Alice sees room metadata for rooms she had not previously encountered.
She can then decide to join, ignore, or block them based on local policy.

Nodes SHOULD NOT flood pool exchanges with room metadata for rooms the
requester is unlikely to care about. Heuristics for selecting which rooms
to advertise are implementation-defined but MAY include:
- Rooms with recent activity
- Rooms with `replication: open` policy
- Rooms that share members with the requester's known rooms

### 12.5.3. Gossip Limits

To prevent gossip floods, nodes SHOULD:
- Limit `node.peers` responses to 20-50 peers
- Limit gossiped metadata age to 90 days
- Rate-limit gossip requests (max 1 request per peer per 5 minutes)

## 12.6. Room Metadata Discovery

Rooms include endpoint relay descriptors (`via[]`) in metadata (channel -1).

### 12.6.1. Via Hints

Room metadata MAY include:

```json
{
  "name": "My Room",
  "via": [
    {
      "proto": "https",
      "host": "relay1.example",
      "port": 443,
      "path": "/toloo",
      "direct": true
    },
    {
      "proto": "ws",
      "host": "relay2.example",
      "port": 8080,
      "path": "/toloo"
    }
  ],
  "rules": [...]
}
```

The `via[]` array lists endpoints that:
- Store this room's data
- Can forward uncommitted events to key holders
- Are reachable by new members

### 12.6.2. Mobile Summary

Mobile and bandwidth-constrained clients can use `room.summary` (see
[10#room.summary]) to check room status without full sync. This is useful
for:

- **Wake-up sync**: On app launch, request summaries for all joined rooms
  to display unread counts and latest activity before fetching full events.
- **Background refresh**: Periodically poll summaries to update badges
  without downloading event payloads.
- **Room preview**: Before joining a room, request its summary to see
  member count and activity level.

Typical flow:

```
1. Client wakes up with 15 joined rooms
2. Client sends room.summary with all 15 room keys
3. Relay responds with 15 room.summary.result envelopes
4. Client compares latest_tc with local state to compute unread counts
5. Client does full room.sync only for rooms with new activity
```

This reduces bandwidth by orders of magnitude compared to syncing all
rooms on every wake-up.

### 12.6.3. Room Discovery via Search

No protocol-level room search exists. Rooms are private by default.

Third-party services MAY index public rooms (rooms with `replication: open`)
and provide search, but this is outside the protocol scope.

## 12.7. Optional Metadata Mirrors

Implementations MAY expose unsigned HTTP metadata mirrors for convenience.
These mirrors are non-normative and SHOULD NOT be required for discovery.
Deployments that prioritize scanner resistance MAY disable public mirrors.

### 12.7.1. CORS Headers

Public nodes SHOULD include CORS headers to enable browser-based discovery:

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET
Access-Control-Allow-Headers: Content-Type
```

### 12.7.2. Caching

Responses SHOULD include cache headers:

```
Cache-Control: public, max-age=3600
```

Metadata typically does not change frequently; caching reduces load.

## 12.8. Local Network Discovery

Nodes MAY use local transport hints for peer discovery (non-normative):

### 12.8.1. Bluetooth LE

Nodes MAY advertise via BLE beacons containing:
- Service UUID (application-specific)
- Node signature (truncated to fit BLE packet size)

Follow-up communication occurs over the selected endpoint descriptor and
transport profile after BLE discovery.

These methods are implementation-specific and not part of the core protocol.

## 12.9. Discovery Privacy

### 12.9.1. Passive Discovery Leaks

Pool gossip reveals social graphs:
- Who knows whom
- Who is in which rooms
- Network topology

Nodes in high-security scenarios SHOULD limit gossip participation.

### 12.9.2. Active Discovery Leaks

Connecting to seeds and requesting pool exchanges reveal:
- Node's IP address
- Active time patterns
- Rooms of interest

Nodes requiring anonymity SHOULD use Tor or similar transport-level anonymity.

### 12.9.3. Selective Disclosure

Nodes MAY limit gossip about themselves:

```json
{
  "d": {
    "n": "NODE_SIG",
    "v": "0.2",
    "t": "node.meta",
    "ts": 1710000000000,
    "meta": {
      "public": false
    }
  },
  "p": "..."
}
```

This is advisory; peers can ignore it.

## 12.10. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
