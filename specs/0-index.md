# Toloo Protocol Specification v0.2

Status: Draft
Version: 0.2

This specification is maintained in the [`toloo`](../README.md) monorepo under
[`specs/`](README.md), alongside the reference implementation.

## Overview

Toloo is a messaging protocol where room-level policy controls the degree
of decentralization. It defines a self-authenticating data model,
cryptographic commit flow, and pool-based replication that supports
deployments from fully decentralized peer-to-peer networks to centralized
single-server products.

This specification is organized as a series of documents. Each document covers
one concern and may reference earlier documents by number. Appendices provide
supplementary material.

## Documents

| #  | Document | Status | Description |
|----|----------|--------|-------------|
| 0  | [Index](0-index.md) | Draft | This file. Document map and status. |
| 1  | [Concepts](1-concepts.md) | Draft | Terminology, architecture, design goals. |
| 2  | [Data Model](2-data-model.md) | Draft | Datum format, canonical JSON, datum identity, standard fields. |
| 3  | [Cryptography](3-cryptography.md) | Draft | Algorithms, key sizes, signing, encryption, Proof-of-Work. |
| 4  | [Identity](4-identity.md) | Draft | Node keypairs, metadata, connectivity (public/NAT'd), TOFU, `toloo://` URIs, `.toloo` files. |
| 5  | [Rooms](5-rooms.md) | Draft | Room keypairs, channels, metadata (ch -1), membership (ch -2), rules, portable authority. |
| 6  | [Events](6-events.md) | Draft | Event types, uncommitted/committed/side categories, action classification, timestamps (ts/tc), immutability. |
| 7  | [Replication](7-replication.md) | Draft | Pool model, seeding, committing flow, bisection sync, pool exchange, partition recovery. |
| 8  | [Trust](8-trust.md) | Draft | Attestation format, claims, trust evaluation, reputation, Sybil resistance. |
| 9  | [Transport](9-transport.md) | Draft | Carrier model, endpoint descriptors, message framing. |
| 10 | [Endpoints](10-endpoints.md) | Draft | Request/response definitions, error codes, endpoint registry. |
| 11 | [Storage](11-storage.md) | Draft | Retention classes, pruning rules, archival nodes, garbage collection. |
| 12 | [Discovery](12-discovery.md) | Draft | Seed nodes, bootstrap flow, URI import, gossip, discovery without seeds. |
| 13 | [Moderation](13-moderation.md) | Draft | Local blocking, spam marking, PoW enforcement, room defaults, invite-only. |
| 14 | [Lifecycle](14-lifecycle.md) | Draft | Room key transfer, forking, migration, deprecation. |

## Appendices

| #  | Document | Status | Description |
|----|----------|--------|-------------|
| A  | [Registries](A-registries.md) | Draft | Normative lists of event types, rule types, error codes, attestation claims, content schemas, constants. |
| B  | [Wire Examples](B-wire-examples.md) | Draft | Annotated JSON traces of common flows (join, send, commit, pool sync, partition recovery, fork, attestation, PoW). |
| C  | [Security Considerations](C-security-considerations.md) | Draft | Threat model, cryptographic assumptions, key management, metadata leakage, known limitations, security recommendations. |
| D  | [Conformance](D-conformance.md) | Draft | Conformance levels (minimal/client/relay), feature matrix, validation checklist, interoperability requirements. |
| E  | [Test Vectors](E-test-vectors.md) | Draft | Deterministic test vectors for cross-implementation verification: crypto, canonical JSON, envelopes, encryption, PoW, membership, rules. |

## Reading Order

For implementers:
1. Start with **1 Concepts** for the mental model.
2. Read **2 Data Model** and **3 Cryptography** for the foundation.
3. Read **4 Identity** and **5 Rooms** for the core entities.
4. Read **6 Events** and **7 Replication** for how data flows.
5. Read **9 Transport** and **10 Endpoints** for the wire protocol.
6. Validate your implementation against **E Test Vectors**.
7. Remaining documents in any order.

For auditors / reviewers:
1. Start with **1 Concepts** and **C Security Considerations**.
2. Read **3 Cryptography** and **8 Trust**.
3. Remaining documents as needed.

## Conventions

All normative documents in this specification use keywords defined in
[RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119) and
[RFC 8174](https://datatracker.ietf.org/doc/html/rfc8174).

Cross-references use the format `[doc-number]` (e.g., "see [2] Data Model").
Section references use `[doc-number#section]` (e.g., "[5#rules]").
