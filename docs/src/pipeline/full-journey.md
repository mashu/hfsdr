# The full signal journey

Follow one batch of IQ samples from the antenna to your ears and eyes.

---

## Stage 0 — Front end (hardware or Kiwi)

The radio digitizes a chunk of HF spectrum centered on **Fc** (your dial
frequency). Samples arrive continuously on a device thread.

**Design choice:** hfsdr never blocks that thread. Samples go into a **ring
buffer**; if the engine is slow, excess samples are **discarded** and counted.

---

## Stage 1 — Engine drain

The **engine thread** pulls a bounded number of samples per loop (so one heavy
FFT cannot freeze the app for seconds). From that block, three consumers run:

```text
  IQ block  →  Listen (CwChannel)  →  audio
            →  Spectrum (FFT)       →  waterfall
            →  Skimmer (peaks)      →  spot table
```

They share **input** but not **filters**. Widening the panadapter zoom does not
widen your CW filter.

---

## Stage 2 — Listen path (one station, best quality)

Order matters — each step prepares the next:

1. **Optional noise blanker** — kills wide impulses before they smear in narrow filters.
2. **NCO shift** — moves your **listen offset** (RIT) so the desired signal sits near DC.
3. **Decimation** — lowers sample rate (~12 kHz) so FIR filters are affordable.
4. **Optional manual notches** — surgical cuts at known birdies.
5. **Channel FIR** — the main **bandpass** (shape + width you set in the GUI).
6. **AGC or manual gain** — comfortable listening level.
7. **Product detector + BFO** — CW RF becomes keyed audio tone.
8. **Optional APF, auto-notch, NR, squelch** — polish and comfort.

See [How CW becomes audio](./cw-demodulation.md) for stages 5–7 in plain language.

---

## Stage 3 — Spectrum path (everyone at once)

1. **Plan** — given zoom span, decide whether to **decimate** before FFT (Airspy zoomed in).
2. **Front-end mix-down** — rotate pan center to DC, lowpass, decimate.
3. **Windowed FFT** — complex spectrum → magnitude → dB.
4. **Extract view** — map FFT bins to the pixels you see.
5. **Waterfall** — stack rows into a scrolling texture; optional spatial smooth.

See [How the panadapter works](./spectrum-and-waterfall.md).

---

## Stage 4 — Skimmer path (many stations, lighter DSP)

1. **Peak pick** on the latest spectrum row (SNR vs noise floor, min separation).
2. For each peak, spawn/maintain a **cheap channel**: NCO + decimate + 2-pole LPF.
3. **Envelope** feeds a **Morse decoder** (bigram beam search by default).
4. **Pattern + SCP** turn raw text into CQ / call / heard spots in the store.

The skimmer **trades fidelity for parallelism** — see [What the skimmer does](./../skimmer/contest-use.md).

---

## Stage 5 — GUI (display only)

The UI thread **never** processes IQ. It:

- Sends updated settings to the engine.
- **try_poll** copies the latest spectrum row, spots, stats (non-blocking).
- Draws widgets and handles mouse/keyboard.

If poll fails this frame, the display skips an update — the next frame catches up.

---

## Why this architecture?

| Goal | Mechanism |
|------|-----------|
| Responsive GUI | Engine off main thread; try_lock not block |
| Good CW copy | Long FIR listen chain at audio rate |
| Wide panadapter | FFT + zoom decimation |
| Many decodes | Lightweight skimmer channels |
| New radios | One IQ interface |

The journey is the same for Airspy and Kiwi; only stage 0 and sample rates change.
