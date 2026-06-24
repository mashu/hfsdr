//! The source-agnostic interface that every front end implements.
//!
//! A source delivers interleaved baseband IQ (`Complex32`) at `sample_rate()`,
//! centered on `frequency()`, by pushing into a single-producer/single-consumer
//! ring. The caller drains the [`Consumer`] returned by [`IqSource::start`].

use std::fmt;

pub use num_complex::Complex32;
pub use rtrb::Consumer;

/// Errors a source can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// No matching device could be found or opened.
    NotFound,
    /// A backend call returned a non-zero status code.
    Backend { op: &'static str, code: i32 },
    /// The requested configuration is not supported by the device.
    Unsupported(String),
    /// The operation is not valid in the current state (e.g. streaming).
    InvalidState(&'static str),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::NotFound => write!(f, "no device found"),
            SourceError::Backend { op, code } => write!(f, "{op} failed (code {code})"),
            SourceError::Unsupported(s) => write!(f, "unsupported: {s}"),
            SourceError::InvalidState(s) => write!(f, "invalid state: {s}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, SourceError>;

/// A tunable IQ front end.
///
/// Contract:
/// - `start` may only be called when not already streaming.
/// - `set_sample_rate` requires streaming to be stopped.
/// - Sub-Hz tuning residual is left to the DSP NCO; the device may quantize
///   `tune` to integer Hz.
/// - On consumer backpressure, samples are dropped (never blocking the source's
///   real-time thread) and counted by `dropped_samples`.
pub trait IqSource {
    /// Sample rates (samples/second) the device reports as supported.
    fn sample_rates(&self) -> Vec<u32>;

    /// Currently selected sample rate (samples/second).
    fn sample_rate(&self) -> u32;

    /// Select a sample rate. Must be one of [`IqSource::sample_rates`].
    fn set_sample_rate(&mut self, sr: u32) -> Result<()>;

    /// Tune the hardware center frequency, in Hz.
    fn tune(&mut self, hz: f64) -> Result<()>;

    /// Last requested center frequency, in Hz.
    fn frequency(&self) -> f64;

    /// Begin streaming; returns the consumer end of the IQ ring.
    fn start(&mut self) -> Result<Consumer<Complex32>>;

    /// Stop streaming. Idempotent: safe to call when already stopped.
    fn stop(&mut self) -> Result<()>;

    /// Total IQ samples dropped due to a full ring since the source was opened.
    fn dropped_samples(&self) -> u64;

    /// Whether the source is currently streaming.
    fn is_streaming(&self) -> bool;

    /// Latest S-meter in dBm when the backend reports one (KiwiSDR).
    fn rssi_dbm(&self) -> Option<f32> {
        None
    }

    /// Last hardware RF gain command when the backend tracks one (Kiwi `manGain`).
    fn hw_rf_gain(&self) -> Option<u8> {
        None
    }

    /// Whether [`IqSource::set_passband`] can retune the remote filter.
    fn supports_passband(&self) -> bool {
        false
    }

    /// IQ passband edges in Hz relative to center (KiwiSDR). Ignored on local SDRs.
    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
        let _ = (low_hz, high_hz);
        Ok(())
    }

    /// Toggle AGC when supported (KiwiSDR, Airspy HF+).
    fn set_agc(&mut self, on: bool) -> Result<()> {
        let _ = on;
        Ok(())
    }

    /// RF gain 0..=100 (`manGain` CAT); manual gain when Kiwi RF AGC is off (ignored when AGC on).
    fn set_man_gain(&mut self, gain: u8) -> Result<()> {
        let _ = gain;
        Ok(())
    }

    /// KiwiSDR 2 hardware RF attenuator is available on this link.
    fn has_rf_attn(&self) -> bool {
        false
    }

    /// Latest hardware RF attenuator setting in dB (KiwiSDR 2).
    fn rf_attn_db(&self) -> Option<f32> {
        None
    }

    /// Hardware RF attenuator in dB (KiwiSDR 2 `SET rf_attn=`).
    fn set_rf_attn_db(&mut self, db: f32) -> Result<()> {
        let _ = db;
        Ok(())
    }

    /// HF attenuator step 0..=8 (Airspy HF+, 6 dB per step).
    fn set_hf_att(&mut self, _step: u8) -> Result<()> {
        Ok(())
    }

    /// HF LNA / preamp (Airspy HF+).
    fn set_hf_lna(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }

    /// HF AGC threshold: `false` = low, `true` = high (Airspy HF+).
    fn set_hf_agc_threshold(&mut self, _high: bool) -> Result<()> {
        Ok(())
    }

    /// Frontend option flags (Airspy HF+ Discovery / Ranger).
    fn set_frontend_options(&mut self, _flags: u32) -> Result<()> {
        Ok(())
    }

    /// Antenna-port bias tee (Airspy HF+ Discovery / Ranger, RTL-SDR).
    fn set_bias_tee(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }

    /// Tuner gain in tenths of a dB (RTL-SDR manual mode).
    fn set_tuner_gain(&mut self, _gain_db10: i32) -> Result<()> {
        Ok(())
    }

    /// Manual vs automatic tuner gain (RTL-SDR).
    fn set_tuner_gain_mode(&mut self, _manual: bool) -> Result<()> {
        Ok(())
    }

    /// Frequency correction in parts-per-million (RTL-SDR).
    fn set_freq_correction(&mut self, _ppm: i32) -> Result<()> {
        Ok(())
    }

    /// RF gain in dB (QMX / QMX+).
    fn set_rf_gain_db(&mut self, _db: u8) -> Result<()> {
        Ok(())
    }

    /// Front end finished handshake and is delivering IQ (KiwiSDR).
    fn link_ready(&self) -> bool {
        true
    }

    /// Reader / device thread is still running (KiwiSDR).
    fn link_alive(&self) -> bool {
        true
    }

    /// Human-readable connection error from the remote front end.
    fn link_error(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_error_display() {
        assert_eq!(SourceError::NotFound.to_string(), "no device found");
        assert_eq!(
            SourceError::Backend { op: "open", code: 42 }.to_string(),
            "open failed (code 42)"
        );
        assert_eq!(
            SourceError::Unsupported("bad rate".into()).to_string(),
            "unsupported: bad rate"
        );
        assert_eq!(
            SourceError::InvalidState("streaming").to_string(),
            "invalid state: streaming"
        );
    }

    struct StubSource;

    impl IqSource for StubSource {
        fn sample_rates(&self) -> Vec<u32> {
            vec![48_000]
        }

        fn sample_rate(&self) -> u32 {
            48_000
        }

        fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
            if sr == 48_000 {
                Ok(())
            } else {
                Err(SourceError::Unsupported("rate".into()))
            }
        }

        fn tune(&mut self, hz: f64) -> Result<()> {
            let _ = hz;
            Ok(())
        }

        fn frequency(&self) -> f64 {
            14_030_000.0
        }

        fn start(&mut self) -> Result<Consumer<Complex32>> {
            Err(SourceError::InvalidState("offline"))
        }

        fn stop(&mut self) -> Result<()> {
            Ok(())
        }

        fn dropped_samples(&self) -> u64 {
            0
        }

        fn is_streaming(&self) -> bool {
            false
        }
    }

    #[test]
    fn trait_default_controls_are_noops() {
        let mut src = StubSource;
        assert!(src.rssi_dbm().is_none());
        assert!(!src.supports_passband());
        src.set_passband(-2_500, 2_500).unwrap();
        src.set_agc(true).unwrap();
        src.set_hf_att(3).unwrap();
        src.set_hf_lna(true).unwrap();
        src.set_hf_agc_threshold(true).unwrap();
        src.set_frontend_options(0).unwrap();
        src.set_bias_tee(false).unwrap();
        src.set_tuner_gain(200).unwrap();
        src.set_tuner_gain_mode(true).unwrap();
        src.set_freq_correction(1).unwrap();
        src.set_rf_gain_db(10).unwrap();
        src.set_rf_attn_db(3.0).unwrap();
        assert!(src.link_ready());
        assert!(src.link_alive());
        assert!(src.link_error().is_none());
        assert_eq!(src.frequency(), 14_030_000.0);
    }
}
