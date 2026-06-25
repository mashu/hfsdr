# Notches and birdies

Two different notch systems address **different kinds** of QRM.

---

## Manual IQ notches (up to four)

**Domain:** Complex IQ **before** channel FIR.

**Best for:** Stable **birdies** at known offset — spur, heterodyne, RTTY line,
broadcast spur aliasing.

**Controls:**

| Parameter | Meaning |
|-----------|---------|
| Offset Hz | Distance from **listen center** |
| Width Hz | Notch width |
| Enabled | Per-notch toggle |

**On plot:** Vertical markers at notch positions.

**Keyboard:** `N` toggles notch at cursor (when focused on plot).

**Why IQ domain:** Removes energy **before** it passes through the BFO and
becomes audio whistle.

```text
  Spectrum slice:

  ────█──────█──────█──────
      spur   YOU    neighbor
      ↑
   notch here
```

---

## Auto-notch (audio LMS)

**Domain:** Real audio **after** product detector.

**Best for:** Drifting or unknown **audio-domain** tones near pitch — often
another CW carrier close enough to demod partially.

**Protected by pitch guard** — see [Optional stages](./optional-stages.md).

**Not a substitute** for narrow channel filter when a strong adjacent **carrier**
sits just outside passband — the channel FIR should handle that first.

---

## Decision guide

| Symptom | Try first | Then |
|---------|-----------|------|
| Strong CW 400 Hz away audible | Narrow BW + Blackman | Manual notch at offset |
| Steady whistle when not keyed | Auto-notch | Check BFO / APF settings |
| Wide hash on every stroke | Noise blanker | Check RF overload |
| One pure tone on pan always there | Manual notch at pan frequency | — |

---

## RF vs DSP

Airspy **attenuator / LNA / HF AGC** affect what reaches ADC. If the front end
is overloaded, notches fight symptoms. Reduce gain when S-meter pegs on noise alone.

Kiwi passband is negotiated in software protocol; notches still apply in DSP.

---

## Interaction with skimmer

Notches affect **listen path IQ** shared conceptually with the same snapshot;
skimmer per-peak channels use their own NCO isolation. A manual notch at +500 Hz
does not remove that signal from skimmer decoders tuned there — skimmer is
independent per peak.

For contest logging, trust **your ears + filter** over skimmer when they disagree.
