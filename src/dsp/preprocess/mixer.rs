//! Block IQ mixer — complex rotation without per-sample `sin/cos`.

use std::f32::consts::TAU;

use crate::source::Complex32;

use super::fir_decim::FirDecimator;
use super::super::simd::complex_mul;

/// Stateful complex rotator `exp(-j·2π·f·t)` for mix-down (or inverse for mix-up).
#[derive(Clone, Debug)]
pub struct IqRotator {
    rot: Complex32,
    step: Complex32,
    last_freq_hz: f32,
    last_rate_hz: f32,
    mix_up: bool,
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
        }
    }

    pub fn reset(&mut self) {
        self.rot = Complex32 { re: 1.0, im: 0.0 };
    }

    fn sync_step(&mut self, freq_hz: f32, sample_rate_hz: f32) {
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
        output.clear();
        if input.is_empty() || sample_rate_hz <= 0.0 {
            return;
        }
        self.sync_step(freq_hz, sample_rate_hz);
        output.reserve(input.len());
        let mut r = self.rot;
        for &sample in input {
            output.push(complex_mul(sample, r));
            r = complex_mul(r, self.step);
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
            if let Some(z) = decim.push(mixed) {
                output.push(z);
            }
        }
        self.rot = r;
    }
}
