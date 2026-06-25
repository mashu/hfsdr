//! RF level helpers, S-unit conversion, and tuning hints.

pub const SCOPE_LEN: usize = 320;
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
    let ratio = iq_rf_level.max(1e-9) / SMETER_IQ_S9;
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
