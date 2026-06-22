# hfsdr

[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Documentation](https://img.shields.io/badge/docs-mdBook-blue)](docs/src/introduction.md)

A **CW-focused** HF receiver and panadapter for **KiwiSDR** and **Airspy HF+**.
The UI is built around what CW operators actually touch: band presets, VFO, RIT,
filter chain, and skimmer — without phone/AM/FM modes or unrelated clutter.

Works with KiwiSDR and Airspy through one `IqSource` interface; the same DSP
chain, skimmer, and GUI apply to both. Live spectrum and waterfall, configurable
CW demod and filters, audio out, and contest-style skimming with MASTER.SCP
callsign checks.
