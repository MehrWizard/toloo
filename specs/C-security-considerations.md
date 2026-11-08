# Appendix C: Security Considerations

Status: Draft
Part of: Toloo Protocol Specification v0.2
See also: [0-index.md](0-index.md)

## C.1. Purpose

This appendix describes the threat model, security assumptions, known
limitations, and what Toloo does NOT protect against.

Toloo is designed for censorship resistance, not anonymity. It provides
message authenticity and integrity, but metadata protection requires
additional layers (Tor, I2P, VPNs).

## C.2. Threat Model

### C.2.1. Design Goals

Toloo is designed to resist:

| Threat | Mitigation |
|--------|------------|
| **Message tampering** | Ed25519 signatures on all datums |
| **Impersonation** | Cryptographic proofs (TOFU + attestations) |
| **Censorship** | Distributed pool model, portable room keys |
| **Single point of failure** | No central authority required |
| **Network partitions** | Store-and-forward pool, uncommitted events |
| **Spam and Sybil attacks** | Proof-of-Work, rate limiting, replication policies |
| **History fabrication** | Immutable commit envelopes, nested signatures |

### C.2.2. Adversary Capabilities

**Assumed adversary capabilities:**
- Can monitor network traffic (deep packet inspection)
- Can block IP addresses and domain names
- Can compromise individual nodes (but not steal keys retroactively)
- Can perform traffic analysis
- Can run malicious nodes
- Can attempt spam/Sybil attacks

**Assumed adversary limitations:**
- Cannot break Ed25519, X25519, ChaCha20-Poly1305, or SHA-256
- Cannot steal private keys from secure hardware
- Cannot compromise all public nodes simultaneously
- Cannot prevent protected endpoint connections between willing parties

## C.3. Cryptographic Assumptions

### C.3.1. Algorithm Security

Toloo's security depends on:

| Algorithm | Purpose | Security Assumption |
|-----------|---------|---------------------|
| **Ed25519** | Signatures | Discrete log problem hard in Curve25519 |
| **X25519** | Key agreement | Computational Diffie-Hellman hard |
| **ChaCha20-Poly1305** | Encryption | Semantic security, authenticated encryption |
| **SHA-256** | Hashing, PoW | Collision resistance, preimage resistance |
| **HKDF-SHA256** | Key derivation | Pseudorandom function assumption |

If any of these algorithms are broken, Toloo's security guarantees fail.

### C.3.2. No Cryptographic Agility

Toloo does NOT support algorithm negotiation. All implementations MUST use
the exact algorithms specified.

**Rationale**: Cryptographic agility has historically been a source of
downgrade attacks (FREAK, Logjam, POODLE). Fixed algorithms are simpler and
safer.

**Consequence**: If SHA-256 or Ed25519 is broken, the protocol MUST be
updated (major version bump).

## C.4. Key Management

### C.4.1. Private Key Storage

**Node keys (sig_priv, enc_priv):**
- MUST be stored encrypted at rest
- SHOULD use OS keychain (macOS Keychain, Windows DPAPI, Linux Secret Service)
- SHOULD use hardware security modules if available (YubiKey, TPM)
- MUST NOT be stored in plaintext files or environment variables

**Room keys (room_priv):**
- MUST be encrypted with a local key-encryption mechanism (passphrase/KMS/HSM)
- SHOULD be backed up securely (encrypted cloud, paper wallet)
- MUST be transmitted out-of-band for key transfer (never via protocol)

### C.4.2. Key Loss

**Permanent consequences:**
- Node key loss → lose identity, all room memberships, all reputation
- Room key loss → room becomes read-only (no new commits)

**No recovery mechanism exists in v0.2.** Key loss is catastrophic.

Implementations SHOULD:
- Warn users during key generation
- Require explicit backup before allowing participation
- Support export to encrypted files or QR codes

### C.4.3. Key Compromise

**If node key stolen:**
- Attacker can impersonate user
- Cannot retroactively modify already-validated historical datums
- User MUST generate new identity and announce out-of-band
- Old identity cannot be revoked (attestations remain)

**If room key stolen:**
- Attacker can commit arbitrary events (bans, metadata changes)
- Attacker can fork the room
- Legitimate key holder MUST fork room with new key
- Old room becomes untrustworthy

## C.5. Metadata Leakage

### C.5.1. What Metadata Is Exposed

Toloo does NOT hide:
- Room membership (who is in which room)
- Message timing (ts field visible to all members)
- Message authors (n field visible to all members)
- Social graph (who talks to whom)
- Network topology (via hints, pool gossip)
- Message frequency (uncommitted events visible before commit)

### C.5.2. Traffic Analysis

**Passive observers can infer:**
- Alice and Bob are communicating (even if content is encrypted)
- Alice is active at specific times (ts timestamps)
- Alice is member of Room X (membership channel visible)
- Alice uses Relay Y (connection metadata)

**Active attackers running public nodes can:**
- Log all connection attempts
- Correlate IP addresses with node signatures
- Build social graphs from pool exchange patterns
- Timing attacks on uncommitted → committed flow

### C.5.3. Mitigations

**For anonymity, users MUST use additional layers:**

| Layer | Protection | Trade-off |
|-------|------------|-----------|
| **Tor** | Hides IP address | Slower, may be blocked |
| **I2P** | Hides IP address, traffic shape | Complex setup |
| **VPN** | Hides IP from relay | Relay still sees node sig |
| **Timing obfuscation** | Random delays in sending | Higher latency |

Toloo provides transport encryption and data authenticity, NOT anonymity.

### C.5.4. DPI Fingerprinting and Resistance

Using standard TLS (`wss`/`https`) makes carrier traffic indistinguishable
from ordinary HTTPS traffic, which resists protocol-specific fingerprinting.
Toloo defines no custom handshake or framing on the wire: everything above
the carrier is ordinary JSON (or length-prefixed binary pieces) inside the
TLS channel.

Residual DPI risk remains:
- Traffic timing/volume can still reveal usage patterns
- Long-lived connections can still be profiled statistically
- TLS-layer metadata (SNI, certificate) may reveal the relay host
- Without TLS (`ws`/`http`), all traffic is plaintext

Operational mitigations for hostile networks:
- Use `wss`/`https` so carrier traffic looks like ordinary TLS
- Route connections through a SOCKS5 proxy to hide the destination IP
- Rotate endpoint descriptors periodically
- Use network-layer anonymity systems (Tor) when needed

### C.5.5. Private Message Metadata

Private messages encrypt content but expose metadata:
- `d.n` (sender) and `d.to` (recipient) are visible
- `d.ts` (timestamp) is visible
- Message size (ciphertext length) is observable

This is the accepted tradeoff for pool-based delivery. Content privacy
is provided by static-static X25519 + ChaCha20-Poly1305 with a random
per-message salt. This is **not forward secret**: a leaked `enc` key
decrypts stored messages (see C.12.1). Metadata privacy requires
additional layers (Tor, timing obfuscation).

## C.6. Network Attacks

### C.6.1. Man-in-the-Middle (MITM)

**Connection MITM:**
- Mitigated by datum signature verification and TOFU pinning
- Active MITM on first contact is possible (TOFU limitation)

**Content MITM:**
- Transport tampering is resisted by TLS on `wss`/`https` carriers
- All datums are signed by authors (tampering always detectable)

### C.6.2. Replay Attacks

**Datum replay:**
- Timestamps (ts) provide freshness indication
- Nodes use `d.exp` and room rules for forwarding/storage decisions, not clock comparison against `d.ts`
- Duplicate eid detection prevents processing same event twice

**Transport replay:**
- Fresh ephemeral keys create fresh transport keys per connection
- Old transport keys do not apply to new connections
- Nonce counters prevent replay within one connection

### C.6.3. Denial of Service (DoS)

**Protocol-level DoS vectors:**
- Spam uncommitted events (mitigated by PoW, rate limits)
- Large pool.exchange requests (mitigated by limit parameter)
- Excessive membership events (mitigated by room signature verification)

**Network-level DoS vectors:**
- Flood public nodes with connections (mitigated by IP rate limiting)
- DDoS against relay infrastructure (no protocol-level mitigation)

Public nodes SHOULD implement:
- Connection rate limiting (max N connections per IP per minute)
- Request rate limiting (max M requests per connection per second)
- Bandwidth throttling
- Blacklisting of malicious IPs

### C.6.4. Eclipse Attacks

**Definition**: Attacker surrounds victim with malicious nodes, filtering their view of the network.

**Attack scenario:**
1. Alice only connects to attacker-controlled relays
2. Attacker withholds legitimate events
3. Attacker fabricates false events (fake commits)
4. Alice has distorted view of room state

**Mitigations:**
- Connect to multiple geographically diverse public nodes
- Verify room signatures on all committed events
- Compare state with out-of-band contacts
- Attestation-based trust (malicious nodes accumulate negative attestations)

**Residual risk**: First-time users with no TOFU pins are vulnerable.

## C.7. Spam and Sybil Attacks

### C.7.1. Sybil Attack Vectors

**Creating unlimited identities:**
- Sig keys are free to generate
- No global identity registry
- Attacker can create 10,000 fake nodes

**Mitigated by:**
- Proof-of-Work (computational cost per identity)
- Replication policies (designated/members-only rooms)
- Invite-only defaults
- Attestation-based reputation (new nodes have zero trust)

**Residual risk**: Well-resourced attacker with GPU farms can overcome PoW.

### C.7.2. Spam Attacks

**Uncommitted message spam:**
- Attacker floods room with uncommitted messages
- Nodes see spam before key holder can reject

**Mitigated by:**
- Rate limits (max N messages per sig per minute)
- Proof-of-Work per message (post_pow rule)
- Uncommitted TTL (discard after timeout)
- Local blocklists/spam filtering (client-side)

**Committed spam:**
- Requires room key holder approval
- Attacker must compromise key holder or be the key holder

**Residual risk**: Malicious key holder can commit spam. Mitigation: fork.

### C.7.3. Join Spam

**Attacker creates 1000 identities and joins room:**
- Membership channel bloated
- Snapshot size grows
- Sync becomes expensive

**Mitigated by:**
- join_pow rule (computational cost per join)
- invite_only rule (only invited nodes can join)
- Committed membership events are verified by room signature

**Residual risk**: GPU farm can still generate PoW at scale.

## C.8. Trust and Reputation

### C.8.1. Attestation Attacks

**False positive attestations:**
- Attacker creates 100 fake nodes
- Each attests positively to BadNode
- BadNode appears trustworthy

**Mitigated by:**
- Weight attestations by attester's own reputation (transitive trust)
- Require direct experience (local testing of claims)
- Long observation periods (10+ days)

**False negative attestations:**
- Attacker slanders GoodNode with false claims
- GoodNode's reputation damaged

**Mitigated by:**
- Reputation is local (each node decides for itself)
- Check attester's history (serial false accusers lose credibility)
- Out-of-band verification

### C.8.2. Trust Aggregation Services

**Centralization risk:**
- If everyone uses one trust service, it becomes a single point of control
- Compromised service can manipulate reputation globally

**Mitigations:**
- Multiple independent trust services
- Open-source aggregation (auditable)
- Clients can fork and run their own

**Best practice**: Use 2-3 trust services and compare results.

## C.9. Room Security

### C.9.1. Malicious Key Holder

**Attack scenarios:**
- Key holder bans all members
- Key holder modifies room metadata arbitrarily
- Key holder commits spam messages
- Key holder refuses to commit legitimate events

**Protocol response:**
- All actions are visible (logged in event history)
- Members can detect abuse
- Members can fork the room (escape mechanism)

**No protocol-level enforcement of "good" key holder behavior.** Governance is social, not technical.

### C.9.2. Multiple Key Holders

**Conflict scenario:**
- Key holder A commits event at tc=1000
- Key holder B commits conflicting event at tc=1001
- Both commits are valid

**Resolution:**
- Last-write-wins based on tc (deterministic)
- All nodes converge to same state

**Risk**: Malicious key holder with faster clock can override others.

**Mitigation**: Key holders should coordinate out-of-band.

### C.9.3. Room Forking

**Fork abuse:**
- Attacker forks room repeatedly
- Announces multiple forks via side.fork
- Confuses members

**Mitigations:**
- Forks are opt-in (members choose which to join)
- Original room identity unchanged
- Reputation of forker matters (attestations)

**Best practice**: Only fork with community consensus.

## C.10. Implementation Vulnerabilities

### C.10.1. Timing Attacks

**Signature verification timing:**
- Ed25519.verify() MUST be constant-time
- Variable-time implementations leak key bits

**Mitigation**: Use well-vetted cryptographic libraries (libsodium, ring, noble-crypto).

### C.10.2. Side-Channel Attacks

**Hardware attacks:**
- Power analysis during key operations
- Electromagnetic emanations
- Cache timing attacks

**Mitigation**:
- Use hardware security modules (HSM, TPM)
- Constant-time implementations
- Memory protection (prevent key swapping to disk)

### C.10.3. Memory Safety

**Buffer overflows, use-after-free:**
- Common in C/C++ implementations
- Can lead to key extraction or remote code execution

**Mitigation**:
- Use memory-safe languages (Rust, Go, TypeScript)
- Extensive fuzzing and static analysis
- Sandboxing (run node in restricted environment)

## C.11. Compliance and Legal Risks

### C.11.1. Lawful Interception

Toloo does NOT provide lawful interception capabilities.

**Consequences:**
- May be illegal to operate in some jurisdictions
- Relay operators may face legal pressure
- Users in authoritarian regimes face prosecution risk

This is by design. Toloo is a censorship-resistant protocol.

### C.11.2. Data Retention Laws

Some jurisdictions require service providers to retain user data.

**Toloo's position:**
- Nodes SHOULD prune old data (storage efficiency)
- No protocol-level retention mandates
- Archival nodes are optional, not required

Relay operators MUST understand local legal requirements.

### C.11.3. Export Control

Cryptographic software may be subject to export control laws (ITAR, EAR, Wassenaar).

**Recommendations:**
- Implementations SHOULD include export compliance notices
- Open-source publication may qualify for exemptions
- Consult legal counsel before distribution

## C.12. Known Limitations

### C.12.1. Forward Secrecy

**Room content**: No forward secrecy. Room data is durable and publicly
verifiable by design. Stealing `room_priv` allows forging commit envelopes.
Stealing `sig_priv` allows impersonation.

**Private messages**: No forward secrecy. v2 private messages are
static-static X25519: the sender's and recipient's long-lived `enc` keys
derive a pairwise key, and a random per-message salt derives the message
key. Compromising a node's `enc` key reveals every stored message sent to
that node; because the sender's static key is used too, it also reveals
messages that node sent. (The legacy v1 ephemeral scheme did not fix
this: `eph_pub` is stored with the ciphertext, so the recipient's
long-term key still decrypts history.) Forward secrecy would require
prekeys/a ratchet, which adds client state and bootstrap metadata and is
not part of the core envelope model.

**Transport**: TLS session keys are per-connection and erased on close.
Past transport sessions are protected by TLS forward secrecy.

### C.12.2. No Post-Quantum Resistance

Ed25519, X25519, and SHA-256 are vulnerable to quantum computers (Shor's algorithm, Grover's algorithm).

**Timeline:**
- Current quantum computers: ~50 qubits (not a threat)
- Breaking Ed25519 requires ~4000 logical qubits
- Estimated timeline: 10-20 years (uncertain)

**Future work**: Migrate to post-quantum algorithms (CRYSTALS-Dilithium, Kyber).

### C.12.3. No Anonymous Credentials

Room membership is not anonymous. All members know each other's signatures.

**Consequence**: Cannot have anonymous voting, pseudonymous posting, or hidden membership.

**Workaround**: Use separate sig keys per room (operational burden).

### C.12.4. Metadata Not Protected

As described in C.5, Toloo exposes extensive metadata.

**Not suitable for:**
- High-risk activists requiring anonymity
- Whistleblowers
- Journalists protecting sources

**Use cases requiring metadata protection MUST layer Tor/I2P.**

## C.13. Comparison with Other Protocols

### C.13.1. Signal

| Aspect | Signal | Toloo |
|--------|--------|-------|
| **Architecture** | Central servers | Hybrid (pool-based) |
| **Metadata protection** | Sealed Sender (partial) | None (use Tor) |
| **E2E encryption** | Yes (Double Ratchet) | Private messages only (static-static X25519, no forward secrecy) |
| **Censorship resistance** | Domain fronting (deprecated) | Distributed pools |
| **Offline messaging** | Via central servers | Via public nodes |
| **Key recovery** | None | None |
| **Disappearing messages** | Yes | No |

**Signal is better for**: 1:1 messaging, metadata protection, mobile UX.

**Toloo is better for**: Censorship resistance, no central authority, group rooms.

### C.13.2. Matrix

| Aspect | Matrix | Toloo |
|--------|--------|-------|
| **Architecture** | Federated servers | Pool-based |
| **E2E encryption** | Yes (Olm/Megolm) | Private messages only (static-static X25519, no forward secrecy) |
| **Storage model** | Full state replication | Prunable pools |
| **Room authority** | Distributed (federation) | Portable room keys |
| **Metadata protection** | None | None |
| **Complexity** | High (federation) | Moderate (pool sync) |

**Matrix is better for**: Federation, rich features, established ecosystem.

**Toloo is better for**: Portable authority, bounded storage, simplicity.

### C.13.3. Tor Messenger (Defunct)

| Aspect | Tor Messenger | Toloo |
|--------|---------------|-------|
| **Anonymity** | Yes (via Tor) | No (add Tor layer) |
| **Offline messaging** | No | Yes |
| **Speed** | Slow (Tor overhead) | Fast (direct connections) |
| **Censorship resistance** | High (if Tor not blocked) | High (if public nodes reachable) |

**Toloo can use Tor as transport** for anonymity while preserving offline messaging.

## C.14. Security Recommendations

### C.14.1. For Users

1. **Use Tor transport** if anonymity is required
2. **Backup keys securely** (encrypted cloud, paper wallet)
3. **Verify TOFU pins** out-of-band (phone call, in-person)
4. **Connect to multiple public nodes** (avoid eclipse attacks)
5. **Check attestations** before trusting unknown nodes
6. **Review room rules** before joining
7. **Fork abusive rooms** rather than staying

### C.14.2. For Relay Operators

1. **Use dedicated servers** with static IPs
2. **Enable HTTPS** (TLS 1.3 minimum)
3. **Rate-limit connections** (prevent DoS)
4. **Monitor for abuse** (spam, malicious datums)
5. **Geographic diversity** (operate in multiple jurisdictions)
6. **Publish attestations** (transparency about uptime, behavior)
7. **Secure key storage** (HSM, encrypted filesystems)

### C.14.3. For Implementers

1. **Use vetted crypto libraries** (libsodium, ring, noble-crypto)
2. **Constant-time implementations** (prevent timing attacks)
3. **Memory safety** (Rust, Go, or extensive testing in C/C++)
4. **Fuzzing** (AFL, libFuzzer, Honggfuzz)
5. **Static analysis** (Coverity, Clang analyzer)
6. **Security audits** (third-party review before production)
7. **Incident response plan** (key compromise, vulnerability disclosure)

## C.15. Vulnerability Disclosure

Toloo is an open protocol. Security issues should be reported responsibly.

**Recommended process:**
1. **Private disclosure** to implementation maintainers
2. **Coordinate fix** (90-day disclosure window)
3. **Public disclosure** with patches available
4. **CVE assignment** for tracking

**Do NOT:**
- Publicly disclose 0-days without coordination
- Exploit vulnerabilities for personal gain
- Attack production deployments for research

Responsible disclosure protects users while allowing fixes.

## C.16. Conclusion

Toloo is designed for **censorship resistance**, not anonymity or confidentiality (in v0.2).

**What Toloo protects:**
- Message authenticity (Ed25519 signatures)
- Message integrity (cannot be tampered)
- Censorship resistance (distributed pools, portable keys)
- Network partition tolerance (store-and-forward)

**What Toloo does NOT protect:**
- Metadata (use Tor/I2P)
- Room content privacy (group E2E encryption deferred; private messages are E2E encrypted)
- Key loss (no recovery mechanism)
- Quantum attacks (no post-quantum crypto)
- Malicious key holders (social governance only)

**Use Toloo when:**
- Censorship is the primary threat
- You need offline messaging
- You can accept metadata leakage
- You can layer Tor for anonymity if needed

**Do NOT use Toloo when:**
- Metadata protection is critical
- You need provable anonymity
- You cannot backup keys securely
- You need compliance with lawful interception

Security is a spectrum, not a binary. Toloo makes specific trade-offs for
censorship resistance. Understand these trade-offs before deployment.

## C.17. References

- [RFC2119] Key words for use in RFCs to Indicate Requirement Levels
- [RFC8174] Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words
- [RFC7748] Elliptic Curves for Security (X25519)
- [RFC8032] Edwards-Curve Digital Signature Algorithm (Ed25519)
- [RFC8439] ChaCha20 and Poly1305 for IETF Protocols
- [Signal] Signal Protocol Documentation
- [Matrix] Matrix Specification
- [Tor] Tor Project Design Documents
