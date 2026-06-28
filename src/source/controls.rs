//! Device-specific hardware control extension traits.
//!
//! Each trait groups RF/control hooks for one front end. The slim [`IqSource`]
//! trait covers streaming only; concrete sources implement the matching trait in
//! their device module (`kiwi/`, `airspyhf/`, …).

use super::Result;

/// KiwiSDR passband, RF AGC, manual gain, link health, and hardware attenuator.
pub trait KiwiControls {
    fn supports_passband(&self) -> bool;
    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()>;
    fn set_agc(&mut self, on: bool) -> Result<()>;
    fn rf_agc_on(&self) -> bool;
    fn set_man_gain(&mut self, gain: u8) -> Result<()>;
    fn set_rf_attn_db(&mut self, db: f32) -> Result<()>;
    fn has_rf_attn(&self) -> bool;
    fn rf_attn_db(&self) -> Option<f32>;
    fn rssi_dbm(&self) -> Option<f32>;
    fn hw_rf_gain(&self) -> Option<u8>;
    fn link_ready(&self) -> bool;
    fn link_alive(&self) -> bool;
    fn link_error(&self) -> Option<String>;
}

/// Airspy HF+ RF chain controls.
pub trait AirspyControls {
    fn set_hf_att(&mut self, step: u8) -> Result<()>;
    fn set_hf_lna(&mut self, on: bool) -> Result<()>;
    fn set_hf_agc_threshold(&mut self, high: bool) -> Result<()>;
    fn set_frontend_options(&mut self, flags: u32) -> Result<()>;
    fn set_bias_tee(&mut self, on: bool) -> Result<()>;
    fn set_agc(&mut self, on: bool) -> Result<()>;
}

/// RTL-SDR tuner and GPIO controls.
pub trait RtlSdrControls {
    fn set_agc(&mut self, on: bool) -> Result<()>;
    fn set_tuner_gain_mode(&mut self, manual: bool) -> Result<()>;
    fn set_tuner_gain(&mut self, gain_db10: i32) -> Result<()>;
    fn set_bias_tee(&mut self, on: bool) -> Result<()>;
    fn set_freq_correction(&mut self, ppm: i32) -> Result<()>;
}

/// QMX / QMX+ RF gain (CAT `RG`) and S-meter.
pub trait QmxControls {
    fn set_rf_gain_db(&mut self, db: u8) -> Result<()>;
    fn rssi_dbm(&self) -> Option<f32>;
}

/// SoapySDR overall gain, AGC, and antenna selection.
pub trait SoapyControls {
    fn set_gain_db(&mut self, db: f64) -> Result<()>;
    fn set_agc(&mut self, on: bool) -> Result<()>;
    fn set_antenna(&mut self, name: &str) -> Result<()>;
    fn gain_db(&self) -> f64;
    fn agc_on(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiwi::KiwiSource;

    #[test]
    fn kiwi_controls_before_streaming() {
        let mut src = KiwiSource::new("example.test", 8073);
        assert!(KiwiControls::supports_passband(&src));
        KiwiControls::set_passband(&mut src, -4_000, 4_000).unwrap();
        KiwiControls::set_agc(&mut src, true).unwrap();
        assert!(KiwiControls::rf_agc_on(&src));
        KiwiControls::set_agc(&mut src, false).unwrap();
        assert!(!KiwiControls::rf_agc_on(&src));
        KiwiControls::set_agc(&mut src, true).unwrap();
        KiwiControls::set_man_gain(&mut src, 60).unwrap();
        KiwiControls::set_rf_attn_db(&mut src, 6.0).unwrap();
        assert_eq!(KiwiControls::hw_rf_gain(&src), Some(60));
        assert!(KiwiControls::rssi_dbm(&src).is_some());
    }
}
