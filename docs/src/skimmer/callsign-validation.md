# MASTER.SCP and spot quality

Contest logging tools use **MASTER.SCP** (super check partial) — a dictionary of
valid callsigns maintained for N1MM+ and similar loggers. hfsdr uses the same
file to **validate** and **complete** skimmer output.

---

## Why not trust raw decode?

Morse decoders emit **plausible letter sequences** that are not callsigns. On a
noisy band you might see `EE5EEE` or `TEST` without SCP filtering.

SCP answers: **Is this a known call?** Can this prefix uniquely complete?

---

## Loaded vs heuristic mode

| Mode | Behavior |
|------|----------|
| **SCP loaded** | Strict validation; unique prefix completion |
| **SCP missing** | Heuristic patterns only — **more false positives** |
| **Require SCP match** (UI toggle) | Spots rejected unless SCP validates |

Download from the GUI (background fetch) or install N1MM+ copy; path shown in
status when loaded.

---

## What SCP does not do

- Fix bad timing decode — garbage in still garbage out.
- Know every special event call without updated file.
- Replace operator judgment for your log.

---

## Prefix filter vs SCP

**Call filter** in UI (`G`, `DL`, …) filters **display** by prefix string.

**SCP** validates **existence** in database. Both can combine: continent filter +
CQ only + SCP for a clean CQ DX table.

---

## Reload after download

After successful download, engine **reloads** SCP without restart. Log panel shows
path and errors.

---

## For developers

SCP parsing lives in the library (`skimmer/scp`); patterns module calls into it
from `analyze()`. Tests use small fixture files — extend when changing validation rules.
