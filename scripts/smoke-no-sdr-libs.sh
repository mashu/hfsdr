#!/usr/bin/env bash
# Verify the release binary does not hard-link optional SDR driver libraries.
set -euo pipefail

BIN="${1:-target/release/hfsdr}"

if [[ ! -x "$BIN" ]]; then
  echo "missing executable: $BIN" >&2
  exit 1
fi

if ldd "$BIN" | grep -E 'libairspyhf|librtlsdr|libSoapySDR|SoapySDR'; then
  echo "binary still hard-links SDR libraries:" >&2
  ldd "$BIN" | grep -E 'libairspyhf|librtlsdr|libSoapySDR|SoapySDR' >&2
  exit 1
fi

echo "OK: $BIN has no hard dependency on libairspyhf, librtlsdr, or libSoapySDR"
