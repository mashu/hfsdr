#!/usr/bin/env bash
# Install libairspyhf / RTL-SDR dependencies for macOS CI and local Homebrew builds.
# Sets PKG_CONFIG_PATH and LIBRARY_PATH via GITHUB_ENV when running in GitHub Actions.

set -euo pipefail

# GitHub-hosted macOS runners ship pre-tapped third-party repos. Homebrew 6+
# requires explicit trust before loading them (https://docs.brew.sh/Tap-Trust).
for tap in aws/tap azure/bicep; do
  brew trust "$tap" 2>/dev/null || true
done

packages=(airspyhf pkg-config)
if [[ "${1:-}" != "--no-rtlsdr" ]]; then
  packages+=(librtlsdr soapysdr)
fi

brew install "${packages[@]}"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "PKG_CONFIG_PATH=$(brew --prefix)/lib/pkgconfig" >> "$GITHUB_ENV"
  echo "LIBRARY_PATH=$(brew --prefix)/lib" >> "$GITHUB_ENV"
fi
