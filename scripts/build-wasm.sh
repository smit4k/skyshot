#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
node scripts/fonts.mjs
features=wasm
out=packages/skyshot/generated
if [ "${SKYSHOT_VERIFY:-0}" = 1 ]; then
  features=wasm,verify
  out=target/chrome
fi
cargo build --locked --release --target wasm32-unknown-unknown --no-default-features --features "$features"
pkg=packages/skyshot
bindgen="${WASM_BINDGEN:-wasm-bindgen}"
"$bindgen" --target web --out-dir "$out" --no-typescript "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/skyshot.wasm"
mkdir -p "$pkg/fonts"
cp fonts/fallback/* "$pkg/fonts/"
cp fonts/OFL.txt "$pkg/fonts/OFL-Inter.txt"
