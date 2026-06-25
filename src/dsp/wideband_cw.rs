//! Wideband CW listen path: preprocess ingress + baseband [`CwChannel`].

use super::freq_offset::{ChannelOffsetHz, ListenOrigin};
use crate::source::Complex32;

use super::cw::{effective_decimation, CwChannel, CwChannelSettings, DecimFilterKind};
use super::preprocess::IqShiftDecim;

/// IQ rates above this use the compact ingress + baseband channel path.
pub const WIDEBAND_IQ_THRESHOLD: f32 = 96_000.0;

/// Mix to listen offset and decimate wideband IQ before the full CW chain.
#[derive(Clone, Debug)]
pub struct WidebandCwIngress {
    ingress: IqShiftDecim,
    audio_rate: f32,
    decim_factor: usize,
}

impl WidebandCwIngress {
    pub fn new(iq_rate: f32, manual_decim: u32, decim_filter: DecimFilterKind) -> Self {
        let factor = effective_decimation(iq_rate, manual_decim);
        Self {
            ingress: IqShiftDecim::new(iq_rate, factor, true, decim_filter),
            audio_rate: iq_rate / factor as f32,
            decim_factor: factor,
        }
    }

    pub fn sync(&mut self, iq_rate: f32, manual_decim: u32, decim_filter: DecimFilterKind) {
        let factor = effective_decimation(iq_rate, manual_decim);
        let audio_rate = iq_rate / factor as f32;
        if factor != self.decim_factor || (audio_rate - self.audio_rate).abs() > 1.0 {
            self.ingress = IqShiftDecim::new(iq_rate, factor, true, decim_filter);
            self.decim_factor = factor;
            self.audio_rate = audio_rate;
        } else {
            self.ingress.sync(iq_rate, factor, decim_filter);
        }
    }

    pub fn audio_rate(&self) -> f32 {
        self.audio_rate
    }

    pub fn to_baseband(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        listen_offset_hz: ChannelOffsetHz,
        diagnostic: &super::cw::DiagnosticBypassSettings,
    ) -> &[Complex32] {
        let shift = if diagnostic.listen_nco {
            0.0
        } else {
            listen_offset_hz.hz()
        };
        self.ingress
            .process(input, shift, iq_rate, diagnostic.decim_fir)
    }
}

/// Run the CW chain on wideband IQ via compact ingress decimation.
pub fn demod_wideband(
    ingress: &mut WidebandCwIngress,
    channel: &mut CwChannel,
    input: &[Complex32],
    iq_rate: f32,
    settings: &CwChannelSettings,
    out: &mut Vec<f32>,
) {
    out.clear();
    if input.is_empty() || iq_rate <= WIDEBAND_IQ_THRESHOLD {
        return;
    }
    ingress.sync(iq_rate, settings.decimation, settings.decim_filter);
    let audio_rate = ingress.audio_rate();
    let listen = settings.listen_offset_hz;
    let bb = ingress.to_baseband(input, iq_rate, listen, &settings.diagnostic);
    if bb.is_empty() {
        return;
    }
    let mut base = settings.clone();
    base.listen_offset_hz = ChannelOffsetHz::ZERO;
    base.decimation = 1;
    let origin = ListenOrigin::after_upstream_mix(listen);
    channel.process(bb, audio_rate, &base, origin, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelOffsetHz;
    use crate::dsp::cw::{DiagnosticBypassSettings, WindowKind};
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
    fn wideband_notch_uses_absolute_plot_offset_with_rit() {
        let iq_rate = 384_000.0;
        let listen = ChannelOffsetHz::new(150.0);
        let interferer = ChannelOffsetHz::new(420.0);
        let n = 16_384;
        let iq = tone_iq(iq_rate, interferer.hz(), n);
        let mut ingress = WidebandCwIngress::new(iq_rate, 0, DecimFilterKind::LinearFir);
        let mut channel = CwChannel::new(12_000.0);
        let mut settings = CwChannelSettings {
            listen_offset_hz: listen,
            bfo_hz: 650.0,
            passband_hz: 900.0,
            window: WindowKind::Gaussian,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut without = Vec::new();
        demod_wideband(
            &mut ingress,
            &mut channel,
            &iq,
            iq_rate,
            &settings,
            &mut without,
        );

        settings.notches[0].enabled = true;
        settings.notches[0].offset_hz = interferer;
        settings.notches[0].width_hz = 80.0;
        let mut with = Vec::new();
        demod_wideband(&mut ingress, &mut channel, &iq, iq_rate, &settings, &mut with);

        let skip = without.len() / 2;
        let rms = |v: &[f32]| {
            let s = &v[skip..];
            (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
        };
        assert!(
            rms(&with) < rms(&without) * 0.5,
            "wideband notch: with={} without={}",
            rms(&with),
            rms(&without)
        );
    }

    #[test]
    fn wideband_path_produces_audio() {
        let iq_rate = 384_000.0;
        let n = 8192;
        let iq = tone_iq(iq_rate, 200.0, n);
        let mut ingress = WidebandCwIngress::new(iq_rate, 0, DecimFilterKind::LinearFir);
        let mut channel = CwChannel::new(12_000.0);
        let mut settings = CwChannelSettings {
            listen_offset_hz: ChannelOffsetHz::new(200.0),
            bfo_hz: 650.0,
            passband_hz: 500.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut out = Vec::new();
        demod_wideband(&mut ingress, &mut channel, &iq, iq_rate, &settings, &mut out);
        assert!(!out.is_empty());
    }

    #[test]
    fn demod_skips_narrow_iq_rates() {
        let mut ingress = WidebandCwIngress::new(48_000.0, 0, DecimFilterKind::LinearFir);
        let mut channel = CwChannel::new(12_000.0);
        let settings = CwChannelSettings::default();
        let iq = tone_iq(48_000.0, 100.0, 512);
        let mut out = vec![1.0];
        demod_wideband(&mut ingress, &mut channel, &iq, 48_000.0, &settings, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn diagnostic_bypass_skips_listen_nco_shift() {
        let iq_rate = 384_000.0;
        let iq = tone_iq(iq_rate, 400.0, 2048);
        let listen = ChannelOffsetHz::new(400.0);
        let mut ingress_normal = WidebandCwIngress::new(iq_rate, 0, DecimFilterKind::LinearFir);
        let normal = ingress_normal.to_baseband(
            &iq,
            iq_rate,
            listen,
            &DiagnosticBypassSettings::default(),
        );
        let mut ingress_bypass = WidebandCwIngress::new(iq_rate, 0, DecimFilterKind::LinearFir);
        let bypass = ingress_bypass.to_baseband(
            &iq,
            iq_rate,
            listen,
            &DiagnosticBypassSettings {
                listen_nco: true,
                ..Default::default()
            },
        );
        assert!(!normal.is_empty());
        assert!(!bypass.is_empty());
        let p_normal: f32 = normal.iter().map(|s| s.norm_sqr()).sum();
        let p_bypass: f32 = bypass.iter().map(|s| s.norm_sqr()).sum();
        assert!(p_normal > 0.0 && p_bypass > 0.0);
    }
}
