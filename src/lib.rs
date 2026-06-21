//! # hfsdr
//!
//! Core of an HF SDR / CW client. Every front end implements [`IqSource`], so
//! the DSP, decoder, and UI layers never know which radio they are talking to.
//!
//! - [`airspyhf::AirspyHf`] — direct libairspyhf binding (no SoapySDR layer).
//! - [`kiwi::KiwiSource`] — KiwiSDR over WebSocket, same trait, ~12 kHz IQ.
//! - [`dsp::SpectrumAnalyzer`] — windowed complex FFT for the panadapter.

pub mod airspyhf;
pub mod dsp;
pub mod kiwi;
pub mod source;

pub use airspyhf::AirspyHf;
pub use dsp::{extract_passband_view, extract_view_window, DemodSettings, iq_to_audio, IqAudioDemod, SpectrumAnalyzer};
pub use kiwi::protocol::KIWI_IQ_HALF_HZ;
pub use kiwi::KiwiSource;
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
