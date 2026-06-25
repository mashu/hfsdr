//! Apply hardware controls through device extension traits.

#[cfg(feature = "airspy")]
use hfsdr::AirspyControls;
#[cfg(feature = "qmx")]
use hfsdr::QmxControls;
#[cfg(feature = "rtlsdr")]
use hfsdr::RtlSdrControls;

use hfsdr::KiwiControls;

use super::device::DeviceSource;

pub fn kiwi_set_rf_agc(source: &mut DeviceSource, on: bool) {
    if let DeviceSource::Kiwi(s) = source {
        let _ = KiwiControls::set_agc(s, on);
    }
}

pub fn kiwi_set_man_gain(source: &mut DeviceSource, gain: u8) {
    if let DeviceSource::Kiwi(s) = source {
        let _ = KiwiControls::set_man_gain(s, gain);
    }
}

pub fn kiwi_set_rf_attn_db(source: &mut DeviceSource, db: f32) {
    if let DeviceSource::Kiwi(s) = source {
        let _ = KiwiControls::set_rf_attn_db(s, db);
    }
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_att(source: &mut DeviceSource, step: u8) {
    if let DeviceSource::Airspy(s) = source {
        let _ = AirspyControls::set_hf_att(s, step);
    }
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_lna(source: &mut DeviceSource, on: bool) {
    if let DeviceSource::Airspy(s) = source {
        let _ = AirspyControls::set_hf_lna(s, on);
    }
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_agc_threshold(source: &mut DeviceSource, high: bool) {
    if let DeviceSource::Airspy(s) = source {
        let _ = AirspyControls::set_hf_agc_threshold(s, high);
    }
}

#[cfg(feature = "airspy")]
pub fn airspy_set_frontend_options(source: &mut DeviceSource, flags: u32) {
    if let DeviceSource::Airspy(s) = source {
        let _ = AirspyControls::set_frontend_options(s, flags);
    }
}

#[cfg(feature = "airspy")]
pub fn airspy_set_bias_tee(source: &mut DeviceSource, on: bool) {
    if let DeviceSource::Airspy(s) = source {
        let _ = AirspyControls::set_bias_tee(s, on);
    }
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_rtl_agc(source: &mut DeviceSource, on: bool) {
    if let DeviceSource::RtlSdr(s) = source {
        let _ = RtlSdrControls::set_agc(s, on);
    }
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_manual_gain(source: &mut DeviceSource, manual: bool) {
    if let DeviceSource::RtlSdr(s) = source {
        let _ = RtlSdrControls::set_tuner_gain_mode(s, manual);
    }
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_tuner_gain(source: &mut DeviceSource, gain_db10: i32) {
    if let DeviceSource::RtlSdr(s) = source {
        let _ = RtlSdrControls::set_tuner_gain(s, gain_db10);
    }
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_bias_tee(source: &mut DeviceSource, on: bool) {
    if let DeviceSource::RtlSdr(s) = source {
        let _ = RtlSdrControls::set_bias_tee(s, on);
    }
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_ppm(source: &mut DeviceSource, ppm: i32) {
    if let DeviceSource::RtlSdr(s) = source {
        let _ = RtlSdrControls::set_freq_correction(s, ppm);
    }
}

#[cfg(feature = "qmx")]
pub fn qmx_set_rf_gain_db(source: &mut DeviceSource, db: u8) {
    if let DeviceSource::Qmx(s) = source {
        let _ = QmxControls::set_rf_gain_db(s, db);
    }
}
