//! RF level helpers, S-unit conversion, and tuning hints.

/// Classic “half scale” target (~−6 dB of full swing).
pub const HALF_SCALE: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioLevelHint {
    Idle,
    TooQuiet,
    SweetSpot,
    TooHot,
}

pub fn classify_level(
    peak: f32,
    agc_enabled: bool,
    agc_gain: f32,
    agc_envelope: f32,
    agc_target: f32,
    streaming: bool,
) -> AudioLevelHint {
    if !streaming {
        return AudioLevelHint::Idle;
    }
    if !peak.is_finite()
        || !agc_gain.is_finite()
        || !agc_envelope.is_finite()
        || !agc_target.is_finite()
    {
        return AudioLevelHint::Idle;
    }
    if peak < 1e-5 && agc_envelope < 1e-5 {
        return AudioLevelHint::Idle;
    }
    let agc_starved = agc_enabled && agc_gain > 14.0;
    let agc_saturated = agc_enabled && agc_gain < 0.12;
    let rf_hot = agc_enabled && agc_envelope > agc_target * 2.5;
    if peak < 0.07 || agc_starved {
        return AudioLevelHint::TooQuiet;
    }
    if peak > 0.88 || agc_saturated || rf_hot {
        return AudioLevelHint::TooHot;
    }
    if peak >= 0.10 && peak <= 0.70 && !agc_starved && !agc_saturated {
        return AudioLevelHint::SweetSpot;
    }
    if peak > 0.70 {
        AudioLevelHint::TooHot
    } else {
        AudioLevelHint::SweetSpot
    }
}

/// Pre-AGC IQ magnitude that maps to S9 (−73 dBm). Each ×10 in IQ amplitude is +20 dB.
const SMETER_IQ_S9: f32 = 0.1;

/// Pre-software-AGC IQ level mapped to an approximate dBm scale.
///
/// Calibrated so a healthy CW signal lands mid-scale and a ×10 change in IQ amplitude
/// (e.g. +20 dB of software RF gain) moves the needle by +20 dB (~3 S-units).
pub fn iq_rf_level_to_dbm(iq_rf_level: f32) -> f32 {
    let level = if iq_rf_level.is_finite() {
        iq_rf_level
    } else {
        0.0
    };
    let ratio = level.max(1e-9) / SMETER_IQ_S9;
    (-73.0 + 20.0 * ratio.log10()).clamp(SMETER_DBM_MIN, SMETER_DBM_MAX)
}

/// RF level for the S-meter needle.
///
/// Driven by the pre-AGC IQ tap so it behaves identically on every source and tracks the
/// software RF gain even when hardware/RF AGC is on. Hardware RSSI (`_rssi_dbm`) is shown
/// separately as a reference in the meter and is intentionally not blended in here.
pub fn rf_level_dbm(_rssi_dbm: Option<f32>, iq_rf_level: f32) -> f32 {
    iq_rf_level_to_dbm(iq_rf_level)
}

/// Map dBm to classic S-unit readout (S1..S9, S9+n).
pub fn dbm_to_s_reading(dbm: f32) -> String {
    if dbm >= -73.0 {
        let over = ((dbm + 73.0) / 6.0).round().max(0.0) as i32;
        if over == 0 {
            "S9".to_string()
        } else {
            format!("S9+{over}")
        }
    } else {
        let s = ((dbm + 127.0) / 6.0).ceil().clamp(1.0, 9.0) as i32;
        format!("S{s}")
    }
}


pub(crate) const SMETER_DBM_MIN: f32 = -127.0;
pub(crate) const SMETER_DBM_MAX: f32 = -33.0;

pub(crate) fn dbm_to_needle_t(dbm: f32) -> f32 {
    ((dbm - SMETER_DBM_MIN) / (SMETER_DBM_MAX - SMETER_DBM_MIN)).clamp(0.0, 1.0)
}

pub(crate) fn needle_angle(t: f32) -> f32 {
    std::f32::consts::PI * (1.0 - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_level_idle_when_not_streaming() {
        assert_eq!(
            classify_level(0.5, true, 1.0, 0.5, 0.25, false),
            AudioLevelHint::Idle
        );
    }

    #[test]
    fn classify_level_sweet_spot_mid_peak() {
        assert_eq!(
            classify_level(0.35, true, 2.0, 0.2, 0.25, true),
            AudioLevelHint::SweetSpot
        );
    }

    #[test]
    fn classify_level_too_hot_when_clipping() {
        assert_eq!(
            classify_level(0.95, true, 1.0, 0.5, 0.25, true),
            AudioLevelHint::TooHot
        );
    }

    #[test]
    fn classify_level_too_quiet_when_agc_starved() {
        assert_eq!(
            classify_level(0.2, true, 20.0, 0.01, 0.25, true),
            AudioLevelHint::TooQuiet
        );
    }

    #[test]
    fn iq_rf_level_maps_decades_to_db() {
        let s9 = iq_rf_level_to_dbm(0.1);
        let ten_x = iq_rf_level_to_dbm(1.0);
        assert!((ten_x - s9 - 20.0).abs() < 0.5);
    }

    #[test]
    fn dbm_to_s_reading_classic_scale() {
        assert_eq!(dbm_to_s_reading(-73.0), "S9");
        assert_eq!(dbm_to_s_reading(-67.0), "S9+1");
        assert_eq!(dbm_to_s_reading(-97.0), "S5");
    }

    #[test]
    fn needle_position_monotonic() {
        let low = dbm_to_needle_t(-120.0);
        let mid = dbm_to_needle_t(-80.0);
        let high = dbm_to_needle_t(-40.0);
        assert!(low < mid);
        assert!(mid < high);
        assert!((needle_angle(0.0) - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn rf_level_ignores_hardware_rssi() {
        assert_eq!(rf_level_dbm(Some(-50.0), 0.05), iq_rf_level_to_dbm(0.05));
    }

    #[test]
    fn iq_rf_level_nan_maps_to_floor() {
        assert_eq!(iq_rf_level_to_dbm(f32::NAN), -127.0);
        assert!(iq_rf_level_to_dbm(f32::NAN).is_finite());
    }

    #[test]
    fn classify_level_idle_on_nan_inputs() {
        assert_eq!(
            classify_level(f32::NAN, true, 1.0, 0.2, 0.25, true),
            AudioLevelHint::Idle
        );
    }
}
