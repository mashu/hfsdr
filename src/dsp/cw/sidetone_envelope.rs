//! Sidetone keying envelope — softens BFO product-detector edges.
//!
//! Tracks keyed energy from AGC-normalized IQ magnitude, then ramps audio gain
//! with configurable rise/fall times and edge shape (cosine default).

/// Edge profile for rise and fall ramps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidetoneEnvelopeShape {
    /// Ease-in/out — least clicky (default).
    #[default]
    Cosine,
    /// Constant slope — sharper than cosine.
    Linear,
    /// Ease-in — fastest attack, clickiest.
    Exponential,
}

/// User-facing sidetone envelope parameters.
#[derive(Clone, Copy, Debug)]
pub struct SidetoneEnvelopeSettings {
    pub enabled: bool,
    pub rise_ms: f32,
    pub fall_ms: f32,
    pub shape: SidetoneEnvelopeShape,
}

impl Default for SidetoneEnvelopeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            rise_ms: 2.5,
            fall_ms: 4.0,
            shape: SidetoneEnvelopeShape::Cosine,
        }
    }
}

impl SidetoneEnvelopeSettings {
    pub fn clamped(self) -> Self {
        Self {
            enabled: self.enabled,
            rise_ms: self.rise_ms.clamp(0.1, 20.0),
            fall_ms: self.fall_ms.clamp(0.1, 30.0),
            shape: self.shape,
        }
    }
}

/// Sample-wise gain shaper after the BFO product detector.
#[derive(Clone, Debug)]
pub struct SidetoneEnvelope {
    detect_level: f32,
    /// Slow-release envelope for key detection (ignores audio zero-crossings).
    detect_env: f32,
    peak: f32,
    keyed: bool,
    gain: f32,
    ramp_from: f32,
    ramp_to: f32,
    ramp_pos: u32,
    ramp_len: u32,
}

impl Default for SidetoneEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl SidetoneEnvelope {
    pub fn new() -> Self {
        Self {
            detect_level: 0.0,
            detect_env: 0.0,
            peak: 0.0,
            keyed: false,
            gain: 0.0,
            ramp_from: 0.0,
            ramp_to: 0.0,
            ramp_pos: 0,
            ramp_len: 0,
        }
    }

    pub fn reset_state(&mut self) {
        *self = Self::new();
    }

    #[cfg(test)]
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Shape demod audio using keyed IQ magnitude (`iq_level` = |z| after AGC).
    pub fn process(
        &mut self,
        audio: f32,
        iq_level: f32,
        sample_rate: f32,
        settings: &SidetoneEnvelopeSettings,
    ) -> f32 {
        let settings = settings.clamped();
        if !settings.enabled || sample_rate <= 0.0 {
            return audio;
        }

        self.update_keyed(iq_level, audio.abs(), settings.rise_ms, settings.fall_ms, sample_rate);
        self.tick_ramp(settings.shape);
        audio * self.gain
    }

    fn update_keyed(
        &mut self,
        iq_level: f32,
        audio_level: f32,
        rise_ms: f32,
        fall_ms: f32,
        sample_rate: f32,
    ) {
        let inst = iq_level.max(audio_level).max(0.0);
        if inst > self.detect_env {
            self.detect_env += 0.12 * (inst - self.detect_env);
        } else {
            self.detect_env += 0.00008 * (inst - self.detect_env);
        }
        self.detect_level = self.detect_env;
        if self.detect_level > self.peak {
            self.peak = self.detect_level;
        } else {
            self.peak *= 0.9999;
        }

        let floor = 0.001;
        let span = (self.peak - floor).max(0.01);
        let thr_on = 0.08 * span + 0.002;
        let thr_off = 0.04 * span + 0.001;
        let want_keyed = if self.keyed {
            self.detect_level > thr_off
        } else {
            self.detect_level > thr_on
        };

        if want_keyed != self.keyed {
            self.keyed = want_keyed;
            let (to, ms) = if want_keyed {
                (1.0, rise_ms)
            } else {
                (0.0, fall_ms)
            };
            self.start_ramp(self.gain, to, ms, sample_rate);
        }
    }

    fn start_ramp(&mut self, from: f32, to: f32, ms: f32, sample_rate: f32) {
        self.ramp_from = from;
        self.ramp_to = to;
        self.ramp_len = (sample_rate * ms / 1000.0).round().max(1.0) as u32;
        self.ramp_pos = 0;
    }

    fn tick_ramp(&mut self, shape: SidetoneEnvelopeShape) {
        if self.ramp_len == 0 {
            return;
        }
        if self.ramp_pos >= self.ramp_len {
            self.gain = self.ramp_to;
            return;
        }
        self.ramp_pos += 1;
        let t = self.ramp_pos as f32 / self.ramp_len as f32;
        let k = shape_factor(shape, t);
        self.gain = self.ramp_from + (self.ramp_to - self.ramp_from) * k;
    }
}

fn shape_factor(shape: SidetoneEnvelopeShape, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match shape {
        SidetoneEnvelopeShape::Cosine => 0.5 * (1.0 - (std::f32::consts::PI * t).cos()),
        SidetoneEnvelopeShape::Linear => t,
        SidetoneEnvelopeShape::Exponential => {
            let inv = 1.0 - t;
            1.0 - inv * inv
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn settings(enabled: bool, rise_ms: f32, fall_ms: f32) -> SidetoneEnvelopeSettings {
        SidetoneEnvelopeSettings {
            enabled,
            rise_ms,
            fall_ms,
            shape: SidetoneEnvelopeShape::Cosine,
        }
    }

    #[test]
    fn disabled_passes_through() {
        let mut env = SidetoneEnvelope::new();
        let s = settings(false, 2.5, 4.0);
        assert_eq!(env.process(0.42, 0.2, 12_000.0, &s), 0.42);
    }

    #[test]
    fn gain_ramps_after_key_down() {
        let mut env = SidetoneEnvelope::new();
        let s = settings(true, 5.0, 5.0);
        let rate = 12_000.0;
        let level = 0.2;
        for _ in 0..200 {
            env.process(0.0, 0.001, rate, &s);
        }
        let first = env.process(0.5, level, rate, &s);
        assert!(first.abs() < 0.12, "first keyed sample should be soft, got {first}");
        let mut peak_ratio = 0.0f32;
        for _ in 0..600 {
            let out = env.process(0.5, level, rate, &s);
            peak_ratio = peak_ratio.max(out.abs() / 0.5);
        }
        assert!(peak_ratio > 0.85, "gain should reach unity, ratio={peak_ratio}");
    }

    #[test]
    fn cosine_softer_than_exponential_on_first_sample() {
        let rate = 12_000.0;
        let level = 0.2;
        let mut cosine = SidetoneEnvelope::new();
        let mut exp = SidetoneEnvelope::new();
        for _ in 0..200 {
            cosine.process(0.0, 0.001, rate, &settings(true, 3.0, 3.0));
            exp.process(0.0, 0.001, rate, &settings(true, 3.0, 3.0));
        }
        let c = SidetoneEnvelopeSettings {
            shape: SidetoneEnvelopeShape::Cosine,
            ..settings(true, 3.0, 3.0)
        };
        let e = SidetoneEnvelopeSettings {
            shape: SidetoneEnvelopeShape::Exponential,
            ..settings(true, 3.0, 3.0)
        };
        let out_c = cosine.process(0.5, level, rate, &c);
        let out_e = exp.process(0.5, level, rate, &e);
        assert!(out_e.abs() > out_c.abs());
    }

    #[test]
    fn continuous_carrier_reaches_full_gain() {
        let mut env = SidetoneEnvelope::new();
        let s = settings(true, 2.5, 4.0);
        let rate = 12_000.0;
        for i in 0..rate as usize * 2 {
            let t = i as f32 / rate;
            let audio = (TAU * 650.0 * t).sin() * 0.3;
            let _ = env.process(audio, 0.05, rate, &s);
        }
        assert!(env.gain() > 0.9, "gain={}", env.gain());
    }

    #[test]
    fn keyed_square_wave_produces_audible_tone() {
        let rate = 12_000.0;
        let mut env = SidetoneEnvelope::new();
        let s = settings(true, 2.5, 4.0);
        let mut peak = 0.0f32;
        for i in 0..rate as usize {
            let keyed = (i / 600) % 2 == 0;
            let level = if keyed { 0.25 } else { 0.001 };
            let t = i as f32 / rate;
            let audio = (TAU * 650.0 * t).sin() * if keyed { 0.25 } else { 0.0 };
            let out = env.process(audio, level, rate, &s);
            peak = peak.max(out.abs());
        }
        assert!(peak > 0.05);
    }
}
