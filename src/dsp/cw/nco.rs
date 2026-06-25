//! Complex numerically controlled oscillator (software LO / BFO).

use crate::source::Complex32;

use crate::dsp::preprocess::IqRotator;

/// Phase accumulator driving complex mixing (rotation recurrence — no per-sample trig).
#[derive(Clone, Debug)]
pub struct ComplexNco {
    down: IqRotator,
    up: IqRotator,
}

impl Default for ComplexNco {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexNco {
    pub fn new() -> Self {
        Self {
            down: IqRotator::default(),
            up: IqRotator::new(true),
        }
    }

    pub fn reset(&mut self) {
        self.down.reset();
        self.up.reset();
    }

    /// Multiply by `exp(-j·2π·f·t)` — shifts `+freq_hz` down to DC.
    pub fn mix_down(&mut self, sample: Complex32, freq_hz: f32, sample_rate: f32) -> Complex32 {
        self.down.mix_one(sample, freq_hz, sample_rate)
    }

    /// Multiply by `exp(+j·2π·f·t)` — shifts DC up to `+freq_hz`.
    pub fn mix_up(&mut self, sample: Complex32, freq_hz: f32, sample_rate: f32) -> Complex32 {
        self.up.mix_one(sample, freq_hz, sample_rate)
    }

    /// Block mix-down into `output` (efficient for wideband preprocess).
    pub fn mix_down_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        freq_hz: f32,
        sample_rate: f32,
    ) {
        self.down.mix_block(input, output, freq_hz, sample_rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn mix_down_shifts_tone_to_dc() {
        let rate = 12_000.0;
        let mut nco = ComplexNco::new();
        let mut peak = 0.0f32;
        for n in 0..rate as usize {
            let t = n as f32 / rate;
            let phase = TAU * 500.0 * t;
            let sample = Complex32 {
                re: phase.cos(),
                im: phase.sin(),
            };
            let base = nco.mix_down(sample, 500.0, rate);
            peak = peak.max(base.re.abs() + base.im.abs());
        }
        assert!(peak > 0.9);
    }
}
