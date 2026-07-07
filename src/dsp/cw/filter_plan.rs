//! Channel FIR planning: passband limits, tap budget, and group-delay tiers.
//!
//! Centralizes every knob that maps GUI passband width to sinc cutoff, tap count,
//! and maximum group delay. UI and DSP both read these constants so limits stay aligned.

/// Minimum GUI passband width (Hz).
pub const CHANNEL_PASSBAND_MIN_HZ: f32 = 25.0;

/// Maximum GUI passband width in wide mode (Hz).
pub const CHANNEL_PASSBAND_MAX_HZ: f32 = 2_000.0;

/// Maximum GUI passband width in contest narrow mode (Hz).
pub const CHANNEL_PASSBAND_NARROW_MAX_HZ: f32 = 500.0;

/// Default passband when no persisted setting exists (Hz).
pub const DEFAULT_CHANNEL_PASSBAND_HZ: f32 = 200.0;

/// Ctrl+scroll / keyboard step for passband width (Hz).
pub const PASSBAND_STEP_HZ: f32 = 25.0;

/// Default sinc cutoff as a fraction of GUI passband width (Hz).
pub const DEFAULT_PASSBAND_CUTOFF_FRAC: f32 = 0.34;

/// GUI range for adjustable passband cutoff fraction.
pub const MIN_PASSBAND_CUTOFF_FRAC: f32 = 0.20;
pub const MAX_PASSBAND_CUTOFF_FRAC: f32 = 0.50;

/// Floor on lowpass cutoff (Hz) for ultra-narrow passbands.
pub const MIN_PASSBAND_CUTOFF_HZ: f32 = 8.0;

/// Group-delay ceiling when [`super::settings::CwChannelSettings::deep_selectivity`] is on.
pub const DEEP_SELECTIVITY_MAX_GROUP_DELAY_MS: f32 = 50.0;

/// Dolph–Chebyshev sidelobe attenuation (dB) — GUI range.
pub const DEFAULT_DOLPH_SIDELOBE_DB: f32 = 60.0;
pub const MIN_DOLPH_SIDELOBE_DB: f32 = 40.0;
pub const MAX_DOLPH_SIDELOBE_DB: f32 = 80.0;

/// Oversampling factor when deriving tap count from sample_rate / cutoff.
pub const TAPS_PER_CUTOFF: f32 = 6.0;

pub const MIN_CHANNEL_FIR_TAPS: usize = 31;
pub const MAX_CHANNEL_FIR_TAPS: usize = 2047;

/// Group-delay ceiling for wide passbands (ms).
pub const DEFAULT_MAX_GROUP_DELAY_MS: f32 = 12.0;

/// Kaiser β defaults and clamp range (ignored for non-Kaiser windows).
pub const DEFAULT_KAISER_BETA: f32 = 6.0;
pub const MIN_KAISER_BETA: f32 = 2.0;
pub const MAX_KAISER_BETA: f32 = 14.0;

/// One row of [`GROUP_DELAY_BUDGETS`]: narrower passbands may use longer delay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupDelayBudget {
    pub max_passband_hz: f32,
    pub max_group_delay_ms: f32,
}

/// Narrower passbands trade keying smear for skirt rejection (sorted by `max_passband_hz`).
pub const GROUP_DELAY_BUDGETS: &[GroupDelayBudget] = &[
    GroupDelayBudget {
        max_passband_hz: 30.0,
        max_group_delay_ms: 50.0,
    },
    GroupDelayBudget {
        max_passband_hz: 50.0,
        max_group_delay_ms: 35.0,
    },
    GroupDelayBudget {
        max_passband_hz: 100.0,
        max_group_delay_ms: 28.0,
    },
    GroupDelayBudget {
        max_passband_hz: 200.0,
        max_group_delay_ms: 22.0,
    },
    GroupDelayBudget {
        max_passband_hz: 500.0,
        max_group_delay_ms: 16.0,
    },
];

/// Clamp a GUI passband width to [`CHANNEL_PASSBAND_MIN_HZ`], [`CHANNEL_PASSBAND_MAX_HZ`].
pub fn clamp_passband_hz(hz: f32) -> f32 {
    hz.clamp(CHANNEL_PASSBAND_MIN_HZ, CHANNEL_PASSBAND_MAX_HZ)
}

/// Lowpass cutoff (Hz) used when designing the channel FIR.
pub fn passband_cutoff_hz(bandwidth_hz: f32, cutoff_frac: f32) -> f32 {
    let frac = cutoff_frac.clamp(MIN_PASSBAND_CUTOFF_FRAC, MAX_PASSBAND_CUTOFF_FRAC);
    (bandwidth_hz * frac).max(MIN_PASSBAND_CUTOFF_HZ)
}

/// Maximum linear-phase group delay allowed for this passband width (ms).
pub fn max_group_delay_ms(bandwidth_hz: f32, deep_selectivity: bool) -> f32 {
    if deep_selectivity {
        return DEEP_SELECTIVITY_MAX_GROUP_DELAY_MS;
    }
    for tier in GROUP_DELAY_BUDGETS {
        if bandwidth_hz <= tier.max_passband_hz {
            return tier.max_group_delay_ms;
        }
    }
    DEFAULT_MAX_GROUP_DELAY_MS
}

/// Tap count for a channel filter (matches [`super::fir::design_lowpass_with`]).
pub fn plan_num_taps(
    sample_rate: f32,
    bandwidth_hz: f32,
    cutoff_frac: f32,
    deep_selectivity: bool,
) -> usize {
    let cutoff = passband_cutoff_hz(bandwidth_hz, cutoff_frac);
    let mut num_taps = ((sample_rate / cutoff) * TAPS_PER_CUTOFF).round() as usize;
    let delay_ms = max_group_delay_ms(bandwidth_hz, deep_selectivity);
    let max_taps_delay =
        ((sample_rate * delay_ms / 1000.0) * 2.0).round() as usize | 1;
    num_taps = num_taps
        .min(max_taps_delay)
        .clamp(MIN_CHANNEL_FIR_TAPS, MAX_CHANNEL_FIR_TAPS);
    if num_taps.is_multiple_of(2) {
        num_taps += 1;
    }
    num_taps
}

/// Linear-phase group delay of the channel FIR (~half the tap count).
pub fn channel_group_delay_ms(
    sample_rate: f32,
    bandwidth_hz: f32,
    cutoff_frac: f32,
    deep_selectivity: bool,
) -> f32 {
    if sample_rate <= 0.0 {
        return 0.0;
    }
    let n = plan_num_taps(sample_rate, bandwidth_hz, cutoff_frac, deep_selectivity) as f32;
    (n - 1.0) * 0.5 / sample_rate * 1000.0
}

/// PARIS-standard dit duration (seconds) at `wpm`.
pub fn dit_duration_s(wpm: f32) -> f32 {
    1.2 / wpm.clamp(5.0, 60.0)
}

/// Audio samples per dit at `wpm` (matched-filter / coherent integration length).
pub fn dit_samples(wpm: f32, sample_rate: f32) -> usize {
    (sample_rate * dit_duration_s(wpm)).round().max(1.0) as usize
}

/// Suggested channel passband (Hz) for a keying speed — passes dit sidebands without excess noise BW.
pub fn passband_hz_for_wpm(wpm: f32) -> f32 {
    let dit_s = dit_duration_s(wpm);
    (4.8 / dit_s).clamp(CHANNEL_PASSBAND_MIN_HZ, CHANNEL_PASSBAND_NARROW_MAX_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_passband_respects_limits() {
        assert_eq!(clamp_passband_hz(10.0), CHANNEL_PASSBAND_MIN_HZ);
        assert_eq!(clamp_passband_hz(25.0), CHANNEL_PASSBAND_MIN_HZ);
        assert_eq!(clamp_passband_hz(3_000.0), CHANNEL_PASSBAND_MAX_HZ);
    }

    #[test]
    fn group_delay_budget_tiers_are_monotonic() {
        let mut prev = 0.0f32;
        for tier in GROUP_DELAY_BUDGETS {
            assert!(tier.max_passband_hz > prev);
            prev = tier.max_passband_hz;
        }
        assert_eq!(
            max_group_delay_ms(CHANNEL_PASSBAND_MIN_HZ, false),
            GROUP_DELAY_BUDGETS[0].max_group_delay_ms
        );
        assert_eq!(
            max_group_delay_ms(CHANNEL_PASSBAND_MAX_HZ, false),
            DEFAULT_MAX_GROUP_DELAY_MS
        );
        assert_eq!(
            max_group_delay_ms(CHANNEL_PASSBAND_MAX_HZ, true),
            DEEP_SELECTIVITY_MAX_GROUP_DELAY_MS
        );
    }

    #[test]
    fn ultra_narrow_uses_longer_delay_budget() {
        let ms = channel_group_delay_ms(
            12_000.0,
            CHANNEL_PASSBAND_MIN_HZ,
            DEFAULT_PASSBAND_CUTOFF_FRAC,
            false,
        );
        assert!(ms > DEFAULT_MAX_GROUP_DELAY_MS);
        assert!(ms <= GROUP_DELAY_BUDGETS[0].max_group_delay_ms + 2.0);
        let wide_ms = channel_group_delay_ms(12_000.0, 500.0, DEFAULT_PASSBAND_CUTOFF_FRAC, false);
        assert!(wide_ms <= GROUP_DELAY_BUDGETS[4].max_group_delay_ms + 2.0);
        let deep_ms = channel_group_delay_ms(
            12_000.0,
            500.0,
            DEFAULT_PASSBAND_CUTOFF_FRAC,
            true,
        );
        assert!(deep_ms > wide_ms);
    }

    #[test]
    fn cutoff_scales_with_passband() {
        let narrow = passband_cutoff_hz(25.0, DEFAULT_PASSBAND_CUTOFF_FRAC);
        let wide = passband_cutoff_hz(200.0, DEFAULT_PASSBAND_CUTOFF_FRAC);
        assert!(narrow < wide);
        assert!((narrow / 25.0 - DEFAULT_PASSBAND_CUTOFF_FRAC).abs() < 1e-6);
    }

    #[test]
    fn passband_for_wpm_is_reasonable() {
        let bw20 = passband_hz_for_wpm(20.0);
        assert!((bw20 - 80.0).abs() < 15.0, "20 WPM bw={bw20}");
        let bw30 = passband_hz_for_wpm(30.0);
        assert!(bw30 > bw20);
    }
}
