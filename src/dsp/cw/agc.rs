//! CW-oriented automatic gain control (fast attack, adjustable decay).
//!
//! Short attack so a strong dit does not blast through; a longer decay/hang so
//! the floor does not pump up between dits. When disabled the caller applies a
//! fixed manual gain instead — many contesters prefer that so a loud neighbour
//! cannot pump the wanted signal down.
//!
//! [`AgcMode::Lookahead`] delays the signal by the lookahead window: each gain
//! is computed from the *full* forward window (including across block
//! boundaries) and applied to the sample from `lookahead_ms` ago, so peaks are
//! always pre-ducked and level changes stay continuous (no gain steps → no
//! clicks). The caller keeps the matching sample delay line
//! ([`Self::lookahead_delay_samples`] samples).

use super::settings::AgcMode;
use super::smoothing::alpha_for_tau;

/// Gain-smoothing time constants (seconds) per AGC mode — rate-invariant.
const ENVELOPE_GAIN_TAU_S: f32 = 8.3e-4;
const HANG_DOWN_TAU_S: f32 = 5.6e-4;
const DUAL_GAIN_TAU_S: f32 = 6.9e-4;

/// Cached per-sample coefficients (exp() stays out of the sample loop).
#[derive(Clone, Copy, Debug, Default)]
struct AgcCoeffs {
    rate: f32,
    attack_ms: f32,
    decay_ms: f32,
    attack: f32,
    decay: f32,
    slow_attack: f32,
    slow_decay: f32,
    hang: f32,
    env_gain_alpha: f32,
    hang_down_alpha: f32,
    dual_gain_alpha: f32,
}

impl AgcCoeffs {
    fn sync(&mut self, sample_rate: f32, attack_ms: f32, decay_ms: f32) {
        if (sample_rate - self.rate).abs() <= 1.0
            && (attack_ms - self.attack_ms).abs() <= 1e-3
            && (decay_ms - self.decay_ms).abs() <= 1e-3
        {
            return;
        }
        let keep = |ms: f32| (-1.0 / (sample_rate * (ms / 1000.0))).exp();
        self.attack = keep(attack_ms.max(0.1));
        self.decay = keep(decay_ms.max(1.0));
        self.slow_attack = keep(attack_ms.max(0.1) * 10.0);
        self.slow_decay = keep(decay_ms.max(1.0) * 8.0);
        self.hang = keep(decay_ms.max(1.0) * 4.0);
        self.env_gain_alpha = alpha_for_tau(sample_rate, ENVELOPE_GAIN_TAU_S);
        self.hang_down_alpha = alpha_for_tau(sample_rate, HANG_DOWN_TAU_S);
        self.dual_gain_alpha = alpha_for_tau(sample_rate, DUAL_GAIN_TAU_S);
        self.rate = sample_rate;
        self.attack_ms = attack_ms;
        self.decay_ms = decay_ms;
    }
}

/// Envelope-following AGC with configurable attack/decay.
#[derive(Clone, Debug)]
pub struct CwAgc {
    gain: f32,
    envelope: f32,
    fast_env: f32,
    slow_env: f32,
    coeffs: AgcCoeffs,
    lookahead_history: Vec<f32>,
    lookahead_scratch: Vec<f32>,
    lookahead_delay: usize,
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
            coeffs: AgcCoeffs::default(),
            lookahead_history: Vec::new(),
            lookahead_scratch: Vec::new(),
            lookahead_delay: 0,
        }
    }

    pub fn reset_state(&mut self) {
        self.gain = 1.0;
        self.envelope = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.lookahead_history.clear();
        self.lookahead_scratch.clear();
        self.lookahead_delay = 0;
    }

    /// Sample delay used by [`AgcMode::Lookahead`] — the caller delays the
    /// signal path by this many samples so gains land on the right samples.
    pub fn lookahead_delay_samples(sample_rate: f32, lookahead_ms: f32) -> usize {
        if sample_rate <= 0.0 {
            return 1;
        }
        (sample_rate * lookahead_ms.clamp(0.5, 40.0) / 1000.0)
            .round()
            .max(1.0) as usize
    }

    fn update_envelope(&mut self, level: f32, mode: AgcMode) {
        let attack = self.coeffs.attack;
        let decay = self.coeffs.decay;
        match mode {
            AgcMode::DualLoop => {
                let slow_attack = self.coeffs.slow_attack;
                let slow_decay = self.coeffs.slow_decay;
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
                self.envelope = self.fast_env.max(self.slow_env * 0.55);
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

    fn smooth_gain_toward(&mut self, desired: f32) {
        let attack = self.coeffs.attack;
        let decay = self.coeffs.decay;
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
        if sample_rate <= 0.0 {
            return;
        }
        self.coeffs.sync(sample_rate, attack_ms, decay_ms);
        let track_mode = if mode == AgcMode::Lookahead {
            AgcMode::Envelope
        } else {
            mode
        };
        self.update_envelope(level, track_mode);
    }

    /// Block lookahead AGC with delayed application.
    ///
    /// `gains[i]` is the gain for the sample `delay` positions before
    /// `levels[i]` in the input stream (the caller's delay line provides it),
    /// so every gain sees its complete forward window — including windows that
    /// span block boundaries.
    #[allow(clippy::too_many_arguments)]
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
        self.coeffs.sync(sample_rate, attack_ms, decay_ms);

        let delay = Self::lookahead_delay_samples(sample_rate, lookahead_ms);
        if delay != self.lookahead_delay {
            self.lookahead_history.clear();
            self.lookahead_history.resize(delay, 0.0);
            self.lookahead_delay = delay;
        }

        self.lookahead_scratch.clear();
        self.lookahead_scratch
            .extend_from_slice(&self.lookahead_history);
        self.lookahead_scratch.extend_from_slice(levels);

        for (i, gain_out) in gains.iter_mut().enumerate() {
            // Window [i, i + delay] over history+levels — always complete.
            let peak = self.lookahead_scratch[i..=i + delay]
                .iter()
                .copied()
                .fold(0.0f32, f32::max)
                .max(1e-7);

            self.update_envelope(peak, AgcMode::Envelope);
            let desired = target / self.envelope.max(1e-7);
            self.smooth_gain_toward(desired);
            *gain_out = self.gain;
        }

        let ext_len = self.lookahead_scratch.len();
        self.lookahead_history.clear();
        self.lookahead_history
            .extend_from_slice(&self.lookahead_scratch[ext_len - delay..]);
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
        self.coeffs.sync(sample_rate, attack_ms, decay_ms);
        self.update_envelope(level, mode);

        let desired = target / self.envelope.max(1e-7);

        self.gain = match mode {
            AgcMode::Envelope => {
                let a = self.coeffs.env_gain_alpha;
                (1.0 - a) * self.gain + a * desired
            }
            AgcMode::Hang => {
                if desired < self.gain {
                    let a = self.coeffs.hang_down_alpha;
                    (1.0 - a) * self.gain + a * desired
                } else {
                    let hang = self.coeffs.hang;
                    hang * self.gain + (1.0 - hang) * desired
                }
            }
            AgcMode::DualLoop => {
                let a = self.coeffs.dual_gain_alpha;
                (1.0 - a) * self.gain + a * desired
            }
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

        // Quiet, then a spike; gains[i] applies to the delayed sample i - delay.
        let spike_start = delay * 3;
        let mut levels = vec![0.05f32; spike_start];
        levels.extend(std::iter::repeat_n(0.9f32, delay * 2));

        let mut agc = CwAgc::new();
        let mut gains = vec![0.0f32; levels.len()];
        agc.compute_lookahead_gains(&levels, &mut gains, rate, target, attack, decay, lookahead);

        // Output timeline: the spike is emitted at gains index spike_start + delay.
        // Gains just before that must already dip (window saw the spike coming).
        let baseline = gains[delay];
        let pre_spike = gains[spike_start..spike_start + delay]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        assert!(
            pre_spike < baseline * 0.95,
            "lookahead should duck before the delayed spike: pre={pre_spike} baseline={baseline}"
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

    #[test]
    fn lookahead_window_spans_block_boundary() {
        // A spike at the very start of block B must already duck the gains at
        // the end of block A — the flaw in the truncated-window version.
        let rate = 12_000.0;
        let lookahead = 8.0;
        let delay = CwAgc::lookahead_delay_samples(rate, lookahead);

        // One-shot reference: same levels in a single block.
        let block = 240usize;
        let mut all = vec![0.05f32; block];
        all.extend(std::iter::repeat_n(0.9f32, block));
        let mut ref_agc = CwAgc::new();
        let mut ref_gains = vec![0.0f32; all.len()];
        ref_agc.compute_lookahead_gains(&all, &mut ref_gains, rate, 0.25, 3.0, 120.0, lookahead);

        // Split at the block boundary right before the spike.
        let mut split_agc = CwAgc::new();
        let mut gains_a = vec![0.0f32; block];
        split_agc.compute_lookahead_gains(
            &all[..block],
            &mut gains_a,
            rate,
            0.25,
            3.0,
            120.0,
            lookahead,
        );
        let mut gains_b = vec![0.0f32; block];
        split_agc.compute_lookahead_gains(
            &all[block..],
            &mut gains_b,
            rate,
            0.25,
            3.0,
            120.0,
            lookahead,
        );

        let split_gains: Vec<f32> = gains_a.into_iter().chain(gains_b).collect();
        let mut max_err = 0.0f32;
        for (i, (a, b)) in ref_gains.iter().zip(split_gains.iter()).enumerate() {
            let e = (a - b).abs();
            if e > max_err {
                max_err = e;
            }
            let _ = i;
        }
        assert!(
            max_err < 1e-5,
            "split-block lookahead gains must match one-shot (err={max_err}, delay={delay})"
        );
    }
}
