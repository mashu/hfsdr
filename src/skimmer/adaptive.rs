//! Adaptive-WPM envelope CW decoder.
//!
//! A sample-by-sample state machine: rectify the tone into an envelope, threshold
//! it with an adaptive noise/peak tracker, time the mark/space runs, and classify
//! them against a continuously-updated dot length. Marks become dits/dahs;
//! character and word gaps flush the accumulated element string through the
//! [`morse`](super::morse) table.
//!
//! This is intentionally a robust, dependency-free baseline. The [`CwDecoder`]
//! trait lets a fancier beam-search/bigram decoder drop in later unchanged.

use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::morse::decode_elements;

/// Default seed speed before adaptation kicks in.
const DEFAULT_WPM: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Down,
    Up,
}

/// Envelope/threshold CW decoder with adaptive speed tracking.
#[derive(Clone, Debug)]
pub struct AdaptiveCwDecoder {
    sample_rate: f32,
    env: f32,
    peak: f32,
    noise: f32,
    key: Key,
    run: usize,
    dot_samples: f32,
    symbol: String,
    space_phase: u8,
    ends_with_space: bool,
    emitted_any: bool,
}

impl Default for AdaptiveCwDecoder {
    fn default() -> Self {
        Self::new(12_000.0)
    }
}

impl AdaptiveCwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        let mut d = Self {
            sample_rate: sample_rate.max(1.0),
            env: 0.0,
            peak: 0.0,
            noise: 0.0,
            key: Key::Up,
            run: 0,
            dot_samples: 0.0,
            symbol: String::new(),
            space_phase: 2,
            ends_with_space: true,
            emitted_any: false,
        };
        d.dot_samples = d.seed_dot();
        d
    }

    fn seed_dot(&self) -> f32 {
        (1.2 / DEFAULT_WPM) * self.sample_rate
    }

    fn clamp_dot(&self, dot: f32) -> f32 {
        // ~6..60 WPM.
        dot.clamp(0.02 * self.sample_rate, 0.20 * self.sample_rate)
    }

    fn process_sample(&mut self, x: f32, out: &mut String) {
        let inst = x.abs();
        // Rectify the BFO tone into an envelope (fast attack, slower release).
        let a = if inst > self.env { 0.05 } else { 0.01 };
        self.env += a * (inst - self.env);

        // Track on/off levels.
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
        let signal_present = span > 0.02 * self.peak.max(1e-6) && self.peak > 1e-5;
        let thr_high = self.noise + 0.6 * span;
        let thr_low = self.noise + 0.4 * span;

        let want = if !signal_present {
            Key::Up
        } else {
            match self.key {
                Key::Up if self.env > thr_high => Key::Down,
                Key::Down if self.env < thr_low => Key::Up,
                k => k,
            }
        };

        if want == self.key {
            self.run += 1;
            if self.key == Key::Up {
                self.advance_space(out);
            }
            return;
        }

        match self.key {
            Key::Down => self.end_mark(),
            Key::Up => self.space_phase = 0,
        }
        self.key = want;
        self.run = 1;
    }

    fn end_mark(&mut self) {
        let run = self.run as f32;
        if run < 0.35 * self.dot_samples {
            return; // ignore noise spike
        }
        if run < 2.0 * self.dot_samples {
            self.symbol.push('.');
            self.dot_samples = self.clamp_dot(0.8 * self.dot_samples + 0.2 * run);
        } else {
            self.symbol.push('-');
            self.dot_samples = self.clamp_dot(0.8 * self.dot_samples + 0.2 * (run / 3.0));
        }
    }

    fn advance_space(&mut self, out: &mut String) {
        let run = self.run as f32;
        if self.space_phase < 1 && run >= 2.0 * self.dot_samples {
            self.flush_symbol(out);
            self.space_phase = 1;
        }
        if self.space_phase < 2 && run >= 5.0 * self.dot_samples {
            if self.emitted_any && !self.ends_with_space {
                out.push(' ');
                self.ends_with_space = true;
            }
            self.space_phase = 2;
        }
    }

    fn flush_symbol(&mut self, out: &mut String) {
        if self.symbol.is_empty() {
            return;
        }
        let ch = decode_elements(&self.symbol).unwrap_or('?');
        out.push(ch);
        self.symbol.clear();
        self.emitted_any = true;
        self.ends_with_space = false;
    }
}

impl CwDecoder for AdaptiveCwDecoder {
    fn push_audio(&mut self, audio: &[f32], sample_rate: f32) -> String {
        if (sample_rate - self.sample_rate).abs() > 1.0 && sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.dot_samples = self.clamp_dot(self.dot_samples);
        }
        let mut out = String::new();
        for &x in audio {
            self.process_sample(x, &mut out);
        }
        out
    }

    fn wpm(&self) -> f32 {
        wpm_from_dot_seconds(self.dot_samples / self.sample_rate)
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

    /// Render text as a keyed BFO tone at `wpm`.
    fn keyed_tone(text: &str, wpm: f32, sample_rate: f32, pitch: f32) -> Vec<f32> {
        let dot = (1.2 / wpm * sample_rate) as usize;
        let mut samples = Vec::new();
        let mut phase = 0.0f32;
        let mut push = |on: bool, len: usize, phase: &mut f32, out: &mut Vec<f32>| {
            for _ in 0..len {
                *phase += TAU * pitch / sample_rate;
                out.push(if on { phase.sin() } else { 0.0 });
            }
        };
        // lead-in silence so the tracker settles
        push(false, dot * 8, &mut phase, &mut samples);
        for (wi, word) in text.split(' ').enumerate() {
            if wi > 0 {
                push(false, dot * 7, &mut phase, &mut samples);
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    push(false, dot * 3, &mut phase, &mut samples);
                }
                let code = encode_char(ch).unwrap_or("");
                for (ei, el) in code.chars().enumerate() {
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

    fn decode(text: &str, wpm: f32) -> String {
        let sr = 8_000.0;
        let audio = keyed_tone(text, wpm, sr, 650.0);
        let mut dec = AdaptiveCwDecoder::new(sr);
        dec.push_audio(&audio, sr)
    }

    #[test]
    fn decodes_cq() {
        let out = decode("CQ", 25.0);
        assert!(out.contains("CQ"), "got {out:?}");
    }

    #[test]
    fn decodes_callsign_and_word() {
        let out = decode("CQ TEST", 22.0);
        assert!(out.contains("CQ"), "got {out:?}");
        assert!(out.contains("TEST"), "got {out:?}");
    }

    #[test]
    fn tracks_speed_roughly() {
        let sr = 8_000.0;
        let audio = keyed_tone("PARIS PARIS", 30.0, sr, 600.0);
        let mut dec = AdaptiveCwDecoder::new(sr);
        dec.push_audio(&audio, sr);
        let wpm = dec.wpm();
        assert!((20.0..40.0).contains(&wpm), "wpm estimate off: {wpm}");
    }
}
