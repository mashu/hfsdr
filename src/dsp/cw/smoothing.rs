//! Rate-aware exponential smoothing helpers.
//!
//! DSP stages express their ballistics as time constants (seconds) and convert
//! to per-sample coefficients here, so behavior does not change with the audio
//! or IQ sample rate. Callers cache the result and recompute only when the rate
//! changes — `exp()` stays out of per-sample loops.

/// One-pole smoothing factor for time constant `tau_s` at `sample_rate`:
/// `y += alpha * (x - y)` converges with an e-folding time of `tau_s`.
#[inline]
pub fn alpha_for_tau(sample_rate: f32, tau_s: f32) -> f32 {
    if sample_rate <= 0.0 || tau_s <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (sample_rate * tau_s)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_tracks_time_constant_across_rates() {
        // Feeding a unit step for exactly tau seconds converges to ~1 - 1/e
        // regardless of the sample rate.
        for rate in [8_000.0f32, 12_000.0, 48_000.0, 768_000.0] {
            let tau = 0.05;
            let alpha = alpha_for_tau(rate, tau);
            let mut y = 0.0f32;
            for _ in 0..(rate * tau) as usize {
                y += alpha * (1.0 - y);
            }
            assert!(
                (y - 0.632).abs() < 0.01,
                "rate {rate}: step response after tau was {y}"
            );
        }
    }

    #[test]
    fn degenerate_inputs_snap_immediately() {
        assert_eq!(alpha_for_tau(0.0, 0.1), 1.0);
        assert_eq!(alpha_for_tau(12_000.0, 0.0), 1.0);
    }
}
