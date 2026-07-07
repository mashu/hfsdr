#!/usr/bin/env python3
"""Patch libSoapySDR.so install search paths for AppImage isolation."""

from __future__ import annotations

import sys
from pathlib import Path


def patch_bytes(data: bytes) -> bytes:
    buf = bytearray(data)
    # Placeholders must match original string lengths (ELF rodata).
    p1 = b"=" * 48
    p2 = b"+" * 31
    steps: list[tuple[bytes, bytes]] = [
        (b"/usr/local/lib/x86_64-linux-gnu/SoapySDR/modules", p1),
        (b"/usr/local/lib/SoapySDR/modules", p2),
        (b"/lib/x86_64-linux-gnu/SoapySDR/modules", b"/nil/x86_64-linux-gnu/SoapySDR/modules"),
        (p1, b"/xxx/local/lib/x86_64-linux-gnu/SoapySDR/modules"),
        (p2, b"/xxx/local/lib/SoapySDR/modules"),
    ]
    for old, new in steps:
        if len(old) != len(new):
            raise ValueError(f"length mismatch: {old!r} vs {new!r}")
        count = buf.count(old)
        if count:
            buf = buf.replace(old, new)
            print(f"  replaced {count}x {old.decode()}")
    return bytes(buf)


def patch_file(path: Path) -> None:
    original = path.read_bytes()
    patched = patch_bytes(original)
    if patched != original:
        path.write_bytes(patched)
        print(f"Patched {path.name}")
    else:
        print(f"No changes for {path.name}")


def main() -> int:
    if len(sys.argv) < 2:
        print(f"usage: {sys.argv[0]} LIB_SOAPYSDR [more...]", file=sys.stderr)
        return 2
    seen: set[str] = set()
    for arg in sys.argv[1:]:
        path = Path(arg).resolve()
        key = str(path)
        if key in seen or not path.is_file():
            continue
        seen.add(key)
        patch_file(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
