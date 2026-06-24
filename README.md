# hfsdr
[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/mashu/hfsdr/graph/badge.svg)](https://codecov.io/gh/mashu/hfsdr)
[![GitHub release](https://img.shields.io/github/v/release/mashu/hfsdr)](https://github.com/mashu/hfsdr/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Documentation](https://img.shields.io/badge/docs-mdBook-blue)](https://mashu.github.io/hfsdr/)

A **CW-focused** HF receiver and panadapter for **KiwiSDR**, **Airspy HF+**,
**RTL-SDR**, and **QRP Labs QMX/QMX+**. The UI is built around what CW operators
actually touch: band presets, VFO, RIT, filter chain, and skimmer — without
phone/AM/FM modes or unrelated clutter.

<img width="3160" height="1920" alt="image" src="https://github.com/user-attachments/assets/28ae56f8-50c2-4610-8a8b-2443028ac87f" />
<img width="3160" height="1920" alt="image" src="https://github.com/user-attachments/assets/ac3946fe-59c8-4308-805a-529d03b1556f" />

### Platform support

| | **Linux** | **macOS** | **Windows** |
|---|:---:|:---:|:---:|
| **KiwiSDR** | ✓ | ✓ | ✓ |
| **Airspy HF+** | ✓ | ✓ | — |
| **RTL-SDR** | ✓ | ✓ | — |
| **QMX / QMX+** | ✓ | ✓ | ✓ |

Build the GUI with all local sources (Linux/macOS): `cargo build --features gui --bin hfsdr`.

Windows (Kiwi + QMX): `cargo build --no-default-features --features gui-core --bin hfsdr`.

CLI auto-connect examples:

```bash
hfsdr kiwi kiwisdr.example.com [port] [center_hz]
hfsdr airspy [sample_rate_hz] [center_hz] [process_hz]
hfsdr rtlsdr [sample_rate_hz] [center_hz] [process_hz]
hfsdr qmx [center_hz] [process_hz] [serial_port]
```
