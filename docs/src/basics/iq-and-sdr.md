# What IQ data is (and why SDR needs it)

Before any filter or decoder runs, the radio delivers **IQ samples**: pairs of
numbers describing a slice of RF spectrum as a **rotating phasor** in the
complex plane.

You do not need a math degree to use hfsdr, but this picture explains every
later chapter.

---

## From RF to numbers

At the antenna, energy at many frequencies is mixed down to **baseband** inside
the SDR. The result is two time-varying voltages, **I** (in-phase) and **Q**
(quadrature):

```text
  RF band at center frequency Fc
           │
           ▼  (mixer + ADC)
  stream of (I, Q) pairs  ──►  software
```

Each pair is one **complex sample**. The **sample rate** says how many pairs
arrive per second — e.g. 12 000/s for Kiwi, up to 768 000/s for Airspy at full
rate.

**Why two channels?** One real-only stream would lose whether energy sits above
or below the center frequency. I and Q together preserve **side** (USB vs LSB
relative to center) and allow precise **frequency shifting** in software — that
is how RIT and the skimmer move to individual stations without retuning the
hardware for each one.

---

## What one CW signal looks like in IQ

Imagine you are tuned to 7.030 MHz and a station transmits CW 500 Hz above
center (on 7.0305 MHz). After mixing, their carrier is a **steady tone at
+500 Hz offset** — a slow spiral in the IQ plane. When they key down, the
amplitude jumps; when they key up, it falls.

```text
  frequency axis (relative to dial)
  ─────────────────────────────────────────►
        │                    │
     noise floor          +500 Hz CW carrier
                              (pulses when keyed)
```

The panadapter FFT turns a block of IQ into **how much energy sits at each
offset**. The listen chain **shifts** one offset to DC, **filters** everything
else away, then **demodulates** to audio.

---

## Sample rate vs what you see

| Source | Approx. IQ bandwidth | Implication |
|--------|----------------------|-------------|
| KiwiSDR | ~12 kHz | Whole passband fits one FFT; zoom is mostly visual |
| Airspy HF+ | Up to ~600+ kHz usable | Full-span FFT is expensive; zoom triggers real decimation |

Higher sample rate does **not** automatically mean better CW copy — it means
**more spectrum visible at once**. Copy quality comes from the **listen-chain**
filters after the signal is isolated.

---

## Drops and backpressure

IQ arrives on a **real-time thread** (USB callback or network reader). The
engine consumes samples in a loop. If the engine falls behind, **new samples
are dropped** rather than blocking the radio thread.

You may see **drops** in the status bar. That is intentional: a frozen USB
driver is worse than a momentary glitch. Persistent drops mean the CPU cannot
keep up — reduce skimmer channels, disable history, or zoom the spectrum less.

---

## Takeaway

**IQ = many frequencies measured at once as complex numbers.**  
Everything in hfsdr — waterfall, audio, skimmer — is a different **view** or
**processing path** on that same stream. The next chapter assumes IQ exists and
focuses on what you click in the GUI.
