# What the skimmer does on a busy band

During a contest opening, dozens of stations call CQ between 7030 and 7035 kHz.
Your ears follow one run; the **skimmer** attempts to **copy many** simultaneously
and list them as **spots**.

---

## Operator value

| Without skimmer | With skimmer |
|-----------------|--------------|
| Manual scan for CQs | Table of calls + offsets |
| Miss short CQ windows | Labels on waterfall |
| Guess who is calling | CQ / heard classification |

It is **assistive** — verify before logging. Decoder errors happen on weak or
overlapping signals.

---

## How it works (overview)

```text
  Latest spectrum row
        │
        ▼
   find peaks (SNR, separation)
        │
        ├── peak A ──► decoder channel A ──► "CQ DL1ABC"
        ├── peak B ──► decoder channel B ──► "G0XYZ"
        └── peak C ──► ...
        │
        ▼
   merge into spots (bucket by frequency)
        │
        ▼
   table + plot labels (after UI filters)
```

Each **decoder channel**:

1. Mix IQ so that peak sits at baseband.
2. Decimate lightly.
3. **2-pole lowpass** (~cheap, not contest-grade FIR).
4. Envelope → Morse decoder.

Channels **retire** when peak vanishes for several seconds. **Max decoders** caps CPU.

---

## Settings that matter

All skimmer DSP and decoder parameters are in the **Decoder & channel DSP**
panel (right side when skimmer is enabled). Defaults match contest-style
operation; every tunable has a UI control and persists in settings.

| Setting | What it does |
|---------|----------------|
| **Algorithm** | **Bayesian** (self-tuning, best copy), **Bigram beam**, or **Adaptive** (CPU) |
| **Peak min SNR / separation** | Which FFT peaks spawn decoders |
| **Max decoders** | Parallel channel cap |
| **Channel LPF Hz** | Per-channel filter half-width before decode |
| **Initial WPM** | Dot-length seed before speed adapts |
| **Beam width** | Beam hypothesis count (Bayesian / Bigram) |
| **Key thr low / high** | Envelope key-down thresholds (Bigram / Adaptive; Bayesian estimates its own) |
| **Channel timeout** | Retire decoder when peak vanishes (seconds) |
| **Store max age** | Drop stale spots from engine store |
| **Max decode chars** | Text buffer per channel |
| **Require SCP** | Strict callsign validation |

**Spot display** filters (table min SNR, CQ only, continents) affect the UI
only; they do not change decoder algorithms.

Settings persist in `~/.config/hfsdr/settings.json`.

---

## UI filters (table vs skimmer engine)

The engine publishes **all** spots meeting skimmer SNR. The GUI applies **display**
filters separately (pure logic, testable):

- Min table SNR, max age
- Call prefix, CQ only
- Continent filter
- Sort order (call, freq, SNR, last heard)

Plot labels dedupe by bucket and respect **hide heard** / label limit.

---

## When to disable

- CPU or drops rising — skimmer is parallel work.
- Testing listen filters — reduce variables.
- Band empty — no benefit.

Clear spots when changing band or after major retune to avoid stale calls.

---

## Next

[How decoding works](./how-decoding-works.md) — Morse timing and beam search.
