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

## Notes

- `Trust/attestations`, `room.flag`, and blob pieces are implemented stubs,
  intentionally kept for future work; their designs are postponed.
