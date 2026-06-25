# Refactor roadmap

Incremental maintainability work on the GUI binary. The **library** (`src/dsp`,
`src/skimmer`, front ends) is already split by stage; this roadmap targets
`engine/` and `app/`.

---

## Goals

- Shrink monoliths (`engine`, `WaterfallApp`, `widgets`, `interaction`)
- Extract testable pump policy from the real-time loop
- Group `WaterfallApp` fields into named sub-states
- Use normal Rust `impl` modules instead of build-time `include!`
- Slim hardware-control duplication over time

---

## Phase status

| Phase | Topic | Status |
|-------|--------|--------|
| 0 | Docs and guardrails | Done |
| 1 | `engine/policy.rs` + unit tests | Done |
| 2 | `engine/` module split | Done |
| 3 | `app/state/` sub-structs | Done |
| 4 | Real `app/methods` modules (no `include!`) | Done |
| 5 | `widgets/` + `interaction/` split | Done |
| 6 | `source/` settings + connection split | Done |
| 7 | `source/controls.rs` extension traits | Done |

---

## Where to put new code

| Change | Location |
|--------|----------|
| CW filter stage | `src/dsp/cw/<stage>.rs` → `channel.rs` → `settings.rs` |
| Skimmer decoder | `src/skimmer/` |
| IQ source | `src/<device>/` + `src/source/controls.rs` |
| Connect / CLI | `src/bin/waterfall/source/connection.rs`, `cli.rs` |
| Pump / wideband policy | `src/bin/waterfall/engine/policy.rs` |
| UI panel | `src/bin/waterfall/app/methods/ui/...` |
| Pure UI logic | sibling `.rs` without egui (see `spot_filter.rs`) |

---

## Editing `WaterfallApp` methods

Methods live in `src/bin/waterfall/app/methods/**/*.rs`, each with its own
`impl WaterfallApp` block. Shared imports: `app/prelude.rs`. State fields are
grouped under `app/state/` (`connection`, `radio`, `plot`, …).

Do **not** edit generated `OUT_DIR` files — there are none after phase 4.
