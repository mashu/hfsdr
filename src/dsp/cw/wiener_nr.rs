//! Pre-demod Wiener-style gain on complex IQ.
//!
//! Tracks a slow noise-floor estimate and applies a smoothed Wiener gain so
//! broadband hiss is attenuated without pumping on every CW key-up.

use crate::source::Complex32;

/// Adaptive IQ noise suppressor with envelope-hung gain (CW-safe).
#[derive(Clone, Debug)]
pub struct IqWienerNr {
    sample_rate: f32,
    noise_var: f32,
    envelope: f32,
    gain: f32,
}

impl Default for IqWienerNr {
    fn default() -> Self {
        Self::new()
    }
}

impl IqWienerNr {
    pub fn new() -> Self {
        Self {
            sample_rate: 12_000.0,
            noise_var: 1e-6,
            envelope: 0.0,
            gain: 1.0,
        }
    }

    pub fn reset_state(&mut self) {
        let rate = self.sample_rate;
        *self = Self::new();
        self.sample_rate = rate;
    }

    fn sync_rate(&mut self, sample_rate: f32) {
        let rate = sample_rate.max(1.0);
        if (rate - self.sample_rate).abs() > 1.0 {
            self.sample_rate = rate;
            self.envelope = 0.0;
            self.gain = 1.0;
        }
    }

    fn exp_alpha(dt: f32, tau_s: f32) -> f32 {
        1.0 - (-dt / tau_s.max(1e-4)).exp()
    }

    /// `level` in 0..1 scales suppression strength.
    pub fn process(&mut self, sample: Complex32, sample_rate: f32, level: f32) -> Complex32 {
        self.sync_rate(sample_rate);
        let level = level.clamp(0.0, 1.0);
        if level <= 0.0 {
            return sample;
        }

        let dt = 1.0 / self.sample_rate;
        let inst = sample.norm();

        // Envelope: fast attack, slow release — hang through dit/dash spaces.
        const ENV_ATTACK_S: f32 = 0.014;
        const ENV_RELEASE_S: f32 = 0.38;
        let env_alpha = if inst > self.envelope {
            Self::exp_alpha(dt, ENV_ATTACK_S)
        } else {
            Self::exp_alpha(dt, ENV_RELEASE_S)
        };
        self.envelope += env_alpha * (inst - self.envelope);

        let power = self.envelope.max(1e-9).powi(2);
        let noise_rms = self.noise_var.sqrt();
        if self.envelope < noise_rms * 2.2 + 1e-6 {
            const NOISE_TAU_S: f32 = 1.0;
            let na = Self::exp_alpha(dt, NOISE_TAU_S);
            self.noise_var += na * (power - self.noise_var);
        }
        self.noise_var = self.noise_var.max(1e-12);

        let snr = (power / self.noise_var - 1.0).max(0.0);
        let knee = 0.12 + level * 0.35;
        let wiener = (snr / (snr + knee)).sqrt();
        let target_gain = 1.0 - level + level * wiener;

        // Smooth the applied gain — slow release prevents keyed dropouts.
        const GAIN_ATTACK_S: f32 = 0.025;
        const GAIN_RELEASE_S: f32 = 0.50;
        let g_alpha = if target_gain > self.gain {
            Self::exp_alpha(dt, GAIN_ATTACK_S)
        } else {
            Self::exp_alpha(dt, GAIN_RELEASE_S)
        };
        self.gain += g_alpha * (target_gain - self.gain);
        self.gain = self.gain.clamp(0.05, 1.0);

        Complex32::new(sample.re * self.gain, sample.im * self.gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyed_burst(nr: &mut IqWienerNr, rate: f32, level: f32, mark: usize, space: usize, rounds: usize) {
        for _ in 0..rounds {
            for _ in 0..mark {
                let _ = nr.process(Complex32::new(0.22, 0.0), rate, level);
            }
            for _ in 0..space {
                let _ = nr.process(Complex32::new(0.012, -0.009), rate, level);
            }
        }
    }

    #[test]
    fn attenuates_noise_more_than_tone() {
        let mut nr = IqWienerNr::new();
        let rate = 12_000.0;
        let mut tone_out = 0.0f32;
        let mut noise_out = 0.0f32;
        for i in 0..rate as usize * 4 {
            let t = i as f32 / rate;
            let tone = Complex32::new((std::f32::consts::TAU * 0.0 * t).cos() * 0.2, 0.0);
            tone_out += nr.process(tone, rate, 0.5).norm_sqr();
            let noise = Complex32::new(0.01, -0.008);
            noise_out += nr.process(noise, rate, 0.5).norm_sqr();
        }
        assert!(tone_out > noise_out * 1.5);
    }

    #[test]
    fn zero_level_passes_through() {
        let mut nr = IqWienerNr::new();
        let s = Complex32::new(0.3, -0.2);
        let o = nr.process(s, 12_000.0, 0.0);
        assert!((o.re - s.re).abs() < 1e-6);
        assert!((o.im - s.im).abs() < 1e-6);
    }

    #[test]
    fn keyed_cw_does_not_pump_gain_to_zero() {
        let mut nr = IqWienerNr::new();
        let rate = 12_000.0;
        let mark = (0.05 * rate) as usize;
        let space = mark;
        keyed_burst(&mut nr, rate, 0.5, mark, space, 8);
        let gain_mid_key = nr.gain;
        keyed_burst(&mut nr, rate, 0.5, mark, space, 2);
        assert!(
            nr.gain > 0.25,
            "gain should hang through keying gaps, got {}",
            nr.gain
        );
        assert!(
            gain_mid_key > 0.2,
            "gain should stay open on keyed carrier, got {gain_mid_key}"
        );
    }

    #[test]
    fn mark_output_stays_continuous_across_gap() {
        let mut nr = IqWienerNr::new();
        let rate = 12_000.0;
        let mark = (0.06 * rate) as usize;
        let space = mark / 2;
        for _ in 0..mark {
            let _ = nr.process(Complex32::new(0.2, 0.0), rate, 0.45);
        }
        let out_before_gap = nr.process(Complex32::new(0.2, 0.0), rate, 0.45).norm();
        for _ in 0..space {
            let _ = nr.process(Complex32::new(0.01, 0.008), rate, 0.45);
        }
        let out_after_gap = nr.process(Complex32::new(0.2, 0.0), rate, 0.45).norm();
        assert!(
            out_after_gap > out_before_gap * 0.35,
            "mark level should not collapse after a short gap: before={out_before_gap} after={out_after_gap}"
        );
    }
}
