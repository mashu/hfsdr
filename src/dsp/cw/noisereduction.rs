//! Light LMS line-enhancer noise reduction.
//!
//! Same LMS core as the auto-notch, but here we keep the *prediction* (the
//! correlated, tonal part) and blend it back over the noisy input. For CW the
//! matched channel filter is the primary noise reducer, so this stays gentle —
//! heavy settings add the familiar "underwater" artefacts and smear keying.

use super::lms::LmsPredictor;

/// Adaptive line enhancer that lifts the tone out of broadband hiss.
#[derive(Clone, Debug)]
pub struct NoiseReduction {
    lms: LmsPredictor,
}

impl Default for NoiseReduction {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseReduction {
    pub fn new() -> Self {
        let mut lms = LmsPredictor::new(64, 1);
        lms.set_rate(0.02);
        Self { lms }
    }

    pub fn reset_state(&mut self) {
        self.lms.reset_state();
    }

    /// `level` in 0..1 blends the enhanced (tonal) estimate over the input.
    pub fn process(&mut self, sample: f32, level: f32) -> f32 {
        let level = level.clamp(0.0, 1.0);
        let step = self.lms.step(sample, 1.0);
        (1.0 - level) * sample + level * step.prediction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends_tonal_estimate() {
        let mut nr = NoiseReduction::new();
        let rate = 8_000.0;
        let tone_hz = 700.0;
        let mut out = 0.0f32;
        for i in 0..rate as usize * 2 {
            let t = i as f32 / rate;
            let s = (std::f32::consts::TAU * tone_hz * t).sin();
            out = nr.process(s, 0.5);
        }
        assert!(out.abs() > 0.1);
    }

    #[test]
    fn zero_level_passes_through() {
        let mut nr = NoiseReduction::new();
        assert_eq!(nr.process(0.42, 0.0), 0.42);
    }

    #[test]
    fn reset_state_does_not_panic() {
        let mut nr = NoiseReduction::new();
        let _ = nr.process(0.5, 0.8);
        nr.reset_state();
        let _ = nr.process(0.25, 0.5);
    }
}
