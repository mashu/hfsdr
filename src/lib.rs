//! # hfsdr
//!
//! Core of an HF SDR / CW client. Every front end implements [`IqSource`], so
//! the DSP, decoder, and UI layers never know which radio they are talking to.
//!
//! - [`airspyhf::AirspyHf`] — direct libairspyhf binding (no SoapySDR layer).
//! - [`kiwi::KiwiSource`] — KiwiSDR over WebSocket, same trait, ~12 kHz IQ.
//! - [`dsp::SpectrumAnalyzer`] — windowed complex FFT for the panadapter.

pub mod airspyhf;
pub mod cty;
pub mod dsp;
pub mod history;
pub mod kiwi;
pub mod multisource;
pub mod skimmer;
pub mod source;

pub use airspyhf::AirspyHf;
pub use cty::{Continent, ContinentResolver};
pub use history::{Annotation, RowFold, SlowWaterfall};
pub use dsp::{
    decimation_factor, design_gaussian_lowpass, design_lowpass, extract_passband_view,
    extract_view_window, iq_to_audio, AgcSettings, ApfSettings, AutoNotchSettings, CwChannelSettings,
    IqAudioDemod, NoiseBlankerSettings, NoiseReductionSettings, NotchSpec, SpectrumAnalyzer,
    WindowKind, MAX_NOTCHES,
};
pub use kiwi::protocol::KIWI_IQ_HALF_HZ;
pub use kiwi::KiwiSource;
pub use skimmer::{
    detect_peaks, strongest_offset_hz, AdaptiveCwDecoder, CwDecoder, Peak, Skimmer, SkimmerConfig,
    Spot, SpotKind, SpotSort, SpotStore,
};
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
