//! Optional 2-pole IIR channel filter (I/Q parallel biquads).
//!
//! Steeper skirts than a short FIR but **non-linear phase** — can ring on keying
//! edges. Prefer [`super::fir::FirFilter`] for normal CW; this exists for A/B.

use crate::dsp::biquad::Biquad;
use crate::source::Complex32;

#[derive(Clone, Debug)]
pub struct IirChannelFilter {
    i: Biquad,
    q: Biquad,
    last_rate: f32,
    last_cutoff: f32,
}

impl Default for IirChannelFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl IirChannelFilter {
    pub fn new() -> Self {
        Self {
            i: Biquad::new(),
            q: Biquad::new(),
            last_rate: 0.0,
            last_cutoff: 0.0,
        }
    }

    pub fn sync(&mut self, sample_rate: f32, bandwidth_hz: f32) {
        let cutoff = (bandwidth_hz * 0.5).max(10.0);
        if (sample_rate - self.last_rate).abs() > 1.0
            || (cutoff - self.last_cutoff).abs() > 1.0
        {
            let q = 0.707;
            self.i.set_lowpass(sample_rate, cutoff, q);
            self.q.set_lowpass(sample_rate, cutoff, q);
            self.last_rate = sample_rate;
            self.last_cutoff = cutoff;
        }
    }

    pub fn reset_state(&mut self) {
        self.i.reset_state();
        self.q.reset_state();
    }

    pub fn process_complex(&mut self, sample: Complex32) -> Complex32 {
        Complex32 {
            re: self.i.process(sample.re),
            im: self.q.process(sample.im),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn lowpass_attenuates_high_tone() {
        let rate = 12_000.0;
        let bw = 200.0;

        fn tone_power(rate: f32, bw: f32, tone_hz: f32) -> f32 {
            let mut filt = IirChannelFilter::new();
            filt.sync(rate, bw);
            let mut pwr = 0.0f32;
            for i in 0..rate as usize {
                let t = i as f32 / rate;
                let s = Complex32::new((TAU * tone_hz * t).cos(), (TAU * tone_hz * t).sin());
                let o = filt.process_complex(s);
                if i > rate as usize / 2 {
                    pwr += o.norm_sqr();
                }
            }
            pwr
        }

        let lo_pwr = tone_power(rate, bw, 30.0);
        let hi_pwr = tone_power(rate, bw, 900.0);
        assert!(lo_pwr > hi_pwr * 2.0, "IIR should pass DC-near tone more than far tone");
    }
}
