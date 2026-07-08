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

The default **Bayesian decoder** treats this stream as a hidden Markov model
with two states — key-down and key-up. Instead of a threshold crossing, every
sample contributes *evidence*: how likely is this level under the current
mark-level estimate vs the noise-level estimate? Both levels are re-estimated
continuously from the state posterior (online EM), so there is nothing to
tune per band — the model follows the signal through QSB, drifting noise
floors and level changes on its own. A short **fixed-lag smoother** lets each
key decision see ~24 ms of future evidence, so a brief noise dip inside a
dash is outvoted by its surroundings instead of splitting the element.

Timing is a mixture model: mark durations belong to *dit*, *dah* or *outlier*
components; gaps to *element*, *character*, *word* or *outlier*. Classifying
an element is reading off the component posterior, and every event also
adapts the dit period and gap centres by responsibility-weighted updates —
flutter fragments land in the outlier class instead of dragging the clock.
Speed jumps (a new sender, QRQ mid-QSO) are caught by periodic
likelihood-based **model selection** over candidate dit periods from about
8 to 60 WPM:

| Gap cluster | Meaning |
|-------------|---------|
| ~1 dot | within same character |
| shorter gap cluster | between letters |
| longer gap cluster | between words |

An **evidence gate** accumulates how well recent marks *and* gaps fit the
Morse mixture: random noise or an unintelligible pileup fits poorly, so the
decoder emits nothing instead of spraying `E T E E` garbage.

Decoding is focused around the tuned frequency (default ±1.5 kHz, the
*Decode span* setting; 0 decodes the whole band) so CPU stays bounded no
matter how wide the input is. The audible receive chain is completely
separate from the skimmer's decode chain — decoder settings never change
what you hear.

---

## Beam search and the language prior

Morse is **ambiguous** when dots run together (`·` vs `·` boundary). A single
greedy decode confuses `E`/`T`/`I` chains.

The Bayesian and Bigram decoders keep multiple hypotheses scored by:

1. **Timing fit** — the duration posterior of each element reading.
2. **Language model** — common letter pairs in CW traffic (bigrams, biased
   toward CQ/DE and callsign shapes).

```text
  elements:  · − · ·   ?
  hypotheses compete until word boundary or timeout
  winner → "CQ" or "G0ABC" fragment
```

Better on **calls and CQ** than random letters; still fails on heavy QRM same frequency.

---

## Alternative decoders

**Bigram beam** is the previous default: an adaptive Schmitt trigger with a
debouncing keyer and a percentile two-cluster dot fit, feeding the same beam
search. Solid, but its key thresholds are fixed fractions of the tracked
span and can want manual tuning on hard bands.

**Adaptive** is a plain finite-state machine with the same front end —
lightest CPU, used in tests and as a reference.

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
| Wrong call one letter off | Beam chose wrong path — verify on air |
| Nothing | SNR/separation too strict, or SCP rejected |

Use **MASTER.SCP** when you want stricter callsign acceptance — next chapter.
