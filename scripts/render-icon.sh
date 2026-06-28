#!/usr/bin/env bash
# Render packaging/hfsdr.png from packaging/hfsdr.svg (source of truth).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVG="${ROOT}/packaging/hfsdr.svg"
PNG="${ROOT}/packaging/hfsdr.png"
SIZE="${1:-256}"

if [[ ! -f "$SVG" ]]; then
  echo "Missing icon source: $SVG" >&2
  exit 1
fi

if command -v rsvg-convert >/dev/null 2>&1; then
  rsvg-convert -w "$SIZE" -h "$SIZE" -o "$PNG" "$SVG"
elif command -v inkscape >/dev/null 2>&1; then
  inkscape "$SVG" --export-type=png --export-filename="$PNG" -w "$SIZE" -h "$SIZE"
else
  echo "Install rsvg-convert (librsvg) or inkscape to render $PNG from $SVG" >&2
  exit 1
fi

echo "Rendered ${PNG} (${SIZE}x${SIZE}) from ${SVG}"
