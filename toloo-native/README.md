# toloo-native

Native Toloo client: a Slint UI over `toloo-lib` in one Rust process. No
WebView, no JavaScript runtime, no FFI layer.

## Run

```sh
# Linux build/runtime libraries for the winit backend
sudo apt-get install -y libxkbcommon-dev libfontconfig1-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libgl1-mesa-dev

cargo run -p toloo-native
# CPU renderer:
SLINT_BACKEND=winit-software cargo run -p toloo-native
```

Data lives in `$TOLOO_NATIVE_DATA_DIR`, or the platform data directory
(`~/.local/share/toloo-native`, `%APPDATA%/toloo-native`,
`~/Library/Application Support/toloo-native`). It contains `config.json`,
`pool.jsonl`, and pool side files. Identity private keys are currently plain
JSON; a keystore is still TODO.

## Architecture

- `ui/app.slint` — identity, relay, room, invite, and chat UI.
- `src/engine.rs` — owned engine thread with a Tokio current-thread runtime,
  `toloo-lib` pool/transports, relay polling, room commit authority, and
  snapshot generation.
- `src/lib.rs` — Slint bridge: commands out, snapshots in via a 10 Hz timer.
- `src/main.rs` — desktop entry point.
- `android_main` in `src/lib.rs` — Android entry point using
  `slint::android::init` and the app's internal data path.

The UI never handles transports or keys directly. The engine uses native
WebSocket/HTTP via `toloo-lib`; TLS currently uses the native `AcceptAny` mode
to support self-signed relays and still needs a per-relay pinning policy.

## Tests

```sh
cargo test -p toloo-native
# Optional live relay transport test:
TOLOO_TEST_RELAY=ws://127.0.0.1:17701 cargo test -p toloo-native syncs_via_configured_relay
TOLOO_TEST_RELAY=http://127.0.0.1:17702/toloo cargo test -p toloo-native syncs_via_configured_relay
```

## Licensing

Slint is available under GPL-3.0-only, a royalty-free option, or a commercial
license. Confirm which applies to Toloo before distributing binaries. This is
independent of the protocol crate licenses.

## Android

Slint Android requires **API 26 minimum**, Skia/OpenGL ES (or Vulkan), Java 17,
and the Android SDK/NDK (CI uses NDK 27.0.12077973). arm64 and armv7 targets
are configured in `Cargo.toml`; `.github/workflows/native.yml` builds an
experimental release-mode APK with cargo-apk. CI signs it with the project's
release keystore from the `ANDROID_KEYSTORE_BASE64` and
`ANDROID_KEYSTORE_PASSWORD` Actions secrets:

```sh
# Download the toloo-native-android artifact from the Native workflow, then:
adb install -r toloo-native-arm64-v8a.apk     # modern phones
adb install -r toloo-native-armeabi-v7a.apk   # 32-bit ARM devices
```

The APK has not yet been validated on a physical device. Slint has no
documented Android software renderer, which is why the `SLINT_BACKEND=software`
plan is not applicable here.
