//! CW-oriented automatic gain control (fast attack, adjustable decay).
//!
//! Short attack so a strong dit does not blast through; a longer decay/hang so
//! the floor does not pump up between dits. When disabled the caller applies a
//! fixed manual gain instead — many contesters prefer that so a loud neighbour
//! cannot pump the wanted signal down.
//!
//! [`AgcMode::Lookahead`] scans a short forward window each block, smooths gain
//! toward the anticipated peak, and applies it without output delay so level
//! changes stay continuous (no gain steps → no clicks).

use super::settings::AgcMode;

/// Envelope-following AGC with configurable attack/decay.
#[derive(Clone, Debug)]
pub struct CwAgc {
    gain: f32,
    envelope: f32,
    fast_env: f32,
    slow_env: f32,
    lookahead_history: Vec<f32>,
    lookahead_scratch: Vec<f32>,
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
            lookahead_history: Vec::new(),
            lookahead_scratch: Vec::new(),
        }
    }

    pub fn reset_state(&mut self) {
        self.gain = 1.0;
        self.envelope = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.lookahead_history.clear();
        self.lookahead_scratch.clear();
    }

    fn attack_coeff(sample_rate: f32, attack_ms: f32) -> f32 {
        (-1.0 / (sample_rate * (attack_ms.max(0.1) / 1000.0))).exp()
    }

    fn decay_coeff(sample_rate: f32, decay_ms: f32) -> f32 {
        (-1.0 / (sample_rate * (decay_ms.max(1.0) / 1000.0))).exp()
    }

    fn lookahead_delay_samples(sample_rate: f32, lookahead_ms: f32) -> usize {
        if sample_rate <= 0.0 {
            return 1;
        }
        (sample_rate * lookahead_ms.clamp(0.5, 40.0) / 1000.0)
            .round()
            .max(1.0) as usize
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
        let attack = Self::attack_coeff(sample_rate, attack_ms);
        let decay = Self::decay_coeff(sample_rate, decay_ms);

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

    fn smooth_gain_toward(
        &mut self,
        desired: f32,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
    ) {
        let attack = Self::attack_coeff(sample_rate, attack_ms);
        let decay = Self::decay_coeff(sample_rate, decay_ms);
        if desired < self.gain {
            self.gain = attack * self.gain + (1.0 - attack) * desired;
        } else {
            self.gain = decay * self.gain + (1.0 - decay) * desired;
        }
        self.gain = self.gain.clamp(0.02, 64.0);
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
        let track_mode = if mode == AgcMode::Lookahead {
            AgcMode::Envelope
        } else {
            mode
        };
        self.update_envelope(level, sample_rate, attack_ms, decay_ms, track_mode);
    }

    /// Block lookahead AGC: scan forward peaks, ramp gain smoothly, one gain per level.
    pub fn compute_lookahead_gains(
        &mut self,
        levels: &[f32],
        gains: &mut [f32],
        sample_rate: f32,
        target: f32,
        attack_ms: f32,
        decay_ms: f32,
        lookahead_ms: f32,
    ) {
        assert_eq!(levels.len(), gains.len());
        if sample_rate <= 0.0 || levels.is_empty() {
            gains.fill(1.0);
            return;
        }

        let delay = Self::lookahead_delay_samples(sample_rate, lookahead_ms);
        self.lookahead_scratch.clear();
        self.lookahead_scratch
            .extend_from_slice(&self.lookahead_history);
        self.lookahead_scratch.extend_from_slice(levels);
        let hist_len = self.lookahead_history.len();

        for (i, gain_out) in gains.iter_mut().enumerate() {
            let pos = hist_len + i;
            let end = (pos + delay).min(self.lookahead_scratch.len() - 1);
            let peak = self.lookahead_scratch[pos..=end]
                .iter()
                .copied()
                .fold(0.0f32, f32::max)
                .max(1e-7);

            self.update_envelope(peak, sample_rate, attack_ms, decay_ms, AgcMode::Envelope);
            let desired = target / self.envelope.max(1e-7);
            self.smooth_gain_toward(desired, sample_rate, attack_ms, decay_ms);
            *gain_out = self.gain;
        }

        self.lookahead_history.clear();
        let ext_len = self.lookahead_scratch.len();
        let start = ext_len.saturating_sub(delay);
        self.lookahead_history
            .extend_from_slice(&self.lookahead_scratch[start..]);
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
        if mode == AgcMode::Lookahead {
            return self.gain;
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
            AgcMode::Lookahead => self.gain,
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

    #[test]
    fn lookahead_ducks_before_peak() {
        let rate = 12_000.0;
        let target = 0.25;
        let attack = 3.0;
        let decay = 120.0;
        let lookahead = 8.0;
        let delay = CwAgc::lookahead_delay_samples(rate, lookahead);

        let mut quiet = vec![0.05f32; delay * 2];
        let mut spike = vec![0.05f32; delay];
        spike.extend(std::iter::repeat_n(0.9f32, delay * 2));
        quiet.extend(spike.iter().copied());

        let mut env_agc = CwAgc::new();
        let mut la_agc = CwAgc::new();
        let mut env_gains = vec![0.0f32; quiet.len()];
        let mut la_gains = vec![0.0f32; quiet.len()];

        for (i, &level) in quiet.iter().enumerate() {
            env_gains[i] = env_agc.gain_for(level, rate, target, attack, decay, AgcMode::Envelope);
        }
        la_agc.compute_lookahead_gains(
            &quiet,
            &mut la_gains,
            rate,
            target,
            attack,
            decay,
            lookahead,
        );

        let spike_start = delay * 2;
        let pre = spike_start.saturating_sub(delay / 2);
        let la_pre = la_gains[pre..spike_start]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        let env_at_spike = env_gains[spike_start];
        assert!(
            la_pre < env_at_spike * 0.95,
            "lookahead should reduce gain before spike: la_pre={la_pre} env_at_spike={env_at_spike}"
        );
    }

    #[test]
    fn lookahead_gains_are_continuous() {
        let rate = 12_000.0;
        let mut levels = vec![0.08f32; 500];
        for slot in levels.iter_mut().skip(200).take(100) {
            *slot = 0.55;
        }
        let mut gains = vec![0.0f32; levels.len()];
        let mut agc = CwAgc::new();
        agc.compute_lookahead_gains(&levels, &mut gains, rate, 0.25, 3.0, 120.0, 8.0);
        for pair in gains.windows(2) {
            let step = (pair[1] - pair[0]).abs();
            assert!(step < 0.08, "gain step too sharp: {step}");
        }
    }

    #[test]
    fn lookahead_carries_history_across_blocks() {
        let rate = 12_000.0;
        let mut agc = CwAgc::new();
        let block_a = vec![0.05f32; 240];
        let mut gains_a = vec![0.0f32; block_a.len()];
        agc.compute_lookahead_gains(&block_a, &mut gains_a, rate, 0.25, 3.0, 120.0, 8.0);
        assert!(!agc.lookahead_history.is_empty());

        let mut block_b = vec![0.05f32; 240];
        for slot in block_b.iter_mut().take(48) {
            *slot = 0.85;
        }
        let mut gains_b = vec![0.0f32; block_b.len()];
        agc.compute_lookahead_gains(&block_b, &mut gains_b, rate, 0.25, 3.0, 120.0, 8.0);
        let min_early = gains_b[..24]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_early < gains_a.last().copied().unwrap_or(1.0) * 0.9,
            "history should allow pre-duck into next block"
        );
    }
}
