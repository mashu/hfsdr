//! Adaptive-WPM envelope CW decoder.
//!
//! Pipeline per sample: adaptive Schmitt threshold over the tracked envelope
//! ([`KeyingEnvelope`]), debounced run-length extraction ([`Keyer`] — bridges
//! QSB dropouts, drops noise blips), then two-cluster dit/dah classification
//! ([`ElementClock`]). Characters flush as soon as the inter-character gap is
//! confirmed, so decode latency is one character.

use super::config::DecoderParams;
use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::envelope::KeyingEnvelope;
use super::morse::decode_elements;
use super::timing::{ElementClock, KeyEvent, Keyer, SpaceKind};

/// Longest real Morse pattern in the table is 6 elements.
const MAX_ELEMENTS: usize = 7;

/// Envelope/threshold CW decoder with adaptive speed tracking.
#[derive(Clone, Debug)]
pub struct AdaptiveCwDecoder {
    sample_rate: f32,
    params: DecoderParams,
    envelope: KeyingEnvelope,
    key_down: bool,
    keyer: Keyer,
    clock: ElementClock,
    symbol: String,
    poisoned: bool,
    /// 0 = collecting a char, 1 = char flushed, 2 = word gap emitted.
    space_flushed: u8,
    emitted_any: bool,
    ends_with_space: bool,
    last_char: Option<char>,
}

impl Default for AdaptiveCwDecoder {
    fn default() -> Self {
        Self::with_params(500.0, DecoderParams::default())
    }
}

impl AdaptiveCwDecoder {
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
            symbol: String::new(),
            poisoned: false,
            space_flushed: 2,
            emitted_any: false,
            ends_with_space: true,
            last_char: None,
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

        if let Some(KeyEvent::Mark(secs)) = self.keyer.step(self.key_down) {
            let el = self.clock.classify_mark(secs);
            self.keyer.set_dot_seconds(self.clock.dot_seconds());
            if self.symbol.len() >= MAX_ELEMENTS {
                self.poisoned = true;
            } else {
                self.symbol.push(el.as_char());
            }
            self.space_flushed = 0;
        }

        if self.keyer.in_space() && self.space_flushed < 2 {
            let kind = self.clock.space_kind(self.keyer.space_seconds());
            if kind >= SpaceKind::Char && self.space_flushed < 1 {
                self.flush_symbol(out);
                self.space_flushed = 1;
            }
            if kind == SpaceKind::Word {
                if self.emitted_any && !self.ends_with_space {
                    out.push(' ');
                    self.ends_with_space = true;
                }
                self.space_flushed = 2;
            }
        }
    }

    fn flush_symbol(&mut self, out: &mut String) {
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
        // Runs of '?' carry no information — keep one as a garble marker.
        if ch == '?' && self.last_char == Some('?') {
            return;
        }
        out.push(ch);
        self.last_char = Some(ch);
        self.emitted_any = true;
        self.ends_with_space = false;
    }
}

impl CwDecoder for AdaptiveCwDecoder {
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
    fn decodes_across_speed_range() {
        for wpm in [10.0, 15.0, 20.0, 28.0, 36.0, 45.0] {
            let out = decode("CQ CQ DE W1AW W1AW K", wpm);
            assert!(
                out.contains("W1AW"),
                "failed at {wpm} wpm: got {out:?}"
            );
        }
    }

    #[test]
    fn tracks_speed_roughly() {
        let sr = 8_000.0;
        let audio = keyed_tone("PARIS PARIS", 30.0, sr, 600.0);
        let mut dec = AdaptiveCwDecoder::new(sr);
        dec.push_audio(&audio, sr);
        let wpm = dec.wpm();
        assert!((24.0..38.0).contains(&wpm), "wpm estimate off: {wpm}");
    }
}
