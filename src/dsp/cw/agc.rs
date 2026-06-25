//! CW-oriented automatic gain control (fast attack, adjustable decay).
//!
//! Short attack so a strong dit does not blast through; a longer decay/hang so
//! the floor does not pump up between dits. When disabled the caller applies a
//! fixed manual gain instead — many contesters prefer that so a loud neighbour
//! cannot pump the wanted signal down.

use super::settings::AgcMode;

/// Envelope-following AGC with configurable attack/decay.
#[derive(Clone, Debug)]
pub struct CwAgc {
    gain: f32,
    envelope: f32,
    fast_env: f32,
    slow_env: f32,
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
            fast_env: 0.0,
            slow_env: 0.0,
        }
    }

    pub fn reset_state(&mut self) {
        self.gain = 1.0;
        self.envelope = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
    }

    fn update_envelope(
        &mut self,
        level: f32,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
        mode: AgcMode,
    ) {
        if sample_rate <= 0.0 {
            return;
        }
        let attack = (-1.0 / (sample_rate * (attack_ms.max(0.1) / 1000.0))).exp();
        let decay = (-1.0 / (sample_rate * (decay_ms.max(1.0) / 1000.0))).exp();

        match mode {
            AgcMode::DualLoop => {
                let slow_attack =
                    (-1.0 / (sample_rate * (attack_ms.max(0.1) * 10.0 / 1000.0))).exp();
                let slow_decay =
                    (-1.0 / (sample_rate * (decay_ms.max(1.0) * 8.0 / 1000.0))).exp();
                if level > self.fast_env {
                    self.fast_env = attack * self.fast_env + (1.0 - attack) * level;
                } else {
                    self.fast_env = decay * self.fast_env + (1.0 - decay) * level;
                }
                if level > self.slow_env {
                    self.slow_env = slow_attack * self.slow_env + (1.0 - slow_attack) * level;
                } else {
                    self.slow_env = slow_decay * self.slow_env + (1.0 - slow_decay) * level;
                }
                let control = self.fast_env.max(self.slow_env * 0.55);
                self.envelope = control;
            }
            _ => {
                if level > self.envelope {
                    self.envelope = attack * self.envelope + (1.0 - attack) * level;
                } else {
                    self.envelope = decay * self.envelope + (1.0 - decay) * level;
                }
            }
        }
    }

    /// Track IQ envelope for metering without changing AGC gain.
    pub fn track_envelope(
        &mut self,
        level: f32,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
        mode: AgcMode,
    ) {
        self.update_envelope(level, sample_rate, attack_ms, decay_ms, mode);
    }

    /// Return the gain to apply for a sample whose magnitude is `level`.
    pub fn gain_for(
        &mut self,
        level: f32,
        sample_rate: f32,
        target: f32,
        attack_ms: f32,
        decay_ms: f32,
        mode: AgcMode,
    ) -> f32 {
        if sample_rate <= 0.0 {
            return 1.0;
        }
        self.update_envelope(level, sample_rate, attack_ms, decay_ms, mode);

        let desired = target / self.envelope.max(1e-7);

        self.gain = match mode {
            AgcMode::Envelope => 0.9 * self.gain + 0.1 * desired,
            AgcMode::Hang => {
                if desired < self.gain {
                    0.85 * self.gain + 0.15 * desired
                } else {
                    let hang = (-1.0 / (sample_rate * (decay_ms.max(1.0) * 4.0 / 1000.0))).exp();
                    hang * self.gain + (1.0 - hang) * desired
                }
            }
            AgcMode::DualLoop => 0.88 * self.gain + 0.12 * desired,
        };
        self.gain = self.gain.clamp(0.02, 64.0);
        self.gain
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }

    pub fn envelope(&self) -> f32 {
        self.envelope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::settings::AgcMode;

    #[test]
    fn tracks_envelope_and_clamps_gain() {
        let mut agc = CwAgc::new();
        let g0 = agc.gain_for(0.5, 12_000.0, 0.25, 3.0, 120.0, AgcMode::Envelope);
        let g1 = agc.gain_for(0.5, 12_000.0, 0.25, 3.0, 120.0, AgcMode::Envelope);
        assert!(g0 > 0.0);
        assert!(g1 > 0.0);
        assert!(g1 <= 64.0);
    }

    #[test]
    fn dual_loop_responds_to_level() {
        let mut agc = CwAgc::new();
        for _ in 0..4_000 {
            let _ = agc.gain_for(0.4, 12_000.0, 0.25, 3.0, 120.0, AgcMode::DualLoop);
        }
        let g = agc.gain();
        assert!(g > 0.02);
        assert!(g <= 64.0);
    }

    #[test]
    fn hang_recovers_slower_than_envelope_after_peak() {
        let rate = 12_000.0;
        let target = 0.25;
        let attack = 3.0;
        let decay = 120.0;
        let mut env_agc = CwAgc::new();
        let mut hang_agc = CwAgc::new();

        for _ in 0..2_000 {
            let _ = env_agc.gain_for(0.8, rate, target, attack, decay, AgcMode::Envelope);
            let _ = hang_agc.gain_for(0.8, rate, target, attack, decay, AgcMode::Hang);
        }

        for _ in 0..6_000 {
            let _ = env_agc.gain_for(0.02, rate, target, attack, decay, AgcMode::Envelope);
            let _ = hang_agc.gain_for(0.02, rate, target, attack, decay, AgcMode::Hang);
        }

        assert!(
            env_agc.gain() > hang_agc.gain() * 1.05,
            "envelope should raise gain faster in silence (more noise pump): env={} hang={}",
            env_agc.gain(),
            hang_agc.gain()
        );
    }

    #[test]
    fn zero_sample_rate_returns_unity() {
        let mut agc = CwAgc::new();
        assert_eq!(agc.gain_for(1.0, 0.0, 0.25, 3.0, 120.0, AgcMode::Envelope), 1.0);
    }

    #[test]
    fn track_envelope_without_changing_gain() {
        let mut agc = CwAgc::new();
        for _ in 0..500 {
            agc.track_envelope(0.4, 12_000.0, 3.0, 120.0, AgcMode::Envelope);
        }
        assert!(agc.envelope() > 0.1);
        assert_eq!(agc.gain(), 1.0);
    }

    #[test]
    fn reset_restores_defaults() {
        let mut agc = CwAgc::new();
        let _ = agc.gain_for(2.0, 12_000.0, 0.25, 3.0, 120.0, AgcMode::Envelope);
        agc.reset_state();
        assert_eq!(agc.gain, 1.0);
    }
}
