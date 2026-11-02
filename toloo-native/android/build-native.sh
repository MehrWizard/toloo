#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$PROJECT_DIR/../.." && pwd)"
JNI_DIR="$PROJECT_DIR/app/src/main/jniLibs"
ABIS=("$@")
if [ "${#ABIS[@]}" -eq 0 ]; then
    ABIS=(arm64-v8a armeabi-v7a)
fi

: "${ANDROID_HOME:?set ANDROID_HOME}"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME}"

# Slint's build script compiles and embeds its Java helper.
export ANDROID_JAR="${ANDROID_JAR:-$ANDROID_HOME/platforms/android-35/android.jar}"
export ANDROID_BUILD_TOOLS_VERSION="${ANDROID_BUILD_TOOLS_VERSION:-35.0.0}"

rm -rf "$JNI_DIR"
mkdir -p "$JNI_DIR"

cargo ndk \
    --manifest-path "$REPO_ROOT/Cargo.toml" \
    "${ABIS[@]/#/--target=}" \
    --platform 26 \
    -o "$JNI_DIR" \
    build --release --locked -p toloo-native --lib
