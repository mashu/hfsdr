//! # hfsdr
//!
//! Core of an HF SDR / CW client. Every front end implements [`IqSource`], so
//! the DSP, decoder, and UI layers never know which radio they are talking to.
//!
//! - [`airspyhf::AirspyHf`] — direct libairspyhf binding (no SoapySDR layer), `airspy` feature.
//! - [`kiwi::KiwiSource`] — KiwiSDR over WebSocket, same trait, ~12 kHz IQ.
//! - [`dsp::SpectrumAnalyzer`] — windowed complex FFT for the panadapter.

#[cfg(feature = "airspy")]
pub mod airspyhf;
pub mod cty;
pub mod dsp;
pub mod history;
pub mod kiwi;
pub mod multisource;
pub mod skimmer;
pub mod source;

#[cfg(feature = "airspy")]
pub use airspyhf::AirspyHf;
pub use cty::{Continent, ContinentResolver};
pub use history::{Annotation, RowFold, SlowWaterfall};
pub use dsp::{
    auto_fft_size, bin_width_hz, decimation_factor, design_gaussian_lowpass, design_lowpass,
    effective_decimation, audio_sample_rate, extract_passband_view, extract_view_window, iq_to_audio, spectrum_plan, spectrum_view_mapping,
    AgcSettings, ApfSettings, AutoNotchSettings, CwChannelSettings, IqAudioDemod,
    NoiseBlankerSettings, NoiseReductionSettings, NotchSpec, SpectrumAnalyzer, SpectrumFrontEnd,
    SpectrumViewMapping, WindowKind, MAX_FFT_SIZE, MAX_NOTCHES, TARGET_BIN_HZ,
    ZOOM_DECIM_THRESHOLD,
};
pub use kiwi::protocol::KIWI_IQ_HALF_HZ;
pub use kiwi::KiwiSource;
pub use skimmer::{
    detect_peaks, strongest_offset_hz, AdaptiveCwDecoder, BigramCwDecoder, CwDecoder, MasterScp,
    Peak, Skimmer, SkimmerConfig, Spot, SpotKind, SpotSort, SpotStore, MASTER_SCP_URL,
};
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
