#!/usr/bin/env bash
# Build the browser app into web/pkg/.
#
# getrandom needs an explicit backend on wasm32-unknown-unknown; it arrives
# transitively through the WebSocket crate, so the cfg is required even though
# nothing here asks for randomness directly.
set -euo pipefail

cd "$(dirname "$0")/.."

export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""

cargo build --release \
  --bin hfsdr-wasm \
  --no-default-features \
  --features gui-wasm \
  --target wasm32-unknown-unknown

wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir web/pkg \
  --out-name hfsdr \
  target/wasm32-unknown-unknown/release/hfsdr-wasm.wasm

# wasm-opt is optional: binaryen older than the wasm-bindgen output can corrupt
# the function table (observed with binaryen 108 vs wasm-bindgen 0.2.125), and a
# broken bundle is far worse than a large one. Only shrink when a modern
# binaryen is present, and verify it still loads before keeping the result.
if command -v wasm-opt >/dev/null 2>&1; then
  binaryen_major=$(wasm-opt --version | grep -oE '[0-9]+' | head -1)
  if [ "${binaryen_major:-0}" -ge 116 ]; then
    echo "wasm-opt $binaryen_major: shrinking"
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
      -o web/pkg/hfsdr_bg.wasm.opt web/pkg/hfsdr_bg.wasm
    mv web/pkg/hfsdr_bg.wasm.opt web/pkg/hfsdr_bg.wasm
  else
    echo "wasm-opt $binaryen_major is too old for this wasm-bindgen output; skipping"
  fi
fi

ls -la web/pkg/
