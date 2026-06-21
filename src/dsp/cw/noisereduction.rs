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
