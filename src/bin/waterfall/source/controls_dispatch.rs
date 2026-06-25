//! Apply hardware controls through [`IqSource`] trait methods.
//!
//! These helpers keep engine command handlers concise. Calls on the wrong front
//! end are safe no-ops via [`IqSource`] default implementations.

use hfsdr::IqSource;

pub fn kiwi_set_rf_agc(source: &mut dyn IqSource, on: bool) {
    let _ = source.set_agc(on);
}

pub fn kiwi_set_man_gain(source: &mut dyn IqSource, gain: u8) {
    let _ = source.set_man_gain(gain);
}

pub fn kiwi_set_rf_attn_db(source: &mut dyn IqSource, db: f32) {
    let _ = source.set_rf_attn_db(db);
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_att(source: &mut dyn IqSource, step: u8) {
    let _ = source.set_hf_att(step);
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_lna(source: &mut dyn IqSource, on: bool) {
    let _ = source.set_hf_lna(on);
}

#[cfg(feature = "airspy")]
pub fn airspy_set_hf_agc_threshold(source: &mut dyn IqSource, high: bool) {
    let _ = source.set_hf_agc_threshold(high);
}

#[cfg(feature = "airspy")]
pub fn airspy_set_frontend_options(source: &mut dyn IqSource, flags: u32) {
    let _ = source.set_frontend_options(flags);
}

#[cfg(feature = "airspy")]
pub fn airspy_set_bias_tee(source: &mut dyn IqSource, on: bool) {
    let _ = source.set_bias_tee(on);
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_rtl_agc(source: &mut dyn IqSource, on: bool) {
    let _ = source.set_agc(on);
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_manual_gain(source: &mut dyn IqSource, manual: bool) {
    let _ = source.set_tuner_gain_mode(manual);
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_tuner_gain(source: &mut dyn IqSource, gain_db10: i32) {
    let _ = source.set_tuner_gain(gain_db10);
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_bias_tee(source: &mut dyn IqSource, on: bool) {
    let _ = source.set_bias_tee(on);
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_set_ppm(source: &mut dyn IqSource, ppm: i32) {
    let _ = source.set_freq_correction(ppm);
}

#[cfg(feature = "qmx")]
pub fn qmx_set_rf_gain_db(source: &mut dyn IqSource, db: u8) {
    let _ = source.set_rf_gain_db(db);
}
