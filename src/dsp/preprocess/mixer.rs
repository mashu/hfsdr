//! Block IQ mixer — complex rotation without per-sample `sin/cos`.

use std::f32::consts::TAU;

use crate::source::Complex32;

use super::fir_decim::FirDecimator;
use super::super::simd::{complex_mul, complex_mul_block};

/// Stateful complex rotator `exp(-j·2π·f·t)` for mix-down (or inverse for mix-up).
#[derive(Clone, Debug)]
pub struct IqRotator {
    rot: Complex32,
    step: Complex32,
    last_freq_hz: f32,
    last_rate_hz: f32,
    mix_up: bool,
    rot_scratch: Vec<Complex32>,
}

impl Default for IqRotator {
    fn default() -> Self {
        Self::new(false)
    }
}

impl IqRotator {
    pub fn new(mix_up: bool) -> Self {
        Self {
            rot: Complex32 { re: 1.0, im: 0.0 },
            step: Complex32 { re: 1.0, im: 0.0 },
            last_freq_hz: 0.0,
            last_rate_hz: 0.0,
            mix_up,
            rot_scratch: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.rot = Complex32 { re: 1.0, im: 0.0 };
    }

    pub fn sync_step(&mut self, freq_hz: f32, sample_rate_hz: f32) {
        if sample_rate_hz <= 0.0 {
            return;
        }
        if (freq_hz - self.last_freq_hz).abs() <= f32::EPSILON
            && (sample_rate_hz - self.last_rate_hz).abs() <= 1.0
        {
            return;
        }
        let omega = TAU * freq_hz / sample_rate_hz;
        let (sin, cos) = omega.sin_cos();
        self.step = if self.mix_up {
            Complex32 { re: cos, im: sin }
        } else {
            Complex32 { re: cos, im: -sin }
        };
        self.last_freq_hz = freq_hz;
        self.last_rate_hz = sample_rate_hz;
    }

    #[inline]
    pub fn mix_one(&mut self, sample: Complex32, freq_hz: f32, sample_rate_hz: f32) -> Complex32 {
        self.sync_step(freq_hz, sample_rate_hz);
        self.mix_sample(sample)
    }

    /// Mix one sample after [`Self::sync_step`] has been called for this block.
    #[inline]
    pub fn mix_sample(&mut self, sample: Complex32) -> Complex32 {
        let out = complex_mul(sample, self.rot);
        self.rot = complex_mul(self.rot, self.step);
        out
    }

    /// Mix a block in one pass; reuses rotation recurrence (no trig in the inner loop).
    pub fn mix_block(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<Complex32>,
        freq_hz: f32,
        sample_rate_hz: f32,
    ) {
        if input.is_empty() || sample_rate_hz <= 0.0 {
            output.clear();
            return;
        }
        self.sync_step(freq_hz, sample_rate_hz);
        output.clear();
        output.reserve(input.len());
        if input.len() >= 64 {
            self.rot_scratch.resize(input.len(), Complex32 { re: 0.0, im: 0.0 });
            let mut r = self.rot;
            let step = self.step;
            for slot in self.rot_scratch.iter_mut() {
                *slot = r;
                r = complex_mul(r, step);
            }
            self.rot = r;
            output.resize(input.len(), Complex32 { re: 0.0, im: 0.0 });
            complex_mul_block(input, &self.rot_scratch, &mut output[..]);
            return;
        }
        let mut r = self.rot;
        let step = self.step;
        for &sample in input {
            output.push(complex_mul(sample, r));
            r = complex_mul(r, step);
        }
        self.rot = r;
    }

    /// Fused block mix-down + decimation — one pass, no intermediate IQ buffer.
    pub fn mix_and_decimate(
        &mut self,
        input: &[Complex32],
        freq_hz: f32,
        sample_rate_hz: f32,
        decim: &mut FirDecimator,
        output: &mut Vec<Complex32>,
        bypass_decim_fir: bool,
    ) {
        if input.is_empty() || sample_rate_hz <= 0.0 {
            return;
        }
        self.sync_step(freq_hz, sample_rate_hz);
        let mut r = self.rot;
        let step = self.step;
        for &sample in input {
            let mixed = complex_mul(sample, r);
            r = complex_mul(r, step);
            if let Some(z) = decim.push(mixed, bypass_decim_fir) {
                output.push(z);
            }
        }
        self.rot = r;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone_block(n: usize, rate: f32, hz: f32) -> Vec<Complex32> {
        (0..n)
            .map(|i| {
                let p = TAU * hz * i as f32 / rate;
                Complex32::new(p.cos(), p.sin())
            })
            .collect()
    }

    #[test]
    fn mix_one_shifts_tone() {
        let mut rot = IqRotator::default();
        let rate = 12_000.0;
        let block = tone_block(256, rate, 500.0);
        let mixed = rot.mix_one(block[128], 500.0, rate);
        assert!(mixed.norm() > 0.5);
    }

    #[test]
    fn mix_block_matches_length() {
        let mut rot = IqRotator::default();
        let block = tone_block(128, 12_000.0, 100.0);
        let mut out = Vec::new();
        rot.mix_block(&block, &mut out, 100.0, 12_000.0);
        assert_eq!(out.len(), block.len());
    }

    #[test]
    fn mix_block_uses_simd_path_for_long_input() {
        let mut rot = IqRotator::default();
        let block = tone_block(128, 48_000.0, 200.0);
        let mut out = Vec::new();
        rot.mix_block(&block, &mut out, 200.0, 48_000.0);
        assert_eq!(out.len(), 128);
    }

    #[test]
    fn mix_and_decimate_reduces_rate() {
        let mut rot = IqRotator::default();
        let mut decim = FirDecimator::with_factor(48_000.0, 4, true);
        let block = tone_block(200, 48_000.0, 1_000.0);
        let mut out = Vec::new();
        rot.mix_and_decimate(&block, 1_000.0, 48_000.0, &mut decim, &mut out, false);
        assert!(!out.is_empty());
        assert!(out.len() < block.len());
    }
}
