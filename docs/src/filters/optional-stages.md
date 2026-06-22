# Optional stages: blanker, AGC, NR, and the rest

The core path (NCO → decimate → channel FIR → BFO) is always active. Other
stages are **tools** — enable when you understand what problem they solve.

---

## Noise blanker (IQ domain)

**Problem:** Lightning, car ignition, power-line **impulses** — short, **wideband**
bursts.

**Mechanism:** Compare sample magnitude to a slow average; if spike, output zero
for a few samples (**hold** width).

**Why before the channel filter:** Impulses spread in time after narrow filtering;
blanking works on **raw** IQ where they still look like spikes.

**When to enable:** Summer QRN, urban noise. **When to disable:** If it clips
legitimate fast rise times on very strong CW (rare at sensible threshold).

---

## Manual gain vs AGC (after channel filter)

**AGC** tracks envelope, attacks fast on peaks, decays slower, targets a set
level.

**Manual gain** fixed multiplier — preferred when AGC pumps on fluttery signals.

RF AGC on Airspy (hardware) is separate — prevents ADC clipping; does not replace
audio AGC.

---

## Audio peak filter (APF)

Resonant **bump** at BFO pitch added to demodulated audio — not a second brick
filter.

**Use:** Weak desired signal still in passband but buried in noise **after**
selectivity is already good.

**Avoid:** Substitute for narrow BW when neighbor is the problem — APF does not
remove them.

---

## Auto-notch (audio, LMS)

Adaptive filter learns a **notch** at steady interfering tones **off your pitch**.

**Pitch guard:** When energy at your BFO is high (you are copying CW), adaptation
**freezes** so the notch does not eat your signal.

**Use:** Persistent heterodyne whistle 200–400 Hz away from pitch.

**Tune:** Guard width (Hz), adaptation rate — too aggressive causes artifacts.

---

## Noise reduction

Soft-knee **downward expansion** on envelope — reduces gain when signal below
threshold.

**Use:** Constant hiss after filter is tight.

**Avoid:** Masking adjacent CW that should have been filtered out.

---

## Squelch

Mutes audio when envelope below threshold for comfortable monitoring between
transmissions.

---

## Stage order (why it is fixed)

```text
  blanker → shift → decimate → notches → FIR → AGC → detect → APF → auto-notch → NR → squelch
```

- Blanker and notches on **complex IQ** before detection.
- APF/auto-notch/NR on **real audio** after detection.
- Reordering would break assumptions and tests.

Each stage is a small module with `reset_state()` on disconnect — contributors
should preserve the order unless rewriting tests and docs together.

---

## Skimmer does not use these

Skimmer channels skip APF/NR/AGC for CPU reasons. Do not enable skimmer NR expecting
headphone behavior.
