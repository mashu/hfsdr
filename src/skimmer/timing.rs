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
/// All recent marks (including rejected fragments) for fast-sender reseed.
const RECENT_ALL: usize = 12;
/// Spaces kept for the char/word gap fit (in dit units).
const SPACE_HISTORY: usize = 16;
/// p85/p15 ratio above which the mark window clearly holds dits and dahs.
const TWO_CLUSTER_RATIO: f32 = 1.9;
/// Marks below this fraction of a dit are flutter fragments — classified as
/// noise, kept out of the cluster fit so they cannot drag the clock down.
const FRAGMENT_FRAC: f32 = 0.4;

fn percentile(sorted: &[f32], q: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * q) as usize]
}

fn median_of(values: &[f32]) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Adaptive dit/dah duration tracker with robust two-cluster fitting for both
/// mark lengths (dit vs dah) and gap lengths (char vs word).
#[derive(Clone, Debug)]
pub struct ElementClock {
    dot_s: f32,
    dash_s: f32,
    marks: VecDeque<f32>,
    recent_all: VecDeque<f32>,
    /// Non-element spaces in dit units — the char/word cluster window.
    spaces_dits: VecDeque<f32>,
    char_gap_dits: f32,
    word_gap_dits: f32,
    /// How well recent marks cluster around dit/dah (0–1). Random noise
    /// keying does not cluster; emission is suppressed while low.
    confidence: f32,
}

impl ElementClock {
    pub fn new(initial_wpm: f32) -> Self {
        let dot = (1.2 / initial_wpm.max(1.0)).clamp(MIN_DOT_S, MAX_DOT_S);
        Self {
            dot_s: dot,
            dash_s: 3.0 * dot,
            marks: VecDeque::with_capacity(HISTORY),
            recent_all: VecDeque::with_capacity(RECENT_ALL),
            spaces_dits: VecDeque::with_capacity(SPACE_HISTORY),
            char_gap_dits: 3.0,
            word_gap_dits: 7.0,
            confidence: 0.5,
        }
    }

    /// True while recent mark timing is coherent enough to trust the output.
    pub fn is_confident(&self) -> bool {
        self.confidence > 0.55 && self.marks.len() >= 4
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

    /// Inter-element / inter-character boundary in dits.
    fn char_boundary_dits(&self) -> f32 {
        self.char_gap_dits.sqrt().clamp(1.6, 2.6)
    }

    /// Inter-character / inter-word boundary in dits.
    fn word_boundary_dits(&self) -> f32 {
        (self.char_gap_dits * self.word_gap_dits).sqrt().clamp(4.0, 10.0)
    }

    /// Record a mark duration and classify it. `None` marks a flutter
    /// fragment that should not become an element.
    pub fn classify_mark(&mut self, secs: f32) -> Option<Element> {
        if self.recent_all.len() == RECENT_ALL {
            self.recent_all.pop_front();
        }
        self.recent_all.push_back(secs);

        if secs < FRAGMENT_FRAC * self.dot_s {
            // Distinguish flutter from a genuinely faster sender: real dits
            // are self-consistent in length, flutter fragments are spread
            // out. A tight cluster of short marks reseeds the clock from the
            // full recent window (dits and dahs together).
            let thr = FRAGMENT_FRAC * self.dot_s;
            let (mut n, mut mn, mut mx) = (0usize, f32::MAX, 0.0f32);
            for &m in self.recent_all.iter().filter(|&&m| m < thr) {
                n += 1;
                mn = mn.min(m);
                mx = mx.max(m);
            }
            if n >= 5 && mx / mn.max(1e-4) < 1.6 {
                self.marks.clear();
                self.marks.extend(self.recent_all.iter().copied());
                self.refit();
            }
            return None;
        }

        if self.marks.len() == HISTORY {
            self.marks.pop_front();
        }
        self.marks.push_back(secs);
        self.refit();
        Some(if secs < self.mark_boundary_s() {
            Element::Dot
        } else {
            Element::Dash
        })
    }

    /// Record a completed space so char/word gap statistics adapt to the
    /// sender's rhythm (some operators leave 5-dit character gaps).
    pub fn record_space(&mut self, secs: f32) {
        let dits = secs / self.dot_s.max(1e-4);
        if dits < self.char_boundary_dits() || dits > 25.0 {
            return;
        }
        if self.spaces_dits.len() == SPACE_HISTORY {
            self.spaces_dits.pop_front();
        }
        self.spaces_dits.push_back(dits);
        if self.spaces_dits.len() < 4 {
            return;
        }
        let mut sorted: Vec<f32> = self.spaces_dits.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let lo = percentile(&sorted, 0.15);
        let hi = percentile(&sorted, 0.85);
        if hi / lo.max(0.1) >= 1.8 {
            let split = (lo * hi).sqrt();
            let idx = sorted.partition_point(|&s| s < split);
            if idx > 0 && idx < sorted.len() {
                self.char_gap_dits = sorted[idx / 2];
                self.word_gap_dits = sorted[idx + (sorted.len() - idx) / 2];
            }
        } else {
            // One gap size in the window — nudge whichever estimate is nearer.
            let m = percentile(&sorted, 0.5);
            let to_char = (m / self.char_gap_dits).ln().abs();
            let to_word = (m / self.word_gap_dits).ln().abs();
            if to_char <= to_word {
                self.char_gap_dits += 0.3 * (m - self.char_gap_dits);
            } else {
                self.word_gap_dits += 0.3 * (m - self.word_gap_dits);
            }
        }
        self.char_gap_dits = self.char_gap_dits.clamp(2.2, 6.0);
        self.word_gap_dits = self
            .word_gap_dits
            .clamp(1.6 * self.char_gap_dits, 20.0);
    }

    /// Classify a space duration against the adaptive gap statistics.
    pub fn space_kind(&self, secs: f32) -> SpaceKind {
        let dits = secs / self.dot_s.max(1e-4);
        if dits < self.char_boundary_dits() {
            SpaceKind::Element
        } else if dits < self.word_boundary_dits() {
            SpaceKind::Char
        } else {
            SpaceKind::Word
        }
    }

    /// Robust two-cluster fit over the recent-mark window.
    ///
    /// Splits at the geometric mean of the 15th/85th percentiles (immune to a
    /// few flutter fragments or stuck-key outliers, unlike min/max) and takes
    /// each cluster's median. With a single cluster present, the median
    /// nudges whichever estimate it is closer to in log distance, so the
    /// clock still tracks speed drift from an all-dit or all-dah stretch.
    fn refit(&mut self) {
        if self.marks.is_empty() {
            return;
        }
        let mut sorted: Vec<f32> = self.marks.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let lo = percentile(&sorted, 0.15).max(1e-4);
        let hi = percentile(&sorted, 0.85).max(1e-4);

        let (mut cand_dot, mut cand_dash) = (self.dot_s, self.dash_s);
        let mut split_done = false;
        if hi / lo >= TWO_CLUSTER_RATIO {
            let split = (lo * hi).sqrt();
            let idx = sorted.partition_point(|&m| m < split);
            if idx > 0 && idx < sorted.len() {
                let dot = median_of(&sorted[..idx]);
                let dash = median_of(&sorted[idx..]);
                if dash / dot >= 1.8 {
                    cand_dot = dot;
                    cand_dash = dash;
                    split_done = true;
                }
            }
        }
        if !split_done {
            let m = percentile(&sorted, 0.5);
            let to_dot = (m / self.dot_s).ln().abs();
            let to_dash = (m / self.dash_s).ln().abs();
            if to_dot <= to_dash {
                cand_dot += 0.3 * (m - cand_dot);
            } else {
                cand_dash += 0.3 * (m - cand_dash);
            }
        }
        cand_dot = cand_dot.clamp(MIN_DOT_S, MAX_DOT_S);
        cand_dash = cand_dash.clamp(2.0 * cand_dot, (4.5 * cand_dot).min(6.0 * MAX_DOT_S));

        // Only commit a fit the window actually supports: genuine keying
        // clusters tightly around dit/dah with an empty zone between them,
        // random noise marks fill the whole range. A poor fit freezes the
        // clock and drops confidence, which suppresses emission until
        // coherent keying returns.
        let q = self.timing_quality(cand_dot, cand_dash);
        if q >= 0.5 {
            self.dot_s = cand_dot;
            self.dash_s = cand_dash;
            self.confidence += 0.3 * (q.min(1.0) - self.confidence);
        } else {
            let old_q = self.timing_quality(self.dot_s, self.dash_s);
            self.confidence += 0.3 * (old_q.max(q).clamp(0.0, 1.0) - self.confidence);
        }
        self.confidence = self.confidence.clamp(0.0, 1.0);
    }

    /// Cluster fit quality: fraction of window marks near dit or dah, minus a
    /// penalty for marks in the forbidden zone around the dit/dah boundary
    /// where real keying never lands.
    fn timing_quality(&self, dot: f32, dash: f32) -> f32 {
        const LOG_TOL: f32 = 0.28;
        const BOUNDARY_TOL: f32 = 0.20;
        let boundary = (dot * dash).sqrt().max(1e-4);
        let (mut near, mut forbidden) = (0usize, 0usize);
        for &m in &self.marks {
            let d = (m / dot).ln().abs().min((m / dash).ln().abs());
            if d < LOG_TOL {
                near += 1;
            }
            if (m / boundary).ln().abs() < BOUNDARY_TOL {
                forbidden += 1;
            }
        }
        let n = self.marks.len().max(1) as f32;
        near as f32 / n - 1.5 * forbidden as f32 / n
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
            if i >= 4 && el != Some(want) {
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
        assert_eq!(c.classify_mark(0.10), Some(Element::Dot));
        assert_eq!(c.classify_mark(0.30), Some(Element::Dash));
        assert!((c.dot_seconds() - 0.10).abs() < 0.02);
    }

    #[test]
    fn space_kinds_follow_dit_clock() {
        let c = ElementClock::new(24.0); // dit = 50 ms
        assert_eq!(c.space_kind(0.05), SpaceKind::Element);
        assert_eq!(c.space_kind(0.15), SpaceKind::Char);
        assert_eq!(c.space_kind(0.40), SpaceKind::Word);
    }

    #[test]
    fn flutter_fragments_do_not_poison_the_clock() {
        // Real keying at 22 WPM (55/165 ms) with ~25% flutter fragments,
        // as seen on HF: the clock must stay near 55 ms and fragments must
        // be rejected instead of dragging the fit to its floor.
        let mut c = ElementClock::new(22.0);
        let pattern = [0.055, 0.165, 0.012, 0.055, 0.165, 0.022, 0.055, 0.165];
        let mut rejected = 0;
        for &m in pattern.iter().cycle().take(64) {
            if c.classify_mark(m).is_none() {
                rejected += 1;
            }
        }
        assert!(
            (c.dot_seconds() - 0.055).abs() < 0.012,
            "clock dragged by fragments: dot={}",
            c.dot_seconds()
        );
        assert!(rejected >= 8, "fragments not rejected: {rejected}");
        assert_eq!(c.classify_mark(0.055), Some(Element::Dot));
        assert_eq!(c.classify_mark(0.165), Some(Element::Dash));
    }

    #[test]
    fn genuinely_fast_sender_still_reseeds() {
        // Lock to 12 WPM first, then a 45 WPM sender (dit 26.7 ms) appears.
        let mut c = ElementClock::new(12.0);
        for _ in 0..8 {
            c.classify_mark(0.10);
            c.classify_mark(0.30);
        }
        let mut locked = false;
        for _ in 0..24 {
            c.classify_mark(0.027);
            c.classify_mark(0.080);
            if (c.dot_seconds() - 0.027).abs() < 0.01 {
                locked = true;
                break;
            }
        }
        assert!(locked, "never reseeded to fast sender: dot={}", c.dot_seconds());
    }

    #[test]
    fn wide_character_gaps_adapt_word_boundary() {
        // Operator keys with ~5.5-dit character gaps and ~12-dit word gaps
        // (the RODEZ capture). After a few gaps the 5.5-dit spaces must
        // classify as Char, not Word.
        let mut c = ElementClock::new(22.0); // dit ≈ 54.5 ms
        let dit = c.dot_seconds();
        for _ in 0..6 {
            c.record_space(5.5 * dit);
            c.record_space(5.5 * dit);
            c.record_space(12.0 * dit);
        }
        assert_eq!(c.space_kind(5.5 * dit), SpaceKind::Char, "5.5-dit gap");
        assert_eq!(c.space_kind(12.0 * dit), SpaceKind::Word, "12-dit gap");
        // Element gaps still classify as Element.
        assert_eq!(c.space_kind(1.0 * dit), SpaceKind::Element);
    }

    #[test]
    fn standard_gaps_keep_standard_boundaries() {
        let mut c = ElementClock::new(25.0);
        let dit = c.dot_seconds();
        for _ in 0..8 {
            c.record_space(3.0 * dit);
            c.record_space(7.0 * dit);
        }
        assert_eq!(c.space_kind(3.0 * dit), SpaceKind::Char);
        assert_eq!(c.space_kind(7.0 * dit), SpaceKind::Word);
    }
}
