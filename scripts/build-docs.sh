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

has_airspy=false
has_gui=false
if pkg-config --exists libairspyhf 2>/dev/null; then
  has_airspy=true
fi
if pkg-config --exists alsa 2>/dev/null; then
  has_gui=true
fi

if ! $has_gui; then
  echo "==> alsa not found; API docs will omit gui feature (no cpal/eframe)"
fi
if ! $has_airspy; then
  echo "==> libairspyhf not found; API docs will omit airspy feature"
fi

features=()
doc_flags=(--no-default-features)
if $has_airspy; then
  features+=(airspy)
fi
if $has_gui; then
  features+=(gui)
fi
if ((${#features[@]} > 0)); then
  doc_flags+=(--features "$(IFS=,; echo "${features[*]}")")
fi
features_label="${features[*]:-(none)}"
features_label="${features_label// /,}"

echo "==> cargo doc (${features_label})"
cargo doc --no-deps "${doc_flags[@]}"

echo ""
echo "Book:  file://$root/docs/book/index.html"
echo "API:   file://$root/target/doc/hfsdr/index.html"
