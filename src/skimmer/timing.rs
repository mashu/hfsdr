//! Run-length keying support for the Bayesian skimmer decoder.
//!
//! [`Keyer`] converts a per-sample key-down decision into debounced mark/space
//! events: key flips shorter than a short glitch window are treated as noise,
//! so a level-crossing relic inside a dash does not split it and a noise blip
//! inside a space does not create a phantom dit. [`SpaceKind`] names the gap
//! classes the timing model distinguishes.

/// Committed keying event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyEvent {
    /// Completed key-down run (seconds), QSB dropouts bridged.
    Mark(f32),
    /// Completed key-up run (seconds), noise blips removed.
    Space(f32),
}

/// Debouncing run-length extractor.
#[derive(Clone, Debug)]
pub struct Keyer {
    sample_rate: f32,
    glitch_samples: u32,
    started: bool,
    in_mark: bool,
    /// Samples spent in the committed phase (includes the provisional tail).
    run: u32,
    /// Consecutive samples contradicting the committed phase.
    prov: u32,
}

impl Keyer {
    pub fn new(sample_rate: f32) -> Self {
        let mut k = Self {
            sample_rate: sample_rate.max(1.0),
            glitch_samples: 1,
            started: false,
            in_mark: false,
            run: 0,
            prov: 0,
        };
        k.set_dot_seconds(0.05);
        k
    }

    /// Update the glitch window from the current dit estimate.
    pub fn set_dot_seconds(&mut self, dot_s: f32) {
        let glitch_s = (0.28 * dot_s).clamp(0.004, 0.018);
        self.glitch_samples = ((glitch_s * self.sample_rate) as u32).max(1);
    }

    /// True once the first mark has been committed and the key is now up.
    pub fn in_space(&self) -> bool {
        self.started && !self.in_mark
    }

    /// Current key-up run in seconds (0.0 while a mark is in progress).
    pub fn space_seconds(&self) -> f32 {
        if self.in_space() {
            self.run as f32 / self.sample_rate
        } else {
            0.0
        }
    }

    /// Feed one key-state sample; returns a committed event on phase changes.
    pub fn step(&mut self, key_down: bool) -> Option<KeyEvent> {
        if !self.started {
            if key_down {
                self.prov += 1;
                if self.prov > self.glitch_samples {
                    self.started = true;
                    self.in_mark = true;
                    self.run = self.prov;
                    self.prov = 0;
                }
            } else {
                self.prov = 0;
            }
            return None;
        }
        self.run += 1;
        if key_down != self.in_mark {
            self.prov += 1;
            if self.prov > self.glitch_samples {
                let dur = self.run.saturating_sub(self.prov) as f32 / self.sample_rate;
                let ev = if self.in_mark {
                    KeyEvent::Mark(dur)
                } else {
                    KeyEvent::Space(dur)
                };
                self.in_mark = !self.in_mark;
                self.run = self.prov;
                self.prov = 0;
                return Some(ev);
            }
        } else {
            self.prov = 0;
        }
        None
    }

    pub fn reset(&mut self) {
        self.started = false;
        self.in_mark = false;
        self.run = 0;
        self.prov = 0;
    }
}

/// Space classification relative to the dit clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpaceKind {
    /// Inter-element gap (~1 dit).
    Element,
    /// Inter-character gap (~3 dits).
    Char,
    /// Inter-word gap (~7 dits).
    Word,
}


#[cfg(test)]
mod tests {
    use super::*;

    fn feed(keyer: &mut Keyer, down: bool, n: usize, out: &mut Vec<KeyEvent>) {
        for _ in 0..n {
            if let Some(ev) = keyer.step(down) {
                out.push(ev);
            }
        }
    }

    #[test]
    fn keyer_emits_clean_runs() {
        let mut k = Keyer::new(1000.0);
        k.set_dot_seconds(0.048);
        let mut evs = Vec::new();
        feed(&mut k, false, 200, &mut evs);
        feed(&mut k, true, 48, &mut evs); // dit
        feed(&mut k, false, 48, &mut evs);
        feed(&mut k, true, 144, &mut evs); // dah
        feed(&mut k, false, 200, &mut evs);
        feed(&mut k, true, 48, &mut evs);
        assert!(matches!(evs[0], KeyEvent::Mark(d) if (d - 0.048).abs() < 0.005), "{evs:?}");
        assert!(matches!(evs[1], KeyEvent::Space(d) if (d - 0.048).abs() < 0.005), "{evs:?}");
        assert!(matches!(evs[2], KeyEvent::Mark(d) if (d - 0.144).abs() < 0.005), "{evs:?}");
    }

    #[test]
    fn keyer_bridges_qsb_dropout_in_dash() {
        let mut k = Keyer::new(1000.0);
        k.set_dot_seconds(0.048);
        let mut evs = Vec::new();
        feed(&mut k, true, 60, &mut evs);
        feed(&mut k, false, 8, &mut evs); // 8 ms dropout — below glitch window
        feed(&mut k, true, 76, &mut evs);
        feed(&mut k, false, 100, &mut evs);
        feed(&mut k, true, 30, &mut evs); // force the mark event out
        let marks: Vec<f32> = evs
            .iter()
            .filter_map(|e| match e {
                KeyEvent::Mark(d) => Some(*d),
                _ => None,
            })
            .collect();
        assert_eq!(marks.len(), 1, "{evs:?}");
        assert!((marks[0] - 0.144).abs() < 0.01, "dash split by dropout: {evs:?}");
    }

    #[test]
    fn keyer_drops_noise_blip_in_space() {
        let mut k = Keyer::new(1000.0);
        k.set_dot_seconds(0.048);
        let mut evs = Vec::new();
        feed(&mut k, true, 48, &mut evs);
        feed(&mut k, false, 70, &mut evs);
        feed(&mut k, true, 6, &mut evs); // 6 ms blip
        feed(&mut k, false, 70, &mut evs);
        feed(&mut k, true, 48, &mut evs);
        feed(&mut k, false, 40, &mut evs);
        let spaces: Vec<f32> = evs
            .iter()
            .filter_map(|e| match e {
                KeyEvent::Space(d) => Some(*d),
                _ => None,
            })
            .collect();
        assert_eq!(spaces.len(), 1, "{evs:?}");
        assert!((spaces[0] - 0.146).abs() < 0.012, "blip split the space: {evs:?}");
    }

}
