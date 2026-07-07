#!/usr/bin/env bash
# Build a portable AppImage with bundled SDR driver libraries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=appimage/soapy-appimage-integration.sh
source "${SCRIPT_DIR}/appimage/soapy-appimage-integration.sh"
cd "$ROOT"

VERSION="${HFSDR_VERSION:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')}"
APPDIR="${ROOT}/AppDir"
ARTIFACT="${ROOT}/hfsdr-${VERSION}-x86_64.AppImage"
LIBDIR="${APPDIR}/usr/lib"
SOAPY_MODULES="${LIBDIR}/SoapySDR/modules0.8"

if [[ "${CI:-}" != "true" ]]; then
  echo "Building release binary…"
  cargo build --release --features gui
fi

require_soapy_modules() {
  for dir in /usr/lib/x86_64-linux-gnu/SoapySDR/modules0.8 /usr/lib/SoapySDR/modules0.8; do
    if [[ -d "$dir" ]] && compgen -G "${dir}/*.so" >/dev/null; then
      return 0
    fi
  done
  echo "error: no SoapySDR driver modules found on the build host." >&2
  echo "Install all drivers: sudo apt install soapysdr-module-all" >&2
  exit 1
}
require_soapy_modules

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$LIBDIR" "$SOAPY_MODULES" "$APPDIR/usr/share/applications"

cp target/release/hfsdr "$APPDIR/usr/bin/"
cp packaging/hfsdr.desktop "$APPDIR/usr/share/applications/"
cp README.md "$APPDIR/"

copy_sdr_lib() {
  local stem="$1"
  shift
  for name in "$@"; do
    for dir in /usr/lib/x86_64-linux-gnu /usr/lib /usr/local/lib; do
      if [[ -f "${dir}/${name}" ]]; then
        cp -L "${dir}/${name}" "$LIBDIR/"
        echo "Bundled ${dir}/${name}"
        return 0
      fi
    done
  done
  echo "warning: ${stem} shared library not found — AppImage will still run without that source" >&2
  return 0
}

skip_ldd_dep() {
  local base="$1"
  case "$base" in
    linux-vdso.so.*|ld-linux*.so.*|libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|\
    libresolv.so.*|libstdc++.so.*|libgcc_s.so.*) return 0 ;;
  esac
  return 1
}

bundle_ldd_deps() {
  local lib="$1"
  [[ -f "$lib" ]] || return 0
  local dep resolved base
  while read -r dep; do
    [[ -n "$dep" && -f "$dep" ]] || continue
    base="$(basename "$dep")"
    skip_ldd_dep "$base" && continue
    if [[ -f "${LIBDIR}/${base}" ]]; then
      continue
    fi
    resolved="$(readlink -f "$dep")"
    cp -L "$resolved" "$LIBDIR/"
    echo "Bundled dependency ${resolved}"
    bundle_ldd_deps "${LIBDIR}/${base}"
  done < <(ldd "$lib" 2>/dev/null | awk '/=> \// {print $3}')
}

copy_soapy_plugins() {
  local copied=0
  for dir in /usr/lib/x86_64-linux-gnu/SoapySDR/modules0.8 /usr/lib/SoapySDR/modules0.8; do
    if [[ ! -d "$dir" ]]; then
      continue
    fi
    for f in "$dir"/*.so; do
      [[ -f "$f" ]] || continue
      cp -L "$f" "$SOAPY_MODULES/"
      echo "Bundled Soapy plugin ${f}"
      bundle_ldd_deps "$f"
      copied=$((copied + 1))
    done
  done
  for dir in /usr/lib/x86_64-linux-gnu /usr/lib; do
    for f in "$dir"/libSoapy*.so*; do
      [[ -f "$f" ]] || continue
      case "$(basename "$f")" in
        libSoapySDR.so*) continue ;;
      esac
      cp -L "$f" "$LIBDIR/"
      echo "Bundled Soapy module ${f}"
      bundle_ldd_deps "$f"
      copied=$((copied + 1))
    done
  done
  if [[ "$copied" -eq 0 ]]; then
    echo "error: failed to copy any SoapySDR driver plugins into AppDir" >&2
    exit 1
  fi
  echo "Bundled ${copied} SoapySDR plugin file(s) into ${SOAPY_MODULES}"
}

copy_sdr_lib airspyhf libairspyhf.so.1 libairspyhf.so
copy_sdr_lib rtlsdr librtlsdr.so.0 librtlsdr.so
copy_sdr_lib soapysdr libSoapySDR.so.0.8 libSoapySDR.so.0 libSoapySDR.so
copy_soapy_plugins

# Re-resolve deps after all libs landed (plugins may share dependencies).
for plugin in "$SOAPY_MODULES"/*.so; do
  [[ -f "$plugin" ]] || continue
  bundle_ldd_deps "$plugin"
done

if ! compgen -G "${SOAPY_MODULES}/libPlutoSDRSupport.so" >/dev/null \
  && ! compgen -G "${SOAPY_MODULES}/*[Pp]luto*.so" >/dev/null; then
  echo "warning: Pluto Soapy plugin not found in bundle — install soapysdr-module-plutosdr" >&2
fi

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

export HFSDR_LIB_DIR="$LIBDIR"
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

run_soapy_appimage_prep

"$LINUXDEPLOY" \
  --appdir "$APPDIR" \
  --desktop-file="$APPDIR/usr/share/applications/hfsdr.desktop" \
  --executable="$APPDIR/usr/bin/hfsdr" \
  --icon-file="$ICON"

"$PLUGIN" --appdir "$APPDIR"

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

verify_soapy_plugins() {
  local modules_dir="$1"
  local plugin_count
  plugin_count="$(find "$modules_dir" -maxdepth 1 -name '*.so' 2>/dev/null | wc -l)"
  echo "SoapySDR plugins in image: ${plugin_count}"
  if [[ "${plugin_count}" -lt 1 ]]; then
    echo "error: AppImage contains no SoapySDR driver plugins" >&2
    exit 1
  fi
  if ! compgen -G "${modules_dir}/*[Pp]luto*.so" >/dev/null; then
    echo "warning: Pluto Soapy plugin not found inside AppImage" >&2
  fi
  while IFS= read -r plugin; do
    basename "$plugin"
  done < <(find "$modules_dir" -maxdepth 1 -name '*.so' 2>/dev/null | sort)
}

# GitHub Actions runners cannot execute AppImages for --appimage-extract (no FUSE).
if [[ "${CI:-}" == "true" ]]; then
  verify_soapy_plugins "$SOAPY_MODULES"
else
  rm -rf squashfs-root
  if ./"$ARTIFACT" --appimage-extract >/dev/null 2>&1; then
    verify_soapy_plugins "squashfs-root/usr/lib/SoapySDR/modules0.8"
    rm -rf squashfs-root
  else
    echo "warning: --appimage-extract unavailable; verifying AppDir instead" >&2
    verify_soapy_plugins "$SOAPY_MODULES"
  fi
fi
