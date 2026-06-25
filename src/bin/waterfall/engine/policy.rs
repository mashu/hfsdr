//! Pure pump / wideband policy (unit-tested, no I/O).

use std::time::Duration;

pub const WIDEBAND_IQ_THRESHOLD: f32 = 96_000.0;
pub const MAX_DRAIN_NARROW: usize = 1 << 16;
pub const MAX_DRAIN_WIDEBAND: usize = 1 << 16;
pub const MAX_SPECTRUM_ROWS_PER_PUMP: usize = 4;
pub const MIN_SPECTRUM_ROWS_WIDEBAND: usize = 2;
pub const MAX_SPECTRUM_ROWS_WIDEBAND: usize = 8;
pub const MAX_CATCHUP_PUMPS: usize = 8;
pub const MAX_CATCHUP_PUMPS_LIGHT: usize = 2;
pub const MAX_AUDIO_SAMPLES_WB: usize = 8192;
pub const MAX_FFT_INPUT_WB: usize = 20_480;
pub const SKIMMER_PEAK_HOLD_DECAY_DB: f32 = 0.25;
pub const RING_CATCHUP_FILL: f32 = 0.55;
pub const RING_CATCHUP_TARGET: f32 = 0.25;
pub const STALL_TIMEOUT_KIWI: Duration = Duration::from_secs(20);
pub const STALL_TIMEOUT_LOCAL: Duration = Duration::from_secs(12);
pub const KIWI_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(45);
pub const SLOW_FRACTION: f32 = 0.7;
pub const SLOW_HOLD: Duration = Duration::from_secs(5);

/// When the IQ ring is over-filled, drop down to this slot count (unless recording).
pub fn ring_catchup_target_slots(slots: usize, cap: usize, recording: bool) -> Option<usize> {
    if recording || cap == 0 {
        return None;
    }
    let fill = slots as f32 / cap as f32;
    if fill >= RING_CATCHUP_FILL {
        Some((cap as f32 * RING_CATCHUP_TARGET) as usize)
    } else {
        None
    }
}

pub fn is_wideband_rate(rate: f32) -> bool {
    rate > WIDEBAND_IQ_THRESHOLD
}

pub fn max_drain_for(sample_rate: f32) -> usize {
    if sample_rate > WIDEBAND_IQ_THRESHOLD {
        MAX_DRAIN_WIDEBAND
    } else if sample_rate > 48_000.0 {
        MAX_DRAIN_NARROW
    } else {
        1 << 15
    }
}

pub fn max_fft_input_for(sample_rate: f32, spectrum_hop: usize, fft_size: usize) -> usize {
    if sample_rate > WIDEBAND_IQ_THRESHOLD {
        (spectrum_hop * MAX_SPECTRUM_ROWS_WIDEBAND + fft_size).min(MAX_FFT_INPUT_WB)
    } else {
        usize::MAX
    }
}

pub fn wideband_tail_len(sample_len: usize, rate: f32, max: usize) -> usize {
    if rate <= WIDEBAND_IQ_THRESHOLD || sample_len <= max {
        sample_len
    } else {
        max
    }
}

pub fn demod_tail_max(rate: f32) -> usize {
    if rate > WIDEBAND_IQ_THRESHOLD {
        MAX_AUDIO_SAMPLES_WB
    } else {
        usize::MAX
    }
}

pub fn adaptive_spectrum_rows(
    device_rate: f32,
    cached_rate: f32,
    iq_buffer_fill: f32,
) -> usize {
    if device_rate <= WIDEBAND_IQ_THRESHOLD {
        return MAX_SPECTRUM_ROWS_PER_PUMP;
    }
    let nominal = device_rate.max(1.0);
    let sps_ratio = (cached_rate / nominal).clamp(0.0, 1.25);
    let ring_headroom = 1.0 - iq_buffer_fill.clamp(0.0, 1.0);
    let score = (sps_ratio * 0.55 + ring_headroom * 0.45).clamp(0.0, 1.0);
    if score > 0.85 {
        MAX_SPECTRUM_ROWS_WIDEBAND
    } else if score > 0.65 {
        6
    } else if score > 0.45 {
        4
    } else {
        MIN_SPECTRUM_ROWS_WIDEBAND
    }
}

pub fn skimmer_throttle(is_kiwi: bool, skimmer_iq_rate: f32) -> u64 {
    if is_kiwi && skimmer_iq_rate <= 24_000.0 {
        2
    } else if skimmer_iq_rate > 96_000.0 {
        4
    } else if skimmer_iq_rate > 48_000.0 {
        2
    } else {
        1
    }
}

pub fn handshake_timeout(is_kiwi: bool) -> Duration {
    if is_kiwi {
        KIWI_HANDSHAKE_TIMEOUT
    } else {
        STALL_TIMEOUT_LOCAL
    }
}

pub fn stall_timeout(is_kiwi: bool) -> Duration {
    if is_kiwi {
        STALL_TIMEOUT_KIWI
    } else {
        STALL_TIMEOUT_LOCAL
    }
}

pub fn catchup_pumps_max(
    ring_fill: f32,
    iq_recording: bool,
    full_drain_spectrum: bool,
) -> usize {
    if iq_recording {
        if ring_fill > 0.2 {
            MAX_CATCHUP_PUMPS * 4
        } else {
            MAX_CATCHUP_PUMPS
        }
    } else if ring_fill > 0.35 {
        if full_drain_spectrum {
            MAX_CATCHUP_PUMPS + 4
        } else {
            MAX_CATCHUP_PUMPS
        }
    } else {
        MAX_CATCHUP_PUMPS_LIGHT
    }
}

pub fn slow_link(effective: f32, nominal: f32, slow_since_secs: Option<f32>) -> bool {
    if effective >= SLOW_FRACTION * nominal {
        return false;
    }
    slow_since_secs.is_some_and(|s| s >= SLOW_HOLD.as_secs_f32())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_catchup_only_when_full_and_not_recording() {
        let cap = 1000;
        assert!(ring_catchup_target_slots(400, cap, false).is_none());
        assert_eq!(ring_catchup_target_slots(600, cap, false), Some(250));
        assert!(ring_catchup_target_slots(600, cap, true).is_none());
    }

    #[test]
    fn max_drain_wide_vs_narrow() {
        assert_eq!(max_drain_for(12_000.0), 1 << 15);
        assert_eq!(max_drain_for(96_000.0), MAX_DRAIN_NARROW);
        assert_eq!(max_drain_for(384_000.0), MAX_DRAIN_WIDEBAND);
    }

    #[test]
    fn adaptive_rows_narrow_is_fixed() {
        assert_eq!(adaptive_spectrum_rows(12_000.0, 12_000.0, 0.5), 4);
    }

    #[test]
    fn adaptive_rows_wideband_scales_with_headroom() {
        assert_eq!(
            adaptive_spectrum_rows(384_000.0, 380_000.0, 0.1),
            MAX_SPECTRUM_ROWS_WIDEBAND
        );
        assert_eq!(adaptive_spectrum_rows(384_000.0, 100_000.0, 0.9), 2);
    }

    #[test]
    fn skimmer_throttle_matrix() {
        assert_eq!(skimmer_throttle(true, 12_000.0), 2);
        assert_eq!(skimmer_throttle(false, 384_000.0), 4);
        assert_eq!(skimmer_throttle(false, 49_000.0), 2);
        assert_eq!(skimmer_throttle(false, 12_000.0), 1);
    }

    #[test]
    fn stall_timeouts() {
        assert_eq!(stall_timeout(true), STALL_TIMEOUT_KIWI);
        assert_eq!(stall_timeout(false), STALL_TIMEOUT_LOCAL);
        assert_eq!(handshake_timeout(true), KIWI_HANDSHAKE_TIMEOUT);
    }

    #[test]
    fn catchup_pumps_recording_boost() {
        assert_eq!(catchup_pumps_max(0.5, true, false), MAX_CATCHUP_PUMPS * 4);
        assert_eq!(catchup_pumps_max(0.1, true, false), MAX_CATCHUP_PUMPS);
    }

    #[test]
    fn wideband_tail_len_cases() {
        assert_eq!(super::wideband_tail_len(100, 384_000.0, 50), 50);
        assert_eq!(super::wideband_tail_len(100, 12_000.0, 50), 100);
    }
}
