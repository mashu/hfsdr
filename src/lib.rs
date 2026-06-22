//! # hfsdr — HF SDR / CW client library
//!
//! Source-agnostic IQ pipeline: panadapter FFT, contest-grade CW demodulation, in-band skimmer.
//!
//! ## Documentation (read this first)
//!
//! The **[mdBook](https://github.com/mashu/hfsdr/tree/main/docs)** explains behavior and
//! algorithms for operators and contributors — IQ basics, filter shapes, CW demod, skimmer,
//! and why the UI stays responsive. Build locally: `./scripts/build-docs.sh`.
//!
//! `cargo doc` is the API reference (types and functions), not a substitute for the book.
//!
//! ## Architecture in one diagram
//!
//! ```text
//! IqSource → ring → engine { listen (CwChannel) | FFT | skimmer } → GUI try_poll
//! ```

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
    auto_fft_size, bin_width_hz, channel_group_delay_ms, decimation_factor, design_gaussian_lowpass,
    design_lowpass,
    effective_decimation, audio_sample_rate, extract_passband_view, extract_view_window, iq_to_audio, spectrum_plan, spectrum_view_mapping,
    AgcSettings, ApfSettings, AutoNotchSettings, CwChannel, CwChannelSettings, IqAudioDemod,
    NoiseBlankerSettings, NoiseReductionSettings, NotchSpec, SpectrumAnalyzer, SpectrumFrontEnd,
    SpectrumViewMapping, WindowKind, MAX_FFT_SIZE, MAX_NOTCHES, TARGET_BIN_HZ,
    ZOOM_DECIM_THRESHOLD,
};
pub use kiwi::protocol::KIWI_IQ_HALF_HZ;
pub use kiwi::KiwiSource;
pub use skimmer::{
    detect_peaks, strongest_offset_hz, AdaptiveCwDecoder, BigramCwDecoder, CwDecoder, DecoderParams,
    EnvelopeSettings, MasterScp, Peak, Skimmer, SkimmerConfig, SkimmerDecoderKind, Spot,
    SpotKind, SpotSort, SpotStore, MASTER_SCP_URL,
};
pub use source::{Complex32, Consumer, IqSource, Result, SourceError};
