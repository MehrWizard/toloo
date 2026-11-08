# 8. Trust

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md), [1] Concepts, [6] Events

## 8.1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174].

## 8.2. Trust Model

Toloo uses cryptographic verification and local reputation tracking. There
is no global trust authority or reputation ledger.

Every datum is self-authenticating via Ed25519 signatures. Verification is
automatic and mandatory. Trust evaluation (deciding whether to interact with
a peer beyond signature verification) is local to each node.

## 8.3. Attestations

Attestations are signed statements one node makes about another. They are
side-events (see [6#side-attestation]) that propagate through pools.

### 8.3.1. Attestation Structure

```json
{
  "d": {
    "n": "ATTESTER_SIG",
    "v": "0.2",
    "t": "side.attestation",
    "ts": 1710000000000,
    "c": {
      "subject": "NODE_SIG",
      "claim": "seeded",
      "detail": {"bytes": 52428800, "period_days": 30}
    }
  },
  "p": "ATTESTER_SIGNATURE"
}
```

Fields in `c`:
- `subject`: Public key of the node being attested to (REQUIRED)
- `claim`: Claim type (REQUIRED, see §8.4)
- `detail`: Claim-specific structured data (OPTIONAL)

### 8.3.2. Attestation Semantics

Attestations are **observations**, not **endorsements**. An attestation
records a fact that the attester witnessed, not a judgment of character.

Example: `{"claim": "seeded"}` means "this node seeded data to me," not
"this node is trustworthy."

## 8.4. Attestation Claims

Claim types and their meanings:

### 8.4.1. Positive Claims

**seeded**
- Subject provided room data to attester
- Detail fields: `bytes` (total bytes), `rooms` (room count), `period_days` (observation period)

**committed**
- Subject successfully forwarded an uncommitted event to a key holder, resulting in a commit
- Detail fields: `room` (room signature), `events` (count of events committed)

**available**
- Subject was reachable and responsive
- Detail fields: `uptime_percent` (percentage), `latency_ms` (average), `period_days`

**state_confirmed**
- Subject's room state claim was verified as correct by the attester
- Detail fields: `room` (room signature), `channel` (channel number)

### 8.4.2. Negative Claims

**failed_proof**
- Subject provided data with invalid cryptographic proofs
- Detail fields: `datum_id` (hash of invalid datum), `reason` (description)

**false_data**
- Subject provided fabricated or tampered data
- Detail fields: `datum_id`, `reason`

**state_denied**
- Subject's room state claim was verified as false by the attester
- Detail fields: `room`, `channel`, `reason`

**timeout**
- Subject was unreachable or unresponsive
- Detail fields: `timeout_ms`, `attempts`

## 8.5. Attestation Propagation

Attestations propagate through pools like any other side-event:

```
1. Node A makes observation about Node B
2. Node A creates and signs attestation
3. Node A adds attestation to its pool
4. Node A includes attestation in pool.exchange with peers
5. Peers verify attestation signature (d.n = A's public key)
6. Peers store attestation in their pools
7. Attestation gradually spreads across the network
```

Attestations have no expiry at the protocol level. Nodes MAY prune old
attestations based on local policy.

## 8.6. Trust Evaluation

Each node evaluates peers independently using multiple signals:

### 8.6.1. Direct Experience

The strongest signal: the node's own interactions with a peer.

```
Node tracks per peer:
- Bytes seeded
- Successful connections
- Failed connections
- Invalid data received
- Response time
```

This is Sybil-resistant: an attacker cannot forge direct experience.

### 8.6.2. Attestation Count

Number of attestations received about a peer.

```
For peer P:
  positive_attestations = count(attestations with positive claims about P)
  negative_attestations = count(attestations with negative claims about P)
  score = positive_attestations - negative_attestations
```

This is partially Sybil-resistant: an attacker can create fake identities
to attest to each other, but these attestations only propagate within the
attacker's network.

### 8.6.3. Attester Quality

Attestations from trusted nodes carry more weight.

```
For attestation A about peer P:
  attester = A.d.n
  attester_trust = node's trust score for attester
  weighted_value = attester_trust * 1.0
```

This is Sybil-resistant: an attacker's self-attestations have zero weight
to nodes outside the attacker's network.

### 8.6.4. Age

Older identities with consistent behavior are more trustworthy.

```
age_days = (current_time - first_seen_time) / 86400000
age_score = min(age_days / 365, 1.0)  // Cap at 1 year
```

This is Sybil-resistant: attackers cannot backdate first-seen timestamps
(TOFU enforces this).

### 8.6.5. Consistency

Nodes with steady positive behavior over time score higher than nodes with
erratic behavior.

```
consistency = variance(weekly_scores_over_6_months)
consistency_score = 1.0 - normalize(consistency)
```

This is Sybil-resistant: new attackers have no history to fake.

## 8.7. Trust Aggregation

Nodes MAY aggregate signals into an overall trust score:

```
trust_score(peer) =
  0.5 * direct_experience_score(peer)
  + 0.2 * weighted_attestation_score(peer)
  + 0.15 * age_score(peer)
  + 0.15 * consistency_score(peer)
```

Weights are implementation-defined. Direct experience SHOULD have the
highest weight.

Trust scores are local and private. Nodes do not share their trust scores
(only attestations, which are public observations).

## 8.8. Trust-Based Actions

Nodes MAY use trust scores to prioritize or deprioritize peers:

### 8.8.1. Connection Priority

```
if trust_score(peer) > HIGH_THRESHOLD:
  maintain_persistent_connection(peer)
elif trust_score(peer) > LOW_THRESHOLD:
  connect_on_demand(peer)
else:
  avoid(peer)
```

### 8.8.2. Data Prioritization

```
if trust_score(peer) > HIGH_THRESHOLD:
  accept_pool_exchange_eagerly(peer)
elif trust_score(peer) > LOW_THRESHOLD:
  accept_pool_exchange_cautiously(peer)
else:
  reject_pool_exchange(peer)
```

### 8.8.3. Relay Selection

```
relays = filter(all_public_nodes, lambda n: trust_score(n) > RELAY_THRESHOLD)
selected_relay = random_choice(relays)
```

## 8.9. Web of Trust (Transitive Trust)

Nodes MAY compute transitive trust:

```
Trust path: A trusts B, B trusts C => A may trust C (with decay)

transitive_trust(A, C) = trust(A, B) * trust(B, C) * decay_factor

decay_factor = 0.5 per hop (halves at each step)
```

Transitive trust helps bootstrap trust for new nodes:
- A knows B (high trust)
- B introduces C (attestation from B about C)
- A gives C provisional trust based on B's attestation
- A validates C through direct experience

## 8.10. Sybil Resistance

Toloo's trust model resists Sybil attacks (where an attacker creates many
fake identities) through:

1. **Direct experience dominates**: Your own observations outweigh all attestations
2. **Attester quality matters**: Attestations from strangers have zero weight
3. **Age matters**: New identities have low trust
4. **Consistency matters**: Erratic behavior lowers trust
5. **TOFU prevents impersonation**: Cannot steal an existing trusted identity

An attacker can create a million fake nodes, but they will all have:
- Zero direct experience with honest nodes
- Zero attestations from honest nodes
- Zero age
- No path to honest nodes in the trust graph

These fake nodes remain isolated and untrusted.

## 8.11. Trust Aggregation Services (Optional)

Nodes or third parties MAY run **trust aggregation services** that:
- Collect attestations from the network
- Compute aggregate reputation scores
- Provide queryable APIs

These services are:
- OPTIONAL (not part of the protocol)
- Non-authoritative (nodes decide whether to use them)
- Forkable (anyone can run their own)
- Auditable (based on public attestations)

Example: A service like Spamhaus for email, but for Toloo nodes.

Nodes accessing such services SHOULD:
- Validate returned attestations (verify signatures)
- Treat scores as advisory, not authoritative
- Maintain their own local trust scores

## 8.12. Blacklists and Whitelists

Nodes MAY maintain local blacklists (blocked nodes) and whitelists (always
trusted nodes):

**Blacklist**: Nodes MUST reject all data from blacklisted nodes, regardless
of trust score.

**Whitelist**: Nodes MAY bypass trust checks for whitelisted nodes.

These lists are local and private. There is no protocol-level blacklist or
whitelist.

## 8.13. Privacy Considerations

### 8.13.1. Attestations are Public

Attestations propagate through pools and are visible to anyone. Nodes SHOULD
NOT include sensitive information in attestation `detail` fields.

### 8.13.2. Trust Scores are Private

Trust scores are computed locally and are not shared. A node's trust score
for a peer is private to that node.

### 8.13.3. Correlation Risks

Attestations can be correlated to infer social graphs:
- Who interacts with whom
- Who trusts whom
- Network topology

Nodes in high-security scenarios SHOULD be aware of this metadata leakage.

## 8.14. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
