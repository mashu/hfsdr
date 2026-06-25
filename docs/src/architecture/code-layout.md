# How the code is organized

The repository splits **reusable signal processing** (library) from **desktop
shell** (binary). This chapter maps folders to concepts — not a file listing for
its own sake.

---

## Top level

```text
  hfsdr/
  ├── src/           library + engine algorithms
  ├── src/bin/       executables (GUI, airspy-probe)
  ├── docs/          this book (mdBook)
  ├── tests/         integration + optional live Kiwi test
  └── scripts/       build-docs.sh, install-windows-sdr-deps.ps1
```

---

## Library (`src/`)

| Area | Folder | Responsibility |
|------|--------|----------------|
| Front ends | `airspyhf/`, `kiwi/`, `rtlsdr/`, `qmx/` | Device I/O → IQ ring |
| Contract | `source/mod.rs` | Slim `IqSource` + `source/controls.rs` extension traits |
| DSP | `dsp/` | Spectrum, CW chain, view math |
| Skimmer | `skimmer/` | Peaks, decoders, spots, SCP |
| Geography | `cty/` | Call → continent |
| History | `history/` | Slow waterfall buffer |

**Rule:** no egui, no cpal in library code.

---

## CW chain inside `dsp/cw/`

One file ≈ one processing stage (`fir.rs`, `agc.rs`, …). `channel.rs` composes
order; `settings.rs` holds serializable parameters.

Adding a stage:

1. Implement struct with `process()` + `reset_state()`.
2. Insert in `CwChannel` in documented order.
3. Extend `CwChannelSettings` + GUI panel + tests.

---

## GUI binary (`src/bin/waterfall/`)

| Module | Role |
|--------|------|
| `app/mod.rs` | `WaterfallApp` shell, panel layout |
| `app/methods/` | `impl WaterfallApp` per feature (plot, tuning, connection, …) |
| `app/state/` | Grouped UI state (`ConnectionFormState`, `KiwiDirectoryState`, `EngineUiState`, `PlotState`, …) |
| `app/methods/plot/cache.rs` | Waterfall texture upload and incremental cache sync |
| `engine/` | Background thread, pump, connection lifecycle |
| `engine/policy.rs` | Pure wideband / pump policy (unit-tested) |
| `widgets/` | Spectrum/waterfall draw layers |
| `interaction/` | Plot state, geometry, freq formatting |
| `source/` | Connect requests, `DeviceSource`, per-device settings, `connect()` |
| `spot_filter.rs` | Pure spot filter/sort |
| `settings.rs` | JSON persistence |
| `log.rs` | Ring log |

**Rule:** business logic out of `show()` closures — keep widgets dumb.

Largest real-time hotspot: `engine/pump.rs` (`pump_stream`). Policy thresholds
live in `engine/policy.rs`.

---

## Tests

| Location | Covers |
|----------|--------|
| `src/**/tests` | DSP, skimmer, patterns |
| `src/bin/waterfall/*` tests | UI helpers, settings |
| `tests/integration.rs` | Public API smoke |

Run: `cargo test --features gui`

---

## Features

| Feature | Links |
|---------|--------|
| `airspy` | libairspyhf (default) |
| `rtlsdr` | librtlsdr |
| `gui` | `gui-core` + `rtlsdr` |
| `gui-core` | eframe, cpal, Kiwi, QMX — no RTL-SDR |
| `qmx` | serialport + cpal for QMX |

Platform-specific library setup: [Building hfsdr](../building.md).

---

## API reference

`cargo doc --no-deps --features gui` generates type reference. **This book**
explains behavior; rustdoc lists signatures.
