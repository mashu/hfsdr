//! Pre-FFT mix-down and decimation for zoomed panadapter views.

use crate::source::Complex32;

use super::cw::{Decimator, ComplexNco};

/// Shift the view center to DC and decimate before FFT when heavily zoomed.
#[derive(Clone, Debug)]
pub struct SpectrumFrontEnd {
    nco: ComplexNco,
    decimator: Decimator,
    iq_rate: f32,
    shift_hz: f32,
    decim: usize,
}

impl SpectrumFrontEnd {
    pub fn new(iq_rate: f32, decim: usize, shift_hz: f32) -> Self {
        let decim = decim.max(1);
        Self {
            nco: ComplexNco::new(),
            decimator: Decimator::with_factor(iq_rate, decim),
            iq_rate,
            shift_hz,
            decim,
        }
    }

    pub fn sync(&mut self, iq_rate: f32, decim: usize, shift_hz: f32) {
        let decim = decim.max(1);
        if (self.iq_rate - iq_rate).abs() > 1.0
            || self.decim != decim
            || (self.shift_hz - shift_hz).abs() > 0.5
        {
            self.decimator = Decimator::with_factor(iq_rate, decim);
            self.nco.reset();
            self.iq_rate = iq_rate;
            self.shift_hz = shift_hz;
            self.decim = decim;
        } else {
            self.shift_hz = shift_hz;
        }
    }

    /// Produce decimated IQ for the spectrum analyzer (or pass-through when `decim == 1`).
    pub fn process(&mut self, input: &[Complex32], output: &mut Vec<Complex32>) {
        output.clear();
        if self.decim <= 1 {
            output.extend_from_slice(input);
            return;
        }
        for &sample in input {
            let shifted = self.nco.mix_down(sample, self.shift_hz, self.iq_rate);
            if let Some(decimated) = self.decimator.push(shifted) {
                output.push(decimated);
            }
        }
    }
}
