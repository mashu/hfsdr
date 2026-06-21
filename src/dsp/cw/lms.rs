//! Adaptive LMS predictor — shared core for auto-notch and noise reduction.
//!
//! An LMS predictor estimates the current sample from a *delayed* window of past
//! samples. Broadband noise decorrelates across the delay and cannot be
//! predicted, but periodic content (carriers, steady tones) can. Two products
//! fall out of the same filter:
//!
//! - the **prediction** is the tonal part (used by the line enhancer / NR),
//! - the **error** = input − prediction is the de-toned part (used by the notch).
//!
//! Allocation-free after construction.

/// Normalised LMS predictor with a decorrelation delay.
#[derive(Clone, Debug)]
pub struct LmsPredictor {
    weights: Vec<f32>,
    history: Vec<f32>,
    pos: usize,
    delay: usize,
    mu: f32,
    leak: f32,
}

/// One predictor step: the tonal estimate and the residual.
#[derive(Clone, Copy, Debug)]
pub struct LmsStep {
    pub prediction: f32,
    pub error: f32,
}

impl LmsPredictor {
    pub fn new(taps: usize, delay: usize) -> Self {
        let taps = taps.max(1);
        Self {
            weights: vec![0.0; taps],
            history: vec![0.0; taps + delay + 1],
            pos: 0,
            delay,
            mu: 0.01,
            leak: 1.0 - 1e-4,
        }
    }

    pub fn set_rate(&mut self, mu: f32) {
        self.mu = mu.clamp(0.0, 0.5);
    }

    pub fn reset_state(&mut self) {
        self.weights.fill(0.0);
        self.history.fill(0.0);
        self.pos = 0;
    }

    /// Push `input`, returning the tonal prediction and residual error.
    ///
    /// `adapt` scales the update step this sample (0.0 freezes the weights, e.g.
    /// to protect the wanted CW tone when it is keyed on).
    pub fn step(&mut self, input: f32, adapt: f32) -> LmsStep {
        let n = self.history.len();
        self.history[self.pos] = input;

        let taps = self.weights.len();
        let mut prediction = 0.0f32;
        let mut energy = 1e-6f32;
        let mut idx = (self.pos + n - self.delay - 1) % n;
        for &w in &self.weights {
            let x = self.history[idx];
            prediction += w * x;
            energy += x * x;
            idx = if idx == 0 { n - 1 } else { idx - 1 };
        }

        let error = input - prediction;
        if adapt > 0.0 {
            let step = adapt * self.mu * error / energy;
            let mut idx = (self.pos + n - self.delay - 1) % n;
            for w in &mut self.weights {
                *w = self.leak * *w + step * self.history[idx];
                idx = if idx == 0 { n - 1 } else { idx - 1 };
            }
        }

        self.pos = if self.pos + 1 == n { 0 } else { self.pos + 1 };
        let _ = taps;
        LmsStep { prediction, error }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn predictor_locks_onto_steady_tone() {
        let rate = 12_000.0;
        let mut lms = LmsPredictor::new(32, 1);
        lms.set_rate(0.1);
        let mut residual_pow = 0.0f32;
        let mut input_pow = 0.0f32;
        let mut count = 0usize;
        for n in 0..rate as usize {
            let t = n as f32 / rate;
            let x = (TAU * 700.0 * t).sin();
            let step = lms.step(x, 1.0);
            if n > rate as usize / 2 {
                residual_pow += step.error * step.error;
                input_pow += x * x;
                count += 1;
            }
        }
        let _ = count;
        // Steady tone is largely predicted away in the error path.
        assert!(residual_pow < input_pow * 0.2, "residual too high");
    }
}
