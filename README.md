# hfsdr

Core of an HF SDR / CW client built around one idea: every front end implements
the `IqSource` trait, so the DSP, decoder, and UI never know which radio they are
talking to.

```
src/
├─ lib.rs            crate root + re-exports
├─ source.rs         IqSource trait + SourceError
├─ main.rs           demo driver (opens an Airspy, streams 1 s, reports stats)
├─ airspyhf/
│  ├─ sys.rs         raw libairspyhf FFI (the only unsafe)
│  └─ mod.rs         safe AirspyHf wrapper (impl IqSource + device-specific knobs)
├─ kiwi/
│  ├─ mod.rs         KiwiSource (impl IqSource)
│  ├─ protocol.rs    SND frame parsing + IQ decode
│  └─ reader.rs      WebSocket reader thread
├─ dsp/
│  ├─ mod.rs         module exports
│  └─ spectrum.rs    SpectrumAnalyzer (windowed FFT → dB rows)
└─ bin/waterfall/
   ├─ main.rs        egui entry point
   ├─ app.rs         waterfall UI + state
   ├─ colormap.rs    dB → colour ramp
   └─ source.rs      CLI source builder
tests/
└─ integration.rs    public API integration tests
```

## Build

The library, the Airspy source, the Kiwi source, and the DSP build on any recent
stable toolchain:

```sh
sudo apt install libairspyhf-dev          # Airspy FFI links against this
cargo build --release
cargo run --release --bin hfsdr           # Airspy probe/demo
cargo test                              # unit + integration tests
```

The GUI is behind the `gui` feature (pulls eframe + wgpu + winit):

```sh
# Linux build deps for winit/wgpu:
sudo apt install libxkbcommon-dev libwayland-dev \
                 libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev

cargo run --release --features gui --bin waterfall airspy
cargo run --release --features gui --bin waterfall airspy 768000 7030000
cargo run --release --features gui --bin waterfall kiwi <host> 8073 7030000
```

### Toolchain requirement for the GUI

Current `eframe`/`wgpu` pull crates that use **edition 2024**, so the `gui`
feature needs **Rust >= 1.85** (`rustup update stable`). The non-GUI crates build
on older toolchains (verified on 1.75). This is the one reason the GUI was not
compiled in the environment it was authored in — only an old packaged Cargo was
available there.

## What is verified vs. what to watch locally

Compiled and run against the latest crates: the libairspyhf FFI (linked against
1.6.8), the lock-free USB-thread callback, the Kiwi SND/IQ frame parser
(tungstenite 0.29 `Bytes`/`Utf8Bytes` API), and the FFT spectrum stage
(rustfft 6.4).

`bin/waterfall` targets egui 0.34 (`App::ui`, `Panel::right`, `ColorImage::new`).

## Design notes

- IQ flows source -> lock-free SPSC ring (`rtrb`) -> DSP. On consumer
  backpressure the source drops and counts samples; it never blocks its
  real-time thread (the Airspy USB callback or the Kiwi reader).
- `num_complex::Complex32` is layout-compatible with `airspyhf_complex_float_t`,
  so Airspy samples are reinterpreted with no copy/convert.
- Airspy exposes device-specific controls a generic HAL hides:
  `set_calibration_ppb`, `set_hf_agc`/`set_hf_att`/`set_hf_lna`, `set_lib_dsp`,
  `is_low_if`.
- Kiwi requests `mod=iq`, answers the server's `audio_rate` with `SET AR OK`,
  and parses SND frames (tag, flags, seq, S-meter, 10-byte GPS header, then
  big-endian interleaved int16 I/Q). Retune/keepalive go over a command channel
  to the reader thread, which uses a socket read timeout so it can both read and
  write.

## Next

- CW path: NCO shift + decimate + variable linear-phase FIR (50 Hz–2 kHz) +
  product detector, then the beam-search/bigram decoder.
- Audio out via `cpal`.
- Zoom/decimation so the Airspy panadapter isn't a fixed full-span FFT.
