//! Beam-search CW decoder with a callsign-oriented bigram language model.
//!
//! Shares the adaptive front-end (envelope tracker, debounced keyer, dit/dah
//! clock) with [`super::adaptive::AdaptiveCwDecoder`], but keeps several
//! hypotheses alive when a mark duration falls near the dit/dah boundary.
//! Each hypothesis is scored with a duration log-likelihood plus a bigram
//! prior biased toward CQ/DE/callsign text. Beams collapse at word gaps so
//! emitted text is always a single consistent hypothesis.

use super::config::DecoderParams;
use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::envelope::KeyingEnvelope;
use super::morse::decode_elements;
use super::timing::{Element, KeyEvent, Keyer, SpaceKind};
use super::timing::ElementClock;

/// Longest real Morse pattern in the table is 6 elements.
const MAX_ELEMENTS: usize = 7;
/// |ln(duration / boundary)| below which a mark spawns both interpretations.
const AMBIGUITY_LOG: f32 = 0.30;
/// Log-duration sigma for the element likelihood.
const DURATION_SIGMA: f32 = 0.40;
/// Collapse even without a word gap once the best text grows this long.
const MAX_PENDING_CHARS: usize = 24;

#[derive(Clone, Debug)]
struct Hypothesis {
    symbol: String,
    text: String,
    score: f32,
    poisoned: bool,
    last_char: Option<char>,
}

impl Hypothesis {
    fn new() -> Self {
        Self {
            symbol: String::new(),
            text: String::new(),
            score: 0.0,
            poisoned: false,
            last_char: None,
        }
    }

    fn push_element(&mut self, el: char, weight: f32) {
        if self.symbol.len() >= MAX_ELEMENTS {
            self.poisoned = true;
        } else {
            self.symbol.push(el);
        }
        self.score += weight;
    }

    fn discard_symbol(&mut self) {
        self.symbol.clear();
        self.poisoned = false;
    }

    fn flush_symbol(&mut self) {
        let ch = if self.poisoned {
            self.poisoned = false;
            self.symbol.clear();
            '?'
        } else if self.symbol.is_empty() {
            return;
        } else {
            let ch = decode_elements(&self.symbol).unwrap_or('?');
            self.symbol.clear();
            ch
        };
        if ch == '?' {
            self.score -= 1.5;
            if self.last_char == Some('?') {
                return;
            }
        }
        self.score += bigram_log(self.last_char, ch);
        self.text.push(ch);
        self.last_char = Some(ch);
    }
}

/// Beam-search decoder with callsign-biased bigram scoring.
#[derive(Clone, Debug)]
pub struct BigramCwDecoder {
    sample_rate: f32,
    params: DecoderParams,
    envelope: KeyingEnvelope,
    key_down: bool,
    keyer: Keyer,
    clock: ElementClock,
    beams: Vec<Hypothesis>,
    /// 0 = collecting a char, 1 = char flushed, 2 = word gap handled.
    space_flushed: u8,
}

impl Default for BigramCwDecoder {
    fn default() -> Self {
        Self::with_params(500.0, DecoderParams::default())
    }
}

impl BigramCwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self::with_params(sample_rate, DecoderParams::default())
    }

    pub fn with_params(sample_rate: f32, params: DecoderParams) -> Self {
        let params = params.clamped();
        let sample_rate = sample_rate.max(1.0);
        let clock = ElementClock::new(params.initial_wpm);
        let mut keyer = Keyer::new(sample_rate);
        keyer.set_dot_seconds(clock.dot_seconds());
        Self {
            sample_rate,
            envelope: KeyingEnvelope::new(params.envelope, sample_rate),
            key_down: false,
            keyer,
            clock,
            beams: vec![Hypothesis::new()],
            space_flushed: 2,
            params,
        }
    }

    fn process_sample(&mut self, x: f32, out: &mut String) {
        let step = self.envelope.update(x);
        self.key_down = if !step.signal_present {
            false
        } else if self.key_down {
            step.env >= step.thr_low
        } else {
            step.env > step.thr_high
        };

        match self.keyer.step(self.key_down) {
            Some(KeyEvent::Mark(secs)) => self.on_mark(secs),
            Some(KeyEvent::Space(secs)) => self.clock.record_space(secs),
            None => {}
        }

        if self.keyer.in_space() && self.space_flushed < 2 {
            let kind = self.clock.space_kind(self.keyer.space_seconds());
            if kind >= SpaceKind::Char && self.space_flushed < 1 {
                let confident = self.clock.is_confident();
                for h in &mut self.beams {
                    if confident {
                        h.flush_symbol();
                    } else {
                        h.discard_symbol();
                    }
                }
                self.prune();
                self.space_flushed = 1;
                if self.best().text.len() >= MAX_PENDING_CHARS {
                    self.collapse(out, false);
                }
            }
            if kind == SpaceKind::Word {
                self.collapse(out, true);
                self.space_flushed = 2;
            }
        }
    }

    fn on_mark(&mut self, secs: f32) {
        let Some(el) = self.clock.classify_mark(secs) else {
            return; // flutter fragment — not an element
        };
        self.keyer.set_dot_seconds(self.clock.dot_seconds());
        let boundary = self.clock.mark_boundary_s().max(1e-4);
        let lr = (secs.max(1e-4) / boundary).ln();
        let ll = |expected: f32| -> f32 {
            let r = (secs.max(1e-4) / expected.max(1e-4)).ln() / DURATION_SIGMA;
            -0.5 * r * r
        };
        if lr.abs() < AMBIGUITY_LOG {
            let dot_w = ll(self.clock.dot_seconds());
            let dash_w = ll(self.clock.dash_seconds());
            let mut next = Vec::with_capacity(self.beams.len() * 2);
            for h in self.beams.drain(..) {
                let mut dash = h.clone();
                dash.push_element('-', dash_w);
                next.push(dash);
                let mut dot = h;
                dot.push_element('.', dot_w);
                next.push(dot);
            }
            self.beams = next;
            self.prune();
        } else {
            let ch = match el {
                Element::Dot => '.',
                Element::Dash => '-',
            };
            for h in &mut self.beams {
                h.push_element(ch, 0.0);
            }
        }
        self.space_flushed = 0;
    }

    fn prune(&mut self) {
        self.beams
            .sort_by(|a, b| b.score.total_cmp(&a.score));
        // Merge hypotheses that converged to identical text.
        self.beams
            .dedup_by(|a, b| a.text == b.text && a.symbol == b.symbol);
        self.beams.truncate(self.params.beam_width.max(1));
        if self.beams.is_empty() {
            self.beams.push(Hypothesis::new());
        }
    }

    fn best(&self) -> &Hypothesis {
        self.beams
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .expect("at least one beam")
    }

    /// Emit the best hypothesis and restart the beam from it.
    fn collapse(&mut self, out: &mut String, word_gap: bool) {
        let best_idx = self
            .beams
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.score.total_cmp(&b.score))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut best = self.beams.swap_remove(best_idx);
        if !best.text.is_empty() {
            out.push_str(&best.text);
            if word_gap {
                out.push(' ');
            }
            best.text.clear();
        }
        best.score = 0.0;
        self.beams.clear();
        self.beams.push(best);
    }
}

/// Log-prior for character bigrams, biased toward CQ/DE and callsign shapes.
fn bigram_log(prev: Option<char>, ch: char) -> f32 {
    let p = prev.unwrap_or(' ');
    let c = ch.to_ascii_uppercase();
    match (p, c) {
        (' ', 'C') => 1.2,
        (' ', 'Q') => 0.8,
        ('C', 'Q') => 1.5,
        ('Q', ' ') => 0.6,
        (' ', 'D') => 0.5,
        ('D', 'E') => 0.8,
        ('E', ' ') => 0.4,
        (' ', 'W') | (' ', 'K') | (' ', 'N') | (' ', 'V') | (' ', 'G') => 0.9,
        ('W', d) | ('K', d) | ('N', d) | ('V', d) | ('G', d) if d.is_ascii_digit() => 1.0,
        (a, b) if a.is_ascii_digit() && b.is_ascii_digit() => 0.6,
        (a, b) if a.is_ascii_digit() && b.is_ascii_alphabetic() => 0.7,
        (a, b) if a.is_ascii_alphabetic() && b.is_ascii_digit() => 0.6,
        (a, b) if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => 0.15,
        (' ', b) if b.is_ascii_alphanumeric() => 0.3,
        _ => 0.0,
    }
}

impl CwDecoder for BigramCwDecoder {
    fn push_audio(&mut self, audio: &[f32], sample_rate: f32) -> String {
        if (sample_rate - self.sample_rate).abs() > 1.0 && sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.envelope = KeyingEnvelope::new(self.params.envelope, sample_rate);
            self.keyer = Keyer::new(sample_rate);
            self.keyer.set_dot_seconds(self.clock.dot_seconds());
            self.key_down = false;
        }
        let mut out = String::new();
        for &x in audio {
            self.process_sample(x, &mut out);
        }
        out
    }

    fn wpm(&self) -> f32 {
        wpm_from_dot_seconds(self.clock.dot_seconds())
    }

    fn reset(&mut self) {
        *self = Self::with_params(self.sample_rate, self.params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::morse::encode_char;
    use std::f32::consts::TAU;

    fn keyed_tone(text: &str, wpm: f32, sample_rate: f32, pitch: f32) -> Vec<f32> {
        let dot = (1.2 / wpm * sample_rate) as usize;
        let mut samples = Vec::new();
        let mut phase = 0.0f32;
        let push = |on: bool, len: usize, phase: &mut f32, out: &mut Vec<f32>| {
            for _ in 0..len {
                *phase += TAU * pitch / sample_rate;
                out.push(if on { phase.sin() } else { 0.0 });
            }
        };
        push(false, dot * 8, &mut phase, &mut samples);
        for (wi, word) in text.split(' ').enumerate() {
            if wi > 0 {
                push(false, dot * 7, &mut phase, &mut samples);
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    push(false, dot * 3, &mut phase, &mut samples);
                }
                for (ei, el) in encode_char(ch).unwrap_or("").chars().enumerate() {
                    if ei > 0 {
                        push(false, dot, &mut phase, &mut samples);
                    }
                    let len = if el == '-' { dot * 3 } else { dot };
                    push(true, len, &mut phase, &mut samples);
                }
            }
        }
        push(false, dot * 10, &mut phase, &mut samples);
        samples
    }

    #[test]
    fn decodes_cq() {
        let sr = 8_000.0;
        let audio = keyed_tone("CQ", 25.0, sr, 650.0);
        let mut dec = BigramCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        assert!(out.contains("CQ"), "got {out:?}");
    }

    #[test]
    fn decodes_callsign() {
        let sr = 8_000.0;
        let audio = keyed_tone("CQ W1AW", 22.0, sr, 650.0);
        let mut dec = BigramCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        assert!(out.contains("CQ"), "got {out:?}");
        assert!(out.contains("W1AW"), "got {out:?}");
    }

    #[test]
    fn decodes_across_speed_range() {
        for wpm in [12.0, 18.0, 25.0, 32.0, 40.0] {
            let sr = 8_000.0;
            let audio = keyed_tone("CQ CQ DE OH2BH OH2BH K", wpm, sr, 600.0);
            let mut dec = BigramCwDecoder::new(sr);
            let out = dec.push_audio(&audio, sr);
            assert!(out.contains("OH2BH"), "failed at {wpm} wpm: got {out:?}");
        }
    }
}
