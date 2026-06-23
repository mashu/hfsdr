//! Shift wideband IQ to a listen offset and decimate to audio/baseband rate.

use crate::source::Complex32;

use super::fir_decim::FirDecimator;
use super::mixer::IqRotator;

/// Combined mix-down + FIR decimation (preserves f32 IQ dynamic range end-to-end).
#[derive(Clone, Debug)]
pub struct IqShiftDecim {
    mixer: IqRotator,
    decim: FirDecimator,
    output: Vec<Complex32>,
    iq_rate_hz: f32,
    decim_factor: usize,
    wideband: bool,
}

impl IqShiftDecim {
    pub fn new(iq_rate_hz: f32, decim_factor: usize, wideband: bool) -> Self {
        let factor = decim_factor.max(1);
        Self {
            mixer: IqRotator::default(),
            decim: FirDecimator::with_factor(iq_rate_hz, factor, wideband),
            output: Vec::new(),
            iq_rate_hz,
            decim_factor: factor,
            wideband,
        }
    }

    pub fn sync(&mut self, iq_rate_hz: f32, decim_factor: usize) {
        let factor = decim_factor.max(1);
        if factor != self.decim_factor || (iq_rate_hz - self.iq_rate_hz).abs() > 1.0 {
            self.decim = FirDecimator::with_factor(iq_rate_hz, factor, self.wideband);
            self.mixer.reset();
            self.decim_factor = factor;
            self.iq_rate_hz = iq_rate_hz;
        }
    }

    pub fn output_rate_hz(&self) -> f32 {
        self.decim.output_rate(self.iq_rate_hz)
    }

    pub fn reset(&mut self) {
        self.mixer.reset();
        self.decim.reset_state();
    }

    /// Mix to `shift_hz` relative to center, anti-alias decimate, return baseband slice.
    pub fn process(
        &mut self,
        input: &[Complex32],
        shift_hz: f32,
        iq_rate_hz: f32,
    ) -> &[Complex32] {
        self.sync(iq_rate_hz, self.decim_factor);
        self.output.clear();
        if input.is_empty() || iq_rate_hz <= 0.0 {
            return &self.output;
        }
        self.output.reserve(input.len() / self.decim_factor.max(1));
        self.mixer
            .mix_and_decimate(input, shift_hz, iq_rate_hz, &mut self.decim, &mut self.output);
        &self.output
    }
}
