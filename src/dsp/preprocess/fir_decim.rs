//! Anti-alias decimation for wideband IQ (f32 taps — full float headroom).

use crate::source::Complex32;

use super::super::cw::{AntiAliasFilter, DecimFilterKind};

/// Integer decimation with selectable anti-alias (Gaussian FIR or 2-pole IIR).
#[derive(Clone, Debug)]
pub struct FirDecimator {
    factor: usize,
    filter: AntiAliasFilter,
    counter: usize,
    wideband: bool,
}

impl FirDecimator {
    pub fn with_factor(
        iq_rate_hz: f32,
        factor: usize,
        wideband: bool,
        filter_kind: DecimFilterKind,
    ) -> Self {
        let factor = factor.clamp(1, 256);
        Self {
            factor,
            filter: AntiAliasFilter::new(filter_kind, iq_rate_hz, factor, wideband),
            counter: 0,
            wideband,
        }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    pub fn output_rate(&self, input_rate_hz: f32) -> f32 {
        input_rate_hz / self.factor as f32
    }

    pub fn sync_filter(
        &mut self,
        iq_rate_hz: f32,
        filter_kind: DecimFilterKind,
    ) {
        self.filter
            .sync(filter_kind, iq_rate_hz, self.factor, self.wideband);
    }

    pub fn reset_state(&mut self) {
        self.filter.reset_state();
        self.counter = 0;
    }

    pub fn push(&mut self, sample: Complex32, bypass_fir: bool) -> Option<Complex32> {
        if self.factor == 1 {
            return Some(sample);
        }
        self.counter += 1;
        let emit = self.counter.is_multiple_of(self.factor);
        if bypass_fir {
            return if emit { Some(sample) } else { None };
        }
        self.filter.push_decimate(sample, emit)
    }

    /// Decimate a block into `output` (state carries across calls).
    pub fn decimate_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        bypass_fir: bool,
    ) {
        output.clear();
        if self.factor == 1 {
            output.extend_from_slice(input);
            return;
        }
        output.reserve(input.len() / self.factor.max(1));
        for &sample in input {
            if let Some(z) = self.push(sample, bypass_fir) {
                output.push(z);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::cw::DecimFilterKind;
    use std::time::{Duration, Instant};

    #[test]
    fn decimate_block_completes_quickly() {
        let mut decim = FirDecimator::with_factor(48_000.0, 2, true, DecimFilterKind::LinearFir);
        let raw: Vec<Complex32> = (0..128)
            .map(|i| Complex32::new((i as f32 * 0.1).cos(), 0.0))
            .collect();
        let mut out = Vec::new();
        let t0 = Instant::now();
        decim.decimate_block(&raw, &mut out, false);
        assert!(t0.elapsed() < Duration::from_secs(1));
        assert!(!out.is_empty());
        assert!(out.len() < raw.len());
    }

    #[test]
    fn unity_factor_passthrough() {
        let mut d = FirDecimator::with_factor(48_000.0, 1, false, DecimFilterKind::LinearFir);
        let s = Complex32::new(1.0, -1.0);
        assert_eq!(d.push(s, false), Some(s));
    }

    #[test]
    fn decimate_block_reduces_length() {
        let mut d = FirDecimator::with_factor(48_000.0, 4, true, DecimFilterKind::LinearFir);
        let input: Vec<Complex32> = (0..64)
            .map(|i| Complex32::new(i as f32, 0.0))
            .collect();
        let mut out = Vec::new();
        d.decimate_block(&input, &mut out, false);
        assert!(!out.is_empty());
        assert!(out.len() <= input.len() / 4 + 1);
    }

    #[test]
    fn output_rate_scales_with_factor() {
        let d = FirDecimator::with_factor(48_000.0, 8, false, DecimFilterKind::LinearFir);
        assert_eq!(d.output_rate(48_000.0), 6_000.0);
        assert_eq!(d.factor(), 8);
    }

    #[test]
    fn reset_state_clears_counter() {
        let mut d = FirDecimator::with_factor(48_000.0, 4, false, DecimFilterKind::LinearFir);
        let _ = d.push(Complex32::new(1.0, 0.0), false);
        d.reset_state();
        let mut out = Vec::new();
        d.decimate_block(&[Complex32::new(1.0, 0.0); 8], &mut out, false);
        assert!(!out.is_empty());
    }
}
