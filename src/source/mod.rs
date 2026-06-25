//! The source-agnostic interface that every front end implements.
//!
//! A source delivers interleaved baseband IQ (`Complex32`) at `sample_rate()`,
//! centered on `frequency()`, by pushing into a single-producer/single-consumer
//! ring. The caller drains the [`Consumer`] returned by [`IqSource::start`].
//!
//! Device-specific RF controls live in [`controls`] extension traits, not on
//! [`IqSource`].

pub mod controls;
pub mod ingress;

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

/// A tunable IQ front end — streaming and tuning only.
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
    fn trait_core_streaming_contract() {
        let mut src = StubSource;
        assert_eq!(src.frequency(), 14_030_000.0);
        assert!(!src.is_streaming());
        assert_eq!(src.dropped_samples(), 0);
        assert!(src.start().is_err());
        src.stop().unwrap();
    }

    #[test]
    fn unsupported_sample_rate_errors() {
        let mut src = StubSource;
        assert!(src.set_sample_rate(96_000).is_err());
    }

    #[test]
    fn source_error_implements_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(SourceError::NotFound);
        assert!(!err.to_string().is_empty());
    }
}
