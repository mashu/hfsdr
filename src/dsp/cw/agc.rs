//! CW-oriented automatic gain control (fast attack, adjustable decay).
//!
//! Short attack so a strong dit does not blast through; a longer decay/hang so
//! the floor does not pump up between dits. When disabled the caller applies a
//! fixed manual gain instead — many contesters prefer that so a loud neighbour
//! cannot pump the wanted signal down.

/// Envelope-following AGC with configurable attack/decay.
#[derive(Clone, Debug)]
pub struct CwAgc {
    gain: f32,
    envelope: f32,
}

impl Default for CwAgc {
    fn default() -> Self {
        Self::new()
    }
}

impl CwAgc {
    pub fn new() -> Self {
        Self {
            gain: 1.0,
            envelope: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.gain = 1.0;
        self.envelope = 0.0;
    }

    /// Return the gain to apply for a sample whose magnitude is `level`.
    pub fn gain_for(
        &mut self,
        level: f32,
        sample_rate: f32,
        target: f32,
        attack_ms: f32,
        decay_ms: f32,
    ) -> f32 {
        if sample_rate <= 0.0 {
            return 1.0;
        }
        let attack = (-1.0 / (sample_rate * (attack_ms.max(0.1) / 1000.0))).exp();
        let decay = (-1.0 / (sample_rate * (decay_ms.max(1.0) / 1000.0))).exp();
        if level > self.envelope {
            self.envelope = attack * self.envelope + (1.0 - attack) * level;
        } else {
            self.envelope = decay * self.envelope + (1.0 - decay) * level;
        }

        let desired = target / self.envelope.max(1e-7);
        self.gain = 0.9 * self.gain + 0.1 * desired;
        self.gain = self.gain.clamp(0.02, 64.0);
        self.gain
    }
}
