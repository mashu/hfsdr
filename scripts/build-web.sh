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
  --bin hfsdr \
  --no-default-features \
  --features gui-web \
  --target wasm32-unknown-unknown

wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir web/pkg \
  --out-name hfsdr \
  target/wasm32-unknown-unknown/release/hfsdr.wasm

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

# Point index.html at this exact build.
#
# GitHub Pages caches assets, so without a changing URL a returning visitor
# keeps the module they already have and a fresh deploy appears to have changed
# nothing. Substitution is idempotent: it rewrites whatever version is there.
build_id=$(sha256sum web/pkg/hfsdr_bg.wasm | cut -c1-12)
sed -i -E "s|(\./pkg/hfsdr\.js\?v=)[^\"]*|\1${build_id}|" web/index.html
echo "build id: ${build_id}"

# Bake the public receiver list into the deployment.
#
# The browser cannot be relied on to fetch it live: the directory is a
# third-party host, and whether it sends CORS headers is not ours to control.
# CI has ordinary network access, so fetch it here and serve it from our own
# origin, where neither CORS nor mixed content applies. The app still tries the
# live URL when the user asks to refresh; this is what makes the list work on
# first load.
#
# A failure here is not fatal — the app falls back to the live fetch and says so
# — because a directory outage must not break the build.
LIST_URL="https://rx.linkfanel.net/kiwisdr_com.js"
echo "fetching receiver list from $LIST_URL"
if curl -fsS --max-time 60 "$LIST_URL" -o web/receivers.js.tmp; then
  mv web/receivers.js.tmp web/receivers.js
  echo "receiver list: $(wc -c < web/receivers.js) bytes"
else
  rm -f web/receivers.js.tmp
  echo "WARNING: could not fetch the receiver list; the app will try the live URL instead"
fi

ls -la web/pkg/
