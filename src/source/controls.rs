//! Device-specific hardware control extension traits.
//!
//! Each trait groups the [`IqSource`] control hooks for one front end. Concrete
//! sources implement the matching trait by forwarding to their [`IqSource`] impl;
//! [`IqSource`] default methods remain for backward compatibility on `dyn IqSource`.

#![allow(unused_macros)]

use super::{IqSource, Result};

/// KiwiSDR passband, RF AGC, manual gain, and hardware attenuator controls.
pub trait KiwiControls {
    fn supports_passband(&self) -> bool;
    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()>;
    fn set_agc(&mut self, on: bool) -> Result<()>;
    fn set_man_gain(&mut self, gain: u8) -> Result<()>;
    fn set_rf_attn_db(&mut self, db: f32) -> Result<()>;
    fn has_rf_attn(&self) -> bool;
    fn rf_attn_db(&self) -> Option<f32>;
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

/// QMX / QMX+ RF gain (CAT `RG`).
pub trait QmxControls {
    fn set_rf_gain_db(&mut self, db: u8) -> Result<()>;
}

macro_rules! forward_kiwi_controls {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl KiwiControls for $ty {
                fn supports_passband(&self) -> bool {
                    IqSource::supports_passband(self)
                }

                fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
                    IqSource::set_passband(self, low_hz, high_hz)
                }

                fn set_agc(&mut self, on: bool) -> Result<()> {
                    IqSource::set_agc(self, on)
                }

                fn set_man_gain(&mut self, gain: u8) -> Result<()> {
                    IqSource::set_man_gain(self, gain)
                }

                fn set_rf_attn_db(&mut self, db: f32) -> Result<()> {
                    IqSource::set_rf_attn_db(self, db)
                }

                fn has_rf_attn(&self) -> bool {
                    IqSource::has_rf_attn(self)
                }

                fn rf_attn_db(&self) -> Option<f32> {
                    IqSource::rf_attn_db(self)
                }
            }
        )+
    };
}

macro_rules! forward_airspy_controls {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl AirspyControls for $ty {
                fn set_hf_att(&mut self, step: u8) -> Result<()> {
                    IqSource::set_hf_att(self, step)
                }

                fn set_hf_lna(&mut self, on: bool) -> Result<()> {
                    IqSource::set_hf_lna(self, on)
                }

                fn set_hf_agc_threshold(&mut self, high: bool) -> Result<()> {
                    IqSource::set_hf_agc_threshold(self, high)
                }

                fn set_frontend_options(&mut self, flags: u32) -> Result<()> {
                    IqSource::set_frontend_options(self, flags)
                }

                fn set_bias_tee(&mut self, on: bool) -> Result<()> {
                    IqSource::set_bias_tee(self, on)
                }

                fn set_agc(&mut self, on: bool) -> Result<()> {
                    IqSource::set_agc(self, on)
                }
            }
        )+
    };
}

macro_rules! forward_rtlsdr_controls {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl RtlSdrControls for $ty {
                fn set_agc(&mut self, on: bool) -> Result<()> {
                    IqSource::set_agc(self, on)
                }

                fn set_tuner_gain_mode(&mut self, manual: bool) -> Result<()> {
                    IqSource::set_tuner_gain_mode(self, manual)
                }

                fn set_tuner_gain(&mut self, gain_db10: i32) -> Result<()> {
                    IqSource::set_tuner_gain(self, gain_db10)
                }

                fn set_bias_tee(&mut self, on: bool) -> Result<()> {
                    IqSource::set_bias_tee(self, on)
                }

                fn set_freq_correction(&mut self, ppm: i32) -> Result<()> {
                    IqSource::set_freq_correction(self, ppm)
                }
            }
        )+
    };
}

macro_rules! forward_qmx_controls {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl QmxControls for $ty {
                fn set_rf_gain_db(&mut self, db: u8) -> Result<()> {
                    IqSource::set_rf_gain_db(self, db)
                }
            }
        )+
    };
}

use crate::kiwi::KiwiSource;

forward_kiwi_controls!(KiwiSource);

#[cfg(feature = "airspy")]
forward_airspy_controls!(crate::airspyhf::AirspyHf);

#[cfg(feature = "rtlsdr")]
forward_rtlsdr_controls!(crate::rtlsdr::RtlSdr);

#[cfg(feature = "qmx")]
forward_qmx_controls!(crate::qmx::QmxSource);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiwi::KiwiSource;

    #[test]
    fn kiwi_controls_forward_to_iqsource() {
        let mut src = KiwiSource::new("example.test", 8073);
        assert!(KiwiControls::supports_passband(&src));
        KiwiControls::set_passband(&mut src, -4_000, 4_000).unwrap();
        KiwiControls::set_agc(&mut src, true).unwrap();
        KiwiControls::set_man_gain(&mut src, 60).unwrap();
        KiwiControls::set_rf_attn_db(&mut src, 6.0).unwrap();
        assert_eq!(IqSource::hw_rf_gain(&src), Some(60));
    }
}
