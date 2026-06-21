//! Beam-search CW decoder with a callsign-oriented bigram language model.
//!
//! Each hypothesis tracks its own timing estimate (dot length) and morse
//! element string. On ambiguous mark lengths the beam branches; after each
//! character is flushed, hypotheses are rescored with digram log-probabilities
//! tuned for amateur callsigns and contest traffic, then pruned to a fixed width.

use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::morse::decode_elements;

const BEAM_WIDTH: usize = 12;
const DEFAULT_WPM: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Down,
    Up,
}

#[derive(Clone, Debug)]
struct Hypothesis {
    dot_samples: f32,
    symbol: String,
    text: String,
    score: f32,
    space_phase: u8,
    emitted_any: bool,
    ends_with_space: bool,
}

impl Hypothesis {
    fn new(sample_rate: f32) -> Self {
        let dot = (1.2 / DEFAULT_WPM) * sample_rate;
        Self {
            dot_samples: dot,
            symbol: String::new(),
            text: String::new(),
            score: 0.0,
            space_phase: 2,
            emitted_any: false,
            ends_with_space: true,
        }
    }

    fn clamp_dot(&self, sample_rate: f32, dot: f32) -> f32 {
        dot.clamp(0.02 * sample_rate, 0.20 * sample_rate)
    }
}

/// Shared envelope tracker feeding all beam hypotheses.
#[derive(Clone, Debug)]
struct Envelope {
    env: f32,
    peak: f32,
    noise: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            env: 0.0,
            peak: 0.0,
            noise: 0.0,
        }
    }
}

impl Envelope {
    fn update(&mut self, x: f32) -> (f32, f32, f32) {
        let inst = x.abs();
        let a = if inst > self.env { 0.05 } else { 0.01 };
        self.env += a * (inst - self.env);
        if self.env > self.peak {
            self.peak = self.env;
        } else {
            self.peak *= 0.99995;
        }
        if self.env < self.noise {
            self.noise += 0.02 * (self.env - self.noise);
        } else {
            self.noise += 0.0002 * (self.env - self.noise);
        }
        let span = self.peak - self.noise;
        let thr_high = self.noise + 0.6 * span;
        let thr_low = self.noise + 0.4 * span;
        (span, thr_high, thr_low)
    }
}

/// Beam-search decoder with callsign-biased bigram scoring.
#[derive(Clone, Debug)]
pub struct BigramCwDecoder {
    sample_rate: f32,
    envelope: Envelope,
    key: Key,
    run: usize,
    beams: Vec<Hypothesis>,
    emitted_len: usize,
}

impl Default for BigramCwDecoder {
    fn default() -> Self {
        Self::new(12_000.0)
    }
}

impl BigramCwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            envelope: Envelope::default(),
            key: Key::Up,
            run: 0,
            beams: vec![Hypothesis::new(sample_rate.max(1.0))],
            emitted_len: 0,
        }
    }

    fn best_beam(&self) -> &Hypothesis {
        self.beams
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .unwrap_or(&self.beams[0])
    }

    fn drain_new_text(&mut self) -> String {
        let best_len = self.best_beam().text.len();
        if best_len <= self.emitted_len {
            return String::new();
        }
        let delta = self.best_beam().text[self.emitted_len..].to_string();
        self.emitted_len = best_len;
        delta
    }

    fn process_sample(&mut self, x: f32) {
        let (span, thr_high, thr_low) = self.envelope.update(x);
        let signal_present = span > 0.02 * self.envelope.peak.max(1e-6) && self.envelope.peak > 1e-5;

        let want = if !signal_present {
            Key::Up
        } else {
            match self.key {
                Key::Up if self.env_above(thr_high) => Key::Down,
                Key::Down if self.env_below(thr_low) => Key::Up,
                k => k,
            }
        };

        if want == self.key {
            self.run += 1;
            if self.key == Key::Up {
                self.advance_space_all();
            }
            return;
        }

        if self.key == Key::Down {
            self.end_mark_all();
        } else {
            for h in &mut self.beams {
                h.space_phase = 0;
            }
        }
        self.key = want;
        self.run = 1;
    }

    fn env_above(&self, thr: f32) -> bool {
        self.envelope.env > thr
    }

    fn env_below(&self, thr: f32) -> bool {
        self.envelope.env < thr
    }

    fn end_mark_all(&mut self) {
        let run = self.run as f32;
        let mut next = Vec::new();
        for h in self.beams.drain(..) {
            next.extend(branch_mark(h, run, self.sample_rate));
        }
        self.beams = prune_beams(next);
    }

    fn advance_space_all(&mut self) {
        let run = self.run as f32;
        for h in &mut self.beams {
            if h.space_phase < 1 && run >= 2.0 * h.dot_samples {
                Self::flush_symbol(h);
                h.space_phase = 1;
            }
            if h.space_phase < 2 && run >= 5.0 * h.dot_samples {
                if h.emitted_any && !h.ends_with_space {
                    h.text.push(' ');
                    h.ends_with_space = true;
                    h.score += 0.1;
                }
                h.space_phase = 2;
            }
        }
    }

    fn flush_symbol(h: &mut Hypothesis) {
        if h.symbol.is_empty() {
            return;
        }
        let ch = decode_elements(&h.symbol).unwrap_or('?');
        let prev = h.text.chars().last();
        h.score += bigram_log(prev, ch);
        if ch == '?' {
            h.score -= 1.5;
        }
        h.text.push(ch);
        h.symbol.clear();
        h.emitted_any = true;
        h.ends_with_space = false;
    }

    fn best_dot(&self) -> f32 {
        self.beams
            .iter()
            .map(|h| h.dot_samples)
            .sum::<f32>()
            / self.beams.len().max(1) as f32
    }
}

/// Branch a hypothesis on ambiguous mark length (dit vs dah).
fn branch_mark(h: Hypothesis, run: f32, sample_rate: f32) -> Vec<Hypothesis> {
    if run < 0.35 * h.dot_samples {
        return vec![h];
    }

    let mut out = Vec::new();
    let dit_hi = 2.2 * h.dot_samples;
    let dah_lo = 1.8 * h.dot_samples;

    let mut push_branch = |el: char, weight: f32, dot_scale: f32| {
        let mut b = h.clone();
        b.symbol.push(el);
        b.dot_samples = b.clamp_dot(sample_rate, 0.85 * b.dot_samples + 0.15 * dot_scale);
        b.score += weight;
        out.push(b);
    };

    if run < dah_lo {
        push_branch('.', 0.2, run);
    } else if run > dit_hi {
        push_branch('-', 0.2, run / 3.0);
    } else {
        // Ambiguous zone: beam both interpretations.
        push_branch('.', 0.0, run);
        let mut dah = h;
        dah.symbol.push('-');
        dah.dot_samples = dah.clamp_dot(sample_rate, 0.85 * dah.dot_samples + 0.15 * (run / 3.0));
        out.push(dah);
    }
    out
}

fn prune_beams(mut beams: Vec<Hypothesis>) -> Vec<Hypothesis> {
    beams.sort_by(|a, b| b.score.total_cmp(&a.score));
    beams.truncate(BEAM_WIDTH);
    if beams.is_empty() {
        beams.push(Hypothesis::new(12_000.0));
    }
    beams
}

/// Log-probability style score for letter pairs common in callsigns / contest CW.
fn bigram_log(prev: Option<char>, ch: char) -> f32 {
    let p = prev.unwrap_or(' ');
    let c = ch.to_ascii_uppercase();
    match (p, c) {
        (' ', 'C') => 1.2, // CQ
        (' ', 'Q') => 0.8,
        ('C', 'Q') => 1.5,
        ('Q', ' ') => 0.6,
        (' ', 'D') => 0.5, // DE
        ('D', 'E') => 0.8,
        ('E', ' ') => 0.4,
        (' ', 'W') | (' ', 'K') | (' ', 'N') | (' ', 'V') | (' ', 'G') => 0.9,
        ('W', d) | ('K', d) | ('N', d) | ('V', d) | ('G', d) if d.is_ascii_digit() => 1.0,
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
            for h in &mut self.beams {
                h.dot_samples = h.clamp_dot(sample_rate, h.dot_samples);
            }
        }
        let mut out = String::new();
        for &x in audio {
            self.process_sample(x);
            out.push_str(&self.drain_new_text());
        }
        out
    }

    fn wpm(&self) -> f32 {
        wpm_from_dot_seconds(self.best_dot() / self.sample_rate)
    }

    fn reset(&mut self) {
        let sr = self.sample_rate;
        *self = Self::new(sr);
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
        assert!(out.contains('W'), "got {out:?}");
    }
}
