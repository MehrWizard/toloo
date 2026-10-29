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

- [ ] **One canonicalization per envelope.** Currently ~5–7 per stored
      message: parse validation (envelope.rs:338, 341), verify
      (envelope.rs:45-53), `datum_id` (ids.rs:50), and `Pool::put`
      (pool.rs:782-783, 1870). Compute canonical bytes once and thread them
      through verify → id → put.
- [ ] **Canonical serializer without `serde_json::Value`.** Implement a
      `Serializer` that writes canonical JSON bytes directly from `DatumBody`
      (including `#[serde(flatten)] extra`), eliminating the `to_value`
      tree allocation in `signable_bytes_verify`.
- [ ] **`signable_bytes_verify` clones its thread-local buffer**
      (envelope.rs:48) — the allocation it was meant to avoid. Pass an
      `&mut Vec` through the call chain or return the buffer to a pool.
- [ ] **Persistence: one open/write/close per envelope.** `append_event`
      reopens the log for every event (pool.rs:637-650), and `put_batch`
      calls it per envelope (pool.rs:939-941). Keep a mutex-guarded
      `BufWriter<File>`, write a whole batch in one call, add optional
      group fsync.
- [ ] **Spill check on every `put`.** O(n) count + global `spill_lock` per
      insert (pool.rs:484-505). Track a live-count atomic and only spill
      when it crosses the threshold; move the rewrite to a blocking task.
- [ ] **Blocking disk I/O on the LocalSet.** `append_event`, `persist_meta`,
      blob reads/writes use `std::fs` on the executor thread (pool.rs:637,
      :400, :1397). Move persistence to a writer thread with a channel.
- [ ] **`build_membership` replayed per request** — clones every channel -2
      entry (pool.rs:1064, called from handlers.rs:463, 570). Cache keyed by
      `(room, latest_tc)` or maintain incrementally.
- [ ] **Rate-limit check is O(n) per message** — full-pool `select()` per
      auto-commit with rules (handlers.rs:621-637) → O(n²). Maintain
      per-(room, author) hourly counters.
- [ ] **`system.fetch` full scan** instead of `by_id` lookups
      (pool.rs:1276-1284). `latest_tc` traverses the whole BTreeMap; use
      `range((room, 0)..=(room, u64::MAX)).next_back()` (pool.rs:986).
- [ ] **`get_wanted` allocates an `InternedStr` per room per call**
      (pool.rs:1168); the all-rooms branch has no per-room upper bound and
      `take(limit*2)` can silently drop results (pool.rs:1149).
- [ ] **Deep `PoolEntry` clones per returned row** (`select`,
      `get_room_channel*`, pool.rs:375, 972, 1158, 1266). Return
      `Arc<PoolEntry>` or borrowed refs; `id: String` → `InternedStr`.
- [ ] **HTTP header parse reads one byte per syscall** (server.rs:321-335).
      Use `read_until(b'\n')`/`BufReader`.
- [ ] **WS frame copy:** `json_str.as_bytes().to_vec()` in
      `dispatch_common` (server.rs:430) — take `text.into_bytes()` instead.
- [ ] **`maybe_commit` deep-clones the envelope** right before consuming it
      (`wrap_commit(env.clone(), …)`, handlers.rs:534). Same in
      `parse_envelope(item.clone())` (handlers.rs:186).
- [ ] **Missing `MAX_OFFERS` cap** in pool.exchange — unbounded verification
      work per request (DoS). Cap the offer array (e.g. 256) like
      `MAX_FETCH_IDS`.
- [ ] **`hkdf_sha256` allocates per block** (crypto.rs:177-204); HMAC-SHA256
      output is fixed 32 bytes — use stack arrays.

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
