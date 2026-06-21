//! Baseband audio from IQ — CW demod with BFO, bandpass, notch, squelch.

use std::f32::consts::TAU;

use crate::source::Complex32;

use super::biquad::{BandpassChain, Biquad};

/// CW listen chain parameters.
#[derive(Clone, Debug)]
pub struct DemodSettings {
    pub listen_offset_hz: f32,
    pub bfo_hz: f32,
    pub passband_hz: f32,
    pub notch_enabled: bool,
    pub notch_offset_hz: f32,
    pub notch_width_hz: f32,
    pub squelch: f32,
    pub software_agc: bool,
}

impl Default for DemodSettings {
    fn default() -> Self {
        Self {
            listen_offset_hz: 0.0,
            bfo_hz: 650.0,
            passband_hz: 300.0,
            notch_enabled: false,
            notch_offset_hz: 0.0,
            notch_width_hz: 50.0,
            squelch: 0.0,
            software_agc: true,
        }
    }
}

/// Mix IQ to audio: BFO shift, sharp bandpass, optional IQ notch, squelch, AGC.
pub struct IqAudioDemod {
    mix_phase: f32,
    notch_phase: f32,
    bpf: BandpassChain,
    notch: Biquad,
    audio_lp: Biquad,
    agc_gain: f32,
    squelch_env: f32,
    amp_env: f32,
    last_sample_rate: f32,
    last_bfo_hz: f32,
    last_passband_hz: f32,
    last_notch_width_hz: f32,
}

impl IqAudioDemod {
    pub fn new() -> Self {
        Self {
            mix_phase: 0.0,
            notch_phase: 0.0,
            bpf: BandpassChain::new(),
            notch: Biquad::new(),
            audio_lp: Biquad::new(),
            agc_gain: 1.0,
            squelch_env: 0.0,
            amp_env: 0.0,
            last_sample_rate: 0.0,
            last_bfo_hz: 0.0,
            last_passband_hz: 0.0,
            last_notch_width_hz: 0.0,
        }
    }

    pub fn process(
        &mut self,
        samples: &[Complex32],
        sample_rate: f32,
        settings: &DemodSettings,
    ) -> Vec<f32> {
        if samples.is_empty() || sample_rate <= 0.0 {
            return Vec::new();
        }

        self.sync_filters(sample_rate, settings);

        let mix_inc = TAU * (settings.bfo_hz - settings.listen_offset_hz) / sample_rate;
        let notch_inc = if settings.notch_enabled {
            TAU * settings.notch_offset_hz / sample_rate
        } else {
            0.0
        };

        // ~3 ms attack, ~45 ms release at 12 kS/s — soft CW keying envelope.
        let attack = (-1.0 / (sample_rate * 0.003)).exp();
        let release = (-1.0 / (sample_rate * 0.045)).exp();

        let mut out = Vec::with_capacity(samples.len());
        for s in samples {
            let mut re = s.re;
            let mut im = s.im;

            if settings.notch_enabled {
                let (sin_n, cos_n) = self.notch_phase.sin_cos();
                self.notch_phase += notch_inc;
                if self.notch_phase >= TAU {
                    self.notch_phase -= TAU;
                }
                let rot_re = re * cos_n + im * sin_n;
                let rot_im = -re * sin_n + im * cos_n;
                let filtered_re = self.notch.process(rot_re);
                let filtered_im = self.notch.process(rot_im);
                re = filtered_re * cos_n - filtered_im * sin_n;
                im = filtered_re * sin_n + filtered_im * cos_n;
            }

            let (sin_m, cos_m) = self.mix_phase.sin_cos();
            self.mix_phase += mix_inc;
            if self.mix_phase >= TAU {
                self.mix_phase -= TAU;
            }
            let mixed = re * cos_m + im * sin_m;
            let filtered = self.bpf.process(mixed);

            let inst = filtered.abs();
            if inst > self.amp_env {
                self.amp_env = attack * self.amp_env + (1.0 - attack) * inst;
            } else {
                self.amp_env = release * self.amp_env + (1.0 - release) * inst;
            }
            let env_gain = if inst > 1e-7 {
                (self.amp_env / inst).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let smoothed = filtered * env_gain;

            self.squelch_env = 0.94 * self.squelch_env + 0.06 * inst;

            let mut sample = self.audio_lp.process(smoothed);
            if settings.squelch > 0.0 && self.squelch_env < settings.squelch {
                sample *= 0.02;
            }

            if settings.software_agc {
                let target = 0.22;
                let level = self.amp_env.max(1e-6);
                let desired = target / level;
                self.agc_gain = 0.9995 * self.agc_gain + 0.0005 * desired;
                self.agc_gain = self.agc_gain.clamp(0.05, 24.0);
                sample *= self.agc_gain;
            }

            out.push(sample);
        }

        out
    }

    fn sync_filters(&mut self, sample_rate: f32, settings: &DemodSettings) {
        if sample_rate != self.last_sample_rate
            || settings.bfo_hz != self.last_bfo_hz
            || settings.passband_hz != self.last_passband_hz
        {
            self.bpf.set(
                sample_rate,
                settings.bfo_hz.clamp(200.0, sample_rate * 0.4),
                settings.passband_hz.clamp(50.0, 2_000.0),
            );
            self.audio_lp.set_lowpass(sample_rate, 2_800.0);
            self.last_sample_rate = sample_rate;
            self.last_bfo_hz = settings.bfo_hz;
            self.last_passband_hz = settings.passband_hz;
        }

        if settings.notch_enabled
            && (settings.notch_width_hz != self.last_notch_width_hz
                || sample_rate != self.last_sample_rate)
        {
            self.notch.set_notch(
                sample_rate,
                20.0,
                settings.notch_width_hz.clamp(10.0, 500.0),
            );
            self.last_notch_width_hz = settings.notch_width_hz;
        }
    }
}

/// Quick listen path without frequency shift (signal must be near DC).
pub fn iq_to_audio(samples: &[Complex32]) -> Vec<f32> {
    let mut demod = IqAudioDemod::new();
    let settings = DemodSettings {
        bfo_hz: 650.0,
        passband_hz: 500.0,
        ..DemodSettings::default()
    };
    demod.process(samples, 12_000.0, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone_iq(sample_rate: f32, offset_hz: f32, n: usize) -> Vec<Complex32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate;
                let phase = TAU * offset_hz * t;
                Complex32 {
                    re: phase.cos(),
                    im: phase.sin(),
                }
            })
            .collect()
    }

    #[test]
    fn cw_demod_produces_bfo_tone() {
        let sample_rate = 12_000.0;
        let bfo = 650.0;
        let n = sample_rate as usize * 2;
        let iq = tone_iq(sample_rate, 0.0, n);
        let mut demod = IqAudioDemod::new();
        let settings = DemodSettings {
            listen_offset_hz: 0.0,
            bfo_hz: bfo,
            passband_hz: 200.0,
            software_agc: false,
            ..DemodSettings::default()
        };
        let audio = demod.process(&iq, sample_rate, &settings);
        assert!(!audio.is_empty());

        let mut power_bfo = 0.0f32;
        let mut power_far = 0.0f32;
        for (i, &s) in audio.iter().enumerate().skip(n / 2) {
            let t = i as f32 / sample_rate;
            power_bfo += s * (TAU * bfo * t).sin();
            power_far += s * (TAU * 2_000.0 * t).sin();
        }
        assert!(power_bfo.abs() > power_far.abs() * 2.0);
    }

    #[test]
    fn narrow_passband_rejects_adjacent_signal() {
        let sample_rate = 12_000.0;
        let n = sample_rate as usize * 2;
        let mut iq = tone_iq(sample_rate, 0.0, n);
        let interferer = tone_iq(sample_rate, 800.0, n);
        for (a, b) in iq.iter_mut().zip(interferer.iter()) {
            a.re += b.re * 0.8;
            a.im += b.im * 0.8;
        }

        let mut demod = IqAudioDemod::new();
        let settings = DemodSettings {
            listen_offset_hz: 0.0,
            bfo_hz: 650.0,
            passband_hz: 150.0,
            software_agc: false,
            ..DemodSettings::default()
        };
        let audio_clean = demod.process(&tone_iq(sample_rate, 0.0, n), sample_rate, &settings);
        let audio_mixed = demod.process(&iq, sample_rate, &settings);

        let rms_clean = (audio_clean.iter().map(|x| x * x).sum::<f32>() / audio_clean.len() as f32).sqrt();
        let rms_mixed = (audio_mixed.iter().map(|x| x * x).sum::<f32>() / audio_mixed.len() as f32).sqrt();
        assert!(rms_mixed < rms_clean * 1.6);
    }
}
