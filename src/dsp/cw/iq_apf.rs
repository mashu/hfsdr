//! IQ-domain resonant peak — coherent boost at the channel center before demod.
//!
//! A leaky complex integrator forms a narrow bandpass at DC (where the keyed
//! carrier sits after the listen NCO). Adding a scaled copy pulls weak carriers
//! forward without the phase smear of post-demod audio APF.

use crate::source::Complex32;

/// Complex resonant peak at DC on filtered baseband.
#[derive(Clone, Debug)]
pub struct IqPeakFilter {
    state: Complex32,
    last_rate: f32,
    last_width: f32,
}

impl Default for IqPeakFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl IqPeakFilter {
    pub fn new() -> Self {
        Self {
            state: Complex32::new(0.0, 0.0),
            last_rate: 0.0,
            last_width: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.state = Complex32::new(0.0, 0.0);
    }

    fn beta(sample_rate: f32, width_hz: f32) -> f32 {
        let w = width_hz.clamp(5.0, sample_rate * 0.45);
        (std::f32::consts::TAU * w / sample_rate).clamp(0.0001, 0.5)
    }

    /// `gain` scales the resonant component added to the dry IQ sample.
    pub fn process(
        &mut self,
        sample: Complex32,
        sample_rate: f32,
        width_hz: f32,
        gain: f32,
    ) -> Complex32 {
        if sample_rate <= 0.0 {
            return sample;
        }
        let beta = Self::beta(sample_rate, width_hz);
        if sample_rate != self.last_rate || width_hz != self.last_width {
            self.reset_state();
            self.last_rate = sample_rate;
            self.last_width = width_hz;
        }
        self.state = Complex32::new(
            self.state.re * (1.0 - beta) + sample.re * beta,
            self.state.im * (1.0 - beta) + sample.im * beta,
        );
        let g = gain.max(0.0);
        Complex32::new(
            sample.re + g * self.state.re,
            sample.im + g * self.state.im,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn boosts_dc_carrier() {
        let mut apf = IqPeakFilter::new();
        let rate = 12_000.0;
        let mut boosted = 0.0f32;
        let mut dry = 0.0f32;
        for _ in 0..rate as usize {
            let s = Complex32::new(1.0, 0.0);
            let o = apf.process(s, rate, 80.0, 2.0);
            boosted = o.norm();
            dry = s.norm();
        }
        assert!(boosted > dry * 1.5);
    }

    #[test]
    fn reset_clears_state() {
        let mut apf = IqPeakFilter::new();
        let _ = apf.process(Complex32::new(1.0, 0.0), 12_000.0, 80.0, 1.0);
        apf.reset_state();
        let o = apf.process(Complex32::new(0.0, 0.0), 12_000.0, 80.0, 1.0);
        assert!(o.norm() < 1e-3);
    }

    #[test]
    fn passes_tone_near_dc() {
        let mut apf = IqPeakFilter::new();
        let rate = 12_000.0;
        let mut peak = 0.0f32;
        for i in 0..rate as usize * 2 {
            let t = i as f32 / rate;
            let phase = TAU * 5.0 * t;
            let s = Complex32::new(phase.cos() * 0.1, phase.sin() * 0.1);
            peak = peak.max(apf.process(s, rate, 40.0, 1.5).norm());
        }
        assert!(peak > 0.12);
    }
}
