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

    /// Block mix-up into `output` (efficient counterpart to [`Self::mix_up`]).
    pub fn mix_up_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        freq_hz: f32,
        sample_rate: f32,
    ) {
        self.up.mix_block(input, output, freq_hz, sample_rate);
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

    #[test]
    fn mix_up_shifts_dc_to_tone() {
        let rate = 12_000.0;
        let tone = 400.0;
        let mut nco = ComplexNco::new();
        let dc = Complex32::new(1.0, 0.0);
        let mut peak = 0.0f32;
        for _ in 0..rate as usize {
            let mixed = nco.mix_up(dc, tone, rate);
            peak = peak.max(mixed.norm());
        }
        assert!(peak > 0.9);
    }

    #[test]
    fn mix_down_block_matches_scalar_path() {
        let rate = 12_000.0;
        let shift = 300.0;
        let input: Vec<Complex32> = (0..256)
            .map(|i| {
                let t = i as f32 / rate;
                let p = TAU * shift * t;
                Complex32::new(p.cos(), p.sin())
            })
            .collect();
        let mut block_out = Vec::new();
        let mut scalar_out = Vec::new();
        let mut nco_block = ComplexNco::new();
        let mut nco_scalar = ComplexNco::new();
        nco_block.mix_down_block(&input, &mut block_out, shift, rate);
        for &s in &input {
            scalar_out.push(nco_scalar.mix_down(s, shift, rate));
        }
        assert_eq!(block_out.len(), scalar_out.len());
        let err: f32 = block_out
            .iter()
            .zip(scalar_out.iter())
            .map(|(a, b)| (a.re - b.re).abs() + (a.im - b.im).abs())
            .sum();
        assert!(err < 0.05, "block/scalar mismatch err={err}");
    }

    #[test]
    fn reset_clears_phase() {
        let mut nco = ComplexNco::new();
        let _ = nco.mix_down(Complex32::new(1.0, 0.0), 500.0, 12_000.0);
        nco.reset();
        let out = nco.mix_down(Complex32::new(1.0, 0.0), 0.0, 12_000.0);
        assert!((out.re - 1.0).abs() < 0.01);
    }
}
