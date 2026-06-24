# Why the UI never freezes

Perception of “hangs” usually means the **main thread blocked** waiting for DSP.
hfsdr treats that as a bug by design.

---

## Two threads you care about

<div data-diagram="thread-map"></div>

The main thread **never** waits on a mutex held while processing 100k samples.

---

## try_poll instead of lock

Older pattern: UI calls `lock()` → clone entire spot list + spectrum → unlock.
If engine holds lock during FFT, UI **stutters**.

Current pattern: `try_lock()` — if busy, **skip this frame’s update**. Waterfall
may pause one frame; interaction stays instant.

---

## Bounded work per engine tick

Each loop drains at most **MAX_DRAIN** IQ samples before returning to poll
commands and publish state. Prevents one giant backlog from monopolizing CPU.

---

## Background tasks

| Task | Where |
|------|--------|
| SCP HTTP download | Worker thread + channel |
| Settings save | Debounced, not every slider tick |
| Auto-reconnect | Engine thread with backoff |

---

## What still can stress CPU

- Skimmer **max decoders** high on wide Airspy view
- **History** waterfall texture
- Very high **target FPS**

Status bar **drops** and **slow link** warn before the UI freezes. Log panel
records connection errors.

---

## For contributors

Never call **blocking** engine APIs from `app.rs` ui(). Never process IQ in widgets.
If adding features, default to **engine thread** or **pure functions** called from UI.

See [Code layout](./code-layout.md).
