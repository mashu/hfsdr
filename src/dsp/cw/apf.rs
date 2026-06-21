//! Audio peak filter (APF) — a gentle resonant boost at the CW pitch.
//!
//! Sits on top of the channel filter: it pulls the wanted tone forward without
//! the harshness of a narrow brick filter. Implemented as the input plus a
//! scaled, modest-Q bandpass at the pitch.

use crate::dsp::biquad::Biquad;

/// Resonant audio peak at the CW pitch.
#[derive(Clone, Debug)]
pub struct AudioPeakFilter {
    band: Biquad,
    last_rate: f32,
    last_pitch: f32,
    last_width: f32,
}

impl Default for AudioPeakFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPeakFilter {
    pub fn new() -> Self {
        Self {
            band: Biquad::new(),
            last_rate: 0.0,
            last_pitch: 0.0,
            last_width: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.band.reset_state();
    }

    fn sync(&mut self, sample_rate: f32, pitch_hz: f32, width_hz: f32) {
        if sample_rate != self.last_rate
            || pitch_hz != self.last_pitch
            || width_hz != self.last_width
        {
            self.band
                .set_bandpass(sample_rate, pitch_hz.max(50.0), width_hz.max(40.0));
            self.last_rate = sample_rate;
            self.last_pitch = pitch_hz;
            self.last_width = width_hz;
        }
    }

    /// `gain` scales the resonant boost added to the dry signal.
    pub fn process(&mut self, sample: f32, sample_rate: f32, pitch_hz: f32, width_hz: f32, gain: f32) -> f32 {
        self.sync(sample_rate, pitch_hz, width_hz);
        sample + gain.max(0.0) * self.band.process(sample)
    }
}
