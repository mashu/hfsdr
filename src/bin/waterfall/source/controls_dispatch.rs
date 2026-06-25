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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::device::DeviceSource;
    use hfsdr::kiwi::KiwiSource;
    use hfsdr::source::controls::KiwiControls;

    #[test]
    fn kiwi_set_man_gain_updates_device() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        kiwi_set_man_gain(&mut src, 60);
        if let DeviceSource::Kiwi(k) = &src {
            assert_eq!(KiwiControls::hw_rf_gain(k).unwrap(), 60);
        }
    }

    #[test]
    fn kiwi_set_rf_agc_toggles_mode() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        kiwi_set_rf_agc(&mut src, false);
        if let DeviceSource::Kiwi(k) = &src {
            assert!(!KiwiControls::rf_agc_on(k));
        }
        kiwi_set_rf_agc(&mut src, true);
        if let DeviceSource::Kiwi(k) = &src {
            assert!(KiwiControls::rf_agc_on(k));
        }
    }

    #[test]
    fn kiwi_set_rf_attn_db_updates_device() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        kiwi_set_rf_attn_db(&mut src, 12.0);
        if let DeviceSource::Kiwi(k) = &src {
            assert_eq!(KiwiControls::rf_attn_db(k), Some(12.0));
        }
    }

    #[test]
    fn kiwi_controls_noop_on_empty_device_source() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        kiwi_set_rf_agc(&mut src, true);
        kiwi_set_man_gain(&mut src, 0);
        kiwi_set_rf_attn_db(&mut src, 0.0);
    }

    #[cfg(feature = "airspy")]
    #[test]
    fn airspy_controls_noop_on_kiwi_source() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        airspy_set_hf_att(&mut src, 1);
        airspy_set_hf_lna(&mut src, true);
        airspy_set_hf_agc_threshold(&mut src, true);
        airspy_set_frontend_options(&mut src, 3);
        airspy_set_bias_tee(&mut src, false);
    }

    #[cfg(feature = "rtlsdr")]
    #[test]
    fn rtlsdr_controls_noop_on_kiwi_source() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        rtlsdr_set_rtl_agc(&mut src, true);
        rtlsdr_set_manual_gain(&mut src, true);
        rtlsdr_set_tuner_gain(&mut src, 196);
        rtlsdr_set_bias_tee(&mut src, false);
        rtlsdr_set_ppm(&mut src, 42);
    }

    #[cfg(feature = "qmx")]
    #[test]
    fn qmx_controls_noop_on_kiwi_source() {
        let kiwi = KiwiSource::new("test.local", 8073);
        let mut src = DeviceSource::Kiwi(kiwi);
        qmx_set_rf_gain_db(&mut src, 10);
    }
}
