#!/usr/bin/env bash
# Build a portable AppImage with bundled SDR driver libraries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${HFSDR_VERSION:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')}"
APPDIR="${ROOT}/AppDir"
ARTIFACT="${ROOT}/hfsdr-${VERSION}-x86_64.AppImage"

if [[ "${CI:-}" != "true" ]]; then
  echo "Building release binary…"
  cargo build --release --features gui
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" "$APPDIR/usr/share/applications"

cp target/release/hfsdr "$APPDIR/usr/bin/"
cp packaging/hfsdr.desktop "$APPDIR/usr/share/applications/"
cp README.md "$APPDIR/"

copy_sdr_lib() {
  local stem="$1"
  shift
  for name in "$@"; do
    for dir in /usr/lib/x86_64-linux-gnu /usr/lib /usr/local/lib; do
      if [[ -f "${dir}/${name}" ]]; then
        cp -L "${dir}/${name}" "$APPDIR/usr/lib/"
        echo "Bundled ${dir}/${name}"
        return 0
      fi
    done
  done
  echo "warning: ${stem} shared library not found — AppImage will still run without that source" >&2
  return 0
}

copy_sdr_lib airspyhf libairspyhf.so.1 libairspyhf.so
copy_sdr_lib rtlsdr librtlsdr.so.0 librtlsdr.so
copy_sdr_lib soapysdr libSoapySDR.so.0.8 libSoapySDR.so.0 libSoapySDR.so

LINUXDEPLOY="${ROOT}/.cache/linuxdeploy-x86_64.AppImage"
PLUGIN="${ROOT}/.cache/linuxdeploy-plugin-appimage-x86_64.AppImage"

if [[ ! -x "$LINUXDEPLOY" ]]; then
  mkdir -p "$(dirname "$LINUXDEPLOY")"
  curl -fsSL -o "$LINUXDEPLOY" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
  chmod +x "$LINUXDEPLOY"
fi

if [[ ! -x "$PLUGIN" ]]; then
  curl -fsSL -o "$PLUGIN" \
    "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
  chmod +x "$PLUGIN"
fi

export HFSDR_LIB_DIR="$APPDIR/usr/lib"
export LINUXDEPLOY_OUTPUT_APP_NAME=hfsdr
export LINUXDEPLOY_OUTPUT_VERSION="$VERSION"

ICON="${ROOT}/packaging/hfsdr.png"
SVG="${ROOT}/packaging/hfsdr.svg"

if [[ ! -f "$ICON" ]] || [[ "$SVG" -nt "$ICON" ]]; then
  if [[ -x "${ROOT}/scripts/render-icon.sh" ]]; then
    bash "${ROOT}/scripts/render-icon.sh" 256
  elif [[ ! -f "$ICON" ]]; then
    echo "Missing $ICON — run scripts/render-icon.sh or commit a rendered PNG from packaging/hfsdr.svg" >&2
    exit 1
  fi
fi

"$LINUXDEPLOY" \
  --appdir "$APPDIR" \
  --desktop-file="$APPDIR/usr/share/applications/hfsdr.desktop" \
  --executable="$APPDIR/usr/bin/hfsdr" \
  --icon-file="$ICON" \
  --output appimage

if [[ -f "$ARTIFACT" ]]; then
  :
elif [[ -f "hfsdr-${VERSION}-x86_64.AppImage" ]]; then
  mv "hfsdr-${VERSION}-x86_64.AppImage" "$ARTIFACT"
else
  latest="$(ls -t hfsdr-*.AppImage 2>/dev/null | head -1 || true)"
  if [[ -n "$latest" ]]; then
    mv "$latest" "$ARTIFACT"
  else
    echo "AppImage output not found" >&2
    exit 1
  fi
fi

echo "Created ${ARTIFACT}"
ls -lh "$ARTIFACT"
