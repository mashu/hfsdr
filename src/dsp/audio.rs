//! Baseband audio from IQ — thin wrapper over the modular CW channel.
//!
//! The UI owns a [`CwChannelSettings`] and mutates it directly; this wrapper
//! just keeps the per-source channel state and re-creates it when the IQ rate
//! changes.

use crate::source::Complex32;

use super::cw::{CwChannel, CwChannelSettings};
use super::freq_offset::ListenOrigin;
use super::wideband_cw::{demod_wideband, WidebandCwIngress, WIDEBAND_IQ_THRESHOLD};

/// Stateful CW demodulator backed by [`CwChannel`].
pub struct IqAudioDemod {
    channel: CwChannel,
    last_audio_rate: f32,
    wideband: Option<WidebandCwIngress>,
}

impl Default for IqAudioDemod {
    fn default() -> Self {
        Self::new()
    }
}

impl IqAudioDemod {
    pub fn new() -> Self {
        Self {
            channel: CwChannel::new(12_000.0),
            last_audio_rate: 12_000.0,
            wideband: None,
        }
    }

    /// Per-channel SNR estimate in dB.
    pub fn snr_db(&self) -> f32 {
        self.channel.snr_db()
    }

    pub fn agc_gain(&self) -> f32 {
        self.channel.agc_gain()
    }

    pub fn agc_envelope(&self) -> f32 {
        self.channel.agc_envelope()
    }

    /// Demodulate `samples` into `out` (reused buffer, no per-call allocation).
    pub fn process(
        &mut self,
        samples: &[Complex32],
        sample_rate: f32,
        settings: &CwChannelSettings,
        out: &mut Vec<f32>,
    ) {
        out.clear();
        if samples.is_empty() || sample_rate <= 0.0 {
            return;
        }

        if sample_rate > WIDEBAND_IQ_THRESHOLD {
            let ingress = self.wideband.get_or_insert_with(|| {
                WidebandCwIngress::new(sample_rate, settings.decimation, settings.decim_filter)
            });
            ingress.sync(sample_rate, settings.decimation, settings.decim_filter);
            let audio_rate = ingress.audio_rate();
            if (audio_rate - self.last_audio_rate).abs() > 1.0 {
                self.channel = CwChannel::new(audio_rate);
                self.last_audio_rate = audio_rate;
            }
            demod_wideband(
                ingress,
                &mut self.channel,
                samples,
                sample_rate,
                settings,
                out,
            );
            return;
        }

        self.wideband = None;
        if (sample_rate - self.last_audio_rate).abs() > 1.0 {
            self.channel = CwChannel::new(sample_rate);
            self.last_audio_rate = sample_rate;
        }
        let origin = ListenOrigin::from_settings(settings.listen_offset_hz);
        self.channel.process(samples, sample_rate, settings, origin, out);
    }
}

/// Quick listen path for a signal already near DC (used by tests/integration).
pub fn iq_to_audio(samples: &[Complex32]) -> Vec<f32> {
    let mut demod = IqAudioDemod::new();
    let mut settings = CwChannelSettings {
        bfo_hz: 650.0,
        passband_hz: 500.0,
        ..CwChannelSettings::default()
    };
    settings.agc.enabled = false;
    let mut out = Vec::new();
    demod.process(samples, 12_000.0, &settings, &mut out);
    out
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
        let mut settings = CwChannelSettings {
            bfo_hz: bfo,
            passband_hz: 200.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut audio = Vec::new();
        demod.process(&iq, sample_rate, &settings, &mut audio);
        assert!(!audio.is_empty());

        let mut power_bfo = 0.0f32;
        let mut power_far = 0.0f32;
        for (i, &s) in audio.iter().enumerate().skip(n / 4) {
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
        let mut settings = CwChannelSettings {
            bfo_hz: 650.0,
            passband_hz: 150.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;

        let mut audio_clean = Vec::new();
        demod.process(&tone_iq(sample_rate, 0.0, n), sample_rate, &settings, &mut audio_clean);
        let mut audio_mixed = Vec::new();
        demod.process(&iq, sample_rate, &settings, &mut audio_mixed);

        let rms_clean =
            (audio_clean.iter().map(|x| x * x).sum::<f32>() / audio_clean.len() as f32).sqrt();
        let rms_mixed =
            (audio_mixed.iter().map(|x| x * x).sum::<f32>() / audio_mixed.len() as f32).sqrt();
        assert!(rms_mixed < rms_clean * 1.4, "adjacent leaked: {rms_clean} vs {rms_mixed}");
    }

    #[test]
    fn wideband_384k_demod_produces_audio() {
        let iq_rate = 384_000.0;
        let iq = tone_iq(iq_rate, 300.0, 8192);
        let mut demod = IqAudioDemod::new();
        let mut settings = CwChannelSettings {
            listen_offset_hz: crate::ChannelOffsetHz::new(300.0),
            bfo_hz: 650.0,
            passband_hz: 500.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut audio = Vec::new();
        demod.process(&iq, iq_rate, &settings, &mut audio);
        assert!(!audio.is_empty());
    }
}
