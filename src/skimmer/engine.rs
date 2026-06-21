//! In-band skimmer engine: a peak-driven bank of narrowband CW decoders.
//!
//! Given a spectrum row (to find signals) and the matching IQ block (to demod
//! them), the engine spins a decoder up per peak, decodes every signal across
//! the span, validates callsigns/CQ, and folds results into a [`SpotStore`].
//! Decoders retire when their peak vanishes.
//!
//! Each channel uses a cheap NCO + decimator + 2-pole complex lowpass so a
//! whole bank stays affordable; the per-channel envelope drives
//! [`AdaptiveCwDecoder`]. Heavier matched filtering lives in the listen chain
//! ([`crate::dsp::cw`]); the skimmer favours breadth over per-signal fidelity.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::source::Complex32;

use super::adaptive::AdaptiveCwDecoder;
use super::decoder::CwDecoder;
use super::patterns::analyze;
use super::peaks::detect_peaks;
use super::spots::{SpotKind, SpotStore};

const MAX_TEXT: usize = 64;

/// Configuration for the skimmer bank.
#[derive(Clone, Debug)]
pub struct SkimmerConfig {
    pub bucket_hz: f32,
    pub min_snr_db: f32,
    pub min_separation_bins: usize,
    pub max_channels: usize,
    pub channel_timeout: Duration,
    pub spot_max_age: Duration,
    pub source_label: String,
}

impl Default for SkimmerConfig {
    fn default() -> Self {
        Self {
            bucket_hz: 80.0,
            min_snr_db: 14.0,
            min_separation_bins: 6,
            max_channels: 24,
            channel_timeout: Duration::from_secs(8),
            spot_max_age: Duration::from_secs(120),
            source_label: "rx".to_string(),
        }
    }
}

/// Cheap 2-pole complex lowpass for channel isolation.
#[derive(Clone, Copy, Debug)]
struct ComplexLowpass {
    y1: Complex32,
    y2: Complex32,
    a: f32,
}

impl ComplexLowpass {
    fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let a = 1.0 - (-std::f32::consts::TAU * cutoff_hz / sample_rate.max(1.0)).exp();
        Self {
            y1: Complex32 { re: 0.0, im: 0.0 },
            y2: Complex32 { re: 0.0, im: 0.0 },
            a: a.clamp(0.0, 1.0),
        }
    }

    fn process(&mut self, x: Complex32) -> Complex32 {
        self.y1.re += self.a * (x.re - self.y1.re);
        self.y1.im += self.a * (x.im - self.y1.im);
        self.y2.re += self.a * (self.y1.re - self.y2.re);
        self.y2.im += self.a * (self.y1.im - self.y2.im);
        self.y2
    }
}

struct DecoderChannel {
    offset_hz: f32,
    phase: f32,
    decim_factor: usize,
    decim_counter: usize,
    filter: ComplexLowpass,
    audio_rate: f32,
    decoder: AdaptiveCwDecoder,
    audio: Vec<f32>,
    text: String,
    last_seen: Instant,
    snr_db: f32,
}

impl DecoderChannel {
    fn new(offset_hz: f32, iq_rate: f32, snr_db: f32) -> Self {
        let decim_factor = (iq_rate / 12_000.0).round().clamp(1.0, 256.0) as usize;
        let audio_rate = iq_rate / decim_factor as f32;
        Self {
            offset_hz,
            phase: 0.0,
            decim_factor,
            decim_counter: 0,
            filter: ComplexLowpass::new(audio_rate, 150.0),
            audio_rate,
            decoder: AdaptiveCwDecoder::new(audio_rate),
            audio: Vec::new(),
            text: String::new(),
            last_seen: Instant::now(),
            snr_db,
        }
    }

    /// Demodulate the block into the channel's envelope, decode, return new text.
    fn process(&mut self, iq: &[Complex32], iq_rate: f32) -> String {
        self.audio.clear();
        let inc = std::f32::consts::TAU * self.offset_hz / iq_rate;
        for &s in iq {
            let (sin, cos) = self.phase.sin_cos();
            self.phase += inc;
            if self.phase >= std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
            let shifted = Complex32 {
                re: s.re * cos + s.im * sin,
                im: -s.re * sin + s.im * cos,
            };
            self.decim_counter += 1;
            if self.decim_counter >= self.decim_factor {
                self.decim_counter = 0;
                let mag = self.filter.process(shifted).norm();
                self.audio.push(mag);
            }
        }
        self.decoder.push_audio(&self.audio, self.audio_rate)
    }
}

/// Peak-driven bank of CW decoders feeding a spot store.
pub struct Skimmer {
    config: SkimmerConfig,
    channels: HashMap<i64, DecoderChannel>,
    store: SpotStore,
    center_hz: f64,
    last_center_hz: f64,
}

impl Skimmer {
    pub fn new(config: SkimmerConfig) -> Self {
        Self {
            config,
            channels: HashMap::new(),
            store: SpotStore::new(),
            center_hz: 0.0,
            last_center_hz: f64::NAN,
        }
    }

    pub fn store(&self) -> &SpotStore {
        &self.store
    }

    pub fn active_channels(&self) -> usize {
        self.channels.len()
    }

    fn bucket(&self, offset_hz: f32) -> i64 {
        (offset_hz / self.config.bucket_hz).round() as i64
    }

    /// Run one block: `spectrum` (fftshifted dB) finds peaks, `iq` feeds decoders.
    pub fn process(
        &mut self,
        iq: &[Complex32],
        iq_rate: f32,
        spectrum: &[f32],
        center_hz: f64,
    ) {
        if iq.is_empty() || iq_rate <= 0.0 {
            return;
        }
        self.center_hz = center_hz;
        if (center_hz - self.last_center_hz).abs() > 1.0 {
            self.channels.clear();
            self.last_center_hz = center_hz;
        }

        let peaks = detect_peaks(
            spectrum,
            iq_rate,
            self.config.min_snr_db,
            self.config.min_separation_bins,
        );
        for p in &peaks {
            let key = self.bucket(p.offset_hz);
            if let Some(ch) = self.channels.get_mut(&key) {
                ch.last_seen = Instant::now();
                ch.snr_db = p.snr_db;
            } else if self.channels.len() < self.config.max_channels {
                self.channels
                    .insert(key, DecoderChannel::new(p.offset_hz, iq_rate, p.snr_db));
            }
        }

        let center = self.center_hz;
        let label = self.config.source_label.clone();
        for ch in self.channels.values_mut() {
            let delta = ch.process(iq, iq_rate);
            if delta.is_empty() {
                continue;
            }
            ch.text.push_str(&delta);
            if ch.text.len() > MAX_TEXT {
                let cut = ch.text.len() - MAX_TEXT;
                ch.text.drain(..cut);
            }
            if let Some(m) = analyze(&ch.text) {
                let freq = center + ch.offset_hz as f64;
                let wpm = ch.decoder.wpm();
                self.store
                    .observe(freq, m.callsign, m.kind, ch.snr_db, wpm, &label);
                if matches!(m.kind, SpotKind::CallingCq) {
                    ch.text.clear();
                }
            }
        }

        let timeout = self.config.channel_timeout;
        self.channels
            .retain(|_, ch| ch.last_seen.elapsed() <= timeout);
        self.store.prune(self.config.spot_max_age);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::morse::encode_char;
    use std::f32::consts::TAU;

    fn keyed_iq(text: &str, wpm: f32, rate: f32, offset: f32) -> Vec<Complex32> {
        let dot = (1.2 / wpm * rate) as usize;
        let mut out = Vec::new();
        let mut phase = 0.0f32;
        let mut push = |on: bool, len: usize, phase: &mut f32, out: &mut Vec<Complex32>| {
            for _ in 0..len {
                *phase += TAU * offset / rate;
                let (s, c) = phase.sin_cos();
                let amp = if on { 1.0 } else { 0.0 };
                out.push(Complex32 { re: amp * c, im: amp * s });
            }
        };
        push(false, dot * 8, &mut phase, &mut out);
        for (ci, ch) in text.chars().enumerate() {
            if ci > 0 {
                push(false, dot * 3, &mut phase, &mut out);
            }
            for (ei, el) in encode_char(ch).unwrap_or("").chars().enumerate() {
                if ei > 0 {
                    push(false, dot, &mut phase, &mut out);
                }
                let len = if el == '-' { dot * 3 } else { dot };
                push(true, len, &mut phase, &mut out);
            }
        }
        push(false, dot * 10, &mut phase, &mut out);
        out
    }

    fn spectrum_with_peak(offset: f32, rate: f32, n: usize) -> Vec<f32> {
        let mut row = vec![-100.0f32; n];
        let bin = ((offset / rate) * n as f32 + n as f32 / 2.0).round() as usize;
        if bin < n {
            row[bin] = -20.0;
        }
        row
    }

    #[test]
    fn decodes_signal_into_spot() {
        let rate = 12_000.0;
        let offset = 1_000.0;
        let iq = keyed_iq("CQ", 25.0, rate, offset);
        let spectrum = spectrum_with_peak(offset, rate, 2048);
        let mut sk = Skimmer::new(SkimmerConfig::default());
        // Feed in chunks so peaks register and decoders run continuously.
        for chunk in iq.chunks(1024) {
            sk.process(chunk, rate, &spectrum, 7_030_000.0);
        }
        assert!(sk.store().len() >= 1, "no spot produced");
    }
}
