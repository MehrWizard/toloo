# bench

Benchmark-only crate and tooling. Nothing here is linked into the relay or
client binaries.

- `Cargo.toml` — package `toloo-bench` (`publish = false`)
- `benches/canonical_bench.rs` — Toloo Criterion benchmarks for canonical JSON,
  envelope parsing/verification/serialization, event construction, crypto,
  and pool operations
- `benches/protocols.rs` — cross-protocol comparisons using Signal
  (`libsignal`), Matrix (`ruma` + `vodozemac`), and Nostr
- `src/` — shared payload helpers and cross-protocol adapters
- `scripts/` — local aggregation, dashboard substitution, and OG-image helpers
- `templates/index.html.template` — dashboard template
- `assets/benchmark-og.png` — social preview asset

## Run

```sh
# Core/runtime benchmarks
cargo bench -p toloo-bench --bench canonical_bench

# Cross-protocol comparisons (Linux x86_64; optional AGPL dependencies)
cargo bench -p toloo-bench --features protocols --bench protocols

# Local JSON snapshot from Criterion output (write anywhere except this repo)
python3 bench/scripts/generate_benchmarks.py /tmp/toloo-latest.json
```

The heavy cross-protocol dependencies are behind the non-default `protocols`
feature. Results are never stored in this repository: CI publishes them, along
with the generated dashboard, to
[`MehrWizard/toloo-benchmarks`](https://github.com/MehrWizard/toloo-benchmarks).
`bench/scripts/substitute_template.py` is invoked by
`.github/workflows/benchmark.yml` against the downloaded benchmark report.
