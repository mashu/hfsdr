//! Steerable IQ notch (single interferer).

use std::f32::consts::TAU;

use crate::dsp::biquad::Biquad;
use crate::source::Complex32;

/// Rotate to notch frequency, apply narrow band-reject, rotate back.
#[derive(Clone, Debug)]
pub struct IqNotch {
    phase: f32,
    notch: Biquad,
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
            notch: Biquad::new(),
            last_width_hz: 0.0,
            last_rate: 0.0,
        }
    }

    pub fn sync(&mut self, sample_rate: f32, width_hz: f32) {
        if sample_rate != self.last_rate || width_hz != self.last_width_hz {
            self.notch
                .set_notch(sample_rate, 20.0, width_hz.clamp(10.0, 500.0));
            self.last_rate = sample_rate;
            self.last_width_hz = width_hz;
        }
    }

    pub fn reset_state(&mut self) {
        self.phase = 0.0;
        self.notch.reset_state();
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
        let filtered_re = self.notch.process(rot_re);
        let filtered_im = self.notch.process(rot_im);

        Complex32 {
            re: filtered_re * cos - filtered_im * sin,
            im: filtered_re * sin + filtered_im * cos,
        }
    }
}
