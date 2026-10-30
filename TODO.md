# TODO — Current Roadmap

The original spec-gap tracker (Feb 2026) is retired; all Relay-conformance
gaps it tracked were implemented or consciously removed. History lives in
git. Current remaining work, in priority order:

## Client tier (constrained devices — Galaxy J5 2016 class)

- [ ] `PoolConfig`: resident-event cap + cold-log fallback (stream old
      ranges from `<base>.jsonl` on demand) + tail-only replay on open
- [ ] Default retention per room class (public rooms: hours–days;
      personal: months) applied when a room defines no rules
- [ ] `app.js`: background rooms as sync cursors only (no live ingest)
- [ ] `app.js`: per-room LRU tail windows + batched foreground ingest
- [ ] `app.js`: virtualized message list (render visible rows only)

## Networking

- [ ] SOCKS5 client-side routing (hand-rolled CONNECT, no dependency) for
      CLI and app clients behind censored networks
- [ ] Batched fsync policy in the pool append path (crash-durability polish)

## Verification

- [ ] `toloo-js` `selfTest()` against the regenerated Appendix E vectors
      (the JS client still has zero tests)

## CI & Benchmarking (Complete)

- [x] GitHub Actions benchmark workflow (`.github/workflows/benchmark.yml`)
      — 5 platforms (linux-x86, linux-arm, windows, macos-x86, macos-arm)
      with `continue-on-error: true` and `if: always()` resilience
- [x] Aggregation into `results/latest.json` with per-platform `mean_ns`, `median_ns`, `std_dev_ns`
- [x] Static HTML benchmark visualization (`index.html.template` + `substitute_template.py`)
      — log-scale bars, parity detection, CSS hover tooltips, equal-width columns
- [x] Cross-repo push to `MehrWizard/toloo-benchmarks` with per-commit results
- [x] Baseline comparison with 10%/20% regression thresholds (non-blocking)

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
      use a bounded `(room, …)` range — previously the open-ended range
      leaked events from lexicographically later rooms. The all-rooms branch
      and the type filter no longer use `take(limit*2)`, so filters cannot
      silently drop valid events (regression test added). One small
      `InternedStr` allocation per queried room remains.
- [x] **`PoolEntry` clones are allocation-free.** `id` is now an interned
      `Arc<str>`; every field is an `Arc`, so returning owned entries from
      `select`/`get_room_channel*` costs only refcount bumps.
- [x] **HTTP header parse is buffered** (`BufReader` + `read_until`), not one
      syscall per byte.
- [ ] **WS frame copy:** tungstenite 0.28 exposes `Message::Text` as
      immutable `Utf8Bytes`; simd-json needs a mutable slice, so one copy
      remains. Only HTTP can pass its body buffer directly.
- [x] **`parse_envelope_ref` avoids deep-cloning offer values** in
      pool.exchange. The `maybe_commit` `env.clone()` remains (needed for
      the wrap-failure fallback).
- [x] **`MAX_OFFERS` (256) caps pool.exchange verification work.**
- [x] **`hkdf_sha256` uses stack buffers** instead of a Vec per block.
- [x] **Benchmark integrity:** `verify_chain` no longer skips depth-1
      verification on a cached layer (the depth-1 bench was measuring a
      no-op); `get_by_id` now benches the protocol's 100-id system.fetch cap.

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
