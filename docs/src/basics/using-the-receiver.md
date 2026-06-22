# Using the receiver

This chapter is the **operator’s tour** of the hfsdr window: what to click, what
to expect, and which controls interact.

---

## First launch

1. Start `hfsdr` (release binary or `cargo run --features gui --bin hfsdr`).
2. Choose **Airspy** or **Kiwi**, enter host/port if needed, set **center
   frequency** (MHz), click **Connect**.
3. Wait for **Streaming** in the status bar. The waterfall fills from the top.

If connect fails, open the **Log** panel (status bar **Log** or `` ` `` key) —
errors land there instead of spamming the terminal.

---

## Main areas of the window

```text
┌─ status: link, SNR, drops, toggles ─────────────────────────┐
│ left panel          │  spectrum (live trace)                 │
│ · connection        │  waterfall (time ↓, frequency →)       │
│ · recent hosts      │  cyan edges = your listen passband     │
│ · spots table       │                                        │
│                     │                                        │
│ right panel         │                                        │
│ · CW demod / filters│                                        │
│ · skimmer settings  │                                        │
│ · audio / display   │                                        │
└─────────────────────┴────────────────────────────────────────┘
 optional bottom: history · log console (hidden by default)
```

| Area | Purpose |
|------|---------|
| **Spectrum** | Instant power vs frequency — find signals |
| **Waterfall** | Same data over time — see CQ activity patterns |
| **Cyan passband** | Where the **listen** chain is focused |
| **Spot table** | Skimmer results — click a row to tune |
| **Right panel → CW demod** | BFO pitch, filter BW and **shape**, wide/CW mode |

---

## Tuning: mouse and keyboard

| Action | Effect |
|--------|--------|
| **Click** on spectrum/waterfall | Move **hardware center** to that frequency |
| **Double-click** | Center on click point and reset pan |
| **Drag** (zoomed in) | **Pan** left/right across the band (no retune) |
| **Drag** (full span) | **Retune** by dragging (scrolls dial with mouse) |
| **Shift + drag** | Pan (also when zoomed) |
| **Scroll** | Zoom frequency axis in/out |
| **Ctrl + scroll** on plot | Widen/narrow **audio passband** |
| **Drag cyan edges** | Resize passband (respects CW vs wide limit) |
| **, / .** | RIT down/up (move listen offset) |
| **[ / ]** | Narrow/widen passband |
| **Z** | Zero-beat (align listen to strongest peak near center) |
| **L** | Pitch lock |
| **Space** | Mute/unmute audio |

Three different concepts — **center frequency**, **pan**, and **RIT** — are
easy to confuse. See [Zoom, pan, and RIT](./pipeline/tuning-explained.md).

---

## CW filter controls (right panel)

**CW (≤500 Hz) vs Wide (≤2 kHz)** — contest CW uses narrow slots; wide mode
allows broader presets for unusual situations.

**BW presets and slider** — audio bandwidth after demod. Narrower = less bleed
from adjacent stations, but you must stay “on frequency”.

**Shape: Gauss / RaisedCos / Blackman** — how aggressively energy **outside**
the passband is rejected. On a **crowded band with one signal in your passband**,
try **Blackman** and ≤200–300 Hz before reaching for notches.

**BFO** — pitch of the sidetone in your headphones (typically 500–800 Hz).
Does not change what you transmit; only what you hear.

---

## Skimmer (optional)

Enable in the right/left panel depending on layout. Useful during contests:

1. Set **Peak min SNR** and **separation** so one signal = one decoder.
2. Load or **Download MASTER.SCP** for trustworthy callsigns.
3. Use **Call filter**, **CQ only**, and **continent filter** to thin the table.
4. **Clear spots** when the band changes.

Plot labels dedupe by **bucket** so twenty decoders on one frequency do not
paint twenty overlapping tags.

---

## When copy sounds wrong

| Symptom | Likely cause | Try |
|---------|--------------|-----|
| Wrong pitch / chipmunk | Audio rate mismatch (should not happen in normal use) | Reconnect; check Log |
| Adjacent CW audible | Passband too wide or soft filter shape | Blackman, narrower BW |
| Thumps on lightning | Impulse noise | Enable **noise blanker** |
| Steady whistle near pitch | Birdie | Manual **notch** or **auto-notch** |
| UI stutters | CPU overload | Fewer skimmer channels, lower FPS |

Detailed DSP reasoning is in Part III (filters) and Part II (CW demod).

---

## Settings persistence

Most controls save to `~/.config/hfsdr/settings.json` after ~1 s of stability.
Recent hosts and last center frequency are remembered for quick reconnect.
