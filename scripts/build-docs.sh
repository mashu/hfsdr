#!/usr/bin/env bash
# Build mdBook + rustdoc for hfsdr.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if ! command -v mdbook >/dev/null 2>&1; then
  echo "error: mdbook not found. Install with: cargo install mdbook --locked" >&2
  exit 1
fi

echo "==> mdbook build"
mdbook build docs

features="airspy,gui"
if ! pkg-config --exists libairspyhf 2>/dev/null; then
  echo "==> libairspyhf not found; API docs will use --no-default-features (Kiwi-only)"
  features="gui"
  doc_flags=(--no-default-features --features gui)
else
  doc_flags=(--features "$features")
fi

echo "==> cargo doc (${features})"
cargo doc --no-deps "${doc_flags[@]}"

echo ""
echo "Book:  file://$root/docs/book/index.html"
echo "API:   file://$root/target/doc/hfsdr/index.html"
