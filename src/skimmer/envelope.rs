//! Shared envelope tracker and decode gate for skimmer decoders.

use super::config::EnvelopeSettings;

/// Smoothed magnitude envelope with Schmitt (hysteresis) keying thresholds.
#[derive(Clone, Debug)]
pub struct KeyingEnvelope {
    env: f32,
    peak: f32,
    noise: f32,
    settings: EnvelopeSettings,
}

impl KeyingEnvelope {
    pub fn new(settings: EnvelopeSettings) -> Self {
        Self {
            env: 0.0,
            peak: 0.0,
            noise: 0.0,
            settings: settings.clamped(),
        }
    }

    pub fn update(&mut self, x: f32) -> EnvelopeStep {
        let inst = x.abs();
        let a = if inst > self.env { 0.05 } else { 0.01 };
        self.env += a * (inst - self.env);
        if self.env > self.peak {
            self.peak = self.env;
        } else {
            self.peak *= 0.99995;
        }
        if self.env < self.noise {
            self.noise += 0.02 * (self.env - self.noise);
        } else {
            self.noise += 0.0002 * (self.env - self.noise);
        }
        let span = self.peak - self.noise;
        let min_span = self.settings.min_span_fraction * self.peak.max(1e-6);
        let signal_present = span > min_span && self.peak > 1e-5;
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
    pub fn new(audio_rate: f32, gate_ms: f32) -> Self {
        let warmup = (audio_rate * gate_ms / 1000.0).round() as u32;
        let warmup = warmup.clamp(12, 2_000);
        Self {
            armed: false,
            above: 0,
            below: 0,
            warmup,
            release: (warmup * 10).clamp(80, 8_000),
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
        let mut env = KeyingEnvelope::new(EnvelopeSettings::default());
        let mut last = env.update(0.0);
        for _ in 0..200 {
            last = env.update(0.8);
        }
        assert!(last.env > 0.3);
        assert!(last.signal_present);
        assert!(last.thr_high > last.thr_low);
    }

    #[test]
    fn decode_gate_arms_after_warmup() {
        let mut gate = DecodeGate::new(12_000.0, 25.0);
        let mut env = KeyingEnvelope::new(EnvelopeSettings {
            thr_low: 0.3,
            thr_high: 0.45,
            min_span_fraction: 0.05,
        });
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
