#!/usr/bin/env sh
# Build the browser package consumed by the SolidJS UI.
# Requires: wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.126.
set -eu

required_bindgen=0.2.126
have_bindgen=$(wasm-bindgen --version | awk '{print $2}')
if [ "$have_bindgen" != "$required_bindgen" ]; then
    echo "wasm-bindgen $required_bindgen required (found ${have_bindgen:-none})" >&2
    echo "cargo install wasm-bindgen-cli --version $required_bindgen --locked" >&2
    exit 1
fi

crate_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_dir=$(dirname -- "$crate_dir")
target_dir="${CARGO_TARGET_DIR:-$workspace_dir/target}"
out_dir="$crate_dir/pkg"

cd "$workspace_dir"
cargo build -p toloo-wasm --release --target wasm32-unknown-unknown

rm -rf "$out_dir"
wasm-bindgen \
    --target web \
    --out-dir "$out_dir" \
    --out-name toloo_wasm \
    "$target_dir/wasm32-unknown-unknown/release/toloo_wasm.wasm"

if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz \
        "$out_dir/toloo_wasm_bg.wasm" \
        -o "$out_dir/toloo_wasm_bg.wasm"
fi

cat > "$out_dir/package.json" <<'JSON'
{
  "name": "toloo-wasm",
  "version": "0.1.0",
  "type": "module",
  "module": "toloo_wasm.js",
  "types": "toloo_wasm.d.ts"
}
JSON

echo "wasm package: $out_dir"
