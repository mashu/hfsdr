//! In-band skimmer engine: a peak-driven bank of narrowband CW decoders.

use std::collections::HashMap;
use std::time::Instant;

use rayon::prelude::*;

use crate::dsp::{design_gaussian_lowpass, DecimFilterKind, Decimator, FirFilter, IqRotator};
use crate::source::Complex32;

use super::adaptive::AdaptiveCwDecoder;
use super::bigram::BigramCwDecoder;
use super::config::{DecoderParams, SkimmerConfig, SkimmerDecoderKind};
use super::decoder::CwDecoder;
use super::envelope::{DecodeGate, KeyingEnvelope};
use super::patterns::analyze;
use super::quality::decode_is_garbage;
use super::peaks::{
    detect_peaks_with_floor, noise_floor_db_into, offset_hz_to_bin, strongest_offset_hz_with_floor,
    Peak,
};
use super::scp::MasterScp;
use super::spots::SpotStore;

/// Intermediate complex rate after anti-aliased decimation.
const MID_RATE_TARGET_HZ: f32 = 2_000.0;
/// Envelope rate the Morse decoders run at.
const ENV_RATE_TARGET_HZ: f32 = 500.0;
/// Slow AFC pull range — recenters the narrow filter on the exact carrier
/// (spectrum peak bins can be tens of Hz coarse on wideband inputs).
const AFC_MAX_DEV_HZ: f32 = 30.0;
/// Fraction of the measured residual applied per processed block.
const AFC_GAIN: f32 = 0.2;

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
    /// Fine frequency correction from the AFC discriminator.
    afc_hz: f32,
    afc_prev: Complex32,
    rotator: IqRotator,
    decim: Decimator,
    mid_rate: f32,
    fir: FirFilter,
    fir_cutoff_hz: f32,
    env_decim: usize,
    env_acc: f32,
    env_count: usize,
    env_rate: f32,
    env_buf: Vec<f32>,
    gate_env: KeyingEnvelope,
    gate: DecodeGate,
    decoder: ChannelDecoder,
    text: String,
    last_seen: Instant,
    snr_db: f32,
    min_decode_snr_db: f32,
}

impl DecoderChannel {
    fn new(offset_hz: f32, iq_rate: f32, snr_db: f32, config: &SkimmerConfig) -> Self {
        let iq_rate = iq_rate.max(1.0);
        let d1 = (iq_rate / MID_RATE_TARGET_HZ).round().clamp(1.0, 256.0) as usize;
        let mid_rate = iq_rate / d1 as f32;
        let cutoff = channel_cutoff_hz(config.lpf_cutoff_hz, mid_rate);
        let env_decim = (mid_rate / ENV_RATE_TARGET_HZ).round().max(1.0) as usize;
        let env_rate = mid_rate / env_decim as f32;
        Self {
            offset_hz,
            afc_hz: 0.0,
            afc_prev: Complex32 { re: 0.0, im: 0.0 },
            rotator: IqRotator::new(false),
            decim: Decimator::with_factor(iq_rate, d1, DecimFilterKind::LinearFir),
            mid_rate,
            fir: design_gaussian_lowpass(mid_rate, cutoff * 2.0),
            fir_cutoff_hz: cutoff,
            env_decim,
            env_acc: 0.0,
            env_count: 0,
            env_rate,
            env_buf: Vec::with_capacity(512),
            gate_env: KeyingEnvelope::new(config.decoder_params.envelope, env_rate),
            gate: DecodeGate::new(env_rate, config.decode_gate_ms),
            decoder: ChannelDecoder::new(config.decoder, env_rate, config.decoder_params),
            text: String::new(),
            last_seen: Instant::now(),
            snr_db,
            min_decode_snr_db: config.min_decode_snr_db,
        }
    }

    /// Run envelope tracking when the peak is plausible; decode only when keyed.
    fn should_run_dsp(&self, _poll: u64) -> bool {
        if self.gate.is_armed() {
            return true;
        }
        self.snr_db >= self.min_decode_snr_db + 2.0
    }

    fn sync_filter(&mut self, lpf_cutoff_hz: f32) {
        let cutoff = channel_cutoff_hz(lpf_cutoff_hz, self.mid_rate);
        if (cutoff - self.fir_cutoff_hz).abs() > 1.0 {
            self.fir = design_gaussian_lowpass(self.mid_rate, cutoff * 2.0);
            self.fir_cutoff_hz = cutoff;
        }
    }

    fn process(&mut self, iq: &[Complex32], iq_rate: f32, lpf_cutoff_hz: f32) -> String {
        if self.snr_db < self.min_decode_snr_db {
            self.gate.reset();
            return String::new();
        }
        self.sync_filter(lpf_cutoff_hz);
        self.env_buf.clear();
        self.rotator
            .sync_step(self.offset_hz + self.afc_hz, iq_rate);
        let mut afc_prev = self.afc_prev;
        let mut afc_acc = 0.0f64;
        let mut afc_weight = 0.0f64;
        for &s in iq {
            let shifted = self.rotator.mix_sample(s);
            if let Some(mid) = self.decim.push(shifted, false) {
                let z = self.fir.process_complex(mid);
                // Magnitude-weighted frequency discriminator for the AFC.
                let cross_re = z.re * afc_prev.re + z.im * afc_prev.im;
                let cross_im = z.im * afc_prev.re - z.re * afc_prev.im;
                let w = (cross_re * cross_re + cross_im * cross_im).sqrt();
                if w > 0.0 {
                    afc_acc += (cross_im.atan2(cross_re) * w) as f64;
                    afc_weight += w as f64;
                }
                afc_prev = z;
                self.env_acc += z.norm();
                self.env_count += 1;
                if self.env_count >= self.env_decim {
                    self.env_buf.push(self.env_acc / self.env_decim as f32);
                    self.env_acc = 0.0;
                    self.env_count = 0;
                }
            }
        }
        self.afc_prev = afc_prev;
        if afc_weight > 1e-12 {
            let rad_per_sample = (afc_acc / afc_weight) as f32;
            let residual_hz = rad_per_sample / std::f32::consts::TAU * self.mid_rate;
            self.afc_hz =
                (self.afc_hz + AFC_GAIN * residual_hz).clamp(-AFC_MAX_DEV_HZ, AFC_MAX_DEV_HZ);
        }
        if self.env_buf.is_empty() {
            return String::new();
        }
        self.last_seen = Instant::now();
        for &mag in &self.env_buf {
            let step = self.gate_env.update(mag);
            self.gate.feed(&step);
        }
        if !self.gate.is_armed() {
            if decode_is_garbage(&self.text) {
                self.text.clear();
            }
            return String::new();
        }
        self.decoder.push_audio(&self.env_buf, self.env_rate)
    }

    fn is_keyed(&self) -> bool {
        self.gate.is_armed()
    }
}

/// Clamp the configured single-sided cutoff to what the mid rate supports.
fn channel_cutoff_hz(lpf_cutoff_hz: f32, mid_rate: f32) -> f32 {
    lpf_cutoff_hz.clamp(25.0, (0.35 * mid_rate).max(25.0))
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

    pub fn keyed_channels(&self) -> usize {
        self.channels.values().filter(|ch| ch.is_keyed()).count()
    }

    /// Hidden debug helper for integration tests and capture replay.
    #[doc(hidden)]
    pub fn debug_channels(&self) -> Vec<(f32, String, f32)> {
        self.live_channels()
            .into_iter()
            .map(|ch| (ch.offset_hz, ch.text, ch.snr_db))
            .collect()
    }

    /// Snapshot of in-flight decoder channels for the UI.
    pub fn live_channels(&self) -> Vec<super::DecodeChannel> {
        let center = self.center_hz;
        let mut out: Vec<_> = self
            .channels
            .values()
            .filter_map(|ch| {
                let keyed = ch.is_keyed();
                let text = if decode_is_garbage(&ch.text) && !keyed {
                    String::new()
                } else {
                    ch.text.clone()
                };
                if !keyed && text.is_empty() {
                    return None;
                }
                Some(super::DecodeChannel {
                    offset_hz: ch.offset_hz,
                    frequency_hz: center + ch.offset_hz as f64,
                    text,
                    snr_db: ch.snr_db,
                    wpm: ch.decoder.wpm(),
                    keyed,
                })
            })
            .collect();
        out.sort_by(|a, b| a.offset_hz.total_cmp(&b.offset_hz));
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

    /// Inject an SCP database directly (tests and capture replay).
    #[doc(hidden)]
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
        // Focus: only decode near the tuned frequency (bounded CPU); the
        // whole band is still displayed, just not all of it decoded.
        if self.config.focus_span_hz > 0.0 {
            let half = self.config.focus_span_hz * 0.5;
            let center = self.config.focus_center_hz;
            peaks.retain(|p| (p.offset_hz + spectrum_pan_hz - center).abs() <= half);
            let margin = half + self.config.bucket_hz;
            self.channels
                .retain(|_, ch| (ch.offset_hz - center).abs() <= margin);
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
                let weakest = &self.channels[&weakest_key];
                // A channel that is actively decoding keeps its slot unless the
                // newcomer is clearly stronger — transient peaks must not
                // thrash decoders mid-transmission.
                let guard = if weakest.gate.is_armed() { 6.0 } else { 1.0 };
                if p.snr_db > weakest.snr_db + guard {
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

    /// Offline diagnostic: dump the channel envelope chain for a capture.
    /// CAPTURE=path OFFSET=hz OUT=csv cargo test --lib diagnose_capture -- --ignored --nocapture
    #[test]
    #[ignore]
    fn diagnose_capture() {
        use std::io::Read as _;
        let path = std::env::var("CAPTURE").expect("CAPTURE env");
        let offset: f32 = std::env::var("OFFSET").map(|s| s.parse().unwrap()).unwrap_or(-11.7);
        let out_path = std::env::var("OUT").unwrap_or_else(|_| "/tmp/env.csv".into());
        let meta = crate::read_meta(std::path::Path::new(&path)).expect("meta");
        let mut file = std::fs::File::open(&path).expect("open");
        use std::io::Seek as _;
        file.seek(std::io::SeekFrom::Start(32)).unwrap();
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(file).read_to_end(&mut raw).unwrap();
        let samples: Vec<Complex32> = raw
            .chunks_exact(8)
            .map(|c| Complex32 {
                re: f32::from_le_bytes(c[0..4].try_into().unwrap()),
                im: f32::from_le_bytes(c[4..8].try_into().unwrap()),
            })
            .collect();
        let rate = meta.sample_rate as f32;
        let dc: (f32, f32) = samples.iter().fold((0.0, 0.0), |a, s| (a.0 + s.re, a.1 + s.im));
        eprintln!(
            "capture {} samples @ {rate} Hz; DC = ({:.5}, {:.5})",
            samples.len(),
            dc.0 / samples.len() as f32,
            dc.1 / samples.len() as f32
        );

        let config = SkimmerConfig { min_decode_snr_db: 0.0, min_snr_db: 0.0, ..SkimmerConfig::default() };
        let mut ch = DecoderChannel::new(offset, rate, 99.0, &config);
        let mut env_all: Vec<f32> = Vec::new();
        let mut afc_track: Vec<f32> = Vec::new();
        for chunk in samples.chunks(2048) {
            let _ = ch.process(chunk, rate, config.lpf_cutoff_hz);
            env_all.extend_from_slice(&ch.env_buf);
            afc_track.push(ch.afc_hz);
        }
        eprintln!("env samples: {} @ {} Hz, afc first/last: {:.2} / {:.2} Hz", env_all.len(), ch.env_rate, afc_track.first().unwrap_or(&0.0), afc_track.last().unwrap_or(&0.0));
        let mut sorted = env_all.clone();
        sorted.sort_by(f32::total_cmp);
        let pct = |p: f32| sorted[((sorted.len() - 1) as f32 * p) as usize];
        eprintln!(
            "env percentiles: p05 {:.4} p25 {:.4} p50 {:.4} p75 {:.4} p90 {:.4} p99 {:.4}",
            pct(0.05), pct(0.25), pct(0.50), pct(0.75), pct(0.90), pct(0.99)
        );

        // Replay tracker + keyer over the envelope and log everything.
        let mut env = KeyingEnvelope::new(config.decoder_params.envelope, ch.env_rate);
        let mut keyer = crate::skimmer::timing::Keyer::new(ch.env_rate);
        let mut clock = crate::skimmer::timing::ElementClock::new(config.decoder_params.initial_wpm);
        keyer.set_dot_seconds(clock.dot_seconds());
        let mut key = false;
        let verbose = std::env::var("VERBOSE").is_ok();
        let mut csv = String::from("i,env,noise_thr_low,thr_high,key,signal\n");
        let mut marks: Vec<f32> = Vec::new();
        let mut spaces: Vec<f32> = Vec::new();
        for (i, &x) in env_all.iter().enumerate() {
            let step = env.update(x);
            key = if !step.signal_present { false } else if key { step.env >= step.thr_low } else { step.env > step.thr_high };
            if let Some(ev) = keyer.step(key) {
                match ev {
                    crate::skimmer::timing::KeyEvent::Mark(s) => {
                        clock.classify_mark(s);
                        keyer.set_dot_seconds(clock.dot_seconds());
                        marks.push(s);
                        if verbose {
                            eprintln!(
                                "t={:6.2}s mark {:4.0} ms   dot {:4.0} dash {:4.0}",
                                i as f32 / ch.env_rate, s * 1e3,
                                clock.dot_seconds() * 1e3, clock.dash_seconds() * 1e3
                            );
                        }
                    }
                    crate::skimmer::timing::KeyEvent::Space(s) => {
                        clock.record_space(s);
                        if verbose && s / clock.dot_seconds() > 1.6 {
                            eprintln!(
                                "t={:6.2}s space {:5.1} dits  kind {:?}",
                                i as f32 / ch.env_rate, s / clock.dot_seconds(),
                                clock.space_kind(s)
                            );
                        }
                        spaces.push(s);
                    }
                }
            }
            csv.push_str(&format!(
                "{i},{:.5},{:.5},{:.5},{},{}\n",
                step.env, step.thr_low, step.thr_high, key as u8, step.signal_present as u8
            ));
        }
        std::fs::write(&out_path, csv).unwrap();
        let fmt = |v: &Vec<f32>| {
            let mut s: Vec<f32> = v.clone();
            s.sort_by(f32::total_cmp);
            if s.is_empty() { return "none".into(); }
            format!(
                "n={} min {:.0}ms p25 {:.0} p50 {:.0} p75 {:.0} max {:.0}",
                s.len(), s[0] * 1e3, s[s.len() / 4] * 1e3, s[s.len() / 2] * 1e3, s[3 * s.len() / 4] * 1e3, s[s.len() - 1] * 1e3
            )
        };
        eprintln!("marks:  {}", fmt(&marks));
        eprintln!("spaces: {}", fmt(&spaces));
        eprintln!("clock dot {:.0} ms dash {:.0} ms → {:.0} wpm", clock.dot_seconds() * 1e3, clock.dash_seconds() * 1e3, 1.2 / clock.dot_seconds());
        eprintln!("csv written to {out_path}");
    }
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
        let iq = keyed_iq("CQ DE W1AW", 25.0, rate, offset);
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
        assert!(
            sk.store()
                .sorted(SpotSort::SnrDesc)
                .iter()
                .any(|s| s.callsign.is_some()),
            "no callsign spot produced"
        );
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
        let iq = keyed_iq("CQ DE W1AW", 25.0, rate, offset);
        let spectrum = spectrum_with_peak(offset, rate, 2048);
        let scp = MasterScp::from_text(SAMPLE_SCP);
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
        sk.replace_scp(scp);
        for chunk in iq.chunks(1024) {
            sk.process(chunk, rate, &spectrum, rate, 0.0, 7_030_000.0);
        }
        assert!(
            sk.store()
                .sorted(SpotSort::SnrDesc)
                .iter()
                .any(|s| s.callsign.is_some()),
            "adaptive decoder produced no callsign spot"
        );
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
