//! Impulse noise blanker (QRN) — operates on wideband IQ before channelization.
//!
//! Impulse energy (ignition, power-line, lightning) is wideband, so blanking it
//! before the narrow channel filter is far more effective than afterwards. A
//! slow magnitude average sets the reference; samples that spike above
//! `threshold ×` that average are soft-limited (not hard-muted) so CW does not
//! stutter or click. Ballistics are time constants, so behavior is identical at
//! 12 kHz Kiwi rates and 768 kHz wideband rates.

use crate::source::Complex32;

use super::smoothing::alpha_for_tau;

/// Reference-average time constant (seconds).
const AVG_TAU_S: f32 = 0.167;
/// Gain recovery during the post-impulse hold window (seconds).
const HOLD_RECOVERY_S: f32 = 0.0004;
/// Gain recovery once the hold window has passed (seconds).
const IDLE_RECOVERY_S: f32 = 0.0014;

/// Toggleable per-source impulse blanker.
#[derive(Clone, Debug)]
pub struct NoiseBlanker {
    avg_mag: f32,
    hold: usize,
    gain: f32,
    alpha_rate: f32,
    avg_alpha: f32,
    hold_alpha: f32,
    idle_alpha: f32,
}

impl Default for NoiseBlanker {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseBlanker {
    pub fn new() -> Self {
        Self {
            avg_mag: 0.0,
            hold: 0,
            gain: 1.0,
            alpha_rate: 0.0,
            avg_alpha: 0.0005,
            hold_alpha: 0.22,
            idle_alpha: 0.06,
        }
    }

    pub fn reset_state(&mut self) {
        self.avg_mag = 0.0;
        self.hold = 0;
        self.gain = 1.0;
    }

    fn sync_alphas(&mut self, sample_rate: f32) {
        if (sample_rate - self.alpha_rate).abs() <= 1.0 {
            return;
        }
        self.avg_alpha = alpha_for_tau(sample_rate, AVG_TAU_S);
        self.hold_alpha = alpha_for_tau(sample_rate, HOLD_RECOVERY_S);
        self.idle_alpha = alpha_for_tau(sample_rate, IDLE_RECOVERY_S);
        self.alpha_rate = sample_rate;
    }

    /// Blank a block into `output` (state carries across calls).
    pub fn process_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        sample_rate: f32,
        threshold: f32,
        width: usize,
    ) {
        self.sync_alphas(sample_rate);
        output.clear();
        output.reserve(input.len());
        for &sample in input {
            output.push(self.process(sample, threshold, width));
        }
    }

    /// Attenuate `sample` if it (or a recent impulse) exceeds `threshold ×` the
    /// running average magnitude. `width` is the recovery tail in samples.
    pub fn process(&mut self, sample: Complex32, threshold: f32, width: usize) -> Complex32 {
        let mag = sample.norm();
        if self.avg_mag < 1e-9 {
            self.avg_mag = mag.max(1e-9);
            return sample;
        }
        self.avg_mag += self.avg_alpha * (mag - self.avg_mag);
        let limit = self.avg_mag * threshold.max(1.5);
        if mag > limit {
            self.hold = width.max(1);
            let soft = (limit / mag).clamp(0.06, 1.0);
            self.gain = self.gain.min(soft);
        } else if self.hold > 0 {
            self.hold -= 1;
            self.gain += (1.0 - self.gain) * self.hold_alpha;
        } else {
            self.gain += (1.0 - self.gain) * self.idle_alpha;
        }
        Complex32 {
            re: sample.re * self.gain,
            im: sample.im * self.gain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primed(rate: f32) -> NoiseBlanker {
        let mut nb = NoiseBlanker::new();
        nb.sync_alphas(rate);
        nb
    }

    #[test]
    fn soft_limits_impulse_keeps_steady() {
        let mut nb = primed(12_000.0);
        for _ in 0..2000 {
            nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        }
        let spike = nb.process(Complex32 { re: 50.0, im: 0.0 }, 4.0, 4);
        assert!(spike.re < 5.0, "spike should be squashed, got {}", spike.re);
        assert!(spike.re > 0.05, "hard mute causes keying clicks");
        for _ in 0..40 {
            nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        }
        let recovered = nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        assert!(recovered.re > 0.85);
    }

    #[test]
    fn reset_state_clears_hold() {
        let mut nb = primed(12_000.0);
        for _ in 0..100 {
            nb.process(Complex32 { re: 50.0, im: 0.0 }, 4.0, 8);
        }
        nb.reset_state();
        for _ in 0..50 {
            nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        }
        let steady = nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        assert!(steady.re > 0.85);
    }

    #[test]
    fn threshold_floor_prevents_divide_by_zero() {
        let mut nb = primed(12_000.0);
        let out = nb.process(Complex32 { re: 2.0, im: 0.0 }, 0.5, 4);
        assert!(out.re.is_finite());
    }

    #[test]
    fn reference_average_is_rate_invariant() {
        // A 10 ms burst of raised level moves the reference average by the same
        // fraction at 12 kHz and 384 kHz.
        let mut refs = Vec::new();
        for rate in [12_000.0f32, 384_000.0] {
            let mut nb = NoiseBlanker::new();
            let mut out = Vec::new();
            let steady: Vec<Complex32> =
                vec![Complex32 { re: 1.0, im: 0.0 }; (rate * 0.05) as usize];
            nb.process_block(&steady, &mut out, rate, 4.0, 4);
            let raised: Vec<Complex32> =
                vec![Complex32 { re: 2.0, im: 0.0 }; (rate * 0.01) as usize];
            nb.process_block(&raised, &mut out, rate, 4.0, 4);
            refs.push(nb.avg_mag);
        }
        assert!(
            (refs[0] - refs[1]).abs() < 0.05,
            "avg reference should match across rates: {refs:?}"
        );
    }
}
