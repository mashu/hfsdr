//! Beam-search CW decoder with a callsign-oriented bigram language model.

use super::config::DecoderParams;
use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::envelope::KeyingEnvelope;
use super::morse::decode_elements;

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
    fn new(sample_rate: f32, initial_wpm: f32) -> Self {
        let dot = (1.2 / initial_wpm.max(1.0)) * sample_rate;
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

/// Beam-search decoder with callsign-biased bigram scoring.
#[derive(Clone, Debug)]
pub struct BigramCwDecoder {
    sample_rate: f32,
    params: DecoderParams,
    envelope: KeyingEnvelope,
    key: Key,
    run: usize,
    beams: Vec<Hypothesis>,
    emitted_len: usize,
}

impl Default for BigramCwDecoder {
    fn default() -> Self {
        Self::with_params(12_000.0, DecoderParams::default())
    }
}

impl BigramCwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self::with_params(sample_rate, DecoderParams::default())
    }

    pub fn with_params(sample_rate: f32, params: DecoderParams) -> Self {
        let params = params.clamped();
        let sample_rate = sample_rate.max(1.0);
        Self {
            sample_rate,
            envelope: KeyingEnvelope::new(params.envelope),
            key: Key::Up,
            run: 0,
            beams: vec![Hypothesis::new(sample_rate, params.initial_wpm)],
            emitted_len: 0,
            params,
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
        let step = self.envelope.update(x);

        let want = if !step.signal_present {
            Key::Up
        } else {
            match self.key {
                Key::Up if step.env > step.thr_high => Key::Down,
                Key::Down if step.env < step.thr_low => Key::Up,
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

    fn end_mark_all(&mut self) {
        let run = self.run as f32;
        let mut next = Vec::new();
        for h in self.beams.drain(..) {
            next.extend(branch_mark(h, run, self.sample_rate));
        }
        self.beams = prune_beams(next, self.params.beam_width, self.sample_rate, self.params.initial_wpm);
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
        push_branch('.', 0.0, run);
        let mut dah = h;
        dah.symbol.push('-');
        dah.dot_samples = dah.clamp_dot(sample_rate, 0.85 * dah.dot_samples + 0.15 * (run / 3.0));
        out.push(dah);
    }
    out
}

fn prune_beams(
    mut beams: Vec<Hypothesis>,
    beam_width: usize,
    sample_rate: f32,
    initial_wpm: f32,
) -> Vec<Hypothesis> {
    beams.sort_by(|a, b| b.score.total_cmp(&a.score));
    beams.truncate(beam_width.max(1));
    if beams.is_empty() {
        beams.push(Hypothesis::new(sample_rate, initial_wpm));
    }
    beams
}

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
        assert!(out.contains('W'), "got {out:?}");
    }
}
