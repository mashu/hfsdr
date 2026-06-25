# Adding another radio front end

New hardware should plug in at **one boundary** — the IQ source trait — without
forking the FFT or skimmer.

---

## Implement `IqSource`

Required capabilities:

1. List and set **sample rate**.
2. **Tune** center frequency (Hz).
3. **Start** streaming into provided `rtrb` producer; return consumer.
4. **Stop** and release resources.
5. Report **dropped** sample count on overload.

Contract details in `src/source.rs` module documentation.

---

## Real-time rules

The thread calling `push()` into the ring must:

- **Never block** on UI or disk.
- **Never allocate** per sample if avoidable.
- **Drop** on full ring and increment counter.

Violating this causes USB dropouts or Kiwi disconnects.

---

## Wire into the GUI

Add variant to connection UI in `bin/waterfall/source/connection.rs` (`ConnectRequest`).

Engine creates the source inside **engine thread** on `Connect` command — device
handles need not be `Send`.

---

## Test without hardware

Provide a **synthetic source** (test double) generating tone IQ — pattern used in
`tests/integration.rs` and skimmer engine tests.

---

## Document for operators

Add a short subsection to this book under Part I: sample rates, typical bandwidth,
known quirks (e.g. integer Hz tuning only).

---

## Checklist

- [ ] `IqSource` impl + error mapping
- [ ] Connect path in GUI
- [ ] Drop counter visible in stats when stressed
- [ ] Unit/integration test with synthetic IQ
- [ ] Book chapter or paragraph for operators
