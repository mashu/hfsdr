//! Wideband CW listen path: preprocess ingress + baseband [`CwChannel`].

use crate::source::Complex32;

use super::cw::{effective_decimation, CwChannel, CwChannelSettings};
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
    pub fn new(iq_rate: f32, manual_decim: u32) -> Self {
        let factor = effective_decimation(iq_rate, manual_decim);
        Self {
            ingress: IqShiftDecim::new(iq_rate, factor, true),
            audio_rate: iq_rate / factor as f32,
            decim_factor: factor,
        }
    }

    pub fn sync(&mut self, iq_rate: f32, manual_decim: u32) {
        let factor = effective_decimation(iq_rate, manual_decim);
        let audio_rate = iq_rate / factor as f32;
        if factor != self.decim_factor || (audio_rate - self.audio_rate).abs() > 1.0 {
            self.ingress = IqShiftDecim::new(iq_rate, factor, true);
            self.decim_factor = factor;
            self.audio_rate = audio_rate;
        }
    }

    pub fn audio_rate(&self) -> f32 {
        self.audio_rate
    }

    pub fn to_baseband(
        &mut self,
        input: &[Complex32],
        iq_rate: f32,
        listen_offset_hz: f32,
    ) -> &[Complex32] {
        self.ingress.process(input, listen_offset_hz, iq_rate)
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
    ingress.sync(iq_rate, settings.decimation);
    let audio_rate = ingress.audio_rate();
    let bb = ingress.to_baseband(input, iq_rate, settings.listen_offset_hz);
    if bb.is_empty() {
        return;
    }
    let mut base = settings.clone();
    base.listen_offset_hz = 0.0;
    base.decimation = 1;
    channel.process(bb, audio_rate, &base, out);
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
    fn wideband_path_produces_audio() {
        let iq_rate = 384_000.0;
        let n = 8192;
        let iq = tone_iq(iq_rate, 200.0, n);
        let mut ingress = WidebandCwIngress::new(iq_rate, 0);
        let mut channel = CwChannel::new(12_000.0);
        let mut settings = CwChannelSettings {
            listen_offset_hz: 200.0,
            bfo_hz: 650.0,
            passband_hz: 500.0,
            ..CwChannelSettings::default()
        };
        settings.agc.enabled = false;
        let mut out = Vec::new();
        demod_wideband(&mut ingress, &mut channel, &iq, iq_rate, &settings, &mut out);
        assert!(!out.is_empty());
    }
}
