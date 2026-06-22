//! In-band skimmer engine: a peak-driven bank of narrowband CW decoders.

use std::collections::HashMap;
use std::time::Instant;

use crate::source::Complex32;

use super::adaptive::AdaptiveCwDecoder;
use super::bigram::BigramCwDecoder;
use super::config::{DecoderParams, SkimmerConfig, SkimmerDecoderKind};
use super::decoder::CwDecoder;
use super::envelope::{DecodeGate, KeyingEnvelope};
use super::patterns::analyze;
use super::peaks::detect_peaks;
use super::scp::MasterScp;
use super::spots::{SpotKind, SpotStore};

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

enum ChannelDecoder {
    Adaptive(AdaptiveCwDecoder),
    Bigram(BigramCwDecoder),
}

impl ChannelDecoder {
    fn new(kind: SkimmerDecoderKind, audio_rate: f32, params: DecoderParams) -> Self {
        match kind {
            SkimmerDecoderKind::Adaptive => {
                Self::Adaptive(AdaptiveCwDecoder::with_params(audio_rate, params))
            }
            SkimmerDecoderKind::Bigram => {
                Self::Bigram(BigramCwDecoder::with_params(audio_rate, params))
            }
        }
    }

    fn push_audio(&mut self, audio: &[f32], sample_rate: f32) -> String {
        match self {
            Self::Adaptive(d) => d.push_audio(audio, sample_rate),
            Self::Bigram(d) => d.push_audio(audio, sample_rate),
        }
    }

    fn wpm(&self) -> f32 {
        match self {
            Self::Adaptive(d) => d.wpm(),
            Self::Bigram(d) => d.wpm(),
        }
    }
}

struct DecoderChannel {
    offset_hz: f32,
    phase: f32,
    decim_factor: usize,
    decim_counter: usize,
    filter: ComplexLowpass,
    audio_rate: f32,
    gate_env: KeyingEnvelope,
    gate: DecodeGate,
    decoder: ChannelDecoder,
    audio: Vec<f32>,
    text: String,
    last_seen: Instant,
    snr_db: f32,
    min_decode_snr_db: f32,
}

impl DecoderChannel {
    fn new(offset_hz: f32, iq_rate: f32, snr_db: f32, config: &SkimmerConfig) -> Self {
        let target = config.target_audio_rate_hz.max(1_000.0);
        let decim_factor = (iq_rate / target).round().clamp(1.0, 256.0) as usize;
        let audio_rate = iq_rate / decim_factor as f32;
        Self {
            offset_hz,
            phase: 0.0,
            decim_factor,
            decim_counter: 0,
            filter: ComplexLowpass::new(audio_rate, config.lpf_cutoff_hz),
            audio_rate,
            gate_env: KeyingEnvelope::new(config.decoder_params.envelope),
            gate: DecodeGate::new(audio_rate, config.decode_gate_ms),
            decoder: ChannelDecoder::new(config.decoder, audio_rate, config.decoder_params),
            audio: Vec::new(),
            text: String::new(),
            last_seen: Instant::now(),
            snr_db,
            min_decode_snr_db: config.min_decode_snr_db,
        }
    }

    fn process(&mut self, iq: &[Complex32], iq_rate: f32) -> String {
        if self.snr_db < self.min_decode_snr_db {
            self.gate.reset();
            return String::new();
        }
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
        if self.audio.is_empty() {
            return String::new();
        }
        let mut gated = false;
        for &mag in &self.audio {
            let step = self.gate_env.update(mag);
            if self.gate.feed(&step) {
                gated = true;
            }
        }
        if !gated || !self.gate.is_armed() {
            return String::new();
        }
        self.decoder.push_audio(&self.audio, self.audio_rate)
    }
}

/// Peak-driven bank of CW decoders feeding a spot store.
pub struct Skimmer {
    config: SkimmerConfig,
    scp: MasterScp,
    channels: HashMap<i64, DecoderChannel>,
    store: SpotStore,
    center_hz: f64,
    last_center_hz: f64,
}

impl Skimmer {
    pub fn new(config: SkimmerConfig) -> Self {
        Self {
            config: config.clamped(),
            scp: MasterScp::discover(),
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

    pub fn set_config(&mut self, config: SkimmerConfig) {
        let config = config.clamped();
        if self.config.channel_dsp_changed(&config) {
            self.channels.clear();
        }
        self.config = config;
    }

    pub fn clear(&mut self) {
        self.channels.clear();
        self.store.clear();
    }

    pub fn scp(&self) -> &MasterScp {
        &self.scp
    }

    pub fn reload_scp_from(&mut self, path: &std::path::Path) -> bool {
        match MasterScp::from_file(path) {
            Ok(scp) if scp.is_loaded() => {
                self.scp = scp;
                true
            }
            _ => false,
        }
    }

    pub fn reload_scp_discover(&mut self) -> bool {
        let scp = MasterScp::discover();
        if scp.is_loaded() {
            self.scp = scp;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn replace_scp(&mut self, scp: MasterScp) {
        self.scp = scp;
    }

    fn bucket(&self, offset_hz: f32) -> i64 {
        (offset_hz / self.config.bucket_hz).round() as i64
    }

    pub fn process(
        &mut self,
        iq: &[Complex32],
        iq_rate: f32,
        spectrum: &[f32],
        spectrum_rate: f32,
        spectrum_pan_hz: f32,
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
            spectrum_rate,
            self.config.min_snr_db,
            self.config.min_separation_bins,
        );
        for p in &peaks {
            let offset_hz = p.offset_hz + spectrum_pan_hz;
            let key = self.bucket(offset_hz);
            if let Some(ch) = self.channels.get_mut(&key) {
                ch.last_seen = Instant::now();
                ch.snr_db = p.snr_db;
                ch.min_decode_snr_db = self.config.min_decode_snr_db;
            } else if self.channels.len() < self.config.max_channels {
                self.channels.insert(
                    key,
                    DecoderChannel::new(offset_hz, iq_rate, p.snr_db, &self.config),
                );
            }
        }

        let center = self.center_hz;
        let label = self.config.source_label.clone();
        let max_chars = self.config.decoder_params.max_text_chars;
        for ch in self.channels.values_mut() {
            let delta = ch.process(iq, iq_rate);
            if delta.is_empty() {
                continue;
            }
            ch.text.push_str(&delta);
            if ch.text.len() > max_chars {
                let cut = ch.text.len() - max_chars;
                ch.text.drain(..cut);
            }
            if let Some(m) = analyze(
                &ch.text,
                Some(&self.scp),
                self.config.require_scp && self.scp.is_loaded(),
            ) {
                let freq = center + ch.offset_hz as f64;
                let wpm = ch.decoder.wpm();
                self.store
                    .observe(freq, m.callsign, m.kind, ch.snr_db, wpm, &label);
                if matches!(m.kind, SpotKind::CallingCq) {
                    ch.text.clear();
                }
            }
        }

        let timeout = self.config.channel_timeout();
        self.channels
            .retain(|_, ch| ch.last_seen.elapsed() <= timeout);
        self.store.prune(self.config.spot_store_max_age());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::config::{DecoderParams, EnvelopeSettings, SkimmerConfig, SkimmerDecoderKind};
    use crate::skimmer::morse::encode_char;
    use std::f32::consts::TAU;

    const SAMPLE_SCP: &str = "VER20260202\nOH2BH\n";

    fn keyed_iq(text: &str, wpm: f32, rate: f32, offset: f32) -> Vec<Complex32> {
        let dot = (1.2 / wpm * rate) as usize;
        let mut out = Vec::new();
        let mut phase = 0.0f32;
        let push = |on: bool, len: usize, phase: &mut f32, out: &mut Vec<Complex32>| {
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
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let mut sk = Skimmer::new(SkimmerConfig {
            decoder: SkimmerDecoderKind::Bigram,
            require_scp: false,
            min_snr_db: 10.0,
            min_decode_snr_db: 10.0,
            decode_gate_ms: 25.0,
            ..SkimmerConfig::default()
        });
        sk.replace_scp(scp);
        for chunk in iq.chunks(1024) {
            sk.process(chunk, rate, &spectrum, rate, 0.0, 7_030_000.0);
        }
        assert!(!sk.store().is_empty(), "no spot produced");
    }

    #[test]
    fn adaptive_decoder_produces_spot() {
        let rate = 12_000.0;
        let offset = 1_000.0;
        let iq = keyed_iq("CQ", 25.0, rate, offset);
        let spectrum = spectrum_with_peak(offset, rate, 2048);
        let mut sk = Skimmer::new(SkimmerConfig {
            decoder: SkimmerDecoderKind::Adaptive,
            require_scp: false,
            min_snr_db: 10.0,
            min_decode_snr_db: 10.0,
            decode_gate_ms: 25.0,
            decoder_params: DecoderParams {
                envelope: EnvelopeSettings {
                    thr_low: 0.4,
                    thr_high: 0.55,
                    min_span_fraction: 0.05,
                },
                ..DecoderParams::default()
            },
            ..SkimmerConfig::default()
        });
        for chunk in iq.chunks(1024) {
            sk.process(chunk, rate, &spectrum, rate, 0.0, 7_030_000.0);
        }
        assert!(!sk.store().is_empty(), "adaptive decoder produced no spot");
    }
}
