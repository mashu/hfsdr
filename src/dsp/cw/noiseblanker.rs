//! Impulse noise blanker (QRN) — operates on wideband IQ before channelization.
//!
//! Impulse energy (ignition, power-line, lightning) is wideband, so blanking it
//! before the narrow channel filter is far more effective than afterwards. A
//! slow magnitude average sets the reference; samples that spike above
//! `threshold ×` that average are soft-limited (not hard-muted) so CW does not
//! stutter or click.

use crate::source::Complex32;

/// Toggleable per-source impulse blanker.
#[derive(Clone, Debug)]
pub struct NoiseBlanker {
    avg_mag: f32,
    hold: usize,
    gain: f32,
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
        }
    }

    pub fn reset_state(&mut self) {
        self.avg_mag = 0.0;
        self.hold = 0;
        self.gain = 1.0;
    }

    /// Blank a block into `output` (state carries across calls).
    pub fn process_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        threshold: f32,
        width: usize,
    ) {
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
        self.avg_mag = 0.9995 * self.avg_mag + 0.0005 * mag;
        let limit = self.avg_mag * threshold.max(1.5);
        if mag > limit {
            self.hold = width.max(1);
            let soft = (limit / mag).clamp(0.06, 1.0);
            self.gain = self.gain.min(soft);
        } else if self.hold > 0 {
            self.hold -= 1;
            self.gain += (1.0 - self.gain) * 0.22;
        } else {
            self.gain += (1.0 - self.gain) * 0.06;
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

    #[test]
    fn soft_limits_impulse_keeps_steady() {
        let mut nb = NoiseBlanker::new();
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
        let mut nb = NoiseBlanker::new();
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
        let mut nb = NoiseBlanker::new();
        let out = nb.process(Complex32 { re: 2.0, im: 0.0 }, 0.5, 4);
        assert!(out.re.is_finite());
    }
}
