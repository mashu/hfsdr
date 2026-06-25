//! Spotlight-aware automatic notch (LMS) on the detected audio.
//!
//! A naive auto-notch eats CW because the wanted signal *is* a tone. This notch
//! protects a guard band around the BFO pitch: when wanted-tone energy is high
//! it freezes adaptation, so the LMS weights only ever learn the steady,
//! off-pitch carriers and never lock onto the keyed CW.

use crate::dsp::biquad::Biquad;

use super::lms::LmsPredictor;

/// Adaptive notch that removes steady off-pitch carriers.
#[derive(Clone, Debug)]
pub struct AutoNotch {
    lms: LmsPredictor,
    guard: Biquad,
    guard_env: f32,
    wide_env: f32,
    last_rate: f32,
    last_pitch: f32,
    last_guard_hz: f32,
}

impl Default for AutoNotch {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoNotch {
    pub fn new() -> Self {
        Self {
            lms: LmsPredictor::new(48, 2),
            guard: Biquad::new(),
            guard_env: 0.0,
            wide_env: 1e-6,
            last_rate: 0.0,
            last_pitch: 0.0,
            last_guard_hz: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.lms.reset_state();
        self.guard.reset_state();
        self.guard_env = 0.0;
        self.wide_env = 1e-6;
    }

    fn sync(&mut self, sample_rate: f32, pitch_hz: f32, guard_hz: f32, rate: f32) {
        if sample_rate != self.last_rate
            || pitch_hz != self.last_pitch
            || guard_hz != self.last_guard_hz
        {
            self.guard
                .set_bandpass(sample_rate, pitch_hz.max(50.0), guard_hz.max(40.0));
            self.last_rate = sample_rate;
            self.last_pitch = pitch_hz;
            self.last_guard_hz = guard_hz;
        }
        self.lms.set_rate(rate);
    }

    /// Process one audio sample. Returns the de-toned (notched) sample.
    pub fn process(
        &mut self,
        sample: f32,
        sample_rate: f32,
        pitch_hz: f32,
        guard_hz: f32,
        rate: f32,
    ) -> f32 {
        self.sync(sample_rate, pitch_hz, guard_hz, rate);

        let guard_mag = self.guard.process(sample).abs();
        self.guard_env = 0.98 * self.guard_env + 0.02 * guard_mag;
        self.wide_env = 0.98 * self.wide_env + 0.02 * sample.abs();

        // Freeze adaptation while the wanted tone dominates the guard band.
        let wanted_ratio = self.guard_env / self.wide_env.max(1e-6);
        let adapt = (1.0 - (wanted_ratio - 0.5).clamp(0.0, 1.0) * 2.0).clamp(0.0, 1.0);

        self.lms.step(sample, adapt).error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_steady_carrier() {
        let mut notch = AutoNotch::new();
        let rate = 8_000.0;
        let pitch = 650.0;
        let mut last = 0.0f32;
        for i in 0..rate as usize * 2 {
            let t = i as f32 / rate;
            let off_pitch = (std::f32::consts::TAU * 900.0 * t).sin();
            last = notch.process(off_pitch, rate, pitch, 120.0, 0.02);
        }
        assert!(last.is_finite());
    }

    #[test]
    fn reset_clears_state() {
        let mut notch = AutoNotch::new();
        let _ = notch.process(1.0, 8_000.0, 650.0, 120.0, 0.02);
        notch.reset_state();
        let out = notch.process(0.0, 8_000.0, 650.0, 120.0, 0.02);
        assert!(out.abs() < 1e-3);
    }
}
