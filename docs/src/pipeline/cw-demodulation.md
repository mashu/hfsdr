# How CW becomes audio you can copy

CW on the air is **on/off carrier**. Your ear wants **on/off tone** near 600 Hz.
The listen chain bridges that gap in three conceptual steps: **select**,
**shift**, **detect**.

---

## Step 1 — Select one station (channel filter)

After RIT moves the target signal near the center of the complex baseband, the
**channel FIR** keeps energy within ±BW/2 and attenuates adjacent CW:

```text
  Before filter (conceptual spectrum around listen point)

      neighbor     YOUR CW     neighbor
         │           │           │
         ▼           ▼           ▼
  ───────█──────────███──────────█──────
                    ↑
              passband (cyan in GUI)

  After narrow Blackman filter

  ───────────────────█──────────────────
                    ↑
              mostly only you
```

**Why linear-phase FIR?** CW edges must stay sharp. IIR “brick wall” filters
ring and smear dots — operators hear it instantly. FIR trades CPU for clean
keying.

Filter shape and width are covered in [Channel filter shapes](../filters/channel-shapes.md).

---

## Step 2 — Shift to audio frequency (BFO + product detector)

The filtered signal is still **radio-frequency baseband** — a tone at perhaps
0–800 Hz offset from DC. The **BFO** (beat frequency oscillator) introduces a
local tone at your chosen **pitch** (e.g. 650 Hz).

The **product detector** multiplies filtered RF by the BFO and takes the real
part — classic **heterodyne** demodulation:

```text
  keyed RF tone  ×  steady BFO  →  audio near 650 Hz keyed on/off
```

When the operator keys, you hear a 650 Hz note; when they stop, silence (minus
noise). Changing BFO changes **pitch**, not the station’s on-air frequency.

---

## Step 3 — Polish (optional)

| Stage | What you hear without it | What it fixes |
|-------|--------------------------|---------------|
| **APF** | Flat response in passband | Adds gentle resonance at pitch — weak signals pop |
| **Auto-notch** | Whistle from nearby carrier | Removes steady off-pitch tones; guard protects your CW |
| **NR** | Hiss | Expands quiet parts down |
| **AGC** | Level swings on fades | Keeps output steady |
| **Squelch** | Noise between transmissions | Mutes when envelope low |

None of these replace a bad filter setting on a crowded band — fix **BW and
shape** first.

---

## Decimation — why not filter at 768 kHz?

Airspy can deliver **hundreds of thousands** of samples per second. A 200 Hz FIR
at that rate would need enormous tap counts per second of CPU.

The chain **decimates** to ~12 kHz after the initial NCO shift:

```text
  768 kHz IQ  ──► LPF + drop samples ──► 12 kHz complex  ──► manageable FIR
```

Anti-aliasing lowpass before dropping samples prevents energy from far-away
frequencies folding into your passband — the decimator is part of **selectivity**,
not just a speed hack.

---

## SNR meter on the status bar

The listen chain tracks a **fast envelope peak** and a **slow noise floor** on
the filtered signal. Their ratio becomes **SNR dB** — useful for zero-beat (Z)
and comparing spots, not a lab-grade measurement.

---

## Listen chain vs skimmer channel

Your ears use the **full chain** above. The skimmer uses a **2-pole lowpass** per
peak so twenty signals decode in parallel. Do not expect skimmer copy quality to
match headphones — it is for **situational awareness** and spot lists, not contest
logging without verification.
