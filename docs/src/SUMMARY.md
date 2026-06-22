# Summary

[Introduction](./introduction.md)

---

# Part I — Concepts and operation

- [What IQ data is (and why SDR needs it)](./basics/iq-and-sdr.md)
- [Using the receiver](./basics/using-the-receiver.md)
- [Reading the spectrum and waterfall](./basics/reading-the-display.md)

---

# Part II — From RF to sound and pixels

- [The full signal journey](./pipeline/full-journey.md)
- [How CW becomes audio you can copy](./pipeline/cw-demodulation.md)
- [How the panadapter works](./pipeline/spectrum-and-waterfall.md)
- [Zoom, pan, and RIT — three different “tuning” knobs](./pipeline/tuning-explained.md)

---

# Part III — Filters and interference

- [Why bandwidth and filter shape matter](./filters/why-filters-matter.md)
- [The channel filter: Gaussian, RaisedCos, Blackman](./filters/channel-shapes.md)
- [Optional stages: blanker, AGC, NR, and the rest](./filters/optional-stages.md)
- [Notches and birdies](./filters/notches-and-qrm.md)

---

# Part IV — The contest skimmer

- [What the skimmer does on a busy band](./skimmer/contest-use.md)
- [From Morse elements to callsigns](./skimmer/how-decoding-works.md)
- [MASTER.SCP and spot quality](./skimmer/callsign-validation.md)

---

# Part V — For developers

- [Why the UI never freezes](./architecture/responsiveness.md)
- [How the code is organized](./architecture/code-layout.md)
- [Adding another radio front end](./architecture/extending-sources.md)
- [Contributing](./contributing.md)
- [Building this book and API docs](./building-docs.md)
