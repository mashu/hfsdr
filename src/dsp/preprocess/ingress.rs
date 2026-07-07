//! Shift wideband IQ to a listen offset and decimate to audio/baseband rate.

use crate::source::Complex32;

use super::super::cw::DecimFilterKind;
use super::fir_decim::FirDecimator;
use super::mixer::IqRotator;

/// Combined mix-down + anti-alias decimation (preserves f32 IQ dynamic range end-to-end).
#[derive(Clone, Debug)]
pub struct IqShiftDecim {
    mixer: IqRotator,
    decim: FirDecimator,
    mixed: Vec<Complex32>,
    output: Vec<Complex32>,
    iq_rate_hz: f32,
    decim_factor: usize,
    wideband: bool,
    filter_kind: DecimFilterKind,
}

impl IqShiftDecim {
    pub fn new(
        iq_rate_hz: f32,
        decim_factor: usize,
        wideband: bool,
        filter_kind: DecimFilterKind,
    ) -> Self {
        let factor = decim_factor.max(1);
        Self {
            mixer: IqRotator::default(),
            decim: FirDecimator::with_factor(iq_rate_hz, factor, wideband, filter_kind),
            mixed: Vec::new(),
            output: Vec::new(),
            iq_rate_hz,
            decim_factor: factor,
            wideband,
            filter_kind,
        }
    }

    pub fn sync(&mut self, iq_rate_hz: f32, decim_factor: usize, filter_kind: DecimFilterKind) {
        let factor = decim_factor.max(1);
        if factor != self.decim_factor
            || (iq_rate_hz - self.iq_rate_hz).abs() > 1.0
            || filter_kind != self.filter_kind
        {
            self.decim = FirDecimator::with_factor(iq_rate_hz, factor, self.wideband, filter_kind);
            self.mixer.reset();
            self.decim_factor = factor;
            self.iq_rate_hz = iq_rate_hz;
            self.filter_kind = filter_kind;
        } else {
            self.decim.sync_filter(iq_rate_hz, filter_kind);
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
        bypass_decim_fir: bool,
    ) -> &[Complex32] {
        self.sync(iq_rate_hz, self.decim_factor, self.filter_kind);
        self.output.clear();
        if input.is_empty() || iq_rate_hz <= 0.0 {
            return &self.output;
        }
        self.mixed.clear();
        self.mixer
            .mix_block(input, &mut self.mixed, shift_hz, iq_rate_hz);
        self.output
            .reserve(self.mixed.len() / self.decim_factor.max(1));
        self.decim
            .decimate_block(&self.mixed, &mut self.output, bypass_decim_fir);
        &self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::cw::DecimFilterKind;
    use std::f32::consts::TAU;

    #[test]
    fn process_decimates_mixed_block() {
        let mut ingress = IqShiftDecim::new(48_000.0, 4, true, DecimFilterKind::LinearFir);
        let input: Vec<Complex32> = (0..400)
            .map(|i| {
                let p = TAU * 500.0 * i as f32 / 48_000.0;
                Complex32::new(p.cos(), p.sin())
            })
            .collect();
        let out = ingress.process(&input, 500.0, 48_000.0, false);
        assert!(!out.is_empty());
        assert!(out.len() < input.len());
    }

    #[test]
    fn empty_input_returns_empty_slice() {
        let mut ingress = IqShiftDecim::new(12_000.0, 2, false, DecimFilterKind::LinearFir);
        let out = ingress.process(&[], 0.0, 12_000.0, false);
        assert!(out.is_empty());
    }

    #[test]
    fn sync_updates_output_rate() {
        let mut ingress = IqShiftDecim::new(12_000.0, 2, false, DecimFilterKind::LinearFir);
        ingress.sync(48_000.0, 4, DecimFilterKind::LinearFir);
        assert_eq!(ingress.output_rate_hz(), 12_000.0);
        ingress.reset();
    }
}
