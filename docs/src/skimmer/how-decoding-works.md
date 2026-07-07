# From Morse elements to callsigns

Skimmer decoders turn **envelope** (key-down vs key-up) into **characters**, then
pattern matching turns characters into **spots**.

---

## Channel front end

Each detected peak gets its own narrowband receive chain: the IQ stream is
mixed to baseband at the peak frequency, decimated with anti-alias filtering
to ~2 kHz, run through a narrow Gaussian filter (the *Channel LPF*), and
rectified into a ~500 Hz envelope stream. A slow AFC keeps the narrow filter
centred on the carrier even when the spectrum peak bin is coarse. Narrow
filtering plus the low envelope rate is what lets weak stations decode and
keeps neighbours out of the channel.

---

## Envelope and timing

After the channel filter, the signal is **amplitude vs time**:

```text
  envelope
    ▲
    │ ┌──┐     ┌──────┐
    │ │  └─────┘      └───
    └──────────────────────► time
      dot   dash   gap
```

Key-down decisions come from an adaptive Schmitt trigger: the tracker follows
the noise floor and key-down level separately (fast enough to ride through
QSB fades) and places hysteresis thresholds between them. A debouncing keyer
then bridges brief dropouts inside a dash and drops noise blips inside a gap
before any timing is measured.

The decoder estimates **dot length** with a two-cluster fit over recent mark
durations — it locks onto a sender's speed within a few characters, from
about 8 to 60 WPM — and classifies gaps:

| Gap length (approx.) | Meaning |
|----------------------|---------|
| ~1 dot | within same character |
| ~3 dots | between letters |
| ~7 dots | between words |

Speed adapts continuously — operators change keying; the cluster fit follows.

---

## Bigram beam search (default)

Morse is **ambiguous** when dots run together (`·` vs `·` boundary). A single
greedy decode confuses `E`/`T`/`I` chains.

**Bigram decoder** keeps multiple hypotheses scored by:

1. **Timing fit** — does this element length match dot/dash?
2. **Language model** — common letter pairs in English/CW (bigrams).

```text
  elements:  · − · ·   ?
  hypotheses compete until word boundary or timeout
  winner → "CQ" or "G0ABC" fragment
```

Better on **calls and CQ** than random letters; still fails on heavy QRM same frequency.

---

## Adaptive decoder (alternative)

Finite-state machine with simpler adaptive dot tracking — lighter, used in tests
and as reference. Production skimmer prefers **bigram** quality.

---

## Pattern classification

Once text accumulates (`CQ DL1ABC DE DL1ABC` fragments, etc.):

| Pattern | Spot kind |
|---------|-----------|
| Contains CQ, valid call | **Calling CQ** |
| `DE` + valid call | **Answering** |
| Valid call only | **Heard** |

A callsign is only accepted with corroboration — a repeat of the call, or
CQ/QRZ/DE context — whether or not MASTER.SCP is loaded. Without that,
random Morse fragments become false callsigns.

---

## Text buffer limits

Decoders cap stored text length; old characters drop. Very slow CQ may truncate —
increase channel timeout if experimenting (advanced).

---

## Failure modes you will see

| Output | Likely cause |
|--------|--------------|
| `E E E E` | Noise peak, not CW |
| Partial call `G0A` | Weak, overlapping, or speed mismatch |
| Wrong call one letter off | Bigram chose wrong path — verify on air |
| Nothing | SNR/separation too strict, or SCP rejected |

Use **MASTER.SCP** when you want stricter callsign acceptance — next chapter.
