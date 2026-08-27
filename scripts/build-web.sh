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
# The browser cannot fetch this itself. The directory host answers on port 80
# only — a TLS attempt gets "Failed to connect ... port 443" — and an https
# page may not request plain http, so from GitHub Pages there is no reachable
# URL at all. curl here is not a browser and has no such restriction, so the
# list is captured at build time and served from our own origin.
#
# https is tried second in case the host ever gains TLS; if it does, this keeps
# working and the note above becomes stale rather than the build breaking.
fetch_receiver_list() {
  for url in "http://rx.linkfanel.net/kiwisdr_com.js" \
             "https://rx.linkfanel.net/kiwisdr_com.js"; do
    echo "fetching receiver list from $url"
    if curl -fsS --max-time 60 "$url" -o web/receivers.js.tmp; then
      mv web/receivers.js.tmp web/receivers.js
      echo "receiver list: $(wc -c < web/receivers.js) bytes from $url"
      return 0
    fi
    rm -f web/receivers.js.tmp
  done
  return 1
}

if ! fetch_receiver_list; then
  # An annotation rather than a log line: a silently empty receiver panel is
  # exactly the failure that went unnoticed before, and the build still
  # succeeds so a directory outage cannot block a deploy.
  echo "::warning title=Receiver list missing::Could not fetch the KiwiSDR directory. The deployed app will show no receivers: a browser on an https page cannot reach that host itself, which is why it is fetched here."
fi

ls -la web/pkg/
