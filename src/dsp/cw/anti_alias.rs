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

    /// Full-rate anti-alias (used in tests and diagnostics).
    #[allow(dead_code)]
    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        match self.kind {
            ChannelFilterKind::LinearFir => self.fir.process_complex(sample),
            ChannelFilterKind::Iir2Pole => self.iir.process_complex(sample),
        }
    }

    /// Push one IQ sample; return the filtered output only on decimation instants.
    ///
    /// FIR path skips MACs between output samples (polyphase decimation). IIR must run
    /// every sample to preserve state but still only emits on decimation instants.
    pub fn push_decimate(&mut self, sample: Complex32, emit: bool) -> Option<Complex32> {
        match self.kind {
            ChannelFilterKind::LinearFir => self.fir.feed_and_maybe_emit(sample, emit),
            ChannelFilterKind::Iir2Pole => {
                let filtered = self.iir.process_complex(sample);
                if emit { Some(filtered) } else { None }
            }
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

    #[test]
    fn polyphase_decim_matches_full_rate_fir() {
        let rate = 384_000.0;
        let factor = 32;
        let tone = |hz: f32, n: usize| -> Vec<Complex32> {
            (0..n)
                .map(|i| {
                    let t = i as f32 / rate;
                    let p = TAU * hz * t;
                    Complex32::new(p.cos(), p.sin())
                })
                .collect()
        };
        let input = tone(500.0, 4096);

        let mut legacy = AntiAliasFilter::new(ChannelFilterKind::LinearFir, rate, factor, true);
        let mut legacy_out = Vec::new();
        let mut counter = 0usize;
        for &s in &input {
            let filtered = legacy.process_complex(s);
            counter += 1;
            if counter.is_multiple_of(factor) {
                legacy_out.push(filtered);
            }
        }

        let mut poly = AntiAliasFilter::new(ChannelFilterKind::LinearFir, rate, factor, true);
        let mut poly_out = Vec::new();
        counter = 0;
        for &s in &input {
            counter += 1;
            let emit = counter.is_multiple_of(factor);
            if let Some(z) = poly.push_decimate(s, emit) {
                poly_out.push(z);
            }
        }

        assert_eq!(legacy_out.len(), poly_out.len());
        let err: f32 = legacy_out
            .iter()
            .zip(poly_out.iter())
            .map(|(a, b)| (a.re - b.re).abs() + (a.im - b.im).abs())
            .sum();
        assert!(err < 0.5, "polyphase vs legacy mismatch err={err}");
    }
}
