//! CW product detector (BFO → audible pitch).

use crate::source::Complex32;

use super::nco::ComplexNco;

/// Mix baseband to BFO pitch and emit the real (product) component.
#[derive(Clone, Debug)]
pub struct ProductDetector {
    bfo: ComplexNco,
}

impl Default for ProductDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProductDetector {
    pub fn new() -> Self {
        Self {
            bfo: ComplexNco::new(),
        }
    }

    pub fn reset_state(&mut self) {
        self.bfo.reset();
    }

    pub fn process(&mut self, sample: Complex32, bfo_hz: f32, sample_rate: f32) -> f32 {
        let mixed = self.bfo.mix_up(sample, bfo_hz, sample_rate);
        mixed.re
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn emits_audible_tone_at_bfo_pitch() {
        let rate = 12_000.0;
        let bfo = 650.0;
        let mut det = ProductDetector::new();
        let mut peak = 0.0f32;
        for n in 0..rate as usize {
            let t = n as f32 / rate;
            let phase = TAU * bfo * t;
            let sample = Complex32 {
                re: phase.cos(),
                im: phase.sin(),
            };
            let out = det.process(sample, bfo, rate);
            peak = peak.max(out.abs());
        }
        assert!(peak > 0.5, "expected BFO tone, peak={peak}");
    }

    #[test]
    fn reset_restores_initial_state() {
        let mut det = ProductDetector::new();
        let _ = det.process(Complex32::new(1.0, 0.0), 500.0, 12_000.0);
        det.reset_state();
        let out = det.process(Complex32::new(1.0, 0.0), 0.0, 12_000.0);
        assert!((out - 1.0).abs() < 0.01);
    }
}
