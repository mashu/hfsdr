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

| | **Linux** | **macOS** | **Windows** | **Browser** |
|---|:---:|:---:|:---:|:---:|
| **KiwiSDR** | ✓ | ✓ | ✓ | ✓ |
| **Airspy HF+** | ✓ | ✓ | ✓ | — |
| **RTL-SDR** | ✓ | ✓ | ✓ | — |
| **QMX / QMX+** | ✓ | ✓ | ✓ | — |

KiwiSDR is a network receiver, so a tab can reach it over a WebSocket. The
others are local USB or serial devices: a browser cannot load their drivers,
and nothing in the browser build offers them.

Try it in your browser: **[mashu.github.io/hfsdr](https://mashu.github.io/hfsdr/)** — the real
receiver UI as WebAssembly, connected to a live public KiwiSDR, with audio. Not a
demo signal: the page lists public receivers and streams from the one you pick.

The page is served over https, which browsers only let open `wss://` sockets, so
it offers the KiwiSDRs that accept TLS. Local USB devices need drivers a tab
cannot load — use the desktop build for those.

Build and install: see [`docs/src/building.md`](docs/src/building.md).

CLI auto-connect examples:

```bash
hfsdr kiwi kiwisdr.example.com [port] [center_hz]
hfsdr airspy [sample_rate_hz] [center_hz] [process_hz]
hfsdr rtlsdr [sample_rate_hz] [center_hz] [process_hz]
hfsdr qmx [center_hz] [process_hz] [serial_port]
```
