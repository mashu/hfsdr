# hfsdr
[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/mashu/hfsdr/graph/badge.svg)](https://codecov.io/gh/mashu/hfsdr)
[![GitHub release](https://img.shields.io/github/v/release/mashu/hfsdr)](https://github.com/mashu/hfsdr/releases)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Try it in the browser](https://img.shields.io/badge/demo-browser-blue)](https://mashu.github.io/hfsdr/)

A **CW-focused** HF receiver and panadapter for **KiwiSDR**, **Airspy HF+**,
**RTL-SDR**, and **QRP Labs QMX/QMX+**. The UI is built around what CW operators
actually touch: band presets, VFO, RIT, and filter chain — without
phone/AM/FM modes or unrelated clutter.

<img width="3160" height="1920" alt="image" src="https://github.com/user-attachments/assets/8a1443fa-e2a0-45fe-807e-34e90ef18e89" />

### Platform support

| | **Linux** | **macOS** | **Windows** |
|---|:---:|:---:|:---:|
| **KiwiSDR** | ✓ | ✓ | ✓ |
| **Airspy HF+** | ✓ | ✓ | ✓ |
| **RTL-SDR** | ✓ | ✓ | ✓ |
| **QMX / QMX+** | ✓ | ✓ | ✓ |

Try the pipeline in your browser: **[mashu.github.io/hfsdr](https://mashu.github.io/hfsdr/)** — the real
spectrum analyzer and waterfall shader running as WebAssembly on synthetic IQ.

Build and install: see [`docs/src/building.md`](docs/src/building.md).

CLI auto-connect examples:

```bash
hfsdr kiwi kiwisdr.example.com [port] [center_hz]
hfsdr airspy [sample_rate_hz] [center_hz] [process_hz]
hfsdr rtlsdr [sample_rate_hz] [center_hz] [process_hz]
hfsdr qmx [center_hz] [process_hz] [serial_port]
```
