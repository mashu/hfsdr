//! Steerable IQ notch (single interferer).

use std::f32::consts::TAU;

use crate::source::Complex32;

/// Rotate to `offset_hz`, remove DC in the mixed domain, rotate back.
///
/// The interferer becomes DC after mix-down. A biquad cannot place a null at
/// 0 Hz, so a leaky DC estimator (highpass) removes it; `width_hz` sets the
/// corner via `dc_alpha`.
#[derive(Clone, Debug)]
pub struct IqNotch {
    phase: f32,
    dc_i: f32,
    dc_q: f32,
    dc_alpha: f32,
    last_width_hz: f32,
    last_rate: f32,
}

impl Default for IqNotch {
    fn default() -> Self {
        Self::new()
    }
}

impl IqNotch {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            dc_i: 0.0,
            dc_q: 0.0,
            dc_alpha: 0.99,
            last_width_hz: 0.0,
            last_rate: 0.0,
        }
    }

    pub fn sync(&mut self, sample_rate: f32, width_hz: f32) {
        if sample_rate != self.last_rate || width_hz != self.last_width_hz {
            let w = width_hz.clamp(10.0, 500.0);
            self.dc_alpha = (1.0 - (2.0 * TAU * w / sample_rate)).clamp(0.5, 0.9999);
            self.last_rate = sample_rate;
            self.last_width_hz = width_hz;
        }
    }

    pub fn reset_state(&mut self) {
        self.phase = 0.0;
        self.dc_i = 0.0;
        self.dc_q = 0.0;
    }

    fn dc_block(alpha: f32, x: f32, state: &mut f32) -> f32 {
        *state = alpha * *state + (1.0 - alpha) * x;
        x - *state
    }

    pub fn process(&mut self, sample: Complex32, offset_hz: f32, sample_rate: f32) -> Complex32 {
        if sample_rate <= 0.0 {
            return sample;
        }
        let inc = TAU * offset_hz / sample_rate;
        let (sin, cos) = self.phase.sin_cos();
        self.phase += inc;
        if self.phase >= TAU {
            self.phase -= TAU;
        }

        let rot_re = sample.re * cos + sample.im * sin;
        let rot_im = -sample.re * sin + sample.im * cos;
        let alpha = self.dc_alpha;
        let filtered_re = Self::dc_block(alpha, rot_re, &mut self.dc_i);
        let filtered_im = Self::dc_block(alpha, rot_im, &mut self.dc_q);

        Complex32 {
            re: filtered_re * cos - filtered_im * sin,
            im: filtered_re * sin + filtered_im * cos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn measure_power_ratio(
        notch: &mut IqNotch,
        rate: f32,
        tone_hz: f32,
        notch_offset_hz: f32,
        warm: usize,
        measure: usize,
    ) -> f32 {
        let mut in_pwr = 0.0f32;
        let mut out_pwr = 0.0f32;
        for i in 0..warm + measure {
            let t = i as f32 / rate;
            let phase = TAU * tone_hz * t;
            let s = Complex32::new(phase.cos(), phase.sin());
            let o = notch.process(s, notch_offset_hz, rate);
            if i >= warm {
                in_pwr += s.norm_sqr();
                out_pwr += o.norm_sqr();
            }
        }
        out_pwr / in_pwr.max(1e-12)
    }

    #[test]
    fn attenuates_on_frequency_after_warmup() {
        let rate = 12_000.0;
        let offset = 350.0;
        let mut notch = IqNotch::new();
        notch.sync(rate, 80.0);
        let warm = rate as usize * 2;
        let measure = rate as usize;
        let ratio = measure_power_ratio(&mut notch, rate, offset, offset, warm, measure);
        assert!(
            ratio < 0.1,
            "on-frequency power ratio {ratio} — expected strong notch (>10 dB)"
        );
    }

    #[test]
    fn leaves_off_frequency_mostly_intact() {
        let rate = 12_000.0;
        let offset = 350.0;
        let mut notch = IqNotch::new();
        notch.sync(rate, 80.0);
        let warm = rate as usize * 2;
        let measure = rate as usize;
        let ratio = measure_power_ratio(&mut notch, rate, offset + 600.0, offset, warm, measure);
        assert!(
            ratio > 0.4,
            "off-frequency power ratio {ratio} — notch should not dig a wide hole"
        );
    }

    #[test]
    fn process_is_finite_and_reset_clears_state() {
        let rate = 12_000.0;
        let mut notch = IqNotch::new();
        notch.sync(rate, 80.0);
        let mut last = Complex32::new(0.0, 0.0);
        for i in 0..rate as usize {
            let t = i as f32 / rate;
            let s = Complex32::new((TAU * 200.0 * t).cos(), (TAU * 200.0 * t).sin());
            last = notch.process(s, 200.0, rate);
            assert!(last.re.is_finite() && last.im.is_finite());
        }
        assert!(last.norm() > 0.0);
        notch.reset_state();
        let quiet = notch.process(Complex32::new(0.0, 0.0), 0.0, rate);
        assert!(quiet.norm() < 0.1);
    }

    #[test]
    fn zero_rate_is_passthrough() {
        let mut notch = IqNotch::new();
        let s = Complex32::new(1.0, 0.0);
        assert_eq!(notch.process(s, 100.0, 0.0), s);
    }
}
