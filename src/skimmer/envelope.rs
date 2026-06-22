//! Shared envelope tracker for skimmer decoders.

use super::config::EnvelopeSettings;

/// Smoothed magnitude envelope with adaptive key-down/key-up thresholds.
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
        let signal_present = span > 0.02 * self.peak.max(1e-6) && self.peak > 1e-5;
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

#[derive(Clone, Copy, Debug)]
pub struct EnvelopeStep {
    pub env: f32,
    pub span: f32,
    pub thr_high: f32,
    pub thr_low: f32,
    pub signal_present: bool,
}
