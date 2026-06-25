# Reading the spectrum and waterfall

The display answers two questions contest operators ask constantly: **who is on
the band** and **how strong are they**.

---

## Spectrum trace (top)

The trace is a **snapshot**: power in dB vs frequency **right now**.

```text
  power (dB)
    ▲
    │     ╱╲              ╱╲
    │    ╱  ╲   CQ here  ╱  ╲
    │───╱────╲──────────╱────╲────── noise floor
    └──────────────────────────────► frequency
              ↑
         your center (vertical line)
```

- **Peaks** are carriers or keyed CW (a keyed carrier looks like a solid bump).
- **Reference / range** (display settings) slide the trace vertically — like
  adjusting graticule on a classic panadapter.
- **Smoothing** (if enabled) averages along frequency to reduce speckle; it is
  cosmetic for the trace, not for skimmer peak pick.

---

## Waterfall (below)

Each new FFT row becomes one **horizontal line** of colour. Time scrolls
**downward**: the bottom is newest (convention may feel inverted if you are used
to some other software — watch whether activity appears at the bottom).

```text
  older ▲
        │  ·  ·    ·· ·     ·    ← past
        │ · · ·  · ·  ·   · ·
        │  ·    ·    · · ·      ← recent
        └──────────────────────► frequency
```

**How to use it in a pileup:**

1. Find a vertical **column** of bright dashes — someone calling CQ on a fixed
   offset.
2. Compare **brightness** over time — stronger stations leave stronger trails.
3. Watch **width** — very wide trails may be RTTY/ digital, not CW.

Colour mapping uses auto-scaled **display levels** so weak DX does not vanish
and strong locals do not saturate the palette.

---

## Passband overlay (cyan)

The shaded region (and draggable edges) is **not** the RF front-end bandwidth.
It is the **software listen filter** width around your **listen point** (center
+ RIT).

```text
  full IQ passband (what the radio delivers)
  |──────────────────────────────────────|
           cyan listen passband
              |────────|
                    ↑
              listen offset (RIT)
```

Dragging edges changes **audio** selectivity. Scrolling the plot zooms **what
frequencies you see**, not necessarily the same thing.

---

## Spot labels on the plot

When the skimmer runs, callsigns appear near peaks. Labels:

- **Dedupe** within a frequency bucket (strongest SNR wins).
- Can **hide unconfirmed** (“heard” without solid decode).
- Respect table filters (SNR, CQ only, prefix, continent).

If labels collide, layout nudges them — crowded 40 m may still look busy; narrow
**plot label limit** in skimmer settings.

---

## History panel (optional)

A slower, longer waterfall buffer for reviewing band activity over minutes.
Costs memory and GPU texture updates — enable when analyzing openings, not when
every CPU cycle counts in a sprint contest.
