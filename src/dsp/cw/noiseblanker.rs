//! Impulse noise blanker (QRN) — operates on wideband IQ before channelization.
//!
//! Impulse energy (ignition, power-line, lightning) is wideband, so blanking it
//! before the narrow channel filter is far more effective than afterwards. A
//! slow magnitude average sets the reference; samples that spike above
//! `threshold ×` that average are blanked for a short hold window.

use crate::source::Complex32;

/// Toggleable per-source impulse blanker.
#[derive(Clone, Debug)]
pub struct NoiseBlanker {
    avg_mag: f32,
    hold: usize,
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
        }
    }

    pub fn reset_state(&mut self) {
        self.avg_mag = 0.0;
        self.hold = 0;
    }

    /// Blank `sample` if it (or a recent impulse) exceeds `threshold ×` the
    /// running average magnitude. `width` is the blank-hold length in samples.
    pub fn process(&mut self, sample: Complex32, threshold: f32, width: usize) -> Complex32 {
        let mag = sample.norm();
        self.avg_mag = 0.9995 * self.avg_mag + 0.0005 * mag;
        let limit = self.avg_mag * threshold.max(1.5);
        if mag > limit {
            self.hold = width.max(1);
        }
        if self.hold > 0 {
            self.hold -= 1;
            Complex32 { re: 0.0, im: 0.0 }
        } else {
            sample
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_impulse_keeps_steady() {
        let mut nb = NoiseBlanker::new();
        for _ in 0..2000 {
            nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        }
        let spike = nb.process(Complex32 { re: 50.0, im: 0.0 }, 4.0, 4);
        assert_eq!(spike.re, 0.0);
        let steady = nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 0);
        // still in hold from the spike, but next steady sample recovers
        let _ = steady;
        for _ in 0..10 {
            nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        }
        let recovered = nb.process(Complex32 { re: 1.0, im: 0.0 }, 4.0, 4);
        assert!(recovered.re > 0.5);
    }
}
