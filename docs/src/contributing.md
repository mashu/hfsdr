# Contributing

Thanks for helping improve hfsdr. Read **Part I–IV** of this book first if you
are new to the domain — PRs that change DSP should update the matching chapter.

---

## Before opening a PR

```sh
cargo test --features gui
cargo clippy --features gui -- -D warnings
./scripts/build-docs.sh   # if behavior or pipeline changed
```

---

## Layer rules

| Layer | May use | Must not use |
|-------|---------|--------------|
| `src/dsp`, `src/skimmer` | `source`, math crates | egui, cpal |
| `src/kiwi`, `src/airspyhf` | `source` | dsp, gui |
| `src/bin/waterfall` | `hfsdr`, egui | — |

Extract pure functions when UI logic grows — see `spot_filter.rs` and
`engine/policy.rs`.

### Where to put new code

| Change | Location |
|--------|----------|
| CW DSP stage | `src/dsp/cw/` |
| Skimmer | `src/skimmer/` |
| IQ source | `src/<device>/` + `src/source/controls.rs` |
| Pump / wideband tuning | `src/bin/waterfall/engine/policy.rs` |
| UI panel | `src/bin/waterfall/app/methods/ui/...` |
| Connect / CLI | `src/bin/waterfall/source/` |

See [Refactor roadmap](./architecture/refactor-roadmap.md) for the full map.

---

## Documentation expectations

| Change type | Update |
|-------------|--------|
| New CW stage | Part III + `dsp/cw` module doc |
| Skimmer algorithm | Part IV |
| UI workflow | Part I operator chapter |
| Threading / engine | Part V responsiveness |

Avoid API-only docs — explain **behavior and why**.

---

## DSP guidelines

- Preallocate; no alloc in per-sample `process()`.
- `reset_state()` on disconnect.
- Unit tests with synthetic tones — no hardware required.

---

## Commits

Imperative subject, explain **why**:

```text
Use Blackman tap scaling for sub-200 Hz CW mode

Adjacent rejection was insufficient at 150 Hz with Gaussian-only scaling.
```

---

## License

Contributions are under the project MIT license. Author metadata: Mateusz Kaduk (SO5KM).

Questions: GitHub issues on [github.com/mashu/hfsdr](https://github.com/mashu/hfsdr).
