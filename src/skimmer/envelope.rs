//! Shared envelope tracker and decode gate for skimmer decoders.
//!
//! [`KeyingEnvelope`] tracks three levels of the rectified channel envelope:
//! a lightly smoothed instantaneous envelope, a noise-floor estimate, and a
//! key-down ("mark") level estimate. Schmitt thresholds sit between noise and
//! mark as configured fractions of the span. All ballistics are expressed as
//! time constants and converted per sample rate, so behaviour is identical at
//! any envelope rate — and the mark tracker is fast enough to ride through
//! QSB fades of a few seconds' period.

use super::config::EnvelopeSettings;

/// One-pole coefficient for a time constant at a sample rate.
fn alpha(tau_s: f32, sample_rate: f32) -> f32 {
    if tau_s <= 0.0 || sample_rate <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (tau_s * sample_rate)).exp()
}

/// Envelope smoothing attack/release (light — the channel LPF does the work).
const ENV_ATTACK_S: f32 = 0.002;
const ENV_RELEASE_S: f32 = 0.004;
/// Noise floor falls quickly to the space level, rises slowly through marks.
const NOISE_FALL_S: f32 = 0.080;
const NOISE_RISE_S: f32 = 2.0;
/// Mark level attacks in a few dits, decays through a QSB trough.
const MARK_ATTACK_S: f32 = 0.040;
const MARK_DECAY_S: f32 = 0.8;
/// Faster adaptation right after construction so estimates converge.
const WARMUP_S: f32 = 0.25;
/// Minimum mark/noise ratio (~4 dB) before keying is trusted.
const MIN_MARK_NOISE_RATIO: f32 = 1.6;

/// Smoothed magnitude envelope with adaptive Schmitt (hysteresis) thresholds.
#[derive(Clone, Debug)]
pub struct KeyingEnvelope {
    settings: EnvelopeSettings,
    sample_rate: f32,
    env: f32,
    noise: f32,
    mark: f32,
    primed: bool,
    warmup_left: u32,
    a_env_up: f32,
    a_env_dn: f32,
    a_noise_dn: f32,
    a_noise_up: f32,
    a_mark_up: f32,
    a_mark_dn: f32,
}

impl KeyingEnvelope {
    pub fn new(settings: EnvelopeSettings, sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        Self {
            settings: settings.clamped(),
            sample_rate,
            env: 0.0,
            noise: 0.0,
            mark: 0.0,
            primed: false,
            warmup_left: (WARMUP_S * sample_rate) as u32,
            a_env_up: alpha(ENV_ATTACK_S, sample_rate),
            a_env_dn: alpha(ENV_RELEASE_S, sample_rate),
            a_noise_dn: alpha(NOISE_FALL_S, sample_rate),
            a_noise_up: alpha(NOISE_RISE_S, sample_rate),
            a_mark_up: alpha(MARK_ATTACK_S, sample_rate),
            a_mark_dn: alpha(MARK_DECAY_S, sample_rate),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn update(&mut self, x: f32) -> EnvelopeStep {
        let inst = x.abs();
        if !self.primed {
            self.primed = true;
            self.env = inst;
            self.noise = inst;
            self.mark = inst;
        }

        let a_env = if inst > self.env { self.a_env_up } else { self.a_env_dn };
        self.env += a_env * (inst - self.env);

        // Warmup: converge estimates several times faster right after start.
        let boost = if self.warmup_left > 0 {
            self.warmup_left -= 1;
            8.0
        } else {
            1.0
        };

        if self.env < self.noise {
            self.noise += (self.a_noise_dn * boost).min(1.0) * (self.env - self.noise);
        } else {
            self.noise += (self.a_noise_up * boost).min(1.0) * (self.env - self.noise);
        }
        if self.env > self.mark {
            self.mark += (self.a_mark_up * boost).min(1.0) * (self.env - self.mark);
        } else {
            self.mark += (self.a_mark_dn * boost).min(1.0) * (self.env - self.mark);
        }
        if self.mark < self.noise {
            self.mark = self.noise;
        }

        let span = self.mark - self.noise;
        let min_span = self.settings.min_span_fraction * self.mark.max(1e-9);
        let signal_present =
            span > min_span && self.mark > MIN_MARK_NOISE_RATIO * self.noise && self.mark > 1e-7;
        let thr_high = self.noise + self.settings.thr_high * span;
        let thr_low = self.noise + self.settings.thr_low * span;
        EnvelopeStep {
            env: self.env,
            span,
            thr_high,
            thr_low,
            signal_present,
        }
    }
}

/// Require sustained keyed energy before feeding the Morse decoder.
#[derive(Clone, Debug)]
pub struct DecodeGate {
    armed: bool,
    above: u32,
    below: u32,
    warmup: u32,
    release: u32,
}

impl DecodeGate {
    pub fn new(sample_rate: f32, gate_ms: f32) -> Self {
        let warmup = (sample_rate * gate_ms / 1000.0).round() as u32;
        let warmup = warmup.clamp(4, 2_000);
        // Hold the gate for a couple of seconds of silence before disarming.
        let release = (sample_rate * 2.0).round() as u32;
        Self {
            armed: false,
            above: 0,
            below: 0,
            warmup,
            release: release.clamp(warmup * 4, 100_000),
        }
    }

    pub fn feed(&mut self, step: &EnvelopeStep) -> bool {
        let keyed = step.signal_present && step.env > step.thr_high;
        if keyed {
            self.above = self.above.saturating_add(1);
            self.below = 0;
            if self.above >= self.warmup {
                self.armed = true;
            }
        } else {
            self.below = self.below.saturating_add(1);
            if self.below >= self.release {
                self.armed = false;
                self.above = 0;
            }
        }
        self.armed
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn reset(&mut self) {
        self.armed = false;
        self.above = 0;
        self.below = 0;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnvelopeStep {
    pub env: f32,
    pub span: f32,
    pub thr_high: f32,
    pub thr_low: f32,
    pub signal_present: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::config::EnvelopeSettings;

    #[test]
    fn envelope_rises_on_tone() {
        let mut env = KeyingEnvelope::new(EnvelopeSettings::default(), 500.0);
        for _ in 0..50 {
            env.update(0.001);
        }
        let mut last = env.update(0.8);
        for _ in 0..200 {
            last = env.update(0.8);
        }
        assert!(last.env > 0.3);
        assert!(last.signal_present);
        assert!(last.thr_high > last.thr_low);
    }

    #[test]
    fn tracks_qsb_fade_within_a_second() {
        let mut env = KeyingEnvelope::new(EnvelopeSettings::default(), 500.0);
        // Strong keying: alternate 60 ms on / 60 ms off at amplitude 1.0.
        for _ in 0..20 {
            for _ in 0..30 {
                env.update(1.0);
            }
            for _ in 0..30 {
                env.update(0.01);
            }
        }
        // Fade 14 dB: keying at 0.2 must re-cross the adapted threshold within ~1 s.
        let mut crossed = false;
        for _ in 0..10 {
            for _ in 0..30 {
                let step = env.update(0.2);
                if step.signal_present && step.env > step.thr_high {
                    crossed = true;
                }
            }
            for _ in 0..30 {
                env.update(0.01);
            }
        }
        assert!(crossed, "thresholds failed to follow a QSB fade");
    }

    #[test]
    fn no_signal_flag_on_flat_noise() {
        let mut env = KeyingEnvelope::new(EnvelopeSettings::default(), 500.0);
        let mut lcg = 12345u32;
        let mut present = 0;
        for i in 0..2_000 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = 0.1 + 0.02 * ((lcg >> 16) as f32 / 65535.0 - 0.5);
            let step = env.update(n);
            if i > 500 && step.signal_present {
                present += 1;
            }
        }
        assert!(
            present < 150,
            "flat noise flagged as signal {present} times"
        );
    }

    #[test]
    fn decode_gate_arms_after_warmup() {
        let mut gate = DecodeGate::new(500.0, 25.0);
        let mut env = KeyingEnvelope::new(
            EnvelopeSettings {
                thr_low: 0.3,
                thr_high: 0.45,
                min_span_fraction: 0.05,
            },
            500.0,
        );
        for _ in 0..100 {
            env.update(0.001);
        }
        assert!(!gate.is_armed());
        for _ in 0..500 {
            let step = env.update(0.9);
            gate.feed(&step);
        }
        assert!(gate.is_armed());
        gate.reset();
        assert!(!gate.is_armed());
    }
}
