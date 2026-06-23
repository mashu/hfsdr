//! Anti-alias FIR decimation for wideband IQ (f32 taps — full float headroom).

use crate::source::Complex32;

use super::super::cw::{design_gaussian_lowpass, design_gaussian_lowpass_compact, FirFilter};

/// Integer decimation with a Gaussian FIR lowpass (compact taps on wideband rates).
#[derive(Clone, Debug)]
pub struct FirDecimator {
    factor: usize,
    fir: FirFilter,
    counter: usize,
}

impl FirDecimator {
    pub fn with_factor(iq_rate_hz: f32, factor: usize, wideband: bool) -> Self {
        let factor = factor.clamp(1, 256);
        let cutoff = (iq_rate_hz / factor as f32 * 0.45).max(100.0);
        let fir = if wideband {
            design_gaussian_lowpass_compact(iq_rate_hz, cutoff * 2.0)
        } else {
            design_gaussian_lowpass(iq_rate_hz, cutoff * 2.0)
        };
        Self {
            factor,
            fir,
            counter: 0,
        }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    pub fn output_rate(&self, input_rate_hz: f32) -> f32 {
        input_rate_hz / self.factor as f32
    }

    pub fn reset_state(&mut self) {
        self.fir.reset_state();
        self.counter = 0;
    }

    pub fn push(&mut self, sample: Complex32) -> Option<Complex32> {
        if self.factor == 1 {
            return Some(sample);
        }
        let filtered = self.fir.process_complex(sample);
        self.counter += 1;
        if self.counter.is_multiple_of(self.factor) {
            Some(filtered)
        } else {
            None
        }
    }

    /// Decimate a block into `output` (state carries across calls).
    pub fn decimate_block(&mut self, input: &[Complex32], output: &mut Vec<Complex32>) {
        output.clear();
        if self.factor == 1 {
            output.extend_from_slice(input);
            return;
        }
        output.reserve(input.len() / self.factor.max(1));
        for &sample in input {
            if let Some(z) = self.push(sample) {
                output.push(z);
            }
        }
    }
}
