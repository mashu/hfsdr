//! Complex numerically controlled oscillator (software LO / BFO).

use std::f32::consts::TAU;

use crate::source::Complex32;

/// Phase accumulator driving sin/cos for complex mixing.
#[derive(Clone, Debug, Default)]
pub struct ComplexNco {
    phase: f32,
}

impl ComplexNco {
    pub fn new() -> Self {
        Self { phase: 0.0 }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Multiply by `exp(-j·2π·f·t)` — shifts `+freq_hz` down to DC.
    pub fn mix_down(&mut self, sample: Complex32, freq_hz: f32, sample_rate: f32) -> Complex32 {
        let (sin, cos) = self.phase.sin_cos();
        self.advance(freq_hz, sample_rate);
        Complex32 {
            re: sample.re * cos + sample.im * sin,
            im: -sample.re * sin + sample.im * cos,
        }
    }

    /// Multiply by `exp(+j·2π·f·t)` — shifts DC up to `+freq_hz`.
    pub fn mix_up(&mut self, sample: Complex32, freq_hz: f32, sample_rate: f32) -> Complex32 {
        let (sin, cos) = self.phase.sin_cos();
        self.advance(freq_hz, sample_rate);
        Complex32 {
            re: sample.re * cos - sample.im * sin,
            im: sample.re * sin + sample.im * cos,
        }
    }

    fn advance(&mut self, freq_hz: f32, sample_rate: f32) {
        if sample_rate <= 0.0 {
            return;
        }
        self.phase += TAU * freq_hz / sample_rate;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
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
