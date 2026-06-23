//! Pre-FFT mix-down and decimation for zoomed panadapter views.

use crate::source::Complex32;

use super::preprocess::IqShiftDecim;

/// Shift the view center to DC and decimate before FFT when heavily zoomed.
#[derive(Clone, Debug)]
pub struct SpectrumFrontEnd {
    ingress: IqShiftDecim,
    iq_rate: f32,
    shift_hz: f32,
    decim: usize,
}

impl SpectrumFrontEnd {
    pub fn new(iq_rate: f32, decim: usize, shift_hz: f32) -> Self {
        let decim = decim.max(1);
        Self {
            ingress: IqShiftDecim::new(iq_rate, decim, iq_rate > 96_000.0),
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
            self.ingress = IqShiftDecim::new(iq_rate, decim, iq_rate > 96_000.0);
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
        let slice = self.ingress.process(input, self.shift_hz, self.iq_rate);
        output.extend_from_slice(slice);
    }
}
