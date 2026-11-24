#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
node scripts/fonts.mjs
cargo build --locked --release --target wasm32-unknown-unknown --no-default-features --features wasm
pkg=packages/skyshot
bindgen="${WASM_BINDGEN:-wasm-bindgen}"
"$bindgen" --target web --out-dir "$pkg/generated" --no-typescript "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/skyshot.wasm"
mkdir -p "$pkg/fonts"
cp fonts/fallback/* "$pkg/fonts/"
cp fonts/OFL.txt "$pkg/fonts/OFL-Inter.txt"
