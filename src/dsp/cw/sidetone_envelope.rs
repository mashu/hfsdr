//! Sidetone keying envelope — softens BFO product-detector edges.
//!
//! Tracks keyed energy from AGC-normalized IQ magnitude, then ramps audio gain
//! with configurable rise/fall times and edge shape (cosine default). The
//! unkeyed gain rests at [`SidetoneEnvelopeSettings::floor_gain`] rather than
//! zero so weak signals near the detection threshold are dimmed, never chopped.

use super::smoothing::alpha_for_tau;

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

/// Detector ballistics (defaults; see [`SidetoneEnvelopeSettings`]).
pub const DEFAULT_ST_DETECT_ATTACK_S: f32 = 0.0007;
pub const DEFAULT_ST_DETECT_RELEASE_S: f32 = 1.0;
pub const DEFAULT_ST_PEAK_DECAY_S: f32 = 0.83;

/// User-facing sidetone envelope parameters.
#[derive(Clone, Copy, Debug)]
pub struct SidetoneEnvelopeSettings {
    pub enabled: bool,
    pub rise_ms: f32,
    pub fall_ms: f32,
    pub shape: SidetoneEnvelopeShape,
    /// Unkeyed gain (0..0.9). Non-zero keeps weak signals audible between
    /// detections instead of gating them to silence.
    pub floor_gain: f32,
    /// Key-detector envelope attack time constant (seconds).
    pub detect_attack_s: f32,
    /// Key-detector envelope release time constant (seconds).
    pub detect_release_s: f32,
}

impl Default for SidetoneEnvelopeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rise_ms: 2.5,
            fall_ms: 4.0,
            shape: SidetoneEnvelopeShape::Cosine,
            floor_gain: 0.3,
            detect_attack_s: DEFAULT_ST_DETECT_ATTACK_S,
            detect_release_s: DEFAULT_ST_DETECT_RELEASE_S,
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
            floor_gain: self.floor_gain.clamp(0.0, 0.9),
            detect_attack_s: self.detect_attack_s.clamp(1e-4, 0.1),
            detect_release_s: self.detect_release_s.clamp(0.01, 5.0),
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
    /// Cached per-sample coefficients (recomputed when rate/taus change).
    alpha_rate: f32,
    alpha_attack_s: f32,
    alpha_release_s: f32,
    attack_alpha: f32,
    release_alpha: f32,
    peak_keep: f32,
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
            alpha_rate: 0.0,
            alpha_attack_s: 0.0,
            alpha_release_s: 0.0,
            attack_alpha: 0.12,
            release_alpha: 0.00008,
            peak_keep: 0.9999,
        }
    }

    pub fn reset_state(&mut self) {
        *self = Self::new();
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }

    fn sync_alphas(&mut self, sample_rate: f32, attack_s: f32, release_s: f32) {
        if (sample_rate - self.alpha_rate).abs() <= 1.0
            && (attack_s - self.alpha_attack_s).abs() <= 1e-6
            && (release_s - self.alpha_release_s).abs() <= 1e-6
        {
            return;
        }
        self.attack_alpha = alpha_for_tau(sample_rate, attack_s);
        self.release_alpha = alpha_for_tau(sample_rate, release_s);
        self.peak_keep = 1.0 - alpha_for_tau(sample_rate, DEFAULT_ST_PEAK_DECAY_S);
        self.alpha_rate = sample_rate;
        self.alpha_attack_s = attack_s;
        self.alpha_release_s = release_s;
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

        self.sync_alphas(
            sample_rate,
            settings.detect_attack_s,
            settings.detect_release_s,
        );
        self.update_keyed(iq_level, audio.abs(), &settings, sample_rate);
        self.tick_ramp(settings.shape);
        // Gain never rests below the floor — weak signals stay audible even
        // before/without a keying detection (ramps start from >= floor, so
        // this never steps mid-ramp).
        self.gain = self.gain.max(settings.floor_gain);
        audio * self.gain
    }

    fn update_keyed(
        &mut self,
        iq_level: f32,
        audio_level: f32,
        settings: &SidetoneEnvelopeSettings,
        sample_rate: f32,
    ) {
        let inst = iq_level.max(audio_level).max(0.0);
        if inst > self.detect_env {
            self.detect_env += self.attack_alpha * (inst - self.detect_env);
        } else {
            self.detect_env += self.release_alpha * (inst - self.detect_env);
        }
        self.detect_level = self.detect_env;
        if self.detect_level > self.peak {
            self.peak = self.detect_level;
        } else {
            self.peak *= self.peak_keep;
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
                (1.0, settings.rise_ms)
            } else {
                (settings.floor_gain, settings.fall_ms)
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
            floor_gain: 0.0,
            ..SidetoneEnvelopeSettings::default()
        }
    }

    #[test]
    fn disabled_passes_through() {
        let mut env = SidetoneEnvelope::new();
        let s = settings(false, 2.5, 4.0);
        assert_eq!(env.process(0.42, 0.2, 12_000.0, &s), 0.42);
    }

    #[test]
    fn disabled_by_default() {
        assert!(!SidetoneEnvelopeSettings::default().enabled);
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
    fn unkeyed_gain_rests_at_floor() {
        let mut env = SidetoneEnvelope::new();
        let s = SidetoneEnvelopeSettings {
            enabled: true,
            floor_gain: 0.3,
            ..SidetoneEnvelopeSettings::default()
        };
        let rate = 12_000.0;
        // Quiet input below the key-on threshold: audio is dimmed to the
        // floor, not gated to silence.
        for _ in 0..1_000 {
            env.process(0.0015, 0.001, rate, &s);
        }
        assert!(
            (env.gain() - 0.3).abs() < 1e-3,
            "unkeyed gain should rest at floor, got {}",
            env.gain()
        );
        // A keyed passage still ramps up to full gain.
        for _ in 0..2_000 {
            env.process(0.4, 0.25, rate, &s);
        }
        assert!(env.gain() > 0.9, "keyed gain should reach unity, got {}", env.gain());
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

    #[test]
    fn detector_behaves_consistently_across_rates() {
        // Same keying pattern at 12 kHz and 48 kHz should key up in the same
        // wall-clock time (rate-aware ballistics, not per-sample constants).
        let mut gains = Vec::new();
        for rate in [12_000.0f32, 48_000.0] {
            let mut env = SidetoneEnvelope::new();
            let s = settings(true, 2.5, 4.0);
            for _ in 0..(0.05 * rate) as usize {
                env.process(0.0, 0.001, rate, &s);
            }
            for _ in 0..(0.05 * rate) as usize {
                env.process(0.4, 0.25, rate, &s);
            }
            gains.push(env.gain());
        }
        assert!(
            (gains[0] - gains[1]).abs() < 0.1,
            "gain after 50 ms keyed should match across rates: {gains:?}"
        );
    }
}
