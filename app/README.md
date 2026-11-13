# toloo-app

The Toloo application for day-to-day usage: a Slint UI over `toloo-lib` in one
Rust process. No WebView, no JavaScript runtime, no FFI layer. The normative
protocol specification is in [`../specs/`](../specs/).

## Run

```sh
# Linux build/runtime libraries for the winit backend
sudo apt-get install -y libxkbcommon-dev libfontconfig1-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libgl1-mesa-dev

cargo run -p toloo-app
# CPU renderer:
SLINT_BACKEND=winit-software cargo run -p toloo-app
```

Data lives in `$TOLOO_APP_DATA_DIR` (the old `$TOLOO_NATIVE_DATA_DIR` is still
accepted), or the platform data directory (`~/.local/share/toloo-app`,
`%APPDATA%/toloo-app`, `~/Library/Application Support/toloo-app`). An existing
`toloo-native` data directory is migrated on first launch. It contains
`config.vault.json`, `pool.jsonl`, and pool side files. The vault encrypts the
identity and room keys under a passphrase; a legacy plaintext `config.json` is
migrated on first launch.

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
cargo test -p toloo-app
# Optional live relay transport test:
TOLOO_TEST_RELAY=ws://127.0.0.1:17701 cargo test -p toloo-app syncs_via_configured_relay
TOLOO_TEST_RELAY=http://127.0.0.1:17702/toloo cargo test -p toloo-app syncs_via_configured_relay
```

## Licensing

Slint is available under GPL-3.0-only, a royalty-free option, or a commercial
license. Confirm which applies to Toloo before distributing binaries. This is
independent of the protocol crate licenses.

## Android

Slint Android requires **API 26 minimum**, Skia/OpenGL ES (or Vulkan), Java 17,
and the Android SDK/NDK (CI uses NDK 27.0.12077973). Android packaging uses
Gradle (`app/android/`) with a Kotlin `NativeActivity` and AndroidX
SplashScreen. Signed release APKs are built by `.github/workflows/release.yml`
and the `App` workflow (`.github/workflows/native.yml`). CI signs with the
project's release keystore from the `ANDROID_KEYSTORE_BASE64` and
`ANDROID_KEYSTORE_PASSWORD` Actions secrets:

```sh
# Download the versioned APK from the App workflow or GitHub Release, then:
adb install -r toloo-app-<version>-android-arm64-v8a.apk     # modern phones
adb install -r toloo-app-<version>-android-armeabi-v7a.apk   # 32-bit ARM devices
# Universal fallback (both ABIs, slower install):
adb install -r toloo-app-<version>-android-universal.apk
```

Release APKs have been installed and exercised on physical Android 8+ devices
as part of release validation. Continue checking current builds on API
26-class hardware, including upgrades and this app's IME/touch behavior. Slint
has no documented Android software renderer, which is why the
`SLINT_BACKEND=software` plan is not applicable here. The Kotlin activity holds
Android's native SplashScreen until Rust reports the first engine snapshot,
with a 1.5-second safety release. Prefer the per-ABI APKs for the shortest
install.
