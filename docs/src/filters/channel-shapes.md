# The channel filter: Gaussian, RaisedCos, Blackman

The **channel filter** is a windowed **sinc** lowpass applied to complex baseband
after RIT shift and decimation. It defines **selectivity** — how much of the
neighbor’s carrier reaches the BFO.

---

## How it is built (conceptually)

1. Choose cutoff frequency ≈ **half your GUI bandwidth**.
2. Start with ideal **sinc** impulse response (brick-wall frequency response).
3. Multiply by a **window** to truncate the sinc to finite taps.
4. Normalize so passband gain is unity.

**Narrower bandwidth → more taps (up to 2047)** → steeper skirts, more CPU — but
still cheap at ~12 kHz audio rate.

```text
  Frequency response (conceptual)

  gain
    │█████████████╲
    │              ╲_____ stopband
    └──────────────────────► freq
         ↑ passband BW
```

Different windows change how fast the curve falls in the **stopband** and how
much **ringing** appears in time domain.

---

## Gaussian

**Sound:** Softest, most “analog” — minimal ringing on keying edges.

**Stopband:** Gentlest — neighbors farther out are attenuated slowly.

**Use when:** Band is moderately busy but you hate ringing; casual listening;
strong signal already dominates.

---

## RaisedCosine (Hann)

**Sound:** Slightly sharper than Gaussian; still clean for most operators.

**Stopband:** Moderate slope — good default when you are unsure.

**Use when:** Everyday contest copy without extreme QRM.

---

## Blackman

**Sound:** Steepest skirts of the three; slightly wider effective transition.

**Stopband:** Best rejection of **nearby** carriers outside passband.

**Use when:** One signal in passband, strong adjacent CW **500–1500 Hz away** still
audible with other shapes — typical sprint pileup.

This matches on-air goal: **“no extra signal bleeds through when narrowed on
frequency with just one signal.”**

---

## Comparison table

| Shape | Ringing | Adjacent rejection | CPU (taps) |
|-------|---------|-------------------|------------|
| Gaussian | Lowest | Moderate | Similar |
| RaisedCos | Low | Good | Similar |
| Blackman | Low–medium | Best | Similar (tap count set by BW) |

---

## GUI interaction

- **Presets** (150, 200, 300 Hz …) snap common contest widths.
- **Slider** fine-tunes; log scaling helps at low Hz.
- **Ctrl+scroll** on plot adjusts BW without opening the panel.
- **Cyan edges** drag passband; respects CW/wide max.

Changing shape or BW **rebuilds taps** only when values change — no glitch per
sample.

---

## Relationship to decimator lowpass

**Before** decimation, a separate Gaussian lowpass prevents **aliasing** — energy
from far-off frequencies folding into band after sample dropping. That filter
protects **integrity** of the spectrum; the **channel FIR** protects **copy**.

Both matter; only the channel FIR has the three selectable windows in the GUI.
