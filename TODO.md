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
