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
  └── scripts/       build-docs.sh
```

---

## Library (`src/`)

| Area | Folder | Responsibility |
|------|--------|----------------|
| Front ends | `airspyhf/`, `kiwi/` | Device I/O → IQ ring |
| Contract | `source.rs` | `IqSource` trait |
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
| `app.rs` | Panels, shortcuts, state |
| `engine.rs` | Background thread, connection |
| `widgets.rs` | Spectrum/waterfall draw |
| `interaction.rs` | Mouse → tune/BW actions |
| `spot_filter.rs` | Pure spot filter/sort |
| `settings.rs` | JSON persistence |
| `log.rs` | Ring log |

**Rule:** business logic out of `show()` closures — keep widgets dumb.

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

- `airspy` — link libairspyhf
- `gui` — eframe + cpal + settings

Windows CI often builds `--no-default-features --features gui` (Kiwi only).

---

## API reference

`cargo doc --no-deps --features gui` generates type reference. **This book**
explains behavior; rustdoc lists signatures.
