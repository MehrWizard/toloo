# TODO — Current Roadmap

The original spec-gap tracker (Feb 2026) is retired; all Relay-conformance
gaps it tracked were implemented or consciously removed. History lives in
git. Current remaining work, in priority order:

## Documentation & repository hygiene

- [x] Move the normative specification into `specs/`, including the pending
      local edits from the old standalone spec repository.
- [x] Archive retired Python/JS/TS prototypes, the old spec snapshot, branding
      drafts, and the old binary archive out of tree with README notes.
- [x] Document the isolated-compositor rule for native GUI tests and
      screenshots.
- [x] Rename source directories to `app/`, `cli/`, `core/`, `lib/`, `wasm/`,
      and `web/`; product/package names remain `toloo-*`.
- [x] Consolidate all benchmark code, scripts, templates, and assets under
      `bench/`; results and the dashboard now live only in
      `MehrWizard/toloo-benchmarks`.
- [ ] Keep README, TODO, component docs, and spec indexes current with each
      release.

## Client tier (constrained devices — Galaxy J5 2016 class)

- [ ] `PoolConfig`: resident-event cap + cold-log fallback (stream old
      ranges from `<base>.jsonl` on demand) + tail-only replay on open
- [ ] Default retention per room class (public rooms: hours–days;
      personal: months) applied when a room defines no rules
- [ ] Client UI: background rooms as sync cursors only (no live ingest)
- [ ] Client UI: per-room LRU tail windows + batched foreground ingest
- [ ] Client UI: virtualized message list (render visible rows only)

## Toloo app (2026-11-01)

- [x] `toloo-app` crate: Slint UI + engine thread over `toloo-lib`; native
      WS/HTTP transports, local Pool, config persistence, room authority.
- [x] Desktop build and engine tests (identity, restore, create room, send,
      committed status, WS and HTTP relay sync).
- [x] `native.yml` (App workflow): desktop release builds + experimental
      Android APK job; release workflow now ships app binaries instead of
      Tauri.
- [x] Remove the Tauri app from `main`; its last snapshot is preserved as the
      `toloo-app-legacy` tag.
- [x] Install and exercise release APKs on physical Android 8+ devices:
      IME, emoji keyboard, selection handles, Back navigation, and upgrades.
- [ ] Finish Android device validation across API 26-class hardware: TLS and
      renderer coverage, APK size/RSS, and long-session behavior.
- [ ] Android release signing/AAB and store packaging, including a Play Store
      auto-publish workflow using a service-account secret for tagged builds.
- [x] Encrypted local vault: setup/unlock passphrase, plaintext config
      migration, and room keys encrypted with the identity.
- [ ] Native feature parity: embedded relay hosting, private messages/UI,
      discovery import/export, blobs, moderation, notifications and background
      sync (foreground service).
- [ ] iOS feasibility spike with the pinned Slint version.
- [ ] Android API 24–25 alternative is explicitly out of scope for Slint;
      it would require native Android UI + FFI or another WebView shell.

## Networking

- [ ] SOCKS5 client-side routing (hand-rolled CONNECT, no dependency) for
      CLI and app clients behind censored networks
- [ ] Batched fsync policy in the pool append path (crash-durability polish)

## Shared core & browser (2026-11-01)

- [x] Extract portable `toloo-core` (no I/O, no tokio, no TLS) and make
      `toloo-lib` re-export it; native runtime keeps pool/transport/requests/
      trust/keystore. `toloo-lib` crypto deps are now feature-gated.
- [x] `toloo-wasm` release target: JSON contract, `wasm-bindgen` exports,
      `PairKeyCache`, wasm32 tests (Node) and a generated-package smoke test.
- [x] Wasm client protocol surface: all event builders, all request builders,
      chain verification/batching, private v2 with cached pair keys,
      membership (shared core algorithm), room rules, discovery URI/file,
      PoW solve/verify, hashing/base64, envelope metadata.
- [x] Core CI workflow: native workspace tests/checks + wasm tests/build.
- [x] SolidJS browser client (`web/`): onboarding, identity persistence,
      relay settings, create/join rooms, verified sync, IndexedDB envelope
      store, membership display, chat/send, invites, responsive UI.
- [x] Browser store layer (IndexedDB) and JS transport (WebSocket/HTTP) using
      wasm `envelopeInfo`/`buildMembership`/`RoomRules`. Transport policy:
      `https:` pages use only `https/wss`; `http:` pages use only `http/ws`;
      enforced in the adapter + covered by unit tests.
- [x] Relay conformance fix: `pool.exchange`/`events.sync` now return
      uncommitted room events so browser key holders can discover and commit
      pending joins/messages (§7.7.3).
- [x] Browser-compatible HTTP relay endpoint: CORS preflight/response headers,
      plus `toloo relay --http-port <port> --http-path <path>` for local HTTP
      testing.
- [x] Retire `toloo-js`; the legacy client is archived out of tree and the
      browser client uses `toloo-wasm` + `toloo-web`.
- [ ] Web client: direct-message UI using `PairKeyCache.makeMessage/openMessage`.
- [x] Web client: encrypted local vault for identity/room keys, with
      setup/unlock and migration from plaintext IndexedDB records.
- [ ] Web client: notifications, offline/service-worker shell, message
      reactions/edits UI, blob upload/download UI, moderation UI.
- [ ] Deployment CSP for `toloo-web` (`connect-src` scheme-paired).
- [ ] Index uncommitted room events instead of scanning all shards in
      `get_pending` (native performance; key-holder polling path).
- [ ] Blob split/reassemble helpers in wasm (hashing + `makeRoomBlob` exist;
      blob design itself remains postponed).
- [ ] Benchmarks on real target devices (older phones) and a SIMD-vs-scalar
      wasm comparison.

## Verification

- [x] Transport unit tests (`web/src/lib/transport.test.ts`), web build
      in core CI.
- [ ] Automated browser E2E tests (identity → relay → create/join → send →
      commit) replacing ad-hoc local testing.

## CI & Benchmarking (Complete)

- [x] GitHub Actions benchmark workflow (`.github/workflows/benchmark.yml`)
      — 5 platforms (linux-x86, linux-arm, windows, macos-x86, macos-arm)
      with `continue-on-error: true` and `if: always()` resilience
- [x] Aggregation into the external `toloo-benchmarks` results files with per-platform `mean_ns`, `median_ns`, `std_dev_ns`
- [x] Static HTML benchmark visualization (`bench/templates/index.html.template` + `bench/scripts/substitute_template.py`)
      — log-scale bars, parity detection, CSS hover tooltips, equal-width columns
- [x] Cross-repo push to `MehrWizard/toloo-benchmarks` with per-commit results
- [x] Opt-in same-host A/B (`baseline_ref` input) on linux-x86, with a Criterion
      comparison report in the job summary and artifact
- [x] Baseline comparison with 10%/20% regression thresholds (non-blocking)
- [x] Cross-protocol benchmarks: `bench/` (`toloo-bench`) measures Signal
      (libsignal X3DH + Double Ratchet), Matrix (ruma signatures, vodozemac
      Olm) and Nostr (Schnorr, NIP-44). Feature-gated (`protocols`), Linux x86
      CI job, measured rows + 1:1 card on the dashboard, `est` tags for the
      remaining reference estimates.
- [x] Private-message v2: static-static X25519 with cached pair keys, a
      random 32-byte per-message salt, and v1 decrypt compatibility. The 1:1
      card reports cached steady state plus pair-setup first-message costs.
      No forward secrecy (documented); sender static-key compromise exposes
      outbound history.
- [x] First-message economics: `signal_inbound` / `olm_inbound` measure
      responder pre-key processing; the 1:1 card shows setup-inclusive
      send → receive numbers.
- [x] Retire `toloo-js` instead of migrating it: the browser client moves to
      `toloo-wasm` (v2 native) + SolidJS. The retired client and old spec
      snapshot are archived out of tree.
- [ ] Add a deterministic v2 private-message vector to
      `specs/E-test-vectors.md`.
- [x] Faster crypto backend: X25519, ChaCha20-Poly1305, and Ed25519 signing use
      `aws-lc-rs` (assembly + runtime CPU detection; already linked via rustls)
      with the dalek/RustCrypto fallback when TLS/aws-lc is disabled. Ed25519
      verification stays on dalek (faster single verify + Pippenger batch).
      Table-driven base64url, a canonical-string fast path, and in-place AEAD
      removed the remaining per-message overhead. Matching a ratchet per
      message would require a session layer.
- [ ] Future cross-protocol coverage: measured IRC/XMPP serialization (no
      per-message crypto), and MLS/OpenMLS as the room-commit analogue.

## Codebase audit (2026-10-29)

Findings verified against the current tree. Fix in this order: P0 (correctness /
security) before touching performance.

### P0 — Correctness & security (fixed)

- [x] **`toloo relay` panics on startup.** Fixed: `BatchProcessor::new` no
      longer spawns; `run_relay` starts the task from inside its `LocalSet`
      (`BatchProcessor::start`). Verified: relay listens, idle CPU 0%.
      Regression test: `batch_processor_starts_inside_local_set_and_processes`.
- [x] **pool.exchange signature-verification bypass / pool poisoning.**
      Fixed: `verify_chains_batch` returns per-envelope verdicts and the
      handler stores only `Ok` envelopes; `validate_pubkey_field` rejects
      non-decompressible pubkeys at parse. Regression tests:
      `pool_exchange_stores_only_verified_envelopes`,
      `parse_envelope_rejects_invalid_point_pubkey`.
- [x] **Depth ≥ 2 offers never batch-verify.** Fixed: `verify_chains_batch`
      batches every layer of every chain (payload/sig/key per layer) and
      attributes failures per envelope. Regression test:
      `verify_chains_batch_verifies_all_layers`.
- [x] **`unsafe impl Sync for Shard` is unsound.** Fixed: `events` is now
      `RwLock<Vec<…>>`; all `unsafe` access removed from `pool.rs`; writers
      release the events lock before taking index locks. The concurrent
      put/query test now runs on sound code.
- [x] **Spill path is racy and does not bound memory.** Fixed: spill holds
      each shard's `write_mutex`, sets spilled slots to `None` (dropping the
      envelopes), rebuilds indexes from survivors; `append_event` takes
      `spill_lock` in tiered mode so a concurrent append cannot be lost.
      Existing `tiered_storage_spills_to_cold` test passes.
- [x] **CLI `send` panicked** — `make_room_message(..., 0, ...)` uses channel 0,
      which is reserved for announcements. Changed to channel 1. Found while
      verifying the P0 fixes end-to-end.
- [x] **Client ignored binary WS frames** — the relay sends responses and the
      `{"done":true}` terminator as binary frames, but `pool_exchange_ws` only
      parsed `Message::Text`, so `toloo sync` hung. Now accepts both.

### P1 — Hot-path performance

- [x] **One canonicalization per envelope.** `canonical_datum_bytes_direct`
      and `canonical_envelope_bytes_direct` write canonical JSON without a
      `Value` tree; parse caches the datum bytes, verify recomputes them
      (tamper detection), and `put` reuses them for both the datum_id and the
      log line. Per-envelope serializations dropped from ~6-7 to 2.
- [x] **Canonical serializer without `serde_json::Value`.** Direct writers
      handle flattened `extra` fields, nested envelopes, UTF-16 key order,
      and string escaping; byte-for-byte tested against the Value path.
- [x] **`signable_bytes_verify` no longer clones a thread-local buffer.**
      The thread-local buffer was removed; verification writes directly into
      one right-sized Vec.
- [x] **Persistence opens the log once per batch.** `append_events` writes
      all lines from a single `put_batch` call under one file handle.
- [x] **Spill check is O(shards), not O(events).** Each shard tracks a `live`
      atomic; the global `spill_lock` is only taken when the threshold is
      crossed.
- [x] **Event-log I/O runs on a writer thread.** `Pool::open` spawns a
      persistence thread that owns a buffered hot-log handle; `put`/
      `put_batch` enqueue appends, and spill/compact are serialized through
      the same channel (spill now drains to `max - max/4` so the rewrite is
      amortized instead of running on every insert at capacity). Blob fetches
      run via `spawn_blocking`. Meta-file rewrites (`persist_meta`) remain
      synchronous — they are rare moderation operations.
      Tradeoff: appends are written asynchronously, so a crash may lose the
      most recent buffer (the log was already not fsync'd).
- [x] **`build_membership` is cached** by the latest committed channel -2
      tc; returns `Arc<MembershipState>` and invalidates on new membership
      commits (tested).
- [x] **Rate-limit check is index-backed** — `count_author_events_since`
      ranges over `by_author_room_tc` instead of scanning the whole pool.
- [x] **`system.fetch` uses `by_id` lookups** instead of a full scan.
      `latest_tc` uses a BTreeMap range (O(log n)).
- [x] **`get_wanted` range/filter correctness.** Specific-room queries now
      use a per-room index (`DashMap<Arc<str>, BTreeMap<tc, idx>>`) with a
      borrowed `&str` lookup — no room-key allocation per call — and a
      range bounded by the room. The previous open-ended range leaked events
      from lexicographically later rooms. The all-rooms branch and type
      filter no longer use `take(limit*2)`, so filtered results cannot be
      silently dropped (regression test added). `get_by_datum_ids` falls back
      to a single scan for small pools, where 16 shard hashes per id cost
      more than one pass.
- [x] **`PoolEntry` clones are allocation-free.** `id` is now an interned
      `Arc<str>`; every field is an `Arc`, so returning owned entries from
      `select`/`get_room_channel*` costs only refcount bumps.
- [x] **HTTP header parse is buffered** (`BufReader` + `read_until`), not one
      syscall per byte.
- [x] **WS frame copy (accepted limitation).** tungstenite 0.28 exposes
      `Message::Text` as immutable `Utf8Bytes`; simd-json needs a mutable
      slice, so one copy remains. HTTP passes its body buffer directly.
      Revisit if tungstenite exposes mutable text or we switch to binary
      frames end-to-end.
- [x] **`parse_envelope_ref` avoids deep-cloning offer values** in
      pool.exchange. The `maybe_commit` `env.clone()` remains (needed for
      the wrap-failure fallback).
- [x] **`MAX_OFFERS` (256) caps pool.exchange verification work.**
- [x] **`hkdf_sha256` uses stack buffers** instead of a Vec per block.
- [x] **Benchmark integrity:** `verify_chain` no longer skips depth-1
      verification on a cached layer (the depth-1 bench was measuring a
      no-op); `get_by_id` now benches the protocol's 100-id system.fetch cap.
- [x] **Ordered per-channel index (Phase 8).** `by_room_channel` is a
      `BTreeSet` ordered exactly like `cmp_committed_first`; room.sync
      ranges stop after `limit`, and `get_wanted` takes only the first/last
      `limit` candidates per room. Queries return `Arc<PoolEntry>` so row
      clones are refcount bumps. Local: get_room_channel_1k 61→11 µs,
      get_wanted 66→1.3 µs.

### P2 — Infrastructure polish

- [x] `BatchProcessor` busy-spun with `yield_now` when idle; `submit`
      deep-cloned the envelope. Fixed with tokio mpsc + blocking `recv()`
      and ownership transfer (needed to make the P0 startup fix usable).
- [ ] `process_pool_exchange_batch` does not actually merge offers
      (batch.rs) — merge offer arrays across requests.
- [ ] Client rebuilds the rustls config per connection and per HTTP request
      (client.rs:236, 276); cache in a `LazyLock`.
- [ ] WS `accept_async` uses tokio-tungstenite's default 64 MiB message cap
      (server.rs:236); set an explicit, smaller cap for relays.
- [ ] Evaluate `simd-json` upgrade (0.8 → current) for parser perf and fixes.

## Design gap: app relay identity is not persistent

The app generates a fresh relay keypair and a fresh self-signed TLS cert
on every relay start. Consequences: every saved invite URI breaks on
restart, TOFU cert pinning never stabilizes, and relay trust history
(attestations) resets. Fix direction: persist the relay identity per
relay config (generated once, stored — ideally passphrase-wrapped via
keystore), and derive the TLS cert deterministically from it so the
fingerprint is stable.

## Notes

- `Trust/attestations`, `room.flag`, and blob pieces are implemented stubs,
  intentionally kept for future work; their designs are postponed.
