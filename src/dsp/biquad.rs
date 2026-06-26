//! Biquad IIR filters (RBJ audio EQ cookbook).

use std::f32::consts::PI;

#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn set_bandpass(&mut self, sample_rate: f32, fc: f32, bandwidth: f32) {
        self.reset_state();
        if sample_rate <= 0.0 || fc <= 0.0 || bandwidth <= 0.0 {
            return;
        }
        let fc = fc.clamp(20.0, sample_rate * 0.45);
        let q = (fc / bandwidth).clamp(0.5, 50.0);
        let omega = 2.0 * PI * fc / sample_rate;
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.b1 = 0.0;
        self.b2 = -alpha / a0;
        self.a1 = (-2.0 * cos) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// RBJ lowpass — used for optional 2-pole IQ channel shaping (non-linear phase).
    pub fn set_lowpass(&mut self, sample_rate: f32, fc: f32, q: f32) {
        self.reset_state();
        if sample_rate <= 0.0 || fc <= 0.0 {
            return;
        }
        let fc = fc.clamp(20.0, sample_rate * 0.45);
        let q = q.clamp(0.5, 8.0);
        let omega = 2.0 * PI * fc / sample_rate;
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cos) * 0.5) / a0;
        self.b1 = (1.0 - cos) / a0;
        self.b2 = ((1.0 - cos) * 0.5) / a0;
        self.a1 = (-2.0 * cos) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Linear magnitude |H(e^jω)| at `freq_hz` (coefficients only — no state).
    pub fn magnitude_linear(&self, sample_rate: f32, freq_hz: f32) -> f32 {
        if sample_rate <= 0.0 {
            return 1.0;
        }
        let w = 2.0 * PI * freq_hz / sample_rate;
        let c1 = w.cos();
        let s1 = w.sin();
        let c2 = (2.0 * w).cos();
        let s2 = (2.0 * w).sin();
        let num_r = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_i = -self.b1 * s1 - self.b2 * s2;
        let den_r = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_i = -self.a1 * s1 - self.a2 * s2;
        let den_sq = den_r * den_r + den_i * den_i;
        if den_sq < 1e-20 {
            return 1.0;
        }
        let hr = (num_r * den_r + num_i * den_i) / den_sq;
        let hi = (num_i * den_r - num_r * den_i) / den_sq;
        (hr * hr + hi * hi).sqrt()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn bandpass_passes_tone_at_center() {
        let sample_rate = 12_000.0;
        let fc = 650.0;
        let mut bp = Biquad::new();
        bp.set_bandpass(sample_rate, fc, 200.0);

        let mut peak_in = 0.0f32;
        let mut peak_out = 0.0f32;
        for n in 0..sample_rate as usize {
            let t = n as f32 / sample_rate;
            let x = (TAU * fc * t).sin();
            peak_in = peak_in.max(x.abs());
            peak_out = peak_out.max(bp.process(x).abs());
        }
        assert!(peak_out > peak_in * 0.25);
    }

    #[test]
    fn bandpass_rejects_far_off_tone() {
        let sample_rate = 12_000.0;
        let mut bp_near = Biquad::new();
        bp_near.set_bandpass(sample_rate, 650.0, 200.0);
        let mut bp_far = Biquad::new();
        bp_far.set_bandpass(sample_rate, 650.0, 200.0);

        let mut peak_near = 0.0f32;
        let mut peak_far = 0.0f32;
        for n in 0..sample_rate as usize {
            let t = n as f32 / sample_rate;
            peak_near = peak_near.max(bp_near.process((TAU * 650.0 * t).sin()).abs());
            peak_far = peak_far.max(bp_far.process((TAU * 2_000.0 * t).sin()).abs());
        }
        assert!(peak_near > peak_far * 3.0);
    }
}
