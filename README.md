# toloo

Rust implementation of the [Toloo Protocol v0.2](../toloo-docs/).
Targets **Relay conformance** — the highest level defined in
[`D-conformance.md`](../toloo-docs/D-conformance.md) §D.6.

```
toloo/      ← This: Rust relay + Slint native app + wasm browser core   ← you are here
toloo-docs/ ← Protocol specification (the contract)
toloo-js/   ← Legacy browser JS client (being replaced by toloo-wasm)
```

---

## What this is

`toloo` is the reference implementation of the Toloo messaging protocol.
It contains everything needed to run a relay, host rooms, and exchange messages
— as a CLI daemon, as a library, in the native Slint client, or in the browser
client.

Browser support is the same `toloo-core` compiled to WebAssembly (`toloo-wasm`),
so there is one protocol implementation for native and web. The older
hand-written [`toloo-js`](../toloo-js/) client is legacy and is being retired.

---

## Project layout

```
toloo-core/           Portable protocol core — no I/O, native + wasm
  src/
    types.rs          All wire types: Envelope, DatumBody, LocalNode, LocalRoom,
                        EndpointDescriptor, …
    base64url.rs      RFC 4648 §5 encode/decode
    canonical.rs      RFC 8785 canonical JSON (for signing + datum_id)
    crypto.rs         Ed25519 sign/verify, X25519 ECDH, SHA-256, HKDF,
                        ChaCha20-Poly1305 AEAD
    envelope.rs       sign_envelope, verify_chain, maybe_commit, wrap_ack
    ids.rs            eid, datum_id, channel_of
    events.rs         typed event builders + content field validators
    pow.rs            Proof-of-Work compute + verify
    private.rs        private.message v2 E2E (static-static X25519, cached pair keys)
    rules.rs          room rule set parsing and evaluation
    vectors.rs        deterministic test vector computation [REF]
    discovery.rs      toloo: URI and .toloo file encode/decode
    fixtures.rs       deterministic identities/events (test/fixtures feature)

toloo-lib/            Native stateful runtime — depends on toloo-core
  src/
    pool.rs           Append-only event log + in-memory indexes; membership
                        reconstruction; TOFU pins, blocklist, blobs, pruning
    transport/
      server.rs       WebSocket + HTTP accept loops (tokio LocalSet); plain
                        and TLS endpoints
      tls.rs          rustls helpers (self-signed cert generation)
      backoff.rs      exponential retry with jitter
    requests/
      handlers.rs     All nine endpoint handlers: events.sync, room.sync,
                        room.compare, pool.exchange, node.peers, node.rooms,
                        system.fetch, blob.fetch, room.summary;
                        room authority + auto-commit
    trust.rs          TOFU trust levels, side attestations
    keystore.rs       Argon2id + AEAD encrypted-at-rest key wrapping
  tests/
    vectors.rs        cross-implementation test vector assertions

toloo-wasm/           Browser bindings — same core compiled to wasm32
  src/
    contract.rs       JSON-in/JSON-out contract: identities, all event builders,
                        request builders, chain verification/batching, private
                        messages + pair-key cache, membership, room rules,
                        discovery URIs/files, PoW, canonical JSON, hashing
    bindings.rs       Thin wasm-bindgen exports
  tests/
    wasm.rs           wasm-bindgen-test suite (Node)
  build.sh            Release build + generated JS/TS package in pkg/

toloo-web/            SolidJS + TypeScript browser client
  src/lib/
    wasm.ts           Typed bridge to the generated wasm package
    transport.ts      WebSocket/HTTP adapters + page-scheme policy
    db.ts             IndexedDB identity/relay/room/envelope stores
    client.ts         Sync, verification, membership, room authority
  src/components/     Onboarding, sidebar, chat, settings, modals
  scripts/            Builds and copies toloo-wasm/pkg before Vite

toloo-native/         Native Slint client over toloo-lib — no WebView
  ui/app.slint        Declarative UI (identity, relays, rooms, chat)
  src/engine.rs       Engine thread: native transports, pool, room authority
  src/lib.rs          Slint bridge and snapshot polling
  src/main.rs         Desktop entry point
  build.rs            Slint UI compiler

toloo-cli/            Binary — headless relay and scripting
  src/
    main.rs           keygen, relay, create-room, import, send, sync, vectors

```

Dependency chain:
```
toloo-core  ←  toloo-lib  ←  toloo-cli
                         ←  toloo-native (Slint, direct Rust)
            ←  toloo-wasm  ←  toloo-web (wasm core + JS transport/store/UI)
```

---

## Platform support

### Native client (Slint)

No WebView and no JavaScript runtime. The UI is Slint; the protocol and
transport engine is `toloo-lib` in the same process.

| Platform | Minimum version | Notes |
|---|---|---|
| Linux x86_64 | glibc 2.17+ | `toloo-native` binary; X11/Wayland + OpenGL or software renderer |
| Linux aarch64 | glibc 2.17+ | Raspberry Pi 4/5, Ampere, AWS Graviton |
| Windows x86_64 | Windows 10 | `toloo-native.exe` |
| macOS arm64 | macOS 11+ | Slint winit backend |
| Android | **API 26 / Android 8.0+** | Rust + Slint + Skia/OpenGL ES; arm64-v8a and armeabi-v7a; experimental APK workflow |
| iOS | not yet shipped | Slint iOS support is version-dependent; verify before planning |

The old Tauri/WebView app has been removed. Android 7 / API 24–25 devices are
not supported by the Slint backend; they require a separate native-UI path.

### CLI (`toloo-cli`)

The CLI has no UI dependencies — pure Rust, zero system libraries. It builds and runs
natively on Linux, Windows, and macOS with no system libraries required.

| Platform | Minimum | Binary | Notes |
|---|---|---|---|
| Linux x86_64 | Kernel 2.6.39 (Ubuntu 12.04+) | static musl | Zero system deps — copy and run on any distro |
| Linux aarch64 | Kernel 3.x+ | static musl | Raspberry Pi, Ampere, AWS Graviton — cross-compiled with zig |
| Windows x86_64 | Windows 10 | `.exe` (static CRT) | No MSVC redistributable required |
| Windows aarch64 | Windows 10 on ARM | `.exe` (static CRT) | Surface / Snapdragon |
| macOS x86_64 | 10.15 Catalina | binary | No Xcode needed — pure Rust, built on `macos-latest` |
| macOS aarch64 | 11.0 Big Sur | binary | Apple Silicon, no Xcode needed |

---

## Building

### Prerequisites

All platforms need [Rust stable](https://rustup.rs). The native Slint client
additionally needs the platform windowing/OpenGL development libraries listed
below.

### Library and CLI (any platform)

```sh
cargo build                    # dev build (all crates)
cargo build --release          # optimized
cargo test                     # all unit + integration tests
```

Browser package (`wasm32`):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126 --locked

cargo test -p toloo-wasm --target wasm32-unknown-unknown   # wasm test suite (Node)
./toloo-wasm/build.sh                                      # → toloo-wasm/pkg/ (JS + .d.ts + .wasm)
```

`toloo-wasm` has no native I/O or TLS: the browser remains a client, and AWS-LC
is native-only, so wasm uses the dalek/RustCrypto path. WebAssembly is loaded
from the generated `pkg/` by the web UI; hand-written protocol JavaScript is
retired.

Web client (`toloo-web/`):

```sh
cd toloo-web
npm install
npm run dev       # wasm build + Vite dev server on :5173
npm test          # transport policy and HTTP transport tests
npm run build     # production bundle in dist/
```

See [`toloo-web/README.md`](./toloo-web/README.md) for the local-relay workflow
and current feature scope.

CLI — platform-specific static builds:

```sh
# Linux x86_64: fully static musl binary (runs on any Linux, kernel 2.6.39+)
rustup target add x86_64-unknown-linux-musl
sudo apt-get install -y musl-tools
cargo build -p toloo-cli --release --target x86_64-unknown-linux-musl

# Linux aarch64: static musl, cross-compiled with zig (Raspberry Pi, Graviton, etc.)
rustup target add aarch64-unknown-linux-musl
pip3 install ziglang cargo-zigbuild
cargo zigbuild -p toloo-cli --release --target aarch64-unknown-linux-musl

# Windows: static CRT — .exe runs on any Win10+ with no redistributables
# (run this from a Windows machine or windows-latest CI runner)
cargo build -p toloo-cli --release --target x86_64-pc-windows-msvc
# aarch64:
rustup target add aarch64-pc-windows-msvc
cargo build -p toloo-cli --release --target aarch64-pc-windows-msvc
# Set RUSTFLAGS for static CRT linkage:
set RUSTFLAGS=-C target-feature=+crt-static
```

### Native client — Linux

```sh
# Debian/Ubuntu build and runtime libraries for Slint's winit backend
sudo apt-get install -y libxkbcommon-dev libfontconfig1-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libgl1-mesa-dev

cargo run -p toloo-native
cargo build -p toloo-native --release
```

Use `SLINT_BACKEND=winit-software cargo run -p toloo-native` for the CPU
renderer on systems without usable OpenGL.

### Native client — Windows / macOS

```sh
cargo build -p toloo-native --release
# Windows: target/release/toloo-native.exe
# macOS:   target/release/toloo-native
```

No WebView, WebView2, or JavaScript runtime is required.

### Android (Slint, API 26+)

Current Slint Android support is Rust-only and requires **API 26 minimum**.
It cannot target Android 7 / API 24–25.

```sh
# Android SDK + NDK, Java 17, and Rust targets
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
rustup target add aarch64-linux-android armv7-linux-androideabi

# Gradle + cargo-ndk packaging (Kotlin NativeActivity + AndroidX splash)
cargo install cargo-ndk --version 4.1.2 --locked
./toloo-native/android/build-native.sh
(cd toloo-native/android && gradle assembleRelease)
```

Set `ANDROID_KEYSTORE_PATH`, `ANDROID_KEYSTORE_PASSWORD`, and
`ANDROID_KEYSTORE_ALIAS` to sign release APKs.

The `Native` GitHub workflow builds both ABIs. It is marked experimental until
an APK has been validated on a real API 26 device. Slint uses Skia with
OpenGL ES or Vulkan on Android; there is no documented software-renderer
fallback on that platform.

### CLI

```sh
# Generate a node identity keypair
cargo run -p toloo-cli -- keygen

# Start a relay (default: all interfaces, port 17700)
cargo run -p toloo-cli -- relay

# Create a room and print its toloo:// invite URI
cargo run -p toloo-cli -- create-room --name "My Room"

# Decode any toloo:// URI or .toloo file (human-readable)
cargo run -p toloo-cli -- import <uri-or-file>

# Sign and submit a message to a relay
cargo run -p toloo-cli -- send --room <pub> --relay <ws-url>

# Fetch and display messages; optionally save to .toloo bundle
cargo run -p toloo-cli -- sync --relay <ws-url> --room <pub>

# Print deterministic test vectors for cross-implementation validation
cargo run -p toloo-cli -- vectors
```

### CI / automated releases

Pushing a tag (`v*`) triggers `.github/workflows/release.yml`, which builds all
targets in parallel and creates a GitHub Release:

| Job | Runner | Output |
|---|---|---|
| `build-native (linux-x86_64)` | ubuntu-latest | `toloo-native` |
| `build-native (linux-aarch64)` | ubuntu-24.04-arm | `toloo-native` |
| `build-native (windows-x86_64)` | windows-latest | `toloo-native.exe` |
| `build-native (macos-aarch64)` | macos-latest | `toloo-native` |
| `build-cli (cli-linux-x86_64)` | ubuntu-latest | static musl binary |
| `build-cli (cli-linux-aarch64)` | ubuntu-latest | static musl binary (aarch64, zig cross) |
| `build-cli (cli-windows-x86_64)` | windows-latest | `.exe` (static CRT) |
| `build-cli (cli-windows-aarch64)` | windows-latest | `.exe` aarch64 (static CRT) |
| `build-cli (cli-macos-x86_64)` | macos-latest | macOS binary (x86_64) |
| `build-cli (cli-macos-aarch64)` | macos-latest | macOS binary (Apple Silicon) |

Android APKs are built by `.github/workflows/native.yml` (API 26+, arm64 and
armv7) and are currently experimental.

### CI / Benchmarks

`.github/workflows/benchmark.yml` runs microbenchmarks across 5 platforms
(linux-x86, linux-arm, windows, macos-x86, macos-arm), plus a Linux x86
cross-protocol job, and publishes results to
[`MehrWizard/toloo-benchmarks`](https://github.com/MehrWizard/toloo-benchmarks)
with a live static HTML dashboard. Full details in [`THROUGHPUT_AUDIT.md`](./THROUGHPUT_AUDIT.md).

```sh
# Run benchmarks locally (5 minutes, 20 iterations)
cd toloo-lib && cargo bench --bench canonical_bench

# Cross-protocol benchmarks (Signal, Matrix, Nostr)
cargo bench -p toloo-protocol-bench --features protocols --bench protocols
```

GitHub-hosted runner CPUs vary between runs, so absolute cross-run deltas can
be misleading. For trusted regression checks, pass `baseline_ref` to measure a
baseline and the dispatched branch back-to-back on the same linux-x86 runner:

```sh
gh workflow run benchmark.yml \
  -f repo=MehrWizard/toloo-benchmarks \
  -f branch=main \
  -f baseline_ref=<sha-or-tag>
```

The A/B table is written to the job summary and uploaded as
`bench-ab-linux-x86`; other platforms and normal runs are unchanged.

---

### Throughput vs IRC/XMPP/Matrix

Measured on CI (commit 6d8c313, linux-x86):

| Category | Toloo (ns) | IRC (ns) | Ratio |
|----------|-----------|----------|-------|
| ed25519 verify (single-sig) | 35,134 | 45,000 | 1.28x slower |
| ed25519 verify (batch-10, per-sig) | 18,791 | 45,000 | **2.39x faster** ✅ |
| Envelope verify (depth-1) | 35,972 | 50,000 | 1.39x slower |
| Canonical encode (medium) | 1,596 | 150,000 | 94x faster ✅ |

**2x is achievable with batch verification** on all five platforms in this run:
linux-x86 2.39x, linux-arm 2.09x, windows 2.81x, macOS-x86 2.56x, macOS-arm
2.54x. Single-signature verification does not reach 2x on any platform.
AWS-LC Ed25519 signing also cut event-build cost roughly in half. GitHub-runner
variance is significant between runs (absolute numbers can move 20–50%), so use
same-run ratios; the dashboard keeps the per-commit history.

### Cross-protocol benchmarks (Signal, Matrix, Nostr)

`toloo-protocol-bench/` is a benchmark-only crate (not published; heavy deps sit
behind the non-default `protocols` feature). It measures the same ~2 KB message
against real protocol implementations:

| Protocol | Library | Operations |
|---|---|---|
| Signal | `libsignal-protocol-syft` 0.85.3-beta.5 (X3DH, Double Ratchet, Kyber) | protobuf parse, ratchet encrypt/decrypt, session setup |
| Matrix | `ruma-signatures` 0.22 (canonical JSON + ed25519), `vodozemac` 0.11 (Olm) | canonical encode/parse, sign/verify, Olm encrypt/decrypt/session setup |
| Nostr | `nostr` 0.45 (secp256k1 Schnorr, NIP-44 v2) | event build/parse/verify, NIP-44 encrypt/decrypt |
| Toloo | `toloo-lib` | private-message v2 encrypt/decrypt (static-static X25519 via `aws-lc-rs` with dalek fallback, cached pair key + per-message salt) |

Measured rows feed the dashboard's latency cards and a 1:1 private-message card;
rows tagged `est` remain reference estimates. Key generation and session setup
are untimed (`iter_batched`); Signal/Olm decrypt figures are steady-state ratchet
operations, while Toloo/Nostr use a cached pairwise conversation key. The 1:1
card shows a first-message line combining pair/session setup with responder
processing, so setup amortization is visible. Cross-protocol numbers run on
Linux x86 only.

Toloo private messages use a v2 payload: sender X25519 public key, a random
32-byte per-message salt, and the ciphertext. The pair key is one X25519
computation per peer and can be cached; v1 ephemeral-key messages remain
decryptable. This is stateless and asynchronous, but it provides **no forward
secrecy**: a leaked static encryption key decrypts stored messages.

With the AWS-LC backend, the 1:1 card shows Toloo ahead of Nostr on pair setup,
encrypt, decrypt, and round-trip latency. The remaining gap in public-envelope
cost is Ed25519 single verification and canonicalization, not private messages.

```sh
cargo test -p toloo-protocol-bench --features protocols   # round-trip smoke tests
```

Notes: libsignal serializes protobuf at message construction, so only parsing is
measured; Signal has no per-message signatures, so it is absent from the verify
and build cards. The `protocols` feature pulls in `libsignal-protocol-syft`
(AGPL-3.0-only). The crate is `publish = false`, has an empty default feature
set, and is never linked into relay or app artifacts — no combined binaries are
distributed. Details and fairness caveats:
[`THROUGHPUT_AUDIT.md`](./THROUGHPUT_AUDIT.md#phase-9-cross-protocol-benchmarks-2026-10-31).

---

## Execution model

### Native (Slint)

The native client is one Rust process. Slint renders the UI; a dedicated engine
thread owns `toloo-lib`'s pool, Tokio runtime, and native WebSocket/HTTP
transports. The UI sends commands over a channel and applies snapshots on the
Slint event-loop thread.

```
┌──────────────────────────────┐        commands        ┌──────────────────┐
│  Slint UI                    │ ─────────────────────► │  engine thread   │
│  ui/app.slint                │ ◄───────────────────── │  toloo-lib       │
│  (no WebView / JS runtime)   │       snapshots        │  pool + tokio    │
└──────────────────────────────┘                        └──────────────────┘
```

The native engine can later host a relay in-process; the browser client cannot.
A browser node connects to an external relay, while the native app can use raw
transports, self-signed pinning, and OS storage.

### Browser (wasm)

When running outside the native client — in a browser or a web app — protocol logic is the
same `toloo-core` compiled to WebAssembly by `toloo-wasm`. `toloo-web/` is the
SolidJS client: it loads the generated wasm package, owns WebSocket/HTTP
transport and IndexedDB persistence, and keeps all signing, verification,
private-message, membership, and room-rule logic in wasm. A browser cannot host
a network relay, so this mode connects to an external relay and remains a pure
client; rooms whose key lives in the browser are committed by that tab while it
is open.

```
┌────────────────────┐       wss:// or https://      ┌──────────────┐
│  Browser           │ ──────────────────────────►   │  toloo       │
│  SolidJS UI        │           TLS                 │  relay       │
│  toloo-wasm (core) │ ◄──────────────────────────   │  (Rust)      │
└────────────────────┘                               └──────────────┘
```

The wasm build uses the dalek/RustCrypto crypto path (AWS-LC is not available
on wasm) and `getrandom`'s browser/Node backend. Native builds keep AWS-LC.

**Browser transport policy:** an `https://` chat page accepts only `https://`
and `wss://` relay endpoints; an `http://` page accepts only `http://` and
`ws://`. The hosted chat is therefore CA-TLS dependent for secure endpoints:
self-signed relay certificates and certificate fingerprints are for the native
client (and future pinned WebTransport), not for browser `wss://`. Browsers do
allow `https/wss` from an `http` page, but the web client ignores them to keep
one strict scheme pairing per origin. Enforce the pairing in the SolidJS
transport and match it with CSP (`connect-src`).

---

## Transport endpoints

A relay can expose any combination of these endpoints (configured at startup):

| Proto | Browser | Description |
|-------|---------|-------------|
| `ws`  | Yes | WebSocket, no TLS (trusted networks) |
| `wss` | Yes | WebSocket + TLS |
| `http` | Yes | HTTP POST (request/response) |
| `https` | Yes | HTTP POST + TLS |

Transport confidentiality comes from TLS on `wss`/`https` (self-signed certs
with TOFU pinning are supported). Connections MAY be routed through a SOCKS5
proxy (client-side configuration; no protocol support required).

---

## Message lifecycle (3-depth envelope model)

Every room message goes through three envelope depths:

```
depth 1  ─── sender signs ───────────────────────►  relay receives
depth 2  ─── room key holder commits (d.t="commit", d.tc set) ──►  "✓ Sent"
depth 3  ─── sender wraps commit (d.t="ack") ────►  terminal; relays stop forwarding
```

UI states in the app:
- `⋯ Pending…` — trying to reach a relay
- `⏱ Waiting`  — depth-1 delivered, waiting for commit
- `✓ Sent`     — depth-2 commit received (room key holder signed)
- `✗ Failed`   — TTL expired with no commit

The commit can happen directly (relay holds the room key) or via a relay chain
(intermediate relays sign and forward until a key-holder commits).

---

## Spec reference

| File | Covers |
|------|--------|
| [`1-concepts.md`](../toloo-docs/1-concepts.md) | Mental model, terminology |
| [`2-data-model.md`](../toloo-docs/2-data-model.md) | Envelope format, canonical JSON, eid, datum_id |
| [`3-cryptography.md`](../toloo-docs/3-cryptography.md) | All algorithms and parameters |
| [`4-identity.md`](../toloo-docs/4-identity.md) | Node keypairs, node.meta, toloo: URI |
| [`5-rooms.md`](../toloo-docs/5-rooms.md) | Room keypairs, channels, membership, rules |
| [`6-events.md`](../toloo-docs/6-events.md) | All event types, content schemas, lifecycle |
| [`7-replication.md`](../toloo-docs/7-replication.md) | Pool model, commit flow, bisection sync |
| [`9-transport.md`](../toloo-docs/9-transport.md) | Carrier model, endpoint descriptors, message framing |
| [`10-endpoints.md`](../toloo-docs/10-endpoints.md) | All nine endpoint definitions |
| [`11-storage.md`](../toloo-docs/11-storage.md) | Retention, pruning, archival |
| [`12-discovery.md`](../toloo-docs/12-discovery.md) | Bootstrap, gossip, URI import |
| [`13-moderation.md`](../toloo-docs/13-moderation.md) | Local blocking, spam, PoW enforcement |
| [`A-registries.md`](../toloo-docs/A-registries.md) | Event types, field schemas, error codes, constants |
| [`B-wire-examples.md`](../toloo-docs/B-wire-examples.md) | Annotated wire traces |
| [`C-security-considerations.md`](../toloo-docs/C-security-considerations.md) | Threat model, crypto assumptions |
| [`D-conformance.md`](../toloo-docs/D-conformance.md) | MUST/SHOULD/MAY requirements, validation checklist |
| [`E-test-vectors.md`](../toloo-docs/E-test-vectors.md) | Deterministic test inputs and expected outputs |

---

## Mobile

Mobile uses the native Slint app: Android APKs are built in the `Native`
GitHub workflow with Gradle + cargo-ndk for arm64 and armv7. Slint requires
Android API 26+, so API 24–25 devices are out of scope for this UI. The Android
build is experimental until validated on a real device. iOS is not shipped; Slint
iOS support is version-dependent and requires a macOS/Xcode build.
