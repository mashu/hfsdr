# How the panadapter works

The spectrum and waterfall are a **short-time Fourier transform (STFT)** view of
IQ: “how much energy at each frequency **right now**?”

---

## Intuition: FFT as a bank of tone detectors

Take a few thousand IQ samples — a **snapshot in time**. The FFT asks: “If I
had a pure tone at bin *k*, how well does it match the data?” Strong match →
bright pixel at that frequency.

```text
  time domain IQ snapshot  ──FFT──►  one row of "power vs bin"
         (~few ms)                    (~thousands of bins)
```

Repeat every **hop** (half an FFT length in hfsdr) and stack rows → **waterfall**.

---

## dB scale

Power is converted to **decibels** so weak and strong signals fit one graph:

```text
  dB = 10 × log10(power) + calibration offsets
```

**Ref** and **range** in display settings slide the mapping — like adjusting
attenuation on a scope. They do not change what the radio receives.

---

## Auto FFT size (~8 Hz bins)

Bin width ≈ `sample_rate / fft_size`. hfsdr picks FFT size so bins are about
**8 Hz** wide at the effective rate:

| Effective rate | Typical FFT | Bin width |
|----------------|-------------|-----------|
| 12 kHz (Kiwi) | 2048 | ~6 Hz |
| 768 kHz (Airspy full) | 65536 | ~12 Hz |

Finer bins help **peak picking** and zero-beat; coarser bins cost less CPU.

You can fix FFT size manually when auto mode is off.

---

## Zoom-aware decimation (Airspy)

Computing a 768 kHz FFT when you are zoomed to 2 kHz of 40 m wastes CPU and
creates bins far finer than pixels can show.

When visible span < ~90% of IQ bandwidth:

```text
  1. Mix pan center to DC (NCO)
  2. Lowpass to ~1.25× visible span
  3. Decimate (drop samples)
  4. FFT at lower rate
```

```text
  Full rate:     |████████████████████████████████| 768 kHz
  Zoomed view:              |██| 2 kHz on screen
  After decim:              |██| effective ~5 kHz processed
```

You see the **same pixels**; the computer does less work. Kiwi often stays at
decimation 1 because the passband is already narrow.

---

## Pan and extract

The FFT row covers the **full** decimated bandwidth. **Pan** selects which slice
maps to the widget width. Changing pan does not retune the hardware — it scrolls
a window across the stored row.

---

## Smoothing vs skimmer

Trace **smoothing** blurs along frequency for a prettier line. The **skimmer**
reads **unsmoothed** rows for peak detection — do not enable heavy smooth and
expect peak positions to match labels exactly.

---

## Mental model checklist

| Question | Answer |
|----------|--------|
| Why does zoom feel “snappier” on Airspy? | Less data per FFT after decimation |
| Why do signals look wider when zoomed out? | Bins are wider in Hz |
| Does FFT filter my CW audio? | **No** — audio uses the separate listen chain |
