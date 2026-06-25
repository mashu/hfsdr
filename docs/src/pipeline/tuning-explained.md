# Zoom, pan, and RIT — three different “tuning” knobs

Operators confuse these three because all move energy on the display. They control
**different parts** of the system.

---

## Center frequency (hardware tune)

**What moves:** The RF front end (Airspy LO or Kiwi channel).

**What you hear:** Everything shifts — all stations slide on the dial.

**Typical use:** Jump to 7030 kHz, follow a DX station across the band when
they QSY.

**Mouse:** Click spectrum/waterfall to retune here.

```text
  Before click at +1 kHz offset     After click
  center = 7030                     center = 7031
  station was +1 kHz                station now at center
```

---

## Pan (display scroll)

**What moves:** Only the **viewport** over the FFT data.

**What you hear:** **Nothing** — listen chain unchanged.

**Typical use:** Examine activity above/below center without QSY during a run.

**Mouse:** Drag on the plot when zoomed in (scroll wheel first if needed). Shift+drag also pans.

---

## RIT / listen offset

**What moves:** The **software NCO** and passband overlay (cyan), relative to
hardware center.

**What you hear:** The **filter and BFO** follow — you copy a station offset
from center without moving the radio’s LO.

**Typical use:** Center on a pileup, RIT to each caller; or split when TX/RX
offsets differ (depending on your station setup).

**Keys:** `,` and `.` — also drag the center grab when implemented.

```text
  Hardware center:     7030.000 kHz
  RIT:                 +350 Hz
  Listen point:        7030.350 kHz  ← CW filter centered here
```

Passband **edge drags** use the listen point, not raw hardware center — so RIT
and filter editing stay consistent.

---

## Ctrl+scroll passband width

**What moves:** Audio **bandwidth** (FIR passband width).

**What you see:** Cyan region widens/narrows.

**What you hear:** More or less adjacent signal rejection — not the same as zoom.

---

## Quick reference

| Control | Retunes radio? | Changes audio filter? | Changes view only? |
|---------|----------------|----------------------|-------------------|
| Click plot | Yes | Indirectly (center moves) | Yes |
| Drag (zoomed) | No | No | Yes |
| Drag (full span) | Yes | No | Yes |
| RIT | No | Yes (follows listen) | Yes (overlay) |
| Zoom scroll | No | No | Yes |
| Ctrl+scroll | No | Yes | Yes (overlay) |

When adjacent CW bleeds in, you usually want **narrower BW / Blackman**, not
just zoom — zoom is for your eyes, BW is for your ears.
