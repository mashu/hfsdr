//! Integer decimation with FIR anti-aliasing.

use crate::source::Complex32;

use super::fir::{design_gaussian_lowpass, FirFilter};

const TARGET_AUDIO_RATE: f32 = 12_000.0;

/// Decimate complex IQ by an integer factor using a lowpass FIR.
#[derive(Clone, Debug)]
pub struct Decimator {
    factor: usize,
    fir: FirFilter,
    counter: usize,
}

impl Decimator {
    pub fn for_sample_rate(input_rate: f32) -> Self {
        Self::with_factor(input_rate, decimation_factor(input_rate))
    }

    /// Build a decimator with an explicit integer factor (clamped to 1..=256).
    pub fn with_factor(input_rate: f32, factor: usize) -> Self {
        let factor = factor.clamp(1, 256);
        let cutoff = (input_rate / factor as f32 * 0.45).max(100.0);
        let fir = design_gaussian_lowpass(input_rate, cutoff * 2.0);
        Self {
            factor,
            fir,
            counter: 0,
        }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    pub fn output_rate(&self, input_rate: f32) -> f32 {
        input_rate / self.factor as f32
    }

    pub fn reset_state(&mut self) {
        self.fir.reset_state();
        self.counter = 0;
    }

    /// Push one input sample; returns a decimated output when the factor divides.
    pub fn push(&mut self, sample: Complex32) -> Option<Complex32> {
        let filtered = self.fir.process_complex(sample);
        self.counter += 1;
        if self.counter.is_multiple_of(self.factor) {
            Some(filtered)
        } else {
            None
        }
    }
}

pub fn decimation_factor(input_rate: f32) -> usize {
    if input_rate <= TARGET_AUDIO_RATE {
        return 1;
    }
    let factor = (input_rate / TARGET_AUDIO_RATE).round() as usize;
    factor.clamp(1, 256)
}

/// Effective integer decimation for CW audio (manual override or auto).
pub fn effective_decimation(iq_rate: f32, manual: u32) -> usize {
    if manual == 0 {
        decimation_factor(iq_rate)
    } else {
        manual as usize
    }
    .clamp(1, 256)
}

pub fn audio_sample_rate(iq_rate: f32, manual_decimation: u32) -> f32 {
    iq_rate / effective_decimation(iq_rate, manual_decimation) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiwi_rate_is_unity() {
        assert_eq!(decimation_factor(12_000.0), 1);
    }

    #[test]
    fn airspy_rate_decimates() {
        assert!(decimation_factor(768_000.0) >= 32);
    }
}
