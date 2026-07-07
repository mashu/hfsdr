//! Run-length keying analysis shared by the skimmer decoders.
//!
//! [`Keyer`] converts a per-sample key-down decision into debounced mark/space
//! events: key flips shorter than a glitch window (a fraction of a dit) are
//! treated as noise, so a QSB dropout inside a dash does not split it and a
//! noise blip inside a space does not create a phantom dit.
//!
//! [`ElementClock`] classifies mark durations into dits and dahs with a
//! two-cluster fit over a sliding window of recent marks — the same idea used
//! by fldigi's dit/dah histogram and CW Skimmer's statistical timing — which
//! locks onto keying speed within a few characters and tracks 8–60 WPM.

use std::collections::VecDeque;

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
        let glitch_s = (0.25 * dot_s).clamp(0.004, 0.015);
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

/// Element classification result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Dot,
    Dash,
}

impl Element {
    pub fn as_char(self) -> char {
        match self {
            Element::Dot => '.',
            Element::Dash => '-',
        }
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

/// 8 WPM.
const MAX_DOT_S: f32 = 0.15;
/// 60 WPM.
const MIN_DOT_S: f32 = 0.02;
/// Marks kept for the two-cluster fit.
const HISTORY: usize = 24;
/// max/min ratio above which the window clearly holds both dits and dahs.
const TWO_CLUSTER_RATIO: f32 = 1.9;
/// Space boundaries in dits: geometric means of (1,3) and (3,7).
pub const CHAR_SPACE_DITS: f32 = 1.9;
pub const WORD_SPACE_DITS: f32 = 4.6;

/// Adaptive dit/dah duration tracker.
#[derive(Clone, Debug)]
pub struct ElementClock {
    dot_s: f32,
    dash_s: f32,
    marks: VecDeque<f32>,
}

impl ElementClock {
    pub fn new(initial_wpm: f32) -> Self {
        let dot = (1.2 / initial_wpm.max(1.0)).clamp(MIN_DOT_S, MAX_DOT_S);
        Self {
            dot_s: dot,
            dash_s: 3.0 * dot,
            marks: VecDeque::with_capacity(HISTORY),
        }
    }

    pub fn dot_seconds(&self) -> f32 {
        self.dot_s
    }

    pub fn dash_seconds(&self) -> f32 {
        self.dash_s
    }

    /// Dit/dah decision boundary in seconds.
    pub fn mark_boundary_s(&self) -> f32 {
        (self.dot_s * self.dash_s).sqrt()
    }

    /// Record a mark duration and classify it.
    pub fn classify_mark(&mut self, secs: f32) -> Element {
        if self.marks.len() == HISTORY {
            self.marks.pop_front();
        }
        self.marks.push_back(secs);
        self.refit();
        if secs < self.mark_boundary_s() {
            Element::Dot
        } else {
            Element::Dash
        }
    }

    /// Classify a space duration against the current dit clock.
    pub fn space_kind(&self, secs: f32) -> SpaceKind {
        if secs < CHAR_SPACE_DITS * self.dot_s {
            SpaceKind::Element
        } else if secs < WORD_SPACE_DITS * self.dot_s {
            SpaceKind::Char
        } else {
            SpaceKind::Word
        }
    }

    /// Two-cluster fit over the recent-mark window.
    ///
    /// When the window spans both dits and dahs (max/min ratio ≥ ~1.9) the
    /// clusters are split at the geometric mean of the extremes and each
    /// estimate becomes its cluster's mean. With a single cluster present, the
    /// mean nudges whichever estimate it is closer to in log distance, so the
    /// clock still tracks speed drift from an all-dit or all-dah stretch.
    fn refit(&mut self) {
        let (mut mn, mut mx) = (f32::MAX, 0.0f32);
        for &m in &self.marks {
            mn = mn.min(m);
            mx = mx.max(m);
        }
        if mn <= 0.0 || mx <= 0.0 {
            return;
        }
        if mx / mn >= TWO_CLUSTER_RATIO {
            let split = (mn * mx).sqrt();
            let (mut lo_sum, mut lo_n, mut hi_sum, mut hi_n) = (0.0f32, 0u32, 0.0f32, 0u32);
            for &m in &self.marks {
                if m < split {
                    lo_sum += m;
                    lo_n += 1;
                } else {
                    hi_sum += m;
                    hi_n += 1;
                }
            }
            if lo_n > 0 {
                self.dot_s = lo_sum / lo_n as f32;
            }
            if hi_n > 0 {
                self.dash_s = hi_sum / hi_n as f32;
            }
        } else {
            let mean = self.marks.iter().sum::<f32>() / self.marks.len() as f32;
            let to_dot = (mean / self.dot_s).ln().abs();
            let to_dash = (mean / self.dash_s).ln().abs();
            if to_dot <= to_dash {
                self.dot_s += 0.3 * (mean - self.dot_s);
            } else {
                self.dash_s += 0.3 * (mean - self.dash_s);
            }
        }
        self.dot_s = self.dot_s.clamp(MIN_DOT_S, MAX_DOT_S);
        self.dash_s = self
            .dash_s
            .clamp(2.0 * self.dot_s, (4.5 * self.dot_s).min(6.0 * MAX_DOT_S));
    }

    pub fn reset(&mut self, initial_wpm: f32) {
        *self = Self::new(initial_wpm);
    }
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

    #[test]
    fn clock_locks_onto_speed_from_cold_start() {
        // Start assuming 22 WPM, receive 40 WPM (dit 30 ms, dah 90 ms).
        let mut c = ElementClock::new(22.0);
        let pattern = [0.03, 0.09, 0.03, 0.03, 0.09, 0.03, 0.09, 0.09, 0.03];
        let mut wrong = 0;
        for (i, &m) in pattern.iter().cycle().take(30).enumerate() {
            let el = c.classify_mark(m);
            let want = if m < 0.06 { Element::Dot } else { Element::Dash };
            if i >= 4 && el != want {
                wrong += 1;
            }
        }
        assert_eq!(wrong, 0, "misclassified after lock: dot={}", c.dot_seconds());
        assert!((c.dot_seconds() - 0.03).abs() < 0.008);
    }

    #[test]
    fn clock_tracks_slow_sender() {
        // 12 WPM: dit 100 ms, dah 300 ms.
        let mut c = ElementClock::new(25.0);
        for _ in 0..8 {
            c.classify_mark(0.10);
            c.classify_mark(0.30);
        }
        assert!(matches!(c.classify_mark(0.10), Element::Dot));
        assert!(matches!(c.classify_mark(0.30), Element::Dash));
        assert!((c.dot_seconds() - 0.10).abs() < 0.02);
    }

    #[test]
    fn space_kinds_follow_dit_clock() {
        let c = ElementClock::new(24.0); // dit = 50 ms
        assert_eq!(c.space_kind(0.05), SpaceKind::Element);
        assert_eq!(c.space_kind(0.15), SpaceKind::Char);
        assert_eq!(c.space_kind(0.40), SpaceKind::Word);
    }
}
