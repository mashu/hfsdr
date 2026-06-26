//! Anti-alias lowpass for integer decimators (FIR Gaussian or 2-pole IIR).

use crate::source::Complex32;

use super::fir::{design_gaussian_lowpass, design_gaussian_lowpass_compact, FirFilter};
use super::iir_channel::IirChannelFilter;
use super::settings::ChannelFilterKind;
use super::settings::IirFilterKind;

pub fn decim_cutoff_hz(input_rate: f32, factor: usize) -> f32 {
    (input_rate / factor.max(1) as f32 * 0.45).max(100.0)
}

/// Lowpass before decimation (FIR or IIR).
#[derive(Clone, Debug)]
pub struct AntiAliasFilter {
    kind: ChannelFilterKind,
    fir: FirFilter,
    iir: IirChannelFilter,
    last_rate: f32,
    last_bw: f32,
}

impl AntiAliasFilter {
    pub fn new(
        kind: ChannelFilterKind,
        input_rate: f32,
        factor: usize,
        compact: bool,
    ) -> Self {
        let cutoff = decim_cutoff_hz(input_rate, factor);
        let bw = cutoff * 2.0;
        let fir = if compact {
            design_gaussian_lowpass_compact(input_rate, bw)
        } else {
            design_gaussian_lowpass(input_rate, bw)
        };
        let mut iir = IirChannelFilter::new();
        iir.sync(input_rate, bw, IirFilterKind::Butterworth);
        Self {
            kind,
            fir,
            iir,
            last_rate: input_rate,
            last_bw: bw,
        }
    }

    pub fn sync(
        &mut self,
        kind: ChannelFilterKind,
        input_rate: f32,
        factor: usize,
        compact: bool,
    ) {
        let cutoff = decim_cutoff_hz(input_rate, factor);
        let bw = cutoff * 2.0;
        let rebuild = kind != self.kind
            || (input_rate - self.last_rate).abs() > 1.0
            || (bw - self.last_bw).abs() > 1.0;
        if rebuild {
            self.kind = kind;
            self.fir = if compact {
                design_gaussian_lowpass_compact(input_rate, bw)
            } else {
                design_gaussian_lowpass(input_rate, bw)
            };
            self.iir.sync(input_rate, bw, IirFilterKind::Butterworth);
            self.last_rate = input_rate;
            self.last_bw = bw;
        }
    }

    pub fn reset_state(&mut self) {
        self.fir.reset_state();
        self.iir.reset_state();
    }

    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        match self.kind {
            ChannelFilterKind::LinearFir => self.fir.process_complex(sample),
            ChannelFilterKind::Iir2Pole => self.iir.process_complex(sample),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn iir_decim_attenuates_high_before_downsample() {
        let rate = 48_000.0;
        let factor = 4;
        let tone = |hz: f32, n: usize| -> Vec<Complex32> {
            (0..n)
                .map(|i| {
                    let t = i as f32 / rate;
                    let p = TAU * hz * t;
                    Complex32::new(p.cos(), p.sin())
                })
                .collect()
        };
        fn power(filt: &mut AntiAliasFilter, samples: &[Complex32]) -> f32 {
            let mut p = 0.0f32;
            for &s in samples {
                let o = filt.process_complex(s);
                p += o.norm_sqr();
            }
            p
        }
        let n = rate as usize / 10;
        let lo = power(
            &mut AntiAliasFilter::new(ChannelFilterKind::Iir2Pole, rate, factor, true),
            &tone(200.0, n),
        );
        let hi = power(
            &mut AntiAliasFilter::new(ChannelFilterKind::Iir2Pole, rate, factor, true),
            &tone(8_000.0, n),
        );
        assert!(lo > hi * 1.5);
    }
}
