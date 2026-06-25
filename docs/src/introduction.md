# Introduction

**hfsdr** is a desktop HF receiver built for **CW contest work**: you see the
whole band at once, listen to one station with a sharp filter, and optionally
run a **skimmer** that copies many callers in parallel.

This book is **not** an API manual. The Rust reference (`cargo doc`) lists types
and functions; here we explain **what happens to your signal**, **what the controls
do**, and **why the software is built the way it is**.

---

## Who this is for

**Operators (SO5KM and friends on 40 m at 0300 Z)**  
Start with [What IQ data is](./basics/iq-and-sdr.md), then [Using the receiver](./basics/using-the-receiver.md).
When a adjacent signal bleeds into your filter, read [Channel filter shapes](./filters/channel-shapes.md).

**Developers**  
Skim Part I for domain vocabulary, read Part II–IV for algorithms, then Part V
for threads and module boundaries before opening a PR.

---

## What makes hfsdr different from a classic receiver

A traditional HF rig gives you one filter, one BFO, one VFO. A panadapter adds a
spectrum display but often still feeds **one** audio path.

hfsdr splits the work into **parallel paths** from the same IQ stream:

```text
                    IQ stream (one ring buffer)
                              |
        +---------------------+---------------------+
        |                     |                     |
    Listen path          Panadapter             Skimmer
  CwChannel → audio    FFT → waterfall      parallel decoders
```

That split is the central design idea: **looking** at the band must not degrade
**listening**, and **copying** twenty CQs must not stall the waterfall.

---

## Supported hardware

| | Linux | macOS | Windows |
|---|:---:|:---:|:---:|
| **KiwiSDR** | ✓ | ✓ | ✓ |
| **Airspy HF+** | ✓ | ✓ | ✓ |
| **RTL-SDR** | ✓ | ✓ | ✓ |
| **QMX / QMX+** | ✓ | ✓ | ✓ |

| Radio | Typical use |
|-------|-------------|
| **Airspy HF+** | Local USB, up to hundreds of kHz IQ — wide contest views |
| **RTL-SDR** | Local USB, common dongles — budget panadapter |
| **KiwiSDR** | Remote WebSocket, ~12 kHz passband — network receivers |
| **QMX / QMX+** | USB CAT + audio IQ — QRP Labs transceiver |

All sources speak the same internal language (complex IQ samples). Adding another
SDR means implementing one interface, not rewriting the FFT or the skimmer.

Build instructions: [Building hfsdr](./building.md).

---

## How to read the book

| If you want to… | Read |
|-----------------|------|
| Connect and operate the GUI | [Using the receiver](./basics/using-the-receiver.md) |
| Understand dots and dashes in the speaker | [How CW becomes audio](./pipeline/cw-demodulation.md) |
| Pick a filter on a crowded band | [Why filters matter](./filters/why-filters-matter.md) → [Channel shapes](./filters/channel-shapes.md) |
| Trust skimmer callsigns | [MASTER.SCP](./skimmer/callsign-validation.md) |
| Hack on the codebase | [Code layout](./architecture/code-layout.md) |
| Build from source | [Building hfsdr](./building.md) |

---

## A note on units

Throughout the book:

- **Frequency** is in Hz unless written as kHz (7030 kHz = 7 030 000 Hz).
- **Offset** is relative to the tuned center: +500 Hz means 500 Hz above the
  dial frequency.
- **Bandwidth (BW)** is the width of the audio passband around your listen
  point, not the RF front-end width.
