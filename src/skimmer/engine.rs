//! In-band skimmer engine: a peak-driven bank of narrowband CW decoders.

use std::collections::HashMap;
use std::time::Instant;

use rayon::prelude::*;

use crate::dsp::{design_gaussian_lowpass, FirFilter, IqRotator};
use crate::source::Complex32;

use super::adaptive::AdaptiveCwDecoder;
use super::bigram::BigramCwDecoder;
use super::config::{DecoderParams, SkimmerConfig, SkimmerDecoderKind};
use super::decoder::CwDecoder;
use super::envelope::{DecodeGate, KeyingEnvelope};
use super::patterns::analyze;
use super::peaks::{
    detect_peaks_with_floor, noise_floor_db_into, offset_hz_to_bin, strongest_offset_hz_with_floor,
    Peak,
};
use super::scp::MasterScp;
use super::spots::{SpotKind, SpotStore};

/// Gaussian FIR channel filter for skimmer isolation (replaces 2-pole IIR).
struct ChannelFilter {
    fir: FirFilter,
    last_rate: f32,
    last_cutoff: f32,
}

impl ChannelFilter {
    fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        Self {
            fir: design_gaussian_lowpass(sample_rate, cutoff_hz * 2.0),
            last_rate: sample_rate,
            last_cutoff: cutoff_hz,
        }
    }

    fn sync(&mut self, sample_rate: f32, cutoff_hz: f32) {
        if (sample_rate - self.last_rate).abs() > 1.0
            || (cutoff_hz - self.last_cutoff).abs() > 1.0
        {
            self.fir = design_gaussian_lowpass(sample_rate, cutoff_hz * 2.0);
            self.last_rate = sample_rate;
            self.last_cutoff = cutoff_hz;
        }
    }

    fn process(&mut self, sample: Complex32) -> Complex32 {
        self.fir.process_complex(sample)
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
    rotator: IqRotator,
    decim_factor: usize,
    decim_counter: usize,
    filter: ChannelFilter,
    lpf_cutoff_hz: f32,
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
            rotator: IqRotator::new(false),
            decim_factor,
            decim_counter: 0,
            filter: ChannelFilter::new(audio_rate, config.lpf_cutoff_hz),
            lpf_cutoff_hz: config.lpf_cutoff_hz,
            audio_rate,
            gate_env: KeyingEnvelope::new(config.decoder_params.envelope),
            gate: DecodeGate::new(audio_rate, config.decode_gate_ms),
            decoder: ChannelDecoder::new(config.decoder, audio_rate, config.decoder_params),
            audio: Vec::with_capacity(4096),
            text: String::new(),
            last_seen: Instant::now(),
            snr_db,
            min_decode_snr_db: config.min_decode_snr_db,
        }
    }

    /// Skip the expensive IQ mix when the channel is idle but poll occasionally to catch key-down.
    fn should_run_dsp(&self, poll: u64) -> bool {
        if self.gate.is_armed() {
            return true;
        }
        if self.snr_db < self.min_decode_snr_db {
            return false;
        }
        if self.snr_db >= self.min_decode_snr_db + 8.0 {
            return true;
        }
        poll.is_multiple_of(4)
    }

    fn process(&mut self, iq: &[Complex32], iq_rate: f32, lpf_cutoff_hz: f32) -> String {
        if self.snr_db < self.min_decode_snr_db {
            self.gate.reset();
            return String::new();
        }
        self.filter.sync(self.audio_rate, lpf_cutoff_hz);
        self.lpf_cutoff_hz = lpf_cutoff_hz;
        self.audio.clear();
        let est_audio = iq.len() / self.decim_factor.max(1) + 1;
        if self.audio.capacity() < est_audio {
            self.audio.reserve(est_audio);
        }
        let offset = self.offset_hz;
        let decim_factor = self.decim_factor;
        let mut decim_counter = self.decim_counter;
        self.rotator.sync_step(offset, iq_rate);
        for &s in iq {
            let shifted = self.rotator.mix_sample(s);
            decim_counter += 1;
            if decim_counter >= decim_factor {
                decim_counter = 0;
                let filtered = self.filter.process(shifted);
                self.audio.push(filtered.norm());
            }
        }
        self.decim_counter = decim_counter;
        if self.audio.is_empty() {
            return String::new();
        }
        self.last_seen = Instant::now();
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
    floor_scratch: Vec<f32>,
    poll_serial: u64,
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
            floor_scratch: Vec::with_capacity(4096),
            poll_serial: 0,
        }
    }

    pub fn store(&self) -> &SpotStore {
        &self.store
    }

    pub fn active_channels(&self) -> usize {
        self.channels.len()
    }

    /// Hidden debug helper for integration tests and capture replay.
    #[doc(hidden)]
    pub fn debug_channels(&self) -> Vec<(f32, String, f32)> {
        let mut out: Vec<_> = self
            .channels
            .values()
            .map(|ch| (ch.offset_hz, ch.text.clone(), ch.snr_db))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
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
        self.poll_serial = self.poll_serial.wrapping_add(1);
        let poll = self.poll_serial;
        self.center_hz = center_hz;
        if (center_hz - self.last_center_hz).abs() > 1.0 {
            self.channels.clear();
            self.last_center_hz = center_hz;
        }

        let floor = noise_floor_db_into(spectrum, &mut self.floor_scratch);
        let mut peaks = detect_peaks_with_floor(
            spectrum,
            spectrum_rate,
            self.config.min_snr_db,
            self.config.min_separation_bins,
            floor,
        );
        // Always consider the strongest bin in the passband — max-hold / wide peaks can
        // fail the local-max test in detect_peaks and miss the signal you are listening to.
        if let Some(off) = strongest_offset_hz_with_floor(
            spectrum,
            spectrum_rate,
            spectrum_pan_hz,
            spectrum_rate * 0.45,
            floor,
        ) {
            let bin = offset_hz_to_bin(off, spectrum.len(), spectrum_rate);
            let snr = spectrum[bin] - floor;
            if snr >= self.config.min_snr_db {
                let dup = peaks.iter().any(|p| {
                    self.bucket(p.offset_hz + spectrum_pan_hz)
                        == self.bucket(off + spectrum_pan_hz)
                });
                if !dup {
                    peaks.push(Peak {
                        offset_hz: off,
                        snr_db: snr,
                        bin,
                    });
                }
            }
        }
        peaks.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db));
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
            } else if let Some(weakest_key) = self
                .channels
                .iter()
                .min_by(|(_, a), (_, b)| a.snr_db.total_cmp(&b.snr_db))
                .map(|(k, _)| *k)
            {
                let weakest_snr = self.channels[&weakest_key].snr_db;
                if p.snr_db > weakest_snr + 1.0 {
                    self.channels.remove(&weakest_key);
                    self.channels.insert(
                        key,
                        DecoderChannel::new(offset_hz, iq_rate, p.snr_db, &self.config),
                    );
                }
            }
        }

        let center = self.center_hz;
        let label = self.config.source_label.clone();
        let max_chars = self.config.decoder_params.max_text_chars;
        let lpf = self.config.lpf_cutoff_hz;
        let require_scp = self.config.require_scp && self.scp.is_loaded();

        let mut channel_vec: Vec<(i64, DecoderChannel)> = self.channels.drain().collect();
        if channel_vec.len() >= 2 {
            channel_vec.par_iter_mut().for_each(|(_, ch)| {
                if !ch.should_run_dsp(poll) {
                    return;
                }
                let delta = ch.process(iq, iq_rate, lpf);
                if !delta.is_empty() {
                    ch.text.push_str(&delta);
                }
            });
        } else {
            for (_, ch) in channel_vec.iter_mut() {
                if !ch.should_run_dsp(poll) {
                    continue;
                }
                let delta = ch.process(iq, iq_rate, lpf);
                if !delta.is_empty() {
                    ch.text.push_str(&delta);
                }
            }
        }

        for (key, mut ch) in channel_vec {
            if ch.text.len() > max_chars {
                let cut = ch.text.len() - max_chars;
                ch.text.drain(..cut);
            }
            if let Some(m) = analyze(&ch.text, Some(&self.scp), require_scp) {
                let freq = center + ch.offset_hz as f64;
                let wpm = ch.decoder.wpm();
                let rank = m
                    .callsign
                    .as_ref()
                    .map(|c| {
                        let scp_rank = self.scp.callsign_rank(c);
                        if scp_rank > 0 {
                            scp_rank
                        } else {
                            super::patterns::looks_like_callsign(c) as u32 * c.len() as u32
                        }
                    })
                    .unwrap_or(0);
                if let Some(call) = &m.callsign {
                    self.store
                        .observe(freq, m.callsign.clone(), rank, m.kind, ch.snr_db, wpm, &label);
                    trim_decoded_prefix(&mut ch.text, call);
                } else if matches!(m.kind, SpotKind::CallingCq) {
                    self.store
                        .observe(freq, None, rank, m.kind, ch.snr_db, wpm, &label);
                }
            }
            self.channels.insert(key, ch);
        }

        let timeout = self.config.channel_timeout();
        self.channels
            .retain(|_, ch| ch.last_seen.elapsed() <= timeout);
        self.store.prune(self.config.spot_store_max_age());
    }
}

/// Drop decoded prefix through the first occurrence of `call` (keeps later repeats).
fn trim_decoded_prefix(text: &mut String, call: &str) {
    let upper = text.to_ascii_uppercase();
    let call = call.to_ascii_uppercase();
    if let Some(pos) = upper.find(&call) {
        let end = (pos + call.len()).min(text.len());
        text.drain(..end);
    } else {
        text.clear();
    }
    let trim = text.len() - text.trim_start().len();
    if trim > 0 {
        text.drain(..trim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::config::{DecoderParams, EnvelopeSettings, SkimmerConfig, SkimmerDecoderKind};
    use crate::skimmer::morse::encode_char;
    use crate::skimmer::spots::SpotSort;
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
    fn cq_then_callsign_across_chunks() {
        let rate = 12_000.0;
        let offset = 1_000.0;
        let spectrum = spectrum_with_peak(offset, rate, 2048);
        let scp = MasterScp::from_text("VER20260202\nW1AW\n");
        let mut sk = Skimmer::new(SkimmerConfig {
            decoder: SkimmerDecoderKind::Bigram,
            require_scp: true,
            min_snr_db: 10.0,
            min_decode_snr_db: 10.0,
            decode_gate_ms: 25.0,
            ..SkimmerConfig::default()
        });
        sk.replace_scp(scp);
        let cq = keyed_iq("CQ", 25.0, rate, offset);
        let call = keyed_iq("CQ W1AW", 25.0, rate, offset);
        for chunk in cq.chunks(512) {
            sk.process(chunk, rate, &spectrum, rate, 0.0, 7_030_000.0);
        }
        for chunk in call.chunks(512) {
            sk.process(chunk, rate, &spectrum, rate, 0.0, 7_030_000.0);
        }
        let spots = sk.store().sorted(SpotSort::SnrDesc);
        assert!(
            spots.iter().any(|s| s.callsign.as_deref() == Some("W1AW")),
            "expected W1AW after CQ preamble, got {:?}",
            spots
                .iter()
                .map(|s| (s.callsign.clone(), s.kind))
                .collect::<Vec<_>>()
        );
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

    #[test]
    fn trim_decoded_prefix_removes_through_call() {
        let mut text = "CQ CQ DE W1AW".to_string();
        trim_decoded_prefix(&mut text, "W1AW");
        assert!(text.is_empty());

        let mut tail = "CQ W1AW TEST".to_string();
        trim_decoded_prefix(&mut tail, "W1AW");
        assert_eq!(tail.trim(), "TEST");
    }

    #[test]
    fn trim_decoded_prefix_clears_when_call_missing() {
        let mut text = "CQ CQ".to_string();
        trim_decoded_prefix(&mut text, "W1AW");
        assert!(text.is_empty());
    }
}
