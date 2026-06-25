//! Single CW listen chain, composed of toggleable stages.
//!
//! Fixed order (each optional stage is bypassed when disabled):
//! ```text
//! IQ → [noise blanker] → NCO shift → decimate → [manual IQ notches]
//!    → channel filter → AGC/manual gain → product detector (BFO demod)
//!    → [APF] → [auto-notch] → [noise reduction] → audio
//! ```
//! Manual notches and the channel filter run on complex IQ before demod.
//! Auto-notch and NR are post-demod polish: auto-notch uses a BFO guard on audio;
//! the IQ equivalent of NR is narrowing the channel filter.
//! All stages are preallocated. IQ ingress (noise blanker → NCO → decimation) runs in
//! block passes; the post-decimation chain (notches → FIR → AGC → BFO → polish) stays
//! sample-sequential because IIR/AGC state must be ordered.

use super::super::freq_offset::ListenOrigin;
use crate::source::Complex32;

use super::agc::CwAgc;
use super::apf::AudioPeakFilter;
use super::autonotch::AutoNotch;
use super::decimator::Decimator;
use super::detector::ProductDetector;
use super::fir::{design_lowpass_with, FirFilter, LowpassDesign, WindowKind};
use super::iir_channel::IirChannelFilter;
use super::nco::ComplexNco;
use super::noiseblanker::NoiseBlanker;
use super::noisereduction::NoiseReduction;
use super::notch::IqNotch;
use super::filter_plan::{DEFAULT_CHANNEL_PASSBAND_HZ, DEFAULT_KAISER_BETA};
use super::settings::{
    ChannelFilterKind, CwChannelSettings, DecimFilterKind, DEFAULT_CHANNEL_WINDOW, MAX_NOTCHES,
};

/// Per-call CPU breakdown for [`CwChannel::process_profiled`] / engine-bench.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CwStageMetrics {
    pub iq_samples: usize,
    pub audio_samples: usize,
    pub noise_blanker_ns: u64,
    pub nco_ns: u64,
    pub decim_ns: u64,
    /// Sum of post-decimation stages (notches → polish).
    pub audio_chain_ns: u64,
    pub notches_ns: u64,
    pub channel_filter_ns: u64,
    pub agc_ns: u64,
    pub detector_ns: u64,
    pub polish_ns: u64,
}

impl CwStageMetrics {
    pub fn total_ns(&self) -> u64 {
        self.noise_blanker_ns
            .saturating_add(self.nco_ns)
            .saturating_add(self.decim_ns)
            .saturating_add(self.audio_chain_ns)
    }

    pub fn stage_rows(&self) -> [(&'static str, u64); 9] {
        [
            ("noise_blanker", self.noise_blanker_ns),
            ("nco", self.nco_ns),
            ("decim", self.decim_ns),
            ("notches", self.notches_ns),
            ("channel_fir", self.channel_filter_ns),
            ("agc", self.agc_ns),
            ("detector", self.detector_ns),
            ("polish", self.polish_ns),
            ("audio_chain", self.audio_chain_ns),
        ]
    }
}

/// Allocation-free CW receiver channel for one tuned signal.
#[derive(Clone, Debug)]
pub struct CwChannel {
    noise_blanker: NoiseBlanker,
    shift_nco: ComplexNco,
    decimator: Decimator,
    notches: [IqNotch; MAX_NOTCHES],
    channel_fir: FirFilter,
    channel_iir: IirChannelFilter,
    agc: CwAgc,
    detector: ProductDetector,
    apf: AudioPeakFilter,
    auto_notch: AutoNotch,
    noise_reduction: NoiseReduction,
    snr_peak: f32,
    snr_floor: f32,
    /// Pre-software-AGC level for S-meter only (independent AGC loop ballistics).
    rf_meter: f32,
    work_iq: Vec<Complex32>,
    work_mix: Vec<Complex32>,
    work_decim: Vec<Complex32>,
    last_iq_rate: f32,
    last_decimation: u32,
    last_bandwidth: f32,
    last_window: WindowKind,
    last_kaiser_beta: f32,
    last_passband_flatten: bool,
    last_channel_filter: ChannelFilterKind,
    last_decim_filter: DecimFilterKind,
}

impl CwChannel {
    pub fn new(iq_sample_rate: f32) -> Self {
        let decimator = Decimator::for_sample_rate(iq_sample_rate);
        let audio_rate = decimator.output_rate(iq_sample_rate);
        Self {
            noise_blanker: NoiseBlanker::new(),
            shift_nco: ComplexNco::new(),
            decimator,
            notches: std::array::from_fn(|_| IqNotch::new()),
            channel_fir: design_lowpass_with(
                audio_rate,
                DEFAULT_CHANNEL_PASSBAND_HZ,
                LowpassDesign {
                    window: DEFAULT_CHANNEL_WINDOW,
                    ..LowpassDesign::default()
                },
            ),
            channel_iir: IirChannelFilter::new(),
            agc: CwAgc::new(),
            detector: ProductDetector::new(),
            apf: AudioPeakFilter::new(),
            auto_notch: AutoNotch::new(),
            noise_reduction: NoiseReduction::new(),
            snr_peak: 1e-6,
            snr_floor: 1e-6,
            rf_meter: 1e-6,
            work_iq: Vec::new(),
            work_mix: Vec::new(),
            work_decim: Vec::new(),
            last_iq_rate: iq_sample_rate,
            last_decimation: 0,
            last_bandwidth: DEFAULT_CHANNEL_PASSBAND_HZ,
            last_window: DEFAULT_CHANNEL_WINDOW,
            last_kaiser_beta: DEFAULT_KAISER_BETA,
            last_passband_flatten: false,
            last_channel_filter: ChannelFilterKind::LinearFir,
            last_decim_filter: DecimFilterKind::LinearFir,
        }
    }

    pub fn audio_sample_rate(&self, iq_rate: f32) -> f32 {
        self.decimator.output_rate(iq_rate)
    }

    /// Per-channel signal-to-noise estimate in dB (peak envelope vs slow floor).
    pub fn snr_db(&self) -> f32 {
        20.0 * (self.snr_peak / self.snr_floor.max(1e-7)).log10()
    }

    pub fn agc_gain(&self) -> f32 {
        self.agc.gain()
    }

    pub fn agc_envelope(&self) -> f32 {
        self.agc.envelope()
    }

    /// Pre-software-AGC IQ magnitude for the S-meter (never follows AGC gain reduction).
    pub fn iq_rf_level(&self) -> f32 {
        self.rf_meter
    }

    pub fn process(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        settings: &CwChannelSettings,
        origin: ListenOrigin,
        out: &mut Vec<f32>,
    ) {
        self.process_inner(input, iq_rate, settings, origin, out, None);
    }

    /// Like [`Self::process`] but fills per-stage nanosecond timings (for profiling).
    #[doc(hidden)]
    pub fn process_profiled(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        settings: &CwChannelSettings,
        origin: ListenOrigin,
        out: &mut Vec<f32>,
        metrics: &mut CwStageMetrics,
    ) {
        *metrics = CwStageMetrics::default();
        self.process_inner(input, iq_rate, settings, origin, out, Some(metrics));
    }

    fn process_inner(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        settings: &CwChannelSettings,
        origin: ListenOrigin,
        out: &mut Vec<f32>,
        mut metrics: Option<&mut CwStageMetrics>,
    ) {
        out.clear();
        if input.is_empty() || iq_rate <= 0.0 {
            return;
        }

        if let Some(m) = metrics.as_mut() {
            m.iq_samples = input.len();
        }

        self.sync_chain(iq_rate, settings);
        let audio_rate = self.decimator.output_rate(iq_rate);
        let diag = settings.diagnostic;
        let notch_origin = if diag.listen_nco {
            ListenOrigin::at_center()
        } else {
            origin
        };

        let nb_used_scratch = if settings.noise_blanker.enabled {
            let nb = &settings.noise_blanker;
            if let Some(m) = metrics.as_mut() {
                let t = std::time::Instant::now();
                self.noise_blanker.process_block(
                    input,
                    &mut self.work_iq,
                    nb.threshold,
                    nb.width,
                );
                m.noise_blanker_ns = t.elapsed().as_nanos() as u64;
            } else {
                self.noise_blanker.process_block(
                    input,
                    &mut self.work_iq,
                    nb.threshold,
                    nb.width,
                );
            }
            true
        } else {
            false
        };
        let listen_offset = settings.listen_offset_hz.hz();

        if diag.listen_nco {
            if nb_used_scratch {
                let front = std::mem::take(&mut self.work_iq);
                if let Some(m) = metrics.as_mut() {
                    let t = std::time::Instant::now();
                    self.decimator
                        .decimate_block(&front, &mut self.work_decim, diag.decim_fir);
                    m.decim_ns = t.elapsed().as_nanos() as u64;
                } else {
                    self.decimator
                        .decimate_block(&front, &mut self.work_decim, diag.decim_fir);
                }
            } else if let Some(m) = metrics.as_mut() {
                let t = std::time::Instant::now();
                self.decimator
                    .decimate_block(input, &mut self.work_decim, diag.decim_fir);
                m.decim_ns = t.elapsed().as_nanos() as u64;
            } else {
                self.decimator
                    .decimate_block(input, &mut self.work_decim, diag.decim_fir);
            }
        } else {
            let nco_in: &[Complex32] = if nb_used_scratch {
                &self.work_iq
            } else {
                input
            };
            if let Some(m) = metrics.as_mut() {
                let t_nco = std::time::Instant::now();
                self.shift_nco.mix_down_block(
                    nco_in,
                    &mut self.work_mix,
                    listen_offset,
                    iq_rate,
                );
                m.nco_ns = t_nco.elapsed().as_nanos() as u64;
                let mixed = std::mem::take(&mut self.work_mix);
                let t_decim = std::time::Instant::now();
                self.decimator
                    .decimate_block(&mixed, &mut self.work_decim, diag.decim_fir);
                m.decim_ns = t_decim.elapsed().as_nanos() as u64;
            } else {
                self.shift_nco.mix_down_block(
                    nco_in,
                    &mut self.work_mix,
                    listen_offset,
                    iq_rate,
                );
                let mixed = std::mem::take(&mut self.work_mix);
                self.decimator
                    .decimate_block(&mixed, &mut self.work_decim, diag.decim_fir);
            }
        }

        for (notch, spec) in self.notches.iter_mut().zip(settings.notches.iter()) {
            if spec.enabled {
                notch.sync(audio_rate, spec.width_hz);
            }
        }

        let decimated = std::mem::take(&mut self.work_decim);
        out.reserve(decimated.len());
        if let Some(m) = metrics.as_mut() {
            m.audio_samples = decimated.len();
            self.process_audio_chain(
                &decimated,
                audio_rate,
                settings,
                notch_origin,
                diag,
                out,
                Some(m),
            );
        } else {
            self.process_audio_chain(
                &decimated,
                audio_rate,
                settings,
                notch_origin,
                diag,
                out,
                None,
            );
        }
    }

    fn process_audio_chain(
        &mut self,
        decimated: &[Complex32],
        audio_rate: f32,
        settings: &CwChannelSettings,
        notch_origin: ListenOrigin,
        diag: super::settings::DiagnosticBypassSettings,
        out: &mut Vec<f32>,
        mut metrics: Option<&mut CwStageMetrics>,
    ) {
        let channel_filter = settings.effective_channel_filter();

        let t_notch = metrics.as_ref().map(|_| std::time::Instant::now());
        self.work_mix.clear();
        self.work_mix.reserve(decimated.len());
        for &sample in decimated {
            let mut z = sample;
            for (notch, spec) in self.notches.iter_mut().zip(settings.notches.iter()) {
                if spec.enabled {
                    let rel = notch_origin.convert_for_notch(spec.offset_hz);
                    z = notch.process(z, rel, audio_rate);
                }
            }
            self.work_mix.push(z);
        }
        if let (Some(m), Some(t)) = (metrics.as_mut(), t_notch) {
            m.notches_ns = t.elapsed().as_nanos() as u64;
        }

        let t_fir = metrics.as_ref().map(|_| std::time::Instant::now());
        if diag.channel_fir {
            self.work_iq.clear();
            self.work_iq.extend_from_slice(&self.work_mix);
        } else {
            match channel_filter {
                ChannelFilterKind::LinearFir => {
                    self.channel_fir
                        .process_complex_block(&self.work_mix, &mut self.work_iq);
                }
                ChannelFilterKind::Iir2Pole => {
                    self.work_iq.clear();
                    self.work_iq.reserve(self.work_mix.len());
                    for &z in &self.work_mix {
                        self.work_iq.push(self.channel_iir.process_complex(z));
                    }
                }
            }
        }
        for i in 0..self.work_iq.len() {
            let level = self.work_iq[i].norm().max(1e-7);
            self.track_snr(level);
            self.track_rf_meter(level);
        }
        if let (Some(m), Some(t)) = (metrics.as_mut(), t_fir) {
            m.channel_filter_ns = t.elapsed().as_nanos() as u64;
        }

        let t_agc = metrics.as_ref().map(|_| std::time::Instant::now());
        self.work_mix.clear();
        self.work_mix.reserve(self.work_iq.len());
        for &filtered in &self.work_iq {
            let level = filtered.norm().max(1e-7);
            let gain = if settings.agc.enabled {
                self.agc.gain_for(
                    level,
                    audio_rate,
                    settings.agc.target,
                    settings.agc.attack_ms,
                    settings.agc.decay_ms,
                    settings.agc_mode,
                )
            } else {
                self.agc.track_envelope(
                    level,
                    audio_rate,
                    settings.agc.attack_ms,
                    settings.agc.decay_ms,
                    settings.agc_mode,
                );
                settings.agc.manual_gain
            };
            self.work_mix.push(Complex32 {
                re: filtered.re * gain,
                im: filtered.im * gain,
            });
        }
        if let (Some(m), Some(t)) = (metrics.as_mut(), t_agc) {
            m.agc_ns = t.elapsed().as_nanos() as u64;
        }

        let t_det = metrics.as_ref().map(|_| std::time::Instant::now());
        out.reserve(out.len() + self.work_mix.len());
        for &scaled in &self.work_mix {
            out.push(if diag.bfo {
                scaled.re
            } else {
                self.detector.process(scaled, settings.bfo_hz, audio_rate)
            });
        }
        if let (Some(m), Some(t)) = (metrics.as_mut(), t_det) {
            m.detector_ns = t.elapsed().as_nanos() as u64;
        }

        let t_polish = metrics.as_ref().map(|_| std::time::Instant::now());
        let polish_start = out.len().saturating_sub(self.work_iq.len());
        for audio in &mut out[polish_start..] {
            if settings.apf.enabled {
                *audio = self.apf.process(
                    *audio,
                    audio_rate,
                    settings.bfo_hz,
                    settings.apf.width_hz,
                    settings.apf.gain,
                );
            }
            if settings.auto_notch.enabled {
                *audio = self.auto_notch.process(
                    *audio,
                    audio_rate,
                    settings.bfo_hz,
                    settings.auto_notch.guard_hz,
                    settings.auto_notch.rate,
                );
            }
            if settings.noise_reduction.enabled {
                *audio = self
                    .noise_reduction
                    .process(*audio, settings.noise_reduction.level);
            }
        }
        if let (Some(m), Some(t)) = (metrics.as_mut(), t_polish) {
            m.polish_ns = t.elapsed().as_nanos() as u64;
            m.audio_chain_ns = m.notches_ns
                + m.channel_filter_ns
                + m.agc_ns
                + m.detector_ns
                + m.polish_ns;
        }
    }

    fn track_rf_meter(&mut self, level: f32) {
        // Fast attack / moderate decay — classic S-meter ballistics, separate from IF AGC.
        if level > self.rf_meter {
            self.rf_meter += 0.40 * (level - self.rf_meter);
        } else {
            self.rf_meter += 0.12 * (level - self.rf_meter);
        }
    }

    fn track_snr(&mut self, level: f32) {
        // Peak follows the keyed signal (fast up, slow down).
        self.snr_peak = if level > self.snr_peak {
            level
        } else {
            0.9995 * self.snr_peak + 0.0005 * level
        };
        // Floor follows the noise between dits (fast down, slow up).
        self.snr_floor = if level < self.snr_floor {
            0.5 * self.snr_floor + 0.5 * level
        } else {
            0.9999 * self.snr_floor + 0.0001 * level
        };
    }

    fn sync_chain(&mut self, iq_rate: f32, settings: &CwChannelSettings) {
        let factor = if settings.decimation == 0 {
            super::decimator::decimation_factor(iq_rate)
        } else {
            settings.decimation as usize
        };
        if iq_rate != self.last_iq_rate || settings.decimation != self.last_decimation {
            self.decimator =
                Decimator::with_factor(iq_rate, factor, settings.decim_filter);
            self.shift_nco.reset();
            self.detector.reset_state();
            for notch in &mut self.notches {
                notch.reset_state();
            }
            self.last_iq_rate = iq_rate;
            self.last_decimation = settings.decimation;
            self.last_bandwidth = 0.0;
            self.last_decim_filter = settings.decim_filter;
        } else if settings.decim_filter != self.last_decim_filter {
            self.decimator.sync_filter(iq_rate, settings.decim_filter);
            self.last_decim_filter = settings.decim_filter;
        }

        let bandwidth = settings.channel_bandwidth_hz();
        let audio_rate = self.decimator.output_rate(iq_rate);
        let eff_filter = settings.effective_channel_filter();
        if eff_filter != self.last_channel_filter {
            self.channel_iir.reset_state();
            self.last_channel_filter = eff_filter;
        }
        if eff_filter == ChannelFilterKind::Iir2Pole {
            self.channel_iir.sync(audio_rate, bandwidth);
        }
        let design = LowpassDesign {
            window: settings.window,
            kaiser_beta: settings.kaiser_beta,
            passband_flatten: settings.passband_flatten,
        };
        let design_changed = (bandwidth - self.last_bandwidth).abs() > 1.0
            || settings.window != self.last_window
            || settings.passband_flatten != self.last_passband_flatten
            || (settings.window == WindowKind::Kaiser
                && (settings.kaiser_beta - self.last_kaiser_beta).abs() > 0.05);
        if design_changed && eff_filter == ChannelFilterKind::LinearFir {
            self.channel_fir = design_lowpass_with(audio_rate, bandwidth, design);
            self.last_bandwidth = bandwidth;
            self.last_window = settings.window;
            self.last_kaiser_beta = settings.kaiser_beta;
            self.last_passband_flatten = settings.passband_flatten;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::freq_offset::{ChannelOffsetHz, ListenOrigin};
    use super::super::settings::{AgcMode, DiagnosticBypassSettings};
    use std::f32::consts::TAU;

    fn tone_iq(rate: f32, offset_hz: f32, n: usize) -> Vec<Complex32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                let phase = TAU * offset_hz * t;
                Complex32 {
                    re: phase.cos(),
                    im: phase.sin(),
                }
            })
            .collect()
    }

    #[test]
    fn channel_produces_bfo_tone() {
        let rate = 12_000.0;
        let bfo = 650.0;
        let n = rate as usize * 2;
        let iq = tone_iq(rate, 100.0, n);
        let mut channel = CwChannel::new(rate);
        let mut settings = CwChannelSettings {
            listen_offset_hz: ChannelOffsetHz::new(100.0),
            bfo_hz: bfo,
            passband_hz: 200.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let origin = ListenOrigin::from_settings(settings.listen_offset_hz);
        let mut audio = Vec::new();
        channel.process(&iq, rate, &settings, origin, &mut audio);
        assert!(!audio.is_empty());

        let audio_rate = channel.audio_sample_rate(rate);
        let mut power_bfo = 0.0f32;
        for (i, &s) in audio.iter().enumerate().skip(audio.len() / 2) {
            let t = i as f32 / audio_rate;
            power_bfo += s * (TAU * bfo * t).sin();
        }
        assert!(power_bfo.abs() > 0.1);
    }

    fn audio_rms(audio: &[f32], skip: usize) -> f32 {
        let slice = &audio[skip..];
        if slice.is_empty() {
            return 0.0;
        }
        let p = slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32;
        p.sqrt()
    }

    #[test]
    fn manual_notch_uses_absolute_plot_offset_with_rit() {
        let rate = 12_000.0;
        let listen = ChannelOffsetHz::new(100.0);
        let interferer = ChannelOffsetHz::new(400.0);
        let n = rate as usize * 3;
        let iq = tone_iq(rate, interferer.hz(), n);
        let mut channel = CwChannel::new(rate);
        let mut base = CwChannelSettings {
            listen_offset_hz: listen,
            bfo_hz: 650.0,
            passband_hz: 500.0,
            ..CwChannelSettings::default()
        };
        base.agc.enabled = false;
        let origin = ListenOrigin::from_settings(listen);
        let mut without = Vec::new();
        channel.process(&iq, rate, &base, origin, &mut without);

        let mut with_notch = base.clone();
        with_notch.notches[0].enabled = true;
        with_notch.notches[0].offset_hz = interferer;
        with_notch.notches[0].width_hz = 80.0;
        let mut with = Vec::new();
        channel.process(&iq, rate, &with_notch, origin, &mut with);

        let skip = without.len() / 2;
        let rms_without = audio_rms(&without, skip);
        let rms_with = audio_rms(&with, skip);
        assert!(
            rms_with < rms_without * 0.5,
            "notch at plot {} Hz with listen {} Hz: \
             rms with={rms_with} without={rms_without}",
            interferer.hz(),
            listen.hz()
        );
    }

    #[test]
    fn channel_filter_rejects_skirt_interferer() {
        let rate = 12_000.0;
        let n = rate as usize * 3;
        let iq = tone_iq(rate, 120.0, n);
        let mut channel = CwChannel::new(rate);
        let mut narrow = CwChannelSettings {
            bfo_hz: 650.0,
            passband_hz: 200.0,
            window: WindowKind::Blackman,
            ..CwChannelSettings::default()
        };
        narrow.agc.enabled = false;
        let origin = ListenOrigin::at_center();
        let mut filtered = Vec::new();
        channel.process(&iq, rate, &narrow, origin, &mut filtered);
        let mut bypassed = narrow.clone();
        bypassed.diagnostic.channel_fir = true;
        let mut raw = Vec::new();
        channel.process(&iq, rate, &bypassed, origin, &mut raw);
        let rms = |v: &[f32]| {
            let s = &v[v.len() * 2 / 3..];
            (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
        };
        assert!(
            rms(&filtered) < rms(&raw) * 0.12,
            "120 Hz skirt should be crushed vs FIR bypass: filtered={} bypass={}",
            rms(&filtered),
            rms(&raw)
        );
    }

    #[test]
    fn diagnostic_channel_fir_bypass_leaks_adjacent() {
        let rate = 12_000.0;
        let n = rate as usize * 2;
        let mut iq = tone_iq(rate, 0.0, n);
        let interferer = tone_iq(rate, 800.0, n);
        for (a, b) in iq.iter_mut().zip(interferer.iter()) {
            a.re += b.re * 0.8;
            a.im += b.im * 0.8;
        }
        let mut channel = CwChannel::new(rate);
        let mut settings = CwChannelSettings {
            bfo_hz: 650.0,
            passband_hz: 150.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let origin = ListenOrigin::at_center();
        let mut filtered = Vec::new();
        channel.process(&iq, rate, &settings, origin, &mut filtered);
        settings.diagnostic.channel_fir = true;
        let mut bypassed = Vec::new();
        channel.process(&iq, rate, &settings, origin, &mut bypassed);
        let rms = |v: &[f32]| {
            let s = &v[n / 4..];
            (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
        };
        assert!(
            rms(&bypassed) > rms(&filtered) * 1.2,
            "bypass FIR should leak more adjacent energy"
        );
    }

    #[test]
    fn full_filter_chain_produces_audio_and_snr() {
        let rate = 12_000.0;
        let n = rate as usize * 2;
        let iq = tone_iq(rate, 120.0, n);
        let mut channel = CwChannel::new(rate);
        let settings = CwChannelSettings {
            listen_offset_hz: ChannelOffsetHz::new(120.0),
            bfo_hz: 650.0,
            passband_hz: 250.0,
            channel_filter: ChannelFilterKind::LinearFir,
            decim_filter: DecimFilterKind::LinearFir,
            window: WindowKind::Kaiser,
            kaiser_beta: 8.0,
            passband_flatten: true,
            economy_filter: false,
            full_demod: true,
            decimation: 2,
            noise_blanker: super::super::settings::NoiseBlankerSettings {
                enabled: true,
                threshold: 8.0,
                width: 4,
            },
            notches: [{
                let mut n = super::super::settings::NotchSpec::default();
                n.enabled = true;
                n.offset_hz = ChannelOffsetHz::new(300.0);
                n.width_hz = 60.0;
                n
            }; MAX_NOTCHES],
            auto_notch: super::super::settings::AutoNotchSettings {
                enabled: true,
                ..Default::default()
            },
            apf: super::super::settings::ApfSettings {
                enabled: true,
                ..Default::default()
            },
            noise_reduction: super::super::settings::NoiseReductionSettings {
                enabled: true,
                level: 0.4,
            },
            agc: super::super::settings::AgcSettings {
                enabled: true,
                ..Default::default()
            },
            agc_mode: AgcMode::Envelope,
            diagnostic: DiagnosticBypassSettings::default(),
        };
        let mut audio = Vec::new();
        let origin = ListenOrigin::from_settings(settings.listen_offset_hz);
        channel.process(&iq, rate, &settings, origin, &mut audio);
        assert!(!audio.is_empty());
        assert!(channel.snr_db().is_finite());
    }

    fn scaled_tone_iq(rate: f32, offset_hz: f32, n: usize, amp: f32) -> Vec<Complex32> {
        tone_iq(rate, offset_hz, n)
            .into_iter()
            .map(|s| Complex32 {
                re: s.re * amp,
                im: s.im * amp,
            })
            .collect()
    }

    #[test]
    fn rf_meter_tracks_input_level_not_software_agc() {
        let rate = 12_000.0;
        let n = rate as usize * 3;
        let listen = ChannelOffsetHz::new(200.0);
        let mut settings = CwChannelSettings {
            listen_offset_hz: listen,
            bfo_hz: 650.0,
            passband_hz: 400.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = true;
        settings.agc.target = 0.25;
        let origin = ListenOrigin::from_settings(listen);

        let mut quiet = CwChannel::new(rate);
        let iq_quiet = scaled_tone_iq(rate, listen.hz(), n, 0.05);
        let mut audio_quiet = Vec::new();
        quiet.process(&iq_quiet, rate, &settings, origin, &mut audio_quiet);
        let rf_quiet = quiet.iq_rf_level();
        let gain_quiet = quiet.agc_gain();

        let mut loud = CwChannel::new(rate);
        let iq_loud = scaled_tone_iq(rate, listen.hz(), n, 0.5);
        let mut audio_loud = Vec::new();
        loud.process(&iq_loud, rate, &settings, origin, &mut audio_loud);
        let rf_loud = loud.iq_rf_level();
        let gain_loud = loud.agc_gain();

        assert!(
            rf_loud > rf_quiet * 3.0,
            "S-meter tap should follow input: quiet={rf_quiet} loud={rf_loud}"
        );
        assert!(
            gain_quiet > gain_loud,
            "IQ AGC should ride down on hot input: quiet_gain={gain_quiet} loud_gain={gain_loud}"
        );
        let rms_quiet = audio_rms(&audio_quiet, audio_quiet.len() / 2);
        let rms_loud = audio_rms(&audio_loud, audio_loud.len() / 2);
        assert!(
            rms_loud < rms_quiet * 2.5,
            "AF should stay roughly leveled despite hotter RF: quiet={rms_quiet} loud={rms_loud}"
        );
    }
}
