# Why bandwidth and filter shape matter

On a quiet band, almost any filter sounds fine. In a **contest pileup**, filter
settings decide whether you copy **one** station or **three at once**.

---

## The problem: adjacent CW

Each CW signal is a carrier. Keying sidebands are narrow, but the **carrier**
sits at a fixed offset. A **wide** audio passband includes multiple carriers:

```text
  Passband 500 Hz wide on a crowded slice:

  ────────█───────█───────█────────
          A       YOU     B

  All three contribute energy → B sounds in your copy of YOU
```

Narrowing passband helps:

```text
  Passband 150 Hz, good shape:

  ─────────────███────────────────
                YOU
```

---

## Bandwidth vs shape

| Knob | What it controls |
|------|------------------|
| **BW (Hz)** | How wide the passband is |
| **Shape (window)** | How sharply energy **outside** BW is rejected |

A **200 Hz** passband with a **soft** Gaussian still leaks more than a **250 Hz**
passband with **Blackman** in some scenarios — but generally **narrower + steeper
skirts** wins on crowded bands.

---

## CW mode (≤500 Hz) vs Wide (≤2 kHz)

Contest CW rarely needs more than 300–400 Hz for normal keying speeds. Wide mode
exists for:

- Monitoring multiple nearby signals loosely
- Unusual wider transmissions
- Experimentation

Presets above the mode cap are hidden so you do not accidentally run 1 kHz on 40 m
during a sprint.

---

## Practical tuning procedure

1. Set **CW mode**, start around **200–300 Hz** BW.
2. If neighbor audible → try **Blackman**, then reduce BW in steps of 25 Hz.
3. If still whistling **on one steady frequency** → manual **notch** (see
   [Notches and birdies](./notches-and-qrm.md)).
4. If **crackling** between characters → **noise blanker** (impulses, not adjacent CW).
5. If level jumps → **AGC**; if hiss → **NR** after filter is correct.

> **Common mistake:** Turning on NR or auto-notch before narrowing the passband.
> Those tools clean **tone quality**; they do not replace **selectivity**.

---

## What software cannot fix

- **Overlapping keying** (two stations same frequency) — physics.
- **Strong in-band IMD** from front-end overload — reduce RF gain / fix antenna.
- **Key clicks** from dirty transmitters — some energy is very wide.

hfsdr tests verify **adjacent-tone rejection** in synthetic conditions; on-air
behavior also depends on your antenna and RF gain.

---

## Next

[Channel filter shapes](./channel-shapes.md) explains Gaussian vs RaisedCos vs
Blackman in detail.
