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

    /// Whether [`IqSource::set_passband`] can retune the remote filter.
    fn supports_passband(&self) -> bool {
        false
    }

    /// IQ passband edges in Hz relative to center (KiwiSDR). Ignored on local SDRs.
    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
        let _ = (low_hz, high_hz);
        Ok(())
    }

    /// Toggle AGC when supported (KiwiSDR).
    fn set_agc(&mut self, on: bool) -> Result<()> {
        let _ = on;
        Ok(())
    }

    /// Front end finished handshake and is delivering IQ (KiwiSDR).
    fn link_ready(&self) -> bool {
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
}
