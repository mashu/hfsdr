//! Single CW listen chain, composed of toggleable stages.
//!
//! Fixed order (each optional stage is bypassed when disabled):
//! ```text
//! IQ → [noise blanker] → NCO shift → decimate → [manual notches]
//!    → channel filter → AGC/manual gain → product detector
//!    → [APF] → [auto-notch] → [noise reduction] → squelch → audio
//! ```
//! All stages are preallocated; the per-sample path never allocates.

use crate::source::Complex32;

use super::agc::CwAgc;
use super::apf::AudioPeakFilter;
use super::autonotch::AutoNotch;
use super::decimator::Decimator;
use super::detector::ProductDetector;
use super::fir::{design_lowpass_with, FirFilter, LowpassDesign, WindowKind};
use super::nco::ComplexNco;
use super::noiseblanker::NoiseBlanker;
use super::noisereduction::NoiseReduction;
use super::notch::IqNotch;
use super::settings::{CwChannelSettings, MAX_NOTCHES};

/// Allocation-free CW receiver channel for one tuned signal.
#[derive(Clone, Debug)]
pub struct CwChannel {
    noise_blanker: NoiseBlanker,
    shift_nco: ComplexNco,
    decimator: Decimator,
    notches: [IqNotch; MAX_NOTCHES],
    channel_fir: FirFilter,
    agc: CwAgc,
    detector: ProductDetector,
    apf: AudioPeakFilter,
    auto_notch: AutoNotch,
    noise_reduction: NoiseReduction,
    squelch_env: f32,
    snr_peak: f32,
    snr_floor: f32,
    last_iq_rate: f32,
    last_decimation: u32,
    last_bandwidth: f32,
    last_window: WindowKind,
    last_kaiser_beta: f32,
    last_passband_flatten: bool,
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
                200.0,
                LowpassDesign::default(),
            ),
            agc: CwAgc::new(),
            detector: ProductDetector::new(),
            apf: AudioPeakFilter::new(),
            auto_notch: AutoNotch::new(),
            noise_reduction: NoiseReduction::new(),
            squelch_env: 0.0,
            snr_peak: 1e-6,
            snr_floor: 1e-6,
            last_iq_rate: iq_sample_rate,
            last_decimation: 0,
            last_bandwidth: 200.0,
            last_window: WindowKind::Gaussian,
            last_kaiser_beta: 6.0,
            last_passband_flatten: false,
        }
    }

    pub fn audio_sample_rate(&self, iq_rate: f32) -> f32 {
        self.decimator.output_rate(iq_rate)
    }

    /// Per-channel signal-to-noise estimate in dB (peak envelope vs slow floor).
    pub fn snr_db(&self) -> f32 {
        20.0 * (self.snr_peak / self.snr_floor.max(1e-7)).log10()
    }

    pub fn process(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        settings: &CwChannelSettings,
        out: &mut Vec<f32>,
    ) {
        out.clear();
        if input.is_empty() || iq_rate <= 0.0 {
            return;
        }

        self.sync_chain(iq_rate, settings);
        let audio_rate = self.decimator.output_rate(iq_rate);

        for &sample in input {
            let mut iq = sample;
            if settings.noise_blanker.enabled {
                iq = self.noise_blanker.process(
                    iq,
                    settings.noise_blanker.threshold,
                    settings.noise_blanker.width,
                );
            }

            iq = self
                .shift_nco
                .mix_down(iq, settings.listen_offset_hz, iq_rate);

            let Some(mut z) = self.decimator.push(iq) else {
                continue;
            };

            for (notch, spec) in self.notches.iter_mut().zip(settings.notches.iter()) {
                if spec.enabled {
                    notch.sync(audio_rate, spec.width_hz);
                    z = notch.process(z, spec.offset_hz, audio_rate);
                }
            }

            let filtered = self.channel_fir.process_complex(z);
            let level = filtered.norm().max(1e-7);
            self.track_snr(level);

            let gain = if settings.agc.enabled {
                self.agc.gain_for(
                    level,
                    audio_rate,
                    settings.agc.target,
                    settings.agc.attack_ms,
                    settings.agc.decay_ms,
                )
            } else {
                settings.agc.manual_gain
            };
            let scaled = Complex32 {
                re: filtered.re * gain,
                im: filtered.im * gain,
            };

            let mut audio = self.detector.process(scaled, settings.bfo_hz, audio_rate);

            if settings.apf.enabled {
                audio = self.apf.process(
                    audio,
                    audio_rate,
                    settings.bfo_hz,
                    settings.apf.width_hz,
                    settings.apf.gain,
                );
            }
            if settings.auto_notch.enabled {
                audio = self.auto_notch.process(
                    audio,
                    audio_rate,
                    settings.bfo_hz,
                    settings.auto_notch.guard_hz,
                    settings.auto_notch.rate,
                );
            }
            if settings.noise_reduction.enabled {
                audio = self
                    .noise_reduction
                    .process(audio, settings.noise_reduction.level);
            }

            self.squelch_env = 0.94 * self.squelch_env + 0.06 * level;
            if settings.squelch > 0.0 && self.squelch_env < settings.squelch {
                audio *= 0.02;
            }

            out.push(audio);
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
        if iq_rate != self.last_iq_rate || settings.decimation != self.last_decimation {
            self.decimator = if settings.decimation == 0 {
                Decimator::for_sample_rate(iq_rate)
            } else {
                Decimator::with_factor(iq_rate, settings.decimation as usize)
            };
            self.shift_nco.reset();
            self.detector.reset_state();
            for notch in &mut self.notches {
                notch.reset_state();
            }
            self.last_iq_rate = iq_rate;
            self.last_decimation = settings.decimation;
            self.last_bandwidth = 0.0;
        }

        let bandwidth = settings.channel_bandwidth_hz();
        let audio_rate = self.decimator.output_rate(iq_rate);
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
        if design_changed {
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
            listen_offset_hz: 100.0,
            bfo_hz: bfo,
            passband_hz: 200.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut audio = Vec::new();
        channel.process(&iq, rate, &settings, &mut audio);
        assert!(!audio.is_empty());

        let audio_rate = channel.audio_sample_rate(rate);
        let mut power_bfo = 0.0f32;
        for (i, &s) in audio.iter().enumerate().skip(audio.len() / 2) {
            let t = i as f32 / audio_rate;
            power_bfo += s * (TAU * bfo * t).sin();
        }
        assert!(power_bfo.abs() > 0.1);
    }
}
