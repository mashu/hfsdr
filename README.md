# hfsdr

[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)

HF SDR client and CW panadapter for **Airspy HF+** and **KiwiSDR**. Every radio
front end implements a single `IqSource` trait, so the DSP chain, skimmer, and
GUI stay source-agnostic.

The `hfsdr` desktop app provides a live waterfall and spectrum, CW demodulation
with configurable bandpass filters, audio output, and a contest-style skimmer
with MASTER.SCP callsign validation. IQ flows through lock-free rings from the
device thread to spectrum analysis and decoding without blocking real-time
callbacks.
