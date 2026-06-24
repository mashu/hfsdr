# hfsdr
<img width="3824" height="2272" alt="image" src="https://github.com/user-attachments/assets/67d1b346-3e46-447f-9302-b77f44c7ba46" />

[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Documentation](https://img.shields.io/badge/docs-mdBook-blue)](docs/src/introduction.md)

A **CW-focused** HF receiver and panadapter for **KiwiSDR**, **Airspy HF+**,
**RTL-SDR**, and **QRP Labs QMX/QMX+**. The UI is built around what CW operators
actually touch: band presets, VFO, RIT, filter chain, and skimmer — without
phone/AM/FM modes or unrelated clutter.

All radios use one `IqSource` interface; the same DSP chain,
skimmer, and GUI apply to each. Live spectrum and waterfall, configurable CW
demod and filters, audio out, and contest-style skimming with MASTER.SCP callsign
checks.

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
